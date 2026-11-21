//! The `l6-helix` preset file as the HX Stomp stores and sends it: a fixstr magic,
//! a str16 table of 12 offsets, then one MessagePack map of numbered sections.
//! HXtra keeps snapshots 4..6 in section 0x64, a bin that starts with `HX6B`.

use std::io::Cursor;
use std::ops::Range;

use rmpv::Value;

use crate::{Error, Result, msg};

const MAGIC: &[u8] = b"\xa9l6-helix\0";
const TOC_HEADER: &[u8] = b"\xda\x00\x30";
const TOC_LEN: usize = 48;
const MAP_OFFSET: usize = MAGIC.len() + TOC_HEADER.len() + TOC_LEN;

pub const SECTION_META: u64 = 7;
pub const SECTION_BLOCKS: u64 = 0;
const BLOCK_GRID_KEY: u64 = 22;
const BLOCK_TYPE_KEY: u64 = 19;
const BLOCK_TYPE_PROCESSING: u64 = 6;
pub const SECTION_BANK_B: u64 = 0x64;
pub const BANK_B_MAGIC: &[u8] = b"HX6B";

mod meta_key {
    pub const VERSION: u64 = 0x23;
    pub const PRODUCT: u64 = 0x24;
    pub const BUILD: u64 = 0x25;
}

#[derive(Debug, Clone)]
pub struct Section {
    pub key: u64,
    pub value: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct PresetFile {
    bytes: Vec<u8>,
    sections: Vec<Section>,
    trailing: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DeviceMeta {
    pub product: String,
    pub firmware: u64,
    pub build: String,
}

impl DeviceMeta {
    pub fn firmware_text(&self) -> String {
        let v = self.firmware;
        format!("{:x}.{:x}.{:x}", v >> 24, (v >> 16) & 0xFF, (v >> 8) & 0xFF)
    }
}

impl PresetFile {
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        if !bytes.starts_with(MAGIC) {
            return Err(Error::Preset("not an l6-helix preset (bad magic)".into()));
        }
        if bytes.get(MAGIC.len()..MAGIC.len() + TOC_HEADER.len()) != Some(TOC_HEADER) {
            return Err(Error::Preset("missing the 48-byte offset table".into()));
        }
        let count = match bytes.get(MAP_OFFSET) {
            Some(b) if b & 0xF0 == 0x80 => usize::from(b & 0x0F),
            Some(b) => return Err(Error::Preset(format!("sections are not a fixmap (byte {b:#04x})"))),
            None => return Err(Error::Preset("file ends before the section map".into())),
        };
        let mut cur = Cursor::new(&bytes[..]);
        cur.set_position(MAP_OFFSET as u64 + 1);
        let mut sections = Vec::with_capacity(count);
        for _ in 0..count {
            let key = rmpv::decode::read_value(&mut cur)?
                .as_u64()
                .ok_or_else(|| Error::Preset("a section key is not an integer".into()))?;
            let start = cur.position() as usize;
            rmpv::decode::read_value(&mut cur)?;
            sections.push(Section { key, value: start..cur.position() as usize });
        }
        let trailing = bytes.len() - cur.position() as usize;
        Ok(Self { bytes, sections, trailing })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Bytes after the section map (none in the files seen so far).
    pub fn trailing(&self) -> usize {
        self.trailing
    }

    pub fn section(&self, key: u64) -> Option<Value> {
        let s = self.sections.iter().find(|s| s.key == key)?;
        rmpv::decode::read_value(&mut &self.bytes[s.value.clone()]).ok()
    }

    /// The raw `HX6B` record bank (snapshots 4..6), if this preset has one.
    pub fn bank_b(&self) -> Option<Vec<u8>> {
        let v = self.section(SECTION_BANK_B)?;
        msg::bytes(&v).filter(|b| b.starts_with(BANK_B_MAGIC)).map(<[u8]>::to_vec)
    }

    pub fn has_bank_b(&self) -> bool {
        self.bank_b().is_some()
    }

    /// The block grid positions that hold a processing block (type 6 in `section 0`, key 22).
    pub fn occupied_positions(&self) -> Vec<usize> {
        let Some(grid) = self.section(SECTION_BLOCKS).and_then(|s| msg::get(&s, BLOCK_GRID_KEY).cloned()) else {
            return Vec::new();
        };
        grid.as_array()
            .map(|cells| {
                cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| msg::get(c, BLOCK_TYPE_KEY).and_then(Value::as_u64) == Some(BLOCK_TYPE_PROCESSING))
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn device_meta(&self) -> Option<DeviceMeta> {
        let m = self.section(SECTION_META)?;
        Some(DeviceMeta {
            product: msg::get(&m, meta_key::PRODUCT).and_then(msg::text)?,
            firmware: msg::get(&m, meta_key::VERSION).and_then(Value::as_u64)?,
            build: msg::get(&m, meta_key::BUILD).and_then(msg::text)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(sections: &[(u64, Value)]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(TOC_HEADER);
        out.extend_from_slice(&[0; TOC_LEN]);
        out.push(0x80 | sections.len() as u8);
        for (k, v) in sections {
            out.extend(msg::encode_value(&Value::from(*k)));
            out.extend(msg::encode_value(v));
        }
        out
    }

    fn meta() -> Value {
        msg::int_map(&[
            (0x24, Value::from("P33\0")),
            (0x23, Value::from(0x0380_0000u64)),
            (0x25, Value::from("v3.71-32-g1039661\0")),
        ])
    }

    #[test]
    fn finds_bank_b_and_meta() {
        let bank = [BANK_B_MAGIC, &[1, 0, 0, 0], &[7; 16]].concat();
        let p = PresetFile::parse(file(&[(7, meta()), (0, Value::Nil), (0x64, Value::Binary(bank.clone()))])).unwrap();
        assert_eq!(p.sections().len(), 3);
        assert_eq!(p.bank_b(), Some(bank));
        let m = p.device_meta().unwrap();
        assert_eq!((m.product.as_str(), m.firmware_text().as_str(), m.build.as_str()), ("P33", "3.80.0", "v3.71-32-g1039661"));
        assert_eq!(p.trailing(), 0);
    }

    #[test]
    fn stock_preset_has_no_bank_b() {
        let p = PresetFile::parse(file(&[(7, meta()), (10, Value::Nil)])).unwrap();
        assert!(!p.has_bank_b());
    }

    #[test]
    fn rejects_other_data() {
        assert!(PresetFile::parse(b"RIFF....".to_vec()).is_err());
        let mut f = file(&[(7, meta())]);
        f.truncate(f.len() - 3);
        assert!(PresetFile::parse(f).is_err());
    }
}

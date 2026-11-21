//! Snapshots of a preset file: 1..3 from the stock MessagePack section 0x0a,
//! 4..6 from HXtra's bank B (section 0x64), which holds the firmware's raw
//! 0x260-byte snapshot records after an 8-byte `HX6B` header.

use rmpv::Value;

use crate::preset::{BANK_B_MAGIC, PresetFile};
use crate::{Error, Result, msg};

pub const SECTION_SNAPSHOTS: u64 = 0x0a;
pub const BLOCK_POSITIONS: usize = 20;

const BANK_HEADER_LEN: usize = 8;
const BANK_VERSION: u8 = 1;
const RECORDS: usize = 3;
const RECORD_LEN: usize = 0x260;
const CONTROLLERS_AT: usize = 0x28;
const CONTROLLER_LEN: usize = 7;
const CONTROLLERS: usize = 64;
const VALID_AT: usize = 0x24b;
const TEMPO_AT: usize = 0x250;
const NAME_AT: usize = 0x255;
const NAME_LEN: usize = 11;
const UNUSED_CONTROLLER: u8 = 0x40;

mod key {
    pub const LIST: u64 = 10;
    pub const VALID: u64 = 0;
    pub const CONTROLLERS: u64 = 2;
    pub const BLOCKS: u64 = 3;
    pub const NAME: u64 = 4;
    pub const TEMPO: u64 = 5;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bank {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub number: u8,
    pub bank: Bank,
    pub valid: bool,
    pub name: String,
    pub tempo: f32,
    /// One entry per block grid position (`preset[0][22]` order).
    pub blocks_on: Vec<bool>,
    pub controllers: Vec<ControllerValue>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerValue {
    pub index: u8,
    pub value: f32,
    pub footswitch_on: bool,
}

/// Snapshots 1..3, then 4..6 when the preset has a bank B.
pub fn snapshots(preset: &PresetFile) -> Result<Vec<Snapshot>> {
    let mut out = bank_a(preset)?;
    if let Some(raw) = preset.bank_b() {
        out.extend(bank_b(&raw)?);
    }
    Ok(out)
}

fn bank_a(preset: &PresetFile) -> Result<Vec<Snapshot>> {
    let section = preset.section(SECTION_SNAPSHOTS).ok_or_else(|| Error::Preset("no snapshot section (0x0a)".into()))?;
    let list = msg::get(&section, key::LIST)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Preset("snapshot section has no list".into()))?;
    list.iter().enumerate().map(|(i, s)| bank_a_snapshot(i, s)).collect()
}

fn bank_a_snapshot(i: usize, s: &Value) -> Result<Snapshot> {
    let field = |k| msg::get(s, k).ok_or_else(|| Error::Preset(format!("snapshot {} lacks key {k}", i + 1)));
    let blocks_on = field(key::BLOCKS)?
        .as_array()
        .ok_or_else(|| Error::Preset(format!("snapshot {}: blocks are not a list", i + 1)))?
        .iter()
        .map(|pair| pair.as_array().and_then(|p| p.get(1)).and_then(Value::as_bool).unwrap_or(false))
        .collect();
    let controllers = field(key::CONTROLLERS)?
        .as_array()
        .ok_or_else(|| Error::Preset(format!("snapshot {}: controllers are not a list", i + 1)))?
        .iter()
        .filter_map(|c| match c.as_array().map(Vec::as_slice) {
            Some([fs, idx, value]) => Some(ControllerValue {
                index: u8::try_from(idx.as_u64()?).ok()?,
                value: value.as_f64()? as f32,
                footswitch_on: fs.as_bool().unwrap_or(false),
            }),
            _ => None,
        })
        .filter(|c| c.index != UNUSED_CONTROLLER)
        .collect();
    Ok(Snapshot {
        number: i as u8 + 1,
        bank: Bank::A,
        valid: field(key::VALID)?.as_bool().unwrap_or(false),
        name: field(key::NAME).ok().and_then(msg::text).unwrap_or_default(),
        tempo: field(key::TEMPO).ok().and_then(Value::as_f64).unwrap_or(0.0) as f32,
        blocks_on,
        controllers,
    })
}

fn bank_b(raw: &[u8]) -> Result<Vec<Snapshot>> {
    match raw {
        [m0, m1, m2, m3, version, ..] if [*m0, *m1, *m2, *m3] == *BANK_B_MAGIC && *version == BANK_VERSION => {}
        _ => return Err(Error::Preset("bank B has an unknown header".into())),
    }
    let records = &raw[BANK_HEADER_LEN..];
    if records.len() != RECORDS * RECORD_LEN {
        return Err(Error::Preset(format!("bank B holds {} bytes of records, expected {}", records.len(), RECORDS * RECORD_LEN)));
    }
    Ok(records.chunks_exact(RECORD_LEN).enumerate().map(|(i, r)| bank_b_record(i, r)).collect())
}

fn bank_b_record(i: usize, r: &[u8]) -> Snapshot {
    let name = &r[NAME_AT..NAME_AT + NAME_LEN];
    let name_end = name.iter().position(|&c| c == 0).unwrap_or(NAME_LEN);
    Snapshot {
        number: (RECORDS + i) as u8 + 1,
        bank: Bank::B,
        valid: r[VALID_AT] != 0,
        name: String::from_utf8_lossy(&name[..name_end]).into_owned(),
        tempo: f32_at(r, TEMPO_AT),
        blocks_on: r[..BLOCK_POSITIONS].iter().map(|&s| s == 0).collect(),
        controllers: r[CONTROLLERS_AT..CONTROLLERS_AT + CONTROLLERS * CONTROLLER_LEN]
            .chunks_exact(CONTROLLER_LEN)
            .filter(|c| c[0] != UNUSED_CONTROLLER)
            .map(|c| ControllerValue { index: c[0], value: f32_at(c, 2), footswitch_on: c[6] != 0 })
            .collect(),
    }
}

/// `A1`..`A8`, `B1`..`B8` for the processing slots of the block grid; `None` for
/// the inputs, outputs, split and join.
pub fn slot_label(position: usize) -> Option<String> {
    match position {
        1..=8 => Some(format!("A{position}")),
        11..=18 => Some(format!("B{}", position - 10)),
        _ => None,
    }
}

fn f32_at(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, valid: bool, off_positions: &[usize], ctl: &[(u8, f32, bool)]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_LEN];
        for &p in off_positions {
            r[p] = 1;
        }
        for i in 0..CONTROLLERS {
            r[CONTROLLERS_AT + i * CONTROLLER_LEN] = UNUSED_CONTROLLER;
        }
        for (i, &(idx, value, fs)) in ctl.iter().enumerate() {
            let at = CONTROLLERS_AT + i * CONTROLLER_LEN;
            r[at] = idx;
            r[at + 1] = 3;
            r[at + 2..at + 6].copy_from_slice(&value.to_le_bytes());
            r[at + 6] = u8::from(fs);
        }
        r[VALID_AT] = u8::from(valid);
        r[TEMPO_AT..TEMPO_AT + 4].copy_from_slice(&120.0f32.to_le_bytes());
        r[NAME_AT..NAME_AT + name.len()].copy_from_slice(name.as_bytes());
        r
    }

    #[test]
    fn decodes_bank_b_records() {
        let mut raw = [BANK_B_MAGIC, &[BANK_VERSION, 0, 0, 0]].concat();
        raw.extend(record("SNAPSHOT 4", true, &[0, 6, 9, 19], &[(0, 1.0, false), (1, 2.4, true)]));
        raw.extend(record("SNAPSHOT 5", false, &[0, 9, 19], &[]));
        raw.extend(record("SNAPSHOT 6", false, &[0, 9, 19], &[]));
        let s = bank_b(&raw).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].number, s[0].name.as_str(), s[0].valid, s[0].tempo), (4, "SNAPSHOT 4", true, 120.0));
        assert!(!s[0].blocks_on[6] && s[0].blocks_on[7]);
        assert_eq!(s[0].controllers[1], ControllerValue { index: 1, value: 2.4, footswitch_on: true });
        assert!(!s[1].valid && s[1].controllers.is_empty());
    }

    #[test]
    fn refuses_unknown_bank_layouts() {
        assert!(bank_b(b"HX6B\x02\0\0\0").is_err());
        assert!(bank_b(&[BANK_B_MAGIC, &[BANK_VERSION, 0, 0, 0], &[0; 10]].concat()).is_err());
    }

    #[test]
    fn decodes_bank_a_snapshot() {
        let blocks = Value::Array((0..BLOCK_POSITIONS).map(|p| Value::Array(vec![false.into(), (p != 6).into()])).collect());
        let ctl = Value::Array(vec![
            Value::Array(vec![false.into(), 0.into(), Value::F32(1.5)]),
            Value::Array(vec![false.into(), 64.into(), Value::Nil]),
        ]);
        let s = msg::int_map(&[(0, true.into()), (2, ctl), (3, blocks), (4, "Snapshot 1\0".into()), (5, Value::F32(72.5))]);
        let snap = bank_a_snapshot(0, &s).unwrap();
        assert_eq!((snap.number, snap.name.as_str(), snap.tempo, snap.valid), (1, "Snapshot 1", 72.5, true));
        assert!(!snap.blocks_on[6] && snap.blocks_on[5]);
        assert_eq!(snap.controllers, vec![ControllerValue { index: 0, value: 1.5, footswitch_on: false }]);
    }
}

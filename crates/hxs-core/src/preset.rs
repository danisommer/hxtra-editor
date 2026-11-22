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
const BLOCK_BODY_KEY: u64 = 20;
pub const SECTION_BANK_B: u64 = 0x64;
pub const BANK_B_MAGIC: &[u8] = b"HX6B";

mod meta_key {
    pub const VERSION: u64 = 0x23;
    pub const PRODUCT: u64 = 0x24;
    pub const BUILD: u64 = 0x25;
}

mod block_key {
    pub const KIND: u64 = 9;
    pub const ENABLED: u64 = 10;
    pub const VALUES: u64 = 11;
    pub const MODEL: u64 = 24;
    pub const MODEL_ID: u64 = 25;
    pub const CAB_ID: u64 = 26;
    pub const IR_HASH: u64 = 27;
    pub const PARAM_COUNT: u64 = 3;
    pub const VALUE_LIST: u64 = 4;
}

/// Grid positions: 1..=8 the first row (path A), 11..=18 the second (path B, in parallel).
pub const ROW_A: std::ops::RangeInclusive<usize> = 1..=8;
pub const ROW_B: std::ops::RangeInclusive<usize> = 11..=18;

/// A processing block of the grid. `params` are the model's values in catalog order (a preset
/// saved before HXtra lacks the parameters HXtra adds); `extras` follow them on delays, reverbs
/// and FX loops (one bool, Trails by hypothesis).
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub position: usize,
    pub preset_id: u64,
    /// The cab of an amp + cab block (body key 24, its 26; -1 for none).
    pub cab: Option<u64>,
    /// The block kind (body key 9), the model browser's item kind: 17 an amp, 33 an amp + cab...
    pub kind: Option<u64>,
    pub enabled: bool,
    pub params: Vec<Value>,
    pub extras: Vec<Value>,
    pub ir_hash: Option<String>,
}

impl Block {
    /// A block from a grid cell's body (key 20), as presets and favorites (`0x40`) hold it.
    pub fn parse(position: usize, body: &Value) -> Result<Self> {
        let bad = |what: &str| Error::Preset(format!("block at position {position}: {what}"));
        let values = msg::get(body, block_key::VALUES).ok_or_else(|| bad("no values"))?;
        let list = msg::get(values, block_key::VALUE_LIST).and_then(Value::as_array).ok_or_else(|| bad("no value list"))?;
        let count = msg::get(values, block_key::PARAM_COUNT).and_then(Value::as_u64).ok_or_else(|| bad("no parameter count"))? as usize;
        let (params, extras) = match count <= list.len() {
            true => list.split_at(count),
            false => return Err(bad(&format!("{count} parameters but {} values", list.len()))),
        };
        let model = msg::get(body, block_key::MODEL).ok_or_else(|| bad("no model"))?;
        Ok(Self {
            position,
            preset_id: msg::get(model, block_key::MODEL_ID).and_then(Value::as_u64).ok_or_else(|| bad("no model id"))?,
            cab: msg::get(model, block_key::CAB_ID).and_then(Value::as_u64),
            kind: msg::get(body, block_key::KIND).and_then(Value::as_u64),
            enabled: msg::get(body, block_key::ENABLED).and_then(Value::as_bool).ok_or_else(|| bad("no on/off state"))?,
            params: params.to_vec(),
            extras: extras.to_vec(),
            ir_hash: msg::get(body, block_key::IR_HASH).and_then(msg::text),
        })
    }
}

/// The grid cells around the blocks (capture 25): input (cell 0, model 516), output (cell 9,
/// model 517), the split (cell 10) and the join (cell 19).
pub const CELL_INPUT: usize = 0;
pub const CELL_OUTPUT: usize = 9;
pub const CELL_SPLIT: usize = 10;
pub const CELL_JOIN: usize = 19;
pub const INPUT_MODEL: u64 = 516;
pub const OUTPUT_MODEL: u64 = 517;

/// Where the parallel path starts or ends, and its model's values. `position` is a column
/// boundary: 1 before the first column, 9 after the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    pub model: u64,
    pub position: u64,
    pub active: bool,
    pub params: Vec<Value>,
}

/// The input, output, split and join of a preset.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routing {
    pub input: Vec<Value>,
    pub output: Vec<Value>,
    pub split: Option<Junction>,
    pub join: Option<Junction>,
}

/// One thing a footswitch does: turn a block on or off, or move one of its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct FootswitchAssignment {
    pub block: u64,
    /// `None` for the block's on/off, else the parameter index.
    pub parameter: Option<u64>,
    pub label: String,
}

/// A footswitch of the preset (section 3, FS1..FS5): its label, color and assignments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Footswitch {
    pub label: String,
    pub color: Option<u64>,
    pub assignments: Vec<FootswitchAssignment>,
}

fn values_at(v: &Value, key: u64) -> Vec<Value> {
    msg::get(v, key).and_then(|x| msg::get(x, 4)).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn junction(body: &Value, key: u64) -> Option<Junction> {
    let j = msg::get(body, key)?;
    Some(Junction {
        model: msg::get(j, 8).and_then(Value::as_u64)?,
        position: msg::get(j, 13).and_then(Value::as_u64)?,
        active: msg::get(body, 18).and_then(Value::as_bool).unwrap_or(false),
        params: values_at(j, 7),
    })
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        self.processing_cells().into_iter().map(|(i, _)| i).collect()
    }

    /// The processing blocks in grid order.
    pub fn blocks(&self) -> Result<Vec<Block>> {
        self.processing_cells()
            .into_iter()
            .map(|(i, cell)| {
                let body = msg::get(&cell, BLOCK_BODY_KEY).ok_or_else(|| Error::Preset(format!("block at position {i} has no body")))?;
                Block::parse(i, body)
            })
            .collect()
    }

    /// The input, output, split and join cells (an empty `Routing` when the grid is missing).
    pub fn routing(&self) -> Routing {
        let grid = self.section(SECTION_BLOCKS).and_then(|s| msg::get(&s, BLOCK_GRID_KEY).cloned());
        let cells = grid.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
        let body = |i: usize| cells.get(i).and_then(|c| msg::get(c, BLOCK_BODY_KEY)).cloned().unwrap_or(Value::Nil);
        Routing {
            input: values_at(&body(CELL_INPUT), 7),
            output: values_at(&body(CELL_OUTPUT), 7),
            split: junction(&body(CELL_SPLIT), 15),
            join: junction(&body(CELL_JOIN), 17),
        }
    }

    /// The footswitches FS1..FS5 (section 3, key 8; capture 25).
    pub fn footswitches(&self) -> Vec<Footswitch> {
        let list = self.section(3).and_then(|s| msg::get(&s, 8).and_then(Value::as_array).cloned()).unwrap_or_default();
        list.iter()
            .map(|fs| {
                let entries = fs.as_array().cloned().unwrap_or_default();
                let first = entries.first();
                Footswitch {
                    label: first.and_then(|e| msg::get(e, 14)).and_then(msg::text).unwrap_or_default(),
                    color: first.and_then(|e| msg::get(e, 16)).and_then(Value::as_u64),
                    assignments: entries
                        .iter()
                        .filter_map(|e| {
                            let target = msg::get(e, 11)?;
                            let block = msg::get(target, 8).and_then(Value::as_u64)?;
                            let parameter = match msg::get(target, 0).and_then(Value::as_u64) {
                                Some(2) => msg::get(target, 9).and_then(|p| msg::get(p, 29)).and_then(Value::as_u64),
                                _ => None,
                            };
                            Some(FootswitchAssignment { block, parameter, label: msg::get(target, 5).and_then(msg::text).unwrap_or_default() })
                        })
                        .collect(),
                }
            })
            .collect()
    }

    fn processing_cells(&self) -> Vec<(usize, Value)> {
        let grid = self.section(SECTION_BLOCKS).and_then(|s| msg::get(&s, BLOCK_GRID_KEY).cloned());
        grid.as_ref()
            .and_then(Value::as_array)
            .map(|cells| {
                cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| msg::get(c, BLOCK_TYPE_KEY).and_then(Value::as_u64) == Some(BLOCK_TYPE_PROCESSING))
                    .map(|(i, c)| (i, c.clone()))
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

    fn block_cell(preset_id: u64, enabled: bool, params: u64, values: Vec<Value>) -> Value {
        let model = msg::int_map(&[(23, Value::from(false)), (25, Value::from(preset_id)), (26, Value::from(-1))]);
        let list = msg::int_map(&[(2, Value::from(values.len() as u64)), (3, Value::from(params)), (4, Value::Array(values))]);
        let body = msg::int_map(&[(24, model), (9, Value::from(1)), (10, Value::from(enabled)), (11, list)]);
        msg::int_map(&[(19, Value::from(6)), (20, body)])
    }

    #[test]
    fn reads_blocks_with_model_values_and_extras() {
        let empty = msg::int_map(&[(19, Value::from(8))]);
        let reverb = block_cell(636, false, 2, vec![Value::F32(1.5), Value::F32(0.03), Value::from(true)]);
        let comp = block_cell(124, true, 2, vec![Value::F32(-27.5), Value::from(0)]);
        let grid = msg::int_map(&[(22, Value::Array(vec![empty.clone(), comp, empty, reverb]))]);
        let p = PresetFile::parse(file(&[(7, meta()), (0, grid)])).unwrap();
        let blocks = p.blocks().unwrap();
        assert_eq!(p.occupied_positions(), vec![1, 3]);
        assert_eq!((blocks[0].position, blocks[0].preset_id, blocks[0].enabled, blocks[0].params.len()), (1, 124, true, 2));
        assert_eq!((blocks[0].cab, blocks[0].kind), (None, Some(1)), "26 = -1 is no cab");
        assert!(blocks[0].extras.is_empty());
        assert_eq!((blocks[1].preset_id, blocks[1].enabled, blocks[1].extras.clone()), (636, false, vec![Value::from(true)]));
    }

    #[test]
    fn reads_routing_and_footswitches_as_capture_25_saved_them() {
        let values = |v: Vec<Value>| msg::int_map(&[(2, Value::from(v.len() as u64)), (3, Value::from(v.len() as u64)), (4, Value::Array(v))]);
        let cell = |t: u64, body: Value| msg::int_map(&[(19, Value::from(t)), (20, body)]);
        let input = cell(0, msg::int_map(&[(5, Value::from(1)), (7, values(vec![Value::from(true), Value::F32(-44.4), Value::F32(0.327)]))]));
        let output = cell(1, msg::int_map(&[(6, Value::from(1)), (7, values(vec![Value::F32(0.675), Value::F32(-54.7)]))]));
        let split = cell(2, msg::int_map(&[(15, msg::int_map(&[(8, Value::from(257)), (13, Value::from(5)), (7, values(vec![Value::F32(0.36), Value::F32(0.65), Value::from(false)]))])), (18, Value::from(true))]));
        let join = cell(3, msg::int_map(&[(17, msg::int_map(&[(8, Value::from(151)), (13, Value::from(7)), (7, values(vec![Value::F32(-38.3)]))])), (18, Value::from(true))]));
        let empty = cell(8, Value::Nil);
        let mut cells = vec![empty; 20];
        (cells[0], cells[9], cells[10], cells[19]) = (input, output, split, join);
        let bypass = msg::int_map(&[(10, Value::from(0)), (11, msg::int_map(&[(0, Value::from(1)), (5, Value::from("Simple Delay\0")), (8, Value::from(15))])), (14, Value::from("fefewfewfew\0")), (16, Value::from(5))]);
        let param = msg::int_map(&[(10, Value::from(7)), (11, msg::int_map(&[(0, Value::from(2)), (5, Value::from("Treble\0")), (8, Value::from(4)), (9, msg::int_map(&[(29, Value::from(3))]))])), (14, Value::from("fefewfewfew\0")), (16, Value::from(5))]);
        let fs = msg::int_map(&[(7, Value::from(0)), (8, Value::Array(vec![Value::Nil, Value::Array(vec![param, bypass]), Value::Nil, Value::Nil, Value::Nil]))]);
        let p = PresetFile::parse(file(&[(7, meta()), (0, msg::int_map(&[(22, Value::Array(cells))])), (3, fs)])).unwrap();
        let r = p.routing();
        assert_eq!((r.input.len(), r.output.len()), (3, 2));
        assert_eq!(r.split, Some(Junction { model: 257, position: 5, active: true, params: vec![Value::F32(0.36), Value::F32(0.65), Value::from(false)] }));
        assert_eq!(r.join.map(|j| (j.model, j.position)), Some((151, 7)));
        let footswitches = p.footswitches();
        assert_eq!(footswitches[1].label, "fefewfewfew");
        assert_eq!(footswitches[1].color, Some(5));
        assert_eq!(footswitches[1].assignments, vec![
            FootswitchAssignment { block: 4, parameter: Some(3), label: "Treble".into() },
            FootswitchAssignment { block: 15, parameter: None, label: "Simple Delay".into() },
        ]);
        assert!(footswitches[0].assignments.is_empty());
    }

    #[test]
    fn a_block_claiming_more_parameters_than_values_is_an_error() {
        let bad = block_cell(124, true, 3, vec![Value::F32(-27.5)]);
        let grid = msg::int_map(&[(22, Value::Array(vec![bad]))]);
        let p = PresetFile::parse(file(&[(7, meta()), (0, grid)])).unwrap();
        assert!(matches!(p.blocks(), Err(Error::Preset(_))));
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

//! The HX Stomp's editor and librarian services, as HX Edit uses them.
//! Every command here only reads from the device, except
//! [`Stomp::upload_preset`], which overwrites a preset slot.

use std::time::Duration;

use rmpv::Value;

use crate::msg::{self, empty_map, int_map};
use crate::session::{ChannelId, Port, Session};
use crate::{Error, Result};

pub const EDITOR: Port = Port { local: 0x1000, remote: 0x03ED, service: 6 };
pub const LIBRARIAN: Port = Port { local: 0x1001, remote: 0x03EF, service: 2 };
pub const NOTIFY: Port = Port { local: 0x1002, remote: 0x03F0, service: 4 };

pub const MAX_PRESET_NAME: usize = 16;
pub const MAX_PRESET_FILE: usize = 0xFFFF;

const PRESET_LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const NOTE_PRESET_LOADED: u64 = 4;
const NOTE_PAYLOAD: u64 = 0x6a;

mod editor_cmd {
    pub const EDIT_BUFFER: u8 = 0x16;
    pub const CURRENT_PRESET: u8 = 0x17;
    pub const GLOBALS: u8 = 0x55;
}

mod librarian_cmd {
    pub const SETLISTS: u8 = 0x00;
    pub const PRESET_LIST: u8 = 0x01;
    pub const PRESET_FILE: u8 = 0x04;
    pub const PRESET_UPLOAD: u8 = 0x08;
    pub const IR_DATA: u8 = 0x0b;
    pub const IR_INFO: u8 = 0x0c;
    pub const IR_LIST: u8 = 0x0d;
    pub const FAVORITE_LIST: u8 = 0x70;
    pub const FAVORITE: u8 = 0x71;
    pub const BATCH_END: u8 = 0xfe;
    pub const BATCH_BEGIN: u8 = 0xff;
}

mod key {
    pub const SETLIST: u64 = 0x6b;
    pub const PRESET: u64 = 0x6c;
    pub const NAME: u64 = 0x6d;
    pub const IR_INDEX: u64 = 0x70;
    pub const FORMAT: u64 = 0x65;
    pub const FAVORITE_INDEX: u64 = 0x76;
    pub const MODIFIED: u64 = 0x75;
    pub const PRESET_DATA: u64 = 0x6e;
    pub const FLAG_7B: u64 = 0x7b;
    pub const FLAG_7C: u64 = 0x7c;
    pub const FLAG_7D: u64 = 0x7d;
}

/// The value HX Edit sends in key 0x65 of librarian requests.
const LIBRARIAN_FORMAT: u64 = 2;

#[derive(Debug, Clone)]
pub struct CurrentPreset {
    pub setlist: u64,
    pub slot: u64,
    pub name: String,
    pub modified: bool,
}

#[derive(Debug, Clone)]
pub struct PresetEntry {
    pub slot: usize,
    pub file_id: u64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct IndexedEntry {
    pub index: u64,
    pub name: String,
}

pub struct Stomp {
    session: Session,
    editor: ChannelId,
    librarian: ChannelId,
}

impl Stomp {
    pub fn connect(timeout: Duration) -> Result<Self> {
        let mut session = Session::open(timeout)?;
        let librarian = session.open_channel(LIBRARIAN)?;
        let editor = session.open_channel(EDITOR)?;
        session.open_notify(NOTIFY)?;
        Ok(Self { session, editor, librarian })
    }

    pub fn product_id(&self) -> u16 {
        self.session.product_id()
    }

    pub fn current_preset(&mut self) -> Result<CurrentPreset> {
        let r = self.session.request(self.editor, editor_cmd::CURRENT_PRESET, Value::Nil)?;
        Ok(CurrentPreset {
            setlist: msg::get(&r, key::SETLIST).and_then(Value::as_u64).ok_or_else(|| missing("current preset", "setlist"))?,
            slot: msg::get(&r, key::PRESET).and_then(Value::as_u64).ok_or_else(|| missing("current preset", "slot"))?,
            name: msg::get(&r, key::NAME).and_then(msg::text).unwrap_or_default(),
            modified: msg::get(&r, key::MODIFIED).and_then(Value::as_bool).unwrap_or(false),
        })
    }

    pub fn edit_buffer(&mut self) -> Result<Vec<u8>> {
        let r = self.session.request(self.editor, editor_cmd::EDIT_BUFFER, Value::Nil)?;
        blob(&r, "edit buffer")
    }

    pub fn globals(&mut self) -> Result<Vec<u8>> {
        let r = self.session.request(self.editor, editor_cmd::GLOBALS, empty_map())?;
        blob(&r, "globals")
    }

    pub fn setlists(&mut self) -> Result<Vec<IndexedEntry>> {
        let r = self.session.request(self.librarian, librarian_cmd::SETLISTS, Value::Nil)?;
        single_entry_maps(&r, "setlist list").map(|v| v.into_iter().map(|(index, v)| IndexedEntry { index, name: msg::text(v).unwrap_or_default() }).collect())
    }

    /// Presets in slot order; each entry is keyed by the preset's file id.
    pub fn preset_list(&mut self, setlist: u64) -> Result<Vec<PresetEntry>> {
        let params = int_map(&[(key::SETLIST, Value::from(setlist)), (key::FORMAT, Value::from(LIBRARIAN_FORMAT))]);
        let r = self.session.request(self.librarian, librarian_cmd::PRESET_LIST, params)?;
        let entries = single_entry_maps(&r, "preset list")?;
        Ok(entries
            .into_iter()
            .enumerate()
            .map(|(slot, (file_id, info))| PresetEntry { slot, file_id, name: msg::get(info, key::NAME).and_then(msg::text).unwrap_or_default() })
            .collect())
    }

    /// The preset file exactly as stored, including HXtra's snapshot bank B.
    pub fn preset_file(&mut self, setlist: u64, slot: usize) -> Result<Vec<u8>> {
        let params = int_map(&[
            (key::SETLIST, Value::from(setlist)),
            (key::PRESET, Value::from(slot as u64)),
            (key::FORMAT, Value::from(LIBRARIAN_FORMAT)),
        ]);
        let r = self.session.request(self.librarian, librarian_cmd::PRESET_FILE, params)?;
        blob(&r, "preset file")
    }

    pub fn ir_list(&mut self) -> Result<Vec<IndexedEntry>> {
        let params = int_map(&[(key::FORMAT, Value::from(LIBRARIAN_FORMAT))]);
        let r = self.session.request(self.librarian, librarian_cmd::IR_LIST, params)?;
        indexed_list(&r, key::IR_INDEX, "IR list")
    }

    pub fn ir_info(&mut self, index: u64) -> Result<Value> {
        self.session.request(self.librarian, librarian_cmd::IR_INFO, int_map(&[(key::IR_INDEX, Value::from(index))]))
    }

    pub fn ir_data(&mut self, index: u64) -> Result<Vec<u8>> {
        let params = int_map(&[(key::IR_INDEX, Value::from(index)), (key::FORMAT, Value::from(LIBRARIAN_FORMAT))]);
        let r = self.session.request(self.librarian, librarian_cmd::IR_DATA, params)?;
        blob(&r, "IR data")
    }

    pub fn favorite_list(&mut self) -> Result<Vec<IndexedEntry>> {
        let r = self.session.request(self.librarian, librarian_cmd::FAVORITE_LIST, Value::Nil)?;
        indexed_list(&r, key::FAVORITE_INDEX, "favorite list")
    }

    pub fn favorite(&mut self, index: u64) -> Result<Value> {
        self.session.request(self.librarian, librarian_cmd::FAVORITE, int_map(&[(key::FAVORITE_INDEX, Value::from(index))]))
    }

    /// Overwrite a preset slot with a preset file, sent byte for byte as HX Edit
    /// sends its imports (librarian 0x08). When the slot is the current preset
    /// the device reloads it; this returns only after that load has finished
    /// (notification 4 for the slot), as HX Edit waits before reading it again.
    /// Wrap it in [`Stomp::batch_begin`] / [`Stomp::batch_end`] as HX Edit does.
    pub fn upload_preset(&mut self, setlist: u64, slot: usize, name: &str, file: &[u8]) -> Result<()> {
        validate_name(name)?;
        if file.len() > MAX_PRESET_FILE {
            return Err(Error::Preset(format!("{} bytes is more than the {MAX_PRESET_FILE} a preset upload carries", file.len())));
        }
        let current = self.current_preset()?;
        let reloads = current.setlist == setlist && current.slot == slot as u64;
        let params = upload_params(setlist, slot, name, file);
        self.session.clear_notifications();
        self.session.request_raw(self.librarian, librarian_cmd::PRESET_UPLOAD, &params)?;
        match reloads {
            true => self
                .session
                .wait_notification("the device to reload the current preset", PRESET_LOAD_TIMEOUT, |n| is_preset_loaded(n, setlist, slot))
                .map(drop),
            false => Ok(()),
        }
    }

    /// HX Edit brackets its librarian reads with these two commands.
    pub fn batch_begin(&mut self) -> Result<()> {
        self.session.request(self.librarian, librarian_cmd::BATCH_BEGIN, empty_map()).map(drop)
    }

    pub fn batch_end(&mut self) -> Result<()> {
        self.session.request(self.librarian, librarian_cmd::BATCH_END, empty_map()).map(drop)
    }
}

/// Notification 4: `{0x6a: {0x6b: setlist, 0x6c: slot}}` once the device has loaded a preset.
fn is_preset_loaded(note: &Value, setlist: u64, slot: usize) -> bool {
    let Some((NOTE_PRESET_LOADED, body)) = msg::notification(note) else { return false };
    let payload = msg::get(body, NOTE_PAYLOAD);
    let field = |k| payload.and_then(|p| msg::get(p, k)).and_then(Value::as_u64);
    field(key::SETLIST) == Some(setlist) && field(key::PRESET) == Some(slot as u64)
}

pub fn validate_name(name: &str) -> Result<()> {
    match name {
        "" => Err(Error::Preset("the preset name is empty".into())),
        n if n.len() > MAX_PRESET_NAME => Err(Error::Preset(format!("\"{n}\" is longer than {MAX_PRESET_NAME} characters"))),
        n if !n.bytes().all(|b| (0x20..0x7f).contains(&b)) => Err(Error::Preset(format!("\"{n}\" has characters other than printable ASCII"))),
        _ => Ok(()),
    }
}

/// `{0x6b: setlist, 0x6c: slot, 0x6d: "name\0", 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: str(file)}`,
/// in HX Edit's order and types.
fn upload_params(setlist: u64, slot: usize, name: &str, file: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(file.len() + 64);
    p.push(0x87);
    for (k, v) in [(key::SETLIST, setlist), (key::PRESET, slot as u64)] {
        msg::write_uint(&mut p, k);
        msg::write_uint(&mut p, v);
    }
    msg::write_uint(&mut p, key::NAME);
    msg::write_raw_str(&mut p, format!("{name}\0").as_bytes());
    for k in [key::FLAG_7B, key::FLAG_7C] {
        msg::write_uint(&mut p, k);
        p.push(0xc2);
    }
    msg::write_uint(&mut p, key::FLAG_7D);
    msg::write_uint(&mut p, 0);
    msg::write_uint(&mut p, key::PRESET_DATA);
    msg::write_raw_str(&mut p, file);
    p
}

fn missing(what: &str, field: &str) -> Error {
    Error::Protocol(format!("{what}: reply has no {field}"))
}

fn blob(v: &Value, what: &str) -> Result<Vec<u8>> {
    msg::bytes(v).map(<[u8]>::to_vec).ok_or_else(|| Error::Protocol(format!("{what}: expected a binary result, got {v}")))
}

/// `[{k: v}, {k: v}, ...]` -> `[(k, v), ...]`, in order.
fn single_entry_maps<'a>(v: &'a Value, what: &str) -> Result<Vec<(u64, &'a Value)>> {
    let items = v.as_array().ok_or_else(|| Error::Protocol(format!("{what}: expected a list, got {v}")))?;
    items
        .iter()
        .map(|item| match item.as_map().map(Vec::as_slice) {
            Some([(k, v)]) => k.as_u64().map(|k| (k, v)).ok_or_else(|| Error::Protocol(format!("{what}: non-integer key {k}"))),
            _ => Err(Error::Protocol(format!("{what}: unexpected entry {item}"))),
        })
        .collect()
}

/// `[{index_key: i, 0x6d: name, ...}, ...]` -> `[(i, name), ...]`.
fn indexed_list(v: &Value, index_key: u64, what: &str) -> Result<Vec<IndexedEntry>> {
    let items = v.as_array().ok_or_else(|| Error::Protocol(format!("{what}: expected a list, got {v}")))?;
    items
        .iter()
        .map(|item| {
            let index = msg::get(item, index_key).and_then(Value::as_u64).ok_or_else(|| missing(what, "index"))?;
            Ok(IndexedEntry { index, name: msg::get(item, key::NAME).and_then(msg::text).unwrap_or_default() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn upload_request_matches_hx_edit() {
        let file = [0xa9; 3222];
        let req = msg::encode_request_raw(1006, librarian_cmd::PRESET_UPLOAD, &upload_params(0, 11, "AC30 Clean", &file));
        let head = hex("8366cd03ee640865876b006c0b6dab4143333020436c65616e007bc27cc27d006eda0c96");
        assert_eq!(&req[..head.len()], &head[..]);
        assert_eq!(&req[head.len()..], &file[..]);
        assert_eq!(req.len(), 3258);
    }

    #[test]
    fn recognises_the_preset_loaded_notification() {
        let note = |kind: u64, slot: u64| {
            let payload = int_map(&[(key::SETLIST, Value::from(0)), (key::PRESET, Value::from(slot))]);
            let body = int_map(&[(0x52, Value::from(1)), (0x44, Value::from(1)), (0x79, Value::from(6)), (NOTE_PAYLOAD, payload)]);
            int_map(&[(0x69, Value::from(kind)), (0x6a, body)])
        };
        assert!(is_preset_loaded(&note(4, 11), 0, 11));
        assert!(!is_preset_loaded(&note(4, 10), 0, 11));
        assert!(!is_preset_loaded(&note(8, 11), 0, 11));
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("AC30 Clean").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("seventeen chars!!").is_err());
        assert!(validate_name("Ação").is_err());
    }
}

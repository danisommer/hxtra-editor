//! The HX Stomp's editor and librarian services, as HX Edit uses them.
//! Reads, plus the writes a restore needs: [`Stomp::upload_preset`],
//! [`Stomp::write_globals`], [`Stomp::upload_ir`], [`Stomp::delete_ir`] and
//! [`Stomp::write_favorite`].

use std::time::Duration;

use rmpv::Value;
use sha2::{Digest, Sha256};

use crate::link::Link;
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
    pub const LOAD_PRESET: u8 = 0x14;
    pub const EDIT_BUFFER: u8 = 0x16;
    pub const CURRENT_PRESET: u8 = 0x17;
    pub const SET_SETTING: u8 = 0x19;
    pub const SET_PARAMETER: u8 = 0x1e;
    pub const SAVE_PRESET: u8 = 0x47;
    pub const SET_BLOCK_ENABLED: u8 = 0x29;
    pub const SELECT_SNAPSHOT: u8 = 0x58;
    pub const GLOBALS: u8 = 0x55;
    pub const GLOBALS_WRITE: u8 = 0x56;
}

mod librarian_cmd {
    pub const SETLISTS: u8 = 0x00;
    pub const PRESET_LIST: u8 = 0x01;
    pub const PRESET_FILE: u8 = 0x04;
    pub const PRESET_UPLOAD: u8 = 0x08;
    pub const IR_UPLOAD: u8 = 0x09;
    pub const IR_DATA: u8 = 0x0b;
    pub const IR_INFO: u8 = 0x0c;
    pub const IR_LIST: u8 = 0x0d;
    pub const IR_DELETE: u8 = 0x0f;
    pub const FAVORITE_LIST: u8 = 0x70;
    pub const FAVORITE: u8 = 0x71;
    pub const FAVORITE_WRITE: u8 = 0x72;
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
    /// The blob of an upload: a preset file (librarian 0x08) or global settings (editor 0x56).
    pub const PRESET_DATA: u64 = 0x6e;
    pub const FLAG_7B: u64 = 0x7b;
    pub const FLAG_7C: u64 = 0x7c;
    pub const FLAG_7D: u64 = 0x7d;
    pub const SETTING_ID: u64 = 0x76;
    pub const SETTING_VALUE: u64 = 0x77;
    pub const BLOCK: u64 = 0x62;
    pub const COMMIT: u64 = 0x1d;
    pub const PARAMETER_PATH: u64 = 0x1a;
    pub const PARAMETER_INDEX: u64 = 0x1c;
    pub const ENABLED: u64 = 0x3b;
    pub const SNAPSHOT: u64 = 0x5c;
    pub const FAVORITE_DATA: u64 = 0x22;
    pub const FAVORITE_FLAG: u64 = 0x1f;
    pub const FAVORITE_BLOCK: u64 = 0x40;
    pub const FAVORITE_NUMBER: u64 = 0x1c;
}

/// The value HX Edit sends in key 0x65 of librarian requests.
const LIBRARIAN_FORMAT: u64 = 2;

#[derive(Debug, Clone)]
pub struct CurrentPreset {
    pub setlist: u64,
    pub slot: u64,
    pub name: String,
    pub modified: bool,
    /// The current snapshot's index, 0..2 within the live bank (key 0x5c).
    pub snapshot: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PresetEntry {
    pub slot: usize,
    pub file_id: u64,
    pub name: String,
    pub flags: ListFlags,
}

/// The three values each preset list entry carries next to its name (keys
/// 0x7b, 0x7c, 0x7d; meaning not yet known). `None` when the entry lacks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ListFlags {
    pub flag_7b: Option<bool>,
    pub flag_7c: Option<bool>,
    pub flag_7d: Option<u64>,
}

impl ListFlags {
    fn from_entry(info: &Value) -> Self {
        Self {
            flag_7b: msg::get(info, key::FLAG_7B).and_then(Value::as_bool),
            flag_7c: msg::get(info, key::FLAG_7C).and_then(Value::as_bool),
            flag_7d: msg::get(info, key::FLAG_7D).and_then(Value::as_u64),
        }
    }
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
    /// The HX Stomp on USB.
    pub fn connect(timeout: Duration) -> Result<Self> {
        Self::start(Session::open(timeout)?)
    }

    /// The editor, librarian and notify channels over any link (a simulated device in tests).
    pub fn over(link: Box<dyn Link>, timeout: Duration) -> Result<Self> {
        Self::start(Session::over(link, timeout))
    }

    fn start(mut session: Session) -> Result<Self> {
        let librarian = session.open_channel(LIBRARIAN)?;
        let editor = session.open_channel(EDITOR)?;
        session.open_notify(NOTIFY)?;
        Ok(Self { session, editor, librarian })
    }

    /// See [`Session::pump`].
    pub fn pump(&mut self) -> Result<()> {
        self.session.pump()
    }

    /// See [`Session::take_notifications`].
    pub fn take_notifications(&mut self) -> Vec<Value> {
        self.session.take_notifications()
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
            snapshot: msg::get(&r, key::SNAPSHOT).and_then(Value::as_u64),
        })
    }

    /// Load a preset into the edit buffer (editor 0x14), as a footswitch or HX Edit does;
    /// nothing is stored. The unit then reports the load (notifications 8 and 4).
    pub fn select_preset(&mut self, setlist: u64, slot: usize) -> Result<()> {
        let params = int_map(&[(key::SETLIST, Value::from(setlist)), (key::PRESET, Value::from(slot as u64))]);
        self.session.request(self.editor, editor_cmd::LOAD_PRESET, params).map(drop)
    }

    /// Set one global setting (editor 0x19); the value's MessagePack type must be the
    /// setting's own (int, float or bool). The unit echoes it as notification 0x16.
    pub fn set_setting(&mut self, id: u64, value: Value) -> Result<()> {
        let params = int_map(&[(key::SETTING_ID, Value::from(id)), (key::SETTING_VALUE, value)]);
        self.session.request(self.editor, editor_cmd::SET_SETTING, params).map(drop)
    }

    /// Set a block parameter in the edit buffer (editor 0x1e), with the fields its
    /// notification carries; the value's type must be the parameter's own. Nothing is stored.
    pub fn set_parameter(&mut self, block: u64, path: u64, index: u64, value: Value) -> Result<()> {
        let params = int_map(&[
            (key::BLOCK, Value::from(block)),
            (key::COMMIT, Value::from(true)),
            (key::PARAMETER_PATH, Value::from(path)),
            (key::PARAMETER_INDEX, Value::from(index)),
            (key::SETTING_VALUE, value),
        ]);
        self.session.request(self.editor, editor_cmd::SET_PARAMETER, params).map(drop)
    }

    /// Store the edit buffer into a preset slot with a name (editor 0x47), as HX Edit's
    /// Save does. The unit serializes the preset itself, so HXtra's snapshots 4..6 go
    /// with it; it then reports notification 5 (saved).
    pub fn save_preset(&mut self, setlist: u64, slot: usize, name: &str) -> Result<()> {
        validate_name(name)?;
        self.session.request(self.editor, editor_cmd::SAVE_PRESET, save_params(setlist, slot, name)).map(drop)
    }

    /// Turn a block on or off in the edit buffer (editor 0x29). Nothing is stored.
    pub fn set_block_enabled(&mut self, block: u64, enabled: bool) -> Result<()> {
        let params = int_map(&[(key::BLOCK, Value::from(block)), (key::ENABLED, Value::from(enabled))]);
        self.session.request(self.editor, editor_cmd::SET_BLOCK_ENABLED, params).map(drop)
    }

    /// Recall snapshot `index` (0..2) of the live bank (editor 0x58). Nothing is stored.
    pub fn select_snapshot(&mut self, index: u64) -> Result<()> {
        self.session.request(self.editor, editor_cmd::SELECT_SNAPSHOT, int_map(&[(key::SNAPSHOT, Value::from(index))])).map(drop)
    }

    pub fn edit_buffer(&mut self) -> Result<Vec<u8>> {
        let r = self.session.request(self.editor, editor_cmd::EDIT_BUFFER, Value::Nil)?;
        blob(&r, "edit buffer")
    }

    pub fn globals(&mut self) -> Result<Vec<u8>> {
        let r = self.session.request(self.editor, editor_cmd::GLOBALS, empty_map())?;
        blob(&r, "globals")
    }

    /// Write global settings in bulk (editor 0x56), with a blob shaped as
    /// [`crate::globals::Globals::restore_blob`] builds it, sent as a str as HX Edit does.
    pub fn write_globals(&mut self, blob: &[u8]) -> Result<()> {
        self.session.request_raw(self.editor, editor_cmd::GLOBALS_WRITE, &globals_params(blob)).map(drop)
    }

    /// Write an IR into an empty slot (librarian 0x09), with the info map a backup read
    /// through [`Stomp::ir_info`] and the data from [`Stomp::ir_data`].
    pub fn upload_ir(&mut self, slot: u64, info: &Value, data: &[u8]) -> Result<()> {
        let params = ir_upload_params(slot, info, data)?;
        self.session.request_raw(self.librarian, librarian_cmd::IR_UPLOAD, &params).map(drop)
    }

    pub fn delete_ir(&mut self, slot: u64) -> Result<()> {
        self.session.request(self.librarian, librarian_cmd::IR_DELETE, int_map(&[(key::IR_INDEX, Value::from(slot))])).map(drop)
    }

    /// Write a favorite (librarian 0x72) from the value [`Stomp::favorite`] returned,
    /// without the number the device assigns.
    pub fn write_favorite(&mut self, index: u64, favorite: &Value) -> Result<()> {
        if msg::has_invalid_utf8(favorite) {
            return Err(Error::Backup(format!("favorite {index} holds a str that is not UTF-8; it cannot be sent back as it was read")));
        }
        let name = msg::get(favorite, key::NAME).cloned().ok_or_else(|| missing("favorite", "name"))?;
        let params = int_map(&[
            (key::FAVORITE_INDEX, Value::from(index)),
            (key::FAVORITE_DATA, favorite_without_number(favorite)),
            (key::FAVORITE_FLAG, Value::from(false)),
            (key::NAME, name),
        ]);
        self.session.request(self.librarian, librarian_cmd::FAVORITE_WRITE, params).map(drop)
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
            .map(|(slot, (file_id, info))| PresetEntry {
                slot,
                file_id,
                name: msg::get(info, key::NAME).and_then(msg::text).unwrap_or_default(),
                flags: ListFlags::from_entry(info),
            })
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
    /// `flags` are the list entry's values; `None` sends what HX Edit sends on an import.
    pub fn upload_preset(&mut self, setlist: u64, slot: usize, name: &str, flags: ListFlags, file: &[u8]) -> Result<()> {
        validate_name(name)?;
        if file.len() > MAX_PRESET_FILE {
            return Err(Error::Preset(format!("{} bytes is more than the {MAX_PRESET_FILE} a preset upload carries", file.len())));
        }
        let current = self.current_preset()?;
        let reloads = current.setlist == setlist && current.slot == slot as u64;
        let params = upload_params(setlist, slot, name, flags, file);
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

/// `{0x6b: setlist, 0x6c: slot, 0x6d: "name\0"}`, as HX Edit sends its Save.
fn save_params(setlist: u64, slot: usize, name: &str) -> Value {
    int_map(&[
        (key::SETLIST, Value::from(setlist)),
        (key::PRESET, Value::from(slot as u64)),
        (key::NAME, Value::from(format!("{name}\0"))),
    ])
}

/// `{0x6e: str(blob)}`, as HX Edit sends its globals restore.
fn globals_params(blob: &[u8]) -> Vec<u8> {
    let mut params = vec![0x81];
    msg::write_uint(&mut params, key::PRESET_DATA);
    msg::write_raw_str(&mut params, blob);
    params
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

/// `{0x6b: setlist, 0x6c: slot, 0x6d: "name\0", 0x7b, 0x7c, 0x7d, 0x6e: str(file)}`,
/// in HX Edit's order and types (flags default to an import's `false, false, 0`).
fn upload_params(setlist: u64, slot: usize, name: &str, flags: ListFlags, file: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(file.len() + 64);
    p.push(0x87);
    for (k, v) in [(key::SETLIST, setlist), (key::PRESET, slot as u64)] {
        msg::write_uint(&mut p, k);
        msg::write_uint(&mut p, v);
    }
    msg::write_uint(&mut p, key::NAME);
    msg::write_raw_str(&mut p, format!("{name}\0").as_bytes());
    for (k, v) in [(key::FLAG_7B, flags.flag_7b), (key::FLAG_7C, flags.flag_7c)] {
        msg::write_uint(&mut p, k);
        rmp::encode::write_bool(&mut p, v.unwrap_or(false)).expect("writing to a Vec cannot fail");
    }
    msg::write_uint(&mut p, key::FLAG_7D);
    msg::write_uint(&mut p, flags.flag_7d.unwrap_or(0));
    msg::write_uint(&mut p, key::PRESET_DATA);
    msg::write_raw_str(&mut p, file);
    p
}

pub const MAX_IR_NAME: usize = 64;

/// The info map of a new User IR, with the fields HX Edit sends on an import (capture 12):
/// `0x72: 1`, `0x73: 3`, flags off. `0x71` is a u32 whose meaning is not known (not a CRC32,
/// Adler-32, FNV or Murmur hash of the data or of the .wav); it gets the first 4 bytes of the
/// data's SHA-256.
pub fn new_ir_info(slot: u64, name: &str, data: &[u8]) -> Result<Value> {
    match (name.is_empty(), name.len() <= MAX_IR_NAME, name.bytes().all(|b| (0x20..0x7f).contains(&b))) {
        (false, true, true) => {}
        _ => return Err(Error::Backup(format!("an IR name is 1 to {MAX_IR_NAME} printable ASCII characters, not {name:?}"))),
    }
    let digest = Sha256::digest(data);
    let id = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    Ok(int_map(&[
        (key::IR_INDEX, Value::from(slot)),
        (0x71, Value::from(id)),
        (key::NAME, Value::from(format!("{name}\0"))),
        (0x72, Value::from(1)),
        (0x73, Value::from(3)),
        (0x7b, Value::from(false)),
        (0x7c, Value::from(false)),
        (0x7d, Value::from(0)),
    ]))
}

/// The IR's own info map (as the 0x0c read gives it) with 0x70 set to `slot`,
/// then `0x6e: str(data)`: HX Edit's 0x09 import.
fn ir_upload_params(slot: u64, info: &Value, data: &[u8]) -> Result<Vec<u8>> {
    let entries = info.as_map().ok_or_else(|| Error::Backup(format!("IR info is not a map: {info}")))?;
    if msg::has_invalid_utf8(info) {
        return Err(Error::Backup("IR info holds a str that is not UTF-8; it cannot be sent back as it was read".into()));
    }
    let mut p = Vec::with_capacity(data.len() + 128);
    rmp::encode::write_map_len(&mut p, entries.len() as u32 + 1).expect("writing to a Vec cannot fail");
    for (k, v) in entries {
        p.extend(msg::encode_value(k));
        match k.as_u64() {
            Some(key::IR_INDEX) => msg::write_uint(&mut p, slot),
            _ => p.extend(msg::encode_value(v)),
        }
    }
    msg::write_uint(&mut p, key::PRESET_DATA);
    msg::write_raw_str(&mut p, data);
    Ok(p)
}

/// A favorite without the number the device assigns to it (0x40 → 0x1c), as HX Edit sends it back.
pub fn favorite_without_number(favorite: &Value) -> Value {
    let Some(entries) = favorite.as_map() else { return favorite.clone() };
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| match (k.as_u64(), v.as_map()) {
                (Some(key::FAVORITE_BLOCK), Some(block)) => (
                    k.clone(),
                    Value::Map(block.iter().filter(|(bk, _)| bk.as_u64() != Some(key::FAVORITE_NUMBER)).cloned().collect()),
                ),
                _ => (k.clone(), v.clone()),
            })
            .collect(),
    )
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
        let req = msg::encode_request_raw(1006, librarian_cmd::PRESET_UPLOAD, &upload_params(0, 11, "AC30 Clean", ListFlags::default(), &file));
        let head = hex("8366cd03ee640865876b006c0b6dab4143333020436c65616e007bc27cc27d006eda0c96");
        assert_eq!(&req[..head.len()], &head[..]);
        assert_eq!(&req[head.len()..], &file[..]);
        assert_eq!(req.len(), 3258);
    }

    #[test]
    fn ir_upload_request_matches_hx_edit() {
        let info = int_map(&[
            (0x70, Value::from(0)),
            (0x71, Value::from(0xe856311cu64)),
            (0x6d, Value::from("1960-G12M25-R121-Cap-0_5in\0")),
            (0x72, Value::from(1)),
            (0x73, Value::from(3)),
            (0x7b, Value::from(false)),
            (0x7c, Value::from(false)),
            (0x7d, Value::from(0)),
        ]);
        let data = [0x01; 8192];
        let req = msg::encode_request_raw(1006, librarian_cmd::IR_UPLOAD, &ir_upload_params(6, &info, &data).unwrap());
        let head = hex("8366cd03ee64096589700671cee856311c6dbb313936302d4731324d32352d523132312d4361702d305f35696e00720173037bc27cc27d006eda2000");
        assert_eq!(&req[..head.len()], &head[..]);
        assert_eq!(&req[head.len()..], &data[..]);
    }

    #[test]
    fn favorites_lose_only_their_number() {
        let block = int_map(&[(0x13, Value::from(6)), (0x1c, Value::from(3)), (0x14, Value::from("body"))]);
        let fav = int_map(&[(0x76, Value::from(2)), (0x6d, Value::from("Comp\0")), (0x40, block)]);
        let stripped = favorite_without_number(&fav);
        let block = msg::get(&stripped, 0x40).unwrap();
        assert_eq!(msg::get(block, 0x1c), None);
        assert_eq!(msg::get(block, 0x13).and_then(Value::as_u64), Some(6));
        assert_eq!(msg::get(&stripped, 0x76).and_then(Value::as_u64), Some(2));
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
    fn globals_write_request_matches_hx_edit() {
        let blob = [0x84; 620];
        let req = msg::encode_request_raw(1009, editor_cmd::GLOBALS_WRITE, &globals_params(&blob));
        let head = hex("8366cd03f1645665816eda026c");
        assert_eq!(&req[..head.len()], &head[..]);
        assert_eq!(&req[head.len()..], &blob[..]);
    }

    #[test]
    fn save_request_matches_hx_edit() {
        let req = msg::encode_request(1037, editor_cmd::SAVE_PRESET, save_params(0, 11, "SAVE TEST"));
        assert_eq!(req, hex("8366cd040d644765836b006c0b6daa53415645205445535400"));
    }

    #[test]
    fn new_ir_info_has_hx_edits_fields_in_its_order() {
        let info = new_ir_info(2, "VOX AC30 NAM", &[0; 8]).unwrap();
        let keys: Vec<u64> = info.as_map().unwrap().iter().filter_map(|(k, _)| k.as_u64()).collect();
        assert_eq!(keys, vec![0x70, 0x71, 0x6d, 0x72, 0x73, 0x7b, 0x7c, 0x7d]);
        assert_eq!(msg::get(&info, 0x6d).and_then(Value::as_str), Some("VOX AC30 NAM\0"));
        assert!(new_ir_info(2, "", &[]).is_err() && new_ir_info(2, "ação", &[]).is_err());
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("AC30 Clean").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("seventeen chars!!").is_err());
        assert!(validate_name("Ação").is_err());
    }
}

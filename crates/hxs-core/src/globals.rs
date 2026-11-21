//! Global settings as the editor's 0x55 reads them: a map of groups (0..3 hold
//! settings, 4 is nil), each a list of `{id: value}` maps in the device's order.
//! The bulk write 0x56 takes the same shape; HX Edit's restore leaves some IDs
//! out ([`NOT_RESTORED`]) and re-encodes the rest from JSON, which turns ints
//! into floats and rounds floats. [`Globals::restore_blob`] instead copies each
//! entry's bytes as the device sent them.

use std::io::Cursor;
use std::ops::Range;

use rmpv::Value;

use crate::{Error, Result, msg};

pub const SETTING_GROUPS: [u64; 4] = [0, 1, 2, 3];

/// The IDs, per group, that HX Edit's restore does not send (captured
/// 2026-11-21, HX Edit restoring a backup onto HXtra v1.2.1 on stock 3.80).
pub const NOT_RESTORED: &[(u64, &[u64])] = &[
    (0, &[150, 151, 152, 155, 157, 159, 160, 161, 162, 163, 164]),
    (1, &[201]),
    (2, &[21, 28, 29, 64, 74, 78, 118, 119, 120, 121, 122, 123, 124, 125, 132]),
];

/// IDs this restore leaves alone, because they follow the current preset rather
/// than the user's settings: the ones a preset file carries in its section 5,
/// which the unit applies with each preset it loads (16 is the preset's tempo,
/// `TempoPreset` in HX Edit's backups). HX Edit's restore does write them.
pub const FOLLOWS_CURRENT_PRESET: &[(u64, &[u64])] = &[(2, &[16, 30, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 134])];

#[derive(Debug, Clone)]
pub struct Entry {
    pub group: u64,
    pub id: u64,
    pub value: Value,
    raw: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Globals {
    bytes: Vec<u8>,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub group: u64,
    pub id: u64,
    pub from: Option<Value>,
    pub to: Value,
}

impl Globals {
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        let mut cur = Cursor::new(&bytes[..]);
        let groups = rmp::decode::read_map_len(&mut cur).map_err(|e| Error::Protocol(format!("globals: not a map ({e})")))?;
        let mut entries = Vec::new();
        for _ in 0..groups {
            let group = rmpv::decode::read_value(&mut cur)?
                .as_u64()
                .ok_or_else(|| Error::Protocol("globals: a group key is not an integer".into()))?;
            let start = cur.position();
            match rmp::decode::read_array_len(&mut cur) {
                Ok(n) => {
                    for _ in 0..n {
                        entries.push(read_entry(&mut cur, group)?);
                    }
                }
                Err(_) => {
                    cur.set_position(start);
                    rmpv::decode::read_value(&mut cur)?;
                }
            }
        }
        Ok(Self { bytes, entries })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, group: u64, id: u64) -> Option<&Value> {
        self.entries.iter().find(|e| e.group == group && e.id == id).map(|e| &e.value)
    }

    /// The entries a restore writes: the setting groups, without nil values, [`NOT_RESTORED`] and [`FOLLOWS_CURRENT_PRESET`].
    pub fn restorable(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| SETTING_GROUPS.contains(&e.group) && !e.value.is_nil() && !excluded(e.group, e.id))
    }

    /// The 0x56 payload: `{0: [...], 1: [...], 2: [...], 3: [...]}` with each entry's original bytes.
    pub fn restore_blob(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len());
        rmp::encode::write_map_len(&mut out, SETTING_GROUPS.len() as u32).expect("writing to a Vec cannot fail");
        for group in SETTING_GROUPS {
            let entries: Vec<&Entry> = self.restorable().filter(|e| e.group == group).collect();
            msg::write_uint(&mut out, group);
            rmp::encode::write_array_len(&mut out, entries.len() as u32).expect("writing to a Vec cannot fail");
            for e in entries {
                out.extend_from_slice(&self.bytes[e.raw.clone()]);
            }
        }
        out
    }

    /// What writing `self`'s restorable entries would change on a device that holds `current`
    /// (a value of another type counts as a change).
    pub fn changes_from(&self, current: &Globals) -> Vec<Change> {
        self.restorable()
            .filter_map(|e| {
                let now = current.get(e.group, e.id);
                (now != Some(&e.value)).then(|| Change { group: e.group, id: e.id, from: now.cloned(), to: e.value.clone() })
            })
            .collect()
    }
}

fn excluded(group: u64, id: u64) -> bool {
    NOT_RESTORED.iter().chain(FOLLOWS_CURRENT_PRESET).any(|(g, ids)| *g == group && ids.contains(&id))
}

fn read_entry(cur: &mut Cursor<&[u8]>, group: u64) -> Result<Entry> {
    let start = cur.position() as usize;
    match rmp::decode::read_map_len(cur) {
        Ok(1) => {}
        Ok(n) => return Err(Error::Protocol(format!("globals group {group}: an entry has {n} keys, expected 1"))),
        Err(e) => return Err(Error::Protocol(format!("globals group {group}: an entry is not a map ({e})"))),
    }
    let id = rmpv::decode::read_value(cur)?
        .as_u64()
        .ok_or_else(|| Error::Protocol(format!("globals group {group}: a setting id is not an integer")))?;
    let value = rmpv::decode::read_value(cur)?;
    Ok(Entry { group, id, value, raw: start..cur.position() as usize })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// `{0: [{150: nil}, {153: 0.03f32}], 1: [{201: 1}], 2: [{15: 112.2497f32}, {28: 10}, {109: 810}], 3: [{0: 1}], 4: nil}`
    fn sample() -> Vec<u8> {
        hex("85
             00 92 81cc96c0 81cc99ca3cf5bc0f
             01 91 81ccc901
             02 93 810fca42e07fd6 811c0a 816dcd032a
             03 91 810001
             04 c0")
    }

    #[test]
    fn parses_groups_and_keeps_types() {
        let g = Globals::parse(sample()).unwrap();
        assert_eq!(g.entries().len(), 7);
        assert_eq!(g.get(2, 109), Some(&Value::from(810)));
        assert!(matches!(g.get(2, 15), Some(Value::F32(_))));
        assert!(g.get(0, 150).unwrap().is_nil());
    }

    #[test]
    fn restore_blob_copies_bytes_and_skips_what_hx_edit_skips() {
        let g = Globals::parse(sample()).unwrap();
        let blob = g.restore_blob();
        assert_eq!(blob, hex("84 00 91 81cc99ca3cf5bc0f 01 90 02 92 810fca42e07fd6 816dcd032a 03 91 810001"));
        let again = Globals::parse(blob).unwrap();
        assert_eq!(again.get(2, 109), Some(&Value::from(810)));
    }

    #[test]
    fn a_type_change_is_a_change() {
        let backup = Globals::parse(sample()).unwrap();
        let mut now = sample();
        let at = now.windows(5).position(|w| w == hex("816dcd032a")).unwrap();
        now.splice(at..at + 5, hex("816dca444a8000"));
        let now = Globals::parse(now).unwrap();
        let changes = backup.changes_from(&now);
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].group, changes[0].id, changes[0].to.clone()), (2, 109, Value::from(810)));
        assert!(backup.changes_from(&backup).is_empty());
    }
}

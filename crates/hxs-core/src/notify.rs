//! Notifications the unit sends on the notify channel:
//! `{0x69: kind, 0x6a: {0x52, 0x44, 0x79, 0x6a: payload}}` for events and
//! `{0x69: kind, 0x6a: {0x66: seq, 0x67: status, 0x68: result}}` for the final
//! reply of a pending request. Kinds were read from captures of HX Edit and
//! of this tool (see docs/findings.md); anything else, or a known kind whose
//! payload does not have the expected shape, is kept as [`Notification::Unknown`].

use rmpv::Value;

use crate::msg;

mod kind {
    pub const LIBRARIAN_COMPLETION: u64 = 1;
    pub const PRESET_LOADED: u64 = 4;
    pub const PRESET_SAVED: u64 = 5;
    pub const PRESET_RENAMED: u64 = 6;
    pub const PRESET_LOADING: u64 = 8;
    pub const EDITOR_COMPLETION: u64 = 0x14;
    pub const SETTING_CHANGED: u64 = 0x16;
    pub const PARAMETER_CHANGED: u64 = 0x1e;
    pub const BLOCK_SELECTED: u64 = 0x27;
    pub const FOOTSWITCH: u64 = 0x29;
    pub const SNAPSHOT_SELECTED: u64 = 0x2a;
    pub const SNAPSHOT_APPLIED: u64 = 0x2e;
    pub const BLOCK_CHANGED: u64 = 0x31;
    pub const FAVORITE_ADDED: u64 = 0x38;
    pub const FAVORITE_DELETED: u64 = 0x3b;
}

mod key {
    pub const KIND: u64 = 0x69;
    pub const PAYLOAD: u64 = 0x6a;
    pub const SETTING_ID: u64 = 0x76;
    pub const SETTING_VALUE: u64 = 0x77;
    pub const SETLIST: u64 = 0x6b;
    pub const SLOT: u64 = 0x6c;
    pub const NAME: u64 = 0x6d;
    pub const BLOCK: u64 = 0x62;
    pub const FAVORITE: u64 = 0x76;
    pub const PATH: u64 = 0x1a;
    pub const PARAMETER: u64 = 0x1c;
    pub const VALUE: u64 = 0x77;
    pub const MOVED_FROM: u64 = 0x4b;
    pub const MOVED_TO: u64 = 0x4c;
    pub const SNAPSHOT: u64 = 0x5c;
    pub const STAMP: u64 = 0x74;
    pub const SWITCH: u64 = 0x46;
    pub const LIT: u64 = 0x3f;
    pub const COLOR: u64 = 0x42;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Notification {
    /// A global setting has a new value (from the unit, HX Edit or this tool).
    SettingChanged { id: u64, value: Value },
    /// The unit starts loading a preset.
    PresetLoading { setlist: u64, slot: u64 },
    /// The unit has finished loading a preset.
    PresetLoaded { setlist: u64, slot: u64 },
    /// A preset was stored (on the unit, by HX Edit or this tool); `stamp` is key 0x74,
    /// a 32-bit value that differs on every save (its meaning is not known).
    PresetSaved { setlist: u64, slot: u64, stamp: u64 },
    PresetRenamed { setlist: u64, slot: u64, name: String },
    BlockSelected { block: u64 },
    /// A block's model or contents changed (also its bypass).
    BlockChanged { block: u64 },
    /// A block moved from one grid position to another.
    BlockMoved { from: u64, to: u64 },
    /// A parameter of a block has a new value (a knob, HX Edit or this tool).
    ParameterChanged { block: u64, path: u64, index: u64, value: Value },
    /// A snapshot was selected: `index` 0..2 within the live bank (HXtra's
    /// bank B reports the same indexes as bank A).
    SnapshotSelected { index: u64 },
    /// Sent after [`Notification::SnapshotSelected`] with the same index.
    SnapshotApplied { index: u64 },
    /// A footswitch's state changed (seen as a block was toggled from one; the
    /// meaning of `lit` and `color` is inferred).
    Footswitch { switch: u64, lit: bool, color: u64 },
    FavoriteAdded { index: u64, name: String },
    FavoriteDeleted { index: u64 },
    /// The final reply to a pending request (`kind` 1 librarian, 0x14 editor).
    Completion { kind: u64, seq: u64, status: u64 },
    Unknown { kind: Option<u64>, note: Value },
}

pub fn decode(note: &Value) -> Notification {
    let unknown = || Notification::Unknown { kind: msg::get(note, key::KIND).and_then(Value::as_u64), note: note.clone() };
    let Some((k, body)) = msg::notification(note) else { return unknown() };
    let payload = match k {
        kind::SNAPSHOT_SELECTED | kind::SNAPSHOT_APPLIED | kind::FOOTSWITCH => Some(body),
        _ => msg::get(body, key::PAYLOAD),
    };
    let int = |k| payload.and_then(|p| msg::get(p, k)).and_then(Value::as_u64);
    let text = |k| payload.and_then(|p| msg::get(p, k)).and_then(msg::text);
    let decoded = match k {
        kind::SETTING_CHANGED => int(key::SETTING_ID)
            .zip(payload.and_then(|p| msg::get(p, key::SETTING_VALUE)))
            .map(|(id, value)| Notification::SettingChanged { id, value: value.clone() }),
        kind::PRESET_LOADING => int(key::SETLIST).zip(int(key::SLOT)).map(|(setlist, slot)| Notification::PresetLoading { setlist, slot }),
        kind::PRESET_LOADED => int(key::SETLIST).zip(int(key::SLOT)).map(|(setlist, slot)| Notification::PresetLoaded { setlist, slot }),
        kind::PRESET_SAVED => int(key::SETLIST)
            .zip(int(key::SLOT))
            .zip(int(key::STAMP))
            .map(|((setlist, slot), stamp)| Notification::PresetSaved { setlist, slot, stamp }),
        kind::PRESET_RENAMED => int(key::SETLIST)
            .zip(int(key::SLOT))
            .zip(text(key::NAME))
            .map(|((setlist, slot), name)| Notification::PresetRenamed { setlist, slot, name }),
        kind::BLOCK_SELECTED => int(key::BLOCK).map(|block| Notification::BlockSelected { block }),
        kind::BLOCK_CHANGED => match (int(key::MOVED_FROM), int(key::MOVED_TO), int(key::BLOCK)) {
            (Some(from), Some(to), _) => Some(Notification::BlockMoved { from, to }),
            (_, _, Some(block)) => Some(Notification::BlockChanged { block }),
            _ => None,
        },
        kind::PARAMETER_CHANGED => int(key::BLOCK)
            .zip(int(key::PATH))
            .zip(int(key::PARAMETER))
            .zip(payload.and_then(|p| msg::get(p, key::VALUE)))
            .map(|(((block, path), index), value)| Notification::ParameterChanged { block, path, index, value: value.clone() }),
        kind::SNAPSHOT_SELECTED => int(key::SNAPSHOT).map(|index| Notification::SnapshotSelected { index }),
        kind::SNAPSHOT_APPLIED => int(key::SNAPSHOT).map(|index| Notification::SnapshotApplied { index }),
        kind::FOOTSWITCH => int(key::SWITCH)
            .zip(payload.and_then(|p| msg::get(p, key::LIT)).and_then(Value::as_bool))
            .zip(int(key::COLOR))
            .map(|((switch, lit), color)| Notification::Footswitch { switch, lit, color }),
        kind::FAVORITE_ADDED => int(key::FAVORITE)
            .map(|index| Notification::FavoriteAdded { index, name: text(key::NAME).unwrap_or_default() }),
        kind::FAVORITE_DELETED => int(key::FAVORITE).map(|index| Notification::FavoriteDeleted { index }),
        kind::LIBRARIAN_COMPLETION | kind::EDITOR_COMPLETION => {
            let field = |k| msg::get(body, k).and_then(Value::as_u64);
            field(msg::key::SEQ).zip(field(msg::key::STATUS)).map(|(seq, status)| Notification::Completion { kind: k, seq, status })
        }
        _ => None,
    };
    decoded.unwrap_or_else(unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::int_map;

    fn event(kind: u64, payload: Value) -> Value {
        let body = int_map(&[(0x52, Value::from(1)), (0x44, Value::from(1)), (0x79, Value::from(5)), (0x6a, payload)]);
        int_map(&[(0x69, Value::from(kind)), (0x6a, body)])
    }

    #[test]
    fn decodes_the_kinds_seen_in_captures() {
        let setting = event(0x16, int_map(&[(0x76, Value::from(0x62)), (0x77, Value::from(2))]));
        assert_eq!(decode(&setting), Notification::SettingChanged { id: 0x62, value: Value::from(2) });
        let loaded = event(4, int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(11))]));
        assert_eq!(decode(&loaded), Notification::PresetLoaded { setlist: 0, slot: 11 });
        let renamed = event(6, int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(9)), (0x6d, Value::from("AC30 Clean HXS2\0"))]));
        assert_eq!(decode(&renamed), Notification::PresetRenamed { setlist: 0, slot: 9, name: "AC30 Clean HXS2".into() });
        let saved = event(5, int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(11)), (0x74, Value::from(4089953967u64))]));
        assert_eq!(decode(&saved), Notification::PresetSaved { setlist: 0, slot: 11, stamp: 4089953967 });
        let fav = event(0x38, int_map(&[(0x76, Value::from(0x11)), (0x6d, Value::from("Minotaur"))]));
        assert_eq!(decode(&fav), Notification::FavoriteAdded { index: 0x11, name: "Minotaur".into() });
        let done = int_map(&[(0x69, Value::from(0x14)), (0x6a, int_map(&[(0x66, Value::from(1001)), (0x67, Value::from(0)), (0x68, Value::Nil)]))]);
        assert_eq!(decode(&done), Notification::Completion { kind: 0x14, seq: 1001, status: 0 });
    }

    #[test]
    fn decodes_the_kinds_seen_with_hxs_monitor() {
        let knob = event(0x1e, int_map(&[(0x62, Value::from(4)), (0x1d, Value::from(true)), (0x1a, Value::from(0)), (0x1c, Value::from(2)), (0x77, Value::F32(0.028))]));
        assert_eq!(decode(&knob), Notification::ParameterChanged { block: 4, path: 0, index: 2, value: Value::F32(0.028) });
        let moved = event(0x31, int_map(&[(0x4b, Value::from(2)), (0x4c, Value::from(3))]));
        assert_eq!(decode(&moved), Notification::BlockMoved { from: 2, to: 3 });
        let changed = event(0x31, int_map(&[(0x62, Value::from(6))]));
        assert_eq!(decode(&changed), Notification::BlockChanged { block: 6 });
        let snap = int_map(&[(0x69, Value::from(0x2a)), (0x6a, int_map(&[(0x5c, Value::from(1))]))]);
        assert_eq!(decode(&snap), Notification::SnapshotSelected { index: 1 });
        let applied = int_map(&[(0x69, Value::from(0x2e)), (0x6a, int_map(&[(0x5c, Value::from(1))]))]);
        assert_eq!(decode(&applied), Notification::SnapshotApplied { index: 1 });
        let fs = int_map(&[(0x69, Value::from(0x29)), (0x6a, int_map(&[(0x46, Value::from(2)), (0x3f, Value::from(true)), (0x42, Value::from(13055))]))]);
        assert_eq!(decode(&fs), Notification::Footswitch { switch: 2, lit: true, color: 13055 });
    }

    #[test]
    fn keeps_what_it_does_not_understand() {
        let odd = event(0x99, Value::from(3));
        assert!(matches!(decode(&odd), Notification::Unknown { kind: Some(0x99), .. }));
        let malformed = event(0x16, Value::from("no id"));
        assert!(matches!(decode(&malformed), Notification::Unknown { kind: Some(0x16), note } if note == malformed));
        assert!(matches!(decode(&Value::from(5)), Notification::Unknown { kind: None, .. }));
    }
}

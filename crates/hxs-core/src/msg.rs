//! Service messages: `{u16 1, u16 service, u32 length}` + a MessagePack map.
//! Requests are `{0x66: seq, 0x64: cmd, 0x65: params}` (params last, as the
//! device reads it); replies are `{0x66: seq, 0x67: status, 0x68: result}`.

use rmpv::Value;

use crate::{Error, Result};

pub mod key {
    pub const SEQ: u64 = 0x66;
    pub const CMD: u64 = 0x64;
    pub const PARAMS: u64 = 0x65;
    pub const STATUS: u64 = 0x67;
    pub const RESULT: u64 = 0x68;
    pub const ERROR: u64 = 0x6f;
}

pub const HEADER_LEN: usize = 8;
pub const STATUS_OK: u64 = 0;
pub const STATUS_PENDING: u64 = 1;

pub fn header(service: u16, len: usize) -> [u8; HEADER_LEN] {
    let mut h = [0; HEADER_LEN];
    h[0..2].copy_from_slice(&1u16.to_le_bytes());
    h[2..4].copy_from_slice(&service.to_le_bytes());
    h[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    h
}

pub fn body_len(header: &[u8]) -> usize {
    u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize
}

pub fn encode_request(seq: u16, cmd: u8, params: Value) -> Vec<u8> {
    encode_request_raw(seq, cmd, &encode_value(&params))
}

/// A request whose params are already MessagePack (for values `rmpv` cannot
/// express, such as a str holding non-UTF-8 bytes).
pub fn encode_request_raw(seq: u16, cmd: u8, params: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(params.len() + 8);
    out.push(0x83);
    write_uint(&mut out, key::SEQ);
    write_uint(&mut out, u64::from(seq));
    write_uint(&mut out, key::CMD);
    write_uint(&mut out, u64::from(cmd));
    write_uint(&mut out, key::PARAMS);
    out.extend_from_slice(params);
    out
}

pub fn write_uint(out: &mut Vec<u8>, v: u64) {
    rmp::encode::write_uint(out, v).expect("writing to a Vec cannot fail");
}

/// A MessagePack str header followed by `bytes` as they are (HX Edit sends
/// preset files this way).
pub fn write_raw_str(out: &mut Vec<u8>, bytes: &[u8]) {
    rmp::encode::write_str_len(out, bytes.len() as u32).expect("writing to a Vec cannot fail");
    out.extend_from_slice(bytes);
}

#[derive(Debug)]
pub struct Reply {
    pub seq: Option<u64>,
    pub status: u64,
    pub result: Value,
}

pub fn decode_reply(cmd: u8, body: &[u8]) -> Result<Reply> {
    let v = rmpv::decode::read_value(&mut &body[..])?;
    let status = get(&v, key::STATUS)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Protocol(format!("reply to {cmd:#04x} has no status")))?;
    Ok(Reply {
        seq: get(&v, key::SEQ).and_then(Value::as_u64),
        status,
        result: get(&v, key::RESULT).cloned().unwrap_or(Value::Nil),
    })
}

const NOTE_KIND: u64 = 0x69;
const NOTE_BODY: u64 = 0x6a;

/// The notification kind that carries the final reply of a pending request,
/// by service: librarian (2) completions come as kind 1, editor (6) ones as
/// kind 0x14 (seen for librarian 0x08 and 0x09, editor 0x14 and 0x56).
pub fn completion_kind(service: u8) -> Option<u64> {
    match service {
        2 => Some(1),
        6 => Some(0x14),
        _ => None,
    }
}

/// A notification's kind (key 0x69) and body (key 0x6a).
pub fn notification(note: &Value) -> Option<(u64, &Value)> {
    Some((get(note, NOTE_KIND)?.as_u64()?, get(note, NOTE_BODY)?))
}

/// A notification `{0x69: kind, 0x6a: {seq, status, result}}` of the given kind: the
/// final reply to a request that was first answered with status 1.
pub fn completion(note: &Value, kind: u64) -> Option<Reply> {
    if get(note, NOTE_KIND)?.as_u64()? != kind {
        return None;
    }
    let body = get(note, NOTE_BODY)?;
    Some(Reply {
        seq: get(body, key::SEQ).and_then(Value::as_u64),
        status: get(body, key::STATUS).and_then(Value::as_u64)?,
        result: get(body, key::RESULT).cloned().unwrap_or(Value::Nil),
    })
}

pub fn error_code(result: &Value) -> Option<i64> {
    get(result, key::ERROR).and_then(Value::as_i64)
}

/// `{k: v, ...}` with integer keys, in the given order.
pub fn int_map(pairs: &[(u64, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (Value::from(*k), v.clone())).collect())
}

pub fn empty_map() -> Value {
    Value::Map(Vec::new())
}

pub fn get(v: &Value, k: u64) -> Option<&Value> {
    v.as_map()?.iter().find(|(key, _)| key.as_u64() == Some(k)).map(|(_, v)| v)
}

/// The raw bytes of a bin or str value (the device sends names and blobs as either).
pub fn bytes(v: &Value) -> Option<&[u8]> {
    match v {
        Value::Binary(b) => Some(b),
        Value::String(s) => Some(s.as_bytes()),
        _ => None,
    }
}

/// A NUL-terminated name as text.
pub fn text(v: &Value) -> Option<String> {
    let b = bytes(v)?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    Some(String::from_utf8_lossy(&b[..end]).into_owned())
}

/// Whether a str anywhere in `v` holds bytes that are not UTF-8 (`rmpv` would write it back as a bin).
pub fn has_invalid_utf8(v: &Value) -> bool {
    match v {
        Value::String(s) => s.as_str().is_none(),
        Value::Array(items) => items.iter().any(has_invalid_utf8),
        Value::Map(pairs) => pairs.iter().any(|(k, v)| has_invalid_utf8(k) || has_invalid_utf8(v)),
        _ => false,
    }
}

pub fn encode_value(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, v).expect("writing to a Vec cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn requests_match_hx_edit() {
        assert_eq!(encode_request(1005, 0xff, empty_map()), hex("8366cd03ed64ccff6580"));
        assert_eq!(encode_request(1002, 0x17, Value::Nil), hex("8366cd03ea641765c0"));
        let p = int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(10)), (0x65, Value::from(2))]);
        assert_eq!(encode_request(1006, 0x04, p), hex("8366cd03ee640465836b006c0a6502"));
    }

    #[test]
    fn reads_a_completion_notification() {
        let note = |kind: u64| int_map(&[(0x69, Value::from(kind)), (0x6a, int_map(&[(0x66, Value::from(1006)), (0x67, Value::from(0)), (0x68, Value::Nil)]))]);
        let librarian = completion_kind(2).unwrap();
        let editor = completion_kind(6).unwrap();
        let r = completion(&note(1), librarian).unwrap();
        assert_eq!((r.seq, r.status), (Some(1006), 0));
        assert!(completion(&note(1), editor).is_none());
        assert!(completion(&note(0x14), editor).is_some());
        assert!(completion(&int_map(&[(0x69, Value::from(0x16))]), librarian).is_none());
        assert_eq!(completion_kind(4), None);
    }

    #[test]
    fn decodes_reply_and_names() {
        let body = hex("8366cd03e86700680a");
        let r = decode_reply(0x16, &body).unwrap();
        assert_eq!((r.seq, r.status, r.result.as_u64()), (Some(1000), 0, Some(10)));
        assert_eq!(text(&Value::Binary(b"AC30 Clean\0".to_vec())).as_deref(), Some("AC30 Clean"));
    }
}

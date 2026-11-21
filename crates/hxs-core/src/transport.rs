//! Transport packets: every bulk transfer carries one or more of them, each an
//! 8-byte header (`type<<28 | version<<26 | length`, `src | dst<<16`, little-endian)
//! followed by `length` bytes and zero padding to a multiple of 4.

use crate::{Error, Result};

const VERSION: u32 = 2;
const TYPE_OTHER: u32 = 1;
const TYPE_SYN: u32 = 2;

pub mod flags {
    pub const SYN: u8 = 0x01;
    pub const FIN: u8 = 0x02;
    pub const DATA: u8 = 0x04;
    pub const WINDOW: u8 = 0x08;
    pub const HEARTBEAT: u8 = 0x10;
    pub const HEARTBEAT_REQUEST: u8 = 0x20;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub src: u16,
    pub dst: u16,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Stream(StreamPacket),
    Other { packet_type: u8, payload: Vec<u8> },
}

/// A stream packet. `mtu` is present only on SYN packets (transport type 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPacket {
    pub flags: u8,
    pub seq: u8,
    pub window: u32,
    pub mtu: Option<u16>,
    pub data: Vec<u8>,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let (packet_type, body) = match &self.body {
            Body::Stream(sp) => encode_stream(sp),
            Body::Other { packet_type, payload } => (u32::from(*packet_type), payload.clone()),
        };
        let mut out = Vec::with_capacity(8 + body.len() + 3);
        let w0 = (packet_type & 0xF) << 28 | VERSION << 26 | (body.len() as u32 & 0xFFFF);
        let w1 = u32::from(self.src) | u32::from(self.dst) << 16;
        out.extend_from_slice(&w0.to_le_bytes());
        out.extend_from_slice(&w1.to_le_bytes());
        out.extend_from_slice(&body);
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    /// Split one IN transfer into its packets.
    pub fn decode_all(buf: &[u8]) -> Result<Vec<Packet>> {
        let mut out = Vec::new();
        let mut off = 0;
        while off + 8 <= buf.len() {
            let w0 = le32(buf, off);
            let w1 = le32(buf, off + 4);
            if w0 == 0 && w1 == 0 {
                break;
            }
            let len = (w0 & 0xFFFF) as usize;
            let end = off + 8 + len;
            if end > buf.len() {
                return Err(Error::Protocol(format!(
                    "truncated transport packet at {off}: needs {len} bytes, {} left",
                    buf.len() - off - 8
                )));
            }
            out.push(Packet {
                src: (w1 & 0xFFFF) as u16,
                dst: (w1 >> 16) as u16,
                body: decode_body(w0 >> 28, &buf[off + 8..end]),
            });
            off = (end).next_multiple_of(4);
        }
        Ok(out)
    }
}

fn encode_stream(sp: &StreamPacket) -> (u32, Vec<u8>) {
    let mut body = Vec::with_capacity(12 + sp.data.len());
    let packet_type = match sp.mtu {
        Some(mtu) => {
            body.extend_from_slice(&(u32::from(mtu) << 16).to_le_bytes());
            TYPE_SYN
        }
        None => TYPE_OTHER,
    };
    let s0 = u32::from(sp.flags) << 24 | u32::from(sp.seq) << 8;
    body.extend_from_slice(&s0.to_le_bytes());
    body.extend_from_slice(&sp.window.to_le_bytes());
    body.extend_from_slice(&sp.data);
    (packet_type, body)
}

fn decode_body(packet_type: u32, body: &[u8]) -> Body {
    let (mtu, rest) = match packet_type {
        TYPE_SYN if body.len() >= 4 => (Some((le32(body, 0) >> 16) as u16), &body[4..]),
        _ => (None, body),
    };
    match rest.len() {
        n if n >= 8 => {
            let s0 = le32(rest, 0);
            Body::Stream(StreamPacket {
                flags: (s0 >> 24) as u8,
                seq: (s0 >> 8) as u8,
                window: le32(rest, 4),
                mtu,
                data: rest[8..].to_vec(),
            })
        }
        _ => Body::Other { packet_type: packet_type as u8, payload: body.to_vec() },
    }
}

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn syn_matches_hx_edit() {
        let p = Packet {
            src: 0x1001,
            dst: 0x03ef,
            body: Body::Stream(StreamPacket {
                flags: flags::SYN | flags::HEARTBEAT_REQUEST,
                seq: 1,
                window: 0x1000,
                mtu: Some(0x200),
                data: vec![],
            }),
        };
        assert_eq!(p.encode(), hex("0c0000280110ef03000000020001002100100000"));
    }

    #[test]
    fn select_service_matches_hx_edit() {
        let p = Packet {
            src: 0x1001,
            dst: 0x03ef,
            body: Body::Stream(StreamPacket {
                flags: flags::DATA,
                seq: 2,
                window: 0x1000,
                mtu: None,
                data: hex("010005000100000005"),
            }),
        };
        assert_eq!(p.encode(), hex("110000180110ef030002000400100000010005000100000005000000"));
    }

    #[test]
    fn decodes_a_device_transfer() {
        let buf = hex("08000018ed03801000280010ae020000");
        let ps = Packet::decode_all(&buf).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!((ps[0].src, ps[0].dst), (0x03ed, 0x1080));
        let Body::Stream(sp) = &ps[0].body else { panic!("not a stream packet") };
        assert_eq!((sp.flags, sp.seq, sp.window), (flags::HEARTBEAT, 0x28, 0x2ae));
    }

    #[test]
    fn round_trips_and_rejects_truncation() {
        let p = Packet {
            src: 0x1000,
            dst: 0x03ed,
            body: Body::Stream(StreamPacket { flags: flags::DATA, seq: 9, window: 0x1234, mtu: None, data: vec![1, 2, 3] }),
        };
        let bytes = p.encode();
        assert_eq!(bytes.len() % 4, 0);
        assert_eq!(Packet::decode_all(&bytes).unwrap(), vec![p]);
        assert!(Packet::decode_all(&bytes[..bytes.len() - 4]).is_err());
    }
}

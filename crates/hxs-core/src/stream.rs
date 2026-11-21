//! One stream between a host port and a device port: 8-bit sequence numbers per
//! direction, a receive window edge (`RX_BUFFER` + bytes consumed) and heartbeats.

use std::time::{Duration, Instant};

use crate::transport::{Body, Packet, StreamPacket, flags};
use crate::{Error, Result};

pub const RX_BUFFER: u32 = 0x1000;
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(1);
const DEFAULT_MTU: usize = 512;
const PACKET_OVERHEAD: usize = 16;

#[derive(Debug)]
pub struct Stream {
    pub local: u16,
    pub remote: u16,
    tx_seq: u8,
    rx_seq: Option<u8>,
    consumed: u32,
    sent: u32,
    peer_window: u32,
    peer_mtu: u16,
    rx: Vec<u8>,
    last_tx: Instant,
    closed_by_peer: bool,
}

impl Stream {
    pub fn new(local: u16, remote: u16) -> Self {
        Self {
            local,
            remote,
            tx_seq: 1,
            rx_seq: None,
            consumed: 0,
            sent: 0,
            peer_window: 0,
            peer_mtu: 0,
            rx: Vec::new(),
            last_tx: Instant::now(),
            closed_by_peer: false,
        }
    }

    pub fn owns(&self, p: &Packet) -> bool {
        (p.src & 0xFF) == (self.remote & 0xFF) && p.dst == self.local
    }

    pub fn is_connected(&self) -> bool {
        self.rx_seq.is_some() && !self.closed_by_peer
    }

    pub fn heartbeat_due(&self, now: Instant) -> bool {
        self.is_connected() && now.duration_since(self.last_tx) >= HEARTBEAT_EVERY
    }

    pub fn can_send(&self, n: usize) -> bool {
        u64::from(self.sent) + n as u64 <= u64::from(self.peer_window)
    }

    pub fn max_chunk(&self) -> usize {
        match self.peer_mtu {
            0 => DEFAULT_MTU - PACKET_OVERHEAD,
            mtu => usize::from(mtu) - PACKET_OVERHEAD,
        }
    }

    pub fn available(&self) -> usize {
        self.rx.len()
    }

    pub fn take(&mut self, n: usize) -> Vec<u8> {
        let rest = self.rx.split_off(n);
        self.consumed = self.consumed.wrapping_add(n as u32);
        std::mem::replace(&mut self.rx, rest)
    }

    pub fn syn(&mut self, mtu: u16) -> Vec<u8> {
        self.packet(flags::SYN | flags::HEARTBEAT_REQUEST, Vec::new(), Some(mtu))
    }

    pub fn data(&mut self, data: &[u8]) -> Vec<u8> {
        self.sent = self.sent.wrapping_add(data.len() as u32);
        self.packet(flags::DATA, data.to_vec(), None)
    }

    pub fn window_update(&mut self) -> Vec<u8> {
        self.packet(flags::WINDOW, Vec::new(), None)
    }

    pub fn heartbeat(&mut self) -> Vec<u8> {
        self.packet(flags::HEARTBEAT, Vec::new(), None)
    }

    pub fn fin(&mut self) -> Vec<u8> {
        self.packet(flags::FIN, Vec::new(), None)
    }

    fn packet(&mut self, flags: u8, data: Vec<u8>, mtu: Option<u16>) -> Vec<u8> {
        let p = Packet {
            src: self.local,
            dst: self.remote & 0x3FF,
            body: Body::Stream(StreamPacket { flags, seq: self.tx_seq, window: RX_BUFFER.wrapping_add(self.consumed), mtu, data }),
        };
        self.tx_seq = self.tx_seq.wrapping_add(1);
        self.last_tx = Instant::now();
        p.encode()
    }

    /// Apply a packet from the device. Errors leave the stream unusable.
    pub fn handle(&mut self, sp: &StreamPacket) -> Result<()> {
        match (self.rx_seq, sp.flags & flags::SYN != 0) {
            (None, true) => {
                self.rx_seq = Some(sp.seq);
                self.peer_mtu = sp.mtu.unwrap_or(0);
                self.peer_window = sp.window;
                Ok(())
            }
            (Some(_), true) => Err(Error::Protocol(format!("unexpected SYN on stream {:#06x}", self.local))),
            (None, false) => Err(Error::Protocol(format!("packet before SYN on stream {:#06x}", self.local))),
            (Some(last), false) => self.handle_connected(last, sp),
        }
    }

    fn handle_connected(&mut self, last: u8, sp: &StreamPacket) -> Result<()> {
        if sp.seq.wrapping_sub(last) != 1 {
            return Err(Error::Protocol(format!(
                "sequence gap on stream {:#06x}: {} after {}",
                self.local, sp.seq, last
            )));
        }
        self.rx_seq = Some(sp.seq);
        if sp.flags & (flags::DATA | flags::WINDOW) != 0 {
            self.peer_window = sp.window;
        }
        if sp.flags & flags::DATA != 0 {
            self.rx.extend_from_slice(&sp.data);
        }
        if sp.flags & flags::FIN != 0 {
            self.closed_by_peer = true;
            return Err(Error::Protocol(format!("the device closed stream {:#06x}", self.local)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(f: u8, seq: u8, window: u32, data: &[u8]) -> StreamPacket {
        StreamPacket { flags: f, seq, window, mtu: (f & flags::SYN != 0).then_some(0x200), data: data.to_vec() }
    }

    #[test]
    fn connects_receives_and_advertises_window() {
        let mut s = Stream::new(0x1001, 0x03ef);
        let _ = s.syn(0x200);
        assert!(!s.is_connected());
        s.handle(&device(flags::SYN, 1, 0x200, &[])).unwrap();
        assert!(s.is_connected());
        assert_eq!(s.max_chunk(), 0x200 - 16);
        s.handle(&device(flags::DATA, 2, 0x209, b"abcdef")).unwrap();
        assert_eq!(s.take(4), b"abcd");
        assert_eq!(s.available(), 2);
        let w = Packet::decode_all(&s.window_update()).unwrap();
        let Body::Stream(sp) = &w[0].body else { panic!() };
        assert_eq!(sp.window, RX_BUFFER + 4);
    }

    #[test]
    fn rejects_gaps_and_fin() {
        let mut s = Stream::new(0x1000, 0x03ed);
        s.handle(&device(flags::SYN, 7, 0x200, &[])).unwrap();
        assert!(s.handle(&device(flags::DATA, 9, 0x200, b"x")).is_err());
        let mut s = Stream::new(0x1000, 0x03ed);
        s.handle(&device(flags::SYN, 255, 0x200, &[])).unwrap();
        s.handle(&device(flags::HEARTBEAT, 0, 0x200, &[])).unwrap();
        assert!(s.handle(&device(flags::FIN, 1, 0x200, &[])).is_err());
        assert!(!s.is_connected());
    }

    #[test]
    fn respects_peer_window() {
        let mut s = Stream::new(0x1001, 0x03ef);
        s.handle(&device(flags::SYN, 1, 10, &[])).unwrap();
        assert!(s.can_send(10));
        let _ = s.data(&[0; 8]);
        assert!(!s.can_send(3));
        s.handle(&device(flags::WINDOW, 2, 20, &[])).unwrap();
        assert!(s.can_send(12));
    }
}

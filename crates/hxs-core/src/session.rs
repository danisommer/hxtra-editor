//! A session multiplexes several service channels over one USB link: every
//! inbound packet goes to the stream it belongs to, every open stream keeps
//! its heartbeat while any of them waits, and the notify channel (when open)
//! is drained on every poll so the device never stalls on it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rmpv::Value;

use crate::link::{Link, UsbLink};
use crate::msg;
use crate::stream::Stream;
use crate::transport::{Body, Packet};
use crate::{Error, Result};

const POLL: Duration = Duration::from_millis(50);
const PURGE: Duration = Duration::from_millis(500);
const CONNECT_ATTEMPTS: usize = 4;
const BODY_CHUNK: usize = 0x400;
const FIRST_SEQ: u16 = 1000;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_DRAIN: Duration = Duration::from_millis(200);
const KEPT_NOTIFICATIONS: usize = 256;

#[derive(Debug, Clone, Copy)]
pub struct Port {
    pub local: u16,
    pub remote: u16,
    pub service: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelId(usize);

struct Channel {
    stream: Stream,
    service: u8,
    seq: u16,
    error: Option<Error>,
    open: bool,
    /// FIN sent, waiting for the device's: still routed, no heartbeat.
    closing: bool,
}

pub struct Session {
    link: Box<dyn Link>,
    channels: Vec<Channel>,
    timeout: Duration,
    notify: Option<ChannelId>,
    notify_buf: Vec<u8>,
    notifications: VecDeque<Value>,
}

impl Session {
    /// The HX Stomp on USB, after discarding what a previous session left queued.
    pub fn open(timeout: Duration) -> Result<Self> {
        let mut link = UsbLink::open()?;
        link.purge(PURGE)?;
        Ok(Self::over(Box::new(link), timeout))
    }

    pub fn over(link: Box<dyn Link>, timeout: Duration) -> Self {
        Self { link, channels: Vec::new(), timeout, notify: None, notify_buf: Vec::new(), notifications: VecDeque::new() }
    }

    /// Keep the session alive while nothing is asked: read what arrived, drain
    /// notify, send due heartbeats. Fails if a channel broke (the device closed it).
    pub fn pump(&mut self) -> Result<()> {
        self.poll()?;
        match self.channels.iter_mut().find_map(|c| c.error.take()) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Every notification received and not consumed as a request's completion, oldest first.
    pub fn take_notifications(&mut self) -> Vec<Value> {
        self.notifications.drain(..).collect()
    }

    pub fn product_id(&self) -> u16 {
        self.link.product_id()
    }

    pub fn open_channel(&mut self, port: Port) -> Result<ChannelId> {
        let id = ChannelId(self.channels.len());
        self.channels.push(Channel {
            stream: Stream::new(port.local, port.remote),
            service: port.service,
            seq: FIRST_SEQ,
            error: None,
            open: true,
            closing: false,
        });
        for attempt in 1..=CONNECT_ATTEMPTS {
            match self.select_service(id, port.service) {
                Ok(()) => return Ok(id),
                Err(e) if attempt == CONNECT_ATTEMPTS => return Err(e),
                Err(_) => self.abandon_stream(id),
            }
        }
        unreachable!("the last attempt returns")
    }

    /// SYN, then the service select and its one-byte echo. Right after another
    /// session closed, the unit was seen to put the start of an older message ahead
    /// of the echo on the new stream; [`Session::open_channel`] then starts over.
    fn select_service(&mut self, id: ChannelId, service: u8) -> Result<()> {
        self.connect(id)?;
        let select = [&msg::header(u16::from(service), 1)[..], &[service]].concat();
        self.send(id, &select)?;
        match self.read_message(id)?.as_slice() {
            [svc] if *svc == service => Ok(()),
            other => Err(Error::Protocol(format!("service {service} select: unexpected reply {other:02x?}"))),
        }
    }

    /// Close a stream whose start went wrong, so the next SYN finds the port free.
    fn abandon_stream(&mut self, id: ChannelId) {
        if !self.channels[id.0].stream.is_connected() {
            return;
        }
        let fin = self.channels[id.0].stream.fin();
        let _ = self.link.write(&fin);
        let end = Instant::now() + CLOSE_DRAIN;
        while Instant::now() < end && !self.channels[id.0].stream.peer_closed() && self.poll().is_ok() {}
        self.channels[id.0].error = None;
    }

    /// Open the channel the device reports asynchronous completions and state changes on.
    pub fn open_notify(&mut self, port: Port) -> Result<ChannelId> {
        let id = self.open_channel(port)?;
        self.notify = Some(id);
        Ok(id)
    }

    /// Send a request and return its result (key 0x68).
    pub fn request(&mut self, id: ChannelId, cmd: u8, params: Value) -> Result<Value> {
        self.request_raw(id, cmd, &msg::encode_value(&params))
    }

    /// As [`Session::request`], with params already encoded as MessagePack.
    pub fn request_raw(&mut self, id: ChannelId, cmd: u8, params: &[u8]) -> Result<Value> {
        let ch = &mut self.channels[id.0];
        let seq = ch.seq;
        ch.seq = ch.seq.wrapping_add(1);
        let body = msg::encode_request_raw(seq, cmd, params);
        let frame = [&msg::header(u16::from(ch.service), body.len())[..], &body].concat();
        self.send(id, &frame)?;
        let reply = msg::decode_reply(cmd, &self.read_message(id)?)?;
        if reply.seq.is_some_and(|s| s != u64::from(seq)) {
            return Err(Error::Protocol(format!("reply to {cmd:#04x} has seq {:?}, expected {seq}", reply.seq)));
        }
        let reply = match reply.status {
            msg::STATUS_PENDING => self.await_completion(id, cmd, seq)?,
            _ => reply,
        };
        match reply.status {
            msg::STATUS_OK => Ok(reply.result),
            status => Err(Error::Refused { cmd, status, code: msg::error_code(&reply.result) }),
        }
    }

    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }

    /// Wait for (and remove) the first notification that satisfies `matches`.
    pub fn wait_notification(&mut self, what: &'static str, timeout: Duration, matches: impl Fn(&Value) -> bool) -> Result<Value> {
        let Some(notify) = self.notify else {
            return Err(Error::Protocol(format!("waiting for {what} needs the notify channel, which is not open")));
        };
        let end = Instant::now() + timeout;
        loop {
            if let Some(i) = self.notifications.iter().position(&matches) {
                return self.notifications.remove(i).ok_or_else(|| Error::Protocol("notification vanished".into()));
            }
            if Instant::now() >= end {
                return Err(Error::Timeout(timeout, what));
            }
            self.poll_for(notify)?;
        }
    }

    /// Send FIN and wait (briefly) for the device's, so it frees the port at once.
    pub fn close_channel(&mut self, id: ChannelId) {
        let ch = &mut self.channels[id.0];
        if !std::mem::replace(&mut ch.open, false) || !ch.stream.is_connected() {
            return;
        }
        ch.closing = true;
        let fin = ch.stream.fin();
        let _ = self.link.write(&fin);
        let end = Instant::now() + CLOSE_DRAIN;
        while Instant::now() < end && !self.channels[id.0].stream.peer_closed() && self.poll().is_ok() {}
        let ch = &mut self.channels[id.0];
        ch.closing = false;
        ch.error = None;
    }

    fn await_completion(&mut self, id: ChannelId, cmd: u8, seq: u16) -> Result<msg::Reply> {
        let Some(notify) = self.notify else {
            return Err(Error::Protocol(format!("command {cmd:#04x} completes on the notify channel, which is not open")));
        };
        let service = self.channels[id.0].service;
        let kind = msg::completion_kind(service)
            .ok_or_else(|| Error::Protocol(format!("command {cmd:#04x}: no known completion notification for service {service}")))?;
        let end = Instant::now() + self.timeout.max(COMPLETION_TIMEOUT);
        loop {
            if let Some(e) = self.channels[notify.0].error.take() {
                return Err(e);
            }
            let found = self.notifications.iter().position(|n| msg::completion(n, kind).is_some_and(|r| r.seq == Some(u64::from(seq))));
            if let Some(reply) = found.and_then(|i| self.notifications.remove(i)).and_then(|n| msg::completion(&n, kind)) {
                return Ok(reply);
            }
            if Instant::now() >= end {
                return Err(Error::Timeout(self.timeout.max(COMPLETION_TIMEOUT), "the completion of a pending command"));
            }
            self.poll_for(id)?;
        }
    }

    fn connect(&mut self, id: ChannelId) -> Result<()> {
        let mtu = self.link.mtu() as u16;
        for attempt in 1..=CONNECT_ATTEMPTS {
            let ch = &mut self.channels[id.0];
            ch.stream = Stream::new(ch.stream.local, ch.stream.remote);
            ch.error = None;
            let syn = ch.stream.syn(mtu);
            self.link.write(&syn)?;
            match self.wait(id, "the device's SYN", |s| s.is_connected()) {
                Ok(()) => return Ok(()),
                Err(e) if attempt == CONNECT_ATTEMPTS => return Err(e),
                Err(_) => continue,
            }
        }
        unreachable!("the last attempt returns")
    }

    fn send(&mut self, id: ChannelId, data: &[u8]) -> Result<()> {
        let max = self.channels[id.0].stream.max_chunk();
        for chunk in data.chunks(max) {
            self.wait(id, "the device's receive window", |s| s.can_send(chunk.len()))?;
            let pkt = self.channels[id.0].stream.data(chunk);
            self.link.write(&pkt)?;
        }
        Ok(())
    }

    fn read_message(&mut self, id: ChannelId) -> Result<Vec<u8>> {
        let header = self.read_exact(id, msg::HEADER_LEN)?;
        let len = msg::body_len(&header);
        let mut body = Vec::with_capacity(len);
        while body.len() < len {
            body.extend(self.read_exact(id, (len - body.len()).min(BODY_CHUNK))?);
            self.window_update(id)?;
        }
        if len == 0 {
            self.window_update(id)?;
        }
        Ok(body)
    }

    fn read_exact(&mut self, id: ChannelId, n: usize) -> Result<Vec<u8>> {
        self.wait(id, "a reply", |s| s.available() >= n)?;
        Ok(self.channels[id.0].stream.take(n))
    }

    fn window_update(&mut self, id: ChannelId) -> Result<()> {
        let pkt = self.channels[id.0].stream.window_update();
        self.link.write(&pkt)
    }

    fn wait(&mut self, id: ChannelId, what: &'static str, ready: impl Fn(&Stream) -> bool) -> Result<()> {
        let end = Instant::now() + self.timeout;
        while !ready(&self.channels[id.0].stream) {
            match Instant::now() < end {
                true => self.poll_for(id)?,
                false => return Err(Error::Timeout(self.timeout, what)),
            }
        }
        Ok(())
    }

    fn poll_for(&mut self, id: ChannelId) -> Result<()> {
        self.poll()?;
        match self.channels[id.0].error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Read one IN transfer, route its packets, drain notify, send due heartbeats.
    fn poll(&mut self) -> Result<()> {
        if let Some(buf) = self.link.read(POLL)? {
            for p in Packet::decode_all(&buf)? {
                self.route(p);
            }
        }
        self.drain_notify()?;
        let now = Instant::now();
        for i in 0..self.channels.len() {
            let ch = &mut self.channels[i];
            if ch.open && ch.stream.heartbeat_due(now) {
                let hb = ch.stream.heartbeat();
                self.link.write(&hb)?;
            }
        }
        Ok(())
    }

    fn drain_notify(&mut self) -> Result<()> {
        let Some(id) = self.notify.filter(|id| self.channels[id.0].open) else { return Ok(()) };
        let n = self.channels[id.0].stream.available();
        if n == 0 {
            return Ok(());
        }
        let bytes = self.channels[id.0].stream.take(n);
        self.notify_buf.extend(bytes);
        self.window_update(id)?;
        while self.notify_buf.len() >= msg::HEADER_LEN {
            let len = msg::body_len(&self.notify_buf);
            if self.notify_buf.len() < msg::HEADER_LEN + len {
                break;
            }
            let body: Vec<u8> = self.notify_buf.drain(..msg::HEADER_LEN + len).skip(msg::HEADER_LEN).collect();
            let note = rmpv::decode::read_value(&mut &body[..]).unwrap_or(Value::Binary(body));
            if self.notifications.len() == KEPT_NOTIFICATIONS {
                self.notifications.pop_front();
            }
            self.notifications.push_back(note);
        }
        Ok(())
    }

    fn route(&mut self, p: Packet) {
        let Body::Stream(sp) = &p.body else { return };
        if let Some(ch) = self.channels.iter_mut().find(|c| (c.open || c.closing) && c.stream.owns(&p)) {
            if let Err(e) = ch.stream.handle(sp) {
                ch.error.get_or_insert(e);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        for i in 0..self.channels.len() {
            self.close_channel(ChannelId(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::int_map;
    use crate::sim::{SimDevice, SimReply};
    use crate::stomp::{EDITOR, LIBRARIAN, NOTIFY};

    const T: Duration = Duration::from_millis(500);

    fn open(sim: &SimDevice) -> (Session, ChannelId, ChannelId) {
        let mut s = Session::over(sim.link(), T);
        let lib = s.open_channel(LIBRARIAN).unwrap();
        let ed = s.open_channel(EDITOR).unwrap();
        s.open_notify(NOTIFY).unwrap();
        (s, lib, ed)
    }

    #[test]
    fn a_request_gets_its_result() {
        let sim = SimDevice::new();
        sim.on(6, 0x17, |_| SimReply::Done(int_map(&[(0x6c, Value::from(10))])));
        let (mut s, _, ed) = open(&sim);
        let r = s.request(ed, 0x17, Value::Nil).unwrap();
        assert_eq!(msg::get(&r, 0x6c).and_then(Value::as_u64), Some(10));
        assert_eq!(sim.requests().last().map(|r| (r.0, r.1)), Some((6, 0x17)));
    }

    #[test]
    fn pending_requests_complete_on_notify_for_each_service() {
        let sim = SimDevice::new();
        sim.on(2, 0x08, |_| SimReply::Pending(Value::Nil));
        sim.on(6, 0x56, |_| SimReply::Pending(Value::from(7)));
        let (mut s, lib, ed) = open(&sim);
        assert!(s.request(lib, 0x08, Value::Nil).unwrap().is_nil());
        assert_eq!(s.request(ed, 0x56, Value::Nil).unwrap().as_u64(), Some(7));
    }

    #[test]
    fn a_refusal_is_an_error_with_its_code() {
        let sim = SimDevice::new();
        sim.on(6, 0x52, |_| SimReply::Refuse { status: 255, code: -45 });
        let (mut s, _, ed) = open(&sim);
        match s.request(ed, 0x52, Value::Nil) {
            Err(Error::Refused { cmd: 0x52, status: 255, code: Some(-45) }) => {}
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(matches!(s.request(ed, 0x99, Value::Nil), Err(Error::Refused { .. })));
    }

    #[test]
    fn a_stale_message_before_the_select_echo_is_survived() {
        let sim = SimDevice::new();
        sim.on(6, 0x17, |_| SimReply::Done(Value::from(1)));
        let before = [&msg::header(4, 23)[..], &[0x82, 0x69]].concat();
        sim.stale_select_once(before, vec![0; 12]);
        let mut s = Session::over(sim.link(), T);
        s.open_notify(NOTIFY).unwrap();
        let ed = s.open_channel(EDITOR).unwrap();
        assert_eq!(s.request(ed, 0x17, Value::Nil).unwrap().as_u64(), Some(1));
    }

    #[test]
    fn a_large_reply_is_reassembled() {
        let sim = SimDevice::new();
        let blob: Vec<u8> = (0..9000u32).map(|i| i as u8).collect();
        let expected = blob.clone();
        sim.on(2, 0x04, move |_| SimReply::Done(Value::Binary(blob.clone())));
        let (mut s, lib, _) = open(&sim);
        assert_eq!(msg::bytes(&s.request(lib, 0x04, Value::Nil).unwrap()), Some(&expected[..]));
    }

    #[test]
    fn pump_collects_notifications() {
        let sim = SimDevice::new();
        let (mut s, _, _) = open(&sim);
        let note = int_map(&[(0x69, Value::from(0x16)), (0x6a, Value::from(1))]);
        assert!(sim.notify(note.clone()));
        let mut got = Vec::new();
        for _ in 0..20 {
            s.pump().unwrap();
            got.extend(s.take_notifications());
            if !got.is_empty() {
                break;
            }
        }
        assert_eq!(got, vec![note]);
    }
}

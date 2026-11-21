//! A session multiplexes several service channels over one USB link: every
//! inbound packet goes to the stream it belongs to, every open stream keeps
//! its heartbeat while any of them waits, and the notify channel (when open)
//! is drained on every poll so the device never stalls on it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rmpv::Value;

use crate::link::UsbLink;
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
}

pub struct Session {
    link: UsbLink,
    channels: Vec<Channel>,
    timeout: Duration,
    notify: Option<ChannelId>,
    notify_buf: Vec<u8>,
    notifications: VecDeque<Value>,
}

impl Session {
    pub fn open(timeout: Duration) -> Result<Self> {
        let mut link = UsbLink::open()?;
        link.purge(PURGE)?;
        Ok(Self { link, channels: Vec::new(), timeout, notify: None, notify_buf: Vec::new(), notifications: VecDeque::new() })
    }

    pub fn product_id(&self) -> u16 {
        self.link.product_id()
    }

    pub fn open_channel(&mut self, port: Port) -> Result<ChannelId> {
        let id = ChannelId(self.channels.len());
        self.channels.push(Channel { stream: Stream::new(port.local, port.remote), service: port.service, seq: FIRST_SEQ, error: None, open: true });
        self.connect(id)?;
        let select = [&msg::header(u16::from(port.service), 1)[..], &[port.service]].concat();
        self.send(id, &select)?;
        let reply = self.read_message(id)?;
        match reply.as_slice() {
            [svc] if *svc == port.service => Ok(id),
            other => Err(Error::Protocol(format!("service {} select: unexpected reply {other:02x?}", port.service))),
        }
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

    pub fn close_channel(&mut self, id: ChannelId) {
        let ch = &mut self.channels[id.0];
        if !std::mem::replace(&mut ch.open, false) || !ch.stream.is_connected() {
            return;
        }
        let fin = ch.stream.fin();
        let _ = self.link.write(&fin);
        let end = Instant::now() + CLOSE_DRAIN;
        while Instant::now() < end && self.poll().is_ok() {}
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
        if let Some(ch) = self.channels.iter_mut().find(|c| c.open && c.stream.owns(&p)) {
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

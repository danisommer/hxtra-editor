//! A simulated HX Stomp for tests and offline work. It speaks the transport,
//! stream and message layers as the unit does, answers each request with what
//! the handler installed for its service and command returns, completes pending
//! requests on the notify channel, and sends notifications on demand. It holds
//! no data of its own: everything it answers comes from the caller.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rmpv::Value;

use crate::link::{Link, PID_HX_STOMP};
use crate::msg::{self, int_map, key};
use crate::transport::{Body, Packet, StreamPacket, flags};
use crate::{Error, Result};

const MTU: u16 = 0x200;
const CHUNK: usize = MTU as usize - 16;
const WINDOW: u32 = 0x10000;
const NOTIFY_SERVICE: u8 = 4;
const UNHANDLED_STATUS: u64 = 255;
const UNHANDLED_CODE: i64 = -1;

/// How the simulated unit answers one request.
#[derive(Debug, Clone)]
pub enum SimReply {
    /// Status 0 with this result.
    Done(Value),
    /// Status 1, then this result as a completion notification.
    Pending(Value),
    /// This status, with `{0x6f: code}` as the result.
    Refuse { status: u64, code: i64 },
}

type Handler = Box<dyn FnMut(&Value) -> SimReply + Send>;

#[derive(Clone, Default)]
pub struct SimDevice {
    shared: Arc<(Mutex<State>, Condvar)>,
}

#[derive(Default)]
struct State {
    streams: Vec<SimStream>,
    outbox: VecDeque<Vec<u8>>,
    handlers: HashMap<(u8, u8), Handler>,
    requests: Vec<(u8, u8, Value)>,
}

struct SimStream {
    host: u16,
    port: u16,
    service: Option<u8>,
    seq: u8,
    rx: Vec<u8>,
    received: u32,
}

impl SimDevice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `cmd` on `service` (6 editor, 2 librarian) with `handler(params)`.
    pub fn on(&self, service: u8, cmd: u8, handler: impl FnMut(&Value) -> SimReply + Send + 'static) {
        self.state().handlers.insert((service, cmd), Box::new(handler));
    }

    /// Every request received so far: `(service, cmd, params)`.
    pub fn requests(&self) -> Vec<(u8, u8, Value)> {
        self.state().requests.clone()
    }

    /// Send a notification on the notify channel; `false` if the host has not opened it.
    pub fn notify(&self, note: Value) -> bool {
        let mut st = self.state();
        let Some(host) = st.streams.iter().find(|s| s.service == Some(NOTIFY_SERVICE)).map(|s| s.host) else {
            return false;
        };
        st.send_message(host, NOTIFY_SERVICE, &msg::encode_value(&note));
        drop(st);
        self.shared.1.notify_all();
        true
    }

    pub fn link(&self) -> Box<dyn Link> {
        Box::new(self.clone())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.0.lock().expect("the simulated device's lock is never poisoned by its own code")
    }
}

impl Link for SimDevice {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        let packets = Packet::decode_all(data)?;
        let mut st = self.state();
        for p in packets {
            st.receive(p)?;
        }
        drop(st);
        self.shared.1.notify_all();
        Ok(())
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let end = Instant::now() + timeout;
        let mut st = self.state();
        loop {
            if let Some(b) = st.outbox.pop_front() {
                return Ok(Some(b));
            }
            let Some(left) = end.checked_duration_since(Instant::now()) else { return Ok(None) };
            st = self.shared.1.wait_timeout(st, left).map_err(|_| Error::Protocol("simulated device lock poisoned".into()))?.0;
        }
    }

    fn mtu(&self) -> usize {
        usize::from(MTU)
    }

    fn product_id(&self) -> u16 {
        PID_HX_STOMP
    }
}

impl State {
    fn receive(&mut self, p: Packet) -> Result<()> {
        let Body::Stream(sp) = p.body else { return Ok(()) };
        let host = p.src;
        if sp.flags & flags::SYN != 0 {
            self.streams.retain(|s| s.host != host);
            self.streams.push(SimStream { host, port: p.dst, service: None, seq: 0, rx: Vec::new(), received: 0 });
            self.send(host, flags::SYN, Vec::new(), Some(MTU));
            return Ok(());
        }
        let Some(s) = self.streams.iter_mut().find(|s| s.host == host) else { return Ok(()) };
        if sp.flags & flags::FIN != 0 {
            self.send(host, flags::FIN, Vec::new(), None);
            self.streams.retain(|s| s.host != host);
            return Ok(());
        }
        if sp.flags & flags::DATA == 0 || sp.data.is_empty() {
            return Ok(());
        }
        s.rx.extend_from_slice(&sp.data);
        s.received = s.received.wrapping_add(sp.data.len() as u32);
        self.send(host, flags::WINDOW, Vec::new(), None);
        self.process(host)
    }

    fn process(&mut self, host: u16) -> Result<()> {
        loop {
            let Some(s) = self.streams.iter_mut().find(|s| s.host == host) else { return Ok(()) };
            if s.rx.len() < msg::HEADER_LEN || s.rx.len() < msg::HEADER_LEN + msg::body_len(&s.rx) {
                return Ok(());
            }
            let len = msg::body_len(&s.rx);
            let body: Vec<u8> = s.rx.drain(..msg::HEADER_LEN + len).skip(msg::HEADER_LEN).collect();
            match (s.service, body.as_slice()) {
                (None, [svc]) => {
                    s.service = Some(*svc);
                    let svc = *svc;
                    self.send_message(host, svc, &[svc]);
                }
                (None, _) => return Err(Error::Protocol("simulated device: a request before the service select".into())),
                (Some(svc), _) => self.answer(host, svc, &body)?,
            }
        }
    }

    fn answer(&mut self, host: u16, service: u8, body: &[u8]) -> Result<()> {
        let req = rmpv::decode::read_value(&mut &body[..])?;
        let seq = msg::get(&req, key::SEQ).and_then(Value::as_u64).unwrap_or(0);
        let cmd = msg::get(&req, key::CMD).and_then(Value::as_u64).unwrap_or(0) as u8;
        let params = msg::get(&req, key::PARAMS).cloned().unwrap_or(Value::Nil);
        self.requests.push((service, cmd, params.clone()));
        let reply = match self.handlers.get_mut(&(service, cmd)) {
            Some(handler) => handler(&params),
            None => SimReply::Refuse { status: UNHANDLED_STATUS, code: UNHANDLED_CODE },
        };
        let (status, result, completion) = match reply {
            SimReply::Done(v) => (msg::STATUS_OK, v, None),
            SimReply::Pending(v) => (msg::STATUS_PENDING, Value::Nil, Some(v)),
            SimReply::Refuse { status, code } => (status, int_map(&[(key::ERROR, Value::from(code))]), None),
        };
        let rep = int_map(&[(key::SEQ, Value::from(seq)), (key::STATUS, Value::from(status)), (key::RESULT, result)]);
        self.send_message(host, service, &msg::encode_value(&rep));
        if let (Some(result), Some(kind)) = (completion, msg::completion_kind(service)) {
            let note = int_map(&[
                (0x69, Value::from(kind)),
                (0x6a, int_map(&[(key::SEQ, Value::from(seq)), (key::STATUS, Value::from(msg::STATUS_OK)), (key::RESULT, result)])),
            ]);
            if let Some(nhost) = self.streams.iter().find(|s| s.service == Some(NOTIFY_SERVICE)).map(|s| s.host) {
                self.send_message(nhost, NOTIFY_SERVICE, &msg::encode_value(&note));
            }
        }
        Ok(())
    }

    fn send_message(&mut self, host: u16, service: u8, body: &[u8]) {
        let frame = [&msg::header(u16::from(service), body.len())[..], body].concat();
        for chunk in frame.chunks(CHUNK) {
            self.send(host, flags::DATA, chunk.to_vec(), None);
        }
    }

    fn send(&mut self, host: u16, flags: u8, data: Vec<u8>, mtu: Option<u16>) {
        let Some(s) = self.streams.iter_mut().find(|s| s.host == host) else { return };
        s.seq = s.seq.wrapping_add(1);
        let p = Packet {
            src: s.port,
            dst: host,
            body: Body::Stream(StreamPacket { flags, seq: s.seq, window: WINDOW.wrapping_add(s.received), mtu, data }),
        };
        self.outbox.push_back(p.encode());
    }
}

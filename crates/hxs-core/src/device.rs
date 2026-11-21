//! A thread that owns the unit's connection. It runs every request from its
//! queue, keeps the session alive in between (heartbeats, notify drained every
//! [`PUMP_EVERY`]), mirrors the unit's state and turns notifications into
//! [`Event`]s. The unit has the last word: on a preset load it reports, the
//! thread reads the current preset and edit buffer again.
//!
//! Requests queued together are taken as one batch, in which only the last
//! [`DeviceHandle::set_setting`] of each setting is sent; the earlier ones
//! report success, as their value was replaced.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use rmpv::Value;

use crate::notify::{self, Notification};
use crate::stomp::{CurrentPreset, PresetEntry, Stomp};
use crate::{Error, Result};

pub const PUMP_EVERY: Duration = Duration::from_millis(50);

/// The unit's state as the device thread knows it.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub product_id: u16,
    pub current: CurrentPreset,
    /// The edit buffer as the unit serializes it (an `l6-helix` preset file).
    pub edit_buffer: Vec<u8>,
    pub presets: Vec<PresetEntry>,
    /// The global settings blob read at connection (editor 0x55).
    pub globals: Vec<u8>,
    /// Settings the unit reported since, by ID (notification 0x16 carries no group).
    pub changed_settings: BTreeMap<u64, Value>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(Snapshot),
    Disconnected { reason: String },
    /// The unit loaded a preset; the thread has read it again.
    PresetLoaded { current: CurrentPreset, edit_buffer: Vec<u8> },
    SettingChanged { id: u64, value: Value },
    PresetRenamed { setlist: u64, slot: u64, name: String },
    /// Any other notification, [`Notification::Unknown`] included.
    Notified(Notification),
    /// Something failed without breaking the connection.
    Problem { message: String },
}

type Reply<T> = Sender<Result<T>>;

/// A request's answer, to wait for or to poll.
pub struct Pending<T>(Receiver<Result<T>>);

impl<T> Pending<T> {
    pub fn wait(self) -> Result<T> {
        self.0.recv().unwrap_or(Err(Error::Stopped))
    }

    /// `None` while the request has not run yet.
    pub fn poll(&self) -> Option<Result<T>> {
        match self.0.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Stopped)),
        }
    }
}

type Job = Box<dyn FnOnce(&mut Stomp) -> Option<String> + Send>;

enum Request {
    Connect(Reply<Snapshot>),
    Disconnect(Reply<()>),
    State(Reply<Snapshot>),
    SelectPreset { setlist: u64, slot: usize, reply: Reply<()> },
    SetSetting { id: u64, value: Value, reply: Reply<()> },
    SetParameter { block: u64, path: u64, index: u64, value: Value, reply: Reply<()> },
    SetBlockEnabled { block: u64, enabled: bool, reply: Reply<()> },
    SelectSnapshot { index: u64, reply: Reply<()> },
    Job { run: Job, fail: Box<dyn FnOnce(Error) + Send> },
    Shutdown,
}

#[derive(Clone)]
pub struct DeviceHandle {
    tx: Sender<Request>,
}

impl DeviceHandle {
    /// Start the device thread; `connect` opens the unit each time a connection is asked for.
    pub fn spawn(connect: impl FnMut() -> Result<Stomp> + Send + 'static) -> (Self, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let (etx, erx) = mpsc::channel();
        let worker = Worker { connect: Box::new(connect), stomp: None, state: None, events: etx };
        thread::Builder::new()
            .name("hx-stomp".into())
            .spawn(move || worker.run(rx))
            .expect("the OS refused to start the device thread");
        (Self { tx }, erx)
    }

    /// The HX Stomp on USB.
    pub fn usb(timeout: Duration) -> (Self, Receiver<Event>) {
        Self::spawn(move || Stomp::connect(timeout))
    }

    pub fn connect(&self) -> Pending<Snapshot> {
        self.ask(Request::Connect)
    }

    pub fn disconnect(&self) -> Pending<()> {
        self.ask(Request::Disconnect)
    }

    pub fn state(&self) -> Pending<Snapshot> {
        self.ask(Request::State)
    }

    pub fn select_preset(&self, setlist: u64, slot: usize) -> Pending<()> {
        self.ask(|reply| Request::SelectPreset { setlist, slot, reply })
    }

    pub fn set_setting(&self, id: u64, value: Value) -> Pending<()> {
        self.ask(|reply| Request::SetSetting { id, value, reply })
    }

    /// Queued changes of the same parameter are sent as one, the last.
    pub fn set_parameter(&self, block: u64, path: u64, index: u64, value: Value) -> Pending<()> {
        self.ask(|reply| Request::SetParameter { block, path, index, value, reply })
    }

    pub fn set_block_enabled(&self, block: u64, enabled: bool) -> Pending<()> {
        self.ask(|reply| Request::SetBlockEnabled { block, enabled, reply })
    }

    pub fn select_snapshot(&self, index: u64) -> Pending<()> {
        self.ask(|reply| Request::SelectSnapshot { index, reply })
    }

    /// Run `f` on the device thread with the connected unit (a backup, a restore...).
    pub fn run<T: Send + 'static>(&self, f: impl FnOnce(&mut Stomp) -> Result<T> + Send + 'static) -> Pending<T> {
        let (tx, rx) = mpsc::channel();
        let fail_tx = tx.clone();
        let run: Job = Box::new(move |stomp| {
            let result = f(stomp);
            let lost = result.as_ref().err().filter(|e| breaks_connection(e)).map(ToString::to_string);
            let _ = tx.send(result);
            lost
        });
        let fail = Box::new(move |e| {
            let _ = fail_tx.send(Err(e));
        });
        let _ = self.tx.send(Request::Job { run, fail });
        Pending(rx)
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Request::Shutdown);
    }

    fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Request) -> Pending<T> {
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(make(tx));
        Pending(rx)
    }
}

struct Worker {
    connect: Box<dyn FnMut() -> Result<Stomp> + Send>,
    stomp: Option<Stomp>,
    state: Option<Snapshot>,
    events: Sender<Event>,
}

impl Worker {
    fn run(mut self, rx: Receiver<Request>) {
        loop {
            let first = match rx.recv_timeout(PUMP_EVERY) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            for request in coalesce(first.into_iter().chain(rx.try_iter()).collect()) {
                if let Request::Shutdown = request {
                    return;
                }
                self.handle(request);
            }
            self.pump();
        }
    }

    fn handle(&mut self, request: Request) {
        match request {
            Request::Connect(reply) => {
                let _ = reply.send(self.open());
            }
            Request::Disconnect(reply) => {
                if self.stomp.is_some() {
                    self.lost("disconnected on request".into());
                }
                let _ = reply.send(Ok(()));
            }
            Request::State(reply) => {
                let _ = reply.send(self.state.clone().ok_or(Error::NotConnected));
            }
            Request::SelectPreset { setlist, slot, reply } => {
                let _ = reply.send(self.with_stomp(|s| s.select_preset(setlist, slot)));
            }
            Request::SetSetting { id, value, reply } => {
                let _ = reply.send(self.with_stomp(|s| s.set_setting(id, value)));
            }
            Request::SetParameter { block, path, index, value, reply } => {
                let _ = reply.send(self.with_stomp(|s| s.set_parameter(block, path, index, value)));
            }
            Request::SetBlockEnabled { block, enabled, reply } => {
                let _ = reply.send(self.with_stomp(|s| s.set_block_enabled(block, enabled)));
            }
            Request::SelectSnapshot { index, reply } => {
                let _ = reply.send(self.with_stomp(|s| s.select_snapshot(index)));
            }
            Request::Job { run, fail } => match self.stomp.as_mut() {
                Some(stomp) => {
                    if let Some(reason) = run(stomp) {
                        self.lost(reason);
                    }
                }
                None => fail(Error::NotConnected),
            },
            Request::Shutdown => {}
        }
    }

    fn open(&mut self) -> Result<Snapshot> {
        if let Some(state) = &self.state {
            return Ok(state.clone());
        }
        let mut stomp = (self.connect)()?;
        let snapshot = read_snapshot(&mut stomp)?;
        self.stomp = Some(stomp);
        self.state = Some(snapshot.clone());
        self.emit(Event::Connected(snapshot.clone()));
        Ok(snapshot)
    }

    fn with_stomp<T>(&mut self, f: impl FnOnce(&mut Stomp) -> Result<T>) -> Result<T> {
        let stomp = self.stomp.as_mut().ok_or(Error::NotConnected)?;
        let result = f(stomp);
        if let Err(e) = &result {
            if breaks_connection(e) {
                self.lost(e.to_string());
            }
        }
        result
    }

    fn pump(&mut self) {
        let Some(stomp) = self.stomp.as_mut() else { return };
        let pumped = stomp.pump();
        let notes = stomp.take_notifications();
        for note in notes {
            self.on_notification(notify::decode(&note));
        }
        if let Err(e) = pumped {
            self.lost(e.to_string());
        }
    }

    fn on_notification(&mut self, n: Notification) {
        match n {
            Notification::SettingChanged { id, value } => {
                if let Some(state) = self.state.as_mut() {
                    state.changed_settings.insert(id, value.clone());
                }
                self.emit(Event::SettingChanged { id, value });
            }
            Notification::PresetLoaded { setlist, slot } => self.refresh_preset(setlist, slot),
            Notification::PresetRenamed { setlist, slot, name } => {
                if let Some(entry) = self
                    .state
                    .as_mut()
                    .filter(|s| s.current.setlist == setlist)
                    .and_then(|s| s.presets.get_mut(slot as usize))
                {
                    entry.name = name.clone();
                }
                self.emit(Event::PresetRenamed { setlist, slot, name });
            }
            Notification::Completion { .. } => {}
            other => self.emit(Event::Notified(other)),
        }
    }

    /// Read the preset the unit reports as loaded. If the unit already holds another
    /// one, a newer load superseded this report and its own report will follow.
    fn refresh_preset(&mut self, setlist: u64, slot: u64) {
        let read = self.with_stomp(|s| Ok((s.current_preset()?, s.edit_buffer()?)));
        match read {
            Ok((current, _)) if (current.setlist, current.slot) != (setlist, slot) => {}
            Ok((current, edit_buffer)) => {
                if let Some(state) = self.state.as_mut() {
                    state.current = current.clone();
                    state.edit_buffer = edit_buffer.clone();
                }
                self.emit(Event::PresetLoaded { current, edit_buffer });
            }
            Err(e) if self.stomp.is_some() => self.emit(Event::Problem { message: format!("reading the loaded preset: {e}") }),
            Err(_) => {}
        }
    }

    fn lost(&mut self, reason: String) {
        self.stomp = None;
        self.state = None;
        self.emit(Event::Disconnected { reason });
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

fn read_snapshot(stomp: &mut Stomp) -> Result<Snapshot> {
    let current = stomp.current_preset()?;
    Ok(Snapshot {
        product_id: stomp.product_id(),
        edit_buffer: stomp.edit_buffer()?,
        presets: stomp.preset_list(current.setlist)?,
        globals: stomp.globals()?,
        current,
        changed_settings: BTreeMap::new(),
    })
}

/// Whether an error means the connection is gone (as opposed to one request failing).
fn breaks_connection(e: &Error) -> bool {
    match e {
        Error::NotFound | Error::Busy | Error::Usb(_) | Error::Transfer(_) | Error::Timeout(..) | Error::Protocol(_) | Error::Decode(_) => true,
        Error::NotConnected
        | Error::Stopped
        | Error::Refused { .. }
        | Error::Preset(_)
        | Error::Backup(_)
        | Error::Io(_)
        | Error::Json(_) => false,
    }
}

/// What a value change writes to: changes with the same target replace each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    Setting(u64),
    Parameter { block: u64, path: u64, index: u64 },
}

impl Request {
    fn target(&self) -> Option<Target> {
        match self {
            Request::SetSetting { id, .. } => Some(Target::Setting(*id)),
            Request::SetParameter { block, path, index, .. } => Some(Target::Parameter { block: *block, path: *path, index: *index }),
            _ => None,
        }
    }

    /// Answer a request that a later one replaced.
    fn superseded(self) {
        match self {
            Request::SetSetting { reply, .. } | Request::SetParameter { reply, .. } => {
                let _ = reply.send(Ok(()));
            }
            _ => unreachable!("only value changes are superseded"),
        }
    }
}

/// Keep only the last change of each setting and parameter; the dropped ones report success.
fn coalesce(batch: Vec<Request>) -> Vec<Request> {
    let last: HashMap<Target, usize> = batch.iter().enumerate().filter_map(|(i, r)| r.target().map(|t| (t, i))).collect();
    batch
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| match r.target() {
            Some(t) if last.get(&t) != Some(&i) => {
                r.superseded();
                None
            }
            _ => Some(r),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;
    use crate::msg::int_map;
    use crate::sim::{SimDevice, SimReply};

    const T: Duration = Duration::from_millis(500);

    fn device(slot: Arc<Mutex<u64>>) -> SimDevice {
        let sim = SimDevice::new();
        sim.on(6, 0x17, move |_| {
            let slot = *slot.lock().unwrap();
            SimReply::Done(int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(slot)), (0x6d, Value::from("Clean\0"))]))
        });
        sim.on(6, 0x16, |_| SimReply::Done(Value::Binary(b"edit buffer".to_vec())));
        sim.on(6, 0x55, |_| SimReply::Done(Value::Binary(b"globals".to_vec())));
        sim.on(6, 0x14, |_| SimReply::Pending(Value::Nil));
        sim.on(6, 0x19, |_| SimReply::Done(Value::Nil));
        sim.on(6, 0x1e, |_| SimReply::Done(Value::Nil));
        sim.on(6, 0x29, |_| SimReply::Done(Value::Nil));
        sim.on(6, 0x58, |_| SimReply::Done(Value::Nil));
        sim.on(2, 0x01, |_| {
            let entry = |id: u64, name: &str| int_map(&[(id, int_map(&[(0x6d, Value::from(name))]))]);
            SimReply::Done(Value::Array(vec![entry(2, "Clean\0"), entry(0, "Empty\0")]))
        });
        sim
    }

    fn start(sim: &SimDevice) -> (DeviceHandle, Receiver<Event>) {
        let link_of = sim.clone();
        DeviceHandle::spawn(move || Stomp::over(link_of.link(), T))
    }

    fn wait_for(events: &Receiver<Event>, what: impl Fn(&Event) -> bool) -> Event {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let left = end.checked_duration_since(Instant::now()).expect("the expected event did not come");
            if let Ok(e) = events.recv_timeout(left) {
                if what(&e) {
                    return e;
                }
            }
        }
    }

    #[test]
    fn connecting_gives_the_whole_state() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, events) = start(&sim);
        let s = handle.connect().wait().unwrap();
        assert_eq!((s.current.slot, s.current.name.as_str()), (0, "Clean"));
        assert_eq!((s.presets.len(), s.presets[1].file_id), (2, 0));
        assert_eq!((s.edit_buffer.as_slice(), s.globals.as_slice()), (&b"edit buffer"[..], &b"globals"[..]));
        assert!(matches!(wait_for(&events, |e| matches!(e, Event::Connected(_))), Event::Connected(_)));
    }

    #[test]
    fn a_preset_load_reported_by_the_unit_refreshes_the_state() {
        let slot = Arc::new(Mutex::new(0));
        let sim = device(slot.clone());
        let (handle, events) = start(&sim);
        handle.connect().wait().unwrap();
        *slot.lock().unwrap() = 5;
        let payload = int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(5))]);
        let body = int_map(&[(0x52, Value::from(1)), (0x44, Value::from(1)), (0x79, Value::from(6)), (0x6a, payload)]);
        assert!(sim.notify(int_map(&[(0x69, Value::from(4)), (0x6a, body)])));
        let e = wait_for(&events, |e| matches!(e, Event::PresetLoaded { .. }));
        assert!(matches!(e, Event::PresetLoaded { current, .. } if current.slot == 5));
        assert_eq!(handle.state().wait().unwrap().current.slot, 5);
    }

    fn loaded(slot: u64) -> Value {
        let payload = int_map(&[(0x6b, Value::from(0)), (0x6c, Value::from(slot))]);
        let body = int_map(&[(0x52, Value::from(1)), (0x44, Value::from(1)), (0x79, Value::from(6)), (0x6a, payload)]);
        int_map(&[(0x69, Value::from(4)), (0x6a, body)])
    }

    #[test]
    fn a_superseded_load_report_is_not_passed_on() {
        let slot = Arc::new(Mutex::new(0));
        let sim = device(slot.clone());
        let (handle, events) = start(&sim);
        handle.connect().wait().unwrap();
        assert!(sim.notify(loaded(5)));
        assert!(sim.notify(loaded(0)));
        let e = wait_for(&events, |e| matches!(e, Event::PresetLoaded { .. }));
        assert!(matches!(e, Event::PresetLoaded { current, .. } if current.slot == 0));
    }

    #[test]
    fn queued_changes_of_one_setting_send_only_the_last() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, _events) = start(&sim);
        handle.connect().wait().unwrap();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let blocker = handle.run(move |_| {
            release_rx.recv().ok();
            Ok(())
        });
        let pending: Vec<_> = (0..30).map(|i| handle.set_setting(15, Value::F32(100.0 + i as f32))).collect();
        release_tx.send(()).unwrap();
        blocker.wait().unwrap();
        for p in pending {
            p.wait().unwrap();
        }
        let sets: Vec<_> = sim.requests().into_iter().filter(|r| (r.0, r.1) == (6, 0x19)).collect();
        assert_eq!(sets.len(), 1);
        assert_eq!(crate::msg::get(&sets[0].2, 0x77), Some(&Value::F32(129.0)));
    }

    #[test]
    fn queued_changes_of_each_parameter_send_only_the_last() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, _events) = start(&sim);
        handle.connect().wait().unwrap();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let blocker = handle.run(move |_| {
            release_rx.recv().ok();
            Ok(())
        });
        let mut pending = Vec::new();
        for i in 0..10 {
            pending.push(handle.set_parameter(4, 0, 2, Value::F32(i as f32 / 10.0)));
            pending.push(handle.set_parameter(4, 0, 3, Value::F32(1.0 - i as f32 / 10.0)));
        }
        pending.push(handle.set_block_enabled(4, false));
        pending.push(handle.select_snapshot(1));
        release_tx.send(()).unwrap();
        blocker.wait().unwrap();
        for p in pending {
            p.wait().unwrap();
        }
        let sent: Vec<_> = sim.requests().into_iter().filter(|r| r.0 == 6 && [0x1e, 0x29, 0x58].contains(&r.1)).collect();
        let field = |v: &Value, k| crate::msg::get(v, k).cloned();
        assert_eq!(sent.len(), 4);
        assert_eq!((field(&sent[0].2, 0x1c), field(&sent[0].2, 0x77)), (Some(Value::from(2)), Some(Value::F32(0.9))));
        assert_eq!((field(&sent[1].2, 0x1c), field(&sent[1].2, 0x77)), (Some(Value::from(3)), Some(Value::F32(1.0 - 0.9))));
        assert_eq!((sent[2].1, field(&sent[2].2, 0x3b)), (0x29, Some(Value::from(false))));
        assert_eq!((sent[3].1, field(&sent[3].2, 0x5c)), (0x58, Some(Value::from(1))));
    }

    #[test]
    fn selecting_a_preset_waits_for_its_completion() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, _events) = start(&sim);
        handle.connect().wait().unwrap();
        handle.select_preset(0, 1).wait().unwrap();
        let last = sim.requests().into_iter().rfind(|r| r.1 == 0x14).unwrap();
        assert_eq!(crate::msg::get(&last.2, 0x6c).and_then(Value::as_u64), Some(1));
    }

    #[test]
    fn an_unplugged_unit_is_reported_and_requests_fail() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, events) = start(&sim);
        handle.connect().wait().unwrap();
        sim.unplug();
        wait_for(&events, |e| matches!(e, Event::Disconnected { .. }));
        assert!(matches!(handle.state().wait(), Err(Error::NotConnected)));
        assert!(matches!(handle.select_preset(0, 1).wait(), Err(Error::NotConnected)));
    }

    #[test]
    fn requests_before_connecting_fail() {
        let sim = device(Arc::new(Mutex::new(0)));
        let (handle, _events) = start(&sim);
        assert!(matches!(handle.run(|_| Ok(())).wait(), Err(Error::NotConnected)));
        handle.shutdown();
        assert!(matches!(handle.state().wait(), Err(Error::Stopped)));
    }
}

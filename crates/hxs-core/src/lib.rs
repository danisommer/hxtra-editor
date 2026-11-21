//! Protocol, preset and backup library for the HX Stomp running HXtra.
//!
//! Layers, bottom up: [`link`] (USB bulk endpoints), [`transport`] (packet
//! framing), [`stream`] (sequence, window, heartbeat), [`session`] (channels
//! multiplexed on one link), [`msg`] (MessagePack requests and replies),
//! [`stomp`] (the editor and librarian commands), [`preset`], [`snapshot`]
//! and [`backup`].

pub mod backup;
pub mod catalog;
pub mod device;
pub mod error;
pub mod globals;
pub mod link;
pub mod midi;
pub mod msg;
pub mod notify;
pub mod preset;
pub mod restore;
pub mod session;
pub mod sim;
pub mod snapshot;
pub mod stomp;
pub mod stream;
pub mod transport;

pub use error::{Error, Result};

/// Whether `HXS_TRACE` is set: connection steps and requests then go to stderr.
pub fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("HXS_TRACE").is_some())
}

/// Print a timestamped line to stderr when [`tracing`] is on.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::tracing() {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            eprintln!("[hxs {}.{:03}] {}", t.as_secs() % 100_000, t.subsec_millis(), format_args!($($arg)*));
        }
    };
}

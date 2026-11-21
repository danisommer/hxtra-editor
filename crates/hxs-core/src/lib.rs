//! Protocol, preset and backup library for the HX Stomp running HXtra.
//!
//! Layers, bottom up: [`link`] (USB bulk endpoints), [`transport`] (packet
//! framing), [`stream`] (sequence, window, heartbeat), [`session`] (channels
//! multiplexed on one link), [`msg`] (MessagePack requests and replies),
//! [`stomp`] (the editor and librarian commands), [`preset`], [`snapshot`]
//! and [`backup`].

pub mod backup;
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

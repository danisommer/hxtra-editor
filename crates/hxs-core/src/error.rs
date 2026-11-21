use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no HX Stomp found on USB (is it on, connected and running its normal firmware?)")]
    NotFound,
    #[error("the HX Stomp's editor interface is in use: close HX Edit and Line 6 Updater, then retry")]
    Busy,
    #[error("not connected to the HX Stomp")]
    NotConnected,
    #[error("the device thread has stopped")]
    Stopped,
    #[error("MIDI: {0}")]
    Midi(String),
    #[error("USB: {0}")]
    Usb(nusb::Error),
    #[error("USB transfer: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),
    #[error("timed out after {0:?} waiting for {1}")]
    Timeout(Duration, &'static str),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("the device refused command {cmd:#04x} (status {status}, error {code:?})")]
    Refused { cmd: u8, status: u64, code: Option<i64> },
    #[error("preset file: {0}")]
    Preset(String),
    #[error("backup: {0}")]
    Backup(String),
    #[error("MessagePack: {0}")]
    Decode(#[from] rmpv::decode::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

const WINDOWS_ERROR_ACCESS_DENIED: u32 = 5;

impl From<nusb::Error> for Error {
    fn from(e: nusb::Error) -> Self {
        match (e.kind(), e.os_error()) {
            (nusb::ErrorKind::Busy, _) => Error::Busy,
            (_, Some(WINDOWS_ERROR_ACCESS_DENIED)) if cfg!(windows) => Error::Busy,
            _ => Error::Usb(e),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

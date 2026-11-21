//! The USB link: interface 0 of the HX Stomp (vendor class, bound to WinUSB on
//! Windows), one bulk OUT and one bulk IN endpoint.

use std::time::{Duration, Instant};

use nusb::MaybeFuture;
use nusb::descriptors::TransferType;
use nusb::transfer::{Buffer, Bulk, Direction, In, Out};

use crate::{Error, Result};

pub const VID_LINE6: u16 = 0x0E41;
pub const PID_HX_STOMP: u16 = 0x4246;
pub const PID_HX_STOMP_XL: u16 = 0x4253;

const INTERFACE: u8 = 0;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(2);
const IN_PACKETS_PER_TRANSFER: usize = 8;
const IN_TRANSFERS: usize = 2;

/// What a [`crate::session::Session`] needs from the wire: the USB device, or a
/// simulated one in tests.
pub trait Link: Send {
    fn write(&mut self, data: &[u8]) -> Result<()>;

    /// One IN transfer, or `None` if nothing arrived within `timeout`.
    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;

    fn mtu(&self) -> usize;

    fn product_id(&self) -> u16;

    /// Discard whatever a previous session left in the device's IN queue.
    fn purge(&mut self, duration: Duration) -> Result<usize> {
        let end = Instant::now() + duration;
        let mut n = 0;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            n += self.read(left.min(Duration::from_millis(100)))?.map_or(0, |b| b.len());
        }
        Ok(n)
    }
}

pub struct UsbLink {
    ep_out: nusb::Endpoint<Bulk, Out>,
    ep_in: nusb::Endpoint<Bulk, In>,
    mtu: usize,
    product_id: u16,
}

impl UsbLink {
    pub fn open() -> Result<Self> {
        let info = nusb::list_devices()
            .wait()?
            .find(|d| d.vendor_id() == VID_LINE6 && matches!(d.product_id(), PID_HX_STOMP | PID_HX_STOMP_XL))
            .ok_or(Error::NotFound)?;
        let product_id = info.product_id();
        let device = info.open().wait()?;
        let iface = device.claim_interface(INTERFACE).wait()?;
        let (addr_out, addr_in, mtu) = bulk_endpoints(&iface)?;
        Ok(Self {
            ep_out: iface.endpoint::<Bulk, Out>(addr_out)?,
            ep_in: iface.endpoint::<Bulk, In>(addr_in)?,
            mtu,
            product_id,
        })
    }

}

impl Link for UsbLink {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn product_id(&self) -> u16 {
        self.product_id
    }

    /// Unlike `transfer_blocking`, which waits without end for a cancelled transfer
    /// to come back, this gives up after [`CANCEL_TIMEOUT`] and reports it.
    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.ep_out.submit(Buffer::from(data.to_vec()));
        let c = match self.ep_out.wait_next_complete(WRITE_TIMEOUT) {
            Some(c) => c,
            None => {
                self.ep_out.cancel_all();
                self.ep_out.wait_next_complete(CANCEL_TIMEOUT).ok_or_else(|| {
                    Error::Protocol(format!(
                        "the unit took no data for {WRITE_TIMEOUT:?} and the transfer did not cancel; if it stays unresponsive, power-cycle it"
                    ))
                })?
            }
        };
        c.status?;
        match c.actual_len == data.len() {
            true => Ok(()),
            false => Err(Error::Protocol(format!("short bulk OUT: {} of {} bytes", c.actual_len, data.len()))),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        while self.ep_in.pending() < IN_TRANSFERS {
            let buf = self.ep_in.allocate(self.mtu * IN_PACKETS_PER_TRANSFER);
            self.ep_in.submit(buf);
        }
        match self.ep_in.wait_next_complete(timeout) {
            None => Ok(None),
            Some(c) => {
                c.status?;
                let n = c.actual_len.min(c.buffer.len());
                Ok(Some(c.buffer[..n].to_vec()))
            }
        }
    }
}

impl Drop for UsbLink {
    fn drop(&mut self) {
        self.ep_in.cancel_all();
        while self.ep_in.pending() > 0 && self.ep_in.wait_next_complete(Duration::from_millis(500)).is_some() {}
    }
}

fn bulk_endpoints(iface: &nusb::Interface) -> Result<(u8, u8, usize)> {
    let desc = iface
        .descriptor()
        .ok_or_else(|| Error::Protocol("interface 0 has no descriptor".into()))?;
    let bulk: Vec<_> = desc.endpoints().filter(|e| e.transfer_type() == TransferType::Bulk).collect();
    let out = bulk.iter().find(|e| e.direction() == Direction::Out);
    let inp = bulk.iter().find(|e| e.direction() == Direction::In);
    match (out, inp) {
        (Some(o), Some(i)) => Ok((o.address(), i.address(), i.max_packet_size())),
        _ => Err(Error::Protocol("interface 0 lacks a bulk IN/OUT endpoint pair".into())),
    }
}

//! The HX Stomp's MIDI port, for what the USB editor channel cannot do: HXtra
//! recalls snapshots 4..6 on MIDI CC 69 values 3..5, and the editor channel
//! only reaches the live bank's 1..3 (see docs/findings.md).

use midir::{MidiOutput, MidiOutputConnection};

use crate::{Error, Result};

/// The output port is the first whose name contains this.
pub const PORT_NAME: &str = "HX Stomp";
pub const SNAPSHOT_CC: u8 = 69;
/// MIDI channel 1, the unit's default base channel.
pub const DEFAULT_CHANNEL: u8 = 0;
pub const SNAPSHOTS: u8 = 6;

pub struct MidiPort {
    conn: MidiOutputConnection,
    channel: u8,
    name: String,
}

impl MidiPort {
    /// `channel` is 0..15 (MIDI channels 1..16).
    pub fn open(channel: u8) -> Result<Self> {
        if channel > 15 {
            return Err(Error::Midi(format!("MIDI channel {} does not exist (1..16)", u16::from(channel) + 1)));
        }
        let out = MidiOutput::new("hxs").map_err(|e| Error::Midi(e.to_string()))?;
        let ports = out.ports();
        let (port, name) = ports
            .iter()
            .find_map(|p| out.port_name(p).ok().filter(|n| n.contains(PORT_NAME)).map(|n| (p, n)))
            .ok_or_else(|| Error::Midi(format!("no MIDI output named like \"{PORT_NAME}\"")))?;
        let conn = out.connect(port, "hxs").map_err(|e| Error::Midi(e.to_string()))?;
        Ok(Self { conn, channel, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Recall snapshot `number`, 1..6 (4..6 are HXtra's bank B).
    pub fn recall_snapshot(&mut self, number: u8) -> Result<()> {
        match number {
            1..=SNAPSHOTS => self.conn.send(&control_change(self.channel, SNAPSHOT_CC, number - 1)).map_err(|e| Error::Midi(e.to_string())),
            n => Err(Error::Midi(format!("snapshots are 1..{SNAPSHOTS}, not {n}"))),
        }
    }
}

pub fn control_change(channel: u8, control: u8, value: u8) -> [u8; 3] {
    [0xB0 | (channel & 0x0F), control & 0x7F, value & 0x7F]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_four_is_cc_69_value_3_on_channel_1() {
        assert_eq!(control_change(DEFAULT_CHANNEL, SNAPSHOT_CC, 4 - 1), [0xB0, 69, 3]);
        assert_eq!(control_change(15, SNAPSHOT_CC, 5), [0xBF, 69, 5]);
    }
}

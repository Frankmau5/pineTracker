use alsa::{Direction, Rawmidi};
use std::ffi::CString;

pub const MAX_MIDI_EVENTS_PER_BLOCK: usize = 256;

pub const MIDI_TIMING_CLOCK: u8 = 0xF8;
pub const MIDI_START: u8 = 0xFA;
pub const MIDI_CONTINUE: u8 = 0xFB;
pub const MIDI_STOP: u8 = 0xFC;

/// Fixed-size zero-allocation MIDI packet (1 to 3 bytes) with sample offset within the 128-frame block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MidiPacket {
    pub sample_offset: u8,
    pub len: u8,
    pub bytes: [u8; 3],
}

impl Default for MidiPacket {
    fn default() -> Self {
        Self {
            sample_offset: 0,
            len: 0,
            bytes: [0; 3],
        }
    }
}

impl MidiPacket {
    #[inline]
    pub const fn realtime(sample_offset: u8, status: u8) -> Self {
        Self {
            sample_offset,
            len: 1,
            bytes: [status, 0, 0],
        }
    }

    #[inline]
    pub const fn note_on(sample_offset: u8, channel: u8, note: u8, velocity: u8) -> Self {
        Self {
            sample_offset,
            len: 3,
            bytes: [0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F],
        }
    }

    #[inline]
    pub const fn note_off(sample_offset: u8, channel: u8, note: u8) -> Self {
        Self {
            sample_offset,
            len: 3,
            bytes: [0x80 | (channel & 0x0F), note & 0x7F, 0x00],
        }
    }

    #[inline]
    pub const fn control_change(sample_offset: u8, channel: u8, cc: u8, value: u8) -> Self {
        Self {
            sample_offset,
            len: 3,
            bytes: [0xB0 | (channel & 0x0F), cc & 0x7F, value & 0x7F],
        }
    }

    #[inline]
    pub const fn program_change(sample_offset: u8, channel: u8, program: u8) -> Self {
        Self {
            sample_offset,
            len: 2,
            bytes: [0xC0 | (channel & 0x0F), program & 0x7F, 0],
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..(self.len as usize).min(3)]
    }
}

/// Zero-allocation fixed-capacity queue used by the sequencer during each 128-sample callback.
#[derive(Clone, Debug)]
pub struct MidiOutQueue {
    events: [MidiPacket; MAX_MIDI_EVENTS_PER_BLOCK],
    len: usize,
}

impl Default for MidiOutQueue {
    fn default() -> Self {
        Self {
            events: [MidiPacket::default(); MAX_MIDI_EVENTS_PER_BLOCK],
            len: 0,
        }
    }
}

impl MidiOutQueue {
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn push(&mut self, pkt: MidiPacket) -> bool {
        if self.len < MAX_MIDI_EVENTS_PER_BLOCK {
            self.events[self.len] = pkt;
            self.len += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[MidiPacket] {
        &self.events[..self.len]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Non-blocking ALSA RawMIDI hardware driver for 5-pin DIN MIDI output via USB-C hub.
pub struct AlsaMidiDriver {
    rawmidi: Option<Rawmidi>,
    port_name: [u8; 32],
    reconnect_cooldown: u32,
}

impl Default for AlsaMidiDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AlsaMidiDriver {
    pub fn new() -> Self {
        let mut drv = Self {
            rawmidi: None,
            port_name: [b' '; 32],
            reconnect_cooldown: 0,
        };
        drv.try_open_any();
        drv
    }

    /// Probes `/dev/snd/midiC*D*` ALSA RawMIDI hardware ports (`hw:1,0`, `hw:2,0`, `hw:3,0`, `hw:0,0`) in non-blocking mode.
    pub fn try_open_any(&mut self) -> bool {
        const CANDIDATES: [(&str, &str); 4] = [
            ("/dev/snd/midiC1D0", "hw:1,0"),
            ("/dev/snd/midiC2D0", "hw:2,0"),
            ("/dev/snd/midiC3D0", "hw:3,0"),
            ("/dev/snd/midiC0D0", "hw:0,0"),
        ];
        for &(dev_node, name) in &CANDIDATES {
            if !std::path::Path::new(dev_node).exists() {
                continue;
            }
            if let Ok(c_name) = CString::new(name) {
                if let Ok(rm) = Rawmidi::open(&c_name, Direction::Playback, true) {
                    self.port_name = [b' '; 32];
                    for (i, b) in name.bytes().take(32).enumerate() {
                        self.port_name[i] = b;
                    }
                    self.rawmidi = Some(rm);
                    return true;
                }
            }
        }
        false
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.rawmidi.is_some()
    }

    /// Flushes all queued `MidiPacket`s for the current 128-frame audio block without blocking or allocating.
    pub fn flush_queue(&mut self, queue: &MidiOutQueue) {
        if queue.is_empty() {
            return;
        }

        if self.rawmidi.is_none() {
            self.reconnect_cooldown = self.reconnect_cooldown.wrapping_add(1);
            if self.reconnect_cooldown % 512 == 0 {
                self.try_open_any();
            }
            return;
        }

        let mut disconnected = false;
        if let Some(ref rm) = self.rawmidi {
            let mut io = rm.io();
            for pkt in queue.as_slice() {
                use std::io::Write;
                if let Err(err) = io.write(pkt.as_slice()) {
                    // ENODEV (19) on USB-C MIDI interface unplug
                    if err.raw_os_error() == Some(libc::ENODEV) {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if disconnected {
            self.rawmidi = None;
            self.reconnect_cooldown = 1;
        }
    }
}

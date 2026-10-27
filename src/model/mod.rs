use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Core Tracker Constants (Standard M8 Dimensions)
// ============================================================================

pub const NUM_TRACKS: usize = 8;
pub const SONG_ROWS: usize = 256;
pub const NUM_CHAINS: usize = 256;
pub const CHAIN_STEPS: usize = 16;
pub const NUM_PHRASES: usize = 256;
pub const PHRASE_STEPS: usize = 16;
pub const FX_COLS: usize = 3;
pub const NUM_INSTRUMENTS: usize = 128;
pub const NUM_TABLES: usize = 128;
pub const TABLE_STEPS: usize = 16;
pub const NUM_GROOVES: usize = 32;
pub const GROOVE_STEPS: usize = 16;

pub const DRUM_PADS: usize = 12;
pub const VEL_LAYERS: usize = 3;
pub const DRUM_KIT_VOICES: usize = 8;
pub const MIDI_CC_SLOTS: usize = 6;

pub const EMPTY_U8: u8 = 0xFF;
pub const NOTE_OFF: u8 = 0xFE;

/// `C-4` is mapped to note 48 (`48 / 12 == 4`, `48 % 12 == 0`), so `C-4`..`B-4` = `48..=59`.
pub const DRUM_BASE_NOTE: u8 = 48;

/// Closed Hat (F-4, pad 5) and Pedal Hat (F#4, pad 6) choke Open Hat (G#4, pad 8).
pub const PAD_CLOSED_HAT: usize = 5;
pub const PAD_PEDAL_HAT: usize = 6;
pub const PAD_OPEN_HAT: usize = 8;

pub const DRUM_PAD_NAMES: [&str; DRUM_PADS] = [
    "KICK", "RIM ", "SNAR", "CLAP", "LTOM", "CHAT", "PHAT", "MTOM", "OHAT", "HTOM", "CRSH", "RIDE",
];

// ============================================================================
// Zero-Allocation Fixed ASCII String
// ============================================================================

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct FixedStr<const N: usize>(pub [u8; N]);

impl<const N: usize> FixedStr<N> {
    pub const fn empty() -> Self {
        Self([b' '; N])
    }

    pub fn from_str_lossy(s: &str) -> Self {
        let mut buf = [b' '; N];
        for (i, b) in s.bytes().take(N).enumerate() {
            buf[i] = if b.is_ascii_graphic() || b == b' ' {
                b
            } else {
                b'?'
            };
        }
        Self(buf)
    }

    pub fn as_str(&self) -> &str {
        let trimmed_len = self
            .0
            .iter()
            .rposition(|&b| b != b' ' && b != 0)
            .map(|idx| idx + 1)
            .unwrap_or(0);
        std::str::from_utf8(&self.0[..trimmed_len]).unwrap_or("")
    }
}

impl<const N: usize> Serialize for FixedStr<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de, const N: usize> Deserialize<'de> for FixedStr<N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str_lossy(&s))
    }
}

impl<const N: usize> Default for FixedStr<N> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const N: usize> fmt::Debug for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl<const N: usize> fmt::Display for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Note & Pitch Helpers
// ============================================================================

const NOTE_NAMES: [&str; 12] = [
    "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
];

/// Format a tracker note byte (`0x00..=0x7F`, `NOTE_OFF=0xFE`, `EMPTY_U8=0xFF`) into a 3-char string.
pub fn format_note(note: u8) -> [u8; 3] {
    match note {
        EMPTY_U8 => *b"---",
        NOTE_OFF => *b"OFF",
        n if n < 120 => {
            let octave = n / 12;
            let semitone = (n % 12) as usize;
            let prefix = NOTE_NAMES[semitone].as_bytes();
            [prefix[0], prefix[1], b'0' + octave]
        }
        _ => *b"???",
    }
}

/// Convert a tracker note (`C-4` = 48, `A-4` = 57) + fractional semitone offset to Hz (A-4 = 440 Hz).
#[inline]
pub fn note_to_freq_hz(note: f32) -> f32 {
    440.0 * 2.0_f32.powf((note - 57.0) / 12.0)
}

/// Map a chromatic note (`C-4`..`B-4` = 48..=59, or wrapped octave) to a DrumKit pad index `0..12`.
#[inline]
pub fn note_to_drum_pad(note: u8) -> Option<usize> {
    if note >= 120 {
        return None;
    }
    if (DRUM_BASE_NOTE..DRUM_BASE_NOTE + DRUM_PADS as u8).contains(&note) {
        Some((note - DRUM_BASE_NOTE) as usize)
    } else {
        Some((note % 12) as usize)
    }
}

/// Map a tracker velocity/volume byte (`0x00..=0xFF`) to one of the 3 velocity layers (`0..=2`).
#[inline]
pub fn velocity_to_layer(vel: u8) -> usize {
    match vel {
        0x00..=0x54 => 0, // Soft
        0x55..=0xAA => 1, // Medium
        0xAB..=0xFF => 2, // Hard
    }
}

// ============================================================================
// Tracker FX Commands
// ============================================================================

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FxCommand {
    None = 0,
    Arp, // ARP: Arpeggio (xy semitones)
    Del, // DEL: Note delay in 192-PPQN sub-ticks
    Ret, // RET: Retrigger (x=vol delta, y=sub-tick interval)
    Cut, // CUT: Cut note after xx 24-PPQN ticks
    Kil, // KIL: Kill voice after xx 192-PPQN sub-ticks
    Hop, // HOP: Jump to step xx in phrase
    Grv, // GRV: Select Groove 00..1F
    Tpo, // TPO: Set Tempo BPM
    Vol, // VOL: Volume set/slide
    Pit, // PIT: Pitch offset (signed i8 semitones)
    Sli, // SLI: Glide / Portamento speed
    Fcu, // FCU: Filter Cutoff (00..FF)
    Frs, // FRS: Filter Resonance (00..FF)
    Pan, // PAN: Stereo Pan (00=L, 80=C, FF=R)
    Ofs, // OFS: Sample Slice / Start Offset (00..FF)
    Cca, // CCA: Mapped MIDI CC Slot A (0)
    Ccb, // CCB: Mapped MIDI CC Slot B (1)
    Ccc, // CCC: Mapped MIDI CC Slot C (2)
    Ccd, // CCD: Mapped MIDI CC Slot D (3)
    Cce, // CCE: Mapped MIDI CC Slot E (4)
    Ccf, // CCF: Mapped MIDI CC Slot F (5)
    Pgm, // PGM: MIDI Program Change (00..7F)
}

impl FxCommand {
    pub const ALL: [FxCommand; 23] = [
        FxCommand::None,
        FxCommand::Arp,
        FxCommand::Del,
        FxCommand::Ret,
        FxCommand::Cut,
        FxCommand::Kil,
        FxCommand::Hop,
        FxCommand::Grv,
        FxCommand::Tpo,
        FxCommand::Vol,
        FxCommand::Pit,
        FxCommand::Sli,
        FxCommand::Fcu,
        FxCommand::Frs,
        FxCommand::Pan,
        FxCommand::Ofs,
        FxCommand::Cca,
        FxCommand::Ccb,
        FxCommand::Ccc,
        FxCommand::Ccd,
        FxCommand::Cce,
        FxCommand::Ccf,
        FxCommand::Pgm,
    ];

    pub fn code(self) -> &'static str {
        match self {
            FxCommand::None => "---",
            FxCommand::Arp => "ARP",
            FxCommand::Del => "DEL",
            FxCommand::Ret => "RET",
            FxCommand::Cut => "CUT",
            FxCommand::Kil => "KIL",
            FxCommand::Hop => "HOP",
            FxCommand::Grv => "GRV",
            FxCommand::Tpo => "TPO",
            FxCommand::Vol => "VOL",
            FxCommand::Pit => "PIT",
            FxCommand::Sli => "SLI",
            FxCommand::Fcu => "FCU",
            FxCommand::Frs => "FRS",
            FxCommand::Pan => "PAN",
            FxCommand::Ofs => "OFS",
            FxCommand::Cca => "CCA",
            FxCommand::Ccb => "CCB",
            FxCommand::Ccc => "CCC",
            FxCommand::Ccd => "CCD",
            FxCommand::Cce => "CCE",
            FxCommand::Ccf => "CCF",
            FxCommand::Pgm => "PGM",
        }
    }

    pub fn next(self, delta: i32) -> Self {
        let len = Self::ALL.len() as i32;
        let idx = (self as u8 as i32 + delta).rem_euclid(len) as usize;
        Self::ALL[idx]
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FxSlot {
    pub command: FxCommand,
    pub value: u8,
}

impl Default for FxSlot {
    fn default() -> Self {
        Self {
            command: FxCommand::None,
            value: 0x00,
        }
    }
}

// ============================================================================
// Song, Chain, Phrase, Table, Groove Structures
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainRow {
    pub phrase: u8,    // 0x00..=0xFE or EMPTY_U8 (0xFF)
    pub transpose: i8, // -48..=+48 semitones
}

impl Default for ChainRow {
    fn default() -> Self {
        Self {
            phrase: EMPTY_U8,
            transpose: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain {
    pub rows: [ChainRow; CHAIN_STEPS],
}

impl Default for Chain {
    fn default() -> Self {
        Self {
            rows: [ChainRow::default(); CHAIN_STEPS],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhraseStep {
    pub note: u8,       // 0x00..=0x77, NOTE_OFF (0xFE), or EMPTY_U8 (0xFF)
    pub volume: u8,     // 0x00..=0xFF (or EMPTY_U8 for instrument default)
    pub instrument: u8, // 0x00..=0x7F or EMPTY_U8
    pub fx: [FxSlot; FX_COLS],
}

impl Default for PhraseStep {
    fn default() -> Self {
        Self {
            note: EMPTY_U8,
            volume: EMPTY_U8,
            instrument: EMPTY_U8,
            fx: [FxSlot::default(); FX_COLS],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phrase {
    pub steps: [PhraseStep; PHRASE_STEPS],
}

impl Default for Phrase {
    fn default() -> Self {
        Self {
            steps: [PhraseStep::default(); PHRASE_STEPS],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableStep {
    pub transpose: i8,
    pub volume: u8, // EMPTY_U8 = unchanged
    pub fx: [FxSlot; FX_COLS],
}

impl Default for TableStep {
    fn default() -> Self {
        Self {
            transpose: 0,
            volume: EMPTY_U8,
            fx: [FxSlot::default(); FX_COLS],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub steps: [TableStep; TABLE_STEPS],
}

impl Default for Table {
    fn default() -> Self {
        Self {
            steps: [TableStep::default(); TABLE_STEPS],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groove {
    /// Ticks per phrase step in 24-PPQN clock units (`EMPTY_U8` = wrap to index 0).
    pub ticks: [u8; GROOVE_STEPS],
}

impl Default for Groove {
    fn default() -> Self {
        let mut ticks = [EMPTY_U8; GROOVE_STEPS];
        ticks[0] = 6;
        ticks[1] = 6;
        Self { ticks }
    }
}

impl Groove {
    pub fn swing(even_ticks: u8, odd_ticks: u8) -> Self {
        let mut ticks = [EMPTY_U8; GROOVE_STEPS];
        ticks[0] = even_ticks;
        ticks[1] = odd_ticks;
        Self { ticks }
    }
}

// ============================================================================
// 4 Instrument Engines (InstrumentEnum)
// ============================================================================

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SvfFilterMode {
    #[default]
    LowPass = 0,
    BandPass = 1,
    HighPass = 2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvfParams {
    pub mode: SvfFilterMode,
    pub cutoff: u8,     // 0x00..=0xFF
    pub resonance: u8,  // 0x00..=0xFF
    pub env_amount: i8, // -127..=127
}

impl Default for SvfParams {
    fn default() -> Self {
        Self {
            mode: SvfFilterMode::LowPass,
            cutoff: 0xD0,
            resonance: 0x20,
            env_amount: 48,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdsrParams {
    pub attack: u8,  // 0x00..=0xFF
    pub decay: u8,   // 0x00..=0xFF
    pub sustain: u8, // 0x00..=0xFF
    pub release: u8, // 0x00..=0xFF
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack: 0x02,
            decay: 0x40,
            sustain: 0x80,
            release: 0x30,
        }
    }
}

// --- 1. DrumKit (12 Pads x 3 Velocity Layers, 8-Voice Sub-Pool, Choke Groups) ---

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumPadParams {
    pub name: FixedStr<8>,
    pub level: u8,         // 0x00..=0xFF
    pub tune_semitones: i8, // -24..=+24
    pub pan: u8,           // 0x80 = center
    pub choke_target: u8,  // Pad index (0..11) this pad chokes, or EMPTY_U8
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumKitParams {
    pub pads: [DrumPadParams; DRUM_PADS],
    pub master_level: u8,
    pub filter: SvfParams,
}

impl Default for DrumKitParams {
    fn default() -> Self {
        let mut pads = [DrumPadParams {
            name: FixedStr::empty(),
            level: 0xE0,
            tune_semitones: 0,
            pan: 0x80,
            choke_target: EMPTY_U8,
        }; DRUM_PADS];

        for (i, pad) in pads.iter_mut().enumerate() {
            pad.name = FixedStr::from_str_lossy(DRUM_PAD_NAMES[i]);
            // Closed Hat (5) and Pedal Hat (6) choke Open Hat (8) with 3ms fade
            if i == PAD_CLOSED_HAT || i == PAD_PEDAL_HAT {
                pad.choke_target = PAD_OPEN_HAT as u8;
            }
        }

        Self {
            pads,
            master_level: 0xE0,
            filter: SvfParams {
                mode: SvfFilterMode::LowPass,
                cutoff: 0xFF,
                resonance: 0x00,
                env_amount: 0,
            },
        }
    }
}

// --- 2. SingleSample (Pitched/Sliced WAV + Resonant SVF) ---

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleLoopMode {
    OneShot = 0,
    ForwardLoop = 1,
    PingPong = 2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleSampleParams {
    pub sample_name: FixedStr<16>,
    pub root_note: u8,     // Default C-4 (48)
    pub fine_tune: i8,     // -100..=+100 cents
    pub slice_count: u8,   // 1..=64 equal slices (or 1 = continuous offset)
    pub loop_mode: SampleLoopMode,
    pub loop_start: u8,    // 0x00..=0xFF normalized start
    pub loop_end: u8,      // 0x00..=0xFF normalized end
    pub filter: SvfParams,
    pub amp_env: AdsrParams,
    pub filter_env: AdsrParams,
}

impl Default for SingleSampleParams {
    fn default() -> Self {
        Self {
            sample_name: FixedStr::from_str_lossy("BASS_SAW.WAV"),
            root_note: DRUM_BASE_NOTE,
            fine_tune: 0,
            slice_count: 16,
            loop_mode: SampleLoopMode::OneShot,
            loop_start: 0x00,
            loop_end: 0xFF,
            filter: SvfParams::default(),
            amp_env: AdsrParams::default(),
            filter_env: AdsrParams {
                attack: 0x01,
                decay: 0x38,
                sustain: 0x20,
                release: 0x28,
            },
        }
    }
}

// --- 3. InternalSynth (Dual-Mode: 4-Op FM & PolyBLEP Virtual Analog) ---

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthMode {
    VirtualAnalog = 0,
    Fm4Op = 1,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaWaveform {
    Saw = 0,
    Pulse = 1,
    Triangle = 2,
    Noise = 3,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmOpParams {
    pub ratio_coarse: u8, // 1..=16 frequency multiplier (0 = 0.5x)
    pub level: u8,        // 0x00..=0xFF modulation/output index
    pub attack: u8,       // 0x00..=0xFF
    pub decay: u8,        // 0x00..=0xFF
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalSynthParams {
    pub mode: SynthMode,
    // Virtual Analog parameters
    pub osc1_wave: VaWaveform,
    pub osc2_wave: VaWaveform,
    pub pulse_width: u8,    // 0x10..=0xF0
    pub osc2_detune: i8,    // -64..=+64 cents
    pub osc2_semitones: i8, // -24..=+24 semitones
    pub osc_mix: u8,        // 0x00=Osc1, 0x80=50/50, 0xFF=Osc2
    pub sub_osc_level: u8,  // 0x00..=0xFF (-1 octave square sub)
    // 4-Op FM parameters
    pub fm_algorithm: u8,   // 0..=7 (8 classic 4-op topologies)
    pub fm_feedback: u8,    // 0x00..=0xFF Op4 self-feedback
    pub fm_ops: [FmOpParams; 4],
    // Shared Filter & Envelopes
    pub filter: SvfParams,
    pub amp_env: AdsrParams,
    pub mod_env: AdsrParams,
}

impl Default for InternalSynthParams {
    fn default() -> Self {
        Self {
            mode: SynthMode::VirtualAnalog,
            osc1_wave: VaWaveform::Saw,
            osc2_wave: VaWaveform::Pulse,
            pulse_width: 0x80,
            osc2_detune: 12,
            osc2_semitones: 0,
            osc_mix: 0x70,
            sub_osc_level: 0x50,
            fm_algorithm: 0,
            fm_feedback: 0x30,
            fm_ops: [
                FmOpParams {
                    ratio_coarse: 1,
                    level: 0xFF,
                    attack: 0x02,
                    decay: 0x60,
                },
                FmOpParams {
                    ratio_coarse: 2,
                    level: 0x78,
                    attack: 0x02,
                    decay: 0x40,
                },
                FmOpParams {
                    ratio_coarse: 3,
                    level: 0x50,
                    attack: 0x01,
                    decay: 0x30,
                },
                FmOpParams {
                    ratio_coarse: 1,
                    level: 0x40,
                    attack: 0x01,
                    decay: 0x24,
                },
            ],
            filter: SvfParams::default(),
            amp_env: AdsrParams::default(),
            mod_env: AdsrParams {
                attack: 0x01,
                decay: 0x30,
                sustain: 0x10,
                release: 0x20,
            },
        }
    }
}

// --- 4. MidiOut (Hardware 5-Pin DIN MIDI + Named .toml Synth Profiles) ---

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiCcSlot {
    pub cc_number: u8, // 0..=127 (or EMPTY_U8 if disabled)
    pub value: u8,     // 0..=127
    pub name: FixedStr<14>,
}

impl Default for MidiCcSlot {
    fn default() -> Self {
        Self {
            cc_number: 74,
            value: 64,
            name: FixedStr::from_str_lossy("CUTOFF"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiOutParams {
    pub channel: u8,          // 0..=15 (MIDI Ch 1..16)
    pub program: u8,          // 0..=127 or EMPTY_U8
    pub bank_msb: u8,         // 0..=127 or EMPTY_U8
    pub bank_lsb: u8,         // 0..=127 or EMPTY_U8
    pub profile_name: FixedStr<16>,
    pub cc_slots: [MidiCcSlot; MIDI_CC_SLOTS],
}

impl Default for MidiOutParams {
    fn default() -> Self {
        Self {
            channel: 0,
            program: EMPTY_U8,
            bank_msb: EMPTY_U8,
            bank_lsb: EMPTY_U8,
            profile_name: FixedStr::from_str_lossy("ROLAND_SH01A"),
            cc_slots: [
                MidiCcSlot {
                    cc_number: 74,
                    value: 96,
                    name: FixedStr::from_str_lossy("VCF CUTOFF"),
                },
                MidiCcSlot {
                    cc_number: 71,
                    value: 32,
                    name: FixedStr::from_str_lossy("VCF RESO"),
                },
                MidiCcSlot {
                    cc_number: 24,
                    value: 64,
                    name: FixedStr::from_str_lossy("ENV MOD"),
                },
                MidiCcSlot {
                    cc_number: 16,
                    value: 80,
                    name: FixedStr::from_str_lossy("LFO RATE"),
                },
                MidiCcSlot {
                    cc_number: 19,
                    value: 64,
                    name: FixedStr::from_str_lossy("SUB LEVEL"),
                },
                MidiCcSlot {
                    cc_number: 91,
                    value: 40,
                    name: FixedStr::from_str_lossy("CHORUS/REV"),
                },
            ],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentEnum {
    DrumKit(DrumKitParams),
    SingleSample(SingleSampleParams),
    InternalSynth(InternalSynthParams),
    MidiOut(MidiOutParams),
}

impl InstrumentEnum {
    pub fn kind_name(&self) -> &'static str {
        match self {
            InstrumentEnum::DrumKit(_) => "DRUMKIT",
            InstrumentEnum::SingleSample(_) => "SAMPLER",
            InstrumentEnum::InternalSynth(_) => "SYNTH",
            InstrumentEnum::MidiOut(_) => "MIDIOUT",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instrument {
    pub name: FixedStr<12>,
    pub volume: u8,         // 0x00..=0xFF
    pub pan: u8,            // 0x80 = Center
    pub transpose: i8,      // -48..=+48
    pub table_id: u8,       // 0x00..=0x7F or EMPTY_U8
    #[serde(default)]
    pub delay_send: u8,     // 0x00..=0xFF master ping-pong delay send
    #[serde(default)]
    pub reverb_send: u8,    // 0x00..=0xFF master stereo reverb send
    pub engine: InstrumentEnum,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: FixedStr::from_str_lossy("INIT SYNTH"),
            volume: 0xD0,
            pan: 0x80,
            transpose: 0,
            table_id: EMPTY_U8,
            delay_send: 0x00,
            reverb_send: 0x00,
            engine: InstrumentEnum::InternalSynth(InternalSynthParams::default()),
        }
    }
}

// ============================================================================
// Full Project State (Mirrored in UI and Audio Threads)
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub name: FixedStr<16>,
    pub tempo_bpm: f32,
    pub master_volume: u8,
    pub song: Vec<[u8; NUM_TRACKS]>,
    pub chains: Vec<Chain>,
    pub phrases: Vec<Phrase>,
    pub instruments: Vec<Instrument>,
    pub tables: Vec<Table>,
    pub grooves: [Groove; NUM_GROOVES],
}

impl Default for ProjectState {
    fn default() -> Self {
        let mut song = vec![[EMPTY_U8; NUM_TRACKS]; SONG_ROWS];
        let mut chains = vec![Chain::default(); NUM_CHAINS];
        let mut phrases = vec![Phrase::default(); NUM_PHRASES];
        let mut instruments = vec![Instrument::default(); NUM_INSTRUMENTS];
        let mut tables = vec![Table::default(); NUM_TABLES];
        let mut grooves = [Groove::default(); NUM_GROOVES];

        // Groove 00: Straight 16ths (06/06)
        grooves[0] = Groove::swing(6, 6);
        // Groove 01: Classic MPC/M8 Swing (07/05)
        grooves[1] = Groove::swing(7, 5);
        // Groove 02: Hard Shuffle (08/04)
        grooves[2] = Groove::swing(8, 4);

        // Configure Starter Instruments 00..04 covering all 4 InstrumentEnum engines
        instruments[0] = Instrument {
            name: FixedStr::from_str_lossy("808 ACOUKIT"),
            volume: 0xE8,
            pan: 0x80,
            transpose: 0,
            table_id: EMPTY_U8,
            delay_send: 0x08,
            reverb_send: 0x18,
            engine: InstrumentEnum::DrumKit(DrumKitParams::default()),
        };
        instruments[1] = Instrument {
            name: FixedStr::from_str_lossy("RESO SAMP"),
            volume: 0xD8,
            pan: 0x80,
            transpose: 0,
            table_id: 0,
            delay_send: 0x20,
            reverb_send: 0x14,
            engine: InstrumentEnum::SingleSample(SingleSampleParams::default()),
        };
        instruments[2] = Instrument {
            name: FixedStr::from_str_lossy("VA ACIDSUB"),
            volume: 0xD0,
            pan: 0x74,
            transpose: 0,
            table_id: EMPTY_U8,
            delay_send: 0x38,
            reverb_send: 0x24,
            engine: InstrumentEnum::InternalSynth(InternalSynthParams::default()),
        };
        let mut fm_params = InternalSynthParams::default();
        fm_params.mode = SynthMode::Fm4Op;
        fm_params.fm_algorithm = 2;
        instruments[3] = Instrument {
            name: FixedStr::from_str_lossy("FM BELLCHRD"),
            volume: 0xC8,
            pan: 0x8C,
            transpose: 0,
            table_id: 1,
            delay_send: 0x54,
            reverb_send: 0x60,
            engine: InstrumentEnum::InternalSynth(fm_params),
        };
        instruments[4] = Instrument {
            name: FixedStr::from_str_lossy("DIN SH-01A"),
            volume: 0xE0,
            pan: 0x80,
            transpose: 0,
            table_id: EMPTY_U8,
            delay_send: 0x00,
            reverb_send: 0x00,
            engine: InstrumentEnum::MidiOut(MidiOutParams::default()),
        };

        // Table 00: Subtle filter envelope movement
        tables[0].steps[0].fx[0] = FxSlot {
            command: FxCommand::Fcu,
            value: 0xE0,
        };
        tables[0].steps[2].fx[0] = FxSlot {
            command: FxCommand::Fcu,
            value: 0xA0,
        };
        tables[0].steps[4].fx[0] = FxSlot {
            command: FxCommand::Fcu,
            value: 0x70,
        };

        // Table 01: Chord arpeggio (0, +3, +7)
        tables[1].steps[0].transpose = 0;
        tables[1].steps[1].transpose = 3;
        tables[1].steps[2].transpose = 7;
        tables[1].steps[3].fx[0] = FxSlot {
            command: FxCommand::Hop,
            value: 0x00,
        };

        // Seed an 8-row x 8-track full demo song ("CYBER_PINE_DEMO") across Tracks 0..7
        // Row 00..07: Intro -> Main Groove -> Acid Drop -> Turnaround Fill
        for row_idx in 0..8usize {
            let variation = (row_idx % 2) as u8;
            for trk in 0..NUM_TRACKS {
                let chain_id = (trk as u8) * 2 + variation;
                song[row_idx][trk] = chain_id;
            }
        }

        // Populate Chains 0..15 (2 variations per track, each playing 4 phrases of 16 steps)
        for trk in 0..NUM_TRACKS {
            for var in 0..2usize {
                let c_id = trk * 2 + var;
                let p_base = (trk * 2 + var) as u8;
                chains[c_id].rows[0] = ChainRow {
                    phrase: p_base,
                    transpose: 0,
                };
                chains[c_id].rows[1] = ChainRow {
                    phrase: p_base,
                    transpose: if trk == 2 || trk == 3 { 3 } else { 0 },
                };
                chains[c_id].rows[2] = ChainRow {
                    phrase: p_base,
                    transpose: if trk == 2 || trk == 3 { 5 } else { 0 },
                };
                chains[c_id].rows[3] = ChainRow {
                    phrase: p_base,
                    transpose: if var == 1 && (trk == 1 || trk == 2) {
                        -2
                    } else {
                        0
                    },
                };
            }
        }

        // --- Track 1 (Phrases 00 & 01 - DrumKit): Kick, Snare, Clap, Open->Closed Hat Chokes ---
        for p_idx in 0..=1usize {
            let p = &mut phrases[p_idx];
            p.steps[0].fx[0] = FxSlot {
                command: FxCommand::Grv,
                value: 0x01, // Swing Groove 01 (07/05)
            };
            for step in (0..16).step_by(4) {
                p.steps[step].note = DRUM_BASE_NOTE; // C-4 Kick
                p.steps[step].volume = 0xF8;
                p.steps[step].instrument = 0x00;
            }
            if p_idx == 1 {
                // Extra syncopated kick + snare fill on variation B
                p.steps[10].note = DRUM_BASE_NOTE;
                p.steps[10].volume = 0xD8;
                p.steps[10].instrument = 0x00;
                p.steps[14].note = DRUM_BASE_NOTE + 2;
                p.steps[14].volume = 0xE8;
                p.steps[14].instrument = 0x00;
                p.steps[15].note = DRUM_BASE_NOTE + 3;
                p.steps[15].volume = 0xF0;
                p.steps[15].instrument = 0x00;
            }
            for &s in &[4usize, 12] {
                p.steps[s].note = DRUM_BASE_NOTE + 2; // D-4 Snare
                p.steps[s].volume = 0xF0;
                p.steps[s].instrument = 0x00;
            }
            // Open Hat (G#4) choked by Closed Hat (F-4) / Pedal Hat (F#4)
            for &(s, pad, vel) in &[
                (2usize, PAD_OPEN_HAT, 0xC8u8),
                (3, PAD_CLOSED_HAT, 0x98),
                (6, PAD_OPEN_HAT, 0xD0),
                (7, PAD_PEDAL_HAT, 0x88),
                (10, PAD_CLOSED_HAT, 0x80),
                (14, PAD_OPEN_HAT, 0xD0),
                (15, PAD_CLOSED_HAT, 0xA8),
            ] {
                if p.steps[s].note == EMPTY_U8 {
                    p.steps[s].note = DRUM_BASE_NOTE + pad as u8;
                    p.steps[s].volume = vel;
                    p.steps[s].instrument = 0x00;
                }
            }
        }

        // --- Track 2 (Phrases 02 & 03 - SingleSample): Sliced & Filtered Bass ---
        for p_idx in 2..=3usize {
            let p = &mut phrases[p_idx];
            for &(s, note, ofs, fcu) in &[
                (0usize, 36u8, 0x00u8, 0xD0u8),
                (2, 36, 0x20, 0x98),
                (3, 48, 0x40, 0xE8),
                (6, 39, 0x80, 0xC0),
                (8, 36, 0x00, 0xA0),
                (10, 41, 0xC0, 0xD8),
                (12, 43, 0x40, 0xB8),
                (14, 46, 0x60, 0xF0),
            ] {
                p.steps[s].note = if p_idx == 3 && s >= 10 { note + 2 } else { note };
                p.steps[s].volume = 0xE0;
                p.steps[s].instrument = 0x01;
                p.steps[s].fx[0] = FxSlot {
                    command: FxCommand::Ofs,
                    value: ofs,
                };
                p.steps[s].fx[1] = FxSlot {
                    command: FxCommand::Fcu,
                    value: fcu,
                };
            }
        }

        // --- Track 3 (Phrases 04 & 05 - InternalSynth VA): Acid Lead / Bassline ---
        for p_idx in 4..=5usize {
            let p = &mut phrases[p_idx];
            for &(s, note, fcu) in &[
                (0usize, 48u8, 0x90u8),
                (2, 48, 0xC0),
                (4, 60, 0xF8),
                (6, 51, 0xA8),
                (7, 53, 0xD0),
                (9, 55, 0xE0),
                (12, 48, 0x88),
                (14, 58, 0xF0),
            ] {
                p.steps[s].note = note;
                p.steps[s].volume = 0xD4;
                p.steps[s].instrument = 0x02;
                p.steps[s].fx[0] = FxSlot {
                    command: FxCommand::Fcu,
                    value: fcu,
                };
            }
        }

        // --- Track 4 (Phrases 06 & 07 - InternalSynth 4-Op FM): Arpeggiated FM Chords ---
        for p_idx in 6..=7usize {
            let p = &mut phrases[p_idx];
            for &(s, note, arp) in &[
                (0usize, 60u8, 0x37u8),
                (4, 63, 0x47),
                (8, 65, 0x37),
                (12, 67, 0x59),
            ] {
                p.steps[s].note = note;
                p.steps[s].volume = 0xC0;
                p.steps[s].instrument = 0x03;
                p.steps[s].fx[0] = FxSlot {
                    command: FxCommand::Arp,
                    value: arp,
                };
            }
        }

        // --- Track 5 (Phrases 08 & 09 - MidiOut + Internal Preview): Synth Counter-Melody ---
        for p_idx in 8..=9usize {
            let p = &mut phrases[p_idx];
            for &(s, note, cc_val) in &[
                (0usize, 55u8, 0x40u8),
                (3, 58, 0x58),
                (6, 60, 0x74),
                (8, 62, 0x60),
                (11, 65, 0x7C),
                (14, 63, 0x50),
            ] {
                p.steps[s].note = note;
                p.steps[s].volume = 0xC8;
                p.steps[s].instrument = 0x04;
                p.steps[s].fx[0] = FxSlot {
                    command: FxCommand::Cca,
                    value: cc_val,
                };
            }
        }

        // --- Track 6 (Phrases 0A & 0B - FM Pluck Bell Counterpoint) ---
        for p_idx in 10..=11usize {
            let p = &mut phrases[p_idx];
            for &(s, note) in &[
                (1usize, 72u8),
                (5, 75),
                (9, 79),
                (11, 77),
                (13, 75),
                (15, 74),
            ] {
                p.steps[s].note = note;
                p.steps[s].volume = 0xB0;
                p.steps[s].instrument = 0x03;
            }
        }

        // --- Track 7 (Phrases 0C & 0D - Percussion Polyrhythm: Rim, Tom, Clap) ---
        for p_idx in 12..=13usize {
            let p = &mut phrases[p_idx];
            for &(s, pad, vel) in &[
                (1usize, 1u8, 0xA0u8), // Rim
                (5, 4, 0xB8),          // Low Tom
                (9, 7, 0xC0),          // Mid Tom
                (11, 9, 0xB0),         // High Tom
                (13, 3, 0xC8),         // Clap
            ] {
                p.steps[s].note = DRUM_BASE_NOTE + pad;
                p.steps[s].volume = vel;
                p.steps[s].instrument = 0x00;
            }
        }

        // --- Track 8 (Phrases 0E & 0F - Shimmer Pad / Octave Pulse) ---
        for p_idx in 14..=15usize {
            let p = &mut phrases[p_idx];
            for &(s, note) in &[(0usize, 48u8), (8, 55)] {
                p.steps[s].note = note;
                p.steps[s].volume = 0xA8;
                p.steps[s].instrument = 0x02;
                p.steps[s].fx[0] = FxSlot {
                    command: FxCommand::Arp,
                    value: 0x7C,
                };
            }
        }

        Self {
            name: FixedStr::from_str_lossy("CYBER_PINE_DEMO"),
            tempo_bpm: 126.0,
            master_volume: 0xC0,
            song,
            chains,
            phrases,
            instruments,
            tables,
            grooves,
        }
    }
}

impl ProjectState {
    /// Finds the first unused Chain ID (`0x00..=0xFD`) that has no steps assigned and is not referenced in `song`.
    pub fn find_unused_chain(&self) -> Option<u8> {
        for cid in 0..(NUM_CHAINS - 1) {
            let empty_rows = self.chains[cid]
                .rows
                .iter()
                .all(|r| r.phrase == EMPTY_U8);
            if !empty_rows {
                continue;
            }
            let referenced = self
                .song
                .iter()
                .any(|row| row.iter().any(|&c| c as usize == cid));
            if !referenced {
                return Some(cid as u8);
            }
        }
        None
    }

    /// Finds the first unused Phrase ID (`0x00..=0xFD`) that has no notes/FX and is not referenced in any `Chain`.
    pub fn find_unused_phrase(&self) -> Option<u8> {
        for pid in 0..(NUM_PHRASES - 1) {
            let empty_steps = self.phrases[pid]
                .steps
                .iter()
                .all(|s| s.note == EMPTY_U8 && s.instrument == EMPTY_U8 && s.fx[0].command == FxCommand::None);
            if !empty_steps {
                continue;
            }
            let referenced = self
                .chains
                .iter()
                .any(|c| c.rows.iter().any(|r| r.phrase as usize == pid));
            if !referenced {
                return Some(pid as u8);
            }
        }
        None
    }
}

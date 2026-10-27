pub mod dsp;
pub mod midi;
pub mod sequencer;

use crate::audio::dsp::fx::MasterFxBus;
use crate::audio::dsp::TrackDspChannel;
use crate::audio::midi::{AlsaMidiDriver, MidiOutQueue, MidiPacket};
use crate::audio::sequencer::{
    Sequencer, VoiceEventKind, VoiceEventQueue, BUFFER_FRAMES, SAMPLE_RATE,
};
use crate::ipc::{
    AudioDeviceState, AudioIpcHandle, AudioTelemetry, AudioToUiMsg, SampleBuffer, UiToAudioMsg,
};
use crate::model::{
    ProjectState, DRUM_PADS, EMPTY_U8, NUM_INSTRUMENTS, NUM_TRACKS, VEL_LAYERS,
};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const CHANNELS: usize = 2;
pub const BLOCK_SAMPLES: usize = BUFFER_FRAMES * CHANNELS; // 256 interleaved stereo samples

/// 16-byte aligned contiguous stereo buffer so `-C target-cpu=cortex-a53 -C target-feature=+neon`
/// auto-vectorizes mixing loops across 4 `f32` lanes (`float32x4_t`).
#[repr(align(16))]
#[derive(Copy, Clone)]
pub struct AlignedStereoBlock(pub [f32; BLOCK_SAMPLES]);

impl Default for AlignedStereoBlock {
    fn default() -> Self {
        Self([0.0; BLOCK_SAMPLES])
    }
}

impl AlignedStereoBlock {
    #[inline]
    pub fn clear(&mut self) {
        self.0.fill(0.0);
    }

    /// Contiguous 4-lane SIMD-friendly accumulate: `self[i] += src[i] * gain`.
    #[inline]
    pub fn accumulate_scaled(&mut self, src: &AlignedStereoBlock, gain: f32) {
        for (dst, &s) in self.0.iter_mut().zip(src.0.iter()) {
            *dst += s * gain;
        }
    }

    /// Converts float `[-1.0, 1.0]` stereo frames to `i16` with C1-continuous soft-knee limiting
    /// and returns `(peak_l, peak_r)`.
    #[inline]
    pub fn quantize_to_i16(&self, master_gain: f32, out_pcm: &mut [i16; BLOCK_SAMPLES]) -> (f32, f32) {
        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;

        #[inline]
        fn soft_knee_limit(x: f32) -> f32 {
            let ax = x.abs();
            if ax <= 0.68 {
                x
            } else {
                // C1-continuous asymptotic knee from 0.68 -> 0.94 (prevents DAC/speaker hard-clipping)
                let excess = ax - 0.68;
                let compressed = 0.68 + 0.26 * (excess / (0.26 + excess));
                compressed.copysign(x)
            }
        }

        for frame in 0..BUFFER_FRAMES {
            let l_raw = self.0[frame * 2] * master_gain;
            let r_raw = self.0[frame * 2 + 1] * master_gain;

            let l_clip = soft_knee_limit(l_raw);
            let r_clip = soft_knee_limit(r_raw);

            peak_l = peak_l.max(l_clip.abs());
            peak_r = peak_r.max(r_clip.abs());

            out_pcm[frame * 2] = (l_clip * 32760.0) as i16;
            out_pcm[frame * 2 + 1] = (r_clip * 32760.0) as i16;
        }

        (peak_l, peak_r)
    }
}

/// Pre-allocated RAM sample storage owned by Thread 2.
/// All `SampleBuffer` swaps happen in O(1) without heap allocation, and any replaced buffer
/// is immediately sent back to Thread 1 via `AudioToUiMsg::RetireSampleBuffer`.
pub struct RealtimeSampleBank {
    pub drum_layers: Vec<[[Option<SampleBuffer>; VEL_LAYERS]; DRUM_PADS]>,
    pub single_samples: Vec<Option<SampleBuffer>>,
}

impl Default for RealtimeSampleBank {
    fn default() -> Self {
        let empty_pad: [Option<SampleBuffer>; VEL_LAYERS] = [None, None, None];
        let empty_kit: [[Option<SampleBuffer>; VEL_LAYERS]; DRUM_PADS] = [
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad.clone(),
            empty_pad,
        ];
        Self {
            drum_layers: vec![empty_kit; NUM_INSTRUMENTS],
            single_samples: vec![None; NUM_INSTRUMENTS],
        }
    }
}

impl RealtimeSampleBank {
    /// Installs a `DrumKit` velocity layer sample and returns the previous buffer (if any)
    /// so the caller can send it to `AudioToUiMsg::RetireSampleBuffer` instead of dropping it on Thread 2.
    #[inline]
    pub fn swap_drum_layer(
        &mut self,
        inst_id: u8,
        pad_idx: u8,
        vel_layer: u8,
        sample: SampleBuffer,
    ) -> Option<SampleBuffer> {
        let i = (inst_id as usize) % NUM_INSTRUMENTS;
        let p = (pad_idx as usize) % DRUM_PADS;
        let v = (vel_layer as usize) % VEL_LAYERS;
        self.drum_layers[i][p][v].replace(sample)
    }

    /// Installs a `SingleSample` buffer and returns the previous buffer (if any) for non-RT retirement.
    #[inline]
    pub fn swap_single_sample(&mut self, inst_id: u8, sample: SampleBuffer) -> Option<SampleBuffer> {
        let i = (inst_id as usize) % NUM_INSTRUMENTS;
        self.single_samples[i].replace(sample)
    }
}

// ============================================================================
// Thread 2 Affinity (Isolated Cores 2-3) & SCHED_FIFO (Priority 95)
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RealtimeThreadConfigStatus {
    pub affinity_pinned: bool,
    pub sched_fifo_active: bool,
}

/// Pins the calling thread to isolated Cores 2 and 3 and elevates to `SCHED_FIFO` priority 95.
/// Gracefully handles unprivileged execution (`EPERM`) when running on Steam Deck dev host.
pub fn configure_realtime_thread() -> RealtimeThreadConfigStatus {
    let mut affinity_pinned = false;
    let sched_fifo_active;

    unsafe {
        // 1. Pin to Cores 2 and 3 (`isolcpus=2,3 nohz_full=2,3`)
        let online_cores = libc::sysconf(libc::_SC_NPROCESSORS_ONLN);
        if online_cores >= 4 {
            let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut cpuset);
            libc::CPU_SET(2, &mut cpuset);
            libc::CPU_SET(3, &mut cpuset);
            let res = libc::sched_setaffinity(
                0,
                std::mem::size_of::<libc::cpu_set_t>(),
                &cpuset,
            );
            affinity_pinned = res == 0;
        }

        // 2. Request SCHED_FIFO Priority 95
        let param = libc::sched_param { sched_priority: 95 };
        let res = libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param);
        sched_fifo_active = res == 0;
    }

    RealtimeThreadConfigStatus {
        affinity_pinned,
        sched_fifo_active,
    }
}

// ============================================================================
// Host PipeWire / PulseAudio Native Socket Bridge (Steam Deck / Distrobox)
// ============================================================================

const PA_STREAM_PLAYBACK: libc::c_int = 1;
const PA_SAMPLE_S16LE: libc::c_int = 3;

#[repr(C)]
struct PaSampleSpec {
    format: libc::c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct PaBufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type PaSimpleNewFn = unsafe extern "C" fn(
    server: *const libc::c_char,
    name: *const libc::c_char,
    dir: libc::c_int,
    dev: *const libc::c_char,
    stream_name: *const libc::c_char,
    ss: *const PaSampleSpec,
    map: *const libc::c_void,
    attr: *const PaBufferAttr,
    error: *mut libc::c_int,
) -> *mut libc::c_void;

type PaSimpleWriteFn = unsafe extern "C" fn(
    s: *mut libc::c_void,
    data: *const libc::c_void,
    bytes: usize,
    error: *mut libc::c_int,
) -> libc::c_int;

type PaSimpleFreeFn = unsafe extern "C" fn(s: *mut libc::c_void);

/// Zero-allocation runtime wrapper around `libpulse-simple.so.0` (`pa_simple_write`).
/// Connects directly to the host PipeWire server (`/run/user/1000/pulse/native`) on Steam Deck
/// even inside a Distrobox container where `alsa-plugins` (`pipewire-alsa`) is not installed.
/// Dynamically loaded via `dlopen` so `aarch64` PinePhone builds require only `libasound`.
pub struct PulseSimpleStream {
    handle: *mut libc::c_void,
    write_fn: PaSimpleWriteFn,
    free_fn: PaSimpleFreeFn,
}

unsafe impl Send for PulseSimpleStream {}

impl PulseSimpleStream {
    pub fn try_open() -> Option<Self> {
        unsafe {
            let mut lib = libc::dlopen(
                b"libpulse-simple.so.0\0".as_ptr() as *const libc::c_char,
                libc::RTLD_NOW,
            );
            if lib.is_null() {
                lib = libc::dlopen(
                    b"libpulse-simple.so\0".as_ptr() as *const libc::c_char,
                    libc::RTLD_NOW,
                );
            }
            if lib.is_null() {
                return None;
            }

            let new_sym = libc::dlsym(lib, b"pa_simple_new\0".as_ptr() as *const libc::c_char);
            let write_sym = libc::dlsym(lib, b"pa_simple_write\0".as_ptr() as *const libc::c_char);
            let free_sym = libc::dlsym(lib, b"pa_simple_free\0".as_ptr() as *const libc::c_char);
            if new_sym.is_null() || write_sym.is_null() || free_sym.is_null() {
                return None;
            }

            let new_fn: PaSimpleNewFn = std::mem::transmute(new_sym);
            let write_fn: PaSimpleWriteFn = std::mem::transmute(write_sym);
            let free_fn: PaSimpleFreeFn = std::mem::transmute(free_sym);

            let ss = PaSampleSpec {
                format: PA_SAMPLE_S16LE,
                rate: SAMPLE_RATE,
                channels: CHANNELS as u8,
            };
            let block_bytes = (BLOCK_SAMPLES * std::mem::size_of::<i16>()) as u32;
            let attr = PaBufferAttr {
                maxlength: block_bytes * 8,
                tlength: block_bytes * 4,
                prebuf: u32::MAX,
                minreq: block_bytes,
                fragsize: u32::MAX,
            };
            let mut err: libc::c_int = 0;
            let handle = new_fn(
                std::ptr::null(),
                b"pineTracker\0".as_ptr() as *const libc::c_char,
                PA_STREAM_PLAYBACK,
                std::ptr::null(),
                b"pineTracker 48kHz Master Out\0".as_ptr() as *const libc::c_char,
                &ss,
                std::ptr::null(),
                &attr,
                &mut err,
            );

            if handle.is_null() {
                None
            } else {
                Some(Self {
                    handle,
                    write_fn,
                    free_fn,
                })
            }
        }
    }

    #[inline]
    pub fn write_block(&mut self, pcm_buf: &[i16; BLOCK_SAMPLES]) -> bool {
        let mut err: libc::c_int = 0;
        let res = unsafe {
            (self.write_fn)(
                self.handle,
                pcm_buf.as_ptr() as *const libc::c_void,
                BLOCK_SAMPLES * std::mem::size_of::<i16>(),
                &mut err,
            )
        };
        res >= 0
    }
}

impl Drop for PulseSimpleStream {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                (self.free_fn)(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}

// ============================================================================
// Dual-DAC ALSA Routing & Hot-Plug Fallback State Machine
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DacRecoveryAction {
    RecoverSameDevice,
    FallbackToInternalCodec,
    FallbackToDummy,
}

/// Pure classifier for ALSA `writei` error codes according to Section 5 of GEMINI.md:
/// - On PinePhone (`ExternalUsbDac` `"hw:1,0"`): `-ENODEV` (19) on USB-C hub unplug immediately
///   falls back to `"hw:0,0"` (`InternalCodecFallback`); `-EPIPE` (32) attempts recovery first
///   and falls back if recovery fails.
#[inline]
pub fn classify_alsa_write_error(
    errno: i32,
    current_state: AudioDeviceState,
    recover_failed: bool,
) -> DacRecoveryAction {
    if errno == libc::ENODEV {
        return match current_state {
            AudioDeviceState::ExternalUsbDac => DacRecoveryAction::FallbackToInternalCodec,
            _ => DacRecoveryAction::FallbackToDummy,
        };
    }

    if errno == libc::EPIPE || errno == libc::ESTRPIPE {
        if !recover_failed {
            return DacRecoveryAction::RecoverSameDevice;
        }
        return match current_state {
            AudioDeviceState::ExternalUsbDac => DacRecoveryAction::FallbackToInternalCodec,
            _ => DacRecoveryAction::FallbackToDummy,
        };
    }

    if current_state == AudioDeviceState::ExternalUsbDac {
        DacRecoveryAction::FallbackToInternalCodec
    } else {
        DacRecoveryAction::FallbackToDummy
    }
}

pub struct AlsaDualDacDriver {
    pcm: Option<PCM>,
    pulse: Option<PulseSimpleStream>,
    pub state: AudioDeviceState,
    pub xrun_recoveries: u32,
    usb_reprobe_counter: u32,
    deck_mode: bool,
}

impl AlsaDualDacDriver {
    pub fn new(deck_mode: bool) -> Self {
        let mut drv = Self {
            pcm: None,
            pulse: None,
            state: AudioDeviceState::HeadlessDummy,
            xrun_recoveries: 0,
            usb_reprobe_counter: 0,
            deck_mode,
        };
        drv.initialize_preferred_device();
        drv
    }

    pub fn initialize_preferred_device(&mut self) {
        if self.deck_mode {
            // Steam Deck: First connect directly to host PipeWire via libpulse-simple so audio
            // routes to internal speakers (CS35L41) even inside containers without pipewire-alsa.
            if let Some(pulse_stream) = PulseSimpleStream::try_open() {
                self.pulse = Some(pulse_stream);
                self.pcm = None;
                self.state = AudioDeviceState::PipeWireDefault;
                return;
            }
            // Fallback to ALSA "default", then Steam Deck Card 1 ("plughw:1,0" / "hw:1,0")
            for dev in ["default", "plughw:1,0", "hw:1,0"] {
                if let Some(pcm) = Self::try_open_pcm(dev) {
                    self.pcm = Some(pcm);
                    self.pulse = None;
                    self.state = AudioDeviceState::PipeWireDefault;
                    return;
                }
            }
        } else {
            // PinePhone: Prefer "hw:1,0" (external USB-C DAC on hub), fallback to "hw:0,0" (internal A64 codec)
            if let Some(pcm) = Self::try_open_pcm("hw:1,0") {
                self.pcm = Some(pcm);
                self.pulse = None;
                self.state = AudioDeviceState::ExternalUsbDac;
                return;
            }
            if let Some(pcm) = Self::try_open_pcm("hw:0,0") {
                Self::configure_pinephone_a64_mixer();
                self.pcm = Some(pcm);
                self.pulse = None;
                self.state = AudioDeviceState::InternalCodecFallback;
                return;
            }
            // Final fallback to PipeWire / ALSA "default" if running on a desktop Linux host without --features deck-dev
            if let Some(pulse_stream) = PulseSimpleStream::try_open() {
                self.pulse = Some(pulse_stream);
                self.pcm = None;
                self.state = AudioDeviceState::PipeWireDefault;
                return;
            }
            if let Some(pcm) = Self::try_open_pcm("default") {
                self.pcm = Some(pcm);
                self.pulse = None;
                self.state = AudioDeviceState::PipeWireDefault;
                return;
            }
        }

        self.pcm = None;
        self.pulse = None;
        self.state = AudioDeviceState::HeadlessDummy;
    }

    /// Configures the Allwinner A64 (`sun50i-codec-analog` / `sun8i-codec`) ALSA mixer on `hw:0`
    /// according to the PinePhone UCM `HiFi.conf` routing so both the 3.5mm Headphone jack and
    /// internal Speaker output audio immediately without needing `alsaucm` or `PulseAudio`.
    pub fn configure_pinephone_a64_mixer() {
        if let Ok(mixer) = alsa::mixer::Mixer::new("hw:0", false) {
            for name in [
                "DAC",
                "AIF1 Slot 0 Digital DAC",
                "Headphone",
                "Line Out",
            ] {
                let id = alsa::mixer::SelemId::new(name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    let _ = selem.set_playback_switch_all(1);
                }
            }
            // Digital stages ("DAC" and "AIF1 DA0") have 0 dB unity gain at 160 in HiFi.conf
            // (values > 160 boost up to +12 dB and cause digital clipping!)
            for name in ["DAC", "AIF1 DA0"] {
                let id = alsa::mixer::SelemId::new(name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    if selem.has_playback_volume() {
                        let (min, max) = selem.get_playback_volume_range();
                        let unity = 160.clamp(min, max);
                        let _ = selem.set_playback_volume_all(unity);
                    }
                }
            }
            // Analog output stages ("Headphone" and "Line Out" internal speaker): default to clean 70%
            for name in ["Headphone", "Line Out"] {
                let id = alsa::mixer::SelemId::new(name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    if selem.has_playback_volume() {
                        let (min, max) = selem.get_playback_volume_range();
                        let target = min + ((max - min) * 70) / 100;
                        let _ = selem.set_playback_volume_all(target);
                    }
                }
            }
            let routes = [
                ("Headphone Source", "DAC"),
                ("Headphone Source Playback Route", "DAC"),
                ("Line Out Source", "Mono Differential"),
                ("Line Out Source Playback Route", "Mono Differential"),
                ("AIF1 DA0 Stereo", "Stereo"),
                ("AIF1 DA0 Stereo Playback Route", "Stereo"),
            ];
            for (elem_name, target_item) in routes {
                let id = alsa::mixer::SelemId::new(elem_name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    if selem.is_enumerated() {
                        if let Ok(count) = selem.get_enum_items() {
                            for idx in 0..count {
                                if let Ok(item) = selem.get_enum_item_name(idx) {
                                    if item.eq_ignore_ascii_case(target_item) {
                                        let _ = selem.set_enum_item(
                                            alsa::mixer::SelemChannelId::FrontLeft,
                                            idx,
                                        );
                                        let _ = selem.set_enum_item(
                                            alsa::mixer::SelemChannelId::FrontRight,
                                            idx,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Updates the PinePhone Allwinner A64 analog `Headphone` and `Line Out` volume from Thread 1
    /// when the physical Volume Up / Volume Down buttons are pressed (`master_vol` in `0..=255`).
    /// Keeps digital `DAC` and `AIF1 DA0` locked at `160` (`0 dB`) and caps analog gain at `80%`
    /// of hardware range so the internal class-D speaker amp never clips even at 100% volume.
    pub fn set_pinephone_hw_volume(master_vol: u8) {
        if let Ok(mixer) = alsa::mixer::Mixer::new("hw:0", false) {
            let scaled_pct = ((master_vol as i64) * 80) / 255;
            for name in ["Headphone", "Line Out"] {
                let id = alsa::mixer::SelemId::new(name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    if master_vol == 0 {
                        let _ = selem.set_playback_switch_all(0);
                    } else {
                        let _ = selem.set_playback_switch_all(1);
                        if selem.has_playback_volume() {
                            let (min, max) = selem.get_playback_volume_range();
                            let target = min + ((max - min) * scaled_pct) / 100;
                            let _ = selem.set_playback_volume_all(target);
                        }
                    }
                }
            }
        }
    }

    fn try_open_pcm(device_name: &str) -> Option<PCM> {
        let pcm = PCM::new(device_name, Direction::Playback, false).ok()?;
        {
            let hwp = HwParams::any(&pcm).ok()?;
            hwp.set_channels(CHANNELS as u32).ok()?;
            hwp.set_rate(SAMPLE_RATE, ValueOr::Nearest).ok()?;
            hwp.set_format(Format::s16()).ok()?;
            hwp.set_access(Access::RWInterleaved).ok()?;
            let _ = hwp.set_period_size(BUFFER_FRAMES as i64, ValueOr::Nearest);
            let _ = hwp.set_buffer_size((BUFFER_FRAMES * 4) as i64);
            pcm.hw_params(&hwp).ok()?;
        }
        let _ = pcm.prepare();
        Some(pcm)
    }

    /// Writes a 128-frame stereo `i16` block to the active PipeWire or ALSA PCM device.
    /// Automatically catches `-ENODEV` / `-EPIPE` on USB-C hub unplug to fall back to `"hw:0,0"`
    /// without crashing or halting the sequencer clock.
    pub fn write_block(&mut self, pcm_buf: &[i16; BLOCK_SAMPLES]) {
        if let Some(ref mut pulse_stream) = self.pulse {
            if !pulse_stream.write_block(pcm_buf) {
                self.pulse = None;
                self.xrun_recoveries = self.xrun_recoveries.wrapping_add(1);
                if let Some(reopened) = PulseSimpleStream::try_open() {
                    self.pulse = Some(reopened);
                } else {
                    self.state = AudioDeviceState::HeadlessDummy;
                }
            }
            return;
        }

        // If in HeadlessDummy during fast PID 1 boot while ALSA modules finish probing, retry every ~0.5s
        if !self.deck_mode && self.state == AudioDeviceState::HeadlessDummy {
            self.usb_reprobe_counter = self.usb_reprobe_counter.wrapping_add(1);
            if self.usb_reprobe_counter >= 188 {
                self.usb_reprobe_counter = 0;
                self.initialize_preferred_device();
            }
        }

        // If we fell back to internal codec on PinePhone, periodically probe for USB-C DAC hot-plug every ~5s
        if !self.deck_mode && self.state == AudioDeviceState::InternalCodecFallback {
            self.usb_reprobe_counter = self.usb_reprobe_counter.wrapping_add(1);
            if self.usb_reprobe_counter >= 1875 {
                self.usb_reprobe_counter = 0;
                if let Some(usb_pcm) = Self::try_open_pcm("hw:1,0") {
                    self.pcm = Some(usb_pcm);
                    self.state = AudioDeviceState::ExternalUsbDac;
                }
            }
        }

        let mut transition: Option<DacRecoveryAction> = None;

        if let Some(ref pcm) = self.pcm {
            if let Ok(io) = pcm.io_i16() {
                if let Err(err) = io.writei(pcm_buf) {
                    let errno = err.errno();
                    let action = classify_alsa_write_error(errno, self.state, false);
                    match action {
                        DacRecoveryAction::RecoverSameDevice => {
                            if pcm.try_recover(err, true).is_ok() {
                                self.xrun_recoveries = self.xrun_recoveries.wrapping_add(1);
                                let _ = io.writei(pcm_buf);
                            } else {
                                transition =
                                    Some(classify_alsa_write_error(errno, self.state, true));
                            }
                        }
                        other => {
                            transition = Some(other);
                        }
                    }
                }
            }
        }

        if let Some(action) = transition {
            match action {
                DacRecoveryAction::FallbackToInternalCodec => {
                    self.xrun_recoveries = self.xrun_recoveries.wrapping_add(1);
                    if let Some(internal_pcm) = Self::try_open_pcm("hw:0,0") {
                        self.pcm = Some(internal_pcm);
                        self.state = AudioDeviceState::InternalCodecFallback;
                    } else {
                        self.pcm = None;
                        self.state = AudioDeviceState::HeadlessDummy;
                    }
                }
                DacRecoveryAction::FallbackToDummy => {
                    self.pcm = None;
                    self.state = AudioDeviceState::HeadlessDummy;
                }
                DacRecoveryAction::RecoverSameDevice => {}
            }
        }
    }
}



// ============================================================================
// Thread 2 Real-Time Audio Engine
// ============================================================================

pub struct AudioEngine {
    pub project: ProjectState,
    pub sequencer: Sequencer,
    pub sample_bank: RealtimeSampleBank,
    pub midi_q: MidiOutQueue,
    pub voice_q: VoiceEventQueue,
    pub channels: [TrackDspChannel; NUM_TRACKS],
    pub master_mix: AlignedStereoBlock,
    pub track_scratch: AlignedStereoBlock,
    pub delay_send_bus: AlignedStereoBlock,
    pub reverb_send_bus: AlignedStereoBlock,
    pub fx_bus: MasterFxBus,
    pub pcm_out: [i16; BLOCK_SAMPLES],
    pub track_peaks: [(f32, f32); NUM_TRACKS],
    pub callback_counter: u64,
}

impl AudioEngine {
    pub fn new(initial_project: ProjectState) -> Self {
        Self {
            project: initial_project,
            sequencer: Sequencer::new(SAMPLE_RATE),
            sample_bank: RealtimeSampleBank::default(),
            midi_q: MidiOutQueue::default(),
            voice_q: VoiceEventQueue::default(),
            channels: std::array::from_fn(|_| TrackDspChannel::new()),
            master_mix: AlignedStereoBlock::default(),
            track_scratch: AlignedStereoBlock::default(),
            delay_send_bus: AlignedStereoBlock::default(),
            reverb_send_bus: AlignedStereoBlock::default(),
            fx_bus: MasterFxBus::new(),
            pcm_out: [0i16; BLOCK_SAMPLES],
            track_peaks: [(0.0, 0.0); NUM_TRACKS],
            callback_counter: 0,
        }
    }

    /// Processes incoming lock-free commands from Thread 1 without allocating or deallocating heap memory.
    /// Returns `false` if `UiToAudioMsg::Shutdown` was received.
    pub fn drain_commands(&mut self, ipc: &AudioIpcHandle) -> bool {
        while let Ok(msg) = ipc.rx_cmd.try_recv() {
            match msg {
                UiToAudioMsg::Shutdown => return false,
                UiToAudioMsg::TransportPlay { start_song_row } => {
                    self.sequencer
                        .start(&self.project, start_song_row, &mut self.midi_q);
                }
                UiToAudioMsg::TransportStop => {
                    self.sequencer.stop(&mut self.midi_q, &mut self.voice_q);
                    for ch in &mut self.channels {
                        ch.hard_cut();
                    }
                }
                UiToAudioMsg::SetTrackMute { track, muted } => {
                    let t = (track as usize) % NUM_TRACKS;
                    self.sequencer.tracks[t].muted = muted;
                    if muted {
                        self.channels[t].hard_cut();
                    }
                }
                UiToAudioMsg::TriggerPreviewNote {
                    track,
                    note,
                    velocity,
                    instrument,
                } => {
                    let t = (track as usize) % NUM_TRACKS;
                    self.sequencer.tracks[t].last_note = note;
                    self.sequencer.tracks[t].last_instrument = instrument;
                    self.channels[t].trigger_note(
                        note,
                        velocity,
                        instrument,
                        0,
                        0,
                        &self.project,
                        &self.sample_bank,
                    );
                }
                UiToAudioMsg::StopPreviewTrack { track } => {
                    let t = (track as usize) % NUM_TRACKS;
                    self.channels[t].note_off();
                    self.sequencer.tracks[t].last_note = EMPTY_U8;
                }
                UiToAudioMsg::SendMidiCcImmediate { channel, cc, value } => {
                    self.midi_q
                        .push(MidiPacket::control_change(0, channel, cc, value));
                }
                UiToAudioMsg::LoadDrumPadLayer {
                    inst_id,
                    pad_idx,
                    vel_layer,
                    sample,
                } => {
                    if let Some(retired) =
                        self.sample_bank
                            .swap_drum_layer(inst_id, pad_idx, vel_layer, sample)
                    {
                        let _ = ipc
                            .tx_tel
                            .try_send(AudioToUiMsg::RetireSampleBuffer(retired));
                    }
                }
                UiToAudioMsg::LoadSingleSample { inst_id, sample } => {
                    if let Some(retired) = self.sample_bank.swap_single_sample(inst_id, sample) {
                        let _ = ipc
                            .tx_tel
                            .try_send(AudioToUiMsg::RetireSampleBuffer(retired));
                    }
                }
                other => {
                    self.project.apply_delta(&other);
                }
            }
        }
        true
    }

    /// Runs one real-time 128-sample callback (`2.66 ms` deadline @ 48 kHz).
    /// Zero heap allocations, zero mutex locks, zero file I/O.
    pub fn process_callback(&mut self) -> (f32, f32) {
        self.voice_q.clear();

        // 1. Advance sample-accurate 192/24 PPQN Sequencer
        self.sequencer.process_block(
            BUFFER_FRAMES,
            &mut self.project,
            &mut self.midi_q,
            &mut self.voice_q,
        );

        // 2. Dispatch sample-accurate VoiceEvents to the 4 InstrumentEnum DSP engines
        for ev in self.voice_q.as_slice() {
            let t = (ev.track as usize) % NUM_TRACKS;
            match ev.kind {
                VoiceEventKind::TriggerNote {
                    note,
                    velocity,
                    instrument,
                    slice_offset,
                    glide,
                } => {
                    self.channels[t].trigger_note(
                        note,
                        velocity,
                        instrument,
                        slice_offset,
                        glide,
                        &self.project,
                        &self.sample_bank,
                    );
                }
                VoiceEventKind::NoteOff => {
                    self.channels[t].note_off();
                }
                VoiceEventKind::CutVoice => {
                    self.channels[t].hard_cut();
                }
                VoiceEventKind::Retrigger { vol_scale } => {
                    self.channels[t].retrigger(vol_scale, &self.project);
                }
                VoiceEventKind::SetTrackModulation {
                    pitch_semitones,
                    volume_scale,
                    cutoff_override,
                    resonance_override,
                    pan_override,
                } => {
                    self.channels[t].pitch_mod_semitones = pitch_semitones;
                    self.channels[t].vol_scale = volume_scale;
                    self.channels[t].cutoff_override = cutoff_override;
                    self.channels[t].resonance_override = resonance_override;
                    self.channels[t].pan_override = pan_override;
                }
            }
        }

        // 3. Render & mix 8 tracks into 16-byte aligned NEON stereo buffer + Master Send FX buses
        self.master_mix.clear();
        self.delay_send_bus.clear();
        self.reverb_send_bus.clear();

        for t in 0..NUM_TRACKS {
            self.track_scratch.clear();
            let (pk_l, pk_r) = self.channels[t].render_block(
                &self.project,
                &self.sample_bank,
                &mut self.track_scratch,
            );
            // Smooth peak meter decay for the UI HUD
            self.track_peaks[t].0 = (self.track_peaks[t].0 * 0.92).max(pk_l);
            self.track_peaks[t].1 = (self.track_peaks[t].1 * 0.92).max(pk_r);
            // -8.4 dB polyphonic summing headroom across 8 tracks + FX return bus
            self.master_mix.accumulate_scaled(&self.track_scratch, 0.38);

            let inst_idx = (self.channels[t].active_instrument as usize) % NUM_INSTRUMENTS;
            let inst = &self.project.instruments[inst_idx];
            if inst.delay_send > 0 {
                let d_gain = ((inst.delay_send as f32) / 255.0) * 0.30;
                self.delay_send_bus
                    .accumulate_scaled(&self.track_scratch, d_gain);
            }
            if inst.reverb_send > 0 {
                let r_gain = ((inst.reverb_send as f32) / 255.0) * 0.30;
                self.reverb_send_bus
                    .accumulate_scaled(&self.track_scratch, r_gain);
            }
        }

        // 3b. Process Stereo Ping-Pong Delay & Hall Reverb Send Bus (Zero-Alloc)
        self.fx_bus.process_and_return(
            self.project.tempo_bpm,
            &self.delay_send_bus,
            &self.reverb_send_bus,
            &mut self.master_mix,
        );

        // 4. Quantize master stereo bus to S16_LE PCM with soft-clipping
        let master_gain = (self.project.master_volume as f32) / 255.0;
        let (master_l, master_r) = self
            .master_mix
            .quantize_to_i16(master_gain, &mut self.pcm_out);

        self.callback_counter = self.callback_counter.wrapping_add(1);
        (master_l, master_r)
    }
}

/// Spawns Thread 2 (Real-Time Audio & MIDI) on Isolated Cores 2-3 with `SCHED_FIFO` priority 95.
pub fn spawn_audio_thread(
    initial_project: ProjectState,
    ipc: AudioIpcHandle,
    deck_mode: bool,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("pine-rt-audio".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let _rt_status = configure_realtime_thread();
            let mut dac = AlsaDualDacDriver::new(deck_mode);
            let mut midi_drv = AlsaMidiDriver::new();
            let mut engine = Box::new(AudioEngine::new(initial_project));

            let block_duration = Duration::from_nanos(2_666_667); // 128 / 48000s = 2.666667 ms
            let mut next_deadline = Instant::now() + block_duration;

            loop {
                engine.midi_q.clear();

                if !engine.drain_commands(&ipc) {
                    break;
                }

                let (master_l, master_r) = engine.process_callback();

                // Flush hardware 5-pin DIN MIDI (steady 24-PPQN 0xF8 + Notes/CCs)
                midi_drv.flush_queue(&engine.midi_q);

                // Write 128-frame audio buffer to ALSA PCM (or pace via monotonic clock if HeadlessDummy)
                if dac.state != AudioDeviceState::HeadlessDummy {
                    dac.write_block(&engine.pcm_out);
                } else {
                    dac.write_block(&engine.pcm_out);
                    let now = Instant::now();
                    if next_deadline > now {
                        thread::sleep(next_deadline - now);
                    }
                    next_deadline += block_duration;
                }

                // Emit ~60 Hz Telemetry snapshot to Thread 1 (every 6 blocks = 16.0 ms)
                if engine.callback_counter % 6 == 0 {
                    let tel = AudioTelemetry {
                        playing: engine.sequencer.playing,
                        tempo_bpm: engine.project.tempo_bpm,
                        midi_clock_pulses: engine.sequencer.midi_clock_pulses,
                        dac_state: dac.state,
                        midi_connected: midi_drv.is_connected(),
                        tracks: engine.sequencer.snapshot_tracks(&engine.track_peaks),
                        master_peak_l: master_l,
                        master_peak_r: master_r,
                        xrun_recoveries: dac.xrun_recoveries,
                    };
                    let _ = ipc.tx_tel.try_send(AudioToUiMsg::Telemetry(tel));
                }
            }
        })
        .expect("Failed to spawn pine-rt-audio thread")
}

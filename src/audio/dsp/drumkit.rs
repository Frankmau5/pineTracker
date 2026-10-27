use crate::audio::dsp::svf::CytomicSvf;
use crate::audio::sequencer::{BUFFER_FRAMES, SAMPLE_RATE};
use crate::audio::AlignedStereoBlock;
use crate::ipc::SampleBuffer;
use crate::model::{
    note_to_drum_pad, velocity_to_layer, DrumKitParams, DRUM_KIT_VOICES, DRUM_PADS, EMPTY_U8,
    VEL_LAYERS,
};

/// 1.5 ms @ 48 kHz = 72 audio frames linear crossfade on same-pad retrigger.
pub const RETRIG_CROSSFADE_FRAMES: u16 = 72;

/// 3.0 ms @ 48 kHz = 144 audio frames linear choke ramp when Closed/Pedal Hat chokes Open Hat.
pub const HI_HAT_CHOKE_FRAMES: u16 = 144;

#[derive(Copy, Clone, Debug, Default)]
pub struct DrumVoiceSlot {
    pub active: bool,
    pub pad_idx: u8,
    pub vel_layer: u8,
    pub age_frames: u64,
    pub pos_frames: f64,
    pub step_ratio: f64,
    pub gain_l: f32,
    pub gain_r: f32,

    // Same-pad 1.5ms (72-frame) retrigger crossfade tail state
    pub retrig_fade_left: u16,
    pub prev_pos_frames: f64,
    pub prev_vel_layer: u8,
    pub prev_gain_l: f32,
    pub prev_gain_r: f32,

    // 3.0ms (144-frame) Hi-Hat choke group linear attenuation ramp
    pub choking: bool,
    pub choke_frames_left: u16,

    // Built-in fallback synthesis state when no external .wav is loaded in slot
    pub synth_phase: f32,
    pub synth_pitch_env: f32,
    pub synth_amp_env: f32,
    pub synth_rng: u32,
}

/// 8-Voice internal sub-pool for a `DrumKit` instrument.
#[derive(Clone, Debug)]
pub struct DrumKitEngine {
    pub voices: [DrumVoiceSlot; DRUM_KIT_VOICES],
    pub filter: CytomicSvf,
}

impl Default for DrumKitEngine {
    fn default() -> Self {
        Self {
            voices: [DrumVoiceSlot::default(); DRUM_KIT_VOICES],
            filter: CytomicSvf::new(),
        }
    }
}

impl DrumKitEngine {
    /// Triggers a drum pad mapped from `note` (`C-4`..`B-4` = pads `0..11`) and `velocity` (`00..FF`).
    /// Enforces:
    /// 1. **3.0 ms (144-sample) Hi-Hat Choke Group** on any active voice playing `pad.choke_target`.
    /// 2. **1.5 ms (72-sample) Same-Pad Retrigger Crossfade** when retriggering an already active pad.
    /// 3. **8-Voice Sub-Pool Allocation** (stealing the oldest or quietest voice if all 8 are busy).
    pub fn trigger_note(
        &mut self,
        note: u8,
        velocity: u8,
        slice_offset: u8,
        kit: &DrumKitParams,
        layers: &[[Option<SampleBuffer>; VEL_LAYERS]; DRUM_PADS],
    ) {
        let Some(pad_idx) = note_to_drum_pad(note) else {
            return;
        };
        let vel_layer = velocity_to_layer(velocity);
        let pad_cfg = &kit.pads[pad_idx];

        // 1. Hi-Hat Choke Group: if this pad chokes another pad (e.g. Closed/Pedal Hat -> Open Hat),
        // transition all active voices on `choke_target` into a 3.0ms (144-sample) linear fade-out.
        if pad_cfg.choke_target != EMPTY_U8 {
            let target_pad = pad_cfg.choke_target;
            for v in &mut self.voices {
                if v.active && v.pad_idx == target_pad && !v.choking {
                    v.choking = true;
                    v.choke_frames_left = HI_HAT_CHOKE_FRAMES;
                }
            }
        }

        let master_scale = (kit.master_level as f32 / 255.0)
            * (pad_cfg.level as f32 / 255.0)
            * (velocity as f32 / 255.0);
        let pan_norm = (pad_cfg.pan as f32 / 255.0).clamp(0.0, 1.0);
        let gain_l = (1.0 - pan_norm).sqrt() * master_scale;
        let gain_r = pan_norm.sqrt() * master_scale;

        let semitone_ratio = 2.0_f64.powf(pad_cfg.tune_semitones as f64 / 12.0);
        let start_frame = if slice_offset > 0 {
            if let Some(ref sb) = layers[pad_idx][vel_layer] {
                (sb.frame_count() as f64) * (slice_offset as f64 / 256.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        // 2. Check if the same pad is already active in the 8-voice sub-pool -> 1.5ms (72-frame) Crossfade!
        if let Some(existing) = self
            .voices
            .iter_mut()
            .find(|v| v.active && v.pad_idx == pad_idx as u8)
        {
            existing.retrig_fade_left = RETRIG_CROSSFADE_FRAMES;
            existing.prev_pos_frames = existing.pos_frames;
            existing.prev_vel_layer = existing.vel_layer;
            existing.prev_gain_l = existing.gain_l * existing.synth_amp_env.max(0.25);
            existing.prev_gain_r = existing.gain_r * existing.synth_amp_env.max(0.25);

            existing.vel_layer = vel_layer as u8;
            existing.age_frames = 0;
            existing.pos_frames = start_frame;
            existing.step_ratio = semitone_ratio;
            existing.gain_l = gain_l;
            existing.gain_r = gain_r;
            existing.choking = false;
            existing.choke_frames_left = 0;
            existing.synth_phase = 0.0;
            existing.synth_pitch_env = 1.0;
            existing.synth_amp_env = 1.0;
            return;
        }

        // 3. Allocate a free slot in the 8-voice sub-pool, or steal the oldest active slot
        let slot_idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| v.age_frames)
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });

        let slot = &mut self.voices[slot_idx];
        *slot = DrumVoiceSlot {
            active: true,
            pad_idx: pad_idx as u8,
            vel_layer: vel_layer as u8,
            age_frames: 0,
            pos_frames: start_frame,
            step_ratio: semitone_ratio,
            gain_l,
            gain_r,
            retrig_fade_left: 0,
            prev_pos_frames: 0.0,
            prev_vel_layer: 0,
            prev_gain_l: 0.0,
            prev_gain_r: 0.0,
            choking: false,
            choke_frames_left: 0,
            synth_phase: 0.0,
            synth_pitch_env: 1.0,
            synth_amp_env: 1.0,
            synth_rng: 0x9E37_79B9 ^ ((pad_idx as u32) << 16) ^ (velocity as u32),
        };
    }

    pub fn cut_all(&mut self) {
        for v in &mut self.voices {
            v.active = false;
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Renders all active voices in the 8-voice `DrumKit` sub-pool into `out_block`.
    pub fn render_block(
        &mut self,
        kit: &DrumKitParams,
        layers: &[[Option<SampleBuffer>; VEL_LAYERS]; DRUM_PADS],
        cutoff_override: Option<u8>,
        res_override: Option<u8>,
        vol_scale: f32,
        out_block: &mut AlignedStereoBlock,
    ) -> (f32, f32) {
        self.filter
            .configure_from_params(&kit.filter, cutoff_override, res_override, 0.0);

        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;

        for frame in 0..BUFFER_FRAMES {
            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;

            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                v.age_frames = v.age_frames.wrapping_add(1);

                // Compute 3.0ms (144-sample) Hi-Hat Choke attenuation factor
                let choke_gain = if v.choking {
                    if v.choke_frames_left == 0 {
                        v.active = false;
                        continue;
                    }
                    let g = (v.choke_frames_left as f32) / (HI_HAT_CHOKE_FRAMES as f32);
                    v.choke_frames_left -= 1;
                    if v.choke_frames_left == 0 {
                        v.active = false;
                    }
                    g
                } else {
                    1.0
                };

                // Compute 1.5ms (72-sample) same-pad retrigger crossfade factors
                let (new_xfade, old_xfade) = if v.retrig_fade_left > 0 {
                    let old_w = (v.retrig_fade_left as f32) / (RETRIG_CROSSFADE_FRAMES as f32);
                    let new_w = 1.0 - old_w;
                    v.retrig_fade_left -= 1;
                    (new_w, old_w)
                } else {
                    (1.0, 0.0)
                };

                let pad = (v.pad_idx as usize) % DRUM_PADS;
                let lay = (v.vel_layer as usize) % VEL_LAYERS;

                // Sample new hit (either from .wav SampleBuffer in RAM or fallback drum synth)
                let (s_l, s_r, finished) =
                    if let Some(ref sb) = layers[pad][lay] {
                        Self::sample_pcm_linear(sb, v.pos_frames)
                    } else {
                        Self::sample_fallback_drum_synth(v)
                    };

                if finished && v.retrig_fade_left == 0 {
                    v.active = false;
                    continue;
                }

                v.pos_frames += v.step_ratio;
                let mut voice_l = s_l * v.gain_l * new_xfade;
                let mut voice_r = s_r * v.gain_r * new_xfade;

                // Mix outgoing 1.5ms retrigger crossfade tail if active
                if old_xfade > 0.0 {
                    let prev_lay = (v.prev_vel_layer as usize) % VEL_LAYERS;
                    let (old_l, old_r) = if let Some(ref sb) = layers[pad][prev_lay] {
                        let (ol, or, _) = Self::sample_pcm_linear(sb, v.prev_pos_frames);
                        (ol, or)
                    } else {
                        (s_l, s_r)
                    };
                    v.prev_pos_frames += v.step_ratio;
                    voice_l += old_l * v.prev_gain_l * old_xfade;
                    voice_r += old_r * v.prev_gain_r * old_xfade;
                }

                sum_l += voice_l * choke_gain * vol_scale;
                sum_r += voice_r * choke_gain * vol_scale;
            }

            let (flt_l, flt_r) = self.filter.process_stereo(sum_l, sum_r);
            out_block.0[frame * 2] += flt_l;
            out_block.0[frame * 2 + 1] += flt_r;
            peak_l = peak_l.max(flt_l.abs());
            peak_r = peak_r.max(flt_r.abs());
        }

        (peak_l, peak_r)
    }

    #[inline]
    fn sample_pcm_linear(sb: &SampleBuffer, pos_frames: f64) -> (f32, f32, bool) {
        let total_frames = sb.frame_count();
        let idx0 = pos_frames as usize;
        if idx0 >= total_frames {
            return (0.0, 0.0, true);
        }
        let idx1 = (idx0 + 1).min(total_frames.saturating_sub(1));
        let frac = (pos_frames - (idx0 as f64)) as f32;
        let inv = 1.0 / 32768.0;

        if sb.channels >= 2 {
            let l0 = sb.data[idx0 * 2] as f32 * inv;
            let r0 = sb.data[idx0 * 2 + 1] as f32 * inv;
            let l1 = sb.data[idx1 * 2] as f32 * inv;
            let r1 = sb.data[idx1 * 2 + 1] as f32 * inv;
            (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac, false)
        } else {
            let m0 = sb.data[idx0] as f32 * inv;
            let m1 = sb.data[idx1] as f32 * inv;
            let m = m0 + (m1 - m0) * frac;
            (m, m, false)
        }
    }

    #[inline]
    fn sample_fallback_drum_synth(v: &mut DrumVoiceSlot) -> (f32, f32, bool) {
        if v.synth_amp_env < 0.0005 {
            return (0.0, 0.0, true);
        }
        let pad = v.pad_idx % 12;
        v.synth_pitch_env *= 0.992;

        // Xorshift PRNG for snare/hat noise
        let mut x = v.synth_rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v.synth_rng = x;
        let noise = ((x & 0xFFFF) as f32 / 32768.0) - 1.0;

        let (decay, sig) = match pad {
            0 => {
                // Kick
                let f = 52.0 + 185.0 * v.synth_pitch_env * v.synth_pitch_env;
                v.synth_phase = (v.synth_phase + f / SAMPLE_RATE as f32).fract();
                (0.9986, (v.synth_phase * std::f32::consts::TAU).sin() * 1.25)
            }
            1 => {
                // Rimshot
                let f = 420.0 + 300.0 * v.synth_pitch_env;
                v.synth_phase = (v.synth_phase + f / SAMPLE_RATE as f32).fract();
                (0.9955, (v.synth_phase * std::f32::consts::TAU).sin())
            }
            2 | 3 => {
                // Snare / Clap
                let f = 185.0 + 95.0 * v.synth_pitch_env;
                v.synth_phase = (v.synth_phase + f / SAMPLE_RATE as f32).fract();
                let body = (v.synth_phase * std::f32::consts::TAU).sin() * 0.5;
                (0.9980, body + noise * 0.75)
            }
            5 | 6 => {
                // Closed / Pedal Hat
                v.synth_phase = (v.synth_phase + 5100.0 / SAMPLE_RATE as f32).fract();
                let metallic = if v.synth_phase > 0.5 { 0.3 } else { -0.3 };
                (0.9945, (noise * 0.75 + metallic) * 0.85)
            }
            8 | 10 | 11 => {
                // Open Hat / Crash / Ride
                v.synth_phase = (v.synth_phase + 4400.0 / SAMPLE_RATE as f32).fract();
                let metallic = if v.synth_phase > 0.5 { 0.35 } else { -0.35 };
                (0.99935, (noise * 0.70 + metallic) * 0.85)
            }
            _ => {
                // Low / Mid / High Toms
                let base = 95.0 + (pad as f32) * 22.0;
                let f = base * (1.0 + 0.7 * v.synth_pitch_env);
                v.synth_phase = (v.synth_phase + f / SAMPLE_RATE as f32).fract();
                (0.9984, (v.synth_phase * std::f32::consts::TAU).sin())
            }
        };

        v.synth_amp_env *= decay;
        let out = sig * v.synth_amp_env;
        (out, out, false)
    }
}

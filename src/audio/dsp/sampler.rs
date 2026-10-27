use crate::audio::dsp::svf::{AdsrEnvelope, CytomicSvf};
use crate::audio::sequencer::{BUFFER_FRAMES, SAMPLE_RATE};
use crate::audio::AlignedStereoBlock;
use crate::ipc::SampleBuffer;
use crate::model::{note_to_freq_hz, SampleLoopMode, SingleSampleParams};

#[derive(Clone, Debug, Default)]
pub struct SingleSampleEngine {
    pub active: bool,
    pub pos_frames: f64,
    pub direction: f64, // +1.0 forward, -1.0 reverse (for PingPong loop)
    pub current_note: f32,
    pub target_note: f32,
    pub glide_rate: f32,
    pub velocity_gain: f32,
    pub amp_env: AdsrEnvelope,
    pub filter_env: AdsrEnvelope,
    pub filter: CytomicSvf,
    pub fallback_phase: f32,
}

impl SingleSampleEngine {
    pub fn new() -> Self {
        Self {
            active: false,
            pos_frames: 0.0,
            direction: 1.0,
            current_note: 48.0,
            target_note: 48.0,
            glide_rate: 1.0,
            velocity_gain: 0.8,
            amp_env: AdsrEnvelope::default(),
            filter_env: AdsrEnvelope::default(),
            filter: CytomicSvf::new(),
            fallback_phase: 0.0,
        }
    }

    pub fn trigger(
        &mut self,
        note: u8,
        velocity: u8,
        slice_offset: u8,
        glide: u8,
        params: &SingleSampleParams,
        sample: Option<&SampleBuffer>,
    ) {
        self.target_note = note as f32;
        if !self.active || glide == 0 {
            self.current_note = self.target_note;
            self.glide_rate = 1.0;
        } else {
            let glide_sec = 0.005 + (glide as f32 / 255.0) * 0.45;
            self.glide_rate = (1.0 / (glide_sec * SAMPLE_RATE as f32)).clamp(0.0001, 1.0);
        }

        self.active = true;
        self.direction = 1.0;
        self.velocity_gain = (velocity as f32 / 255.0).clamp(0.05, 1.0);
        self.amp_env.trigger(&params.amp_env);
        self.filter_env.trigger(&params.filter_env);

        // Compute starting frame from `slice_offset` (`OFS` FX) and `params.slice_count`
        if let Some(sb) = sample {
            let total = sb.frame_count().max(1) as f64;
            let norm_ofs = if params.slice_count > 1 && slice_offset < params.slice_count {
                (slice_offset as f64) / (params.slice_count as f64)
            } else {
                (slice_offset as f64) / 256.0
            };
            self.pos_frames = (total * norm_ofs).clamp(0.0, (total - 1.0).max(0.0));
        } else {
            self.pos_frames = 0.0;
        }
    }

    pub fn note_off(&mut self) {
        self.amp_env.note_off();
        self.filter_env.note_off();
    }

    pub fn hard_cut(&mut self) {
        self.active = false;
        self.amp_env.hard_cut();
        self.filter_env.hard_cut();
    }

    pub fn retrigger(&mut self, vol_scale: f32, params: &SingleSampleParams) {
        self.pos_frames = 0.0;
        self.direction = 1.0;
        self.velocity_gain = (self.velocity_gain * vol_scale).clamp(0.05, 1.2);
        self.amp_env.trigger(&params.amp_env);
        self.filter_env.trigger(&params.filter_env);
    }

    pub fn render_block(
        &mut self,
        params: &SingleSampleParams,
        sample: Option<&SampleBuffer>,
        pitch_mod_semitones: f32,
        vol_scale: f32,
        cutoff_override: Option<u8>,
        res_override: Option<u8>,
        pan_norm: f32,
        master_inst_vol: f32,
        out_block: &mut AlignedStereoBlock,
    ) -> (f32, f32) {
        if !self.active || !self.amp_env.is_active() {
            self.active = false;
            return (0.0, 0.0);
        }

        self.amp_env.configure(&params.amp_env);
        self.filter_env.configure(&params.filter_env);

        let f_env_val = self.filter_env.level;
        self.filter.configure_from_params(
            &params.filter,
            cutoff_override,
            res_override,
            f_env_val,
        );

        let gain_base = self.velocity_gain * vol_scale * master_inst_vol;
        let gain_l = (1.0 - pan_norm).sqrt() * gain_base;
        let gain_r = pan_norm.sqrt() * gain_base;

        let fine_semi = (params.fine_tune as f32) / 100.0;
        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;

        for frame in 0..BUFFER_FRAMES {
            // Smooth pitch glide
            self.current_note += (self.target_note - self.current_note) * self.glide_rate;
            let amp = self.amp_env.next_sample();
            let _f_env = self.filter_env.next_sample();

            if !self.amp_env.is_active() {
                self.active = false;
                break;
            }

            let semitone_diff =
                (self.current_note - params.root_note as f32) + fine_semi + pitch_mod_semitones;
            let pitch_ratio = 2.0_f64.powf((semitone_diff as f64) / 12.0);

            let (raw_l, raw_r) = if let Some(sb) = sample {
                let total = sb.frame_count();
                if total == 0 {
                    (0.0, 0.0)
                } else {
                    let sr_ratio = (sb.sample_rate as f64) / (SAMPLE_RATE as f64);
                    let step = pitch_ratio * sr_ratio * self.direction;

                    let loop_start_f =
                        ((params.loop_start as f64 / 255.0) * (total as f64)).min((total - 1) as f64);
                    let loop_end_f = (((params.loop_end as f64 / 255.0) * (total as f64))
                        .max(loop_start_f + 4.0))
                    .min(total as f64);

                    if self.pos_frames >= total as f64 {
                        match params.loop_mode {
                            SampleLoopMode::OneShot => {
                                self.active = false;
                                break;
                            }
                            SampleLoopMode::ForwardLoop => {
                                self.pos_frames = loop_start_f;
                            }
                            SampleLoopMode::PingPong => {
                                self.pos_frames = (loop_end_f - 1.0).max(0.0);
                                self.direction = -1.0;
                            }
                        }
                    } else if self.pos_frames >= loop_end_f
                        && params.loop_mode != SampleLoopMode::OneShot
                    {
                        match params.loop_mode {
                            SampleLoopMode::ForwardLoop => {
                                self.pos_frames = loop_start_f
                                    + (self.pos_frames - loop_end_f)
                                        .rem_euclid((loop_end_f - loop_start_f).max(1.0));
                            }
                            SampleLoopMode::PingPong => {
                                self.pos_frames = loop_end_f;
                                self.direction = -1.0;
                            }
                            SampleLoopMode::OneShot => {}
                        }
                    } else if self.pos_frames < loop_start_f && self.direction < 0.0 {
                        self.pos_frames = loop_start_f;
                        self.direction = 1.0;
                    }

                    let idx0 = (self.pos_frames as usize).min(total - 1);
                    let idx1 = (idx0 + 1).min(total - 1);
                    let frac = (self.pos_frames - idx0 as f64) as f32;
                    let inv = 1.0 / 32768.0;

                    let (sl, sr) = if sb.channels >= 2 {
                        let l0 = sb.data[idx0 * 2] as f32 * inv;
                        let r0 = sb.data[idx0 * 2 + 1] as f32 * inv;
                        let l1 = sb.data[idx1 * 2] as f32 * inv;
                        let r1 = sb.data[idx1 * 2 + 1] as f32 * inv;
                        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
                    } else {
                        let m0 = sb.data[idx0] as f32 * inv;
                        let m1 = sb.data[idx1] as f32 * inv;
                        let m = m0 + (m1 - m0) * frac;
                        (m, m)
                    };

                    self.pos_frames += step;
                    (sl, sr)
                }
            } else {
                // Built-in rich saw+sub waveform if no external WAV buffer is loaded yet
                let freq = note_to_freq_hz(self.current_note + pitch_mod_semitones + fine_semi);
                let inc = (freq / SAMPLE_RATE as f32).clamp(0.0001, 0.45);
                self.fallback_phase = (self.fallback_phase + inc).fract();
                let saw = self.fallback_phase * 2.0 - 1.0;
                (saw * 0.7, saw * 0.7)
            };

            let (flt_l, flt_r) = self.filter.process_stereo(raw_l * amp, raw_r * amp);
            let out_l = flt_l * gain_l;
            let out_r = flt_r * gain_r;

            out_block.0[frame * 2] += out_l;
            out_block.0[frame * 2 + 1] += out_r;
            peak_l = peak_l.max(out_l.abs());
            peak_r = peak_r.max(out_r.abs());
        }

        (peak_l, peak_r)
    }
}

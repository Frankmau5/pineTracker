use crate::audio::dsp::svf::{AdsrEnvelope, CytomicSvf};
use crate::audio::sequencer::{BUFFER_FRAMES, SAMPLE_RATE};
use crate::audio::AlignedStereoBlock;
use crate::model::{note_to_freq_hz, InternalSynthParams, SynthMode, VaWaveform};

/// Polynomial Band-Limited Step (PolyBLEP) residual for anti-aliased Saw & Pulse oscillators.
#[inline]
pub fn poly_blep(mut t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        t /= dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct FmOperatorState {
    pub phase: f32,
    pub env_level: f32,
    pub attack_inc: f32,
    pub decay_mul: f32,
    pub in_attack: bool,
    pub last_out: f32,
}

#[derive(Clone, Debug)]
pub struct InternalSynthEngine {
    pub active: bool,
    pub current_note: f32,
    pub target_note: f32,
    pub glide_rate: f32,
    pub velocity_gain: f32,

    // Virtual Analog Oscillator State
    pub osc1_phase: f32,
    pub osc2_phase: f32,
    pub sub_phase: f32,
    pub tri_integrator1: f32,
    pub tri_integrator2: f32,
    pub rng_state: u32,

    // 4-Operator FM State
    pub fm_ops: [FmOperatorState; 4],

    // Envelopes & Resonant SVF
    pub amp_env: AdsrEnvelope,
    pub mod_env: AdsrEnvelope,
    pub filter: CytomicSvf,
}

impl Default for InternalSynthEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InternalSynthEngine {
    pub fn new() -> Self {
        Self {
            active: false,
            current_note: 48.0,
            target_note: 48.0,
            glide_rate: 1.0,
            velocity_gain: 0.8,
            osc1_phase: 0.0,
            osc2_phase: 0.0,
            sub_phase: 0.0,
            tri_integrator1: 0.0,
            tri_integrator2: 0.0,
            rng_state: 0xA5A5_5A5A,
            fm_ops: [FmOperatorState::default(); 4],
            amp_env: AdsrEnvelope::default(),
            mod_env: AdsrEnvelope::default(),
            filter: CytomicSvf::new(),
        }
    }

    pub fn trigger(&mut self, note: u8, velocity: u8, glide: u8, params: &InternalSynthParams) {
        self.target_note = note as f32;
        if !self.active || glide == 0 {
            self.current_note = self.target_note;
            self.glide_rate = 1.0;
        } else {
            let glide_sec = 0.005 + (glide as f32 / 255.0) * 0.45;
            self.glide_rate = (1.0 / (glide_sec * SAMPLE_RATE as f32)).clamp(0.0001, 1.0);
        }

        self.active = true;
        self.velocity_gain = (velocity as f32 / 255.0).clamp(0.05, 1.0);
        self.amp_env.trigger(&params.amp_env);
        self.mod_env.trigger(&params.mod_env);

        let fs = SAMPLE_RATE as f32;
        for (i, op_cfg) in params.fm_ops.iter().enumerate() {
            let att_sec = 0.0005 + (op_cfg.attack as f32 / 255.0).powi(2) * 1.2;
            let dec_sec = 0.010 + (op_cfg.decay as f32 / 255.0).powi(2) * 2.5;
            self.fm_ops[i].in_attack = true;
            self.fm_ops[i].env_level = self.fm_ops[i].env_level.min(0.1);
            self.fm_ops[i].attack_inc = (1.0 / (att_sec * fs)).clamp(0.00001, 1.0);
            self.fm_ops[i].decay_mul = (-1.0 / (dec_sec * fs)).exp();
        }
    }

    pub fn note_off(&mut self) {
        self.amp_env.note_off();
        self.mod_env.note_off();
    }

    pub fn hard_cut(&mut self) {
        self.active = false;
        self.amp_env.hard_cut();
        self.mod_env.hard_cut();
    }

    pub fn retrigger(&mut self, vol_scale: f32, params: &InternalSynthParams) {
        self.velocity_gain = (self.velocity_gain * vol_scale).clamp(0.05, 1.2);
        self.amp_env.trigger(&params.amp_env);
        self.mod_env.trigger(&params.mod_env);
        for op in &mut self.fm_ops {
            op.in_attack = true;
        }
    }

    pub fn render_block(
        &mut self,
        params: &InternalSynthParams,
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
        self.mod_env.configure(&params.mod_env);

        let mod_val = self.mod_env.level;
        self.filter.configure_from_params(
            &params.filter,
            cutoff_override,
            res_override,
            mod_val,
        );

        let gain_base = self.velocity_gain * vol_scale * master_inst_vol * 0.42;
        let gain_l = (1.0 - pan_norm).sqrt() * gain_base;
        let gain_r = pan_norm.sqrt() * gain_base;

        let pw = (params.pulse_width as f32 / 255.0).clamp(0.08, 0.92);
        let osc_mix = (params.osc_mix as f32 / 255.0).clamp(0.0, 1.0);
        let sub_gain = (params.sub_osc_level as f32 / 255.0) * 0.55;
        let osc2_offset_semi =
            (params.osc2_semitones as f32) + (params.osc2_detune as f32) / 100.0;

        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;

        for frame in 0..BUFFER_FRAMES {
            self.current_note += (self.target_note - self.current_note) * self.glide_rate;
            let amp = self.amp_env.next_sample();
            let _mod_e = self.mod_env.next_sample();

            if !self.amp_env.is_active() {
                self.active = false;
                break;
            }

            let note1 = self.current_note + pitch_mod_semitones;
            let freq1 = note_to_freq_hz(note1).clamp(20.0, 18_000.0);

            let raw_mono = match params.mode {
                SynthMode::VirtualAnalog => {
                    let dt1 = (freq1 / SAMPLE_RATE as f32).clamp(0.0001, 0.48);
                    let freq2 = note_to_freq_hz(note1 + osc2_offset_semi).clamp(20.0, 18_000.0);
                    let dt2 = (freq2 / SAMPLE_RATE as f32).clamp(0.0001, 0.48);
                    let dt_sub = dt1 * 0.5;

                    let s1 = Self::render_va_osc(
                        params.osc1_wave,
                        &mut self.osc1_phase,
                        dt1,
                        pw,
                        &mut self.tri_integrator1,
                        &mut self.rng_state,
                    );
                    let s2 = Self::render_va_osc(
                        params.osc2_wave,
                        &mut self.osc2_phase,
                        dt2,
                        pw,
                        &mut self.tri_integrator2,
                        &mut self.rng_state,
                    );

                    // -1 Octave PolyBLEP square sub-oscillator
                    let mut sub_sq = if self.sub_phase < 0.5 { 1.0 } else { -1.0 };
                    sub_sq += poly_blep(self.sub_phase, dt_sub);
                    sub_sq -= poly_blep((self.sub_phase + 0.5).fract(), dt_sub);
                    self.sub_phase = (self.sub_phase + dt_sub).fract();

                    s1 * (1.0 - osc_mix) + s2 * osc_mix + sub_sq * sub_gain
                }
                SynthMode::Fm4Op => self.render_fm4op_sample(freq1, params),
            };

            let (flt_l, flt_r) = self.filter.process_stereo(raw_mono * amp, raw_mono * amp);
            let out_l = flt_l * gain_l;
            let out_r = flt_r * gain_r;

            out_block.0[frame * 2] += out_l;
            out_block.0[frame * 2 + 1] += out_r;
            peak_l = peak_l.max(out_l.abs());
            peak_r = peak_r.max(out_r.abs());
        }

        (peak_l, peak_r)
    }

    #[inline]
    fn render_va_osc(
        wave: VaWaveform,
        phase: &mut f32,
        dt: f32,
        pw: f32,
        tri_state: &mut f32,
        rng: &mut u32,
    ) -> f32 {
        let t = *phase;
        *phase = (t + dt).fract();

        match wave {
            VaWaveform::Saw => {
                let mut v = 2.0 * t - 1.0;
                v -= poly_blep(t, dt);
                v
            }
            VaWaveform::Pulse => {
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t + 1.0 - pw).fract(), dt);
                v
            }
            VaWaveform::Triangle => {
                // Leaky integration of band-limited square wave for alias-free triangle
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5).fract(), dt);
                *tri_state = dt * sq + (1.0 - dt) * (*tri_state);
                (*tri_state * 4.0).clamp(-1.0, 1.0)
            }
            VaWaveform::Noise => {
                let mut x = (*rng).max(1);
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *rng = x;
                ((x & 0xFFFF) as f32 / 32768.0) - 1.0
            }
        }
    }

    #[inline]
    fn step_fm_op(
        op: &mut FmOperatorState,
        base_freq: f32,
        ratio_coarse: u8,
        level_byte: u8,
        phase_mod_rad: f32,
    ) -> f32 {
        if op.in_attack {
            op.env_level += op.attack_inc;
            if op.env_level >= 1.0 {
                op.env_level = 1.0;
                op.in_attack = false;
            }
        } else {
            op.env_level *= op.decay_mul;
        }

        let mult = if ratio_coarse == 0 {
            0.5
        } else {
            ratio_coarse as f32
        };
        let dt = ((base_freq * mult) / SAMPLE_RATE as f32).clamp(0.00005, 0.48);
        op.phase = (op.phase + dt).fract();

        let lvl = (level_byte as f32 / 255.0) * op.env_level;
        let out = (op.phase * std::f32::consts::TAU + phase_mod_rad).sin() * lvl;
        op.last_out = out;
        out
    }

    /// Evaluates the 4-operator FM voice across 8 classic DX/M8 routing algorithms.
    #[inline]
    fn render_fm4op_sample(&mut self, base_freq: f32, params: &InternalSynthParams) -> f32 {
        let fb_scale = (params.fm_feedback as f32 / 255.0) * 2.5;
        let fb_rad = self.fm_ops[3].last_out * fb_scale;
        let idx_scale = 3.6_f32; // Radians of phase modulation at full operator level

        let c0 = params.fm_ops[0];
        let c1 = params.fm_ops[1];
        let c2 = params.fm_ops[2];
        let c3 = params.fm_ops[3];

        // Evaluate Operator 4 first (always modulated only by self-feedback)
        let o4 = Self::step_fm_op(&mut self.fm_ops[3], base_freq, c3.ratio_coarse, c3.level, fb_rad);

        match params.fm_algorithm & 0x07 {
            0 => {
                // Alg 0: Cascade 4 -> 3 -> 2 -> 1
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, o4 * idx_scale);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, o3 * idx_scale);
                Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, o2 * idx_scale)
            }
            1 => {
                // Alg 1: (4 + 3) -> 2 -> 1
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, 0.0);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, (o4 + o3) * idx_scale);
                Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, o2 * idx_scale)
            }
            2 => {
                // Alg 2: Dual-Stack (4 -> 3) + (2 -> 1)
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, o4 * idx_scale);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, 0.0);
                let o1 = Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, o2 * idx_scale);
                (o1 + o3) * 0.65
            }
            3 => {
                // Alg 3: 4 -> (3 + 2) -> 1
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, o4 * idx_scale);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, o4 * idx_scale);
                Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, (o3 + o2) * idx_scale)
            }
            4 => {
                // Alg 4: 3-Mod-1 (4 + 3 + 2) -> 1
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, 0.0);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, 0.0);
                Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, (o4 + o3 + o2) * idx_scale)
            }
            5 => {
                // Alg 5: 1-Mod-3: 4 -> (3, 2, 1)
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, o4 * idx_scale);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, o4 * idx_scale);
                let o1 = Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, o4 * idx_scale);
                (o1 + o2 + o3) * 0.5
            }
            6 => {
                // Alg 6: (4 -> 3) + 2 + 1
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, o4 * idx_scale);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, 0.0);
                let o1 = Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, 0.0);
                (o1 + o2 + o3) * 0.5
            }
            _ => {
                // Alg 7: Pure 4-Operator Additive (4 + 3 + 2 + 1)
                let o3 = Self::step_fm_op(&mut self.fm_ops[2], base_freq, c2.ratio_coarse, c2.level, 0.0);
                let o2 = Self::step_fm_op(&mut self.fm_ops[1], base_freq, c1.ratio_coarse, c1.level, 0.0);
                let o1 = Self::step_fm_op(&mut self.fm_ops[0], base_freq, c0.ratio_coarse, c0.level, 0.0);
                (o1 + o2 + o3 + o4) * 0.45
            }
        }
    }
}

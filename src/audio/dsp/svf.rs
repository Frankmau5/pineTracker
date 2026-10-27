use crate::audio::sequencer::SAMPLE_RATE;
use crate::model::{AdsrParams, SvfFilterMode, SvfParams};

// ============================================================================
// ADSR Envelope Generator (Zero-Allocation, 48 kHz)
// ============================================================================

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EnvStage {
    Idle = 0,
    Attack = 1,
    Decay = 2,
    Sustain = 3,
    Release = 4,
}

#[derive(Copy, Clone, Debug)]
pub struct AdsrEnvelope {
    pub stage: EnvStage,
    pub level: f32,
    pub attack_inc: f32,
    pub decay_mul: f32,
    pub sustain_lvl: f32,
    pub release_mul: f32,
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
            attack_inc: 0.01,
            decay_mul: 0.999,
            sustain_lvl: 0.5,
            release_mul: 0.998,
        }
    }
}

impl AdsrEnvelope {
    #[inline]
    pub fn configure(&mut self, params: &AdsrParams) {
        let fs = SAMPLE_RATE as f32;
        // Map 0x00..=0xFF to 0.5ms .. 1.8s
        let att_sec = 0.0005 + (params.attack as f32 / 255.0).powi(2) * 1.5;
        self.attack_inc = (1.0 / (att_sec * fs)).clamp(0.00001, 1.0);

        let dec_sec = 0.004 + (params.decay as f32 / 255.0).powi(2) * 2.2;
        self.decay_mul = (-1.0 / (dec_sec * fs)).exp();

        self.sustain_lvl = (params.sustain as f32 / 255.0).clamp(0.0, 1.0);

        let rel_sec = 0.004 + (params.release as f32 / 255.0).powi(2) * 2.5;
        self.release_mul = (-1.0 / (rel_sec * fs)).exp();
    }

    #[inline]
    pub fn trigger(&mut self, params: &AdsrParams) {
        self.configure(params);
        self.stage = EnvStage::Attack;
        // Start slightly above 0 or ramp from current level to prevent retrigger clicks
        self.level = self.level.min(0.15);
    }

    #[inline]
    pub fn note_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    #[inline]
    pub fn hard_cut(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
                self.level
            }
            EnvStage::Decay => {
                self.level = self.sustain_lvl + (self.level - self.sustain_lvl) * self.decay_mul;
                if (self.level - self.sustain_lvl).abs() < 0.001 {
                    self.level = self.sustain_lvl;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => self.sustain_lvl,
            EnvStage::Release => {
                self.level *= self.release_mul;
                if self.level < 0.0005 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.level
            }
        }
    }
}

// ============================================================================
// Zero-Delay Feedback Cytomic State Variable Filter (Stereo LP / BP / HP)
// ============================================================================

#[derive(Copy, Clone, Debug, Default)]
pub struct CytomicSvf {
    pub ic1eq_l: f32,
    pub ic2eq_l: f32,
    pub ic1eq_r: f32,
    pub ic2eq_r: f32,
    pub a1: f32,
    pub a2: f32,
    pub a3: f32,
    pub k: f32,
    pub mode: SvfFilterMode,
}

impl CytomicSvf {
    pub fn new() -> Self {
        let mut svf = Self {
            ic1eq_l: 0.0,
            ic2eq_l: 0.0,
            ic1eq_r: 0.0,
            ic2eq_r: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 1.0,
            mode: SvfFilterMode::LowPass,
        };
        svf.update_coefficients(0xD0, 0x20, 0.0, SvfFilterMode::LowPass);
        svf
    }

    #[inline]
    pub fn reset_state(&mut self) {
        self.ic1eq_l = 0.0;
        self.ic2eq_l = 0.0;
        self.ic1eq_r = 0.0;
        self.ic2eq_r = 0.0;
    }

    /// Updates Cytomic SVF coefficients once per block or sub-tick (`cutoff_byte`: `00..FF`, `res_byte`: `00..FF`).
    #[inline]
    pub fn update_coefficients(
        &mut self,
        cutoff_byte: u8,
        res_byte: u8,
        env_mod: f32,
        mode: SvfFilterMode,
    ) {
        self.mode = mode;
        let norm_cut = ((cutoff_byte as f32 / 255.0) + env_mod * 0.45).clamp(0.02, 0.99);
        // Exponential mapping from 35 Hz to 18,000 Hz
        let freq_hz = (35.0 * (514.0_f32).powf(norm_cut)).clamp(35.0, 18_500.0);
        let g = (std::f32::consts::PI * freq_hz / (SAMPLE_RATE as f32)).tan();

        // Resonance Q from 0.55 to 10.0 (`k = 1 / Q`)
        let norm_res = (res_byte as f32 / 255.0).clamp(0.0, 0.96);
        let q = 0.55 + norm_res * norm_res * 9.5;
        let k = 1.0 / q;

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        self.a1 = a1;
        self.a2 = a2;
        self.a3 = a3;
        self.k = k;
    }

    #[inline]
    pub fn configure_from_params(
        &mut self,
        params: &SvfParams,
        cutoff_override: Option<u8>,
        res_override: Option<u8>,
        env_level: f32,
    ) {
        let cut = cutoff_override.unwrap_or(params.cutoff);
        let res = res_override.unwrap_or(params.resonance);
        let env_mod = (params.env_amount as f32 / 127.0) * env_level;
        self.update_coefficients(cut, res, env_mod, params.mode);
    }

    /// Processes a single stereo frame `(in_l, in_r)` through the zero-delay feedback SVF.
    #[inline]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        // Left channel
        let v3_l = in_l - self.ic2eq_l;
        let v1_l = self.a1 * self.ic1eq_l + self.a2 * v3_l;
        let v2_l = self.ic2eq_l + self.a2 * self.ic1eq_l + self.a3 * v3_l;
        self.ic1eq_l = 2.0 * v1_l - self.ic1eq_l;
        self.ic2eq_l = 2.0 * v2_l - self.ic2eq_l;

        // Right channel
        let v3_r = in_r - self.ic2eq_r;
        let v1_r = self.a1 * self.ic1eq_r + self.a2 * v3_r;
        let v2_r = self.ic2eq_r + self.a2 * self.ic1eq_r + self.a3 * v3_r;
        self.ic1eq_r = 2.0 * v1_r - self.ic1eq_r;
        self.ic2eq_r = 2.0 * v2_r - self.ic2eq_r;

        match self.mode {
            SvfFilterMode::LowPass => (v2_l, v2_r),
            SvfFilterMode::BandPass => (v1_l, v1_r),
            SvfFilterMode::HighPass => (
                in_l - self.k * v1_l - v2_l,
                in_r - self.k * v1_r - v2_r,
            ),
        }
    }
}

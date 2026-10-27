use crate::audio::sequencer::{BUFFER_FRAMES, SAMPLE_RATE};
use crate::audio::AlignedStereoBlock;

pub const MAX_DELAY_FRAMES: usize = SAMPLE_RATE as usize; // 48,000 frames (1.0s max @ 48 kHz)

const COMB_LENGTHS_L: [usize; 4] = [1687, 1601, 2053, 2251];
const COMB_LENGTHS_R: [usize; 4] = [1687 + 23, 1601 + 23, 2053 + 23, 2251 + 23];
const ALLPASS_LENGTHS_L: [usize; 2] = [347, 113];
const ALLPASS_LENGTHS_R: [usize; 2] = [347 + 19, 113 + 19];

struct CombFilter {
    buf: Vec<f32>,
    idx: usize,
    filter_store: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(1)],
            idx: 0,
            filter_store: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let output = self.buf[self.idx];
        self.filter_store = (output * damp2) + (self.filter_store * damp1);
        self.buf[self.idx] = input + (self.filter_store * feedback);
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        output
    }
}

struct AllpassFilter {
    buf: Vec<f32>,
    idx: usize,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(1)],
            idx: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let output = -input + buf_out;
        self.buf[self.idx] = input + (buf_out * 0.5);
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        output
    }
}

/// Zero-allocation Master Send FX Bus (`Thread 2`):
/// 1. Tempo-synced Stereo Ping-Pong Delay with warm one-pole high-cut damping.
/// 2. Freeverb-style 4-Comb + 2-Allpass Stereo Hall Reverb.
/// All circular buffers are pre-allocated once at startup (`MasterFxBus::new`).
pub struct MasterFxBus {
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    delay_write_pos: usize,
    delay_lp_l: f32,
    delay_lp_r: f32,
    pub delay_feedback: f32,

    combs_l: [CombFilter; 4],
    combs_r: [CombFilter; 4],
    allpass_l: [AllpassFilter; 2],
    allpass_r: [AllpassFilter; 2],
    pub reverb_room_size: f32,
    pub reverb_damping: f32,
}

impl Default for MasterFxBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterFxBus {
    pub fn new() -> Self {
        Self {
            delay_l: vec![0.0; MAX_DELAY_FRAMES],
            delay_r: vec![0.0; MAX_DELAY_FRAMES],
            delay_write_pos: 0,
            delay_lp_l: 0.0,
            delay_lp_r: 0.0,
            delay_feedback: 0.42,
            combs_l: std::array::from_fn(|i| CombFilter::new(COMB_LENGTHS_L[i])),
            combs_r: std::array::from_fn(|i| CombFilter::new(COMB_LENGTHS_R[i])),
            allpass_l: std::array::from_fn(|i| AllpassFilter::new(ALLPASS_LENGTHS_L[i])),
            allpass_r: std::array::from_fn(|i| AllpassFilter::new(ALLPASS_LENGTHS_R[i])),
            reverb_room_size: 0.78,
            reverb_damping: 0.32,
        }
    }

    /// Processes 128 stereo frames of Delay Send and Reverb Send buses and mixes the wet return
    /// directly into `master_mix`. Zero heap allocations.
    #[inline]
    pub fn process_and_return(
        &mut self,
        tempo_bpm: f32,
        delay_send_in: &AlignedStereoBlock,
        reverb_send_in: &AlignedStereoBlock,
        master_mix: &mut AlignedStereoBlock,
    ) {
        // Dotted-eighth ping-pong delay synced to tempo (3/16 of a bar = 0.75 beats)
        let bpm = tempo_bpm.clamp(40.0, 240.0);
        let delay_frames = (((SAMPLE_RATE as f32 * 60.0 * 0.75) / bpm) as usize)
            .clamp(2400, MAX_DELAY_FRAMES - 1);

        let fb = self.delay_feedback.clamp(0.0, 0.88);
        let damp1 = self.reverb_damping.clamp(0.05, 0.95);
        let damp2 = 1.0 - damp1;
        let room = self.reverb_room_size.clamp(0.1, 0.94);

        for frame in 0..BUFFER_FRAMES {
            let base = frame * 2;

            // --- 1. Stereo Ping-Pong Delay ---
            let read_pos = (self.delay_write_pos + MAX_DELAY_FRAMES - delay_frames) % MAX_DELAY_FRAMES;
            let del_out_l = self.delay_l[read_pos];
            let del_out_r = self.delay_r[read_pos];

            let send_dl_l = delay_send_in.0[base];
            let send_dl_r = delay_send_in.0[base + 1];

            // Warm one-pole lowpass damping inside ping-pong cross-feedback loop
            self.delay_lp_l = self.delay_lp_l * 0.35 + (send_dl_l + del_out_r * fb) * 0.65;
            self.delay_lp_r = self.delay_lp_r * 0.35 + (send_dl_r + del_out_l * fb) * 0.65;

            self.delay_l[self.delay_write_pos] = self.delay_lp_l;
            self.delay_r[self.delay_write_pos] = self.delay_lp_r;
            self.delay_write_pos += 1;
            if self.delay_write_pos >= MAX_DELAY_FRAMES {
                self.delay_write_pos = 0;
            }

            // --- 2. Stereo 4-Comb + 2-Allpass Reverb ---
            let rev_in_l = reverb_send_in.0[base] * 0.25;
            let rev_in_r = reverb_send_in.0[base + 1] * 0.25;

            let mut rev_out_l = 0.0_f32;
            let mut rev_out_r = 0.0_f32;
            for i in 0..4 {
                rev_out_l += self.combs_l[i].process(rev_in_l, room, damp1, damp2);
                rev_out_r += self.combs_r[i].process(rev_in_r, room, damp1, damp2);
            }
            for i in 0..2 {
                rev_out_l = self.allpass_l[i].process(rev_out_l);
                rev_out_r = self.allpass_r[i].process(rev_out_r);
            }

            // Accumulate wet Ping-Pong Delay + Stereo Reverb into Master Mix
            master_mix.0[base] += del_out_l * 0.65 + rev_out_l * 0.45;
            master_mix.0[base + 1] += del_out_r * 0.65 + rev_out_r * 0.45;
        }
    }
}

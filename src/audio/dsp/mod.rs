pub mod drumkit;
pub mod fx;
pub mod sampler;
pub mod svf;
pub mod synth;

use crate::audio::dsp::drumkit::DrumKitEngine;
use crate::audio::dsp::sampler::SingleSampleEngine;
use crate::audio::dsp::synth::InternalSynthEngine;
use crate::audio::{AlignedStereoBlock, RealtimeSampleBank};
use crate::model::{
    InstrumentEnum, InternalSynthParams, ProjectState, NUM_INSTRUMENTS,
};

/// Complete zero-allocation DSP channel for one of the 8 tracker tracks.
/// Routes note triggers, retriggers, cuts, and filter/pitch/pan modulations to the active
/// `InstrumentEnum` engine (`DrumKit`, `SingleSample`, `InternalSynth`, or `MidiOut`).
#[derive(Clone, Debug, Default)]
pub struct TrackDspChannel {
    pub active_instrument: u8,
    pub drumkit: DrumKitEngine,
    pub sampler: SingleSampleEngine,
    pub synth: InternalSynthEngine,
    pub pitch_mod_semitones: f32,
    pub vol_scale: f32,
    pub cutoff_override: Option<u8>,
    pub resonance_override: Option<u8>,
    pub pan_override: Option<u8>,
}

impl TrackDspChannel {
    pub fn new() -> Self {
        Self {
            active_instrument: 0,
            drumkit: DrumKitEngine::default(),
            sampler: SingleSampleEngine::new(),
            synth: InternalSynthEngine::new(),
            pitch_mod_semitones: 0.0,
            vol_scale: 1.0,
            cutoff_override: None,
            resonance_override: None,
            pan_override: None,
        }
    }

    pub fn trigger_note(
        &mut self,
        note: u8,
        velocity: u8,
        inst_id: u8,
        slice_offset: u8,
        glide: u8,
        project: &ProjectState,
        sample_bank: &RealtimeSampleBank,
    ) {
        let i = (inst_id as usize) % NUM_INSTRUMENTS;
        self.active_instrument = i as u8;
        self.vol_scale = 1.0;

        let inst = &project.instruments[i];
        match inst.engine {
            InstrumentEnum::DrumKit(ref kit) => {
                self.drumkit.trigger_note(
                    note,
                    velocity,
                    slice_offset,
                    kit,
                    &sample_bank.drum_layers[i],
                );
            }
            InstrumentEnum::SingleSample(ref samp) => {
                self.sampler.trigger(
                    note,
                    velocity,
                    slice_offset,
                    glide,
                    samp,
                    sample_bank.single_samples[i].as_ref(),
                );
            }
            InstrumentEnum::InternalSynth(ref syn) => {
                self.synth.trigger(note, velocity, glide, syn);
            }
            InstrumentEnum::MidiOut(_) => {
                // In addition to hardware 5-pin DIN MIDI output sent by the Sequencer,
                // render a subtle VA synth monitor voice so MidiOut tracks are also audible on speakers/headphones
                let preview_syn = InternalSynthParams::default();
                self.synth.trigger(note, velocity, glide, &preview_syn);
            }
        }
    }

    pub fn note_off(&mut self) {
        self.sampler.note_off();
        self.synth.note_off();
    }

    pub fn hard_cut(&mut self) {
        self.drumkit.cut_all();
        self.sampler.hard_cut();
        self.synth.hard_cut();
    }

    pub fn retrigger(&mut self, vol_scale: f32, project: &ProjectState) {
        self.vol_scale = vol_scale;
        let i = (self.active_instrument as usize) % NUM_INSTRUMENTS;
        match project.instruments[i].engine {
            InstrumentEnum::SingleSample(ref samp) => {
                self.sampler.retrigger(vol_scale, samp);
            }
            InstrumentEnum::InternalSynth(ref syn) => {
                self.synth.retrigger(vol_scale, syn);
            }
            InstrumentEnum::MidiOut(_) => {
                let preview_syn = InternalSynthParams::default();
                self.synth.retrigger(vol_scale, &preview_syn);
            }
            InstrumentEnum::DrumKit(_) => {}
        }
    }

    pub fn render_block(
        &mut self,
        project: &ProjectState,
        sample_bank: &RealtimeSampleBank,
        out_block: &mut AlignedStereoBlock,
    ) -> (f32, f32) {
        let i = (self.active_instrument as usize) % NUM_INSTRUMENTS;
        let inst = &project.instruments[i];
        let master_inst_vol = (inst.volume as f32 / 255.0).clamp(0.0, 1.0);
        let pan_byte = self.pan_override.unwrap_or(inst.pan);
        let pan_norm = (pan_byte as f32 / 255.0).clamp(0.0, 1.0);

        match inst.engine {
            InstrumentEnum::DrumKit(ref kit) => self.drumkit.render_block(
                kit,
                &sample_bank.drum_layers[i],
                self.cutoff_override,
                self.resonance_override,
                self.vol_scale * master_inst_vol,
                out_block,
            ),
            InstrumentEnum::SingleSample(ref samp) => self.sampler.render_block(
                samp,
                sample_bank.single_samples[i].as_ref(),
                self.pitch_mod_semitones,
                self.vol_scale,
                self.cutoff_override,
                self.resonance_override,
                pan_norm,
                master_inst_vol,
                out_block,
            ),
            InstrumentEnum::InternalSynth(ref syn) => self.synth.render_block(
                syn,
                self.pitch_mod_semitones,
                self.vol_scale,
                self.cutoff_override,
                self.resonance_override,
                pan_norm,
                master_inst_vol,
                out_block,
            ),
            InstrumentEnum::MidiOut(ref midi) => {
                let mut preview_syn = InternalSynthParams::default();
                // Map CC Slot 0 (Cutoff) & CC Slot 1 (Resonance) to preview monitor filter
                preview_syn.filter.cutoff = (midi.cc_slots[0].value.saturating_mul(2)).max(32);
                preview_syn.filter.resonance = midi.cc_slots[1].value.saturating_mul(2);
                self.synth.render_block(
                    &preview_syn,
                    self.pitch_mod_semitones,
                    self.vol_scale,
                    self.cutoff_override,
                    self.resonance_override,
                    pan_norm,
                    master_inst_vol * 0.75,
                    out_block,
                )
            }
        }
    }
}

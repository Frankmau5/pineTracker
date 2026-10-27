use crate::audio::midi::{
    MidiOutQueue, MidiPacket, MIDI_START, MIDI_STOP, MIDI_TIMING_CLOCK,
};
use crate::ipc::TrackTelemetry;
use crate::model::{
    FxCommand, FxSlot, InstrumentEnum, PhraseStep, ProjectState, CHAIN_STEPS, EMPTY_U8,
    GROOVE_STEPS, MIDI_CC_SLOTS, NOTE_OFF, NUM_GROOVES, NUM_TRACKS, PHRASE_STEPS, SONG_ROWS,
    TABLE_STEPS,
};

pub const SAMPLE_RATE: u32 = 48_000;
pub const BUFFER_FRAMES: usize = 128;
pub const PPQN_EXTERNAL_MIDI: u32 = 24;
pub const PPQN_INTERNAL: u32 = 192;
pub const SUBTICKS_PER_MIDI_CLOCK: u32 = PPQN_INTERNAL / PPQN_EXTERNAL_MIDI; // 8 sub-ticks per 24-PPQN MIDI clock
pub const MAX_VOICE_EVENTS_PER_BLOCK: usize = 256;

/// Sample-accurate internal DSP event emitted by the sequencer within a 128-sample block.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum VoiceEventKind {
    TriggerNote {
        note: u8,
        velocity: u8,
        instrument: u8,
        slice_offset: u8,
        glide: u8,
    },
    NoteOff,
    CutVoice,
    Retrigger {
        vol_scale: f32,
    },
    SetTrackModulation {
        pitch_semitones: f32,
        volume_scale: f32,
        cutoff_override: Option<u8>,
        resonance_override: Option<u8>,
        pan_override: Option<u8>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct VoiceEvent {
    pub sample_offset: u8,
    pub track: u8,
    pub kind: VoiceEventKind,
}

impl Default for VoiceEvent {
    fn default() -> Self {
        Self {
            sample_offset: 0,
            track: 0,
            kind: VoiceEventKind::NoteOff,
        }
    }
}

/// Zero-allocation fixed-capacity queue of sample-accurate DSP events for a 128-frame audio block.
#[derive(Clone, Debug)]
pub struct VoiceEventQueue {
    events: [VoiceEvent; MAX_VOICE_EVENTS_PER_BLOCK],
    len: usize,
}

impl Default for VoiceEventQueue {
    fn default() -> Self {
        Self {
            events: [VoiceEvent::default(); MAX_VOICE_EVENTS_PER_BLOCK],
            len: 0,
        }
    }
}

impl VoiceEventQueue {
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn push(&mut self, ev: VoiceEvent) -> bool {
        if self.len < MAX_VOICE_EVENTS_PER_BLOCK {
            self.events[self.len] = ev;
            self.len += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[VoiceEvent] {
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

/// Per-track sequencer runtime state (zero heap allocation, fully `Copy`).
#[derive(Copy, Clone, Debug)]
pub struct TrackSeqState {
    pub active: bool,
    pub muted: bool,
    pub song_row: u8,
    pub chain_id: u8,
    pub chain_step: u8,
    pub phrase_id: u8,
    pub phrase_step: u8,
    pub transpose: i8,

    // Groove accumulator (counts in 24-PPQN MIDI clock ticks)
    pub groove_id: u8,
    pub groove_step: u8,
    pub groove_tick_accum: u8,
    pub current_target_ticks: u8,
    pub first_step_pending: bool,

    // Active Note & Instrument state
    pub last_note: u8,
    pub last_instrument: u8,
    pub last_velocity: u8,
    pub slice_offset: u8,
    pub glide_speed: u8,

    // Active MIDI note tracking (so we send cleanly paired Note Offs on MidiOut instruments)
    pub active_midi_note: u8,
    pub active_midi_channel: u8,

    // Sub-tick FX state (192 PPQN resolution)
    pub delayed_step: Option<PhraseStep>,
    pub delay_subticks_left: u8,
    pub cut_ticks_left: u8,
    pub kill_subticks_left: u8,
    pub retrig_interval_subticks: u8,
    pub retrig_counter: u8,
    pub retrig_vol_delta: i8,
    pub current_vol_scale: f32,
    pub pitch_offset_semitones: f32,
    pub arp_semitones: [u8; 3],
    pub arp_idx: u8,
    pub cutoff_override: Option<u8>,
    pub resonance_override: Option<u8>,
    pub pan_override: Option<u8>,

    // Table automation state
    pub table_id: u8,
    pub table_step: u8,
    pub table_transpose: i8,
}

impl Default for TrackSeqState {
    fn default() -> Self {
        Self {
            active: false,
            muted: false,
            song_row: 0,
            chain_id: EMPTY_U8,
            chain_step: 0,
            phrase_id: EMPTY_U8,
            phrase_step: 0,
            transpose: 0,
            groove_id: 0,
            groove_step: 0,
            groove_tick_accum: 0,
            current_target_ticks: 6,
            first_step_pending: false,
            last_note: EMPTY_U8,
            last_instrument: 0,
            last_velocity: 0xD0,
            slice_offset: 0,
            glide_speed: 0,
            active_midi_note: EMPTY_U8,
            active_midi_channel: 0,
            delayed_step: None,
            delay_subticks_left: 0,
            cut_ticks_left: 0,
            kill_subticks_left: 0,
            retrig_interval_subticks: 0,
            retrig_counter: 0,
            retrig_vol_delta: 0,
            current_vol_scale: 1.0,
            pitch_offset_semitones: 0.0,
            arp_semitones: [0; 3],
            arp_idx: 0,
            cutoff_override: None,
            resonance_override: None,
            pan_override: None,
            table_id: EMPTY_U8,
            table_step: 0,
            table_transpose: 0,
        }
    }
}

/// Sample-accurate 192 PPQN / 24 PPQN decoupled Groove Sequencer.
pub struct Sequencer {
    pub playing: bool,
    pub sample_rate: u32,
    pub phase_samples: f64,
    pub subtick_counter: u64,
    pub midi_clock_pulses: u64,
    pub tracks: [TrackSeqState; NUM_TRACKS],
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new(SAMPLE_RATE)
    }
}

impl Sequencer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            playing: false,
            sample_rate,
            phase_samples: 0.0,
            subtick_counter: 0,
            midi_clock_pulses: 0,
            tracks: [TrackSeqState::default(); NUM_TRACKS],
        }
    }

    /// Exact number of audio frames per 192-PPQN internal sub-tick at the given BPM.
    #[inline]
    pub fn samples_per_subtick(&self, tempo_bpm: f32) -> f64 {
        let bpm = (tempo_bpm as f64).clamp(30.0, 300.0);
        (self.sample_rate as f64 * 60.0) / (bpm * PPQN_INTERNAL as f64)
    }

    /// Starts playback from `start_song_row` (`00..FF`) and enqueues MIDI Start (`0xFA`).
    pub fn start(&mut self, project: &ProjectState, start_song_row: u8, midi_q: &mut MidiOutQueue) {
        self.playing = true;
        self.phase_samples = 0.0;
        self.subtick_counter = 0;
        self.midi_clock_pulses = 0;

        midi_q.push(MidiPacket::realtime(0, MIDI_START));

        for (t_idx, trk) in self.tracks.iter_mut().enumerate() {
            let muted = trk.muted;
            *trk = TrackSeqState::default();
            trk.muted = muted;
            trk.song_row = start_song_row;
            trk.groove_id = 0;
            trk.groove_step = 0;
            trk.groove_tick_accum = 0;
            trk.current_target_ticks = Self::resolve_groove_ticks(project, 0, 0);

            if Self::resolve_track_chain_and_phrase(project, t_idx, trk) {
                trk.active = true;
                trk.first_step_pending = true;
            }
        }
    }

    /// Stops playback, silences active MIDI notes, and enqueues MIDI Stop (`0xFC`).
    pub fn stop(&mut self, midi_q: &mut MidiOutQueue, voice_q: &mut VoiceEventQueue) {
        if !self.playing {
            return;
        }
        self.playing = false;

        for (t_idx, trk) in self.tracks.iter_mut().enumerate() {
            if trk.active_midi_note < 128 {
                midi_q.push(MidiPacket::note_off(
                    0,
                    trk.active_midi_channel,
                    trk.active_midi_note,
                ));
                trk.active_midi_note = EMPTY_U8;
            }
            trk.active = false;
            voice_q.push(VoiceEvent {
                sample_offset: 0,
                track: t_idx as u8,
                kind: VoiceEventKind::CutVoice,
            });
        }

        midi_q.push(MidiPacket::realtime(0, MIDI_STOP));
    }

    /// Advances the sample-accurate clock across `num_frames` (128 samples), populating
    /// `midi_q` (steady 24-PPQN `0xF8` + swung `Note On`/`Off`/`CC`) and `voice_q` (internal DSP triggers).
    pub fn process_block(
        &mut self,
        num_frames: usize,
        project: &mut ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        if !self.playing {
            return;
        }

        for frame_idx in 0..num_frames {
            let sample_offset = frame_idx as u8;

            // Fire sub-tick 0 immediately on the very first sample after `start()`
            if self.subtick_counter == 0 && self.phase_samples == 0.0 {
                self.process_subtick(sample_offset, project, midi_q, voice_q);
                self.subtick_counter = 1;
            }

            self.phase_samples += 1.0;
            let sp_subtick = self.samples_per_subtick(project.tempo_bpm);

            while self.phase_samples >= sp_subtick {
                self.phase_samples -= sp_subtick;
                self.process_subtick(sample_offset, project, midi_q, voice_q);
                self.subtick_counter = self.subtick_counter.wrapping_add(1);
            }
        }
    }

    fn process_subtick(
        &mut self,
        sample_offset: u8,
        project: &mut ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        let is_midi_clock_tick = (self.subtick_counter % SUBTICKS_PER_MIDI_CLOCK as u64) == 0;

        // 1. Outbound 5-pin DIN MIDI Timing Clock (0xF8) MUST pulse at a rock-steady 24 PPQN
        if is_midi_clock_tick {
            midi_q.push(MidiPacket::realtime(sample_offset, MIDI_TIMING_CLOCK));
            self.midi_clock_pulses = self.midi_clock_pulses.wrapping_add(1);
        }

        // 2. Per-track 192-PPQN sub-tick FX (runs before new 24-PPQN step evaluation so
        // newly scheduled DEL/RET/KIL counters start counting on the following sub-tick)
        for t_idx in 0..NUM_TRACKS {
            if !self.tracks[t_idx].active {
                continue;
            }

            self.advance_track_192ppqn_subtick(t_idx, sample_offset, project, midi_q, voice_q);

            if is_midi_clock_tick {
                self.advance_track_24ppqn_tick(t_idx, sample_offset, project, midi_q, voice_q);
            }
        }
    }

    fn advance_track_24ppqn_tick(
        &mut self,
        t_idx: usize,
        sample_offset: u8,
        project: &mut ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        let mut trigger_new_step = false;

        {
            let trk = &mut self.tracks[t_idx];
            if trk.first_step_pending {
                trk.first_step_pending = false;
                trk.groove_tick_accum = 0;
                trigger_new_step = true;
            } else {
                // Handle 24-PPQN Note Cut (`CUT` FX)
                if trk.cut_ticks_left > 0 {
                    trk.cut_ticks_left -= 1;
                    if trk.cut_ticks_left == 0 {
                        Self::stop_track_note(t_idx, trk, sample_offset, midi_q, voice_q, false);
                    }
                }

                trk.groove_tick_accum = trk.groove_tick_accum.saturating_add(1);
                if trk.groove_tick_accum >= trk.current_target_ticks.max(1) {
                    trk.groove_tick_accum = 0;
                    Self::advance_groove_cursor(project, trk);
                    Self::advance_phrase_cursor(project, t_idx, trk);
                    trigger_new_step = trk.active;
                }
            }
        }

        if trigger_new_step {
            let p_id = self.tracks[t_idx].phrase_id as usize;
            let p_step = self.tracks[t_idx].phrase_step as usize;
            if let Some(phrase) = project.phrases.get(p_id) {
                let step_data = phrase.steps[p_step];
                self.evaluate_phrase_step(
                    t_idx,
                    step_data,
                    sample_offset,
                    project,
                    midi_q,
                    voice_q,
                );
            }
        }

        // Advance Arpeggio & Table on every 24-PPQN tick
        self.advance_track_table_and_arp(t_idx, sample_offset, project, voice_q);
    }

    fn advance_track_192ppqn_subtick(
        &mut self,
        t_idx: usize,
        sample_offset: u8,
        project: &mut ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        // 1. Check delayed note trigger (`DEL` FX in 192-PPQN sub-ticks)
        let mut fire_delayed: Option<PhraseStep> = None;
        {
            let trk = &mut self.tracks[t_idx];
            if trk.delay_subticks_left > 0 {
                trk.delay_subticks_left -= 1;
                if trk.delay_subticks_left == 0 {
                    fire_delayed = trk.delayed_step.take();
                }
            }

            // 2. Check sub-tick Kill (`KIL` FX)
            if trk.kill_subticks_left > 0 {
                trk.kill_subticks_left -= 1;
                if trk.kill_subticks_left == 0 {
                    Self::stop_track_note(t_idx, trk, sample_offset, midi_q, voice_q, true);
                }
            }

            // 3. Check sub-tick Retrigger (`RET` FX)
            if trk.retrig_interval_subticks > 0 {
                trk.retrig_counter = trk.retrig_counter.saturating_add(1);
                if trk.retrig_counter >= trk.retrig_interval_subticks {
                    trk.retrig_counter = 0;
                    let delta = (trk.retrig_vol_delta as f32) / 32.0;
                    trk.current_vol_scale = (trk.current_vol_scale + delta).clamp(0.05, 1.5);
                    if !trk.muted {
                        voice_q.push(VoiceEvent {
                            sample_offset,
                            track: t_idx as u8,
                            kind: VoiceEventKind::Retrigger {
                                vol_scale: trk.current_vol_scale,
                            },
                        });
                    }
                }
            }
        }

        if let Some(step_data) = fire_delayed {
            self.trigger_step_now(t_idx, step_data, sample_offset, project, midi_q, voice_q);
        }
    }

    fn evaluate_phrase_step(
        &mut self,
        t_idx: usize,
        step: PhraseStep,
        sample_offset: u8,
        project: &mut ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        // Check for pre-trigger FX (`DEL`, `GRV`, `TPO`, `HOP`)
        let mut delay_subticks = 0u8;
        for fx in &step.fx {
            match fx.command {
                FxCommand::Del if fx.value > 0 => {
                    delay_subticks = fx.value;
                }
                FxCommand::Grv => {
                    let g_id = (fx.value as usize).min(NUM_GROOVES - 1) as u8;
                    let trk = &mut self.tracks[t_idx];
                    trk.groove_id = g_id;
                    trk.groove_step = 0;
                    trk.current_target_ticks = Self::resolve_groove_ticks(project, g_id, 0);
                }
                FxCommand::Tpo if fx.value >= 30 => {
                    project.tempo_bpm = fx.value as f32;
                }
                FxCommand::Hop => {
                    let target = (fx.value & 0x0F) as u8;
                    self.tracks[t_idx].phrase_step = target;
                }
                _ => {}
            }
        }

        if delay_subticks > 0 {
            let trk = &mut self.tracks[t_idx];
            trk.delayed_step = Some(step);
            trk.delay_subticks_left = delay_subticks;
        } else {
            self.trigger_step_now(t_idx, step, sample_offset, project, midi_q, voice_q);
        }
    }

    fn trigger_step_now(
        &mut self,
        t_idx: usize,
        step: PhraseStep,
        sample_offset: u8,
        project: &ProjectState,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
    ) {
        let trk = &mut self.tracks[t_idx];

        // Reset step-scoped FX modulators
        trk.retrig_interval_subticks = 0;
        trk.retrig_counter = 0;
        trk.cut_ticks_left = 0;
        trk.kill_subticks_left = 0;
        trk.arp_semitones = [0; 3];
        trk.arp_idx = 0;

        if step.instrument != EMPTY_U8 && (step.instrument as usize) < project.instruments.len() {
            trk.last_instrument = step.instrument;
        }
        if step.volume != EMPTY_U8 {
            trk.last_velocity = step.volume;
        }

        // Apply FX slots
        for fx in &step.fx {
            Self::apply_fx_slot(trk, fx, sample_offset, project, midi_q);
        }

        // Handle Note Off or Note Trigger
        if step.note == NOTE_OFF {
            Self::stop_track_note(t_idx, trk, sample_offset, midi_q, voice_q, false);
            return;
        }

        if step.note != EMPTY_U8 && step.note < 120 {
            // Stop previous MIDI note if active
            if trk.active_midi_note < 128 {
                midi_q.push(MidiPacket::note_off(
                    sample_offset,
                    trk.active_midi_channel,
                    trk.active_midi_note,
                ));
                trk.active_midi_note = EMPTY_U8;
            }

            let inst_id = trk.last_instrument as usize;
            let inst = &project.instruments[inst_id.min(project.instruments.len() - 1)];

            let total_transpose = (trk.transpose as i16) + (inst.transpose as i16);
            let final_note = ((step.note as i16) + total_transpose).clamp(0, 119) as u8;
            trk.last_note = final_note;
            trk.current_vol_scale = 1.0;

            // Attach instrument Table if configured
            if inst.table_id != EMPTY_U8 && (inst.table_id as usize) < project.tables.len() {
                trk.table_id = inst.table_id;
                trk.table_step = 0;
                trk.table_transpose = 0;
            } else {
                trk.table_id = EMPTY_U8;
                trk.table_transpose = 0;
            }

            if !trk.muted {
                // Outbound 5-pin DIN MIDI if this instrument is `MidiOut`
                if let InstrumentEnum::MidiOut(ref midi_params) = inst.engine {
                    let ch = midi_params.channel & 0x0F;
                    let vel_7bit = (trk.last_velocity >> 1).max(1);
                    if midi_params.program < 128 {
                        midi_q.push(MidiPacket::program_change(
                            sample_offset,
                            ch,
                            midi_params.program,
                        ));
                    }
                    midi_q.push(MidiPacket::note_on(sample_offset, ch, final_note, vel_7bit));
                    trk.active_midi_note = final_note;
                    trk.active_midi_channel = ch;
                }

                // Emit internal DSP VoiceEvent
                voice_q.push(VoiceEvent {
                    sample_offset,
                    track: t_idx as u8,
                    kind: VoiceEventKind::TriggerNote {
                        note: final_note,
                        velocity: trk.last_velocity,
                        instrument: trk.last_instrument,
                        slice_offset: trk.slice_offset,
                        glide: trk.glide_speed,
                    },
                });
            }
        }

        // Emit updated track modulation if any FX modified filter/pitch/pan/vol
        if !trk.muted {
            voice_q.push(VoiceEvent {
                sample_offset,
                track: t_idx as u8,
                kind: VoiceEventKind::SetTrackModulation {
                    pitch_semitones: trk.pitch_offset_semitones + (trk.table_transpose as f32),
                    volume_scale: trk.current_vol_scale,
                    cutoff_override: trk.cutoff_override,
                    resonance_override: trk.resonance_override,
                    pan_override: trk.pan_override,
                },
            });
        }
    }

    fn apply_fx_slot(
        trk: &mut TrackSeqState,
        fx: &FxSlot,
        sample_offset: u8,
        project: &ProjectState,
        midi_q: &mut MidiOutQueue,
    ) {
        match fx.command {
            FxCommand::None | FxCommand::Del | FxCommand::Grv | FxCommand::Tpo | FxCommand::Hop => {
            }
            FxCommand::Arp => {
                trk.arp_semitones = [0, (fx.value >> 4) & 0x0F, fx.value & 0x0F];
                trk.arp_idx = 0;
            }
            FxCommand::Ret => {
                let vol_nibble = (fx.value >> 4) as i8;
                trk.retrig_vol_delta = if vol_nibble >= 8 {
                    vol_nibble - 16
                } else {
                    vol_nibble
                };
                trk.retrig_interval_subticks = (fx.value & 0x0F).max(1);
                trk.retrig_counter = 0;
            }
            FxCommand::Cut => {
                trk.cut_ticks_left = fx.value.max(1);
            }
            FxCommand::Kil => {
                trk.kill_subticks_left = fx.value.max(1);
            }
            FxCommand::Vol => {
                trk.current_vol_scale = (fx.value as f32) / 255.0;
            }
            FxCommand::Pit => {
                let signed = fx.value as i8;
                trk.pitch_offset_semitones = (signed as f32) / 8.0;
            }
            FxCommand::Sli => {
                trk.glide_speed = fx.value;
            }
            FxCommand::Fcu => {
                trk.cutoff_override = Some(fx.value);
            }
            FxCommand::Frs => {
                trk.resonance_override = Some(fx.value);
            }
            FxCommand::Pan => {
                trk.pan_override = Some(fx.value);
            }
            FxCommand::Ofs => {
                trk.slice_offset = fx.value;
            }
            FxCommand::Pgm => {
                let inst_id = (trk.last_instrument as usize).min(project.instruments.len() - 1);
                if let InstrumentEnum::MidiOut(ref mp) = project.instruments[inst_id].engine {
                    midi_q.push(MidiPacket::program_change(
                        sample_offset,
                        mp.channel & 0x0F,
                        fx.value & 0x7F,
                    ));
                }
            }
            FxCommand::Cca
            | FxCommand::Ccb
            | FxCommand::Ccc
            | FxCommand::Ccd
            | FxCommand::Cce
            | FxCommand::Ccf => {
                let slot_idx = match fx.command {
                    FxCommand::Cca => 0,
                    FxCommand::Ccb => 1,
                    FxCommand::Ccc => 2,
                    FxCommand::Ccd => 3,
                    FxCommand::Cce => 4,
                    FxCommand::Ccf => 5,
                    _ => 0,
                };
                let inst_id = (trk.last_instrument as usize).min(project.instruments.len() - 1);
                if let InstrumentEnum::MidiOut(ref mp) = project.instruments[inst_id].engine {
                    if slot_idx < MIDI_CC_SLOTS {
                        let slot = mp.cc_slots[slot_idx];
                        if slot.cc_number < 128 {
                            midi_q.push(MidiPacket::control_change(
                                sample_offset,
                                mp.channel & 0x0F,
                                slot.cc_number,
                                fx.value >> 1,
                            ));
                        }
                    }
                }
            }
        }
    }

    fn advance_track_table_and_arp(
        &mut self,
        t_idx: usize,
        sample_offset: u8,
        project: &ProjectState,
        voice_q: &mut VoiceEventQueue,
    ) {
        let trk = &mut self.tracks[t_idx];
        let mut mod_changed = false;

        // Cycle Arpeggio
        if trk.arp_semitones[1] != 0 || trk.arp_semitones[2] != 0 {
            trk.arp_idx = (trk.arp_idx + 1) % 3;
            mod_changed = true;
        }

        // Advance Table step
        if (trk.table_id as usize) < project.tables.len() {
            let tbl = &project.tables[trk.table_id as usize];
            let t_step = (trk.table_step as usize) % TABLE_STEPS;
            let row = tbl.steps[t_step];

            trk.table_transpose = row.transpose;
            if row.volume != EMPTY_U8 {
                trk.current_vol_scale = (row.volume as f32) / 255.0;
            }
            for fx in &row.fx {
                match fx.command {
                    FxCommand::Fcu => trk.cutoff_override = Some(fx.value),
                    FxCommand::Frs => trk.resonance_override = Some(fx.value),
                    FxCommand::Pan => trk.pan_override = Some(fx.value),
                    FxCommand::Hop => {
                        trk.table_step = fx.value & 0x0F;
                    }
                    _ => {}
                }
            }
            trk.table_step = (trk.table_step + 1) % (TABLE_STEPS as u8);
            mod_changed = true;
        }

        if mod_changed && !trk.muted {
            let arp_semi = trk.arp_semitones[trk.arp_idx as usize] as f32;
            voice_q.push(VoiceEvent {
                sample_offset,
                track: t_idx as u8,
                kind: VoiceEventKind::SetTrackModulation {
                    pitch_semitones: trk.pitch_offset_semitones
                        + (trk.table_transpose as f32)
                        + arp_semi,
                    volume_scale: trk.current_vol_scale,
                    cutoff_override: trk.cutoff_override,
                    resonance_override: trk.resonance_override,
                    pan_override: trk.pan_override,
                },
            });
        }
    }

    fn stop_track_note(
        t_idx: usize,
        trk: &mut TrackSeqState,
        sample_offset: u8,
        midi_q: &mut MidiOutQueue,
        voice_q: &mut VoiceEventQueue,
        hard_cut: bool,
    ) {
        if trk.active_midi_note < 128 {
            midi_q.push(MidiPacket::note_off(
                sample_offset,
                trk.active_midi_channel,
                trk.active_midi_note,
            ));
            trk.active_midi_note = EMPTY_U8;
        }
        trk.last_note = EMPTY_U8;
        voice_q.push(VoiceEvent {
            sample_offset,
            track: t_idx as u8,
            kind: if hard_cut {
                VoiceEventKind::CutVoice
            } else {
                VoiceEventKind::NoteOff
            },
        });
    }

    fn resolve_groove_ticks(project: &ProjectState, groove_id: u8, step: u8) -> u8 {
        let g = &project.grooves[(groove_id as usize) % NUM_GROOVES];
        let s = (step as usize) % GROOVE_STEPS;
        let val = g.ticks[s];
        if val == EMPTY_U8 || val == 0 {
            g.ticks[0].max(1)
        } else {
            val
        }
    }

    fn advance_groove_cursor(project: &ProjectState, trk: &mut TrackSeqState) {
        let g = &project.grooves[(trk.groove_id as usize) % NUM_GROOVES];
        let mut next_step = ((trk.groove_step as usize) + 1) % GROOVE_STEPS;
        if g.ticks[next_step] == EMPTY_U8 || g.ticks[next_step] == 0 {
            next_step = 0;
        }
        trk.groove_step = next_step as u8;
        trk.current_target_ticks = g.ticks[next_step].max(1);
    }

    fn resolve_track_chain_and_phrase(
        project: &ProjectState,
        t_idx: usize,
        trk: &mut TrackSeqState,
    ) -> bool {
        let s_row = (trk.song_row as usize) % SONG_ROWS;
        let chain_id = project.song[s_row][t_idx];
        if chain_id == EMPTY_U8 || (chain_id as usize) >= project.chains.len() {
            trk.chain_id = EMPTY_U8;
            trk.phrase_id = EMPTY_U8;
            return false;
        }
        trk.chain_id = chain_id;
        let chain = &project.chains[chain_id as usize];
        for c_step in 0..CHAIN_STEPS {
            let row = chain.rows[c_step];
            if row.phrase != EMPTY_U8 && (row.phrase as usize) < project.phrases.len() {
                trk.chain_step = c_step as u8;
                trk.phrase_id = row.phrase;
                trk.phrase_step = 0;
                trk.transpose = row.transpose;
                return true;
            }
        }
        false
    }

    fn advance_phrase_cursor(project: &ProjectState, t_idx: usize, trk: &mut TrackSeqState) {
        let next_p_step = (trk.phrase_step as usize) + 1;
        if next_p_step < PHRASE_STEPS {
            trk.phrase_step = next_p_step as u8;
            return;
        }

        // Wrap phrase step -> advance chain step
        trk.phrase_step = 0;
        if (trk.chain_id as usize) < project.chains.len() {
            let chain = &project.chains[trk.chain_id as usize];
            for next_c_step in ((trk.chain_step as usize) + 1)..CHAIN_STEPS {
                let row = chain.rows[next_c_step];
                if row.phrase != EMPTY_U8 && (row.phrase as usize) < project.phrases.len() {
                    trk.chain_step = next_c_step as u8;
                    trk.phrase_id = row.phrase;
                    trk.transpose = row.transpose;
                    return;
                }
            }
        }

        // Wrap chain -> advance song row (or loop back to first active song row)
        let start_row = trk.song_row as usize;
        for offset in 1..=SONG_ROWS {
            let candidate = (start_row + offset) % SONG_ROWS;
            let chain_id = project.song[candidate][t_idx];
            if chain_id != EMPTY_U8 && (chain_id as usize) < project.chains.len() {
                trk.song_row = candidate as u8;
                if Self::resolve_track_chain_and_phrase(project, t_idx, trk) {
                    return;
                }
            }
        }
    }

    /// Exports a `Copy` telemetry snapshot for the 8 tracks without allocating.
    pub fn snapshot_tracks(&self, track_peaks: &[(f32, f32); NUM_TRACKS]) -> [TrackTelemetry; NUM_TRACKS] {
        let mut out = [TrackTelemetry::default(); NUM_TRACKS];
        for i in 0..NUM_TRACKS {
            let t = &self.tracks[i];
            out[i] = TrackTelemetry {
                song_row: t.song_row,
                chain_id: t.chain_id,
                chain_step: t.chain_step,
                phrase_id: t.phrase_id,
                phrase_step: t.phrase_step,
                groove_id: t.groove_id,
                active_note: t.last_note,
                active_inst: t.last_instrument,
                peak_l: track_peaks[i].0,
                peak_r: track_peaks[i].1,
                muted: t.muted,
            };
        }
        out
    }
}

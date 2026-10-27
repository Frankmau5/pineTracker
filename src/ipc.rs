use crate::model::{
    ChainRow, Instrument, PhraseStep, ProjectState, TableStep, EMPTY_U8, GROOVE_STEPS,
    NUM_GROOVES, NUM_TRACKS,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::Arc;

pub const UI_TO_AUDIO_CAPACITY: usize = 4096;
pub const AUDIO_TO_UI_CAPACITY: usize = 1024;

/// Stereo or mono `i16` PCM sample slice stored in RAM and shared via `Arc<[i16]>`.
/// When replaced on the real-time audio thread, the old `SampleBuffer` is sent back via
/// `AudioToUiMsg::RetireSampleBuffer` so `Drop` (heap deallocation) only ever runs on Thread 1.
#[derive(Clone, Debug)]
pub struct SampleBuffer {
    pub data: Arc<[i16]>,
    pub channels: u8,
    pub sample_rate: u32,
}

impl SampleBuffer {
    #[inline]
    pub fn frame_count(&self) -> usize {
        let ch = (self.channels.max(1)) as usize;
        self.data.len() / ch
    }
}

/// Lock-free SPSC commands sent from Thread 1 (UI/Input/Loader) to Thread 2 (Real-Time Audio).
#[derive(Clone, Debug)]
pub enum UiToAudioMsg {
    TransportPlay {
        start_song_row: u8,
    },
    TransportStop,
    SetTempo(f32),
    SetMasterVolume(u8),
    SetTrackMute {
        track: u8,
        muted: bool,
    },
    SetSongCell {
        row: u8,
        track: u8,
        chain_id: u8,
    },
    SetChainRow {
        chain_id: u8,
        step: u8,
        row: ChainRow,
    },
    SetPhraseStep {
        phrase_id: u8,
        step: u8,
        data: PhraseStep,
    },
    SetTableStep {
        table_id: u8,
        step: u8,
        data: TableStep,
    },
    SetGrooveStep {
        groove_id: u8,
        step: u8,
        ticks: u8,
    },
    SetInstrument {
        inst_id: u8,
        instrument: Instrument,
    },
    TriggerPreviewNote {
        track: u8,
        note: u8,
        velocity: u8,
        instrument: u8,
    },
    StopPreviewTrack {
        track: u8,
    },
    LoadDrumPadLayer {
        inst_id: u8,
        pad_idx: u8,
        vel_layer: u8,
        sample: SampleBuffer,
    },
    LoadSingleSample {
        inst_id: u8,
        sample: SampleBuffer,
    },
    SendMidiCcImmediate {
        channel: u8,
        cc: u8,
        value: u8,
    },
    Shutdown,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AudioDeviceState {
    PipeWireDefault = 0,
    ExternalUsbDac = 1,
    InternalCodecFallback = 2,
    HeadlessDummy = 3,
}

impl AudioDeviceState {
    pub fn label(self) -> &'static str {
        match self {
            AudioDeviceState::PipeWireDefault => "ALSA:DEFAULT (PIPEWIRE)",
            AudioDeviceState::ExternalUsbDac => "USB-C DAC (hw:1,0)",
            AudioDeviceState::InternalCodecFallback => "A64 CODEC (hw:0,0)",
            AudioDeviceState::HeadlessDummy => "DUMMY CLOCK (NO DAC)",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct TrackTelemetry {
    pub song_row: u8,
    pub chain_id: u8,
    pub chain_step: u8,
    pub phrase_id: u8,
    pub phrase_step: u8,
    pub groove_id: u8,
    pub active_note: u8,
    pub active_inst: u8,
    pub peak_l: f32,
    pub peak_r: f32,
    pub muted: bool,
}

impl Default for TrackTelemetry {
    fn default() -> Self {
        Self {
            song_row: 0,
            chain_id: EMPTY_U8,
            chain_step: 0,
            phrase_id: EMPTY_U8,
            phrase_step: 0,
            groove_id: 0,
            active_note: EMPTY_U8,
            active_inst: EMPTY_U8,
            peak_l: 0.0,
            peak_r: 0.0,
            muted: false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AudioTelemetry {
    pub playing: bool,
    pub tempo_bpm: f32,
    pub midi_clock_pulses: u64,
    pub dac_state: AudioDeviceState,
    pub midi_connected: bool,
    pub tracks: [TrackTelemetry; NUM_TRACKS],
    pub master_peak_l: f32,
    pub master_peak_r: f32,
    pub xrun_recoveries: u32,
}

impl Default for AudioTelemetry {
    fn default() -> Self {
        Self {
            playing: false,
            tempo_bpm: 126.0,
            midi_clock_pulses: 0,
            dac_state: AudioDeviceState::PipeWireDefault,
            midi_connected: false,
            tracks: [TrackTelemetry::default(); NUM_TRACKS],
            master_peak_l: 0.0,
            master_peak_r: 0.0,
            xrun_recoveries: 0,
        }
    }
}

/// Lock-free SPSC messages sent from Thread 2 (Real-Time Audio) to Thread 1 (UI).
#[derive(Clone, Debug)]
pub enum AudioToUiMsg {
    Telemetry(AudioTelemetry),
    RetireSampleBuffer(SampleBuffer),
}

pub struct UiIpcHandle {
    pub tx_cmd: Sender<UiToAudioMsg>,
    pub rx_tel: Receiver<AudioToUiMsg>,
}

impl UiIpcHandle {
    /// Synchronizes an entire `ProjectState` (e.g., loaded from a `.toml` test song on disk)
    /// to the real-time audio thread using fixed-size `Copy` delta messages so Thread 2
    /// performs zero heap allocations or deallocations.
    pub fn sync_project_to_audio(&self, project: &ProjectState) {
        let _ = self.tx_cmd.try_send(UiToAudioMsg::TransportStop);
        let _ = self
            .tx_cmd
            .try_send(UiToAudioMsg::SetTempo(project.tempo_bpm));
        let _ = self
            .tx_cmd
            .try_send(UiToAudioMsg::SetMasterVolume(project.master_volume));

        for (r_idx, row) in project.song.iter().enumerate() {
            for (t_idx, &chain_id) in row.iter().enumerate() {
                if chain_id != EMPTY_U8 || r_idx < 32 {
                    let _ = self.tx_cmd.try_send(UiToAudioMsg::SetSongCell {
                        row: r_idx as u8,
                        track: t_idx as u8,
                        chain_id,
                    });
                }
            }
        }

        for (c_id, chain) in project.chains.iter().enumerate().take(32) {
            for (s_idx, &row) in chain.rows.iter().enumerate() {
                let _ = self.tx_cmd.try_send(UiToAudioMsg::SetChainRow {
                    chain_id: c_id as u8,
                    step: s_idx as u8,
                    row,
                });
            }
        }

        for (p_id, phrase) in project.phrases.iter().enumerate().take(32) {
            for (s_idx, &data) in phrase.steps.iter().enumerate() {
                let _ = self.tx_cmd.try_send(UiToAudioMsg::SetPhraseStep {
                    phrase_id: p_id as u8,
                    step: s_idx as u8,
                    data,
                });
            }
        }

        for (i_id, &instrument) in project.instruments.iter().enumerate().take(16) {
            let _ = self.tx_cmd.try_send(UiToAudioMsg::SetInstrument {
                inst_id: i_id as u8,
                instrument,
            });
        }

        for (t_id, table) in project.tables.iter().enumerate().take(8) {
            for (s_idx, &data) in table.steps.iter().enumerate() {
                let _ = self.tx_cmd.try_send(UiToAudioMsg::SetTableStep {
                    table_id: t_id as u8,
                    step: s_idx as u8,
                    data,
                });
            }
        }

        for (g_id, groove) in project.grooves.iter().enumerate().take(8) {
            for (s_idx, &ticks) in groove.ticks.iter().enumerate() {
                let _ = self.tx_cmd.try_send(UiToAudioMsg::SetGrooveStep {
                    groove_id: g_id as u8,
                    step: s_idx as u8,
                    ticks,
                });
            }
        }
    }
}

pub struct AudioIpcHandle {
    pub rx_cmd: Receiver<UiToAudioMsg>,
    pub tx_tel: Sender<AudioToUiMsg>,
}

pub fn create_ipc_channels() -> (UiIpcHandle, AudioIpcHandle) {
    let (tx_cmd, rx_cmd) = bounded(UI_TO_AUDIO_CAPACITY);
    let (tx_tel, rx_tel) = bounded(AUDIO_TO_UI_CAPACITY);
    (
        UiIpcHandle { tx_cmd, rx_tel },
        AudioIpcHandle { rx_cmd, tx_tel },
    )
}

impl ProjectState {
    /// Applies a `UiToAudioMsg` state mutation in O(1) time without allocating heap memory.
    #[inline]
    pub fn apply_delta(&mut self, msg: &UiToAudioMsg) {
        match *msg {
            UiToAudioMsg::SetTempo(bpm) => {
                self.tempo_bpm = bpm.clamp(30.0, 300.0);
            }
            UiToAudioMsg::SetMasterVolume(vol) => {
                self.master_volume = vol;
            }
            UiToAudioMsg::SetSongCell {
                row,
                track,
                chain_id,
            } => {
                let r = row as usize;
                let t = track as usize;
                if r < self.song.len() && t < NUM_TRACKS {
                    self.song[r][t] = chain_id;
                }
            }
            UiToAudioMsg::SetChainRow {
                chain_id,
                step,
                row,
            } => {
                let c = chain_id as usize;
                let s = step as usize;
                if let Some(chain) = self.chains.get_mut(c) {
                    if s < chain.rows.len() {
                        chain.rows[s] = row;
                    }
                }
            }
            UiToAudioMsg::SetPhraseStep {
                phrase_id,
                step,
                data,
            } => {
                let p = phrase_id as usize;
                let s = step as usize;
                if let Some(phrase) = self.phrases.get_mut(p) {
                    if s < phrase.steps.len() {
                        phrase.steps[s] = data;
                    }
                }
            }
            UiToAudioMsg::SetTableStep {
                table_id,
                step,
                data,
            } => {
                let t = table_id as usize;
                let s = step as usize;
                if let Some(table) = self.tables.get_mut(t) {
                    if s < table.steps.len() {
                        table.steps[s] = data;
                    }
                }
            }
            UiToAudioMsg::SetGrooveStep {
                groove_id,
                step,
                ticks,
            } => {
                let g = groove_id as usize;
                let s = step as usize;
                if g < NUM_GROOVES && s < GROOVE_STEPS {
                    self.grooves[g].ticks[s] = ticks;
                }
            }
            UiToAudioMsg::SetInstrument {
                inst_id,
                instrument,
            } => {
                let i = inst_id as usize;
                if let Some(slot) = self.instruments.get_mut(i) {
                    *slot = instrument;
                }
            }
            _ => {}
        }
    }
}

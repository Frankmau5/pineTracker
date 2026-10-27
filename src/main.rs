pub mod audio;
pub mod input;
pub mod io;
pub mod ipc;
pub mod model;
pub mod ui;

use crate::audio::spawn_audio_thread;
use crate::io::{
    ensure_and_load_starter_wav_bank, ensure_demo_song_on_disk, ensure_starter_synth_profiles,
    load_project_from_path,
};
use crate::ipc::{create_ipc_channels, UiToAudioMsg};
use crate::model::{InstrumentEnum, ProjectState};
use crate::ui::run_sdl2_ui_loop;
use std::path::Path;

fn main() {
    let demo_path = ensure_demo_song_on_disk();
    let synth_profiles = ensure_starter_synth_profiles();

    // Support optional `--load <path.toml>` CLI flag; defaults to the 8-track CYBER_PINE_DEMO test song
    let args: Vec<String> = std::env::args().collect();
    let mut project = ProjectState::default();
    if let Some(idx) = args.iter().position(|a| a == "--load") {
        if let Some(p) = args.get(idx + 1) {
            if let Ok(loaded) = load_project_from_path(Path::new(p)) {
                project = loaded;
            }
        }
    }

    // Apply first starter .toml synth profile to MidiOut instrument 04 if present
    if let Some((_, prof)) = synth_profiles.first() {
        if let InstrumentEnum::MidiOut(ref mut midi_params) = project.instruments[4].engine {
            prof.apply_to_midi_out(midi_params);
        }
    }

    let (ui_ipc, audio_ipc) = create_ipc_channels();
    let deck_mode = cfg!(feature = "deck-dev") || std::env::var("PINE_TRACKER_DECK").is_ok();
    let headless_smoke = std::env::args().any(|a| a == "--smoke-test")
        || std::env::var("PINE_TRACKER_SMOKE").is_ok();
    let max_frames = std::env::args()
        .find_map(|a| a.strip_prefix("--frames=").and_then(|v| v.parse::<u64>().ok()));

    let wav_count = ensure_and_load_starter_wav_bank(&ui_ipc).unwrap_or(0);
    let audio_handle = spawn_audio_thread(project.clone(), audio_ipc, deck_mode);

    println!(
        "pineTracker v0.2.0 initialized — project '{}' @ {:.1} BPM ({} tracks, {} instruments, {} wavs loaded, {} synth profiles, demo_song={}, deck_mode={})",
        project.name,
        project.tempo_bpm,
        model::NUM_TRACKS,
        project.instruments.len(),
        wav_count,
        synth_profiles.len(),
        demo_path.display(),
        deck_mode
    );

    if !headless_smoke {
        if let Err(err) = run_sdl2_ui_loop(
            project,
            crate::ipc::UiIpcHandle {
                tx_cmd: ui_ipc.tx_cmd.clone(),
                rx_tel: ui_ipc.rx_tel.clone(),
            },
            max_frames,
        ) {
            eprintln!("SDL2 UI notice (falling back to clean shutdown): {err}");
        }
    }

    let _ = ui_ipc.tx_cmd.send(UiToAudioMsg::Shutdown);
    let _ = audio_handle.join();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::drumkit::{
        DrumKitEngine, HI_HAT_CHOKE_FRAMES, RETRIG_CROSSFADE_FRAMES,
    };
    use crate::audio::dsp::sampler::SingleSampleEngine;
    use crate::audio::dsp::synth::InternalSynthEngine;
    use crate::audio::midi::{MidiOutQueue, MIDI_START, MIDI_STOP, MIDI_TIMING_CLOCK};
    use crate::audio::sequencer::{
        Sequencer, VoiceEventKind, VoiceEventQueue, BUFFER_FRAMES, SAMPLE_RATE,
    };
    use crate::audio::{
        classify_alsa_write_error, AlignedStereoBlock, AudioEngine, DacRecoveryAction,
        RealtimeSampleBank,
    };
    use crate::input::{evdev_code_to_keycode, InputMode, Keycode, M8View, TrackerUiState, EVDEV_KEY_POWER};
    use crate::ipc::{
        AudioDeviceState, AudioTelemetry, AudioToUiMsg, SampleBuffer, UiToAudioMsg,
    };
    use crate::model::{
        format_note, note_to_drum_pad, note_to_freq_hz, velocity_to_layer, ChainRow,
        DrumKitParams, FxCommand, FxSlot, InstrumentEnum, InternalSynthParams, MidiOutParams,
        SampleLoopMode, SingleSampleParams, SvfFilterMode, SynthMode, VaWaveform, DRUM_BASE_NOTE,
        DRUM_KIT_VOICES, EMPTY_U8, NOTE_OFF, NUM_CHAINS, NUM_GROOVES, NUM_INSTRUMENTS,
        NUM_PHRASES, NUM_TABLES, NUM_TRACKS, PAD_CLOSED_HAT, PAD_OPEN_HAT, PAD_PEDAL_HAT,
        SONG_ROWS,
    };
    use crate::ui::font::{
        FrameBuffer, COL_BG_DARK, COL_BG_PANEL, LEFT_VIEW_WIDTH, RIGHT_HUD_WIDTH, RIGHT_HUD_X,
        SCREEN_HEIGHT, SCREEN_WIDTH,
    };
    use crate::ui::{render_split_screen_frame, BatteryHudState};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn test_m8_dimensions_and_starter_project() {
        let project = ProjectState::default();
        assert_eq!(project.song.len(), SONG_ROWS);
        assert_eq!(project.song[0].len(), NUM_TRACKS);
        assert_eq!(project.chains.len(), NUM_CHAINS);
        assert_eq!(project.phrases.len(), NUM_PHRASES);
        assert_eq!(project.instruments.len(), NUM_INSTRUMENTS);
        assert_eq!(project.tables.len(), NUM_TABLES);
        assert_eq!(project.grooves.len(), NUM_GROOVES);

        assert_eq!(project.grooves[0].ticks[0], 6);
        assert_eq!(project.grooves[0].ticks[1], 6);
        assert_eq!(project.grooves[1].ticks[0], 7);
        assert_eq!(project.grooves[1].ticks[1], 5);

        assert!(matches!(
            project.instruments[0].engine,
            InstrumentEnum::DrumKit(_)
        ));
        assert!(matches!(
            project.instruments[1].engine,
            InstrumentEnum::SingleSample(_)
        ));
        assert!(matches!(
            project.instruments[2].engine,
            InstrumentEnum::InternalSynth(_)
        ));
        assert!(matches!(
            project.instruments[3].engine,
            InstrumentEnum::InternalSynth(_)
        ));
        assert!(matches!(
            project.instruments[4].engine,
            InstrumentEnum::MidiOut(_)
        ));

        if let InstrumentEnum::DrumKit(ref kit) = project.instruments[0].engine {
            assert_eq!(kit.pads[PAD_CLOSED_HAT].choke_target, PAD_OPEN_HAT as u8);
            assert_eq!(kit.pads[PAD_PEDAL_HAT].choke_target, PAD_OPEN_HAT as u8);
            assert_eq!(kit.pads[PAD_OPEN_HAT].choke_target, EMPTY_U8);
        } else {
            panic!("Expected DrumKit on instrument 0");
        }
    }

    #[test]
    fn test_note_and_velocity_helpers() {
        assert_eq!(&format_note(DRUM_BASE_NOTE), b"C-4");
        assert_eq!(&format_note(DRUM_BASE_NOTE + 11), b"B-4");
        assert_eq!(&format_note(NOTE_OFF), b"OFF");
        assert_eq!(&format_note(EMPTY_U8), b"---");

        assert!((note_to_freq_hz(57.0) - 440.0).abs() < 1e-4);
        assert_eq!(note_to_drum_pad(DRUM_BASE_NOTE), Some(0));
        assert_eq!(note_to_drum_pad(DRUM_BASE_NOTE + 8), Some(PAD_OPEN_HAT));

        assert_eq!(velocity_to_layer(0x20), 0);
        assert_eq!(velocity_to_layer(0x80), 1);
        assert_eq!(velocity_to_layer(0xF0), 2);
    }

    #[test]
    fn test_lock_free_spsc_delta_and_zero_dealloc_sample_retirement() {
        let mut ui_state = ProjectState::default();
        let mut audio_state = ui_state.clone();
        let (ui_ipc, audio_ipc) = create_ipc_channels();

        let mut step = ui_state.phrases[0].steps[0];
        step.note = 60;
        step.fx[0] = FxSlot {
            command: FxCommand::Arp,
            value: 0x47,
        };
        let msg = UiToAudioMsg::SetPhraseStep {
            phrase_id: 0,
            step: 0,
            data: step,
        };
        ui_state.apply_delta(&msg);
        ui_ipc.tx_cmd.try_send(msg).unwrap();

        let chain_msg = UiToAudioMsg::SetChainRow {
            chain_id: 2,
            step: 3,
            row: ChainRow {
                phrase: 0x10,
                transpose: -12,
            },
        };
        ui_state.apply_delta(&chain_msg);
        ui_ipc.tx_cmd.try_send(chain_msg).unwrap();

        while let Ok(received) = audio_ipc.rx_cmd.try_recv() {
            audio_state.apply_delta(&received);
        }

        assert_eq!(audio_state.phrases[0].steps[0], ui_state.phrases[0].steps[0]);
        assert_eq!(audio_state.chains[2].rows[3], ui_state.chains[2].rows[3]);

        let old_pcm: Arc<[i16]> = Arc::from(vec![100i16; 256].into_boxed_slice());
        let new_pcm: Arc<[i16]> = Arc::from(vec![200i16; 256].into_boxed_slice());
        let weak_old = Arc::downgrade(&old_pcm);

        let mut active_slot = Some(SampleBuffer {
            data: old_pcm,
            channels: 2,
            sample_rate: 48_000,
        });

        ui_ipc
            .tx_cmd
            .try_send(UiToAudioMsg::LoadSingleSample {
                inst_id: 1,
                sample: SampleBuffer {
                    data: new_pcm,
                    channels: 2,
                    sample_rate: 48_000,
                },
            })
            .unwrap();

        if let Ok(UiToAudioMsg::LoadSingleSample { sample, .. }) = audio_ipc.rx_cmd.try_recv() {
            if let Some(retired) = active_slot.replace(sample) {
                audio_ipc
                    .tx_tel
                    .try_send(AudioToUiMsg::RetireSampleBuffer(retired))
                    .unwrap();
            }
        }

        assert!(
            weak_old.upgrade().is_some(),
            "Old sample buffer must not be deallocated on the audio thread"
        );

        if let Ok(AudioToUiMsg::RetireSampleBuffer(retired)) = ui_ipc.rx_tel.try_recv() {
            drop(retired);
        }
        assert!(
            weak_old.upgrade().is_none(),
            "Retired sample buffer must be cleanly freed on the UI thread"
        );
    }

    #[test]
    fn test_toml_roundtrip_and_fx_cycling() {
        let mut project = ProjectState::default();
        project.apply_delta(&UiToAudioMsg::SetTempo(142.5));
        project.apply_delta(&UiToAudioMsg::SetMasterVolume(0xC0));
        project.apply_delta(&UiToAudioMsg::SetGrooveStep {
            groove_id: 3,
            step: 0,
            ticks: 9,
        });

        let encoded = toml::to_string(&project).expect("ProjectState should serialize to TOML");
        let decoded: ProjectState =
            toml::from_str(&encoded).expect("ProjectState should deserialize from TOML");

        assert_eq!(decoded.name, project.name);
        assert!((decoded.tempo_bpm - 142.5).abs() < 1e-4);
        assert_eq!(decoded.master_volume, 0xC0);
        assert_eq!(decoded.grooves[3].ticks[0], 9);
        assert_eq!(decoded.instruments[0], project.instruments[0]);
        assert_eq!(decoded.instruments[4], project.instruments[4]);
        assert_eq!(decoded.phrases[0].steps[2], project.phrases[0].steps[2]);

        let cmd = FxCommand::None;
        let prev = cmd.next(-1);
        assert_eq!(prev, FxCommand::Pgm);
        assert_eq!(prev.next(1), FxCommand::None);
    }

    #[test]
    fn test_decoupled_groove_clock_24ppqn_vs_swing() {
        let mut project = ProjectState::default();
        project.tempo_bpm = 120.0;

        let mut seq = Sequencer::new(SAMPLE_RATE);
        let mut midi_q = MidiOutQueue::default();
        let mut voice_q = VoiceEventQueue::default();

        seq.start(&project, 0, &mut midi_q);
        assert!(seq.playing);
        assert_eq!(midi_q.as_slice()[0].bytes[0], MIDI_START);

        let mut trk0_steps_at_clock: Vec<(u64, u8)> = Vec::new();
        let mut trk4_steps_at_clock: Vec<(u64, u8)> = Vec::new();
        let mut total_f8_clocks = 0usize;
        let mut midi_note_ons = 0usize;
        let mut midi_ccs = 0usize;

        let mut last_trk0_step = u8::MAX;
        let mut last_trk4_step = u8::MAX;

        let blocks = 24_000 / BUFFER_FRAMES;
        for _ in 0..blocks {
            midi_q.clear();
            voice_q.clear();
            seq.process_block(BUFFER_FRAMES, &mut project, &mut midi_q, &mut voice_q);

            for pkt in midi_q.as_slice() {
                if pkt.bytes[0] == MIDI_TIMING_CLOCK {
                    total_f8_clocks += 1;
                    let clk_idx = total_f8_clocks as u64 - 1;

                    let s0 = seq.tracks[0].phrase_step;
                    if s0 != last_trk0_step {
                        trk0_steps_at_clock.push((clk_idx, s0));
                        last_trk0_step = s0;
                    }
                    let s4 = seq.tracks[4].phrase_step;
                    if s4 != last_trk4_step {
                        trk4_steps_at_clock.push((clk_idx, s4));
                        last_trk4_step = s4;
                    }
                } else if (pkt.bytes[0] & 0xF0) == 0x90 {
                    midi_note_ons += 1;
                } else if (pkt.bytes[0] & 0xF0) == 0xB0 {
                    midi_ccs += 1;
                }
            }
        }

        assert_eq!(total_f8_clocks, 24);
        assert_eq!(
            trk0_steps_at_clock,
            vec![(0, 0), (7, 1), (12, 2), (19, 3)],
            "Track 0 must swing on 07/05 Groove ticks"
        );
        assert_eq!(
            trk4_steps_at_clock,
            vec![(0, 0), (6, 1), (12, 2), (18, 3)],
            "Track 4 must advance on straight 06/06 Groove ticks"
        );
        assert!(midi_note_ons >= 1);
        assert!(midi_ccs >= 1);

        midi_q.clear();
        voice_q.clear();
        seq.stop(&mut midi_q, &mut voice_q);
        assert!(!seq.playing);
        assert!(midi_q
            .as_slice()
            .iter()
            .any(|p| p.bytes[0] == MIDI_STOP));
    }

    #[test]
    fn test_subtick_fx_del_ret_cut_kil() {
        let mut project = ProjectState::default();
        project.tempo_bpm = 120.0;

        let mut step0 = project.phrases[0].steps[0];
        step0.note = 60;
        step0.instrument = 2;
        step0.fx[0] = FxSlot {
            command: FxCommand::Del,
            value: 3,
        };
        step0.fx[1] = FxSlot {
            command: FxCommand::Ret,
            value: 0x12,
        };
        step0.fx[2] = FxSlot {
            command: FxCommand::Kil,
            value: 5,
        };
        project.phrases[0].steps[0] = step0;

        let mut seq = Sequencer::new(SAMPLE_RATE);
        let mut midi_q = MidiOutQueue::default();
        let mut voice_q = VoiceEventQueue::default();
        seq.start(&project, 0, &mut midi_q);

        let mut saw_trigger_for_trk0 = false;
        let mut saw_retrig_for_trk0 = false;
        let mut saw_cut_for_trk0 = false;

        for block_idx in 0..12 {
            midi_q.clear();
            voice_q.clear();
            seq.process_block(BUFFER_FRAMES, &mut project, &mut midi_q, &mut voice_q);

            for ev in voice_q.as_slice() {
                if ev.track == 0 {
                    match ev.kind {
                        VoiceEventKind::TriggerNote { note: 60, .. } => {
                            assert!(
                                block_idx >= 2,
                                "DEL 03 must delay note trigger past sub-tick 0"
                            );
                            saw_trigger_for_trk0 = true;
                        }
                        VoiceEventKind::Retrigger { .. } => {
                            saw_retrig_for_trk0 = true;
                        }
                        VoiceEventKind::CutVoice => {
                            saw_cut_for_trk0 = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_trigger_for_trk0, "Delayed note should have triggered");
        assert!(saw_retrig_for_trk0, "RET FX should have emitted Retrigger");
        assert!(saw_cut_for_trk0, "KIL FX should have emitted CutVoice");
    }

    #[test]
    fn test_dual_dac_hotplug_fallback_and_neon_aligned_mixer() {
        assert_eq!(std::mem::align_of::<AlignedStereoBlock>(), 16);

        assert_eq!(
            classify_alsa_write_error(libc::ENODEV, AudioDeviceState::ExternalUsbDac, false),
            DacRecoveryAction::FallbackToInternalCodec
        );
        assert_eq!(
            classify_alsa_write_error(libc::EPIPE, AudioDeviceState::ExternalUsbDac, false),
            DacRecoveryAction::RecoverSameDevice
        );
        assert_eq!(
            classify_alsa_write_error(libc::EPIPE, AudioDeviceState::ExternalUsbDac, true),
            DacRecoveryAction::FallbackToInternalCodec
        );

        let mut engine = AudioEngine::new(ProjectState::default());
        engine
            .sequencer
            .start(&engine.project, 0, &mut engine.midi_q);
        let (peak_l, peak_r) = engine.process_callback();
        assert!(peak_l > 0.0 && peak_r > 0.0);
        assert!(engine.pcm_out.iter().any(|&s| s != 0));
    }

    #[test]
    fn test_live_audio_thread_spawn_telemetry_and_shutdown() {
        let project = ProjectState::default();
        let (ui_ipc, audio_ipc) = create_ipc_channels();
        let handle = spawn_audio_thread(project, audio_ipc, true);

        ui_ipc
            .tx_cmd
            .send(UiToAudioMsg::TransportPlay { start_song_row: 0 })
            .unwrap();

        let buf1: Arc<[i16]> = Arc::from(vec![500i16; 128].into_boxed_slice());
        let buf2: Arc<[i16]> = Arc::from(vec![900i16; 128].into_boxed_slice());
        let weak1 = Arc::downgrade(&buf1);

        ui_ipc
            .tx_cmd
            .send(UiToAudioMsg::LoadDrumPadLayer {
                inst_id: 0,
                pad_idx: 0,
                vel_layer: 2,
                sample: SampleBuffer {
                    data: buf1,
                    channels: 2,
                    sample_rate: 48_000,
                },
            })
            .unwrap();
        ui_ipc
            .tx_cmd
            .send(UiToAudioMsg::LoadDrumPadLayer {
                inst_id: 0,
                pad_idx: 0,
                vel_layer: 2,
                sample: SampleBuffer {
                    data: buf2,
                    channels: 2,
                    sample_rate: 48_000,
                },
            })
            .unwrap();

        let mut received_telemetry = false;
        let mut received_retired_sample = false;
        let deadline = Instant::now() + Duration::from_millis(500);

        while Instant::now() < deadline && (!received_telemetry || !received_retired_sample) {
            if let Ok(msg) = ui_ipc.rx_tel.recv_timeout(Duration::from_millis(50)) {
                match msg {
                    AudioToUiMsg::Telemetry(tel) => {
                        assert!(tel.playing);
                        received_telemetry = true;
                    }
                    AudioToUiMsg::RetireSampleBuffer(retired) => {
                        drop(retired);
                        received_retired_sample = true;
                    }
                }
            }
        }

        assert!(received_telemetry, "Should receive live ~60Hz AudioTelemetry from Thread 2");
        assert!(
            received_retired_sample,
            "Should receive retired sample buffer from Thread 2"
        );
        assert!(
            weak1.upgrade().is_none(),
            "Replaced drum layer sample must be freed after Thread 1 drops it"
        );

        ui_ipc.tx_cmd.send(UiToAudioMsg::Shutdown).unwrap();
        handle.join().expect("Audio thread should join cleanly");
    }

    #[test]
    fn test_kb151_anti_ghosting_sticky_latch_and_momentary_hold() {
        let mut project = ProjectState::default();
        let (ui_ipc, _audio_ipc) = create_ipc_channels();
        let mut ui = TrackerUiState::default();
        let mut playing = false;

        assert_eq!(ui.view, M8View::Song);

        ui.handle_key_down(Keycode::LShift, 100, &mut playing, &mut project, &ui_ipc);
        assert!(ui.mod_shift.held);
        ui.handle_key_up(Keycode::LShift, 220, &ui_ipc);
        assert!(!ui.mod_shift.held);
        assert!(ui.mod_shift.latched);

        ui.handle_key_down(Keycode::Right, 260, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Chain);
        assert!(!ui.mod_shift.latched);

        ui.handle_key_down(Keycode::RAlt, 300, &mut playing, &mut project, &ui_ipc);
        assert!(ui.mod_edit.is_active());
        ui.handle_key_up(Keycode::RAlt, 550, &ui_ipc);
        assert!(!ui.mod_edit.is_active());
        assert!(!ui.mod_edit.latched);

        ui.handle_key_down(Keycode::LAlt, 600, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_up(Keycode::LAlt, 650, &ui_ipc);
        assert!(ui.mod_edit.latched);
        ui.handle_key_down(Keycode::Escape, 700, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.mod_edit.latched);
    }

    #[test]
    fn test_dual_input_mode_thumb_hex_vs_qwerty_piano() {
        let mut project = ProjectState::default();
        let (ui_ipc, audio_ipc) = create_ipc_channels();
        let mut ui = TrackerUiState::default();
        let mut playing = false;

        assert_eq!(ui.input_mode, InputMode::M8Thumb);
        ui.high_nibble_next = true;
        ui.handle_key_down(Keycode::A, 10, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_down(Keycode::Num5, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(project.song[0][0], 0xA5);

        ui.handle_key_down(Keycode::LGui, 100, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_down(Keycode::Space, 120, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_up(Keycode::LGui, 140, &ui_ipc);
        assert_eq!(ui.input_mode, InputMode::QwertyPiano);

        ui.handle_key_down(Keycode::F3, 200, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Phrase);
        ui.phrase_row = 0;

        ui.handle_key_down(Keycode::Z, 210, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_down(Keycode::S, 220, &mut playing, &mut project, &ui_ipc);
        ui.handle_key_down(Keycode::Q, 230, &mut playing, &mut project, &ui_ipc);

        let p_id = ui.current_phrase as usize;
        assert_eq!(project.phrases[p_id].steps[0].note, 48);
        assert_eq!(project.phrases[p_id].steps[1].note, 49);
        assert_eq!(project.phrases[p_id].steps[2].note, 60);
        assert_eq!(ui.phrase_row, 3);

        let mut saw_preview = false;
        while let Ok(msg) = audio_ipc.rx_cmd.try_recv() {
            if matches!(msg, UiToAudioMsg::TriggerPreviewNote { note: 60, .. }) {
                saw_preview = true;
            }
        }
        assert!(saw_preview);
    }

    #[test]
    fn test_split_screen_1440x720_all_6_m8_views_and_context_hud() {
        let project = ProjectState::default();
        let mut ui = TrackerUiState::default();
        let mut tel = AudioTelemetry::default();
        tel.playing = true;
        tel.tracks[0].peak_l = 0.75;
        tel.tracks[0].peak_r = 0.72;
        let battery = BatteryHudState::default();
        let mut fb = FrameBuffer::default();

        assert_eq!(fb.width, SCREEN_WIDTH);
        assert_eq!(fb.height, SCREEN_HEIGHT);
        assert_eq!(LEFT_VIEW_WIDTH, 864);
        assert_eq!(RIGHT_HUD_WIDTH, 576);
        assert_eq!(LEFT_VIEW_WIDTH + RIGHT_HUD_WIDTH, SCREEN_WIDTH);

        let all_views = [
            M8View::Song,
            M8View::Chain,
            M8View::Phrase,
            M8View::Instrument,
            M8View::Table,
            M8View::Groove,
        ];

        for view in all_views {
            ui.view = view;
            for inst_idx in 0..5u8 {
                ui.current_instrument = inst_idx;
                render_split_screen_frame(&mut fb, &project, &ui, &tel, &battery);

                let left_pixels =
                    fb.count_drawn_pixels(0, 84, LEFT_VIEW_WIDTH - 4, 590, COL_BG_DARK);
                let right_battery_pixels = fb.count_drawn_pixels(
                    RIGHT_HUD_X + 12,
                    8,
                    RIGHT_HUD_WIDTH - 24,
                    126,
                    COL_BG_PANEL,
                );
                let right_minimap_pixels = fb.count_drawn_pixels(
                    RIGHT_HUD_X + 12,
                    142,
                    RIGHT_HUD_WIDTH - 24,
                    126,
                    COL_BG_PANEL,
                );
                let right_meters_pixels = fb.count_drawn_pixels(
                    RIGHT_HUD_X + 12,
                    276,
                    RIGHT_HUD_WIDTH - 24,
                    206,
                    COL_BG_PANEL,
                );
                let right_cc_panel_pixels = fb.count_drawn_pixels(
                    RIGHT_HUD_X + 12,
                    490,
                    RIGHT_HUD_WIDTH - 24,
                    222,
                    COL_BG_PANEL,
                );

                assert!(left_pixels > 1000);
                assert!(right_battery_pixels > 500);
                assert!(right_minimap_pixels > 500);
                assert!(right_meters_pixels > 500);
                assert!(right_cc_panel_pixels > 500);
            }
        }
    }

    // ========================================================================
    // Phase 4 Tests: 4 Instrument Engines (DrumKit, SingleSample, InternalSynth, MidiOut)
    // ========================================================================

    #[test]
    fn test_drumkit_8_voice_pool_1_5ms_retrig_crossfade_and_3ms_hihat_choke() {
        let kit = DrumKitParams::default();
        let bank = RealtimeSampleBank::default();
        let mut engine = DrumKitEngine::default();
        let mut out_block = AlignedStereoBlock::default();

        // 1. Trigger Open Hat (pad 8 = G#4 = note 56)
        engine.trigger_note(
            DRUM_BASE_NOTE + PAD_OPEN_HAT as u8,
            0xE0,
            0,
            &kit,
            &bank.drum_layers[0],
        );
        assert_eq!(engine.active_voice_count(), 1);
        assert_eq!(engine.voices[0].pad_idx, PAD_OPEN_HAT as u8);
        assert!(!engine.voices[0].choking);

        // 2. Trigger Closed Hat (pad 5 = F-4 = note 53) -> must choke Open Hat over 3.0ms (144 frames)
        engine.trigger_note(
            DRUM_BASE_NOTE + PAD_CLOSED_HAT as u8,
            0xD0,
            0,
            &kit,
            &bank.drum_layers[0],
        );
        assert_eq!(engine.active_voice_count(), 2);
        assert!(
            engine.voices[0].choking,
            "Closed Hat must place Open Hat voice into 3ms choke ramp"
        );
        assert_eq!(engine.voices[0].choke_frames_left, HI_HAT_CHOKE_FRAMES); // 144 frames

        // Render 1 block (128 frames): 144 - 128 = 16 frames of choke left
        out_block.clear();
        let (pk_l, _) =
            engine.render_block(&kit, &bank.drum_layers[0], None, None, 1.0, &mut out_block);
        assert!(pk_l > 0.0);
        assert!(engine.voices[0].active);
        assert_eq!(engine.voices[0].choke_frames_left, 16);

        // Render 2nd block (128 frames): remaining 16 choke frames complete -> Open Hat voice deactivates!
        out_block.clear();
        engine.render_block(&kit, &bank.drum_layers[0], None, None, 1.0, &mut out_block);
        assert!(
            !engine.voices[0].active,
            "Open Hat voice must deactivate after completing 144-frame (3ms) choke ramp"
        );

        // 3. Same-Pad Retrigger Crossfade (1.5ms = 72 frames): trigger Kick (pad 0) twice
        engine.trigger_note(DRUM_BASE_NOTE, 0xC0, 0, &kit, &bank.drum_layers[0]);
        engine.render_block(&kit, &bank.drum_layers[0], None, None, 1.0, &mut out_block);
        engine.trigger_note(DRUM_BASE_NOTE, 0xF8, 0, &kit, &bank.drum_layers[0]);

        let kick_slot = engine
            .voices
            .iter()
            .find(|v| v.active && v.pad_idx == 0)
            .expect("Kick slot must be active");
        assert_eq!(
            kick_slot.retrig_fade_left, RETRIG_CROSSFADE_FRAMES,
            "Same-pad retrigger must arm 72-frame (1.5ms) linear crossfade"
        );

        // Render 1 block (128 frames > 72 frames) -> crossfade completes cleanly
        engine.render_block(&kit, &bank.drum_layers[0], None, None, 1.0, &mut out_block);
        let kick_slot_after = engine
            .voices
            .iter()
            .find(|v| v.active && v.pad_idx == 0)
            .unwrap();
        assert_eq!(kick_slot_after.retrig_fade_left, 0);

        // 4. Verify 8-Voice Sub-Pool ceiling when triggering all 12 pads
        for pad in 0..12u8 {
            engine.trigger_note(DRUM_BASE_NOTE + pad, 0xE0, 0, &kit, &bank.drum_layers[0]);
        }
        assert_eq!(engine.active_voice_count(), DRUM_KIT_VOICES);
    }

    #[test]
    fn test_single_sample_pitch_slice_loops_and_cytomic_svf() {
        let mut params = SingleSampleParams::default();
        params.slice_count = 4;
        params.loop_mode = SampleLoopMode::PingPong;
        params.loop_start = 0x20;
        params.loop_end = 0xC0;

        let pcm: Vec<i16> = (0..4096)
            .map(|i| (((i as f32 * 0.05).sin()) * 24000.0) as i16)
            .collect();
        let sb = SampleBuffer {
            data: Arc::from(pcm.into_boxed_slice()),
            channels: 2,
            sample_rate: SAMPLE_RATE,
        };

        let mut sampler = SingleSampleEngine::new();
        // Trigger slice #2 (`slice_offset = 2` with `slice_count = 4` -> starts at 50% of 2048 frames = frame 1024)
        sampler.trigger(52, 0xE0, 2, 0, &params, Some(&sb));
        assert!((sampler.pos_frames - 1024.0).abs() < 1.0);

        let mut block = AlignedStereoBlock::default();
        for mode in [
            SvfFilterMode::LowPass,
            SvfFilterMode::BandPass,
            SvfFilterMode::HighPass,
        ] {
            params.filter.mode = mode;
            block.clear();
            let (pk_l, pk_r) = sampler.render_block(
                &params,
                Some(&sb),
                0.0,
                1.0,
                Some(0xB0),
                Some(0x60),
                0.5,
                1.0,
                &mut block,
            );
            assert!(pk_l > 0.0 && pk_r > 0.0);
            assert!(block.0.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn test_internal_synth_polyblep_va_and_all_8_fm4op_algorithms() {
        let mut params = InternalSynthParams::default();
        let mut synth = InternalSynthEngine::new();
        let mut block = AlignedStereoBlock::default();

        // 1. Test Virtual Analog across all 4 PolyBLEP waveforms (Saw, Pulse, Triangle, Noise)
        params.mode = SynthMode::VirtualAnalog;
        for wave in [
            VaWaveform::Saw,
            VaWaveform::Pulse,
            VaWaveform::Triangle,
            VaWaveform::Noise,
        ] {
            params.osc1_wave = wave;
            synth.trigger(48, 0xE0, 0, &params);
            block.clear();
            let (pk_l, pk_r) = synth.render_block(
                &params,
                0.0,
                1.0,
                Some(0xD0),
                Some(0x40),
                0.5,
                1.0,
                &mut block,
            );
            assert!(
                pk_l > 0.0 && pk_r > 0.0,
                "VirtualAnalog waveform {:?} must produce audio",
                wave
            );
            assert!(block.0.iter().all(|x| x.is_finite()));
        }

        // 2. Test 4-Op FM across all 8 algorithms (0..=7)
        params.mode = SynthMode::Fm4Op;
        for alg in 0..8u8 {
            params.fm_algorithm = alg;
            synth.trigger(60, 0xE0, 0, &params);
            block.clear();
            let (pk_l, pk_r) =
                synth.render_block(&params, 0.0, 1.0, None, None, 0.5, 1.0, &mut block);
            assert!(
                pk_l > 0.0 && pk_r > 0.0,
                "4-Op FM Algorithm {} must produce finite stereo audio",
                alg
            );
            assert!(block.0.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn test_hound_wav_loader_and_toml_synth_profiles() {
        // 1. Verify .toml synth profile seeding and loading
        let profiles = ensure_starter_synth_profiles();
        assert_eq!(profiles.len(), 3);
        let mut midi_out = MidiOutParams::default();
        profiles[1].1.apply_to_midi_out(&mut midi_out);
        assert_eq!(midi_out.profile_name.as_str(), "ELEK_DIGITAKT");
        assert_eq!(midi_out.cc_slots[0].name.as_str(), "FLT FREQ");
        assert_eq!(midi_out.cc_slots[0].cc_number, 74);

        // 2. Verify hound .wav sample generation & loading for all 12 DrumKit pads x 3 velocity layers + SingleSample
        let (ui_ipc, audio_ipc) = create_ipc_channels();
        let count = ensure_and_load_starter_wav_bank(&ui_ipc)
            .expect("Starter WAV bank should generate and load via hound");
        assert_eq!(count, 12 * 3 + 1); // 37 stereo 16-bit 48kHz WAV files

        let mut engine = AudioEngine::new(ProjectState::default());
        assert!(engine.drain_commands(&audio_ipc));

        // Verify all 12 pads x 3 velocity layers and SingleSample slot 1 are populated in RealtimeSampleBank
        for pad in 0..12 {
            for vel in 0..3 {
                assert!(
                    engine.sample_bank.drum_layers[0][pad][vel].is_some(),
                    "DrumKit pad {} vel {} WAV must be loaded in RAM",
                    pad,
                    vel
                );
            }
        }
        assert!(
            engine.sample_bank.single_samples[1].is_some(),
            "SingleSample slot 1 WAV must be loaded in RAM"
        );
    }

    #[test]
    fn test_in_program_quick_user_guide_toggle_and_pages() {
        let mut project = ProjectState::default();
        let (ui_ipc, _audio_ipc) = create_ipc_channels();
        let mut ui = TrackerUiState::default();
        let mut playing = false;
        let tel = AudioTelemetry::default();
        let battery = BatteryHudState::default();
        let mut fb = FrameBuffer::default();

        assert!(!ui.show_help_guide);

        // Press 'H' in M8Thumb mode (or F8) to open the in-program Quick User Guide
        ui.handle_key_down(Keycode::H, 10, &mut playing, &mut project, &ui_ipc);
        assert!(ui.show_help_guide);
        assert_eq!(ui.help_page, 0);

        // Cycle through all 3 pages (0, 1, 2) with Right arrow and verify each page renders
        for expected_page in 0..3u8 {
            assert_eq!(ui.help_page, expected_page);
            render_split_screen_frame(&mut fb, &project, &ui, &tel, &battery);
            let guide_pixels =
                fb.count_drawn_pixels(10, 86, LEFT_VIEW_WIDTH - 22, 588, COL_BG_PANEL);
            assert!(
                guide_pixels > 2000,
                "Quick User Guide page {} must render text and borders",
                expected_page + 1
            );
            ui.handle_key_down(Keycode::Right, 20 + expected_page as u64, &mut playing, &mut project, &ui_ipc);
        }
        assert_eq!(ui.help_page, 0); // Wrapped 2 -> 0

        // Press Escape to close the guide
        ui.handle_key_down(Keycode::Escape, 100, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.show_help_guide);
    }

    #[test]
    fn test_touch_controls_menus_grids_and_hud() {
        let mut project = ProjectState::default();
        let (ui_ipc, audio_ipc) = create_ipc_channels();
        let mut ui = TrackerUiState::default();
        let mut playing = false;

        // 1. Tap Top Menu Bar tabs: CHN (x=120, y=20), PHR (x=190, y=20), INST (x=270, y=20), HLP (x=490, y=20)
        ui.handle_touch_at(120, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Chain);
        ui.handle_touch_at(190, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Phrase);
        ui.handle_touch_at(270, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Instrument);

        // Tap HLP badge (x=490, y=20) -> opens guide; tap right side of guide (x=600, y=300) -> page 2; tap CLOSE (x=760, y=100)
        ui.handle_touch_at(490, 20, &mut playing, &mut project, &ui_ipc);
        assert!(ui.show_help_guide);
        assert_eq!(ui.help_page, 0);
        ui.handle_touch_at(600, 300, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.help_page, 1);
        ui.handle_touch_at(760, 100, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.show_help_guide);

        // Tap Input Mode badge (x=620, y=20) -> toggles M8Thumb <-> QwertyPiano
        assert_eq!(ui.input_mode, InputMode::M8Thumb);
        ui.handle_touch_at(620, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.input_mode, InputMode::QwertyPiano);

        // 2. Tap Bottom Touch Action Toolbar: [PLAY] at (x=45, y=670) -> starts playback
        ui.handle_touch_at(45, 670, &mut playing, &mut project, &ui_ipc);
        assert!(playing);

        // 3. Tap Song View tab (x=45, y=20), then tap Song Grid cell, then tap same cell again to drill down to Chain!
        ui.handle_touch_at(45, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(ui.view, M8View::Song);
        // Tap already-selected (row 0, col 0) cell -> drills down into Chain 00!
        let row0_y = 126 + 8 * 33; // row 0 is at vis_idx 0 when song_row == 0 -> y = 126
        ui.handle_touch_at(110, 130, &mut playing, &mut project, &ui_ipc);
        let _ = row0_y;
        assert_eq!(ui.view, M8View::Chain);

        // 4. Tap Right Context HUD: 8-Track Meter Track 1 (currently active) to toggle MUTE
        assert!(!ui.track_muted[0]);
        ui.handle_touch_at(900, 380, &mut playing, &mut project, &ui_ipc);
        assert!(ui.track_muted[0]);

        // 5. Tap Right Context HUD: 12-Pad DrumKit Pad 0 (x=910, y=550) when Inst 00 (DrumKit) is active
        ui.current_instrument = 0;
        while audio_ipc.rx_cmd.try_recv().is_ok() {}
        ui.handle_touch_at(910, 550, &mut playing, &mut project, &ui_ipc);
        match audio_ipc.rx_cmd.try_recv() {
            Ok(UiToAudioMsg::TriggerPreviewNote { note, instrument, .. }) => {
                assert_eq!(note, DRUM_BASE_NOTE);
                assert_eq!(instrument, 0);
            }
            other => panic!("Expected TriggerPreviewNote from DrumKit touch pad, got {:?}", other),
        }
    }

    #[test]
    fn test_phase6_master_send_fx_delay_and_reverb_zero_alloc() {
        use crate::audio::dsp::fx::MasterFxBus;

        let mut fx = MasterFxBus::new();
        let mut delay_in = AlignedStereoBlock::default();
        let mut reverb_in = AlignedStereoBlock::default();
        let mut master = AlignedStereoBlock::default();

        // Feed a 1-block stereo impulse into both Delay and Reverb send buses
        for i in 0..32 {
            delay_in.0[i * 2] = 0.8;
            delay_in.0[i * 2 + 1] = -0.8;
            reverb_in.0[i * 2] = 0.8;
            reverb_in.0[i * 2 + 1] = 0.8;
        }
        fx.process_and_return(126.0, &delay_in, &reverb_in, &mut master);

        // Clear send inputs and advance 160 blocks (~426ms) to verify wet Reverb and Ping-Pong Delay tails
        delay_in.clear();
        reverb_in.clear();
        let mut max_wet_tail = 0.0_f32;
        for _ in 0..160 {
            master.clear();
            fx.process_and_return(126.0, &delay_in, &reverb_in, &mut master);
            for &s in &master.0 {
                assert!(s.is_finite(), "Master FX output must never be NaN/Inf");
                max_wet_tail = max_wet_tail.max(s.abs());
            }
        }
        assert!(
            max_wet_tail > 0.01,
            "Master Send FX bus must produce audible stereo Delay/Reverb tail (got {})",
            max_wet_tail
        );
    }

    #[test]
    fn test_phase6_m8_clone_copy_paste_and_track_solo() {
        let mut project = ProjectState::default();
        let (ui_ipc, audio_ipc) = create_ipc_channels();
        let mut ui = TrackerUiState::default();

        // 1. In Song View (row 0, col 0 has Chain 00), clone into a fresh unused Chain ID
        assert_eq!(project.song[0][0], 0x00);
        ui.clone_current_cell(&mut project, &ui_ipc);
        let cloned_cid = project.song[0][0];
        assert!(cloned_cid >= 0x10, "Cloned chain ID should be first unused ID >= 0x10, got {:02X}", cloned_cid);
        assert_eq!(
            project.chains[cloned_cid as usize].rows[0].phrase,
            project.chains[0].rows[0].phrase,
            "Cloned chain must deep-copy source chain rows"
        );

        // 2. Switch to Chain View and clone step 0's Phrase into a fresh unused Phrase ID
        ui.view = M8View::Chain;
        ui.current_chain = cloned_cid;
        ui.chain_row = 0;
        ui.clone_current_cell(&mut project, &ui_ipc);
        let cloned_pid = project.chains[cloned_cid as usize].rows[0].phrase;
        assert!(cloned_pid >= 0x10, "Cloned phrase ID should be first unused ID >= 0x10, got {:02X}", cloned_pid);
        assert_eq!(
            project.phrases[cloned_pid as usize].steps[0].note,
            project.phrases[0].steps[0].note,
            "Cloned phrase must deep-copy source phrase steps"
        );

        // 3. Copy & Paste a Phrase step
        ui.view = M8View::Phrase;
        ui.current_phrase = cloned_pid;
        ui.phrase_row = 0;
        ui.copy_current_selection(&project);
        ui.phrase_row = 3;
        ui.paste_clipboard(&mut project, &ui_ipc);
        assert_eq!(
            project.phrases[cloned_pid as usize].steps[3].note,
            project.phrases[cloned_pid as usize].steps[0].note
        );

        // 4. Track Solo on Track 2 (index 1) -> Track 2 unmuted, all other 7 tracks muted
        ui.toggle_track_solo(1, &ui_ipc);
        assert_eq!(ui.solo_track, Some(1));
        for t in 0..NUM_TRACKS {
            assert_eq!(ui.track_muted[t], t != 1);
        }
        // Toggle solo again -> clears solo and unmutes all 8 tracks
        ui.toggle_track_solo(1, &ui_ipc);
        assert_eq!(ui.solo_track, None);
        assert!(ui.track_muted.iter().all(|&m| !m));

        // Verify audio engine cleanly applies all queued deltas
        let mut engine = AudioEngine::new(ProjectState::default());
        assert!(engine.drain_commands(&audio_ipc));
        assert_eq!(engine.project.song[0][0], cloned_cid);
    }

    #[test]
    fn test_phase6_in_app_browser_modal_wav_projects_and_profiles() {
        let mut project = ProjectState::default();
        let (ui_ipc, _audio_ipc) = create_ipc_channels();
        let _ = ensure_and_load_starter_wav_bank(&ui_ipc);
        let _ = ensure_starter_synth_profiles();
        let _ = ensure_demo_song_on_disk();

        let mut ui = TrackerUiState::default();
        let mut playing = false;
        let tel = AudioTelemetry::default();
        let battery = BatteryHudState::default();
        let mut fb = FrameBuffer::default();

        // Press F7 to open In-App Browser Modal (defaults to SamplesWav tab)
        ui.handle_key_down(Keycode::F7, 10, &mut playing, &mut project, &ui_ipc);
        assert!(ui.show_browser_modal);
        assert!(!ui.browser_files.is_empty(), "SamplesWav tab must list .wav files");

        // Render frame with Browser Modal open
        render_split_screen_frame(&mut fb, &project, &ui, &tel, &battery);
        let modal_pixels = fb.count_drawn_pixels(10, 86, LEFT_VIEW_WIDTH - 22, 560, COL_BG_PANEL);
        assert!(modal_pixels > 1500, "Browser modal must render tabs and file list");

        // Switch to ProjectsToml tab (Right arrow), then SynthProfiles tab (Right arrow), and load profile
        ui.handle_key_down(Keycode::Right, 20, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.browser_files.is_empty(), "ProjectsToml tab must list .toml songs");
        ui.handle_key_down(Keycode::Right, 30, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.browser_files.is_empty(), "SynthProfiles tab must list .toml synth profiles");

        ui.current_instrument = 4;
        ui.handle_key_down(Keycode::Return, 40, &mut playing, &mut project, &ui_ipc);
        assert!(!ui.show_browser_modal, "Loading synth profile should close browser modal");
    }

    #[test]
    fn test_phase5_thread1_affinity_and_pinephone_deploy_configs() {
        use crate::ui::configure_ui_thread_affinity;

        // 1. Verify Thread 1 CPU affinity pinning to Cores 0-1
        let pinned = configure_ui_thread_affinity();
        assert!(pinned, "Thread 1 affinity pinning to Cores 0-1 should succeed on multi-core Linux");

        // 2. Verify Stage 1 & Stage 2 PinePhone deployment configs exist and contain required RT settings
        let rt_tune = std::fs::read_to_string("deploy/stage1-arch/pinetracker-rt-tune.sh")
            .expect("Stage 1 RT tune script must exist");
        assert!(rt_tune.contains("pmdown_time"));
        assert!(rt_tune.contains("smp_affinity"));

        let svc = std::fs::read_to_string("deploy/stage1-arch/pine-tracker.service")
            .expect("Stage 1 systemd service must exist");
        assert!(svc.contains("SDL_VIDEODRIVER=kmsdrm"));
        assert!(svc.contains("LimitRTPRIO=99"));

        let extlinux = std::fs::read_to_string("deploy/stage2-buildroot/extlinux.conf")
            .expect("Stage 2 extlinux.conf must exist");
        assert!(extlinux.contains("isolcpus=2,3 nohz_full=2,3"));
        assert!(extlinux.contains("snd_soc_core.pmdown_time=0"));

        // 3. Verify PinePhone KB151 evdev scan code mapper, Volume buttons, OSD, Boot Splash & [OFF] button
        assert_eq!(EVDEV_KEY_POWER, 116);
        assert_eq!(evdev_code_to_keycode(57), Some(Keycode::Space));
        assert_eq!(evdev_code_to_keycode(114), Some(Keycode::VolumeDown));
        assert_eq!(evdev_code_to_keycode(115), Some(Keycode::VolumeUp));
        assert_eq!(evdev_code_to_keycode(464), Some(Keycode::LGui)); // KB151 physical Fn key
        assert_eq!(evdev_code_to_keycode(103), Some(Keycode::Up));

        let mut ui = TrackerUiState::default();
        let mut project = ProjectState::default();
        let (ui_ipc, _audio_ipc) = create_ipc_channels();
        let mut playing = false;

        let init_vol = project.master_volume;
        ui.handle_key_down(Keycode::VolumeUp, 10, &mut playing, &mut project, &ui_ipc);
        assert_eq!(project.master_volume, init_vol.saturating_add(13));
        assert_eq!(ui.volume_osd_frames, 110);

        ui.handle_key_down(Keycode::VolumeDown, 20, &mut playing, &mut project, &ui_ipc);
        assert_eq!(project.master_volume, init_vol);

        let mut fb = FrameBuffer::default();
        crate::ui::render_boot_splash_frame(&mut fb, 55);
        assert!(
            fb.count_drawn_pixels(624, 76, 192, 192, COL_BG_DARK) > 4000,
            "Pixel-art Pine Tree logo must render in boot splash screen"
        );
        assert!(
            fb.count_drawn_pixels(400, 285, 640, 60, COL_BG_DARK) > 1500,
            "PINE TRACKER title must render in boot splash screen"
        );

        assert!(!ui.request_poweroff);
        ui.handle_touch_at(1380, 20, &mut playing, &mut project, &ui_ipc);
        assert!(ui.request_poweroff, "Tapping [OFF] badge should trigger instant poweroff");
    }
}

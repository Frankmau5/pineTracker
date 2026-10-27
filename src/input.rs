use crate::io::{
    list_files_with_extension, load_or_recreate_demo_song, load_project_from_path,
    load_synth_profile_from_path, load_wav_to_sample_buffer, resolve_projects_dir,
    resolve_samples_dir, resolve_synths_dir, save_project_to_path, USER_SONG_FILENAME,
};
use crate::ipc::{UiIpcHandle, UiToAudioMsg};
use crate::model::{
    Chain, ChainRow, FixedStr, FxCommand, Instrument, InstrumentEnum, Phrase, PhraseStep,
    ProjectState, TableStep, CHAIN_STEPS, DRUM_BASE_NOTE, DRUM_PADS, EMPTY_U8, GROOVE_STEPS,
    MIDI_CC_SLOTS, NOTE_OFF, NUM_CHAINS, NUM_GROOVES, NUM_INSTRUMENTS, NUM_PHRASES, NUM_TABLES,
    NUM_TRACKS, PHRASE_STEPS, SONG_ROWS, TABLE_STEPS,
};
#[cfg(target_arch = "x86_64")]
pub use sdl2::keyboard::Keycode;
use std::path::PathBuf;

#[cfg(not(target_arch = "x86_64"))]
#[repr(u16)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Keycode {
    A,
    VolumeDown,
    VolumeUp,
    B,
    Backspace,
    C,
    D,
    Delete,
    Down,
    E,
    Equals,
    Escape,
    F,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    G,
    H,
    I,
    Insert,
    J,
    K,
    Kp0,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    KpEnter,
    L,
    LAlt,
    LCtrl,
    Left,
    LGui,
    LShift,
    M,
    Minus,
    N,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    O,
    P,
    Q,
    R,
    RAlt,
    RCtrl,
    Return,
    RGui,
    Right,
    RShift,
    S,
    Slash,
    Space,
    T,
    Tab,
    U,
    Up,
    V,
    W,
    X,
    Y,
    Z,
}

/// Linux `evdev` (`<linux/input-event-codes.h>`) keycodes for PinePhone physical side buttons.
pub const EVDEV_KEY_VOLUMEDOWN: u16 = 114;
pub const EVDEV_KEY_VOLUMEUP: u16 = 115;
pub const EVDEV_KEY_POWER: u16 = 116;

/// Maps a Linux `evdev` `EV_KEY` scan code (`/dev/input/event*` from `KB151` or USB keyboard)
/// to our tracker `Keycode`.
pub fn evdev_code_to_keycode(code: u16) -> Option<Keycode> {
    match code {
        1 => Some(Keycode::Escape),
        2 => Some(Keycode::Num1),
        3 => Some(Keycode::Num2),
        4 => Some(Keycode::Num3),
        5 => Some(Keycode::Num4),
        6 => Some(Keycode::Num5),
        7 => Some(Keycode::Num6),
        8 => Some(Keycode::Num7),
        9 => Some(Keycode::Num8),
        10 => Some(Keycode::Num9),
        11 => Some(Keycode::Num0),
        12 => Some(Keycode::Minus),
        13 => Some(Keycode::Equals),
        14 => Some(Keycode::Backspace),
        15 => Some(Keycode::Tab),
        16 => Some(Keycode::Q),
        17 => Some(Keycode::W),
        18 => Some(Keycode::E),
        19 => Some(Keycode::R),
        20 => Some(Keycode::T),
        21 => Some(Keycode::Y),
        22 => Some(Keycode::U),
        23 => Some(Keycode::I),
        24 => Some(Keycode::O),
        25 => Some(Keycode::P),
        28 => Some(Keycode::Return),
        29 => Some(Keycode::LCtrl),
        30 => Some(Keycode::A),
        31 => Some(Keycode::S),
        32 => Some(Keycode::D),
        33 => Some(Keycode::F),
        34 => Some(Keycode::G),
        35 => Some(Keycode::H),
        36 => Some(Keycode::J),
        38 => Some(Keycode::L),
        42 => Some(Keycode::LShift),
        44 => Some(Keycode::Z),
        45 => Some(Keycode::X),
        46 => Some(Keycode::C),
        47 => Some(Keycode::V),
        48 => Some(Keycode::B),
        49 => Some(Keycode::N),
        50 => Some(Keycode::M),
        53 => Some(Keycode::Slash),
        54 => Some(Keycode::RShift),
        56 => Some(Keycode::LAlt),
        57 => Some(Keycode::Space),
        59 => Some(Keycode::F1),
        60 => Some(Keycode::F2),
        61 => Some(Keycode::F3),
        62 => Some(Keycode::F4),
        63 => Some(Keycode::F5),
        64 => Some(Keycode::F6),
        65 => Some(Keycode::F7),
        66 => Some(Keycode::F8),
        67 => Some(Keycode::F9),
        68 => Some(Keycode::F10),
        71 => Some(Keycode::Kp7),
        72 => Some(Keycode::Kp8),
        73 => Some(Keycode::Kp9),
        75 => Some(Keycode::Kp4),
        76 => Some(Keycode::Kp5),
        77 => Some(Keycode::Kp6),
        79 => Some(Keycode::Kp1),
        80 => Some(Keycode::Kp2),
        81 => Some(Keycode::Kp3),
        82 => Some(Keycode::Kp0),
        87 => Some(Keycode::F11),
        88 => Some(Keycode::F12),
        96 => Some(Keycode::KpEnter),
        97 => Some(Keycode::RCtrl),
        100 => Some(Keycode::RAlt),
        103 => Some(Keycode::Up),
        105 => Some(Keycode::Left),
        106 => Some(Keycode::Right),
        108 => Some(Keycode::Down),
        110 => Some(Keycode::Insert),
        111 => Some(Keycode::Delete),
        114 => Some(Keycode::VolumeDown),
        115 => Some(Keycode::VolumeUp),
        125 | 464 => Some(Keycode::LGui), // Left Meta / Pine Key / Fn (KEY_FN = 464 on KB151)
        126 => Some(Keycode::RGui),
        _ => None,
    }
}

/// Threshold in milliseconds below which tapping and releasing a hardware modifier
/// (`Shift`, `Option`, `Edit`) without pressing any chord key latches that modifier ON.
pub const STICKY_TAP_THRESHOLD_MS: u64 = 200;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    M8Thumb = 0,
    QwertyPiano = 1,
}

impl InputMode {
    pub fn label(self) -> &'static str {
        match self {
            InputMode::M8Thumb => "M8 THUMB (HEX)",
            InputMode::QwertyPiano => "QWERTY PIANO",
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum M8View {
    Song = 0,
    Chain = 1,
    Phrase = 2,
    Instrument = 3,
    Table = 4,
    Groove = 5,
}

impl M8View {
    pub fn title(self) -> &'static str {
        match self {
            M8View::Song => "SONG",
            M8View::Chain => "CHAIN",
            M8View::Phrase => "PHRASE",
            M8View::Instrument => "INSTRUMENT",
            M8View::Table => "TABLE",
            M8View::Groove => "GROOVE",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CompassDir {
    Up,
    Down,
    Left,
    Right,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BrowserTab {
    SamplesWav = 0,
    ProjectsToml = 1,
    SynthProfiles = 2,
}

impl BrowserTab {
    pub fn title(self) -> &'static str {
        match self {
            BrowserTab::SamplesWav => "SAMPLES (.WAV)",
            BrowserTab::ProjectsToml => "PROJECTS (.TOML)",
            BrowserTab::SynthProfiles => "SYNTH PROFILES",
        }
    }
}

#[derive(Clone, Debug)]
pub enum TrackerClipboard {
    Empty,
    SongCell(u8),
    ChainRow(ChainRow),
    FullChain(Chain),
    PhraseStep(PhraseStep),
    FullPhrase(Phrase),
    Instrument(Instrument),
    TableStep(TableStep),
    GrooveStep(u8),
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifierLineState {
    pub held: bool,
    pub latched: bool,
    pub press_timestamp_ms: u64,
    pub used_in_chord: bool,
}

impl ModifierLineState {
    #[inline]
    pub fn is_active(&self) -> bool {
        self.held || self.latched
    }

    pub fn on_press(&mut self, now_ms: u64) {
        if !self.held {
            self.held = true;
            self.press_timestamp_ms = now_ms;
            self.used_in_chord = false;
        }
    }

    pub fn on_release(&mut self, now_ms: u64) {
        if self.held {
            let elapsed = now_ms.saturating_sub(self.press_timestamp_ms);
            self.held = false;
            if !self.used_in_chord && elapsed < STICKY_TAP_THRESHOLD_MS {
                // Quick single-tap (<200ms) toggles sticky latch ON/OFF
                self.latched = !self.latched;
            }
        }
    }

    pub fn mark_chord_consumed(&mut self) {
        if self.held {
            self.used_in_chord = true;
        }
        if self.latched {
            self.latched = false;
        }
    }
}

/// Complete UI & Input navigation/cursor state for Thread 1.
#[derive(Clone, Debug)]
pub struct TrackerUiState {
    pub view: M8View,
    pub input_mode: InputMode,
    pub active_track: usize,

    // Cursor positions per view
    pub song_row: usize,
    pub song_col: usize,
    pub current_chain: u8,
    pub chain_row: usize,
    pub chain_col: usize, // 0 = phrase, 1 = transpose
    pub current_phrase: u8,
    pub phrase_row: usize,
    pub phrase_col: usize, // 0=Note, 1=Vol, 2=Inst, 3=FX1_Cmd, 4=FX1_Val, 5=FX2_Cmd, 6=FX2_Val, 7=FX3_Cmd, 8=FX3_Val
    pub current_instrument: u8,
    pub inst_param_row: usize,
    pub current_table: u8,
    pub table_row: usize,
    pub table_col: usize, // 0=Transpose, 1=Vol, 2..7=FX1..3
    pub current_groove: u8,
    pub groove_row: usize,

    // Hex nibble entry toggle (high nibble vs low nibble)
    pub high_nibble_next: bool,
    pub piano_octave: u8,

    // Dedicated Anti-Ghosting Hardware Modifier Lines (KB151 spec)
    pub mod_shift: ModifierLineState,  // LShift / RShift
    pub mod_option: ModifierLineState, // LCtrl / RCtrl / Super (Pine Key)
    pub mod_edit: ModifierLineState,   // LAlt / RAlt (AltGr)
    pub fn_or_super_held: bool,
    pub show_help_guide: bool,
    pub help_page: u8,
    pub show_browser_modal: bool,
    pub browser_tab: BrowserTab,
    pub browser_cursor: usize,
    pub browser_files: Vec<PathBuf>,
    pub clipboard: TrackerClipboard,
    pub solo_track: Option<usize>,
    pub track_muted: [bool; NUM_TRACKS],
    pub request_poweroff: bool,
    pub volume_osd_frames: u16,
    pub status_toast: FixedStr<44>,
}

impl Default for TrackerUiState {
    fn default() -> Self {
        Self {
            view: M8View::Song,
            input_mode: InputMode::M8Thumb,
            active_track: 0,
            song_row: 0,
            song_col: 0,
            current_chain: 0,
            chain_row: 0,
            chain_col: 0,
            current_phrase: 0,
            phrase_row: 0,
            phrase_col: 0,
            current_instrument: 0,
            inst_param_row: 0,
            current_table: 0,
            table_row: 0,
            table_col: 0,
            current_groove: 1,
            groove_row: 0,
            high_nibble_next: false,
            piano_octave: 4,
            mod_shift: ModifierLineState::default(),
            mod_option: ModifierLineState::default(),
            mod_edit: ModifierLineState::default(),
            fn_or_super_held: false,
            show_help_guide: false,
            help_page: 0,
            show_browser_modal: false,
            browser_tab: BrowserTab::SamplesWav,
            browser_cursor: 0,
            browser_files: Vec::new(),
            clipboard: TrackerClipboard::Empty,
            solo_track: None,
            track_muted: [false; NUM_TRACKS],
            request_poweroff: false,
            volume_osd_frames: 0,
            status_toast: FixedStr::from_str_lossy("TAP MENUS/GRID OR PRESS [SPACE] PLAY | [HLP]"),
        }
    }
}

impl TrackerUiState {
    /// Adjusts `project.master_volume` (`0..=255`) by `delta`, syncs with Thread 2 and the
    /// PinePhone A64 hardware codec, and triggers the 1.8s On-Screen Volume Overlay (`110` frames).
    pub fn adjust_master_volume(
        &mut self,
        delta: i32,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        let next_vol = (project.master_volume as i32 + delta).clamp(0, 255) as u8;
        self.set_master_volume(next_vol, project, ipc);
    }

    pub fn set_master_volume(
        &mut self,
        next_vol: u8,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        let msg = UiToAudioMsg::SetMasterVolume(next_vol);
        project.apply_delta(&msg);
        let _ = ipc.tx_cmd.try_send(msg);
        crate::audio::AlsaDualDacDriver::set_pinephone_hw_volume(next_vol);
        self.volume_osd_frames = 110;
        let pct = ((next_vol as u32) * 100 + 127) / 255;
        self.status_toast = FixedStr::from_str_lossy(&format!(
            "MASTER VOLUME: {:02}% ({:02X})",
            pct, next_vol
        ));
    }

    /// Handles a key-down event from SDL2 (or synthetic tests) with timestamp `now_ms`.
    pub fn handle_key_down(
        &mut self,
        keycode: Keycode,
        now_ms: u64,
        playing: &mut bool,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        // 0. Physical PinePhone Volume Up / Volume Down Hardware Buttons (works in all screens/modals)
        match keycode {
            Keycode::VolumeDown => {
                self.adjust_master_volume(-13, project, ipc);
                return;
            }
            Keycode::VolumeUp => {
                self.adjust_master_volume(13, project, ipc);
                return;
            }
            _ => {}
        }

        // 1. Dedicated KB151 Modifier Lines (outside passive alpha matrix)
        match keycode {
            Keycode::LShift | Keycode::RShift => {
                self.mod_shift.on_press(now_ms);
                return;
            }
            Keycode::LCtrl | Keycode::RCtrl => {
                self.mod_option.on_press(now_ms);
                return;
            }
            Keycode::LGui | Keycode::RGui => {
                self.fn_or_super_held = true;
                self.mod_option.on_press(now_ms);
                return;
            }
            Keycode::LAlt | Keycode::RAlt => {
                self.mod_edit.on_press(now_ms);
                return;
            }
            Keycode::Escape => {
                self.show_help_guide = false;
                self.show_browser_modal = false;
                self.mod_shift.latched = false;
                self.mod_option.latched = false;
                self.mod_edit.latched = false;
                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::StopPreviewTrack {
                    track: self.active_track as u8,
                });
                return;
            }
            _ => {}
        }

        // Toggle In-App Sample/Project Browser Modal via F7
        if keycode == Keycode::F7 {
            if self.show_browser_modal {
                self.show_browser_modal = false;
            } else {
                self.show_help_guide = false;
                self.open_or_refresh_browser();
            }
            return;
        }

        if self.show_browser_modal {
            match keycode {
                Keycode::Left => {
                    self.browser_tab = match self.browser_tab {
                        BrowserTab::SamplesWav => BrowserTab::SynthProfiles,
                        BrowserTab::ProjectsToml => BrowserTab::SamplesWav,
                        BrowserTab::SynthProfiles => BrowserTab::ProjectsToml,
                    };
                    self.open_or_refresh_browser();
                }
                Keycode::Right => {
                    self.browser_tab = match self.browser_tab {
                        BrowserTab::SamplesWav => BrowserTab::ProjectsToml,
                        BrowserTab::ProjectsToml => BrowserTab::SynthProfiles,
                        BrowserTab::SynthProfiles => BrowserTab::SamplesWav,
                    };
                    self.open_or_refresh_browser();
                }
                Keycode::Up => {
                    self.browser_cursor = self.browser_cursor.saturating_sub(1);
                }
                Keycode::Down => {
                    if !self.browser_files.is_empty() {
                        self.browser_cursor =
                            (self.browser_cursor + 1).min(self.browser_files.len() - 1);
                    }
                }
                Keycode::Return | Keycode::KpEnter | Keycode::Space => {
                    self.load_selected_browser_entry(project, ipc);
                }
                _ => {}
            }
            return;
        }

        // Toggle In-Program Quick User Guide via F8, '?'/'/', or 'H' (in M8Thumb mode)
        if keycode == Keycode::F8
            || keycode == Keycode::Slash
            || (keycode == Keycode::H && self.input_mode == InputMode::M8Thumb)
        {
            self.show_help_guide = !self.show_help_guide;
            return;
        }

        // M8 Copy / Paste / Clone / Solo Chords (Ctrl/Option + C/V/D/M or Insert)
        if keycode == Keycode::Insert
            || (keycode == Keycode::D && self.mod_option.is_active())
        {
            self.mod_option.mark_chord_consumed();
            self.clone_current_cell(project, ipc);
            return;
        }
        if keycode == Keycode::C && self.mod_option.is_active() {
            self.mod_option.mark_chord_consumed();
            self.copy_current_selection(project);
            return;
        }
        if keycode == Keycode::V && self.mod_option.is_active() {
            self.mod_option.mark_chord_consumed();
            self.paste_clipboard(project, ipc);
            return;
        }
        if keycode == Keycode::M && self.mod_option.is_active() {
            self.mod_option.mark_chord_consumed();
            self.toggle_track_solo(self.active_track, ipc);
            return;
        }

        if self.show_help_guide {
            match keycode {
                Keycode::Left | Keycode::Up => {
                    self.help_page = (self.help_page + 2) % 3;
                    return;
                }
                Keycode::Right | Keycode::Down => {
                    self.help_page = (self.help_page + 1) % 3;
                    return;
                }
                _ => {}
            }
        }

        // 2. Dual Input Mode Toggle: Fn/Super/Ctrl + Space, or Tab / F12 fallback on Steam Deck
        if keycode == Keycode::Tab
            || keycode == Keycode::F12
            || (keycode == Keycode::Space
                && (self.fn_or_super_held || self.mod_option.held))
        {
            self.input_mode = match self.input_mode {
                InputMode::M8Thumb => InputMode::QwertyPiano,
                InputMode::QwertyPiano => InputMode::M8Thumb,
            };
            self.mod_option.mark_chord_consumed();
            return;
        }

        // 3. Play / Stop Transport (Space)
        if keycode == Keycode::Space {
            if *playing {
                *playing = false;
                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TransportStop);
            } else {
                *playing = true;
                let start_row = if self.mod_shift.is_active() {
                    self.mod_shift.mark_chord_consumed();
                    0
                } else {
                    self.song_row as u8
                };
                let _ = ipc
                    .tx_cmd
                    .try_send(UiToAudioMsg::TransportPlay { start_song_row: start_row });
            }
            return;
        }

        // 4. Direct View Function Shortcuts (F1..F6)
        match keycode {
            Keycode::F1 => {
                self.show_help_guide = false;
                self.view = M8View::Song;
                return;
            }
            Keycode::F2 => {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Chain;
                return;
            }
            Keycode::F3 => {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Phrase;
                return;
            }
            Keycode::F4 => {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Instrument;
                return;
            }
            Keycode::F5 => {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Table;
                return;
            }
            Keycode::F6 => {
                self.show_help_guide = false;
                self.view = M8View::Groove;
                return;
            }
            Keycode::F9 => {
                // F9: Load & Start the 8-Track Test Song ("CYBER_PINE_DEMO")
                let (demo, _path) = load_or_recreate_demo_song();
                *project = demo;
                ipc.sync_project_to_audio(project);
                self.song_row = 0;
                self.song_col = 0;
                self.active_track = 0;
                self.sync_context_ids_from_cursor(project);
                *playing = true;
                let _ = ipc
                    .tx_cmd
                    .try_send(UiToAudioMsg::TransportPlay { start_song_row: 0 });
                self.status_toast =
                    FixedStr::from_str_lossy("LOADED & PLAYING: CYBER_PINE_DEMO.TOML");
                return;
            }
            Keycode::F10 => {
                // F10: Save current project to data/projects/user_song.toml
                let path = resolve_projects_dir().join(USER_SONG_FILENAME);
                if save_project_to_path(project, &path).is_ok() {
                    self.status_toast =
                        FixedStr::from_str_lossy("SAVED: DATA/PROJECTS/USER_SONG.TOML");
                }
                return;
            }
            Keycode::F11 => {
                // F11: Load saved user_song.toml (or fallback to demo song)
                let path = resolve_projects_dir().join(USER_SONG_FILENAME);
                if let Ok(loaded) = load_project_from_path(&path) {
                    *project = loaded;
                    ipc.sync_project_to_audio(project);
                    self.sync_context_ids_from_cursor(project);
                    self.status_toast =
                        FixedStr::from_str_lossy("LOADED: DATA/PROJECTS/USER_SONG.TOML");
                } else {
                    self.status_toast =
                        FixedStr::from_str_lossy("NO USER_SONG.TOML YET (PRESS F10 TO SAVE)");
                }
                return;
            }
            _ => {}
        }

        // 5. Compass Directions (Arrow Keys, plus WASD/HJKL in M8Thumb mode when not typing hex A/D/S/W)
        if let Some(dir) = Self::keycode_to_compass(keycode) {
            self.handle_compass(dir, project, ipc);
            return;
        }

        // 6. Clear / Delete Cell (Backspace, Delete, or Option + Edit)
        if keycode == Keycode::Backspace || keycode == Keycode::Delete {
            self.clear_current_cell(project, ipc);
            return;
        }

        // 7. Master Volume (`Option + -/=`) or Octave Shift in Piano Mode (`-` / `=`)
        if keycode == Keycode::Minus {
            if self.mod_option.is_active() {
                self.mod_option.mark_chord_consumed();
                self.adjust_master_volume(-13, project, ipc);
            } else {
                self.piano_octave = self.piano_octave.saturating_sub(1).max(1);
            }
            return;
        }
        if keycode == Keycode::Equals {
            if self.mod_option.is_active() {
                self.mod_option.mark_chord_consumed();
                self.adjust_master_volume(13, project, ipc);
            } else {
                self.piano_octave = (self.piano_octave + 1).min(7);
            }
            return;
        }

        // 8. Mode-Specific Alpha/Number Keys: M8 Thumb Hex Entry vs QWERTY 2-Octave Piano
        match self.input_mode {
            InputMode::M8Thumb => {
                if let Some(nibble) = Self::keycode_to_hex_nibble(keycode) {
                    self.apply_hex_nibble(nibble, project, ipc);
                }
            }
            InputMode::QwertyPiano => {
                if let Some(semi_offset) = Self::keycode_to_piano_semitone(keycode) {
                    let note = ((self.piano_octave as u8) * 12 + semi_offset).min(119);
                    self.trigger_and_record_piano_note(note, project, ipc);
                }
            }
        }
    }

    pub fn handle_key_up(&mut self, keycode: Keycode, now_ms: u64, ipc: &UiIpcHandle) {
        match keycode {
            Keycode::LShift | Keycode::RShift => self.mod_shift.on_release(now_ms),
            Keycode::LCtrl | Keycode::RCtrl => self.mod_option.on_release(now_ms),
            Keycode::LGui | Keycode::RGui => {
                self.fn_or_super_held = false;
                self.mod_option.on_release(now_ms);
            }
            Keycode::LAlt | Keycode::RAlt => self.mod_edit.on_release(now_ms),
            _ => {
                if self.input_mode == InputMode::QwertyPiano
                    && Self::keycode_to_piano_semitone(keycode).is_some()
                {
                    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::StopPreviewTrack {
                        track: self.active_track as u8,
                    });
                }
            }
        }
    }

    fn keycode_to_compass(keycode: Keycode) -> Option<CompassDir> {
        match keycode {
            Keycode::Up => Some(CompassDir::Up),
            Keycode::Down => Some(CompassDir::Down),
            Keycode::Left => Some(CompassDir::Left),
            Keycode::Right => Some(CompassDir::Right),
            _ => None,
        }
    }

    /// Maps `0-9` and `A-F` keycodes to hexadecimal nibble `0x0..=0xF` in M8 Thumb Mode.
    pub fn keycode_to_hex_nibble(keycode: Keycode) -> Option<u8> {
        match keycode {
            Keycode::Num0 | Keycode::Kp0 => Some(0x0),
            Keycode::Num1 | Keycode::Kp1 => Some(0x1),
            Keycode::Num2 | Keycode::Kp2 => Some(0x2),
            Keycode::Num3 | Keycode::Kp3 => Some(0x3),
            Keycode::Num4 | Keycode::Kp4 => Some(0x4),
            Keycode::Num5 | Keycode::Kp5 => Some(0x5),
            Keycode::Num6 | Keycode::Kp6 => Some(0x6),
            Keycode::Num7 | Keycode::Kp7 => Some(0x7),
            Keycode::Num8 | Keycode::Kp8 => Some(0x8),
            Keycode::Num9 | Keycode::Kp9 => Some(0x9),
            Keycode::A => Some(0xA),
            Keycode::B => Some(0xB),
            Keycode::C => Some(0xC),
            Keycode::D => Some(0xD),
            Keycode::E => Some(0xE),
            Keycode::F => Some(0xF),
            _ => None,
        }
    }

    /// Maps 2-octave chromatic QWERTY Piano layout (`Z-M` lower octave, `Q-P` upper octave).
    pub fn keycode_to_piano_semitone(keycode: Keycode) -> Option<u8> {
        match keycode {
            // Lower Octave (Z..M = 0..11 semitones)
            Keycode::Z => Some(0),  // C
            Keycode::S => Some(1),  // C#
            Keycode::X => Some(2),  // D
            Keycode::D => Some(3),  // D#
            Keycode::C => Some(4),  // E
            Keycode::V => Some(5),  // F
            Keycode::G => Some(6),  // F#
            Keycode::B => Some(7),  // G
            Keycode::H => Some(8),  // G#
            Keycode::N => Some(9),  // A
            Keycode::J => Some(10), // A#
            Keycode::M => Some(11), // B
            // Upper Octave (Q..P = 12..28 semitones)
            Keycode::Q => Some(12),    // C+1
            Keycode::Num2 => Some(13), // C#+1
            Keycode::W => Some(14),    // D+1
            Keycode::Num3 => Some(15), // D#+1
            Keycode::E => Some(16),    // E+1
            Keycode::R => Some(17),    // F+1
            Keycode::Num5 => Some(18), // F#+1
            Keycode::T => Some(19),    // G+1
            Keycode::Num6 => Some(20), // G#+1
            Keycode::Y => Some(21),    // A+1
            Keycode::Num7 => Some(22), // A#+1
            Keycode::U => Some(23),    // B+1
            Keycode::I => Some(24),    // C+2
            Keycode::Num9 => Some(25), // C#+2
            Keycode::O => Some(26),    // D+2
            Keycode::Num0 => Some(27), // D#+2
            Keycode::P => Some(28),    // E+2
            _ => None,
        }
    }

    fn handle_compass(&mut self, dir: CompassDir, project: &mut ProjectState, ipc: &UiIpcHandle) {
        // 1. Shift + Compass: M8 Screen Hierarchy Navigation
        if self.mod_shift.is_active() {
            self.mod_shift.mark_chord_consumed();
            self.sync_context_ids_from_cursor(project);
            self.view = match (self.view, dir) {
                (M8View::Song, CompassDir::Right) => M8View::Chain,
                (M8View::Song, CompassDir::Down) => M8View::Groove,
                (M8View::Chain, CompassDir::Left) => M8View::Song,
                (M8View::Chain, CompassDir::Right) => M8View::Phrase,
                (M8View::Phrase, CompassDir::Left) => M8View::Chain,
                (M8View::Phrase, CompassDir::Right) => M8View::Instrument,
                (M8View::Phrase, CompassDir::Down) => M8View::Groove,
                (M8View::Instrument, CompassDir::Left) => M8View::Phrase,
                (M8View::Instrument, CompassDir::Right) => M8View::Table,
                (M8View::Table, CompassDir::Left) => M8View::Instrument,
                (M8View::Groove, CompassDir::Up) => M8View::Song,
                (M8View::Groove, CompassDir::Right) => M8View::Phrase,
                (current, _) => current,
            };
            return;
        }

        // 2. Option + Compass: Switch active Track / Chain / Phrase / Instrument / Table / Groove ID
        if self.mod_option.is_active() && !self.mod_edit.is_active() {
            self.mod_option.mark_chord_consumed();
            match dir {
                CompassDir::Left => {
                    self.active_track = (self.active_track + NUM_TRACKS - 1) % NUM_TRACKS;
                    self.song_col = self.active_track;
                }
                CompassDir::Right => {
                    self.active_track = (self.active_track + 1) % NUM_TRACKS;
                    self.song_col = self.active_track;
                }
                CompassDir::Up => self.cycle_current_view_object_id(1),
                CompassDir::Down => self.cycle_current_view_object_id(-1),
            }
            return;
        }

        // 3. Edit + Compass: Value Modification (±1 fine with Left/Right, ±16 coarse with Up/Down)
        if self.mod_edit.is_active() {
            if self.mod_edit.held {
                self.mod_edit.used_in_chord = true;
            }
            let delta: i32 = match dir {
                CompassDir::Right => 1,
                CompassDir::Left => -1,
                CompassDir::Up => 16,
                CompassDir::Down => -16,
            };
            self.adjust_current_cell(delta, project, ipc);
            return;
        }

        // 4. Plain Compass: Cursor movement within active M8View
        self.high_nibble_next = true;
        match self.view {
            M8View::Song => match dir {
                CompassDir::Up => self.song_row = self.song_row.saturating_sub(1),
                CompassDir::Down => self.song_row = (self.song_row + 1).min(SONG_ROWS - 1),
                CompassDir::Left => {
                    self.song_col = self.song_col.saturating_sub(1);
                    self.active_track = self.song_col;
                }
                CompassDir::Right => {
                    self.song_col = (self.song_col + 1).min(NUM_TRACKS - 1);
                    self.active_track = self.song_col;
                }
            },
            M8View::Chain => match dir {
                CompassDir::Up => self.chain_row = self.chain_row.saturating_sub(1),
                CompassDir::Down => self.chain_row = (self.chain_row + 1).min(15),
                CompassDir::Left => self.chain_col = self.chain_col.saturating_sub(1),
                CompassDir::Right => self.chain_col = (self.chain_col + 1).min(1),
            },
            M8View::Phrase => match dir {
                CompassDir::Up => self.phrase_row = self.phrase_row.saturating_sub(1),
                CompassDir::Down => self.phrase_row = (self.phrase_row + 1).min(PHRASE_STEPS - 1),
                CompassDir::Left => self.phrase_col = self.phrase_col.saturating_sub(1),
                CompassDir::Right => self.phrase_col = (self.phrase_col + 1).min(8),
            },
            M8View::Instrument => match dir {
                CompassDir::Up => self.inst_param_row = self.inst_param_row.saturating_sub(1),
                CompassDir::Down => self.inst_param_row = (self.inst_param_row + 1).min(11),
                CompassDir::Left => {
                    self.current_instrument =
                        self.current_instrument.saturating_sub(1);
                }
                CompassDir::Right => {
                    self.current_instrument =
                        (self.current_instrument + 1).min((NUM_INSTRUMENTS - 1) as u8);
                }
            },
            M8View::Table => match dir {
                CompassDir::Up => self.table_row = self.table_row.saturating_sub(1),
                CompassDir::Down => self.table_row = (self.table_row + 1).min(TABLE_STEPS - 1),
                CompassDir::Left => self.table_col = self.table_col.saturating_sub(1),
                CompassDir::Right => self.table_col = (self.table_col + 1).min(7),
            },
            M8View::Groove => match dir {
                CompassDir::Up => self.groove_row = self.groove_row.saturating_sub(1),
                CompassDir::Down => self.groove_row = (self.groove_row + 1).min(GROOVE_STEPS - 1),
                CompassDir::Left => {
                    self.current_groove = self.current_groove.saturating_sub(1);
                }
                CompassDir::Right => {
                    self.current_groove = (self.current_groove + 1).min((NUM_GROOVES - 1) as u8);
                }
            },
        }
    }

    fn cycle_current_view_object_id(&mut self, delta: i32) {
        match self.view {
            M8View::Chain => {
                self.current_chain =
                    (self.current_chain as i32 + delta).rem_euclid(NUM_CHAINS as i32) as u8;
            }
            M8View::Phrase => {
                self.current_phrase =
                    (self.current_phrase as i32 + delta).rem_euclid(NUM_PHRASES as i32) as u8;
            }
            M8View::Instrument => {
                self.current_instrument = (self.current_instrument as i32 + delta)
                    .rem_euclid(NUM_INSTRUMENTS as i32)
                    as u8;
            }
            M8View::Table => {
                self.current_table =
                    (self.current_table as i32 + delta).rem_euclid(NUM_TABLES as i32) as u8;
            }
            M8View::Groove => {
                self.current_groove =
                    (self.current_groove as i32 + delta).rem_euclid(NUM_GROOVES as i32) as u8;
            }
            M8View::Song => {}
        }
    }

    pub fn sync_context_ids_from_cursor(&mut self, project: &ProjectState) {
        let cell_chain = project.song[self.song_row][self.song_col];
        if cell_chain != EMPTY_U8 {
            self.current_chain = cell_chain;
        }
        let c_row = project.chains[self.current_chain as usize].rows[self.chain_row];
        if c_row.phrase != EMPTY_U8 {
            self.current_phrase = c_row.phrase;
        }
        let p_step = project.phrases[self.current_phrase as usize].steps[self.phrase_row];
        if p_step.instrument < NUM_INSTRUMENTS as u8 {
            self.current_instrument = p_step.instrument;
        }
        let inst = &project.instruments[self.current_instrument as usize];
        if inst.table_id < NUM_TABLES as u8 {
            self.current_table = inst.table_id;
        } else {
            self.current_table = self.current_instrument.min((NUM_TABLES - 1) as u8);
        }
    }

    fn adjust_current_cell(
        &mut self,
        delta: i32,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        match self.view {
            M8View::Song => {
                let cur = project.song[self.song_row][self.song_col];
                let next = if cur == EMPTY_U8 {
                    self.song_col as u8
                } else {
                    (cur as i32 + delta).clamp(0, (NUM_CHAINS - 1) as i32) as u8
                };
                let msg = UiToAudioMsg::SetSongCell {
                    row: self.song_row as u8,
                    track: self.song_col as u8,
                    chain_id: next,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Chain => {
                let mut row = project.chains[self.current_chain as usize].rows[self.chain_row];
                if self.chain_col == 0 {
                    row.phrase = if row.phrase == EMPTY_U8 {
                        self.current_chain
                    } else {
                        (row.phrase as i32 + delta).clamp(0, (NUM_PHRASES - 1) as i32) as u8
                    };
                } else {
                    row.transpose = (row.transpose as i32 + delta).clamp(-48, 48) as i8;
                }
                let msg = UiToAudioMsg::SetChainRow {
                    chain_id: self.current_chain,
                    step: self.chain_row as u8,
                    row,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Phrase => {
                let mut step = project.phrases[self.current_phrase as usize].steps[self.phrase_row];
                match self.phrase_col {
                    0 => {
                        // Note column: Up/Down (±16) shifts by ±12 semitones (1 octave)
                        let note_delta = if delta.abs() == 16 {
                            delta.signum() * 12
                        } else {
                            delta
                        };
                        step.note = if step.note >= 120 {
                            DRUM_BASE_NOTE
                        } else {
                            (step.note as i32 + note_delta).clamp(0, 119) as u8
                        };
                        if step.instrument == EMPTY_U8 {
                            step.instrument = self.current_instrument;
                        }
                    }
                    1 => {
                        step.volume = if step.volume == EMPTY_U8 {
                            0xC0
                        } else {
                            (step.volume as i32 + delta).clamp(0, 255) as u8
                        };
                    }
                    2 => {
                        step.instrument = if step.instrument == EMPTY_U8 {
                            self.current_instrument
                        } else {
                            (step.instrument as i32 + delta)
                                .clamp(0, (NUM_INSTRUMENTS - 1) as i32)
                                as u8
                        };
                    }
                    col => {
                        let fx_idx = (col - 3) / 2;
                        let is_val = (col - 3) % 2 == 1;
                        if is_val {
                            step.fx[fx_idx].value =
                                (step.fx[fx_idx].value as i32 + delta).clamp(0, 255) as u8;
                        } else {
                            step.fx[fx_idx].command = step.fx[fx_idx].command.next(delta.signum());
                        }
                    }
                }
                let msg = UiToAudioMsg::SetPhraseStep {
                    phrase_id: self.current_phrase,
                    step: self.phrase_row as u8,
                    data: step,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Instrument => {
                let mut inst = project.instruments[self.current_instrument as usize];
                match self.inst_param_row {
                    0 => {
                        // Switch Instrument Engine Type
                        inst.engine = match inst.engine {
                            InstrumentEnum::DrumKit(_) => {
                                InstrumentEnum::SingleSample(Default::default())
                            }
                            InstrumentEnum::SingleSample(_) => {
                                InstrumentEnum::InternalSynth(Default::default())
                            }
                            InstrumentEnum::InternalSynth(_) => {
                                InstrumentEnum::MidiOut(Default::default())
                            }
                            InstrumentEnum::MidiOut(_) => {
                                InstrumentEnum::DrumKit(Default::default())
                            }
                        };
                    }
                    1 => inst.volume = (inst.volume as i32 + delta).clamp(0, 255) as u8,
                    2 => inst.pan = (inst.pan as i32 + delta).clamp(0, 255) as u8,
                    3 => inst.transpose = (inst.transpose as i32 + delta).clamp(-48, 48) as i8,
                    4 => {
                        inst.table_id = if inst.table_id == EMPTY_U8 {
                            0
                        } else {
                            (inst.table_id as i32 + delta).clamp(0, (NUM_TABLES - 1) as i32) as u8
                        };
                    }
                    5 => {
                        inst.delay_send = (inst.delay_send as i32 + delta).clamp(0, 255) as u8;
                    }
                    6 => {
                        inst.reverb_send = (inst.reverb_send as i32 + delta).clamp(0, 255) as u8;
                    }
                    row => {
                        Self::adjust_engine_specific_param(&mut inst.engine, row - 7, delta, ipc);
                    }
                }
                let msg = UiToAudioMsg::SetInstrument {
                    inst_id: self.current_instrument,
                    instrument: inst,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Table => {
                let mut step = project.tables[self.current_table as usize].steps[self.table_row];
                match self.table_col {
                    0 => step.transpose = (step.transpose as i32 + delta).clamp(-48, 48) as i8,
                    1 => {
                        step.volume = if step.volume == EMPTY_U8 {
                            0xC0
                        } else {
                            (step.volume as i32 + delta).clamp(0, 255) as u8
                        };
                    }
                    col => {
                        let fx_idx = (col - 2) / 2;
                        let is_val = (col - 2) % 2 == 1;
                        if is_val {
                            step.fx[fx_idx].value =
                                (step.fx[fx_idx].value as i32 + delta).clamp(0, 255) as u8;
                        } else {
                            step.fx[fx_idx].command = step.fx[fx_idx].command.next(delta.signum());
                        }
                    }
                }
                let msg = UiToAudioMsg::SetTableStep {
                    table_id: self.current_table,
                    step: self.table_row as u8,
                    data: step,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Groove => {
                let cur = project.grooves[self.current_groove as usize].ticks[self.groove_row];
                let next = if cur == EMPTY_U8 {
                    6
                } else {
                    (cur as i32 + delta).clamp(1, 32) as u8
                };
                let msg = UiToAudioMsg::SetGrooveStep {
                    groove_id: self.current_groove,
                    step: self.groove_row as u8,
                    ticks: next,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
        }
    }

    fn adjust_engine_specific_param(
        engine: &mut InstrumentEnum,
        sub_row: usize,
        delta: i32,
        ipc: &UiIpcHandle,
    ) {
        match engine {
            InstrumentEnum::DrumKit(kit) => {
                let pad_idx = sub_row % DRUM_PADS;
                kit.pads[pad_idx].level =
                    (kit.pads[pad_idx].level as i32 + delta).clamp(0, 255) as u8;
            }
            InstrumentEnum::SingleSample(samp) => match sub_row {
                0 => samp.filter.cutoff = (samp.filter.cutoff as i32 + delta).clamp(0, 255) as u8,
                1 => {
                    samp.filter.resonance =
                        (samp.filter.resonance as i32 + delta).clamp(0, 255) as u8
                }
                2 => samp.slice_count = (samp.slice_count as i32 + delta).clamp(1, 64) as u8,
                3 => samp.loop_start = (samp.loop_start as i32 + delta).clamp(0, 255) as u8,
                4 => samp.loop_end = (samp.loop_end as i32 + delta).clamp(0, 255) as u8,
                5 => samp.amp_env.decay = (samp.amp_env.decay as i32 + delta).clamp(0, 255) as u8,
                _ => {
                    samp.amp_env.release =
                        (samp.amp_env.release as i32 + delta).clamp(0, 255) as u8
                }
            },
            InstrumentEnum::InternalSynth(syn) => match sub_row {
                0 => {
                    syn.mode = match syn.mode {
                        crate::model::SynthMode::VirtualAnalog => crate::model::SynthMode::Fm4Op,
                        crate::model::SynthMode::Fm4Op => crate::model::SynthMode::VirtualAnalog,
                    }
                }
                1 => syn.filter.cutoff = (syn.filter.cutoff as i32 + delta).clamp(0, 255) as u8,
                2 => {
                    syn.filter.resonance =
                        (syn.filter.resonance as i32 + delta).clamp(0, 255) as u8
                }
                3 => syn.pulse_width = (syn.pulse_width as i32 + delta).clamp(16, 240) as u8,
                4 => syn.sub_osc_level = (syn.sub_osc_level as i32 + delta).clamp(0, 255) as u8,
                5 => syn.fm_algorithm = (syn.fm_algorithm as i32 + delta).rem_euclid(8) as u8,
                _ => syn.fm_feedback = (syn.fm_feedback as i32 + delta).clamp(0, 255) as u8,
            },
            InstrumentEnum::MidiOut(midi) => {
                if sub_row == 0 {
                    midi.channel = (midi.channel as i32 + delta).clamp(0, 15) as u8;
                } else {
                    let cc_idx = (sub_row - 1).min(MIDI_CC_SLOTS - 1);
                    let next_val = (midi.cc_slots[cc_idx].value as i32 + delta).clamp(0, 127) as u8;
                    midi.cc_slots[cc_idx].value = next_val;
                    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::SendMidiCcImmediate {
                        channel: midi.channel,
                        cc: midi.cc_slots[cc_idx].cc_number,
                        value: next_val,
                    });
                }
            }
        }
    }

    /// Direct `0-9` / `A-F` hexadecimal nibble entry in M8 Thumb Mode.
    fn apply_hex_nibble(&mut self, nibble: u8, project: &mut ProjectState, ipc: &UiIpcHandle) {
        let combine = |old: u8, nib: u8, high_next: &mut bool| -> u8 {
            let base = if old == EMPTY_U8 { 0x00 } else { old };
            if *high_next {
                *high_next = false;
                (nib << 4) | (base & 0x0F)
            } else {
                *high_next = true;
                (base & 0xF0) | (nib & 0x0F)
            }
        };

        match self.view {
            M8View::Song => {
                let old = project.song[self.song_row][self.song_col];
                let next = combine(old, nibble, &mut self.high_nibble_next).min(0xFE);
                let msg = UiToAudioMsg::SetSongCell {
                    row: self.song_row as u8,
                    track: self.song_col as u8,
                    chain_id: next,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Chain => {
                let mut row = project.chains[self.current_chain as usize].rows[self.chain_row];
                if self.chain_col == 0 {
                    row.phrase = combine(row.phrase, nibble, &mut self.high_nibble_next).min(0xFE);
                } else {
                    let raw = combine(row.transpose as u8, nibble, &mut self.high_nibble_next);
                    row.transpose = (raw as i8).clamp(-48, 48);
                }
                let msg = UiToAudioMsg::SetChainRow {
                    chain_id: self.current_chain,
                    step: self.chain_row as u8,
                    row,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Phrase => {
                let mut step = project.phrases[self.current_phrase as usize].steps[self.phrase_row];
                match self.phrase_col {
                    0 => {
                        step.note = combine(step.note, nibble, &mut self.high_nibble_next).min(119);
                        if step.instrument == EMPTY_U8 {
                            step.instrument = self.current_instrument;
                        }
                    }
                    1 => {
                        step.volume = combine(step.volume, nibble, &mut self.high_nibble_next);
                    }
                    2 => {
                        step.instrument = combine(
                            step.instrument,
                            nibble,
                            &mut self.high_nibble_next,
                        )
                        .min((NUM_INSTRUMENTS - 1) as u8);
                    }
                    col => {
                        let fx_idx = (col - 3) / 2;
                        let is_val = (col - 3) % 2 == 1;
                        if is_val {
                            step.fx[fx_idx].value = combine(
                                step.fx[fx_idx].value,
                                nibble,
                                &mut self.high_nibble_next,
                            );
                        } else {
                            let idx = (nibble as usize) % FxCommand::ALL.len();
                            step.fx[fx_idx].command = FxCommand::ALL[idx];
                        }
                    }
                }
                let msg = UiToAudioMsg::SetPhraseStep {
                    phrase_id: self.current_phrase,
                    step: self.phrase_row as u8,
                    data: step,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Groove => {
                let old = project.grooves[self.current_groove as usize].ticks[self.groove_row];
                let next = combine(old, nibble, &mut self.high_nibble_next).clamp(1, 32);
                let msg = UiToAudioMsg::SetGrooveStep {
                    groove_id: self.current_groove,
                    step: self.groove_row as u8,
                    ticks: next,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            _ => {}
        }
    }

    fn clear_current_cell(&mut self, project: &mut ProjectState, ipc: &UiIpcHandle) {
        match self.view {
            M8View::Song => {
                let msg = UiToAudioMsg::SetSongCell {
                    row: self.song_row as u8,
                    track: self.song_col as u8,
                    chain_id: EMPTY_U8,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Chain => {
                let msg = UiToAudioMsg::SetChainRow {
                    chain_id: self.current_chain,
                    step: self.chain_row as u8,
                    row: ChainRow::default(),
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Phrase => {
                let mut step = project.phrases[self.current_phrase as usize].steps[self.phrase_row];
                match self.phrase_col {
                    0 => {
                        step.note = if step.note == EMPTY_U8 {
                            NOTE_OFF
                        } else {
                            EMPTY_U8
                        };
                    }
                    1 => step.volume = EMPTY_U8,
                    2 => step.instrument = EMPTY_U8,
                    col => {
                        let fx_idx = (col - 3) / 2;
                        step.fx[fx_idx] = Default::default();
                    }
                }
                let msg = UiToAudioMsg::SetPhraseStep {
                    phrase_id: self.current_phrase,
                    step: self.phrase_row as u8,
                    data: step,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            M8View::Groove => {
                let msg = UiToAudioMsg::SetGrooveStep {
                    groove_id: self.current_groove,
                    step: self.groove_row as u8,
                    ticks: EMPTY_U8,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
            }
            _ => {}
        }
    }

    fn trigger_and_record_piano_note(
        &mut self,
        note: u8,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TriggerPreviewNote {
            track: self.active_track as u8,
            note,
            velocity: 0xD8,
            instrument: self.current_instrument,
        });

        // In Phrase view, live step entry writes the played chromatic note and advances step
        if self.view == M8View::Phrase {
            let mut step = project.phrases[self.current_phrase as usize].steps[self.phrase_row];
            step.note = note;
            if step.instrument == EMPTY_U8 {
                step.instrument = self.current_instrument;
            }
            if step.volume == EMPTY_U8 {
                step.volume = 0xD8;
            }
            let msg = UiToAudioMsg::SetPhraseStep {
                phrase_id: self.current_phrase,
                step: self.phrase_row as u8,
                data: step,
            };
            project.apply_delta(&msg);
            let _ = ipc.tx_cmd.try_send(msg);
            self.phrase_row = (self.phrase_row + 1) % PHRASE_STEPS;
        }
    }

    /// Handles a touch or mouse click at logical `1440x720` coordinates `(lx, ly)`.
    /// Supports full touch operation across top view tabs, modifier latches, bottom action bar,
    /// M8 view grids (with tap-again drill-down), Song Mini-Map, Track Mute meters, 12-Pad DrumKit,
    /// and MIDI CC touch sliders.
    pub fn handle_touch_at(
        &mut self,
        lx: usize,
        ly: usize,
        playing: &mut bool,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        if lx < 864 {
            self.handle_left_panel_touch(lx, ly, playing, project, ipc);
        } else {
            self.handle_right_hud_touch(lx, ly, project, ipc);
        }
    }

    fn handle_left_panel_touch(
        &mut self,
        lx: usize,
        ly: usize,
        playing: &mut bool,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        // 1. Top Menu Bar (y: 0..44): View Tabs + HLP + Input Mode Toggle
        if ly < 44 {
            if (10..=90).contains(&lx) {
                self.show_help_guide = false;
                self.view = M8View::Song;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: SONG VIEW");
            } else if (96..=160).contains(&lx) {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Chain;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: CHAIN VIEW");
            } else if (166..=230).contains(&lx) {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Phrase;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: PHRASE VIEW");
            } else if (236..=316).contains(&lx) {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Instrument;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: INSTRUMENT VIEW");
            } else if (322..=386).contains(&lx) {
                self.show_help_guide = false;
                self.sync_context_ids_from_cursor(project);
                self.view = M8View::Table;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: TABLE VIEW");
            } else if (392..=456).contains(&lx) {
                self.show_help_guide = false;
                self.view = M8View::Groove;
                self.status_toast = FixedStr::from_str_lossy("TOUCH: GROOVE VIEW");
            } else if (462..=526).contains(&lx) {
                self.show_browser_modal = false;
                self.show_help_guide = !self.show_help_guide;
                self.status_toast = if self.show_help_guide {
                    FixedStr::from_str_lossy("QUICK USER GUIDE OPEN (TAP LEFT/RIGHT PAGE)")
                } else {
                    FixedStr::from_str_lossy("CLOSED QUICK USER GUIDE")
                };
            } else if (532..=596).contains(&lx) {
                self.show_help_guide = false;
                if self.show_browser_modal {
                    self.show_browser_modal = false;
                    self.status_toast = FixedStr::from_str_lossy("CLOSED BROWSER");
                } else {
                    self.open_or_refresh_browser();
                }
            } else if lx >= 600 {
                self.input_mode = match self.input_mode {
                    InputMode::M8Thumb => InputMode::QwertyPiano,
                    InputMode::QwertyPiano => InputMode::M8Thumb,
                };
                self.status_toast = FixedStr::from_str_lossy(self.input_mode.label());
            }
            return;
        }

        // 2. Second Status & Sticky Modifier Latch Bar (y: 44..82)
        if ly <= 82 {
            if (316..436).contains(&lx) {
                let delta = if lx < 376 { -1.0 } else { 1.0 };
                let next_bpm = (project.tempo_bpm + delta).clamp(40.0, 240.0);
                let msg = UiToAudioMsg::SetTempo(next_bpm);
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("TEMPO SET TO {:.0} BPM", next_bpm));
            } else if (436..512).contains(&lx) {
                self.active_track = (self.active_track + 1) % NUM_TRACKS;
                self.song_col = self.active_track;
                self.sync_context_ids_from_cursor(project);
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "ACTIVE TRACK: TRK{}",
                    self.active_track + 1
                ));
            } else if (512..620).contains(&lx) {
                self.piano_octave = (self.piano_octave % 7) + 1;
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("PIANO OCTAVE: {}", self.piano_octave));
            } else if (624..700).contains(&lx) {
                self.mod_shift.latched = !self.mod_shift.latched;
                self.status_toast = FixedStr::from_str_lossy(if self.mod_shift.latched {
                    "SHIFT LATCHED ON"
                } else {
                    "SHIFT LATCHED OFF"
                });
            } else if (700..776).contains(&lx) {
                self.mod_option.latched = !self.mod_option.latched;
                self.status_toast = FixedStr::from_str_lossy(if self.mod_option.latched {
                    "OPTION LATCHED ON"
                } else {
                    "OPTION LATCHED OFF"
                });
            } else if lx >= 776 {
                self.mod_edit.latched = !self.mod_edit.latched;
                self.status_toast = FixedStr::from_str_lossy(if self.mod_edit.latched {
                    "EDIT LATCHED ON"
                } else {
                    "EDIT LATCHED OFF"
                });
            }
            return;
        }

        // 3. Bottom Touch Action Toolbar (y: 650..692)
        if (650..=692).contains(&ly) {
            if (8..=74).contains(&lx) {
                // [PLAY / STOP]
                if *playing {
                    *playing = false;
                    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TransportStop);
                    self.status_toast = FixedStr::from_str_lossy("STOPPED PLAYBACK");
                } else {
                    *playing = true;
                    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TransportPlay {
                        start_song_row: self.song_row as u8,
                    });
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "PLAYING FROM ROW {:02X}",
                        self.song_row
                    ));
                }
            } else if (78..=144).contains(&lx) {
                // [DEMO]
                self.handle_key_down(Keycode::F9, 0, playing, project, ipc);
            } else if (148..=198).contains(&lx) {
                // [ID-]
                self.cycle_current_view_object_id(-1);
            } else if (202..=252).contains(&lx) {
                // [ID+]
                self.cycle_current_view_object_id(1);
            } else if (256..=306).contains(&lx) {
                // [-16]
                self.adjust_current_cell(-16, project, ipc);
            } else if (310..=352).contains(&lx) {
                // [-1]
                self.adjust_current_cell(-1, project, ipc);
            } else if (356..=398).contains(&lx) {
                // [+1]
                self.adjust_current_cell(1, project, ipc);
            } else if (402..=452).contains(&lx) {
                // [+16]
                self.adjust_current_cell(16, project, ipc);
            } else if (456..=508).contains(&lx) {
                // [CLR]
                self.clear_current_cell(project, ipc);
            } else if (512..=564).contains(&lx) {
                // [CLN] Clone Chain/Phrase
                self.clone_current_cell(project, ipc);
            } else if (568..=620).contains(&lx) {
                // [CPY] Copy
                self.copy_current_selection(project);
            } else if (624..=676).contains(&lx) {
                // [PST] Paste
                self.paste_clipboard(project, ipc);
            } else if (680..=732).contains(&lx) {
                // [SAV]
                self.handle_key_down(Keycode::F10, 0, playing, project, ipc);
            } else if (736..=788).contains(&lx) {
                // [LOD]
                self.handle_key_down(Keycode::F11, 0, playing, project, ipc);
            } else if (792..=856).contains(&lx) {
                // [BRW] Open Browser Modal
                self.show_help_guide = false;
                if self.show_browser_modal {
                    self.show_browser_modal = false;
                } else {
                    self.open_or_refresh_browser();
                }
            }
            return;
        }

        // 4. Main Left View Area (y: 84..649)
        if ly >= 650 {
            return;
        }

        if self.show_browser_modal {
            if ly <= 122 {
                if lx >= 720 {
                    self.show_browser_modal = false;
                    self.status_toast = FixedStr::from_str_lossy("CLOSED BROWSER");
                } else if lx < 270 {
                    self.browser_tab = BrowserTab::SamplesWav;
                    self.open_or_refresh_browser();
                } else if lx < 510 {
                    self.browser_tab = BrowserTab::ProjectsToml;
                    self.open_or_refresh_browser();
                } else {
                    self.browser_tab = BrowserTab::SynthProfiles;
                    self.open_or_refresh_browser();
                }
            } else if ly >= 134 && !self.browser_files.is_empty() {
                let vis_idx = ((ly - 134) / 32).min(14);
                let start = self.browser_cursor.saturating_sub(7);
                let target = (start + vis_idx).min(self.browser_files.len() - 1);
                if target == self.browser_cursor {
                    self.load_selected_browser_entry(project, ipc);
                } else {
                    self.browser_cursor = target;
                }
            }
            return;
        }

        if self.show_help_guide {
            if ly <= 120 {
                if lx >= 720 {
                    self.show_help_guide = false;
                    self.status_toast = FixedStr::from_str_lossy("CLOSED QUICK USER GUIDE");
                } else if lx < 235 {
                    self.help_page = 0;
                } else if lx < 495 {
                    self.help_page = 1;
                } else {
                    self.help_page = 2;
                }
            } else if lx < 432 {
                self.help_page = (self.help_page + 2) % 3;
            } else {
                self.help_page = (self.help_page + 1) % 3;
            }
            return;
        }

        self.high_nibble_next = true;
        match self.view {
            M8View::Song => {
                if ly < 120 {
                    if lx >= 90 {
                        let trk = ((lx - 90) / 92).min(NUM_TRACKS - 1);
                        self.active_track = trk;
                        self.song_col = trk;
                        self.sync_context_ids_from_cursor(project);
                    }
                    return;
                }
                let vis_idx = ((ly - 120) / 33).min(15);
                let start_row = self.song_row.saturating_sub(8).min(SONG_ROWS - 16);
                let tapped_row = (start_row + vis_idx).min(SONG_ROWS - 1);
                let tapped_col = if lx >= 90 {
                    ((lx - 90) / 92).min(NUM_TRACKS - 1)
                } else {
                    self.song_col
                };

                if tapped_row == self.song_row && tapped_col == self.song_col {
                    // Tapping the already-selected Song cell drills down into Chain view!
                    let cid = project.song[tapped_row][tapped_col];
                    if cid != EMPTY_U8 {
                        self.current_chain = cid;
                        self.view = M8View::Chain;
                        self.status_toast =
                            FixedStr::from_str_lossy(&format!("OPENED CHAIN {:02X}", cid));
                    } else {
                        self.adjust_current_cell(0, project, ipc);
                    }
                } else {
                    self.song_row = tapped_row;
                    self.song_col = tapped_col;
                    self.active_track = tapped_col;
                    self.sync_context_ids_from_cursor(project);
                }
            }
            M8View::Chain => {
                if ly >= 120 {
                    let step = ((ly - 120) / 33).min(CHAIN_STEPS - 1);
                    let col = if lx >= 220 { 1 } else { 0 };
                    if step == self.chain_row && col == self.chain_col && col == 0 {
                        let pid = project.chains[self.current_chain as usize].rows[step].phrase;
                        if pid != EMPTY_U8 {
                            self.current_phrase = pid;
                            self.view = M8View::Phrase;
                            self.status_toast =
                                FixedStr::from_str_lossy(&format!("OPENED PHRASE {:02X}", pid));
                            return;
                        }
                    }
                    self.chain_row = step;
                    self.chain_col = col;
                    self.sync_context_ids_from_cursor(project);
                }
            }
            M8View::Phrase => {
                if ly >= 120 {
                    let step = ((ly - 120) / 33).min(PHRASE_STEPS - 1);
                    let col = Self::phrase_col_from_x(lx);
                    if step == self.phrase_row && col == self.phrase_col && col == 2 {
                        let iid =
                            project.phrases[self.current_phrase as usize].steps[step].instrument;
                        if iid < NUM_INSTRUMENTS as u8 {
                            self.current_instrument = iid;
                            self.view = M8View::Instrument;
                            self.status_toast =
                                FixedStr::from_str_lossy(&format!("OPENED INSTRUMENT {:02X}", iid));
                            return;
                        }
                    }
                    self.phrase_row = step;
                    self.phrase_col = col;
                    self.sync_context_ids_from_cursor(project);
                }
            }
            M8View::Instrument => {
                if ly >= 124 {
                    let row = ((ly - 124) / 42).min(11);
                    if row == self.inst_param_row && lx >= 420 {
                        let delta = if lx < 520 { -1 } else { 1 };
                        self.adjust_current_cell(delta, project, ipc);
                    } else {
                        self.inst_param_row = row;
                    }
                }
            }
            M8View::Table => {
                if ly >= 120 {
                    let step = ((ly - 120) / 33).min(TABLE_STEPS - 1);
                    let col = Self::table_col_from_x(lx);
                    self.table_row = step;
                    self.table_col = col;
                }
            }
            M8View::Groove => {
                if ly >= 120 {
                    let step = ((ly - 120) / 33).min(GROOVE_STEPS - 1);
                    self.groove_row = step;
                    if lx >= 230 {
                        let ticks = (((lx - 230) / 24) + 1).clamp(1, 16) as u8;
                        let msg = UiToAudioMsg::SetGrooveStep {
                            groove_id: self.current_groove,
                            step: step as u8,
                            ticks,
                        };
                        project.apply_delta(&msg);
                        let _ = ipc.tx_cmd.try_send(msg);
                    }
                }
            }
        }
    }

    fn handle_right_hud_touch(
        &mut self,
        lx: usize,
        ly: usize,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        let rx = 864 + 12; // RIGHT_HUD_X + 12

        // 0. Top-Right [OFF] Instant Power-Off Touch Button (y: 8..42, x: 1344..1436)
        if ly <= 42 && lx >= 1344 {
            self.request_poweroff = true;
            self.status_toast = FixedStr::from_str_lossy("POWERING OFF...");
            return;
        }

        // 0b. Top HUD Master Volume Control (`[-]` / Bar / `[+]` at y: 84..134, x: rx+280..rx+544)
        if (84..=134).contains(&ly) && lx >= rx + 280 {
            if lx < rx + 332 {
                self.adjust_master_volume(-13, project, ipc);
            } else if lx >= rx + 492 {
                self.adjust_master_volume(13, project, ipc);
            } else {
                let rel_x = lx.saturating_sub(rx + 336).min(150);
                let next_vol = ((rel_x * 255) / 150) as u8;
                self.set_master_volume(next_vol, project, ipc);
            }
            return;
        }

        // 1. Song Mini-Map (y: 170..264)
        if (170..=264).contains(&ly) && lx >= rx + 68 {
            let r_off = ((ly - 174) / 20).min(3);
            let base_row = self.song_row.saturating_sub(1).min(SONG_ROWS - 4);
            let s_row = (base_row + r_off).min(SONG_ROWS - 1);
            let trk = ((lx - (rx + 68)) / 58).min(NUM_TRACKS - 1);
            self.song_row = s_row;
            self.song_col = trk;
            self.active_track = trk;
            self.sync_context_ids_from_cursor(project);
            self.status_toast = FixedStr::from_str_lossy(&format!(
                "MINI-MAP: ROW {:02X} TRK{}",
                s_row,
                trk + 1
            ));
            return;
        }

        // 2. 8-Track Peak Meters, Solo (top header y: 300..332), & Mute Toggle (y: 333..478)
        if (300..=478).contains(&ly) && lx >= rx + 12 {
            let trk = ((lx - (rx + 12)) / 66).min(NUM_TRACKS - 1);
            if ly <= 332 && trk == self.active_track {
                self.toggle_track_solo(trk, ipc);
            } else if trk == self.active_track {
                self.solo_track = None;
                self.track_muted[trk] = !self.track_muted[trk];
                let muted = self.track_muted[trk];
                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::SetTrackMute {
                    track: trk as u8,
                    muted,
                });
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "TRK{} {}",
                    trk + 1,
                    if muted { "MUTED" } else { "UNMUTED" }
                ));
            } else {
                self.active_track = trk;
                self.song_col = trk;
                self.sync_context_ids_from_cursor(project);
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("SELECTED TRK{}", trk + 1));
            }
            return;
        }

        // 3. Bottom Live Context Panel (y: 490..712): 12-Pad DrumKit / MIDI CC Sliders / Synth Audition
        if (490..=712).contains(&ly) {
            let mut inst = project.instruments[self.current_instrument as usize];
            match inst.engine {
                InstrumentEnum::DrumKit(ref kit) => {
                    if (530..=696).contains(&ly) && lx >= rx + 14 {
                        let col = ((lx - (rx + 14)) / 134).min(3);
                        let row = ((ly - 530) / 54).min(2);
                        let pad_idx = (row * 4 + col).min(DRUM_PADS - 1);
                        let note = DRUM_BASE_NOTE + pad_idx as u8;
                        let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TriggerPreviewNote {
                            track: self.active_track as u8,
                            note,
                            velocity: 0xF0,
                            instrument: self.current_instrument,
                        });
                        self.status_toast = FixedStr::from_str_lossy(&format!(
                            "PAD {:X}: {} TRIGGERED",
                            pad_idx,
                            kit.pads[pad_idx].name.as_str()
                        ));
                    }
                }
                InstrumentEnum::MidiOut(ref mut midi) => {
                    if (524..=696).contains(&ly) {
                        let slot_idx = ((ly - 524) / 28).min(MIDI_CC_SLOTS - 1);
                        if lx >= rx + 306 {
                            let rel_x = (lx - (rx + 310)).min(180);
                            let val = ((rel_x * 127) / 180) as u8;
                            midi.cc_slots[slot_idx].value = val;
                            let cc_num = midi.cc_slots[slot_idx].cc_number;
                            let chan = midi.channel;
                            let msg = UiToAudioMsg::SetInstrument {
                                inst_id: self.current_instrument,
                                instrument: inst,
                            };
                            project.apply_delta(&msg);
                            let _ = ipc.tx_cmd.try_send(msg);
                            let _ = ipc.tx_cmd.try_send(UiToAudioMsg::SendMidiCcImmediate {
                                channel: chan,
                                cc: cc_num,
                                value: val,
                            });
                            self.status_toast = FixedStr::from_str_lossy(&format!(
                                "CC{:03} SET TO {:02X}",
                                cc_num, val
                            ));
                        }
                    }
                }
                InstrumentEnum::SingleSample(_) | InstrumentEnum::InternalSynth(_) => {
                    let note = ((self.piano_octave as u8) * 12).min(119);
                    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TriggerPreviewNote {
                        track: self.active_track as u8,
                        note,
                        velocity: 0xE0,
                        instrument: self.current_instrument,
                    });
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "AUDITION INST {:02X} ({})",
                        self.current_instrument,
                        inst.name.as_str()
                    ));
                }
            }
        }
    }

    fn phrase_col_from_x(lx: usize) -> usize {
        if lx < 184 {
            0 // Note:Hex
        } else if lx < 248 {
            1 // Vol
        } else if lx < 316 {
            2 // Inst
        } else if lx < 378 {
            3 // FX1 Cmd
        } else if lx < 448 {
            4 // FX1 Val
        } else if lx < 514 {
            5 // FX2 Cmd
        } else if lx < 584 {
            6 // FX2 Val
        } else if lx < 650 {
            7 // FX3 Cmd
        } else {
            8 // FX3 Val
        }
    }

    fn table_col_from_x(lx: usize) -> usize {
        if lx < 164 {
            0 // Transpose
        } else if lx < 248 {
            1 // Vol
        } else if lx < 310 {
            2 // FX1 Cmd
        } else if lx < 384 {
            3 // FX1 Val
        } else if lx < 446 {
            4 // FX2 Cmd
        } else if lx < 520 {
            5 // FX2 Val
        } else if lx < 582 {
            6 // FX3 Cmd
        } else {
            7 // FX3 Val
        }
    }

    /// Copies the current cell/row (or entire Chain/Phrase when Shift is active) to `self.clipboard`.
    pub fn copy_current_selection(&mut self, project: &ProjectState) {
        let full_block = self.mod_shift.is_active();
        if full_block {
            self.mod_shift.mark_chord_consumed();
        }
        match self.view {
            M8View::Song => {
                let cid = project.song[self.song_row][self.song_col];
                if full_block && cid != EMPTY_U8 && (cid as usize) < NUM_CHAINS {
                    self.clipboard = TrackerClipboard::FullChain(project.chains[cid as usize]);
                    self.status_toast =
                        FixedStr::from_str_lossy(&format!("COPIED FULL CHAIN {:02X}", cid));
                } else {
                    self.clipboard = TrackerClipboard::SongCell(cid);
                    self.status_toast =
                        FixedStr::from_str_lossy(&format!("COPIED SONG CELL ({:02X})", cid));
                }
            }
            M8View::Chain => {
                if full_block {
                    self.clipboard =
                        TrackerClipboard::FullChain(project.chains[self.current_chain as usize]);
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "COPIED FULL CHAIN {:02X}",
                        self.current_chain
                    ));
                } else {
                    let row = project.chains[self.current_chain as usize].rows[self.chain_row];
                    self.clipboard = TrackerClipboard::ChainRow(row);
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "COPIED CHAIN STEP {:01X}",
                        self.chain_row
                    ));
                }
            }
            M8View::Phrase => {
                if full_block {
                    self.clipboard =
                        TrackerClipboard::FullPhrase(project.phrases[self.current_phrase as usize]);
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "COPIED FULL PHRASE {:02X}",
                        self.current_phrase
                    ));
                } else {
                    let step =
                        project.phrases[self.current_phrase as usize].steps[self.phrase_row];
                    self.clipboard = TrackerClipboard::PhraseStep(step);
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "COPIED PHRASE STEP {:01X}",
                        self.phrase_row
                    ));
                }
            }
            M8View::Instrument => {
                let inst = project.instruments[self.current_instrument as usize];
                self.clipboard = TrackerClipboard::Instrument(inst);
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "COPIED INSTRUMENT {:02X}",
                    self.current_instrument
                ));
            }
            M8View::Table => {
                let step = project.tables[self.current_table as usize].steps[self.table_row];
                self.clipboard = TrackerClipboard::TableStep(step);
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "COPIED TABLE STEP {:01X}",
                    self.table_row
                ));
            }
            M8View::Groove => {
                let ticks = project.grooves[self.current_groove as usize].ticks[self.groove_row];
                self.clipboard = TrackerClipboard::GrooveStep(ticks);
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "COPIED GROOVE STEP {:01X}",
                    self.groove_row
                ));
            }
        }
    }

    /// Pastes `self.clipboard` into the active view's cursor target and dispatches lock-free IPC deltas.
    pub fn paste_clipboard(&mut self, project: &mut ProjectState, ipc: &UiIpcHandle) {
        match (&self.clipboard, self.view) {
            (TrackerClipboard::SongCell(cid), M8View::Song) => {
                let msg = UiToAudioMsg::SetSongCell {
                    row: self.song_row as u8,
                    track: self.song_col as u8,
                    chain_id: *cid,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("PASTED CHAIN {:02X}", cid));
            }
            (TrackerClipboard::ChainRow(row), M8View::Chain) => {
                let msg = UiToAudioMsg::SetChainRow {
                    chain_id: self.current_chain,
                    step: self.chain_row as u8,
                    row: *row,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast = FixedStr::from_str_lossy("PASTED CHAIN STEP");
            }
            (TrackerClipboard::FullChain(chain), M8View::Chain) => {
                for (step, row) in chain.rows.iter().enumerate() {
                    let msg = UiToAudioMsg::SetChainRow {
                        chain_id: self.current_chain,
                        step: step as u8,
                        row: *row,
                    };
                    project.apply_delta(&msg);
                    let _ = ipc.tx_cmd.try_send(msg);
                }
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "PASTED FULL CHAIN -> {:02X}",
                    self.current_chain
                ));
            }
            (TrackerClipboard::PhraseStep(step_data), M8View::Phrase) => {
                let msg = UiToAudioMsg::SetPhraseStep {
                    phrase_id: self.current_phrase,
                    step: self.phrase_row as u8,
                    data: *step_data,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast = FixedStr::from_str_lossy("PASTED PHRASE STEP");
            }
            (TrackerClipboard::FullPhrase(phrase), M8View::Phrase) => {
                for (step, data) in phrase.steps.iter().enumerate() {
                    let msg = UiToAudioMsg::SetPhraseStep {
                        phrase_id: self.current_phrase,
                        step: step as u8,
                        data: *data,
                    };
                    project.apply_delta(&msg);
                    let _ = ipc.tx_cmd.try_send(msg);
                }
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "PASTED FULL PHRASE -> {:02X}",
                    self.current_phrase
                ));
            }
            (TrackerClipboard::Instrument(inst), M8View::Instrument) => {
                let msg = UiToAudioMsg::SetInstrument {
                    inst_id: self.current_instrument,
                    instrument: *inst,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast = FixedStr::from_str_lossy(&format!(
                    "PASTED INSTRUMENT -> {:02X}",
                    self.current_instrument
                ));
            }
            (TrackerClipboard::TableStep(data), M8View::Table) => {
                let msg = UiToAudioMsg::SetTableStep {
                    table_id: self.current_table,
                    step: self.table_row as u8,
                    data: *data,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast = FixedStr::from_str_lossy("PASTED TABLE STEP");
            }
            (TrackerClipboard::GrooveStep(ticks), M8View::Groove) => {
                let msg = UiToAudioMsg::SetGrooveStep {
                    groove_id: self.current_groove,
                    step: self.groove_row as u8,
                    ticks: *ticks,
                };
                project.apply_delta(&msg);
                let _ = ipc.tx_cmd.try_send(msg);
                self.status_toast = FixedStr::from_str_lossy("PASTED GROOVE STEP");
            }
            _ => {
                self.status_toast =
                    FixedStr::from_str_lossy("CLIPBOARD EMPTY OR VIEW MISMATCH");
            }
        }
    }

    /// M8-Style Clone (`[CLN]` / `Ctrl+D` / `Insert`):
    /// - In `Song` view: allocates the first unused Chain ID, deep-copies the current cell's Chain
    ///   into that new Chain ID, and assigns it to `song[song_row][song_col]`.
    /// - In `Chain` view: allocates the first unused Phrase ID, deep-copies the current cell's Phrase
    ///   into that new Phrase ID, and assigns it to `chains[current_chain].rows[chain_row].phrase`.
    pub fn clone_current_cell(&mut self, project: &mut ProjectState, ipc: &UiIpcHandle) {
        match self.view {
            M8View::Song => {
                let Some(new_cid) = project.find_unused_chain() else {
                    self.status_toast = FixedStr::from_str_lossy("NO UNUSED CHAINS FREE!");
                    return;
                };
                let src_cid = project.song[self.song_row][self.song_col];
                if src_cid != EMPTY_U8 && (src_cid as usize) < NUM_CHAINS {
                    let src_chain = project.chains[src_cid as usize];
                    for (s, r) in src_chain.rows.iter().enumerate() {
                        let msg = UiToAudioMsg::SetChainRow {
                            chain_id: new_cid,
                            step: s as u8,
                            row: *r,
                        };
                        project.apply_delta(&msg);
                        let _ = ipc.tx_cmd.try_send(msg);
                    }
                }
                let cell_msg = UiToAudioMsg::SetSongCell {
                    row: self.song_row as u8,
                    track: self.song_col as u8,
                    chain_id: new_cid,
                };
                project.apply_delta(&cell_msg);
                let _ = ipc.tx_cmd.try_send(cell_msg);
                self.current_chain = new_cid;
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("CLONED -> NEW CHAIN {:02X}", new_cid));
            }
            M8View::Chain => {
                let Some(new_pid) = project.find_unused_phrase() else {
                    self.status_toast = FixedStr::from_str_lossy("NO UNUSED PHRASES FREE!");
                    return;
                };
                let mut cur_row = project.chains[self.current_chain as usize].rows[self.chain_row];
                let src_pid = cur_row.phrase;
                if src_pid != EMPTY_U8 && (src_pid as usize) < NUM_PHRASES {
                    let src_phrase = project.phrases[src_pid as usize];
                    for (s, step_data) in src_phrase.steps.iter().enumerate() {
                        let msg = UiToAudioMsg::SetPhraseStep {
                            phrase_id: new_pid,
                            step: s as u8,
                            data: *step_data,
                        };
                        project.apply_delta(&msg);
                        let _ = ipc.tx_cmd.try_send(msg);
                    }
                }
                cur_row.phrase = new_pid;
                let row_msg = UiToAudioMsg::SetChainRow {
                    chain_id: self.current_chain,
                    step: self.chain_row as u8,
                    row: cur_row,
                };
                project.apply_delta(&row_msg);
                let _ = ipc.tx_cmd.try_send(row_msg);
                self.current_phrase = new_pid;
                self.status_toast =
                    FixedStr::from_str_lossy(&format!("CLONED -> NEW PHRASE {:02X}", new_pid));
            }
            _ => {
                self.copy_current_selection(project);
            }
        }
    }

    /// Toggles Track Solo on `trk` (`0..7`), muting all other tracks when soloed or unmuting all when cleared.
    pub fn toggle_track_solo(&mut self, trk: usize, ipc: &UiIpcHandle) {
        let target = trk % NUM_TRACKS;
        if self.solo_track == Some(target) {
            self.solo_track = None;
            for t in 0..NUM_TRACKS {
                self.track_muted[t] = false;
                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::SetTrackMute {
                    track: t as u8,
                    muted: false,
                });
            }
            self.status_toast = FixedStr::from_str_lossy("SOLO CLEARED (ALL TRACKS UNMUTED)");
        } else {
            self.solo_track = Some(target);
            for t in 0..NUM_TRACKS {
                let muted = t != target;
                self.track_muted[t] = muted;
                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::SetTrackMute {
                    track: t as u8,
                    muted,
                });
            }
            self.status_toast =
                FixedStr::from_str_lossy(&format!("SOLOED TRK{}", target + 1));
        }
    }

    /// Opens or refreshes the 3-tab In-App Browser Modal (`SAMPLES (.WAV)` | `PROJECTS (.TOML)` | `SYNTH PROFILES`).
    pub fn open_or_refresh_browser(&mut self) {
        self.show_browser_modal = true;
        self.browser_files = match self.browser_tab {
            BrowserTab::SamplesWav => list_files_with_extension(&resolve_samples_dir(), "wav"),
            BrowserTab::ProjectsToml => list_files_with_extension(&resolve_projects_dir(), "toml"),
            BrowserTab::SynthProfiles => list_files_with_extension(&resolve_synths_dir(), "toml"),
        };
        if self.browser_cursor >= self.browser_files.len() {
            self.browser_cursor = 0;
        }
        self.status_toast = FixedStr::from_str_lossy(&format!(
            "BROWSER: {} ({} FILES)",
            self.browser_tab.title(),
            self.browser_files.len()
        ));
    }

    /// Loads the currently highlighted file in the Browser Modal into the active instrument or project.
    pub fn load_selected_browser_entry(
        &mut self,
        project: &mut ProjectState,
        ipc: &UiIpcHandle,
    ) {
        let Some(path) = self.browser_files.get(self.browser_cursor).cloned() else {
            return;
        };
        let fname = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("FILE")
            .to_uppercase();

        match self.browser_tab {
            BrowserTab::SamplesWav => {
                if let Ok(sb) = load_wav_to_sample_buffer(&path) {
                    let mut inst = project.instruments[self.current_instrument as usize];
                    match inst.engine {
                        InstrumentEnum::DrumKit(_) => {
                            let pad_idx = (self.inst_param_row.saturating_sub(7)) % DRUM_PADS;
                            for vel_layer in 0..3u8 {
                                let _ = ipc.tx_cmd.try_send(UiToAudioMsg::LoadDrumPadLayer {
                                    inst_id: self.current_instrument,
                                    pad_idx: pad_idx as u8,
                                    vel_layer,
                                    sample: sb.clone(),
                                });
                            }
                            let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TriggerPreviewNote {
                                track: self.active_track as u8,
                                note: DRUM_BASE_NOTE + pad_idx as u8,
                                velocity: 0xF0,
                                instrument: self.current_instrument,
                            });
                            self.status_toast = FixedStr::from_str_lossy(&format!(
                                "LOADED {} -> PAD {:X}",
                                fname, pad_idx
                            ));
                        }
                        _ => {
                            if let InstrumentEnum::SingleSample(ref mut samp) = inst.engine {
                                samp.sample_name = FixedStr::from_str_lossy(&fname);
                                let msg = UiToAudioMsg::SetInstrument {
                                    inst_id: self.current_instrument,
                                    instrument: inst,
                                };
                                project.apply_delta(&msg);
                                let _ = ipc.tx_cmd.try_send(msg);
                            }
                            let _ = ipc.tx_cmd.try_send(UiToAudioMsg::LoadSingleSample {
                                inst_id: self.current_instrument,
                                sample: sb,
                            });
                            let _ = ipc.tx_cmd.try_send(UiToAudioMsg::TriggerPreviewNote {
                                track: self.active_track as u8,
                                note: DRUM_BASE_NOTE,
                                velocity: 0xE8,
                                instrument: self.current_instrument,
                            });
                            self.status_toast = FixedStr::from_str_lossy(&format!(
                                "LOADED {} -> INST {:02X}",
                                fname, self.current_instrument
                            ));
                        }
                    }
                }
            }
            BrowserTab::ProjectsToml => {
                if let Ok(loaded) = load_project_from_path(&path) {
                    *project = loaded;
                    ipc.sync_project_to_audio(project);
                    self.sync_context_ids_from_cursor(project);
                    self.show_browser_modal = false;
                    self.status_toast =
                        FixedStr::from_str_lossy(&format!("LOADED SONG: {}", fname));
                }
            }
            BrowserTab::SynthProfiles => {
                if let Ok(prof) = load_synth_profile_from_path(&path) {
                    let mut inst = project.instruments[self.current_instrument as usize];
                    let mut midi_params = match inst.engine {
                        InstrumentEnum::MidiOut(m) => m,
                        _ => Default::default(),
                    };
                    prof.apply_to_midi_out(&mut midi_params);
                    inst.engine = InstrumentEnum::MidiOut(midi_params);
                    let msg = UiToAudioMsg::SetInstrument {
                        inst_id: self.current_instrument,
                        instrument: inst,
                    };
                    project.apply_delta(&msg);
                    let _ = ipc.tx_cmd.try_send(msg);
                    self.show_browser_modal = false;
                    self.status_toast = FixedStr::from_str_lossy(&format!(
                        "APPLIED PROFILE {} -> INST {:02X}",
                        prof.name, self.current_instrument
                    ));
                }
            }
        }
    }
}

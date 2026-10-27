use crate::audio::sequencer::SAMPLE_RATE;
use crate::ipc::{SampleBuffer, UiIpcHandle, UiToAudioMsg};
use crate::model::{
    FixedStr, MidiCcSlot, MidiOutParams, ProjectState, DRUM_PADS, VEL_LAYERS,
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DEMO_SONG_FILENAME: &str = "cyber_pine_demo.toml";
pub const USER_SONG_FILENAME: &str = "user_song.toml";

// ============================================================================
// Hardware 5-Pin DIN MIDI Synth Profile (.toml)
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SynthCcDef {
    pub cc: u8,
    pub default_value: u8,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SynthProfile {
    pub name: String,
    pub default_channel: u8,
    pub parameters: Vec<SynthCcDef>,
}

impl SynthProfile {
    /// Applies this `.toml` synth profile's human-readable CC mappings onto a `MidiOutParams` struct.
    pub fn apply_to_midi_out(&self, midi: &mut MidiOutParams) {
        midi.profile_name = FixedStr::from_str_lossy(&self.name);
        midi.channel = self.default_channel.min(15);
        for (i, slot) in midi.cc_slots.iter_mut().enumerate() {
            if let Some(def) = self.parameters.get(i) {
                *slot = MidiCcSlot {
                    cc_number: def.cc.min(127),
                    value: def.default_value.min(127),
                    name: FixedStr::from_str_lossy(&def.name),
                };
            }
        }
    }
}

/// Resolves the active projects directory (`/projects` on PinePhone Stage 2 Prod OS if writable,
/// or `./data/projects` on Steam Deck / dev host).
pub fn resolve_projects_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("PINE_TRACKER_PROJECTS_DIR") {
        let p = PathBuf::from(custom);
        let _ = fs::create_dir_all(&p);
        return p;
    }

    let pine_prod = Path::new("/projects");
    if pine_prod.is_dir()
        && fs::metadata(pine_prod)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false)
    {
        return pine_prod.to_path_buf();
    }

    let local = PathBuf::from("data/projects");
    let _ = fs::create_dir_all(&local);
    local
}

/// Resolves `/synths` on PinePhone or `./data/synths` on Steam Deck.
pub fn resolve_synths_dir() -> PathBuf {
    let pine_synths = Path::new("/synths");
    if pine_synths.is_dir()
        && fs::metadata(pine_synths)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false)
    {
        return pine_synths.to_path_buf();
    }
    let local = PathBuf::from("data/synths");
    let _ = fs::create_dir_all(&local);
    local
}

/// Resolves `./data/samples` for `.wav` kits and single-samples.
pub fn resolve_samples_dir() -> PathBuf {
    let local = PathBuf::from("data/samples");
    let _ = fs::create_dir_all(&local);
    local
}

/// Seeds 3 starter hardware synth `.toml` profiles (`roland_sh01a.toml`, `elektron_digitakt.toml`,
/// `korg_minilogue_xd.toml`) and returns the parsed profiles.
pub fn ensure_starter_synth_profiles() -> Vec<(PathBuf, SynthProfile)> {
    let dir = resolve_synths_dir();
    let starters = vec![
        (
            "roland_sh01a.toml",
            SynthProfile {
                name: "ROLAND_SH01A".to_string(),
                default_channel: 0,
                parameters: vec![
                    SynthCcDef { cc: 74, default_value: 96, name: "VCF CUTOFF".to_string() },
                    SynthCcDef { cc: 71, default_value: 32, name: "VCF RESO".to_string() },
                    SynthCcDef { cc: 24, default_value: 64, name: "ENV MOD".to_string() },
                    SynthCcDef { cc: 16, default_value: 80, name: "LFO RATE".to_string() },
                    SynthCcDef { cc: 19, default_value: 64, name: "SUB LEVEL".to_string() },
                    SynthCcDef { cc: 91, default_value: 40, name: "CHORUS/REV".to_string() },
                ],
            },
        ),
        (
            "elektron_digitakt.toml",
            SynthProfile {
                name: "ELEK_DIGITAKT".to_string(),
                default_channel: 1,
                parameters: vec![
                    SynthCcDef { cc: 74, default_value: 100, name: "FLT FREQ".to_string() },
                    SynthCcDef { cc: 75, default_value: 24, name: "FLT RESO".to_string() },
                    SynthCcDef { cc: 16, default_value: 64, name: "TUNE".to_string() },
                    SynthCcDef { cc: 19, default_value: 0, name: "SAMPLE START".to_string() },
                    SynthCcDef { cc: 81, default_value: 36, name: "OVERDRIVE".to_string() },
                    SynthCcDef { cc: 85, default_value: 48, name: "DELAY SEND".to_string() },
                ],
            },
        ),
        (
            "korg_minilogue_xd.toml",
            SynthProfile {
                name: "MINILOGUE_XD".to_string(),
                default_channel: 2,
                parameters: vec![
                    SynthCcDef { cc: 43, default_value: 90, name: "CUTOFF".to_string() },
                    SynthCcDef { cc: 44, default_value: 40, name: "RESONANCE".to_string() },
                    SynthCcDef { cc: 36, default_value: 64, name: "VCO1 SHAPE".to_string() },
                    SynthCcDef { cc: 37, default_value: 64, name: "VCO2 SHAPE".to_string() },
                    SynthCcDef { cc: 22, default_value: 52, name: "EG INT".to_string() },
                    SynthCcDef { cc: 26, default_value: 45, name: "LFO INT".to_string() },
                ],
            },
        ),
    ];

    let mut out = Vec::with_capacity(starters.len());
    for (filename, profile) in starters {
        let path = dir.join(filename);
        if !path.exists() {
            if let Ok(toml_str) = toml::to_string_pretty(&profile) {
                let _ = fs::write(&path, toml_str);
            }
        }
        if let Ok(loaded) = load_synth_profile(&path) {
            out.push((path, loaded));
        } else {
            out.push((path, profile));
        }
    }
    out
}

pub fn load_synth_profile(path: &Path) -> Result<SynthProfile, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str::<SynthProfile>(&raw).map_err(|e| e.to_string())
}

// ============================================================================
// Stereo 16-Bit .wav Sample Loader & Starter Kit Generator (via `hound`)
// ============================================================================

/// Loads a 16-bit PCM `.wav` file from disk into a zero-allocation-ready `SampleBuffer` (`Arc<[i16]>`).
pub fn load_wav_to_sample_buffer(path: &Path) -> Result<SampleBuffer, String> {
    let mut reader = WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let channels = spec.channels.clamp(1, 2) as u8;
    let sample_rate = spec.sample_rate.max(8_000);

    let pcm_vec: Vec<i16> = match spec.sample_format {
        SampleFormat::Int => {
            if spec.bits_per_sample <= 16 {
                reader
                    .samples::<i16>()
                    .filter_map(Result::ok)
                    .collect()
            } else {
                reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|s| (s >> (spec.bits_per_sample - 16)) as i16)
                    .collect()
            }
        }
        SampleFormat::Float => reader
            .samples::<f32>()
            .filter_map(Result::ok)
            .map(|f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect(),
    };

    Ok(SampleBuffer {
        data: Arc::from(pcm_vec.into_boxed_slice()),
        channels,
        sample_rate,
    })
}

/// Writes a stereo 16-bit 48 kHz `.wav` file to `path` using `hound::WavWriter`.
pub fn write_stereo_i16_wav(path: &Path, interleaved_samples: &[i16]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let spec = WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in interleaved_samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

/// Synthesizes a multi-velocity stereo `.wav` sample for DrumKit pad `pad_idx` (`0..11`) and `vel_layer` (`0..2`).
fn synthesize_drum_pad_wav_pcm(pad_idx: usize, vel_layer: usize) -> Vec<i16> {
    let frames = match pad_idx {
        5 | 6 => 3_600,       // Closed / Pedal Hat (~75 ms)
        8 | 10 | 11 => 14_400, // Open Hat / Crash / Ride (~300 ms, demonstrates 3ms hat choke!)
        _ => 7_200,           // Kick / Snare / Clap / Toms (~150 ms)
    };
    let vel_gain = match vel_layer {
        0 => 0.48_f32, // Soft
        1 => 0.76_f32, // Medium
        _ => 0.98_f32, // Hard
    };
    let brightness = 0.6 + (vel_layer as f32) * 0.25;

    let mut out = Vec::with_capacity(frames * 2);
    let mut phase = 0.0_f32;
    let mut pitch_env = 1.0_f32;
    let mut amp_env = 1.0_f32;
    let mut rng = 0xCAFE_BABEu32 ^ ((pad_idx as u32) << 8) ^ (vel_layer as u32);

    for _ in 0..frames {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let noise = ((rng & 0xFFFF) as f32 / 32768.0) - 1.0;
        pitch_env *= 0.992;

        let (decay, sig) = match pad_idx {
            0 => {
                // Kick: deep sine + velocity-dependent punch transient
                let f = 50.0 + 195.0 * brightness * pitch_env * pitch_env;
                phase = (phase + f / SAMPLE_RATE as f32).fract();
                let s = (phase * std::f32::consts::TAU).sin();
                let drive = (s * (1.1 + 0.4 * brightness)).tanh();
                (0.9988, drive)
            }
            1 => {
                // Rimshot
                let f = 430.0 + 320.0 * pitch_env;
                phase = (phase + f / SAMPLE_RATE as f32).fract();
                (0.9955, (phase * std::f32::consts::TAU).sin() * 0.8 + noise * 0.2)
            }
            2 | 3 => {
                // Snare / Clap
                let f = 185.0 + 95.0 * pitch_env;
                phase = (phase + f / SAMPLE_RATE as f32).fract();
                let body = (phase * std::f32::consts::TAU).sin() * 0.5;
                (0.9982, body + noise * 0.75 * brightness)
            }
            5 | 6 => {
                // Closed / Pedal Hat
                phase = (phase + 5200.0 / SAMPLE_RATE as f32).fract();
                let sq = if phase > 0.5 { 0.35 } else { -0.35 };
                (0.9945, (noise * 0.75 + sq) * 0.85)
            }
            8 | 10 | 11 => {
                // Open Hat / Crash / Ride
                phase = (phase + 4400.0 / SAMPLE_RATE as f32).fract();
                let sq = if phase > 0.5 { 0.4 } else { -0.4 };
                (0.99945, (noise * 0.70 + sq) * 0.85)
            }
            _ => {
                // Toms
                let base = 92.0 + (pad_idx as f32) * 24.0;
                let f = base * (1.0 + 0.75 * pitch_env);
                phase = (phase + f / SAMPLE_RATE as f32).fract();
                (0.9985, (phase * std::f32::consts::TAU).sin())
            }
        };

        amp_env *= decay;
        let sample_i16 = ((sig * amp_env * vel_gain).clamp(-1.0, 1.0) * 32000.0) as i16;
        out.push(sample_i16);
        out.push(sample_i16);
    }

    out
}

/// Synthesizes a multi-slice stereo bass `.wav` sample for `SingleSample` instrument 01.
fn synthesize_single_sample_bass_wav_pcm() -> Vec<i16> {
    let frames = 24_000; // 0.5s @ 48 kHz (16 slices of 1,500 frames each)
    let mut out = Vec::with_capacity(frames * 2);
    let mut phase = 0.0_f32;
    let mut sub_phase = 0.0_f32;
    let freq = 130.8128_f32; // C-3 / C-4 root

    for frame in 0..frames {
        // Add subtle harmonic variation across slices so `OFS` slice offsets have distinct character
        let slice_idx = (frame * 16) / frames;
        let harmonic_boost = 1.0 + (slice_idx as f32) * 0.06;
        let dt = freq / SAMPLE_RATE as f32;
        phase = (phase + dt).fract();
        sub_phase = (sub_phase + dt * 0.5).fract();

        let saw = (phase * 2.0 - 1.0) * 0.65;
        let sub = if sub_phase < 0.5 { 0.35 } else { -0.35 };
        let l = ((saw * harmonic_boost + sub).tanh() * 28000.0) as i16;
        let r = ((saw + sub * harmonic_boost).tanh() * 28000.0) as i16;
        out.push(l);
        out.push(r);
    }
    out
}

/// Generates starter `.wav` files in `data/samples/` (if not already present), loads them via `hound`,
/// and streams the `Arc<[i16]>` buffers to Thread 2 via lock-free `UiToAudioMsg` commands.
pub fn ensure_and_load_starter_wav_bank(ipc: &UiIpcHandle) -> Result<usize, String> {
    let samples_dir = resolve_samples_dir();
    let mut loaded_count = 0usize;

    // 1. 12 Drum Pads x 3 Velocity Layers for Instrument 00 (`DrumKit`)
    for pad_idx in 0..DRUM_PADS {
        for vel_layer in 0..VEL_LAYERS {
            let filename = format!("kit00_pad{:02}_vel{}.wav", pad_idx, vel_layer);
            let path = samples_dir.join(filename);
            if !path.exists() {
                let pcm = synthesize_drum_pad_wav_pcm(pad_idx, vel_layer);
                write_stereo_i16_wav(&path, &pcm)?;
            }
            let sb = load_wav_to_sample_buffer(&path)?;
            let _ = ipc.tx_cmd.try_send(UiToAudioMsg::LoadDrumPadLayer {
                inst_id: 0,
                pad_idx: pad_idx as u8,
                vel_layer: vel_layer as u8,
                sample: sb,
            });
            loaded_count += 1;
        }
    }

    // 2. Sliced Stereo Bass WAV for Instrument 01 (`SingleSample`)
    let bass_path = samples_dir.join("bass_saw.wav");
    if !bass_path.exists() {
        let pcm = synthesize_single_sample_bass_wav_pcm();
        write_stereo_i16_wav(&bass_path, &pcm)?;
    }
    let bass_sb = load_wav_to_sample_buffer(&bass_path)?;
    let _ = ipc.tx_cmd.try_send(UiToAudioMsg::LoadSingleSample {
        inst_id: 1,
        sample: bass_sb,
    });
    loaded_count += 1;

    Ok(loaded_count)
}

/// Ensures `data/projects/cyber_pine_demo.toml` exists on disk so the user can inspect,
/// edit, or reload the test song at any time (via `F9` or `--load`).
pub fn ensure_demo_song_on_disk() -> PathBuf {
    let dir = resolve_projects_dir();
    let path = dir.join(DEMO_SONG_FILENAME);
    if !path.exists() {
        let demo = ProjectState::default();
        let _ = save_project_to_path(&demo, &path);
    }
    path
}

pub fn save_project_to_path(project: &ProjectState, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let encoded = toml::to_string(project).map_err(|e| e.to_string())?;
    fs::write(path, encoded).map_err(|e| e.to_string())
}

pub fn load_project_from_path(path: &Path) -> Result<ProjectState, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str::<ProjectState>(&raw).map_err(|e| e.to_string())
}

pub fn load_or_recreate_demo_song() -> (ProjectState, PathBuf) {
    let dir = resolve_projects_dir();
    let path = dir.join(DEMO_SONG_FILENAME);
    let demo = ProjectState::default();
    let _ = save_project_to_path(&demo, &path);
    (demo, path)
}

/// Lists all files in `dir` matching the case-insensitive extension `ext` (e.g., `"toml"` or `"wav"`),
/// sorted alphabetically for the In-App Browser Modal.
pub fn list_files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(e) = p.extension().and_then(|s| s.to_str()) {
                    if e.eq_ignore_ascii_case(ext) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

pub fn load_synth_profile_from_path(path: &Path) -> Result<SynthProfile, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str::<SynthProfile>(&raw).map_err(|e| e.to_string())
}

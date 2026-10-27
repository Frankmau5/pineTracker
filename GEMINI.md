# Project: pineTracker (v2.0 Architecture Specification)

## 1. Overview & Target Hardware
- **Project Name:** `pineTracker` (binary/crate: `pine_tracker`)
- **Concept:** A custom M8 / LittleGPTracker-style 8-track hexadecimal music tracker and groovebox written in Rust.
- **Target Device:** Pine64 PinePhone (Allwinner A64: 4x ARM Cortex-A53 @ 1.15 GHz, Mali-400 GPU, 2GB/3GB RAM) docked in the official **PinePhone I2C Physical Keyboard Case** (`KB151` matrix @ I2C `0x15`, `IP5209` charger, 9,000 mAh combined battery).
- **Bootloader & OS Stages:**
  - Bootloader: **Tow-Boot** (already installed; Volume Up exposes USB Mass Storage).
  - Stage 1 Dev OS: **DanctNIX Arch Linux ARM (Barebone)** with the `linux-megi` kernel on microSD.
  - Stage 2 Prod OS: **Buildroot SquashFS (Read-Only Root)** + `linux-megi` with `PREEMPT_RT`, `/projects` mounted sync, `isolcpus=2,3 nohz_full=2,3`, `snd_soc_core.pmdown_time=0` (anti-pop), and USB/I2C IRQs pinned to Cores 0-1 (`smp_affinity=3`).
- **Development Host:** Steam Deck (`x86_64`) running an Arch Linux Distrobox container.

## 2. Threading & Real-Time Audio/MIDI Rules (STRICT)
- **Dual-Thread Split:**
  - **Thread 1 (Cores 0-1, 60 FPS UI & Input):** Runs `sdl2` (`KMSDRM` rotated 90° to `1440x720` landscape on PinePhone; windowed `1440x720` on Steam Deck). Left 60% (`864x720`) renders integer-scaled M8 views (`Song`, `Chain`, `Phrase`, `Instrument`, `Table`, `Groove`). Right 40% (`576x720`) renders the live Context HUD (Mini-Map, 8-track meters, dual I2C battery gauges, named MIDI CC parameters).
  - **Thread 2 (Isolated Cores 2-3, `SCHED_FIFO` Priority 95):** Runs the sample-accurate 24 PPQN / 192 PPQN clock, audio DSP mixer (48 kHz, 128-sample buffer = `2.66 ms` hard deadline), and 5-pin DIN MIDI output via `alsa`.
- **Concurrency:** Use lock-free `crossbeam-channel` SPSC queues between UI and Audio threads.
- **Zero-Allocation Audio Rule:** NEVER allocate heap memory (`Vec::push`, `Box::new`, `String`), lock a blocking `Mutex`, or perform file/SD card I/O inside the real-time audio callback.
- **SIMD Optimization:** Keep sample mixing loops contiguous so `-C target-cpu=cortex-a53 -C target-feature=+neon` auto-vectorizes across 4 lanes.

## 3. Input & Anti-Ghosting Keyboard Spec (`KB151`)
- The PinePhone Keyboard Case has a passive key matrix without per-key diodes.
- **Anti-Ghosting Rule:** Bind M8 modifier chords (`Shift`, `Option`, `Edit`, `Play`) to dedicated hardware modifier lines outside the alpha matrix (`Left Shift`, `Ctrl`, `AltGr`, `Pine` key, `Space`) plus implement a **Sticky/Latch toggle** mode in Rust.
- **Dual Input Mode (`Fn + Space` toggle):**
  1. **M8 Thumb Mode:** 8-button compass navigation + direct `0-9` / `A-F` hex entry.
  2. **QWERTY Piano Mode:** 2-octave chromatic keyboard (`Z-M` and `Q-P`) for live step entry.

## 4. Sequencer, Groove (`GRV`), & 4 Instrument Engines
- **Decoupled Groove Clock:** Outbound 5-pin DIN MIDI Timing Clock (`0xF8`) MUST pulse at a rock-steady 24 PPQN so external hardware delays/arpeggiators never warble. Each of the 8 tracks uses its own `Groove` table step accumulator (e.g., `07` / `05` ticks) to swing internal samples and outbound `Note On` (`0x90`) messages.
- **4 Instrument Types (`InstrumentEnum`):**
  1. `DrumKit` (Acoustic Multi-Velocity Kit): Maps 12 chromatic notes (`C-4` to `B-4`) to 12 drum pads. Each pad supports 3 velocity-switched stereo `.wav` layers (stored as `i16` slices in RAM via `hound`). Uses an **8-voice internal sub-pool** per kit with 1.5ms same-pad retrigger crossfade and 3ms Hi-Hat **Choke Groups** (Closed/Pedal Hat chokes Open Hat).
  2. `SingleSample`: Pitched/sliced `.wav` playback with resonant State Variable Filter (SVF).
  3. `InternalSynth`: 4-op FM and Virtual Analog synth voices.
  4. `MidiOut`: Hardware 5-pin DIN MIDI output via USB-C hub to a physical MIDI Thru box. Loads `/synths/*.toml` profiles (`serde` + `toml`) to map raw MIDI CC numbers to human-readable parameter names on the HUD.

## 5. Dual-DAC & Cross-Compilation Workflow
- **Audio Device Routing:**
  - On Steam Deck (`--features deck-dev` or env var): Open ALSA `"default"` (routes cleanly through host PipeWire) while opening real `/dev/snd/midi*` for USB 5-pin MIDI testing.
  - On PinePhone: Prefer `"hw:1,0"` (external USB-C DAC on hub) and automatically catch `-ENODEV` / `-EPIPE` on hub unplug to fall back to `"hw:0,0"` (internal Allwinner A64 3.5mm codec) without crashing or stopping the sequencer clock.

# 🌲 pineTracker v2.0

**An 8-Track Hexadecimal Music Tracker & Hardware Groovebox for the Pine64 PinePhone + Physical Keyboard Case (`KB151`)**

`pineTracker` (`pine_tracker`) is a custom Dirtywave M8 / LittleGPTracker-inspired 8-track hexadecimal music tracker, synthesizer, multi-velocity drum sampler, and hardware 5-pin DIN MIDI groovebox written in Rust.

Designed specifically for the **Pine64 PinePhone** (Allwinner A64: 4x ARM Cortex-A53 @ 1.15 GHz) docked in the **Official PinePhone I2C Physical Keyboard Case (`KB151`)**, `pineTracker` transforms the PinePhone into a dedicated, instant-on portable hardware instrument that **boots directly to the tracker in ~2.5 seconds** (`PID 1` Appliance Mode) and **powers off cleanly in <0.5 seconds**.

It also runs natively on **Linux x86_64 / Steam Deck** (`1440x720` windowed via SDL2 + PipeWire/ALSA) for cross-platform composition and development.

---

## ✨ Key Features

### 1. Instant-On Hardware Appliance (~2.5s Boot / <0.5s Power-Off)
- **Direct-to-Tracker `PID 1` Init (`pinetracker-init`):** Bypasses `initramfs`, `systemd`, `udev`, `NetworkManager`, and desktop display servers completely. The Linux kernel (`linux-megi`) mounts `/proc`, `/sys`, and `/dev` (`devtmpfs`) and launches `pine_tracker` directly on `/dev/fb0` and `/dev/input/event*`.
- **2-Second Pixel-Art Boot Splash:** Displays a custom 32x32 pixel-art Pine Tree + Synth Chip emblem (`192x192` integer-scaled) while ALSA and I2C kernel modules initialize in the background.
- **Zero-Data-Loss Instant Shutdown:** Pressing the physical **PinePhone Power button**, pressing **`Option + Shift + Q`**, or tapping the **`[OFF]`** badge on the touchscreen flushes filesystems (`sync`), remounts `/` read-only, and powers off the PMIC in `<0.5s`.

### 2. Hierarchical M8-Style 8-Track Hexadecimal Sequencer
- **6 Hierarchical Views (`F1`–`F6`):**
  1. **`SONG` (`F1`):** `256 rows × 8 tracks` master arrangement grid referencing Chains (`00`–`FE`).
  2. **`CHAIN` (`F2`):** `16 steps` per Chain referencing Phrases (`00`–`FE`) with per-step semitone transpose (`TRN`).
  3. **`PHRASE` (`F3`):** `16 steps` per Phrase containing `NOTE` (`C-0`–`B-9` / `OFF`), `VEL` (`00`–`7F`), `INST` (`00`–`7F`), and **3 simultaneous FX slots** (`FX1`, `FX2`, `FX3`) per step.
  4. **`INSTRUMENT` (`F4`):** `128 instruments` (`00`–`7F`) selectable across 4 synthesis/sampling engines, plus per-instrument Delay/Reverb sends and Modulation Table links.
  5. **`TABLE` (`F5`):** `16-step` auxiliary modulation sequencers running alongside notes with independent transposition, volume scaling, and 3 FX lanes.
  6. **`GROOVE` (`F6`):** `16-step` custom tick-duration tables (`24 PPQN` ticks per step) for per-track swing and polymetric micro-timing.
- **Sub-Tick Tracker FX Commands:**
  - `ARP` (Arpeggio), `PIT` (Pitch Slide), `VOL` (Volume Slide), `CUT` (Sub-tick Note Cut), `RET` (Sub-tick Note Retrigger with volume ramp), `DEL` (Sub-tick Note Delay), `FIN` (Fine Detune), `FMP` (FM Feedback/Mod Depth), `KIL` (Kill Voice), `GRV` (Switch Track Groove), `TBL` (Switch Track Table), `TBX` (Table Tick Rate).

### 3. Decoupled 24-PPQN MIDI Clock vs. Internal Swing (`GRV`)
- Outbound 5-pin DIN MIDI Timing Clock (`0xF8`) pulses at a **rock-steady 24 PPQN** (`192 PPQN` internal sub-tick engine) so external hardware delays, loopers, and arpeggiators never warble when a track uses heavy swing (`e.g., 07 / 05` Groove tables).
- Each of the 8 tracks maintains its own independent Groove step accumulator to swing internal audio voices and outbound MIDI `Note On` (`0x90`) / `Note Off` (`0x80`) events.

### 4. Four Instrument Engines (`128` Instrument Slots)
1. **`DrumKit` (12-Pad Acoustic Multi-Velocity Sampler):**
   - Maps 12 chromatic notes (`C-4` to `B-4`) to 12 drum pads (`KICK`, `SNARE`, `RIM/CLAP`, `CLOSED HAT`, `PEDAL HAT`, `OPEN HAT`, `LOW TOM`, `MID TOM`, `HIGH TOM`, `CRASH`, `RIDE`, `PERC`).
   - **3 Velocity-Switched Stereo `.wav` Layers per Pad** (`Soft: 01–3F`, `Med: 40–67`, `Hard: 68–7F`) stored in RAM as contiguous `i16` slices.
   - **8-Voice Internal Polyphony Pool per Kit** with **1.5ms (72-frame) linear crossfade** on same-pad retriggers (zero click) and **3.0ms (144-frame) Hi-Hat Choke Groups** (`Closed Hat` / `Pedal Hat` automatically chokes `Open Hat`).
2. **`SingleSample` (Chromatic / Sliced `.wav` Sampler):**
   - Pitched/sliced playback (`1–64` slices) with `OneShot`, `ForwardLoop`, and `PingPongLoop` modes.
   - Zero-delay feedback **Cytomic State Variable Filter (SVF)** (`LowPass`, `BandPass`, `HighPass`) with resonance.
3. **`InternalSynth` (4-Operator FM & PolyBLEP Virtual Analog):**
   - **4-Operator FM Engine:** All **8 classic DX-style algorithms** (`Alg 0`–`7`) with Operator 1 self-feedback, 4 ratio/level pairs, and sub-tick `FMP` modulation.
   - **PolyBLEP Virtual Analog Engine:** Anti-aliased `Saw`, `Pulse` (variable PWM), `Saw+Sub`, and `Noise+SVF` waveforms routed through the resonant Cytomic SVF.
4. **`MidiOut` (Hardware 5-Pin DIN MIDI + Named Synth Profiles):**
   - Drives external hardware synthesizers over a USB-C MIDI interface / 5-pin DIN MIDI Thru box (`1–16` channels, `Bank MSB/LSB`, `Program Change`, and `4 macro CC lanes`).
   - Loads human-readable `.toml` profiles from `data/synths/*.toml` (e.g., `roland_j6.toml`, `behringer_pro_vs_mini.toml`, `elektron_model_cycles.toml`) to display named hardware parameters directly on the Context HUD.

### 5. Master Send FX Bus & Anti-Clipping Summing Pipeline
- **Zero-Allocation Master Send FX:** Pre-allocated stereo **Tempo-Synced Ping-Pong Delay** (`3/16` dotted eighth) and **4-comb / 2-allpass Schroeder/Moorer Hall Reverb** send buses.
- **Anti-Clipping Summing & Hardware Gain Staging:**
  - `-8.4 dB` (`0.38`) polyphonic summing headroom across all 8 tracks + FX returns.
  - C1-continuous soft-knee master limiter in `quantize_to_i16`.
  - Locks Allwinner A64 digital `DAC` and `AIF1 DA0` mixer stages at `160` (`0 dB` unity gain) while mapping the physical PinePhone **Volume Up / Volume Down** buttons (`sun4i-lradc-keys`) to both the digital Master Volume (`00`–`FF`) and the analog `Headphone` / `Line Out` amplifier stages (with a live on-screen **Master Volume OSD**).

### 6. Dual-Core Real-Time Architecture & Dual-DAC Hot-Plug Fallback
- **Thread 1 (Cores 0–1, 60 FPS UI & Input):** Renders the `1440×720` landscape split-screen UI:
  - **Left 60% (`864×720`):** Integer-scaled M8 tracker views (`Song`, `Chain`, `Phrase`, `Instrument`, `Table`, `Groove`), modal Sample/Project Browser (`F7`), and 3-page Quick User Guide (`F8`).
  - **Right 40% (`576×720`):** Live Context HUD featuring Dual I2C Battery Gauges (`AXP803` Phone + `IP5209` Keyboard Case), DAC/MIDI status badges, interactive Master Volume bar (`[-]` / `[+]`), Song Chain Mini-Map, 8-Track VU meters with touch `[M]` Mute / `[S]` Solo buttons, and named MIDI CC macros.
- **Thread 2 (Isolated Cores 2–3, `SCHED_FIFO` Priority 95):** Runs the 48 kHz / 128-sample (`2.66 ms` hard deadline) audio DSP mixer and 5-pin DIN MIDI clock. Communicates with Thread 1 exclusively via lock-free `crossbeam-channel` SPSC queues with **zero heap allocations or deallocations** on the audio thread.
- **Automatic Dual-DAC Fallback:** Prefers an external USB-C DAC (`hw:1,0`) on PinePhone and automatically catches `-ENODEV` / `-EPIPE` on USB hub unplug to fall back to the internal Allwinner A64 3.5mm/Speaker codec (`hw:0,0`) without dropping a single sequencer tick.

### 7. PinePhone `KB151` Anti-Ghosting & Dual Input Mode
- Designed around the passive key matrix of the PinePhone Keyboard Case (`KB151` @ I2C `0x15`):
  - **Hardware Modifier Isolation:** Binds tracker modifiers (`Shift`, `Option`, `Edit`, `Play`) to dedicated hardware lines outside the passive alpha matrix (`Left Shift`, `Ctrl`/`Pine`, `Alt`/`AltGr`, `Space`).
  - **Sticky Latch Mode:** Quick-tapping (`<200ms`) `Shift`, `Option`, or `Edit` latches the modifier (`!*`) for single-finger chord entry on the handheld keyboard.
  - **Dual Input Mode (`Fn/Pine + Space` or `Tab`):** Switch instantly between **M8 Thumb Hex Mode** (`0–9`, `A–F` nibble entry + `Option + Arrows/HJKL` value editing) and **2-Octave Chromatic QWERTY Piano Mode** (`Z–M` lower octave, `Q–P` upper octave) for live step recording.

---

## 🎮 Controls & Shortcuts Reference

| Action | PinePhone `KB151` / Keyboard | Touchscreen (`1440×720`) |
| :--- | :--- | :--- |
| **Navigate Views** | `Shift + Up/Down/Left/Right` or `F1`–`F6` | Tap `[F1:SONG]` .. `[F6:GRV]` top tabs |
| **Move Cursor** | `Arrow Keys` | Tap any cell in the active grid |
| **Edit Cell Value (`±1` / `±16`)** | `Option (Ctrl/Pine) + Left/Right/Up/Down` (or `H/J/K/L`) | Tap `[-16]` `[-1]` `[+1]` `[+16]` bottom bar |
| **Hex Entry (`M8 THUMB` mode)** | `0`–`9`, `A`–`F` | Use `[-16]` `[-1]` `[+1]` `[+16]` step bar |
| **Toggle `M8 THUMB` / `PIANO` Mode** | `Pine/Fn + Space`, `Ctrl + Space`, `Tab`, or `F12` | Tap `[MODE: M8 THUMB]` / `[MODE: PIANO]` badge |
| **Play / Stop Sequencer** | `Space` (`Shift + Space` plays from row `00`) | Tap `[PLAY]` / `[STOP]` badge |
| **Master Volume Up / Down** | Physical **Volume Up / Down** side buttons, or `Option + =` / `Option + -` | Tap `[-]` / `[+]` or anywhere on the HUD `VOL` bar |
| **Mute / Solo Track (`1`–`8`)** | `Option + M` (Solo active track) | Tap `[M]` (Mute) or `[S]` (Solo) on HUD Track `T1`–`T8` |
| **Clone / Copy / Paste** | `Option + D` (`Insert`), `Option + C`, `Option + V` | Tap `[CLN]`, `[CPY]`, `[PST]` on bottom bar |
| **Clear / Delete Step** | `Option + Alt (Edit)`, `Backspace`, or `Delete` | Tap `[DEL]` on bottom bar |
| **Sample / Project Browser** | `F7` (`Left/Right` tabs, `Up/Down` select, `Enter` load) | Tap `[F7:BROWSE]` badge |
| **Quick User Guide (`3 Pages`)** | `F8`, `?` (`/`), or `H` | Tap `[F8:HELP]` badge |
| **Load & Play Demo Song** | `F9` (`cyber_pine_demo.toml`) | Tap `[DEMO]` on bottom bar |
| **Quick Save / Quick Load** | `F10` (Save `user_song.toml`) / `F11` (Load) | Tap `[SAVE]` on bottom bar |
| **Instant Clean Power-Off** | Physical **Power Button** or `Option + Shift + Q` | Tap red `[OFF]` badge in top-right HUD |

---

## 📖 How to Use `pineTracker` (Step-by-Step User Guide)

### 1. Understanding the M8 Tracker Hierarchy
Like Dirtywave M8 and LittleGPTracker (`LSDJ`), `pineTracker` organizes music top-down from **Songs** to **Chains** to **Phrases**:

```text
[SONG VIEW (F1)]  256 Rows × 8 Tracks (T1..T8). Each cell holds a CHAIN ID (00..FE).
       │
       ▼  (Press Shift + Right or F2)
[CHAIN VIEW (F2)] 16 Steps. Each step sequences a PHRASE ID (00..FE) + Transpose (TRN).
       │
       ▼  (Press Shift + Right or F3)
[PHRASE VIEW (F3)] 16 Steps of actual notes: NOTE | VEL | INST | FX1 | FX2 | FX3
       │
       ├──► [INSTRUMENT VIEW (F4)] Edit the sound of INST (00..7F) + Delay/Reverb Sends
       ├──► [TABLE VIEW (F5)]      16-step per-note modulation & arpeggiator macro table
       └──► [GROOVE VIEW (F6)]     16-step swing timing table (24-PPQN ticks per step)
```

---

### 2. First 30 Seconds: Playing & Exploring the Starter Song
1. **Start Playback:** When `pineTracker` finishes its 2-second boot splash, it automatically loads the 8-track demo song (`CYBER_PINE_DEMO` @ 126 BPM). Press **`Space`** on the keyboard or tap the green **`[PLAY]`** button in the bottom-left toolbar to start or stop the sequencer at any time.
   - Hold **`Shift + Space`** to always restart playback from Song Row `00`.
   - Press **`F9`** or tap **`[DEMO]`** to reload and restart the factory 8-track demo song from scratch.
2. **Adjust Volume:**
   - Press the physical **Volume Up / Volume Down** buttons on the side of the PinePhone (or **`Option + =`** / **`Option + -`** on the keyboard) to bring up the 20-segment **Master Volume OSD** and adjust both the digital mix and the Allwinner A64 analog amplifier.
   - On the touchscreen, you can also tap **`[-]`**, **`[+]`**, or anywhere along the **`VOL`** bar in the top-right Context HUD.
3. **Mute & Solo Tracks Live:**
   - Look at the **8-Track Live Peak Meters (`T1`–`T8`)** in the middle of the Right Context HUD.
   - Tap **`[M]`** on any track to **Mute** it, or tap **`[S]`** (or press **`Option + M`**) to **Solo** a track while performing.

---

### 3. Navigating the 6 Views (`SONG` ↔ `CHAIN` ↔ `PHRASE` ↔ `INST` ↔ `TABLE` ↔ `GROOVE`)
You can move between views using either **M8 Compass Navigation**, **Function Keys**, or **Touch**:
- **M8 Compass Navigation (`Shift + Arrow Keys`):**
  - From **`SONG`**, place the cursor on a Chain ID (e.g., `00` on Track 1) and press **`Shift + Right`** to drill down into **`CHAIN 00`**.
  - From **`CHAIN`**, place the cursor on a Phrase ID and press **`Shift + Right`** to drill down into **`PHRASE 00`**.
  - From **`PHRASE`**, press **`Shift + Right`** to open the **`INSTRUMENT`** used on that step, or press **`Shift + Down`** to open the linked **`TABLE`**, and **`Shift + Down`** again for **`GROOVE`**.
  - Press **`Shift + Left`** (or **`Shift + Up`**) to climb back up toward **`SONG`**.
- **Touch Tabs / Function Keys:** Tap **`[SONG]`**, **`[CHN]`**, **`[PHR]`**, **`[INST]`**, **`[TBL]`**, or **`[GRV]`** in the top bar (or press **`F1`–`F6`**). Context automatically follows the Chain/Phrase/Instrument under your cursor!

---

### 4. Entering & Editing Notes: Dual Input Mode (`THUMB HEX` vs. `PIANO KEY`)
Look at the orange/magenta badge in the top bar (**`THUMB HEX`** vs. **`PIANO KEY`**). You can toggle between these two modes at any time by pressing **`Pine/Fn + Space`**, **`Ctrl + Space`**, **`Tab`**, **`F12`**, or tapping the **`[THUMB HEX]` / `[PIANO KEY]`** badge:

#### Mode 1: `THUMB HEX` Mode (M8-Style Hexadecimal & Value Stepping)
- **Step Values by `±1` or `±16`:**
  - Hold **`Option`** (`Ctrl` or `Pine` key) + **`Left` / `Right`** (or `H` / `L`) to decrement/increment any cell under the cursor by **`±1`** (or `±1 semitone` on a Note).
  - Hold **`Option`** + **`Down` / `Up`** (or `J` / `K`) to decrement/increment by **`±16` (`±0x10`)** (or `±1 octave` on a Note).
  - **On Touchscreen:** Tap any cell in the grid to move the cursor there, then tap **`[-16]`**, **`[-1]`**, **`[+1]`**, or **`[+16]`** on the bottom toolbar!
  - If you do this on an empty (`--`) cell, `pineTracker` automatically inserts a sensible default value (e.g., `C-4` note, velocity `64`, instrument `00`) and previews the sound!
- **Direct Hex Nibble Entry (`0–9`, `A–F`):**
  - Type any hex key (`0`–`9`, `A`–`F`) to overwrite the lower nibble of the current cell (great for typing exact hex values like `40`, `7F`, `0C`).
- **Clear / Delete a Step:**
  - Press **`Backspace`**, **`Delete`**, **`Option + Edit (Alt)`**, or tap **`[CLR]`** on the bottom toolbar to reset the selected cell back to `--` (`EMPTY`).

#### Mode 2: `PIANO KEY` Mode (2-Octave Live Chromatic Keyboard)
- Turns the `KB151` / QWERTY keyboard into a 2-octave musical keyboard with instant audio preview and step-recording in `PHRASE` view:
  - **Lower Octave (`C` to `B`):** `Z` (C), `S` (C#), `X` (D), `D` (D#), `C` (E), `V` (F), `G` (F#), `B` (G), `H` (G#), `N` (A), `J` (A#), `M` (B).
  - **Upper Octave (`C+1` to `E+2`):** `Q` (C), `2` (C#), `W` (D), `3` (D#), `E` (E), `R` (F), `5` (F#), `T` (G), `6` (G#), `Y` (A), `7` (A#), `U` (B), `I` (C+2), `9` (C#+2), `O` (D+2), `0` (D#+2), `P` (E+2).
  - **Change Base Octave (`1`–`7`):** Press **`-`** or **`=`** to shift the piano keyboard down or up an octave.

#### Sticky Modifier Latch (`SHF*`, `OPT*`, `EDT*`) for One-Handed Thumb Entry
- Because the PinePhone keyboard is held with two thumbs, you don't have to hold down modifiers while reaching for another key:
  - **Quick-tap (`<200ms`)** `Shift`, `Ctrl` (`Option`), or `Alt` (`Edit`) and release it.
  - Notice the badge in the top bar lights up amber with an asterisk (**`SHF*`**, **`OPT*`**, **`EDT*`**).
  - The next key you press will execute the modified chord and automatically unlatch! Tap `Esc` at any time to clear all latches.

---

### 5. Cloning, Copying & Pasting Patterns (`CLN`, `CPY`, `PST`)
Fast pattern building is essential in a tracker:
- **Clone (`Ctrl/Option + D`, `Insert`, or tap `[CLN]`):**
  - In **`SONG`** view: Finds the first unused Chain ID, copies the current Chain's contents into it, and assigns the new Chain ID to the cursor cell!
  - In **`CHAIN`** view: Finds the first unused Phrase ID, copies the current Phrase's 16 steps into it, and assigns the new Phrase ID to the cursor row!
- **Copy (`Ctrl/Option + C` or tap `[CPY]`) & Paste (`Ctrl/Option + V` or tap `[PST]`):**
  - Copies and pastes the current cell/row in `SONG`, `CHAIN`, `PHRASE`, `INSTRUMENT`, `TABLE`, or `GROOVE` view.

---

### 6. Designing Sounds with the 4 Instrument Engines (`F4` / `[INST]`)
Switch to **`INSTRUMENT` (`F4` or `[INST]`)** to edit any of the 128 instruments (`00`–`7F`). Use **`[ID-]`** / **`[ID+]`** on the bottom toolbar to flip between Instrument numbers:
- **Row 0 (`ENGINE TYPE`):** Step left/right (`Option + Left/Right` or `[-1]`/`[+1]`) to switch the instrument between:
  1. **`DRUMKIT (12-PAD × 3-VEL)`:**
     - Trigger pads `0`–`11` using notes **`C-4` through `B-4`** (`C-4` = Kick, `C#4` = Snare, `D-4` = Rim/Clap, `D#4` = Closed Hat, `E-4` = Pedal Hat, `F-4` = Open Hat, `F#4`..`G#4` = Toms, `A-4` = Crash, `A#4` = Ride, `B-4` = Perc).
     - Note velocity automatically selects the **Soft (`01–3F`)**, **Medium (`40–67`)**, or **Hard (`68–7F`)** stereo `.wav` sample layer!
     - `Closed Hat` (`D#4`) and `Pedal Hat` (`E-4`) automatically choke `Open Hat` (`F-4`) with a smooth 3ms fade.
  2. **`SINGLESAMPLE (WAV + SVF)`:**
     - Plays any `.wav` file chromatically across the keyboard or sliced across `1–64` slices (`Row 1`), with Loop Mode (`OneShot`, `ForwardLoop`, `PingPongLoop` on `Row 2`) and a resonant State Variable Filter (`Cutoff` on `Row 3`, `Resonance` on `Row 4`, `Filter Mode` LP/BP/HP on `Row 5`).
  3. **`INTERNALSYNTH (FM4OP / VA)`:**
     - Toggle `Row 1` between **`4-OP FM`** (8 DX algorithms on `Row 2`, Op1 Feedback on `Row 3`, Operator Ratios & Levels on `Rows 6–9`) and **`POLYBLEP VA`** (`Saw`, `Pulse`, `Saw+Sub`, `Noise+SVF` with Filter Cutoff/Resonance on `Rows 4–5`).
  4. **`MIDIOUT (5-PIN DIN + TOML)`:**
     - Sends notes and CCs out of a USB-C MIDI interface / 5-pin DIN Thru box. Configure MIDI Channel (`1–16`), Program Change, and 4 named CC macro sliders (`Rows 4–7`) mapped from `/synths/*.toml`.
- **Master FX Sends & Modulation Table (`Bottom Rows of INST View`):**
  - Every instrument has dedicated **Delay Send (`00`–`FF`)**, **Reverb Send (`00`–`FF`)**, and an automatic **Table Link (`00`–`1F` or `--`)** that triggers a modulation Table every time a note plays on that instrument.

---

### 7. Loading `.wav` Samples, `.toml` Projects & Synth Profiles (`F7` / `[BRW]`)
Press **`F7`** or tap **`[BRW]`** in the top/bottom bar to open the **In-App File Browser Modal**:
1. Switch between the 3 tabs using **`Left` / `Right`** (or by tapping the tab headers):
   - **`SAMPLES (.WAV)`:** Browse `.wav` files in `data/samples/`. Press **`Enter`** (or tap the selected file again) to hot-swap it into the current `SingleSample` or `DrumKit` pad without stopping the sequencer!
   - **`PROJECTS (.TOML)`:** Load any saved song `.toml` from `data/projects/` (or `/projects` on PinePhone).
   - **`SYNTH PROFILES`:** Load a hardware MIDI CC mapping profile (`roland_j6.toml`, `behringer_pro_vs_mini.toml`, `elektron_model_cycles.toml`) into the current `MidiOut` instrument.
2. Press **`F10`** (or tap **`[SAV]`**) at any time to quick-save your work to `user_song.toml`, and **`F11`** (or tap **`[LOD]`**) to reload it.

---

### 8. Using Tracker FX Commands (`FX1`–`FX3`), Tables (`F5`), and Grooves (`F6`)
Each step in a `PHRASE` or `TABLE` has **3 FX slots** (`COMMAND` + `HEX VALUE`):
- Move the cursor to an `FX` command column and use `Option + Left/Right` (or tap `[-1]`/`[+1]`) to cycle through the 12 real-time FX commands:
  - **`ARP xy`:** Arpeggio cycling between `0`, `+x`, and `+y` semitones at 60 Hz sub-ticks.
  - **`PIT xx`:** Smooth pitch slide up (`01`–`7F`) or down (`80`–`FF`, two's complement signed byte).
  - **`VOL xx`:** Smooth volume slide up or down per sub-tick.
  - **`CUT xx`:** Cut (gate off) the note after `xx` ticks.
  - **`RET xy`:** Retrigger the note every `y` ticks with volume change `x`.
  - **`DEL xx`:** Delay triggering the note by `xx` ticks behind the beat (micro-timing).
  - **`FIN xx`:** Fine pitch detune.
  - **`FMP xx`:** Modulate 4-Op FM feedback / modulation index live.
  - **`KIL xx`:** Hard-kill the voice after `xx` ticks.
  - **`GRV xx`:** Switch the track's swing Groove table to Groove `xx`.
  - **`TBL xx`:** Start running Modulation Table `xx` on this track.
  - **`TBX xx`:** Set the tick speed multiplier of the active Modulation Table.

---

## 🛠️ Installation & Build Guide

### Option A: Run Natively on Linux Desktop / Steam Deck (`x86_64`)

#### 1. Prerequisites (Arch Linux / SteamOS Distrobox)
```bash
sudo pacman -S --needed base-devel rust cargo sdl2 alsa-lib libpulse
```

#### 2. Build & Run (`1440×720` Windowed Mode)
```bash
git clone https://github.com/Frankmau5/pineTracker.git
cd pineTracker

# Run all 21 real-time DSP, sequencer, and UI unit tests
cargo test

# Launch pineTracker in Steam Deck / Desktop mode (SDL2 + PipeWire/ALSA)
cargo run --release --features deck-dev
```
> **Tip:** Press **`F9`** (or click **`[DEMO]`** in the bottom bar) as soon as the window opens to load and play the 8-track starter song (`CYBER_PINE_DEMO`).

---

### Option B: Build & Flash the Instant-Boot PinePhone microSD Card (`aarch64`)

`pineTracker` includes automated scripts to cross-compile the `aarch64` Cortex-A53 (`+neon`) binary and build a bootable **Direct-to-Tracker (`PID 1`) Appliance OS** microSD card based on DanctNIX Arch Linux ARM (`linux-megi` kernel).

#### 1. Host Prerequisites (on your x86_64 Linux / Steam Deck build machine)
```bash
sudo pacman -S --needed aarch64-linux-gnu-gcc uboot-tools e2fsprogs fuse2fs parted xz zstd wget rsync
rustup target add aarch64-unknown-linux-gnu
```

#### 2. Cross-Compile `pine_tracker` for PinePhone (`aarch64`)
On the PinePhone (`aarch64`), `pine_tracker` uses a zero-SDL2 native **Linux Framebuffer (`/dev/fb0`) + `evdev` (`/dev/input/event*`)** backend so its only runtime shared library dependencies are `libasound.so.2`, `libc.so.6`, `libm.so.6`, and `libgcc_s.so.1`.

```bash
./scripts/build_pinephone.sh
```
This produces:
- `target/aarch64-unknown-linux-gnu/release/pine_tracker`
- A complete deployment bundle in `dist/pinephone-bundle/` (including all 37 starter `.wav` samples, `.toml` synth profiles, `cyber_pine_demo.toml`, and `PID 1` boot scripts).

#### 3. Build & Flash a New PinePhone microSD Card
1. Download the DanctNIX Arch Linux ARM PinePhone Barebone image (`archlinux-pinephone-barebone-*.img.xz`) into `~/Downloads/`.
2. Build the pre-configured `pineTracker` SD card image:
   ```bash
   ./scripts/make_pinephone_sdcard_img.sh ~/Downloads/archlinux-pinephone-barebone-*.img.xz
   ```
3. Flash `dist/pinetracker-pinephone-sd.img` to your PinePhone microSD card (replace `/dev/sdX` with your USB SD card reader — **verify with `lsblk` first!**):
   ```bash
   sudo dd if=dist/pinetracker-pinephone-sd.img of=/dev/sdX bs=4M status=progress conv=fsync
   ```
4. Expand the root partition (`/dev/sdX2`) to use the full capacity of your microSD card for `.wav` samples and projects:
   ```bash
   sudo parted -s /dev/sdX resizepart 2 100%
   sudo e2fsck -f -p /dev/sdX2
   sudo resize2fs /dev/sdX2
   ```

#### 4. Fast 2-Second SD Card Updates (During Development)
Once your microSD card is flashed, you never need to re-flash the whole image. Whenever you modify the Rust code or add new `.wav` samples / `.toml` synth profiles:
```bash
# 1. Cross-compile the updated aarch64 binary
./scripts/build_pinephone.sh

# 2. Plug the PinePhone microSD card into your reader (/dev/sda) and run:
./scripts/update_sdcard.sh
```
`./scripts/update_sdcard.sh` copies the updated `pine_tracker` binary, demo project, and `PID 1` init scripts directly onto `/dev/sda2` and safely unmounts the card in ~2 seconds.

---

## 📂 Repository Structure

```text
pineTracker/
├── Cargo.toml                  # Crate manifest (x86_64 SDL2 dev mode + aarch64 native fb0/evdev)
├── GEMINI.md                   # v2.0 Hardware & Real-Time Architecture Specification
├── data/
│   ├── projects/               # Saved songs (cyber_pine_demo.toml, user_song.toml)
│   ├── samples/                # 12-pad multi-velocity DrumKit & SingleSample .wav bank
│   └── synths/                 # Hardware MIDI synth CC profile TOMLs (Roland J-6, Pro VS Mini, etc.)
├── deploy/
│   ├── stage1-arch/            # PID 1 appliance init (pinetracker-init.sh), RT tuning, U-Boot boot.txt
│   └── stage2-buildroot/       # Buildroot SquashFS + PREEMPT_RT production configs
├── scripts/
│   ├── build_pinephone.sh      # Cross-compiles aarch64 Cortex-A53 + NEON binary & bundle
│   ├── make_pinephone_sdcard_img.sh # Injects pineTracker into DanctNIX Arch ARM image
│   └── update_sdcard.sh        # Fast 2-second incremental updater for /dev/sda2
└── src/
    ├── main.rs                 # Entry point, CLI flags, and 21 unit/integration tests
    ├── model.rs                # Song, Chain, Phrase, Instrument, Table, Groove & TOML serialization
    ├── ipc.rs                  # Lock-free crossbeam SPSC queues & zero-dealloc sample retirement
    ├── input.rs                # KB151 anti-ghosting, Sticky Latch, Dual Input Mode & touch handler
    ├── assets.rs               # Procedural 16-bit .wav generator, Hound loader & demo song builder
    ├── hw.rs                   # Dual I2C battery sysfs polling (AXP803 + IP5209)
    ├── ui/
    │   ├── mod.rs              # 1440x720 split-screen renderer, boot splash, Volume OSD, SDL2 & /dev/fb0
    │   └── font.rs             # Embedded 8x8 bitmap pixel font & 32x32 pixel-art Pine Tree logo
    └── audio/
        ├── mod.rs              # Thread 2 SCHED_FIFO loop, Dual-DAC ALSA/PipeWire driver & soft limiter
        ├── sequencer.rs        # 192-PPQN internal / 24-PPQN MIDI clock, Groove swing & Table engine
        ├── midi.rs             # ALSA RawMidi 5-pin DIN output & TOML CC profile mapper
        └── dsp/
            ├── mod.rs          # Per-track voice router & peak metering
            ├── drumkit.rs      # 12-pad × 3-velocity DrumKit, 8-voice pool, 1.5ms crossfade & 3ms choke
            ├── sample.rs       # SingleSample pitch/slice player & Cytomic SVF filter
            ├── synth.rs        # 8-algorithm 4-Op FM & PolyBLEP Virtual Analog synth
            └── fx.rs           # Zero-allocation Master Stereo Ping-Pong Delay & Hall Reverb bus
```

---

## 📜 License

MIT License. Built with Rust for the Pine64 community.

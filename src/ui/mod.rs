pub mod font;

#[cfg(not(target_arch = "x86_64"))]
use crate::input::{evdev_code_to_keycode, EVDEV_KEY_POWER};
use crate::input::{BrowserTab, InputMode, Keycode, M8View, TrackerUiState};
use crate::ipc::{AudioTelemetry, AudioToUiMsg, UiIpcHandle};
use crate::model::{
    format_note, InstrumentEnum, ProjectState, SynthMode, CHAIN_STEPS, DRUM_PADS, EMPTY_U8,
    GROOVE_STEPS, MIDI_CC_SLOTS, NUM_TRACKS, PHRASE_STEPS, SONG_ROWS, TABLE_STEPS,
};
use crate::ui::font::{
    FrameBuffer, RgbColor, COL_ACCENT_AMBER, COL_ACCENT_CYAN, COL_ACCENT_GREEN,
    COL_ACCENT_MAGENTA, COL_ACCENT_RED, COL_BG_DARK, COL_BG_HEADER, COL_BG_PANEL, COL_BG_ROW_ALT,
    COL_BORDER, COL_CURSOR_BG, COL_CURSOR_FG, COL_PLAYHEAD_BG, COL_TEXT_DIM, COL_TEXT_MAIN,
    LEFT_VIEW_WIDTH, RIGHT_HUD_WIDTH, RIGHT_HUD_X, SCREEN_HEIGHT, SCREEN_WIDTH,
};
#[cfg(target_arch = "x86_64")]
use sdl2::pixels::PixelFormatEnum;
#[cfg(target_arch = "x86_64")]
use sdl2::rect::Rect;
use std::fs;
#[cfg(target_arch = "x86_64")]
use std::mem::MaybeUninit;
use std::time::{Duration, Instant};

#[cfg(target_arch = "x86_64")]
const RAW_SDL_QUIT: u32 = 0x100;
#[cfg(target_arch = "x86_64")]
const RAW_SDL_KEYDOWN: u32 = 0x300;
#[cfg(target_arch = "x86_64")]
const RAW_SDL_KEYUP: u32 = 0x301;
#[cfg(target_arch = "x86_64")]
const RAW_SDL_MOUSEBUTTONDOWN: u32 = 0x401;
#[cfg(target_arch = "x86_64")]
const RAW_SDL_FINGERDOWN: u32 = 0x700;

#[cfg(target_arch = "x86_64")]
#[derive(Copy, Clone, Debug)]
enum SafeSdlEvent {
    Quit,
    KeyDown { keycode: Keycode, repeat: bool },
    KeyUp { keycode: Keycode },
    MouseDown { x: i32, y: i32 },
    FingerDown { norm_x: f32, norm_y: f32 },
    Other,
}

/// Polls `sdl2::sys::SDL_PollEvent` directly without transmuting `u32` into `SDL_EventType`.
/// This prevents `sdl2 0.37`'s `EventType::try_from` from panicking on `sdl2_compat` / SDL3
/// window event IDs such as `0x207` under Rust 1.99 enum validity checks.
#[cfg(target_arch = "x86_64")]
fn poll_sdl_event_safe() -> Option<SafeSdlEvent> {
    unsafe {
        let mut raw = MaybeUninit::<sdl2::sys::SDL_Event>::uninit();
        if sdl2::sys::SDL_PollEvent(raw.as_mut_ptr()) == 0 {
            return None;
        }
        let ev = raw.assume_init();
        match ev.type_ {
            RAW_SDL_QUIT => Some(SafeSdlEvent::Quit),
            RAW_SDL_KEYDOWN => {
                let sym = ev.key.keysym.sym;
                let repeat = ev.key.repeat != 0;
                if let Some(keycode) = Keycode::from_i32(sym) {
                    Some(SafeSdlEvent::KeyDown { keycode, repeat })
                } else {
                    Some(SafeSdlEvent::Other)
                }
            }
            RAW_SDL_KEYUP => {
                let sym = ev.key.keysym.sym;
                if let Some(keycode) = Keycode::from_i32(sym) {
                    Some(SafeSdlEvent::KeyUp { keycode })
                } else {
                    Some(SafeSdlEvent::Other)
                }
            }
            RAW_SDL_MOUSEBUTTONDOWN => Some(SafeSdlEvent::MouseDown {
                x: ev.button.x,
                y: ev.button.y,
            }),
            RAW_SDL_FINGERDOWN => Some(SafeSdlEvent::FingerDown {
                norm_x: ev.tfinger.x,
                norm_y: ev.tfinger.y,
            }),
            _ => Some(SafeSdlEvent::Other),
        }
    }
}

/// Maps window pixel coordinates `(wx, wy)` inside `(win_w, win_h)` to `1440x720` logical space,
/// accounting for 90° clockwise KMSDRM rotation when running on a portrait panel (`win_h > win_w`).
#[inline]
pub fn map_window_coords_to_logical(wx: i32, wy: i32, win_w: u32, win_h: u32) -> (usize, usize) {
    let w = win_w.max(1) as usize;
    let h = win_h.max(1) as usize;
    let cx = wx.max(0) as usize;
    let cy = wy.max(0) as usize;

    if h > w {
        // Portrait KMSDRM (e.g. 720x1440 rotated 90° clockwise to 1440x720)
        let lx = ((cy * SCREEN_WIDTH) / h).min(SCREEN_WIDTH - 1);
        let ly = (((w.saturating_sub(cx)) * SCREEN_HEIGHT) / w).min(SCREEN_HEIGHT - 1);
        (lx, ly)
    } else {
        let lx = ((cx * SCREEN_WIDTH) / w).min(SCREEN_WIDTH - 1);
        let ly = ((cy * SCREEN_HEIGHT) / h).min(SCREEN_HEIGHT - 1);
        (lx, ly)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct BatteryHudState {
    pub phone_pct: u8,
    pub phone_charging: bool,
    pub case_pct: u8,
    pub case_charging: bool,
}

impl Default for BatteryHudState {
    fn default() -> Self {
        Self {
            phone_pct: 86,
            phone_charging: false,
            case_pct: 92,
            case_charging: true,
        }
    }
}

impl BatteryHudState {
    /// Non-blocking sysfs poll for PinePhone (`axp20x-battery`), Keyboard Case (`ip5xxx-battery` / `ip5xxx-charger`),
    /// and Steam Deck / laptop fallback (`BAT1` / `BAT0`).
    pub fn poll_sysfs(&mut self) {
        for candidate in [
            "/sys/class/power_supply/axp20x-battery",
            "/sys/class/power_supply/BAT1",
            "/sys/class/power_supply/BAT0",
        ] {
            if let Some((pct, mut chg)) = Self::read_supply(candidate) {
                if !chg {
                    if let Ok(usb_online) =
                        fs::read_to_string("/sys/class/power_supply/axp20x-usb/online")
                    {
                        if usb_online.trim() == "1" {
                            chg = true;
                        }
                    }
                }
                self.phone_pct = pct;
                self.phone_charging = chg;
                break;
            }
        }
        for case_cand in [
            "/sys/class/power_supply/ip5xxx-battery",
            "/sys/class/power_supply/ip5xxx-charger",
        ] {
            if let Some((pct, chg)) = Self::read_supply(case_cand) {
                self.case_pct = pct;
                self.case_charging = chg;
                break;
            }
        }
    }

    fn read_supply(base: &str) -> Option<(u8, bool)> {
        let pct = if let Ok(cap_str) = fs::read_to_string(format!("{base}/capacity")) {
            cap_str.trim().parse::<u8>().ok()?.min(100)
        } else if let Ok(v_str) = fs::read_to_string(format!("{base}/voltage_now")) {
            // Fallback for IP5209 voltage_now in microvolts (3.20V = 0% .. 4.15V = 100%)
            let uv = v_str.trim().parse::<i64>().ok()?;
            (((uv - 3_200_000).clamp(0, 950_000) * 100) / 950_000) as u8
        } else {
            return None;
        };
        let status = fs::read_to_string(format!("{base}/status")).unwrap_or_default();
        let st = status.trim();
        let charging = st.eq_ignore_ascii_case("Charging") || st.eq_ignore_ascii_case("Full");
        Some((pct, charging))
    }
}

/// Draws a graphical Battery Indicator Icon (with outer shell, positive terminal cap,
/// 5 segmented charge bars colored Green/Amber/Red by level, and charging bolt indicator).
fn draw_battery_icon(
    fb: &mut FrameBuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    pct: u8,
    charging: bool,
) {
    let fill_col = if pct <= 20 {
        COL_ACCENT_RED
    } else if pct <= 45 {
        COL_ACCENT_AMBER
    } else {
        COL_ACCENT_GREEN
    };
    let border_col = if charging { COL_ACCENT_CYAN } else { COL_TEXT_MAIN };

    // Outer battery body + right-hand positive terminal nub
    let body_w = w.saturating_sub(5);
    fb.fill_rect(x, y, body_w, h, COL_BG_DARK);
    fb.draw_rect_outline(x, y, body_w, h, 2, border_col);
    let nub_h = (h / 2).max(6);
    let nub_y = y + (h.saturating_sub(nub_h)) / 2;
    fb.fill_rect(x + body_w, nub_y, 5, nub_h, border_col);

    // 5 internal segmented battery cells
    let segs = 5usize;
    let inner_w = body_w.saturating_sub(8);
    let inner_h = h.saturating_sub(8);
    let seg_gap = 2usize;
    let seg_w = (inner_w.saturating_sub(seg_gap * (segs - 1))) / segs;
    let active_segs = if pct == 0 {
        0
    } else {
        (((pct as usize) * segs + 19) / 100).clamp(1, segs)
    };

    for s in 0..segs {
        let sx = x + 4 + s * (seg_w + seg_gap);
        let col = if s < active_segs {
            fill_col
        } else {
            COL_BG_HEADER
        };
        fb.fill_rect(sx, y + 4, seg_w, inner_h, col);
    }

    if charging && body_w >= 36 && h >= 16 {
        let bx = x + (body_w.saturating_sub(16)) / 2;
        let by = y + (h.saturating_sub(10)) / 2;
        fb.fill_rect(bx, by, 16, 10, COL_BG_DARK);
        fb.draw_text(bx + 4, by + 1, "+", 1, COL_ACCENT_CYAN);
    }
}

/// 32x32 Pixel-Art Pine Tree + Synth Chip Emblem for the 2-second boot splash screen.
const PINE_LOGO_32X32: [&str; 32] = [
    "........CCCCCCCCCCCCCCCC........",
    "......CCBBBBBBBBBBBBBBBBCC......",
    "....CCBBBBBBBBBBBBBBBBBBBBCC....",
    "...CBBBBBBBBBBBLLBBBBBBBBBBBC...",
    "..CBBBBBBBBBBBLLLLBBBBBBBBBBBC..",
    "PPCBBBBBBBBBBLLGGLLBBBBBBBBBBCPP",
    "PPCBBBBBBBBBLLGGGGLLBBBBBBBBBCPP",
    "..CBBBBBBBBLLGGGGGGLLBBBBBBBBC..",
    "..CBBBBBBBLLGGGDDGGGLLBBBBBBBC..",
    "PPCBBBBBBLLDDDDDDDDDDLLBBBBBBCPP",
    "PPCBBBBBBBBBLLGGGGLLBBBBBBBBBCPP",
    "..CBBBBBBBBLLGGGGGGLLBBBBBBBBC..",
    "..CBBBBBBBLLGGGGGGGGLLBBBBBBBC..",
    "PPCBBBBBBLLGGGGDDGGGGLLBBBBBBCPP",
    "PPCBBBBBLLGGGDDDDDDGGGLLBBBBBCPP",
    "..CBBBBLLDDDDDDDDDDDDDDLLBBBBC..",
    "..CBBBBBBBLLGGGGGGGGLLBBBBBBBC..",
    "PPCBBBBBBLLGGGGGGGGGGLLBBBBBBCPP",
    "PPCBBBBBLLGGGGGGGGGGGGLLBBBBBCPP",
    "..CBBBBLLGGGGGGDDGGGGGGLLBBBBC..",
    "..CBBBLLGGGGGDDDDDDGGGGGLLBBBC..",
    "PPCBBLLGGGGDDDDDDDDDDGGGGLLBBCPP",
    "PPCBBDDDDDDDDDDDDDDDDDDDDDDBBCPP",
    "..CBBBBBBBBBBBBTTBBBBBBBBBBBBC..",
    "..CBBBBBBBBBBBTTTTBBBBBBBBBBBC..",
    "PPCBBBBBBBBBBBTTSTBBBBBBBBBBBCPP",
    "PPCBBBBBBBBBBBTTSTBBBBBBBBBBBCPP",
    "..CBBBBBBBBBBTTTTTTBBBBBBBBBBC..",
    "...CBBBBBBBBBBBBBBBBBBBBBBBBC...",
    "....CCBBBBBBBBBBBBBBBBBBBBCC....",
    "......CCBBBBBBBBBBBBBBBBCC......",
    "........CCCCCCCCCCCCCCCC........",
];

/// Renders the 2-second Boot Splash Screen featuring the Pixel-Art Pine Tree Logo,
/// `pineTracker` title, hardware summary, and animated loading bar (`progress_pct` in `0..=100`).
pub fn render_boot_splash_frame(fb: &mut FrameBuffer, progress_pct: u8) {
    fb.clear(COL_BG_DARK);

    // Subtle outer hardware bezel frame
    fb.draw_rect_outline(24, 20, SCREEN_WIDTH - 48, SCREEN_HEIGHT - 40, 2, COL_BORDER);
    fb.draw_rect_outline(
        30,
        26,
        SCREEN_WIDTH - 60,
        SCREEN_HEIGHT - 52,
        1,
        COL_ACCENT_CYAN,
    );

    // 1. Render 32x32 Pixel-Art Pine Tree + Synth Chip Emblem at 6x scale (192x192 px)
    let scale = 6usize;
    let logo_w = 32 * scale;
    let logo_x = (SCREEN_WIDTH - logo_w) / 2;
    let logo_y = 76usize;

    for (ry, row_str) in PINE_LOGO_32X32.iter().enumerate() {
        for (rx, ch) in row_str.bytes().enumerate() {
            let color = match ch {
                b'B' => Some(RgbColor(18, 26, 40)),
                b'C' => Some(COL_ACCENT_CYAN),
                b'P' => Some(COL_ACCENT_AMBER),
                b'L' => Some(RgbColor(140, 255, 200)),
                b'G' => Some(COL_ACCENT_GREEN),
                b'D' => Some(RgbColor(18, 130, 70)),
                b'T' => Some(RgbColor(190, 118, 48)),
                b'S' => Some(RgbColor(125, 72, 26)),
                _ => None,
            };
            if let Some(c) = color {
                fb.fill_rect(logo_x + rx * scale, logo_y + ry * scale, scale, scale, c);
            }
        }
    }

    // 2. Render big "PINE TRACKER" title below the logo (scale 4 = 32x32 per char)
    // "PINE" (4 chars = 128px) + "TRACKER" (7 chars = 224px) = 11 chars = 352px
    let title_x = (SCREEN_WIDTH - 11 * 32) / 2;
    let title_y = 294;
    fb.draw_text(title_x + 3, title_y + 3, "PINETRACKER", 4, COL_BG_HEADER);
    fb.draw_text(title_x, title_y, "PINE", 4, COL_ACCENT_GREEN);
    fb.draw_text(title_x + 4 * 32, title_y, "TRACKER", 4, COL_ACCENT_CYAN);

    let sub = "v2.0 // 8-TRACK HEX MUSIC TRACKER & GROOVEBOX";
    let sub_x = (SCREEN_WIDTH - sub.len() * 16) / 2;
    fb.draw_text(sub_x, 348, sub, 2, COL_ACCENT_AMBER);

    let hw = "PINE64 PINEPHONE (ALLWINNER A64 + NEON)  |  KB151 MATRIX  |  48KHZ DSP";
    let hw_x = (SCREEN_WIDTH - hw.len() * 8) / 2;
    fb.draw_text(hw_x, 384, hw, 1, COL_TEXT_DIM);

    // 3. Animated Progress Bar (x: 440..1000, w: 560, y: 440..476)
    let bar_x = 440usize;
    let bar_y = 440usize;
    let bar_w = 560usize;
    let bar_h = 32usize;
    fb.fill_rect(bar_x, bar_y, bar_w, bar_h, COL_BG_PANEL);
    fb.draw_rect_outline(bar_x, bar_y, bar_w, bar_h, 2, COL_ACCENT_CYAN);

    let pct = progress_pct.min(100) as usize;
    let fill_w = ((bar_w - 8) * pct) / 100;
    if fill_w > 0 {
        fb.fill_rect(bar_x + 4, bar_y + 4, fill_w, bar_h - 8, COL_ACCENT_GREEN);
    }

    let stage_txt = if pct < 35 {
        format!("LOADING 37 STEREO WAV SAMPLES & SYNTH PROFILES... {:02}%", pct)
    } else if pct < 75 {
        format!("INITIALIZING 48KHZ NEON DSP & A64 CODEC (0dB)... {:02}%", pct)
    } else {
        format!("READY — STARTING CYBER_PINE_DEMO SEQUENCER... {:03}%", pct)
    };
    let st_x = (SCREEN_WIDTH - stage_txt.len() * 16) / 2;
    fb.draw_text(st_x, 494, &stage_txt, 2, COL_TEXT_MAIN);

    let tip = "TIP: PRESS [SPACE] OR TAP [PLAY] TO START  |  VOL BUTTONS ADJUST VOLUME";
    let tip_x = (SCREEN_WIDTH - tip.len() * 8) / 2;
    fb.draw_text(tip_x, 640, tip, 1, COL_ACCENT_CYAN);
}

/// Renders the pop-up On-Screen Volume Overlay (OSD) when the physical PinePhone
/// Volume Up / Volume Down buttons (or HUD volume controls) are used.
fn render_volume_osd_overlay(fb: &mut FrameBuffer, master_vol: u8) {
    let bx = 430usize;
    let by = 266usize;
    let bw = 580usize;
    let bh = 164usize;

    fb.fill_rect(bx, by, bw, bh, COL_BG_DARK);
    fb.draw_rect_outline(bx, by, bw, bh, 3, COL_ACCENT_CYAN);
    fb.draw_rect_outline(bx + 5, by + 5, bw - 10, bh - 10, 1, COL_ACCENT_AMBER);

    let pct = ((master_vol as u32) * 100 + 127) / 255;
    let hdr = if master_vol == 0 {
        "MASTER VOLUME: MUTED (00% / HEX 00)".to_string()
    } else {
        format!("MASTER VOLUME: {:02}%  (HEX {:02X})", pct, master_vol)
    };
    let hdr_col = if master_vol == 0 {
        COL_ACCENT_RED
    } else {
        COL_ACCENT_CYAN
    };
    let hx = bx + (bw.saturating_sub(hdr.len() * 16)) / 2;
    fb.draw_text(hx, by + 24, &hdr, 2, hdr_col);

    // 20-segment glowing LED bar
    let segs = 20usize;
    let active_segs = ((master_vol as usize) * segs + 127) / 255;
    let bar_start_x = bx + 40;
    let seg_y = by + 68;
    for s in 0..segs {
        let sx = bar_start_x + s * 25;
        let col = if s < active_segs && master_vol > 0 {
            if s < 14 {
                COL_ACCENT_GREEN
            } else if s < 18 {
                COL_ACCENT_CYAN
            } else {
                COL_ACCENT_AMBER
            }
        } else {
            COL_BG_PANEL
        };
        fb.fill_rect(sx, seg_y, 20, 38, col);
        fb.draw_rect_outline(sx, seg_y, 20, 38, 1, COL_BORDER);
    }

    let sub = "PINEPHONE VOL+ / VOL- BUTTONS  |  0dB UNITY DAC HEADROOM";
    let sub_x = bx + (bw.saturating_sub(sub.len() * 8)) / 2;
    fb.draw_text(sub_x, by + 126, sub, 1, COL_TEXT_DIM);
}

/// Renders the complete `1440x720` Split-Screen frame:
/// - Left 60% (`864x720`): Integer-scaled M8 Views (`Song`, `Chain`, `Phrase`, `Instrument`, `Table`, `Groove`).
/// - Right 40% (`576x720`): Live Context HUD (Dual I2C Battery Gauges, Song Mini-Map, 8-Track Meters, Named MIDI CC Parameters).
pub fn render_split_screen_frame(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
    battery: &BatteryHudState,
) {
    fb.clear(COL_BG_DARK);

    // Left 60% M8 View Panel (0..864)
    render_left_m8_panel(fb, project, ui, telemetry, battery);

    // Vertical split divider at x = 864
    fb.fill_rect(LEFT_VIEW_WIDTH - 2, 0, 4, SCREEN_HEIGHT, COL_BORDER);

    // Right 40% Live Context HUD (864..1440)
    render_right_context_hud(fb, project, ui, telemetry, battery);

    // On-Screen Volume Overlay (OSD) when physical Volume Up / Volume Down buttons are pressed
    if ui.volume_osd_frames > 0 {
        render_volume_osd_overlay(fb, project.master_volume);
    }
}

fn render_left_m8_panel(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
    battery: &BatteryHudState,
) {
    // 1. Top Header Bar (y: 0..44)
    fb.fill_rect(0, 0, LEFT_VIEW_WIDTH - 2, 44, COL_BG_HEADER);
    fb.fill_rect(0, 42, LEFT_VIEW_WIDTH - 2, 2, COL_BORDER);

    let views = [
        (M8View::Song, "SONG"),
        (M8View::Chain, "CHN"),
        (M8View::Phrase, "PHR"),
        (M8View::Instrument, "INST"),
        (M8View::Table, "TBL"),
        (M8View::Groove, "GRV"),
    ];
    let mut vx = 10;
    for (v, label) in views {
        let active = !ui.show_help_guide && ui.view == v;
        let (fg, bg) = if active {
            (COL_CURSOR_FG, COL_ACCENT_CYAN)
        } else {
            (COL_TEXT_DIM, COL_BG_PANEL)
        };
        let w = fb.draw_badge(vx, 8, label, 2, fg, bg);
        vx += w + 6;
    }

    // In-Program Quick User Guide Badge (`HLP`)
    let (hlp_fg, hlp_bg) = if ui.show_help_guide {
        (COL_CURSOR_FG, COL_ACCENT_GREEN)
    } else {
        (COL_TEXT_DIM, COL_BG_PANEL)
    };
    let hlp_w = fb.draw_badge(vx, 8, "HLP", 2, hlp_fg, hlp_bg);
    vx += hlp_w + 6;

    // In-App Sample / Project Browser Badge (`BRW` / F7)
    let (brw_fg, brw_bg) = if ui.show_browser_modal {
        (COL_CURSOR_FG, COL_ACCENT_CYAN)
    } else {
        (COL_TEXT_DIM, COL_BG_PANEL)
    };
    let brw_w = fb.draw_badge(vx, 8, "BRW", 2, brw_fg, brw_bg);
    vx += brw_w + 6;

    // Mode Badge (M8 THUMB vs QWERTY PIANO)
    let (mode_lbl, mode_fg, mode_bg) = match ui.input_mode {
        InputMode::M8Thumb => ("THUMB HEX", COL_CURSOR_FG, COL_ACCENT_AMBER),
        InputMode::QwertyPiano => ("PIANO KEY", COL_TEXT_MAIN, COL_ACCENT_MAGENTA),
    };
    fb.draw_badge(vx + 2, 8, mode_lbl, 2, mode_fg, mode_bg);

    // 2. Second Status / Sticky Modifier Latch Bar (y: 44..80)
    fb.fill_rect(0, 44, LEFT_VIEW_WIDTH - 2, 36, COL_BG_PANEL);
    let hdr_info = format!(
        "PRJ:{:<14} BPM:{:03.0} T:{} OCT:{}",
        project.name.as_str(),
        project.tempo_bpm,
        ui.active_track + 1,
        ui.piano_octave
    );
    fb.draw_text(10, 47, &hdr_info, 1, COL_TEXT_MAIN);

    // Live "NOW PLAYING" Note + Hex Readout for Active Track
    let act_tel = &telemetry.tracks[ui.active_track];
    let act_note_b = format_note(act_tel.active_note);
    let act_note_s = std::str::from_utf8(&act_note_b).unwrap_or("---");
    let act_hex_s = if act_tel.active_note < 120 {
        format!("{:02X}", act_tel.active_note)
    } else {
        "--".to_string()
    };
    let act_phr_s = if act_tel.phrase_id == EMPTY_U8 {
        "--:--".to_string()
    } else {
        format!("{:02X}:{:01X}", act_tel.phrase_id, act_tel.phrase_step)
    };
    let act_ins_s = if act_tel.active_inst == EMPTY_U8 {
        "--".to_string()
    } else {
        format!("{:02X}", act_tel.active_inst)
    };
    let live_banner = format!(
        "PLAY T{}: NOTE {} [HEX {}]  PHR:{}  INS:{}",
        ui.active_track + 1,
        act_note_s,
        act_hex_s,
        act_phr_s,
        act_ins_s
    );
    fb.draw_text(
        10,
        62,
        &live_banner,
        1,
        if telemetry.playing && act_tel.active_note < 120 {
            COL_ACCENT_GREEN
        } else {
            COL_ACCENT_CYAN
        },
    );

    // Compact Graphical Battery Indicator Icon + % in Top Status Bar (x: 472..616, y: 49..73)
    draw_battery_icon(
        fb,
        472,
        50,
        52,
        22,
        battery.phone_pct,
        battery.phone_charging,
    );
    let top_bat_col = if battery.phone_pct <= 20 {
        COL_ACCENT_RED
    } else if battery.phone_pct <= 45 {
        COL_ACCENT_AMBER
    } else {
        COL_ACCENT_GREEN
    };
    let top_bat_lbl = format!(
        "{:02}%{}",
        battery.phone_pct,
        if battery.phone_charging { "+" } else { "" }
    );
    fb.draw_text(530, 53, &top_bat_lbl, 2, top_bat_col);

    // Sticky / Hold Modifier Indicators (KB151 Anti-Ghosting feedback)
    let mut mx = 626;
    for (name, st) in [
        ("SHF", &ui.mod_shift),
        ("OPT", &ui.mod_option),
        ("EDT", &ui.mod_edit),
    ] {
        let (fg, bg) = if st.latched {
            (COL_CURSOR_FG, COL_ACCENT_AMBER)
        } else if st.held {
            (COL_CURSOR_FG, COL_ACCENT_CYAN)
        } else {
            (COL_TEXT_DIM, COL_BG_DARK)
        };
        let badge_txt = if st.latched {
            format!("{name}*")
        } else {
            name.to_string()
        };
        let w = fb.draw_badge(mx, 49, &badge_txt, 2, fg, bg);
        mx += w + 6;
    }

    // 3. Main View Body (y: 84..680)
    if ui.show_browser_modal {
        render_browser_modal_view(fb, ui);
    } else if ui.show_help_guide {
        render_help_guide_view(fb, ui);
    } else {
        match ui.view {
            M8View::Song => render_song_view(fb, project, ui, telemetry),
            M8View::Chain => render_chain_view(fb, project, ui, telemetry),
            M8View::Phrase => render_phrase_view(fb, project, ui, telemetry),
            M8View::Instrument => render_instrument_view(fb, project, ui),
            M8View::Table => render_table_view(fb, project, ui),
            M8View::Groove => render_groove_view(fb, project, ui),
        }
    }

    // 4. Touch Action Toolbar & Bottom Status Toast Bar (y: 650..720)
    fb.fill_rect(0, 650, LEFT_VIEW_WIDTH - 2, 70, COL_BG_HEADER);
    fb.fill_rect(0, 650, LEFT_VIEW_WIDTH - 2, 2, COL_BORDER);

    let play_lbl = if telemetry.playing { "STOP" } else { "PLAY" };
    let play_bg = if telemetry.playing {
        COL_ACCENT_RED
    } else {
        COL_ACCENT_GREEN
    };
    let toolbar_buttons: [(usize, usize, &str, RgbColor, RgbColor); 15] = [
        (8, 66, play_lbl, COL_CURSOR_FG, play_bg),
        (78, 66, "DEMO", COL_CURSOR_FG, COL_ACCENT_AMBER),
        (148, 50, "ID-", COL_CURSOR_FG, COL_ACCENT_CYAN),
        (202, 50, "ID+", COL_CURSOR_FG, COL_ACCENT_CYAN),
        (256, 50, "-16", COL_TEXT_MAIN, COL_BG_PANEL),
        (310, 42, "-1", COL_TEXT_MAIN, COL_BG_PANEL),
        (356, 42, "+1", COL_TEXT_MAIN, COL_BG_PANEL),
        (402, 50, "+16", COL_TEXT_MAIN, COL_BG_PANEL),
        (456, 52, "CLR", COL_TEXT_MAIN, COL_ACCENT_MAGENTA),
        (512, 52, "CLN", COL_CURSOR_FG, COL_ACCENT_GREEN),
        (568, 52, "CPY", COL_CURSOR_FG, COL_ACCENT_CYAN),
        (624, 52, "PST", COL_CURSOR_FG, COL_ACCENT_AMBER),
        (680, 52, "SAV", COL_CURSOR_FG, COL_ACCENT_CYAN),
        (736, 52, "LOD", COL_CURSOR_FG, COL_ACCENT_AMBER),
        (792, 60, "BRW", COL_CURSOR_FG, COL_ACCENT_GREEN),
    ];
    for (bx, bw, lbl, fg, bg) in toolbar_buttons {
        fb.fill_rect(bx, 656, bw, 28, bg);
        fb.draw_rect_outline(bx, 656, bw, 28, 1, COL_BORDER);
        let text_w = lbl.len() * 8;
        let tx = bx + bw.saturating_sub(text_w) / 2;
        fb.draw_text(tx, 666, lbl, 1, fg);
    }

    fb.draw_text(10, 692, ui.status_toast.as_str(), 1, COL_ACCENT_GREEN);
    fb.draw_text(
        470,
        692,
        "[F7]BROWSER [CTRL+D]CLONE [CTRL+C/V]COPY/PASTE",
        1,
        COL_ACCENT_CYAN,
    );
}

fn render_browser_modal_view(fb: &mut FrameBuffer, ui: &TrackerUiState) {
    fb.fill_rect(10, 86, LEFT_VIEW_WIDTH - 22, 560, COL_BG_PANEL);
    fb.draw_rect_outline(10, 86, LEFT_VIEW_WIDTH - 22, 560, 2, COL_ACCENT_CYAN);

    let tabs = [
        (BrowserTab::SamplesWav, "SAMPLES (.WAV)", 22usize),
        (BrowserTab::ProjectsToml, "PROJECTS (.TOML)", 276usize),
        (BrowserTab::SynthProfiles, "SYNTH PROFILES", 540usize),
    ];
    for (tab, title, tx) in tabs {
        let (fg, bg) = if ui.browser_tab == tab {
            (COL_CURSOR_FG, COL_ACCENT_CYAN)
        } else {
            (COL_TEXT_DIM, COL_BG_DARK)
        };
        fb.draw_badge(tx, 94, title, 1, fg, bg);
    }
    fb.draw_badge(732, 94, "CLOSE", 1, COL_CURSOR_FG, COL_ACCENT_RED);
    fb.fill_rect(22, 122, LEFT_VIEW_WIDTH - 46, 2, COL_BORDER);

    let visible = 15usize;
    let start = ui.browser_cursor.saturating_sub(visible / 2);
    for idx in 0..visible {
        let file_idx = start + idx;
        if file_idx >= ui.browser_files.len() {
            break;
        }
        let y = 134 + idx * 32;
        let path = &ui.browser_files[file_idx];
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let is_cur = file_idx == ui.browser_cursor;
        if is_cur {
            fb.fill_rect(20, y - 4, LEFT_VIEW_WIDTH - 42, 26, COL_CURSOR_BG);
            let row_str = format!("> {:02}  {}   [TAP AGAIN OR ENTER TO LOAD]", file_idx, name);
            fb.draw_text(28, y + 2, &row_str, 1, COL_CURSOR_FG);
        } else {
            let row_str = format!("  {:02}  {}", file_idx, name);
            fb.draw_text(28, y + 2, &row_str, 1, COL_TEXT_MAIN);
        }
    }
}

fn render_help_guide_view(fb: &mut FrameBuffer, ui: &TrackerUiState) {
    fb.fill_rect(10, 86, LEFT_VIEW_WIDTH - 22, 560, COL_BG_PANEL);
    fb.draw_rect_outline(10, 86, LEFT_VIEW_WIDTH - 22, 560, 2, COL_ACCENT_GREEN);

    let page_tabs = [
        (0u8, "1:HIERARCHY & NAV", 22usize),
        (1u8, "2:EDIT, TOUCH & PIANO", 242usize),
        (2u8, "3:SYNTHS, FX & HW", 502usize),
    ];
    for (p_idx, title, tx) in page_tabs {
        let (fg, bg) = if ui.help_page == p_idx {
            (COL_CURSOR_FG, COL_ACCENT_GREEN)
        } else {
            (COL_TEXT_DIM, COL_BG_DARK)
        };
        fb.draw_badge(tx, 94, title, 1, fg, bg);
    }
    fb.draw_badge(732, 94, "CLOSE", 1, COL_CURSOR_FG, COL_ACCENT_RED);
    fb.fill_rect(22, 120, LEFT_VIEW_WIDTH - 46, 2, COL_BORDER);

    let sections: &[(&str, RgbColor, &[&str])] = match ui.help_page {
        0 => &[
            (
                "1. M8 TRACKER HIERARCHY (TOP-DOWN WORKFLOW)",
                COL_ACCENT_AMBER,
                &[
                    "SONG (F1)   : 256 rows x 8 tracks (T1..T8). Each cell holds a CHAIN ID (00..FE).",
                    "CHAIN (F2)  : 16 steps of PHRASE IDs (00..FE) + semitone TRANSPOSE (TRN).",
                    "PHRASE (F3) : 16 steps: NOTE (C-4), VEL (00..7F), INST (00..7F) & FX1..FX3.",
                    "INST (F4)   : 128 instruments (DRUMKIT, SAMPLER, VA/FM SYNTH, 5-PIN MIDI OUT).",
                    "TABLE (F5)  : 16-step per-note modulation & arpeggiator sequencer.",
                    "GROOVE (F6) : Per-track swing ticks (e.g. 07/05) while 0xF8 MIDI clock is steady.",
                    "DRILL DOWN  : Tap an already-selected cell (or SHIFT+RIGHT) to open its Chain/Phrase!",
                ],
            ),
            (
                "2. TRANSPORT, FILE BROWSER, VOLUME & INSTANT POWER-OFF",
                COL_ACCENT_CYAN,
                &[
                    "[SPACE] or [PLAY]   : Start / Stop sequencer (SHIFT+SPACE restarts from Row 00).",
                    "[F9] or [DEMO]      : Reload & immediately play the 8-track CYBER_PINE_DEMO song.",
                    "[F10/SAV] [F11/LOD] : Quick-Save to user_song.toml (F10) / Quick-Load (F11).",
                    "[F7] or [BRW]       : Open In-App Browser (.WAV Samples, .TOML Songs, Synth Profiles).",
                    "VOLUME UP / DOWN    : Side VOL+/VOL- buttons, OPT + -/=, or tap HUD VOL [-][+] bar.",
                    "TRACK MUTE / SOLO   : Tap [M] Mute or [S] Solo on HUD T1..T8 (or press OPT + M).",
                    "INSTANT POWER-OFF   : Press Phone Power button, OPT+SHIFT+Q, or tap red [OFF] badge.",
                ],
            ),
        ],
        1 => &[
            (
                "3. EDITING VALUES, TOUCH TOOLBAR & CLONE / COPY / PASTE",
                COL_ACCENT_AMBER,
                &[
                    "[ALT/EDIT + L/R]    : Step current cell value by -1 / +1 (or tap [-1] / [+1]).",
                    "[ALT/EDIT + U/D]    : Step cell by -16 / +16 or -/+1 Octave (or tap [-16] / [+16]).",
                    "TOUCH STEP EDITING  : Tap any cell to select it, then tap [-16][-1][+1][+16] below!",
                    "[ID-] / [ID+]       : Cycle active Chain / Phrase / Instrument / Table / Groove ID.",
                    "[CLN] or [OPT + D]  : Clone current Chain or Phrase into a new unused ID (>= 10)!",
                    "[CPY] / [PST]       : Copy (OPT+C) & Paste (OPT+V) cell/step across any view.",
                    "[CLR] or [BACKSPACE]: Clear cell to empty (--) or insert NOTE OFF in Phrase view.",
                ],
            ),
            (
                "4. DUAL INPUT MODE (THUMB HEX vs PIANO) & KB151 STICKY LATCH",
                COL_ACCENT_CYAN,
                &[
                    "[FN+SPC] or [TAB]   : Toggle between THUMB HEX and PIANO KEY mode (or tap badge).",
                    "THUMB HEX MODE      : Type 0-9 and A-F directly to enter hexadecimal nibbles.",
                    "PIANO KEY MODE      : Play/record 2 chromatic octaves live! Lower=Z..M, Upper=Q..P.",
                    "[-] / [=] KEYS      : Shift QWERTY Piano base octave Down / Up (Octave 1..7).",
                    "KB151 STICKY LATCH  : Quick-tap (<200ms) Shift, Ctrl(Opt), or Alt(Edit) to latch!",
                    "LATCHED BADGES      : Glow Amber as [SHF*], [OPT*], [EDT*] for 1-finger chords.",
                    "[ESCAPE]            : Clears all active sticky latches & stops note preview.",
                ],
            ),
        ],
        _ => &[
            (
                "5. THE 4 INSTRUMENT ENGINES & MASTER DELAY / REVERB SENDS",
                COL_ACCENT_AMBER,
                &[
                    "00 DRUMKIT  : 12 Pads (C-4..B-4), 3 Vel Layers, 8 Voices, 3ms Hat Choke (F-4->G#4).",
                    "01 SAMPLER  : Pitched/Sliced stereo .WAV playback + Resonant Cytomic SVF filter.",
                    "02 VA SYNTH : 2 PolyBLEP Oscillators (Saw/Pulse/Tri/Noise) + Sub-Osc + SVF.",
                    "03 FM SYNTH : 4-Operator FM with 8 DX Algorithms, Op4 Feedback & Envelopes.",
                    "04 MIDIOUT  : 5-Pin DIN MIDI out + /synths/*.toml named CC macro bars on HUD.",
                    "SEND FX BUS : Every instrument has stereo Ping-Pong DELAY & Hall REVERB sends.",
                    "DRUM PADS   : When Inst 00 (DrumKit) is selected, tap the 12 pads on the right HUD!",
                ],
            ),
            (
                "6. TRACKER FX COMMANDS REFERENCE (3 FX SLOTS PER STEP)",
                COL_ACCENT_CYAN,
                &[
                    "ARP xy : Arpeggio (+x, +y semitones)   | DEL xx : Delay note by xx 192-PPQN subticks",
                    "RET xy : Retrigger every y subticks    | CUT xx : Cut note after xx 24-PPQN ticks",
                    "KIL xx : Kill voice after xx subticks  | HOP xx : Jump to step xx in Phrase/Table",
                    "GRV xx : Select Groove 00..1F on track | TPO xx : Set Master Tempo BPM",
                    "FCU xx : SVF Filter Cutoff (00..FF)    | FRS xx : SVF Filter Resonance (00..FF)",
                    "OFS xx : Sample Slice / Start Offset   | SLI xx : Pitch Portamento / Glide Speed",
                    "CCA..F : Send Mapped MIDI CC Slot 1..6 | PGM xx : Send MIDI Program Change (00..7F)",
                ],
            ),
        ],
    };

    let mut y = 130;
    for (title, color, lines) in sections {
        fb.draw_text(22, y, title, 2, *color);
        y += 24;
        for line in *lines {
            fb.draw_text(26, y, line, 1, COL_TEXT_MAIN);
            y += 21;
        }
        y += 12;
    }

    fb.draw_text(
        22,
        626,
        "TIP: TAP TOP PAGE TABS [1..3], TAP LEFT/RIGHT HALF OF CARD, OR USE ARROW KEYS",
        1,
        COL_ACCENT_GREEN,
    );
}

fn render_song_view(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
) {
    fb.draw_text(16, 92, "ROW", 2, COL_ACCENT_AMBER);
    for trk in 0..NUM_TRACKS {
        let x = 96 + trk * 92;
        let t_hdr = format!("TRK{}", trk + 1);
        let col = if trk == ui.active_track {
            COL_ACCENT_CYAN
        } else {
            COL_ACCENT_AMBER
        };
        fb.draw_text(x, 92, &t_hdr, 2, col);
    }

    let visible_rows = 16usize;
    let start_row = ui
        .song_row
        .saturating_sub(visible_rows / 2)
        .min(SONG_ROWS - visible_rows);

    for idx in 0..visible_rows {
        let r = start_row + idx;
        let y = 126 + idx * 33;

        if idx % 2 == 1 {
            fb.fill_rect(8, y - 3, LEFT_VIEW_WIDTH - 20, 30, COL_BG_ROW_ALT);
        }

        let row_lbl = format!("{:02X}", r);
        fb.draw_text(16, y, &row_lbl, 2, COL_TEXT_DIM);

        for trk in 0..NUM_TRACKS {
            let x = 98 + trk * 92;
            let chain_id = project.song[r][trk];
            let cell_str = if chain_id == EMPTY_U8 {
                "--".to_string()
            } else {
                format!("{:02X}", chain_id)
            };

            let is_cursor = r == ui.song_row && trk == ui.song_col;
            let trk_tel = &telemetry.tracks[trk];
            let is_playhead = telemetry.playing
                && trk_tel.song_row as usize == r
                && chain_id != EMPTY_U8;

            if is_playhead {
                fb.fill_rect(x - 4, y - 3, 86, 26, COL_PLAYHEAD_BG);
                fb.draw_rect_outline(x - 4, y - 3, 86, 26, 1, COL_ACCENT_GREEN);
            }

            if is_cursor {
                fb.fill_rect(x - 4, y - 3, 38, 26, COL_CURSOR_BG);
                fb.draw_text(x, y, &cell_str, 2, COL_CURSOR_FG);
            } else if is_playhead {
                fb.draw_text(x, y, &cell_str, 2, COL_ACCENT_GREEN);
            } else {
                let col = if chain_id == EMPTY_U8 {
                    COL_TEXT_DIM
                } else {
                    COL_TEXT_MAIN
                };
                fb.draw_text(x, y, &cell_str, 2, col);
            }

            // Show live Note (C-4) + Hex byte (3C) right inside the active Song cell while playing
            if is_playhead {
                let nb = format_note(trk_tel.active_note);
                let ns = std::str::from_utf8(&nb).unwrap_or("---");
                let hx = if trk_tel.active_note < 120 {
                    format!("x{:02X}", trk_tel.active_note)
                } else {
                    "x--".to_string()
                };
                fb.draw_text(x + 38, y - 1, ns, 1, COL_ACCENT_CYAN);
                fb.draw_text(x + 38, y + 10, &hx, 1, COL_ACCENT_AMBER);
            }
        }
    }
}

fn render_chain_view(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
) {
    let trk_tel = &telemetry.tracks[ui.active_track];
    let nb = format_note(trk_tel.active_note);
    let ns = std::str::from_utf8(&nb).unwrap_or("---");
    let hx = if trk_tel.active_note < 120 {
        format!("{:02X}", trk_tel.active_note)
    } else {
        "--".to_string()
    };
    let hdr = format!(
        "CHAIN {:02X} (STEP PHRASE TRN) [NOW: {} HEX:{}]",
        ui.current_chain, ns, hx
    );
    fb.draw_text(16, 92, &hdr, 2, COL_ACCENT_AMBER);

    let chain = &project.chains[ui.current_chain as usize];
    for step in 0..CHAIN_STEPS {
        let y = 126 + step * 33;
        let is_playhead = telemetry.playing
            && trk_tel.chain_id == ui.current_chain
            && trk_tel.chain_step as usize == step;

        if is_playhead {
            fb.fill_rect(8, y - 3, LEFT_VIEW_WIDTH - 20, 30, COL_PLAYHEAD_BG);
        } else if step % 4 == 0 {
            fb.fill_rect(8, y - 3, LEFT_VIEW_WIDTH - 20, 30, COL_BG_ROW_ALT);
        }

        let step_lbl = if is_playhead {
            format!(">{:01X}", step)
        } else {
            format!(" {:01X}", step)
        };
        fb.draw_text(
            16,
            y,
            &step_lbl,
            2,
            if is_playhead {
                COL_ACCENT_GREEN
            } else {
                COL_TEXT_DIM
            },
        );

        let row = chain.rows[step];
        let phr_str = if row.phrase == EMPTY_U8 {
            "--".to_string()
        } else {
            format!("{:02X}", row.phrase)
        };
        let trn_str = format!("{:+03}", row.transpose);

        // Phrase Column (col 0)
        if step == ui.chain_row && ui.chain_col == 0 {
            fb.fill_rect(122, y - 3, 46, 26, COL_CURSOR_BG);
            fb.draw_text(128, y, &phr_str, 2, COL_CURSOR_FG);
        } else {
            fb.draw_text(128, y, &phr_str, 2, COL_TEXT_MAIN);
        }

        // Transpose Column (col 1)
        if step == ui.chain_row && ui.chain_col == 1 {
            fb.fill_rect(250, y - 3, 62, 26, COL_CURSOR_BG);
            fb.draw_text(256, y, &trn_str, 2, COL_CURSOR_FG);
        } else {
            fb.draw_text(256, y, &trn_str, 2, COL_ACCENT_CYAN);
        }

        if is_playhead {
            let live_str = format!(
                "<-- PLAYING STEP {:01X}: NOTE {} (HEX {})",
                trk_tel.phrase_step, ns, hx
            );
            fb.draw_text(350, y + 4, &live_str, 1, COL_ACCENT_GREEN);
        }
    }
}

fn render_phrase_view(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
) {
    let hdr = format!(
        "PHRASE {:02X}  NOTE(HEX) VOL INS  FX1    FX2    FX3",
        ui.current_phrase
    );
    fb.draw_text(16, 92, &hdr, 2, COL_ACCENT_AMBER);

    let phrase = &project.phrases[ui.current_phrase as usize];
    for step in 0..PHRASE_STEPS {
        let y = 126 + step * 33;
        let is_playhead = telemetry.playing
            && telemetry.tracks[ui.active_track].phrase_id == ui.current_phrase
            && telemetry.tracks[ui.active_track].phrase_step as usize == step;

        if is_playhead {
            fb.fill_rect(8, y - 3, LEFT_VIEW_WIDTH - 20, 30, COL_PLAYHEAD_BG);
        } else if step % 4 == 0 {
            fb.fill_rect(8, y - 3, LEFT_VIEW_WIDTH - 20, 30, COL_BG_ROW_ALT);
        }

        let step_lbl = if is_playhead {
            format!(">{:01X}", step)
        } else {
            format!(" {:01X}", step)
        };
        fb.draw_text(
            16,
            y,
            &step_lbl,
            2,
            if is_playhead {
                COL_ACCENT_GREEN
            } else {
                COL_TEXT_DIM
            },
        );

        let ps = phrase.steps[step];
        let note_bytes = format_note(ps.note);
        let note_str = std::str::from_utf8(&note_bytes).unwrap_or("---");
        let note_with_hex = if ps.note < 120 {
            format!("{}:{:02X}", note_str, ps.note)
        } else {
            format!("{}:--", note_str)
        };
        let vol_str = if ps.volume == EMPTY_U8 {
            "--".to_string()
        } else {
            format!("{:02X}", ps.volume)
        };
        let ins_str = if ps.instrument == EMPTY_U8 {
            "--".to_string()
        } else {
            format!("{:02X}", ps.instrument)
        };

        let cols: [(usize, String, RgbColor); 9] = [
            (76, note_with_hex, COL_ACCENT_CYAN),
            (196, vol_str, COL_TEXT_MAIN),
            (258, ins_str, COL_ACCENT_AMBER),
            (328, ps.fx[0].command.code().to_string(), COL_ACCENT_MAGENTA),
            (384, format!("{:02X}", ps.fx[0].value), COL_TEXT_MAIN),
            (464, ps.fx[1].command.code().to_string(), COL_ACCENT_MAGENTA),
            (520, format!("{:02X}", ps.fx[1].value), COL_TEXT_MAIN),
            (600, ps.fx[2].command.code().to_string(), COL_ACCENT_MAGENTA),
            (656, format!("{:02X}", ps.fx[2].value), COL_TEXT_MAIN),
        ];

        for (col_idx, (x, txt, fg)) in cols.iter().enumerate() {
            let is_cur = step == ui.phrase_row && col_idx == ui.phrase_col;
            if is_cur {
                fb.fill_rect(*x - 4, y - 3, txt.len() * 16 + 8, 26, COL_CURSOR_BG);
                fb.draw_text(*x, y, txt, 2, COL_CURSOR_FG);
            } else {
                fb.draw_text(*x, y, txt, 2, *fg);
            }
        }
    }
}

fn render_instrument_view(fb: &mut FrameBuffer, project: &ProjectState, ui: &TrackerUiState) {
    let inst = &project.instruments[ui.current_instrument as usize];
    let hdr = format!(
        "INST {:02X}: {}  [{}]",
        ui.current_instrument,
        inst.name.as_str(),
        inst.engine.kind_name()
    );
    fb.draw_text(16, 92, &hdr, 2, COL_ACCENT_AMBER);

    let tbl_str = if inst.table_id == EMPTY_U8 {
        "OFF".to_string()
    } else {
        format!("{:02X}", inst.table_id)
    };

    let mut rows: Vec<(String, String)> = vec![
        ("00 ENGINE TYPE".to_string(), inst.engine.kind_name().to_string()),
        ("01 MASTER VOL".to_string(), format!("{:02X}", inst.volume)),
        ("02 STEREO PAN".to_string(), format!("{:02X}", inst.pan)),
        ("03 TRANSPOSE".to_string(), format!("{:+03}", inst.transpose)),
        ("04 TABLE ID".to_string(), tbl_str),
        ("05 DELAY SEND".to_string(), format!("{:02X}", inst.delay_send)),
        ("06 REVERB SEND".to_string(), format!("{:02X}", inst.reverb_send)),
    ];

    match inst.engine {
        InstrumentEnum::DrumKit(ref kit) => {
            for p in 0..5 {
                let pad = kit.pads[p];
                let chk = if pad.choke_target == EMPTY_U8 {
                    "--".to_string()
                } else {
                    format!("P{:01X}", pad.choke_target)
                };
                rows.push((
                    format!("{:02} PAD {} ({})", p + 7, p, pad.name.as_str()),
                    format!("LVL:{:02X} TUN:{:+02} CHK:{}", pad.level, pad.tune_semitones, chk),
                ));
            }
        }
        InstrumentEnum::SingleSample(ref samp) => {
            rows.push(("07 SVF CUTOFF".to_string(), format!("{:02X}", samp.filter.cutoff)));
            rows.push(("08 SVF RESON".to_string(), format!("{:02X}", samp.filter.resonance)));
            rows.push(("09 SLICES".to_string(), format!("{:02}", samp.slice_count)));
            rows.push(("10 LOOP START".to_string(), format!("{:02X}", samp.loop_start)));
            rows.push(("11 LOOP END".to_string(), format!("{:02X}", samp.loop_end)));
        }
        InstrumentEnum::InternalSynth(ref syn) => {
            let mode_str = match syn.mode {
                SynthMode::VirtualAnalog => "VIRTUAL ANALOG (POLYBLEP)",
                SynthMode::Fm4Op => "4-OP FM SYNTH",
            };
            rows.push(("07 SYNTH MODE".to_string(), mode_str.to_string()));
            rows.push(("08 SVF CUTOFF".to_string(), format!("{:02X}", syn.filter.cutoff)));
            rows.push(("09 SVF RESON".to_string(), format!("{:02X}", syn.filter.resonance)));
            rows.push(("10 PULSE WIDTH".to_string(), format!("{:02X}", syn.pulse_width)));
            rows.push(("11 SUB/FM ALG".to_string(), format!("SUB:{:02X} ALG:{}", syn.sub_osc_level, syn.fm_algorithm + 1)));
        }
        InstrumentEnum::MidiOut(ref midi) => {
            rows.push(("07 MIDI CHAN".to_string(), format!("CH {:02}", midi.channel + 1)));
            for i in 0..4 {
                let slot = midi.cc_slots[i];
                rows.push((
                    format!("{:02} CC{:03} {}", i + 8, slot.cc_number, slot.name.as_str()),
                    format!("{:02X} ({:03})", slot.value, slot.value),
                ));
            }
        }
    }

    for (idx, (k, v)) in rows.iter().enumerate() {
        let y = 132 + idx * 42;
        let is_cur = idx == ui.inst_param_row;
        if is_cur {
            fb.fill_rect(14, y - 4, LEFT_VIEW_WIDTH - 32, 30, COL_CURSOR_BG);
            fb.draw_text(20, y, k, 2, COL_CURSOR_FG);
            fb.draw_text(440, y, v, 2, COL_CURSOR_FG);
        } else {
            fb.draw_text(20, y, k, 2, COL_TEXT_MAIN);
            fb.draw_text(440, y, v, 2, COL_ACCENT_CYAN);
        }
    }
}

fn render_table_view(fb: &mut FrameBuffer, project: &ProjectState, ui: &TrackerUiState) {
    let hdr = format!(
        "TABLE {:02X}    TRN   VOL   FX1     FX2     FX3",
        ui.current_table
    );
    fb.draw_text(16, 92, &hdr, 2, COL_ACCENT_AMBER);

    let table = &project.tables[ui.current_table as usize];
    for step in 0..TABLE_STEPS {
        let y = 126 + step * 33;
        let ts = table.steps[step];
        let step_lbl = format!(" {:01X}", step);
        fb.draw_text(16, y, &step_lbl, 2, COL_TEXT_DIM);

        let trn_str = format!("{:+03}", ts.transpose);
        let vol_str = if ts.volume == EMPTY_U8 {
            "--".to_string()
        } else {
            format!("{:02X}", ts.volume)
        };

        let cols: [(usize, String, RgbColor); 8] = [
            (92, trn_str, COL_ACCENT_CYAN),
            (176, vol_str, COL_TEXT_MAIN),
            (260, ts.fx[0].command.code().to_string(), COL_ACCENT_MAGENTA),
            (316, format!("{:02X}", ts.fx[0].value), COL_TEXT_MAIN),
            (396, ts.fx[1].command.code().to_string(), COL_ACCENT_MAGENTA),
            (452, format!("{:02X}", ts.fx[1].value), COL_TEXT_MAIN),
            (532, ts.fx[2].command.code().to_string(), COL_ACCENT_MAGENTA),
            (588, format!("{:02X}", ts.fx[2].value), COL_TEXT_MAIN),
        ];

        for (col_idx, (x, txt, fg)) in cols.iter().enumerate() {
            if step == ui.table_row && col_idx == ui.table_col {
                fb.fill_rect(*x - 4, y - 3, txt.len() * 16 + 8, 26, COL_CURSOR_BG);
                fb.draw_text(*x, y, txt, 2, COL_CURSOR_FG);
            } else {
                fb.draw_text(*x, y, txt, 2, *fg);
            }
        }
    }
}

fn render_groove_view(fb: &mut FrameBuffer, project: &ProjectState, ui: &TrackerUiState) {
    let hdr = format!(
        "GROOVE {:02X}  (24-PPQN TICKS PER STEP — 0xF8 STEADY)",
        ui.current_groove
    );
    fb.draw_text(16, 92, &hdr, 2, COL_ACCENT_AMBER);

    let groove = &project.grooves[ui.current_groove as usize];
    for step in 0..GROOVE_STEPS {
        let y = 126 + step * 33;
        let step_lbl = format!("STEP {:01X}", step);
        fb.draw_text(16, y, &step_lbl, 2, COL_TEXT_DIM);

        let val = groove.ticks[step];
        let val_str = if val == EMPTY_U8 {
            "--".to_string()
        } else {
            format!("{:02X}", val)
        };

        if step == ui.groove_row {
            fb.fill_rect(156, y - 3, 46, 26, COL_CURSOR_BG);
            fb.draw_text(162, y, &val_str, 2, COL_CURSOR_FG);
        } else {
            fb.draw_text(162, y, &val_str, 2, COL_ACCENT_CYAN);
        }

        if val != EMPTY_U8 {
            let bar_w = (val as usize).min(16) * 24;
            fb.fill_rect(230, y + 2, bar_w, 14, COL_ACCENT_GREEN);
        }
    }
}

fn render_right_context_hud(
    fb: &mut FrameBuffer,
    project: &ProjectState,
    ui: &TrackerUiState,
    telemetry: &AudioTelemetry,
    battery: &BatteryHudState,
) {
    let rx = RIGHT_HUD_X + 12;
    let rw = RIGHT_HUD_WIDTH - 24;

    // 1. Dual I2C Battery Gauges & DAC/MIDI Routing Panel (y: 8..134)
    fb.fill_rect(rx, 8, rw, 126, COL_BG_PANEL);
    fb.draw_rect_outline(rx, 8, rw, 126, 2, COL_BORDER);
    fb.draw_text(
        rx + 12,
        16,
        "PINEPHONE + KB151 CONTEXT HUD",
        2,
        COL_ACCENT_CYAN,
    );
    fb.draw_badge(
        rx + rw - 78,
        12,
        "OFF",
        2,
        COL_CURSOR_FG,
        COL_ACCENT_RED,
    );

    // Dual Graphical Battery Indicator Icons + Percentage Readouts (y: 44..84)
    let ph_col = if battery.phone_pct <= 20 {
        COL_ACCENT_RED
    } else if battery.phone_pct <= 45 {
        COL_ACCENT_AMBER
    } else {
        COL_ACCENT_GREEN
    };
    draw_battery_icon(
        fb,
        rx + 12,
        46,
        74,
        28,
        battery.phone_pct,
        battery.phone_charging,
    );
    let ph_lbl = format!(
        "BAT:{:02}%{}",
        battery.phone_pct,
        if battery.phone_charging { "+" } else { "" }
    );
    fb.draw_text(rx + 94, 46, &ph_lbl, 2, ph_col);
    let ph_sub = if battery.phone_charging {
        "PHONE AXP (CHARGING)"
    } else {
        "PHONE AXP (INTERNAL)"
    };
    fb.draw_text(rx + 94, 66, ph_sub, 1, COL_TEXT_DIM);

    let kb_col = if battery.case_pct <= 20 {
        COL_ACCENT_RED
    } else {
        COL_ACCENT_AMBER
    };
    draw_battery_icon(
        fb,
        rx + 286,
        46,
        74,
        28,
        battery.case_pct,
        battery.case_charging,
    );
    let kb_lbl = format!(
        "KB:{:02}%{}",
        battery.case_pct,
        if battery.case_charging { "+" } else { "" }
    );
    fb.draw_text(rx + 368, 46, &kb_lbl, 2, kb_col);
    let kb_sub = if battery.case_charging {
        "CASE IP52 (BOOST ON)"
    } else {
        "CASE IP52 (6000mAh)"
    };
    fb.draw_text(rx + 368, 66, kb_sub, 1, COL_TEXT_DIM);

    let dac_txt = format!(
        "DAC: {}   0xF8: {}",
        telemetry.dac_state.label(),
        telemetry.midi_clock_pulses
    );
    fb.draw_text(rx + 12, 96, &dac_txt, 1, COL_TEXT_MAIN);

    let midi_txt = format!(
        "5-PIN DIN MIDI: {}   XRUN REC: {}",
        if telemetry.midi_connected {
            "ONLINE"
        } else {
            "STANDBY"
        },
        telemetry.xrun_recoveries
    );
    fb.draw_text(rx + 12, 114, &midi_txt, 1, COL_ACCENT_CYAN);

    // Master Volume Bar & Touch Buttons (y: 88..128, x: rx+286..rx+536)
    let vol_pct = ((project.master_volume as u32) * 100 + 127) / 255;
    let vol_lbl = format!("VOL:{:03}% ({:02X})", vol_pct, project.master_volume);
    fb.draw_text(rx + 286, 88, &vol_lbl, 1, COL_ACCENT_AMBER);
    fb.draw_badge(rx + 286, 102, "-", 2, COL_TEXT_MAIN, COL_BG_HEADER);
    fb.fill_rect(rx + 334, 106, 152, 16, COL_BG_DARK);
    fb.draw_rect_outline(rx + 334, 106, 152, 16, 1, COL_BORDER);
    let vol_fill = ((project.master_volume as usize) * 148) / 255;
    if vol_fill > 0 {
        fb.fill_rect(rx + 336, 108, vol_fill, 12, COL_ACCENT_CYAN);
    }
    fb.draw_badge(rx + 494, 102, "+", 2, COL_TEXT_MAIN, COL_BG_HEADER);

    // 2. Song Mini-Map (y: 142..268)
    fb.fill_rect(rx, 142, rw, 126, COL_BG_PANEL);
    fb.draw_rect_outline(rx, 142, rw, 126, 2, COL_BORDER);
    fb.draw_text(rx + 12, 150, "SONG MINI-MAP (8 TRACKS)", 2, COL_ACCENT_AMBER);

    let base_row = ui.song_row.saturating_sub(1).min(SONG_ROWS - 4);
    for r_off in 0..4 {
        let s_row = base_row + r_off;
        let my = 178 + r_off * 20;
        let r_txt = format!("{:02X}:", s_row);
        fb.draw_text(rx + 12, my, &r_txt, 2, COL_TEXT_DIM);
        for t in 0..NUM_TRACKS {
            let cid = project.song[s_row][t];
            let c_txt = if cid == EMPTY_U8 {
                "--".to_string()
            } else {
                format!("{:02X}", cid)
            };
            let col = if s_row == ui.song_row && t == ui.active_track {
                COL_ACCENT_CYAN
            } else if cid != EMPTY_U8 {
                COL_TEXT_MAIN
            } else {
                COL_TEXT_DIM
            };
            fb.draw_text(rx + 74 + t * 58, my, &c_txt, 2, col);
        }
    }

    // 3. 8-Track Live Peak Meters & Voice Readout (y: 276..482)
    fb.fill_rect(rx, 276, rw, 206, COL_BG_PANEL);
    fb.draw_rect_outline(rx, 276, rw, 206, 2, COL_BORDER);
    fb.draw_text(
        rx + 12,
        284,
        "8-TRACK PEAK METERS & GROOVE",
        2,
        COL_ACCENT_CYAN,
    );

    for t in 0..NUM_TRACKS {
        let tx = rx + 16 + t * 66;
        let trk_tel = &telemetry.tracks[t];
        let t_lbl = format!("T{}", t + 1);
        fb.draw_text(
            tx + 8,
            312,
            &t_lbl,
            2,
            if t == ui.active_track {
                COL_ACCENT_AMBER
            } else {
                COL_TEXT_MAIN
            },
        );

        // Stereo Meter background & bars
        let meter_h = 84usize;
        let meter_y = 334usize;
        fb.fill_rect(tx + 6, meter_y, 14, meter_h, COL_BG_DARK);
        fb.fill_rect(tx + 24, meter_y, 14, meter_h, COL_BG_DARK);

        let h_l = ((trk_tel.peak_l.clamp(0.0, 1.0) * meter_h as f32) as usize).min(meter_h);
        let h_r = ((trk_tel.peak_r.clamp(0.0, 1.0) * meter_h as f32) as usize).min(meter_h);
        let bar_col = if trk_tel.peak_l > 0.85 {
            COL_ACCENT_RED
        } else if trk_tel.peak_l > 0.6 {
            COL_ACCENT_AMBER
        } else {
            COL_ACCENT_GREEN
        };
        if h_l > 0 {
            fb.fill_rect(tx + 6, meter_y + (meter_h - h_l), 14, h_l, bar_col);
        }
        if h_r > 0 {
            fb.fill_rect(tx + 24, meter_y + (meter_h - h_r), 14, h_r, bar_col);
        }

        if ui.track_muted[t] {
            fb.draw_text(tx + 6, 424, "MUTE", 1, COL_ACCENT_RED);
        } else {
            let note_b = format_note(trk_tel.active_note);
            let note_s = std::str::from_utf8(&note_b).unwrap_or("---");
            let hex_s = if trk_tel.active_note < 120 {
                format!("{:02X}", trk_tel.active_note)
            } else {
                "--".to_string()
            };
            let note_hex = format!("{} {}", note_s, hex_s);
            fb.draw_text(tx + 2, 424, &note_hex, 1, COL_ACCENT_CYAN);
        }
        let phr_s = if trk_tel.phrase_id == EMPTY_U8 {
            "P--:--".to_string()
        } else {
            format!("P{:02X}:{:01X}", trk_tel.phrase_id, trk_tel.phrase_step)
        };
        fb.draw_text(tx + 2, 440, &phr_s, 1, COL_ACCENT_AMBER);
        if ui.solo_track == Some(t) {
            fb.draw_text(tx + 4, 456, "SOLO", 1, COL_ACCENT_GREEN);
        } else {
            let grv_s = format!("G{:02X}", trk_tel.groove_id);
            fb.draw_text(tx + 8, 456, &grv_s, 1, COL_TEXT_DIM);
        }
    }

    // 4. Named MIDI CC / Live Instrument Context Panel (y: 490..712)
    fb.fill_rect(rx, 490, rw, 222, COL_BG_PANEL);
    fb.draw_rect_outline(rx, 490, rw, 222, 2, COL_BORDER);

    let inst = &project.instruments[ui.current_instrument as usize];
    match inst.engine {
        InstrumentEnum::MidiOut(ref midi) => {
            let title = format!(
                "MIDI PROFILE: {} (CH {:02})",
                midi.profile_name.as_str(),
                midi.channel + 1
            );
            fb.draw_text(rx + 12, 498, &title, 2, COL_ACCENT_MAGENTA);

            for i in 0..MIDI_CC_SLOTS {
                let slot = midi.cc_slots[i];
                let py = 528 + i * 28;
                let lbl = format!("CC{:03} {:<12}", slot.cc_number, slot.name.as_str());
                fb.draw_text(rx + 12, py, &lbl, 2, COL_TEXT_MAIN);

                let bar_max = 180usize;
                let bar_w = ((slot.value as usize) * bar_max) / 127;
                fb.fill_rect(rx + 310, py + 2, bar_max, 14, COL_BG_DARK);
                fb.fill_rect(rx + 310, py + 2, bar_w, 14, COL_ACCENT_CYAN);

                let val_s = format!("{:02X}", slot.value);
                fb.draw_text(rx + 500, py, &val_s, 2, COL_ACCENT_AMBER);
            }
        }
        InstrumentEnum::DrumKit(ref kit) => {
            fb.draw_text(
                rx + 12,
                498,
                "DRUMKIT 12-PAD (8-VOICE + CHOKE)",
                2,
                COL_ACCENT_AMBER,
            );
            for p in 0..DRUM_PADS {
                let col = p % 4;
                let row = p / 4;
                let px = rx + 14 + col * 134;
                let py = 532 + row * 54;
                let pad_note = 60u8 + (p as u8);
                let pad_active = telemetry.tracks.iter().any(|t| {
                    t.active_inst == ui.current_instrument
                        && t.active_note == pad_note
                        && t.peak_l > 0.08
                });
                fb.fill_rect(
                    px,
                    py,
                    124,
                    46,
                    if pad_active {
                        COL_PLAYHEAD_BG
                    } else {
                        COL_BG_DARK
                    },
                );
                fb.draw_rect_outline(
                    px,
                    py,
                    124,
                    46,
                    1,
                    if pad_active {
                        COL_ACCENT_GREEN
                    } else {
                        COL_BORDER
                    },
                );
                let pad = kit.pads[p];
                let p_hdr = format!("P{:X}:{}", p, pad.name.as_str());
                fb.draw_text(px + 6, py + 6, &p_hdr, 1, COL_ACCENT_CYAN);
                let nb = format_note(pad_note);
                let ns = std::str::from_utf8(&nb).unwrap_or("C-4");
                let p_sub = format!("{}:{:02X} LV:{:02X}", ns, pad_note, pad.level);
                fb.draw_text(px + 6, py + 24, &p_sub, 1, COL_TEXT_MAIN);
            }
        }
        InstrumentEnum::SingleSample(ref samp) => {
            let title = format!("SAMPLER: {} (SVF + SLICES)", samp.sample_name.as_str());
            fb.draw_text(rx + 12, 498, &title, 2, COL_ACCENT_GREEN);
            let lines = [
                format!("SVF CUTOFF: {:02X}   RESONANCE: {:02X}", samp.filter.cutoff, samp.filter.resonance),
                format!("SLICES: {:02}       LOOP: {:02X}..{:02X}", samp.slice_count, samp.loop_start, samp.loop_end),
                format!("AMP ADSR: {:02X}/{:02X}/{:02X}/{:02X}", samp.amp_env.attack, samp.amp_env.decay, samp.amp_env.sustain, samp.amp_env.release),
            ];
            for (idx, ln) in lines.iter().enumerate() {
                fb.draw_text(rx + 14, 540 + idx * 36, ln, 2, COL_TEXT_MAIN);
            }
        }
        InstrumentEnum::InternalSynth(ref syn) => {
            let mode_s = match syn.mode {
                SynthMode::VirtualAnalog => "VIRTUAL ANALOG",
                SynthMode::Fm4Op => "4-OPERATOR FM",
            };
            let title = format!("INTERNAL SYNTH: {}", mode_s);
            fb.draw_text(rx + 12, 498, &title, 2, COL_ACCENT_CYAN);
            let lines = [
                format!("SVF CUTOFF: {:02X}   RESONANCE: {:02X}", syn.filter.cutoff, syn.filter.resonance),
                format!("PWM: {:02X}  SUB: {:02X}  DETUNE: {:+03}", syn.pulse_width, syn.sub_osc_level, syn.osc2_detune),
                format!("FM ALGO: {}       FEEDBACK: {:02X}", syn.fm_algorithm + 1, syn.fm_feedback),
            ];
            for (idx, ln) in lines.iter().enumerate() {
                fb.draw_text(rx + 14, 540 + idx * 36, ln, 2, COL_TEXT_MAIN);
            }
        }
    }
}

/// Pins Thread 1 (60 FPS SDL2 UI, Touch/KB151 Input & Disk/I2C Poller) to Cores 0 and 1
/// so UI rendering and disk I/O never contend with Thread 2's isolated Cores 2 and 3.
pub fn configure_ui_thread_affinity() -> bool {
    unsafe {
        let online_cores = libc::sysconf(libc::_SC_NPROCESSORS_ONLN);
        if online_cores >= 2 {
            let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut cpuset);
            libc::CPU_SET(0, &mut cpuset);
            libc::CPU_SET(1, &mut cpuset);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset) == 0
        } else {
            false
        }
    }
}

/// Runs Thread 1's 60 FPS SDL2 UI & Input loop (`1440x720` windowed on Steam Deck `x86_64`).
#[cfg(target_arch = "x86_64")]
pub fn run_sdl2_ui_loop(
    mut project: ProjectState,
    ui_ipc: UiIpcHandle,
    max_frames: Option<u64>,
) -> Result<(), String> {
    let _ = configure_ui_thread_affinity();
    let sdl = sdl2::init()?;
    let video = sdl.video()?;

    let window = video
        .window(
            "pineTracker v2.0 — 8-Track Hex Tracker & Groovebox",
            SCREEN_WIDTH as u32,
            SCREEN_HEIGHT as u32,
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;

    let texture_creator = canvas.texture_creator();
    let mut streaming_tex = texture_creator
        .create_texture_streaming(
            PixelFormatEnum::ARGB8888,
            SCREEN_WIDTH as u32,
            SCREEN_HEIGHT as u32,
        )
        .map_err(|e| e.to_string())?;

    let _event_pump = sdl.event_pump()?;
    let mut fb = FrameBuffer::default();
    let mut ui_state = TrackerUiState::default();
    let mut telemetry = AudioTelemetry::default();
    let mut battery = BatteryHudState::default();
    battery.poll_sysfs();

    let mut playing = false;
    let boot_instant = Instant::now();
    let mut frame_count = 0u64;
    let mut last_touch_ms = 0u64;

    'running: loop {
        let frame_start = Instant::now();
        let now_ms = boot_instant.elapsed().as_millis() as u64;
        let (win_w, win_h) = canvas.output_size().unwrap_or((1440, 720));

        // 1. Drain Telemetry & Retired Sample Buffers from Thread 2 (Zero-Dealloc on Audio Thread)
        while let Ok(msg) = ui_ipc.rx_tel.try_recv() {
            match msg {
                AudioToUiMsg::Telemetry(tel) => {
                    telemetry = tel;
                    playing = tel.playing;
                }
                AudioToUiMsg::RetireSampleBuffer(retired) => {
                    // Explicitly dropped on Thread 1 so heap deallocation never runs on Thread 2
                    drop(retired);
                }
            }
        }

        // 2. Poll KB151 / SDL2 Input & Touch Events safely (compatible with sdl2_compat / SDL3)
        while let Some(event) = poll_sdl_event_safe() {
            match event {
                SafeSdlEvent::Quit => break 'running,
                SafeSdlEvent::KeyDown { keycode, repeat } => {
                    if !repeat
                        || keycode == Keycode::VolumeDown
                        || keycode == Keycode::VolumeUp
                    {
                        if keycode == Keycode::Q
                            && ui_state.mod_option.held
                            && ui_state.mod_shift.held
                        {
                            break 'running;
                        }
                        ui_state.handle_key_down(
                            keycode,
                            now_ms,
                            &mut playing,
                            &mut project,
                            &ui_ipc,
                        );
                    }
                }
                SafeSdlEvent::KeyUp { keycode } => {
                    ui_state.handle_key_up(keycode, now_ms, &ui_ipc);
                }
                SafeSdlEvent::MouseDown { x, y } => {
                    if now_ms.saturating_sub(last_touch_ms) >= 40 || last_touch_ms == 0 {
                        last_touch_ms = now_ms.max(1);
                        let (lx, ly) = map_window_coords_to_logical(x, y, win_w, win_h);
                        ui_state.handle_touch_at(lx, ly, &mut playing, &mut project, &ui_ipc);
                    }
                }
                SafeSdlEvent::FingerDown { norm_x, norm_y } => {
                    if now_ms.saturating_sub(last_touch_ms) >= 40 || last_touch_ms == 0 {
                        last_touch_ms = now_ms.max(1);
                        let wx = (norm_x.clamp(0.0, 1.0) * win_w as f32) as i32;
                        let wy = (norm_y.clamp(0.0, 1.0) * win_h as f32) as i32;
                        let (lx, ly) = map_window_coords_to_logical(wx, wy, win_w, win_h);
                        ui_state.handle_touch_at(lx, ly, &mut playing, &mut project, &ui_ipc);
                    }
                }
                _ => {}
            }
        }

        if ui_state.request_poweroff {
            break 'running;
        }

        ui_state.volume_osd_frames = ui_state.volume_osd_frames.saturating_sub(1);

        // 3. Poll Dual I2C Battery Gauges at 0.2 Hz (every 300 frames @ 60 FPS)
        if frame_count % 300 == 0 && frame_count > 0 {
            battery.poll_sysfs();
        }

        // 4. Render 2-Second Boot Splash Screen (first 120 frames) or Split-Screen Tracker Frame
        if max_frames.is_none() && frame_count < 120 {
            let pct = ((frame_count * 100) / 119).min(100) as u8;
            render_boot_splash_frame(&mut fb, pct);
        } else {
            render_split_screen_frame(&mut fb, &project, &ui_state, &telemetry, &battery);
        }

        // 5. Upload ARGB8888 FrameBuffer to SDL2 Texture and Present (with 90° KMSDRM rotation if portrait panel)
        streaming_tex
            .with_lock(None, |buffer: &mut [u8], pitch: usize| {
                for y in 0..SCREEN_HEIGHT {
                    let src_row = &fb.pixels[y * SCREEN_WIDTH..(y + 1) * SCREEN_WIDTH];
                    let dst_row = &mut buffer[y * pitch..y * pitch + SCREEN_WIDTH * 4];
                    let src_bytes: &[u8] = unsafe {
                        std::slice::from_raw_parts(
                            src_row.as_ptr() as *const u8,
                            SCREEN_WIDTH * 4,
                        )
                    };
                    dst_row.copy_from_slice(src_bytes);
                }
            })
            .map_err(|e| e.to_string())?;

        let (win_w, win_h) = canvas.output_size().unwrap_or((1440, 720));
        if win_h > win_w {
            // PinePhone native 720x1440 portrait KMSDRM panel -> rotate 90° to 1440x720 landscape
            let dst = Rect::new(
                (win_w as i32 - SCREEN_WIDTH as i32) / 2,
                (win_h as i32 - SCREEN_HEIGHT as i32) / 2,
                SCREEN_WIDTH as u32,
                SCREEN_HEIGHT as u32,
            );
            canvas
                .copy_ex(&streaming_tex, None, Some(dst), 90.0, None, false, false)
                .map_err(|e| e.to_string())?;
        } else {
            canvas
                .copy(&streaming_tex, None, None)
                .map_err(|e| e.to_string())?;
        }
        canvas.present();

        frame_count += 1;
        if let Some(limit) = max_frames {
            if frame_count >= limit {
                break 'running;
            }
        }

        let elapsed = frame_start.elapsed();
        let target_frame = Duration::from_micros(16_667);
        if elapsed < target_frame {
            std::thread::sleep(target_frame - elapsed);
        }
    }

    Ok(())
}

// ============================================================================
// PinePhone Native Appliance Backend: Direct /dev/fb0 + Linux evdev (/dev/input/event*)
// Zero SDL2 / SDL3 / Mesa / OpenGL / Wayland / X11 dependencies (< 50ms startup as PID 1)
// ============================================================================

#[cfg(not(target_arch = "x86_64"))]
const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
#[cfg(not(target_arch = "x86_64"))]
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct FbVarScreenInfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct FbFixScreenInfo {
    id: [u8; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    type_: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    _pad: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct LinuxInputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

#[cfg(not(target_arch = "x86_64"))]
const EV_SYN: u16 = 0x00;
#[cfg(not(target_arch = "x86_64"))]
const EV_KEY: u16 = 0x01;
#[cfg(not(target_arch = "x86_64"))]
const EV_ABS: u16 = 0x03;
#[cfg(not(target_arch = "x86_64"))]
const BTN_TOUCH: u16 = 0x14a;
#[cfg(not(target_arch = "x86_64"))]
const ABS_X: u16 = 0x00;
#[cfg(not(target_arch = "x86_64"))]
const ABS_Y: u16 = 0x01;
#[cfg(not(target_arch = "x86_64"))]
const ABS_MT_POSITION_X: u16 = 0x35;
#[cfg(not(target_arch = "x86_64"))]
const ABS_MT_POSITION_Y: u16 = 0x36;
#[cfg(not(target_arch = "x86_64"))]
const ABS_MT_TRACKING_ID: u16 = 0x39;

/// Native PinePhone `/dev/fb0` + `/dev/input/event*` 60 FPS UI & Input loop.
/// Rotates our `1440x720` landscape canvas 90° clockwise into the PinePhone's `720x1440` portrait
/// hardware scanout buffer via a cached intermediate buffer (`rot_buf`) for burst AXI writes.
#[cfg(not(target_arch = "x86_64"))]
pub fn run_sdl2_ui_loop(
    mut project: ProjectState,
    ui_ipc: UiIpcHandle,
    max_frames: Option<u64>,
) -> Result<(), String> {
    let _ = configure_ui_thread_affinity();

    // Hide VT console cursor on tty0/tty1 so it never blinks over the framebuffer
    let _ = fs::write("/dev/tty0", b"\x1b[?25l\x1b[2J");
    let _ = fs::write("/dev/tty1", b"\x1b[?25l\x1b[2J");
    let _ = fs::write("/sys/class/graphics/fbcon/cursor_blink", b"0\n");

    let fb_fd = unsafe {
        libc::open(
            b"/dev/fb0\0".as_ptr() as *const libc::c_char,
            libc::O_RDWR | libc::O_CLOEXEC,
        )
    };
    if fb_fd < 0 {
        return Err("Failed to open /dev/fb0 on PinePhone".to_string());
    }

    let mut vinfo = FbVarScreenInfo::default();
    let mut finfo = FbFixScreenInfo::default();
    unsafe {
        let _ = libc::ioctl(fb_fd, FBIOGET_VSCREENINFO, &mut vinfo);
        let _ = libc::ioctl(fb_fd, FBIOGET_FSCREENINFO, &mut finfo);
    }

    let fb_w = if vinfo.xres > 0 { vinfo.xres as usize } else { 720 };
    let fb_h = if vinfo.yres > 0 { vinfo.yres as usize } else { 1440 };
    let stride_bytes = if finfo.line_length > 0 {
        finfo.line_length as usize
    } else {
        fb_w * 4
    };
    let stride_u32 = (stride_bytes / 4).max(fb_w);
    let map_bytes = if finfo.smem_len > 0 {
        finfo.smem_len as usize
    } else {
        stride_bytes * fb_h
    };

    let fb_ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fb_fd,
            0,
        )
    };
    if fb_ptr == libc::MAP_FAILED || fb_ptr.is_null() {
        unsafe {
            libc::close(fb_fd);
        }
        return Err("Failed to mmap /dev/fb0 on PinePhone".to_string());
    }
    let hw_slice: &mut [u32] =
        unsafe { std::slice::from_raw_parts_mut(fb_ptr as *mut u32, map_bytes / 4) };

    // Cached intermediate buffer for 90° clockwise portrait rotation (`720 x 1440`)
    let mut rot_buf: Vec<u32> = vec![0u32; stride_u32 * fb_h];

    // Ensure the PinePhone Allwinner A64 LRADC Volume Up/Down driver (sun4i-lradc-keys) is loaded
    // when running as PID 1 without systemd-udevd.
    let _ = std::process::Command::new("modprobe")
        .arg("sun4i-lradc-keys")
        .status();

    // Open all `/dev/input/event0..15` non-blocking (`KB151`, `Goodix` touchscreen, `axp20x-pek` power, `sun4i-lradc-keys` volume)
    let mut input_fds: [libc::c_int; 16] = [-1; 16];
    let open_evdev_devices = |fds: &mut [libc::c_int; 16]| {
        for (i, fd_slot) in fds.iter_mut().enumerate() {
            if *fd_slot < 0 {
                let path = format!("/dev/input/event{}\0", i);
                let fd = unsafe {
                    libc::open(
                        path.as_ptr() as *const libc::c_char,
                        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                    )
                };
                if fd >= 0 {
                    *fd_slot = fd;
                }
            }
        }
    };
    open_evdev_devices(&mut input_fds);

    let mut fb = FrameBuffer::default();
    let mut ui_state = TrackerUiState::default();
    let mut telemetry = AudioTelemetry::default();
    let mut battery = BatteryHudState::default();
    battery.poll_sysfs();

    let mut playing = false;
    let boot_instant = Instant::now();
    let mut frame_count = 0u64;
    let mut last_touch_ms = 0u64;

    let mut touch_raw_x: i32 = 0;
    let mut touch_raw_y: i32 = 0;
    let mut finger_down = false;
    let mut pending_tap = false;

    'running: loop {
        let frame_start = Instant::now();
        let now_ms = boot_instant.elapsed().as_millis() as u64;

        // Periodically scan for newly probed/hot-plugged input devices every 0.5 seconds
        if frame_count % 30 == 0 && frame_count > 0 {
            open_evdev_devices(&mut input_fds);
        }

        // 1. Drain Telemetry & Retired Sample Buffers from Thread 2 (Zero-Dealloc on Audio Thread)
        while let Ok(msg) = ui_ipc.rx_tel.try_recv() {
            match msg {
                AudioToUiMsg::Telemetry(tel) => {
                    telemetry = tel;
                    playing = tel.playing;
                }
                AudioToUiMsg::RetireSampleBuffer(retired) => {
                    drop(retired);
                }
            }
        }

        // 2. Poll Linux evdev (/dev/input/event*) for KB151 Keyboard, Goodix Touch & Power Button
        let mut ev_buf = [LinuxInputEvent::default(); 32];
        for fd_slot in input_fds.iter_mut() {
            let fd = *fd_slot;
            if fd < 0 {
                continue;
            }
            loop {
                let bytes_read = unsafe {
                    libc::read(
                        fd,
                        ev_buf.as_mut_ptr() as *mut libc::c_void,
                        std::mem::size_of_val(&ev_buf),
                    )
                };
                if bytes_read < std::mem::size_of::<LinuxInputEvent>() as isize {
                    if bytes_read < 0 {
                        let errno = unsafe { *libc::__errno_location() };
                        if errno == libc::ENODEV {
                            unsafe {
                                libc::close(fd);
                            }
                            *fd_slot = -1;
                        }
                    }
                    break;
                }
                let count = (bytes_read as usize) / std::mem::size_of::<LinuxInputEvent>();
                for ev in &ev_buf[..count] {
                    match ev.type_ {
                        EV_KEY => {
                            if ev.code == EVDEV_KEY_POWER && ev.value == 1 {
                                ui_state.request_poweroff = true;
                                break 'running;
                            }
                            if ev.code == BTN_TOUCH {
                                if ev.value == 1 {
                                    finger_down = true;
                                    pending_tap = true;
                                } else if ev.value == 0 {
                                    finger_down = false;
                                }
                                continue;
                            }
                            if let Some(keycode) = evdev_code_to_keycode(ev.code) {
                                let is_vol = keycode == Keycode::VolumeDown
                                    || keycode == Keycode::VolumeUp;
                                if ev.value == 1 || (ev.value == 2 && is_vol) {
                                    if keycode == Keycode::Q
                                        && ui_state.mod_option.held
                                        && ui_state.mod_shift.held
                                    {
                                        ui_state.request_poweroff = true;
                                        break 'running;
                                    }
                                    ui_state.handle_key_down(
                                        keycode,
                                        now_ms,
                                        &mut playing,
                                        &mut project,
                                        &ui_ipc,
                                    );
                                } else if ev.value == 0 {
                                    ui_state.handle_key_up(keycode, now_ms, &ui_ipc);
                                }
                            }
                        }
                        EV_ABS => match ev.code {
                            ABS_X | ABS_MT_POSITION_X => {
                                touch_raw_x = ev.value;
                            }
                            ABS_Y | ABS_MT_POSITION_Y => {
                                touch_raw_y = ev.value;
                            }
                            ABS_MT_TRACKING_ID => {
                                if ev.value >= 0 && !finger_down {
                                    finger_down = true;
                                    pending_tap = true;
                                } else if ev.value < 0 {
                                    finger_down = false;
                                }
                            }
                            _ => {}
                        },
                        EV_SYN if ev.code == 0 && pending_tap => {
                            pending_tap = false;
                            if now_ms.saturating_sub(last_touch_ms) >= 40 || last_touch_ms == 0 {
                                last_touch_ms = now_ms.max(1);
                                let (lx, ly) = map_window_coords_to_logical(
                                    touch_raw_x,
                                    touch_raw_y,
                                    fb_w as u32,
                                    fb_h as u32,
                                );
                                ui_state.handle_touch_at(
                                    lx,
                                    ly,
                                    &mut playing,
                                    &mut project,
                                    &ui_ipc,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if ui_state.request_poweroff {
            break 'running;
        }

        ui_state.volume_osd_frames = ui_state.volume_osd_frames.saturating_sub(1);

        // 3. Poll Dual I2C Battery Gauges at 0.2 Hz (every 300 frames @ 60 FPS)
        if frame_count % 300 == 0 && frame_count > 0 {
            battery.poll_sysfs();
        }

        // 4. Render 2-Second Boot Splash Screen (first 120 frames) or Split-Screen Tracker Frame
        if max_frames.is_none() && frame_count < 120 {
            let pct = ((frame_count * 100) / 119).min(100) as u8;
            render_boot_splash_frame(&mut fb, pct);
        } else {
            render_split_screen_frame(&mut fb, &project, &ui_state, &telemetry, &battery);
        }

        // 5. Blit 1440x720 landscape canvas into /dev/fb0 (with 90° clockwise rotation on 720x1440 portrait DSI)
        blit_to_linux_fb(&fb, &mut rot_buf, hw_slice, fb_w, fb_h, stride_u32);

        frame_count += 1;
        if let Some(limit) = max_frames {
            if frame_count >= limit {
                break 'running;
            }
        }

        let elapsed = frame_start.elapsed();
        let target_frame = Duration::from_micros(16_667);
        if elapsed < target_frame {
            std::thread::sleep(target_frame - elapsed);
        }
    }

    // Paint clean instant shutdown screen and sync filesystems before powering off
    fb.clear(COL_BG_DARK);
    fb.fill_rect(420, 290, 600, 140, COL_BG_PANEL);
    fb.draw_rect_outline(420, 290, 600, 140, 3, COL_ACCENT_CYAN);
    fb.draw_text(520, 330, "PINE TRACKER v2.0", 3, COL_ACCENT_CYAN);
    fb.draw_text(536, 376, "POWERING OFF...", 2, COL_ACCENT_AMBER);
    blit_to_linux_fb(&fb, &mut rot_buf, hw_slice, fb_w, fb_h, stride_u32);

    for fd in input_fds {
        if fd >= 0 {
            unsafe {
                libc::close(fd);
            }
        }
    }
    unsafe {
        libc::munmap(fb_ptr, map_bytes);
        libc::close(fb_fd);
        libc::sync();
        if max_frames.is_none() {
            libc::reboot(libc::RB_POWER_OFF);
        }
    }

    Ok(())
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn blit_to_linux_fb(
    fb: &FrameBuffer,
    rot_buf: &mut [u32],
    hw_slice: &mut [u32],
    fb_w: usize,
    fb_h: usize,
    stride_u32: usize,
) {
    if fb_h > fb_w {
        // 90° clockwise rotation: Landscape (lx: 0..1440, ly: 0..720) -> Portrait (px: 0..720, py: 0..1440)
        // py = lx, px = (SCREEN_HEIGHT - 1) - ly
        let max_py = fb_h.min(SCREEN_WIDTH);
        let max_px = fb_w.min(SCREEN_HEIGHT);
        for ly in 0..max_px {
            let px = (max_px - 1) - ly;
            let src_row = &fb.pixels[ly * SCREEN_WIDTH..ly * SCREEN_WIDTH + max_py];
            for (py, &pixel) in src_row.iter().enumerate() {
                rot_buf[py * stride_u32 + px] = pixel;
            }
        }
        let copy_len = (stride_u32 * fb_h).min(hw_slice.len()).min(rot_buf.len());
        hw_slice[..copy_len].copy_from_slice(&rot_buf[..copy_len]);
    } else {
        let max_y = fb_h.min(SCREEN_HEIGHT);
        let max_x = fb_w.min(SCREEN_WIDTH);
        for y in 0..max_y {
            let src = &fb.pixels[y * SCREEN_WIDTH..y * SCREEN_WIDTH + max_x];
            let dst_start = y * stride_u32;
            if dst_start + max_x <= hw_slice.len() {
                hw_slice[dst_start..dst_start + max_x].copy_from_slice(src);
            }
        }
    }
}

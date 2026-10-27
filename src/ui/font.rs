pub const SCREEN_WIDTH: usize = 1440;
pub const SCREEN_HEIGHT: usize = 720;
pub const LEFT_VIEW_WIDTH: usize = 864; // 60% of 1440
pub const RIGHT_HUD_X: usize = 864;
pub const RIGHT_HUD_WIDTH: usize = 576; // 40% of 1440

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RgbColor(pub u8, pub u8, pub u8);

impl RgbColor {
    #[inline]
    pub const fn pack_argb8888(self) -> u32 {
        0xFF00_0000 | ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | (self.2 as u32)
    }
}

// M8 High-Contrast Dark Hardware Theme Palette
pub const COL_BG_DARK: RgbColor = RgbColor(12, 14, 20);
pub const COL_BG_PANEL: RgbColor = RgbColor(20, 24, 34);
pub const COL_BG_HEADER: RgbColor = RgbColor(28, 35, 50);
pub const COL_BG_ROW_ALT: RgbColor = RgbColor(16, 20, 28);
pub const COL_BORDER: RgbColor = RgbColor(52, 66, 92);
pub const COL_TEXT_MAIN: RgbColor = RgbColor(225, 232, 245);
pub const COL_TEXT_DIM: RgbColor = RgbColor(110, 125, 150);
pub const COL_ACCENT_CYAN: RgbColor = RgbColor(0, 228, 255);
pub const COL_ACCENT_AMBER: RgbColor = RgbColor(255, 184, 0);
pub const COL_ACCENT_MAGENTA: RgbColor = RgbColor(255, 64, 160);
pub const COL_ACCENT_GREEN: RgbColor = RgbColor(40, 230, 120);
pub const COL_ACCENT_RED: RgbColor = RgbColor(255, 68, 68);
pub const COL_CURSOR_BG: RgbColor = RgbColor(0, 210, 245);
pub const COL_CURSOR_FG: RgbColor = RgbColor(10, 12, 18);
pub const COL_PLAYHEAD_BG: RgbColor = RgbColor(55, 42, 18);

/// Returns an 8x8 bitmap glyph (8 rows of 8 MSB-left bits) for any ASCII character.
/// Zero external font dependencies; designed for crisp 2x (16x16) and 3x (24x24) integer scaling.
#[inline]
pub const fn glyph_8x8(ch: u8) -> [u8; 8] {
    let c = if ch >= b'a' && ch <= b'z' {
        ch - 32
    } else {
        ch
    };
    match c {
        b'0' => [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00],
        b'1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'2' => [0x3C, 0x66, 0x06, 0x1C, 0x30, 0x66, 0x7E, 0x00],
        b'3' => [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00],
        b'4' => [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00],
        b'5' => [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00],
        b'6' => [0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00],
        b'7' => [0x7E, 0x66, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x00],
        b'8' => [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00],
        b'9' => [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00],
        b'A' => [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00],
        b'B' => [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00],
        b'C' => [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00],
        b'D' => [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00],
        b'E' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00],
        b'F' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'G' => [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00],
        b'H' => [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        b'I' => [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'J' => [0x1E, 0x0C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38, 0x00],
        b'K' => [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00],
        b'L' => [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00],
        b'M' => [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00],
        b'N' => [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00],
        b'O' => [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'P' => [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'Q' => [0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00],
        b'R' => [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00],
        b'S' => [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00],
        b'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'U' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'V' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        b'W' => [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00],
        b'X' => [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00],
        b'Y' => [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00],
        b'Z' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        b'+' => [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00],
        b'#' => [0x24, 0x7E, 0x24, 0x24, 0x7E, 0x24, 0x00, 0x00],
        b':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30],
        b'/' => [0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00, 0x00],
        b'[' => [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00],
        b']' => [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00],
        b'(' => [0x18, 0x30, 0x60, 0x60, 0x60, 0x30, 0x18, 0x00],
        b')' => [0x18, 0x0C, 0x06, 0x06, 0x06, 0x0C, 0x18, 0x00],
        b'<' => [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00],
        b'>' => [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00],
        b'|' => [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'%' => [0x62, 0x64, 0x08, 0x10, 0x26, 0x46, 0x00, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00],
        b'=' => [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00],
        b'*' => [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00],
        b'!' => [0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00],
        b'?' => [0x3C, 0x66, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00],
        b'&' => [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00],
        b'\'' => [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00; 8],
    }
}

/// Contiguous ARGB8888 software canvas (`1440 x 720`) uploaded once per frame to an SDL2 texture.
pub struct FrameBuffer {
    pub pixels: Vec<u32>,
    pub width: usize,
    pub height: usize,
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new(SCREEN_WIDTH, SCREEN_HEIGHT)
    }
}

impl FrameBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            pixels: vec![COL_BG_DARK.pack_argb8888(); width * height],
            width,
            height,
        }
    }

    #[inline]
    pub fn clear(&mut self, color: RgbColor) {
        self.pixels.fill(color.pack_argb8888());
    }

    #[inline]
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: RgbColor) {
        let packed = color.pack_argb8888();
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if x >= x_end || y >= y_end {
            return;
        }
        for row in y..y_end {
            let start = row * self.width + x;
            let end = row * self.width + x_end;
            self.pixels[start..end].fill(packed);
        }
    }

    pub fn draw_rect_outline(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        thickness: usize,
        color: RgbColor,
    ) {
        self.fill_rect(x, y, w, thickness, color);
        self.fill_rect(x, (y + h).saturating_sub(thickness), w, thickness, color);
        self.fill_rect(x, y, thickness, h, color);
        self.fill_rect((x + w).saturating_sub(thickness), y, thickness, h, color);
    }

    /// Renders an integer-scaled (`scale` = 1, 2, or 3) monospace string at `(x, y)`.
    pub fn draw_text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: RgbColor) {
        let s = scale.max(1);
        let packed = color.pack_argb8888();
        let char_advance = 8 * s;

        for (char_idx, byte) in text.bytes().enumerate() {
            if byte == b' ' {
                continue;
            }
            let glyph = glyph_8x8(byte);
            let gx = x + char_idx * char_advance;
            if gx >= self.width {
                break;
            }

            for (row_idx, &bits) in glyph.iter().enumerate() {
                if bits == 0 {
                    continue;
                }
                for col_idx in 0..8 {
                    if (bits & (0x80 >> col_idx)) != 0 {
                        let px = gx + col_idx * s;
                        let py = y + row_idx * s;
                        for dy in 0..s {
                            let ry = py + dy;
                            if ry >= self.height {
                                continue;
                            }
                            let row_base = ry * self.width;
                            for dx in 0..s {
                                let rx = px + dx;
                                if rx < self.width {
                                    self.pixels[row_base + rx] = packed;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn draw_badge(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        scale: usize,
        fg: RgbColor,
        bg: RgbColor,
    ) -> usize {
        let s = scale.max(1);
        let pad_x = 6;
        let pad_y = 4;
        let w = text.len() * 8 * s + pad_x * 2;
        let h = 8 * s + pad_y * 2;
        self.fill_rect(x, y, w, h, bg);
        self.draw_text(x + pad_x, y + pad_y, text, s, fg);
        w
    }

    /// Counts how many pixels inside `(x, y, w, h)` differ from `bg_color` (used for automated UI verification).
    pub fn count_drawn_pixels(
        &self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        bg_color: RgbColor,
    ) -> usize {
        let bg = bg_color.pack_argb8888();
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let mut count = 0;
        for row in y..y_end {
            let base = row * self.width;
            for col in x..x_end {
                if self.pixels[base + col] != bg {
                    count += 1;
                }
            }
        }
        count
    }
}

// Rasterisation convention: the firmware generator (build.rs) with its 8-bit
// clamps widened to the pack format's 16-bit fields. Bump CONVENTION_VERSION
// in lib.rs whenever a rule here changes.

use fontdue::Font;
use pulp_fontpack::{GlyphEntry, Metrics};

// coverage at or above this is a black pixel
const THRESHOLD: u8 = 100;

// (line_height, ascent) in pixels, rounded up
pub fn line_metrics(font: &Font, px: f32) -> Option<(u16, u16)> {
    let lm = font.horizontal_line_metrics(px)?;
    Some((lm.new_line_size.ceil() as u16, lm.ascent.ceil() as u16))
}

pub fn glyph(font: &Font, c: char, px: f32) -> GlyphEntry {
    let (m, coverage) = font.rasterize(c, px);
    let width = m.width.min(usize::from(u16::MAX));
    let height = m.height.min(usize::from(u16::MAX));
    let stride = width.div_ceil(8);

    // 1 bpp, msb first, row major; row padding bits stay 0
    let mut bitmap = vec![0u8; stride * height];
    for y in 0..height {
        for x in 0..width {
            if coverage[y * m.width + x] >= THRESHOLD {
                bitmap[y * stride + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }

    // top row relative to the baseline, y down: above the baseline is negative
    let top = -i64::from(m.ymin) - m.height as i64;
    GlyphEntry {
        codepoint: c,
        metrics: Metrics {
            advance: (m.advance_width + 0.5) as u16,
            offset_x: clamp_i16(i64::from(m.xmin)),
            offset_y: clamp_i16(top),
            width: width as u16,
            height: height as u16,
        },
        bitmap,
    }
}

fn clamp_i16(v: i64) -> i16 {
    v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

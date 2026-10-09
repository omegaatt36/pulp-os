// The glyph drawn for a character the selected font lacks: a hollow square on
// the baseline, scaled from the pixel size alone. A pure function of
// `FontInfo`, no I/O and no alloc, so it works with any pack (or none).

use crate::{FontInfo, Metrics};

// box side: 3/4 of the pixel size, kept drawable and inside a u8
fn side(info: &FontInfo) -> u16 {
    let s = (u32::from(info.pixel_size) * 3 / 4).clamp(3, 255);
    // clamped above, so it fits
    s as u16
}

pub fn missing_glyph_metrics(info: &FontInfo) -> Metrics {
    let s = side(info);
    let advance = info.pixel_size.max(s + 2);
    Metrics {
        advance,
        // (advance - s) / 2 <= 32640, fits an i16
        offset_x: ((advance - s) / 2) as i16,
        offset_y: -(s as i16),
        width: s,
        height: s,
    }
}

// writes the bitmap (1 bpp, MSB first) into `out[..stride * height]` and leaves
// the rest of `out` alone; None if `out` is too small
pub fn render_missing_glyph(info: &FontInfo, out: &mut [u8]) -> Option<Metrics> {
    let m = missing_glyph_metrics(info);
    let s = usize::from(m.width);
    let stride = s.div_ceil(8);
    let out = out.get_mut(..stride * s)?;
    out.fill(0);
    for (y, row) in out.chunks_exact_mut(stride).enumerate() {
        for x in 0..s {
            let edge = x == 0 || y == 0 || x == s - 1 || y == s - 1;
            if let Some(byte) = row.get_mut(x / 8).filter(|_| edge) {
                *byte |= 0x80 >> (x % 8);
            }
        }
    }
    Some(m)
}

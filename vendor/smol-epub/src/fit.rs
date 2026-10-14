//! Output geometry of a decoded picture: the largest size within the bounds
//! that keeps the aspect ratio, never larger than the source, and the source
//! pixel each output pixel is taken from (nearest neighbour).
//!
//! The decoders used to divide by a whole number, so a picture a few pixels
//! wider than the bounds came out at half the room it had.

#[derive(Clone, Copy)]
pub(crate) struct Fit {
    w: u64,
    h: u64,
    pub(crate) out_w: usize,
    pub(crate) out_h: usize,
}

impl Fit {
    pub(crate) fn new(w: usize, h: usize, max_w: usize, max_h: usize) -> Self {
        let (w, h) = (w.max(1) as u64, h.max(1) as u64);
        let (max_w, max_h) = (max_w.max(1) as u64, max_h.max(1) as u64);
        // the axis the bounds squeeze more sets the size, the other follows it
        // (rounded, which cannot pass a whole-number bound)
        let (out_w, out_h) = if w <= max_w && h <= max_h {
            (w, h)
        } else if w * max_h >= h * max_w {
            (max_w, ((2 * h * max_w + w) / (2 * w)).clamp(1, max_h))
        } else {
            (((2 * w * max_h + h) / (2 * h)).clamp(1, max_w), max_h)
        };
        Self {
            w,
            h,
            out_w: out_w as usize,
            out_h: out_h as usize,
        }
    }

    /// The same output geometry over a source reduced by `1 << shift` (every
    /// dimension rounded up, as a reduced IDCT yields it).
    pub(crate) fn reduced(&self, shift: u8) -> Self {
        let s = 1u64 << shift;
        Self {
            w: self.w.div_ceil(s),
            h: self.h.div_ceil(s),
            ..*self
        }
    }

    /// Source column of output column `ox`.
    #[inline]
    pub(crate) fn src_x(&self, ox: usize) -> usize {
        (ox as u64 * self.w / self.out_w as u64) as usize
    }

    /// Source row of output row `oy`. Rows are at least one apart, so a
    /// decoder walking the source top to bottom meets each at most once.
    #[inline]
    pub(crate) fn src_y(&self, oy: usize) -> usize {
        (oy as u64 * self.h / self.out_h as u64) as usize
    }
}

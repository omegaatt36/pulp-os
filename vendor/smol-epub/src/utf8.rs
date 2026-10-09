//! Streaming UTF-8 repair.
//!
//! Every malformed stretch becomes one U+FFFD per maximal subpart (the
//! longest prefix of a valid sequence, else one byte), the same count as
//! `String::from_utf8_lossy`. The second-byte ranges after E0, ED, F0 and F4
//! exclude overlong, surrogate and above-U+10FFFF forms; C0, C1 and F5..FF
//! are single-byte subparts. Same policy as `decode_utf8_char` in the Pulp
//! kernel; this crate does not depend on it.

use alloc::vec::Vec;

const REPLACEMENT: [u8; 3] = [0xEF, 0xBF, 0xBD];

/// Repairs a byte stream one byte at a time; the bytes of a sequence cut
/// between two calls are held in the struct.
#[derive(Clone, Copy)]
pub(crate) struct Utf8Fixer {
    // bytes of the sequence in progress
    buf: [u8; 4],
    len: u8,
    // total length of the sequence in progress
    need: u8,
    // allowed range of its second byte
    lo: u8,
    hi: u8,
}

impl Utf8Fixer {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0; 4],
            len: 0,
            need: 0,
            lo: 0,
            hi: 0,
        }
    }

    /// True while a sequence is waiting for more bytes.
    pub(crate) const fn pending(&self) -> bool {
        self.len > 0
    }

    fn start(&mut self, b: u8, need: u8, lo: u8, hi: u8) {
        self.buf[0] = b;
        self.len = 1;
        self.need = need;
        self.lo = lo;
        self.hi = hi;
    }

    /// Feed one byte; `emit` receives each completed scalar (valid UTF-8,
    /// 1 to 4 bytes) or U+FFFD.
    pub(crate) fn push(&mut self, b: u8, emit: &mut impl FnMut(&[u8])) {
        if self.len > 0 {
            let (lo, hi) = if self.len == 1 {
                (self.lo, self.hi)
            } else {
                (0x80, 0xBF)
            };
            if (lo..=hi).contains(&b) {
                self.buf[self.len as usize] = b;
                self.len += 1;
                if self.len == self.need {
                    emit(&self.buf[..self.len as usize]);
                    self.len = 0;
                }
                return;
            }
            // b ends the subpart and is looked at again as a new start
            emit(&REPLACEMENT);
            self.len = 0;
        }
        match b {
            0x00..=0x7F => emit(&[b]),
            0xC2..=0xDF => self.start(b, 2, 0x80, 0xBF),
            0xE0 => self.start(b, 3, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE | 0xEF => self.start(b, 3, 0x80, 0xBF),
            0xED => self.start(b, 3, 0x80, 0x9F),
            0xF0 => self.start(b, 4, 0x90, 0xBF),
            0xF1..=0xF3 => self.start(b, 4, 0x80, 0xBF),
            0xF4 => self.start(b, 4, 0x80, 0x8F),
            // stray continuation, C0, C1, F5..FF
            _ => emit(&REPLACEMENT),
        }
    }

    /// End of input (or of the text run): a sequence still waiting is cut
    /// off, one U+FFFD.
    pub(crate) fn finish(&mut self, emit: &mut impl FnMut(&[u8])) {
        if self.len > 0 {
            emit(&REPLACEMENT);
            self.len = 0;
        }
    }
}

/// Copy `src` into `dst` repaired, keeping whole scalars only: the longest
/// prefix of the repaired text that fits. Returns the bytes written.
pub(crate) fn copy_sanitized(dst: &mut [u8], src: &[u8]) -> usize {
    let mut n = 0;
    let mut full = false;
    let mut emit = |s: &[u8]| {
        if !full && n + s.len() <= dst.len() {
            dst[n..n + s.len()].copy_from_slice(s);
            n += s.len();
        } else {
            full = true;
        }
    };
    let mut fx = Utf8Fixer::new();
    for &b in src {
        fx.push(b, &mut emit);
    }
    fx.finish(&mut emit);
    n
}

/// Repair `buf` in place. Valid input is left alone; otherwise the buffer
/// grows by what the replacements add (a replacement is never shorter than
/// the stretch it stands for), the input moves to the tail and the repaired
/// text is written from the front, always behind the read position.
pub(crate) fn sanitize_inplace(buf: &mut Vec<u8>) {
    if core::str::from_utf8(buf).is_ok() {
        return;
    }
    let len = buf.len();
    let mut total = 0;
    let mut fx = Utf8Fixer::new();
    let mut count = |s: &[u8]| total += s.len();
    for &b in buf.iter() {
        fx.push(b, &mut count);
    }
    fx.finish(&mut count);

    let extra = total - len;
    buf.resize(total, 0);
    buf.copy_within(0..len, extra);
    let mut w = 0;
    for r in extra..total {
        let b = buf[r];
        fx.push(b, &mut |s| {
            buf[w..w + s.len()].copy_from_slice(s);
            w += s.len();
        });
    }
    fx.finish(&mut |s| {
        buf[w..w + s.len()].copy_from_slice(s);
        w += s.len();
    });
    debug_assert_eq!(w, total);
}

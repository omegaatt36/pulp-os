// UTF-8 decoding utilities for no_std environments
//
// provides both iterator-based and single-char decoding interfaces
// for processing UTF-8 byte slices without std::str

// decode one UTF-8 character at buf[pos]
// returns (char, byte_length); a malformed sequence yields one '\u{FFFD}' per
// maximal subpart (the longest prefix of a valid sequence, else one byte),
// the same count as String::from_utf8_lossy
// panics if pos >= buf.len()
#[inline]
pub fn decode_utf8_char(buf: &[u8], pos: usize) -> (char, usize) {
    let b0 = buf[pos];

    // ASCII fast path
    if b0 < 0x80 {
        return (b0 as char, 1);
    }

    // sequence length, lead payload and the range of the second byte
    // (narrowed after E0, ED, F0 and F4 to exclude overlong, surrogate and
    // above-U+10FFFF forms)
    let (mut cp, expected, lo, hi) = match b0 {
        0xC2..=0xDF => ((b0 as u32) & 0x1F, 2, 0x80, 0xBF),
        0xE0 => (0, 3, 0xA0, 0xBF),
        0xE1..=0xEC | 0xEE..=0xEF => ((b0 as u32) & 0x0F, 3, 0x80, 0xBF),
        0xED => (0x0D, 3, 0x80, 0x9F),
        0xF0 => (0, 4, 0x90, 0xBF),
        0xF1..=0xF3 => ((b0 as u32) & 0x07, 4, 0x80, 0xBF),
        0xF4 => (4, 4, 0x80, 0x8F),
        // stray continuation, C0/C1, F5..FF
        _ => return ('\u{FFFD}', 1),
    };

    let len = buf.len();
    for i in 1..expected {
        // cut off by the end of the buffer: the valid prefix is one subpart
        if pos + i >= len {
            return ('\u{FFFD}', i);
        }
        let cont = buf[pos + i];
        let (min, max) = if i == 1 { (lo, hi) } else { (0x80, 0xBF) };
        if cont < min || cont > max {
            return ('\u{FFFD}', i);
        }
        cp = (cp << 6) | (cont as u32 & 0x3F);
    }

    (char::from_u32(cp).unwrap_or('\u{FFFD}'), expected)
}

// length of the longest prefix of s, at most max bytes, that ends on a scalar
// boundary; s is expected to be valid UTF-8
#[inline]
pub fn utf8_prefix_len(s: &[u8], max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut n = max;
    // s[n] a continuation byte: the scalar straddles the cut, drop all of it
    while n > 0 && s[n] & 0xC0 == 0x80 {
        n -= 1;
    }
    n
}

// bytes at the end of buf that start a scalar the buffer is too short to
// finish (0 when buf ends on a boundary or the tail is malformed)
pub fn utf8_incomplete_tail_len(buf: &[u8]) -> usize {
    let len = buf.len();
    for k in 1..=len.min(3) {
        if buf[len - k] & 0xC0 != 0x80 {
            return match core::str::from_utf8(&buf[len - k..]) {
                Err(e) if e.error_len().is_none() => k,
                _ => 0,
            };
        }
    }
    0
}

// iterator over UTF-8 characters in a byte slice
// malformed sequences yield U+FFFD, see decode_utf8_char
pub struct Utf8Iter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Utf8Iter<'a> {
    #[inline]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
}

impl Iterator for Utf8Iter<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if self.pos >= self.data.len() {
            return None;
        }

        let (ch, len) = decode_utf8_char(self.data, self.pos);
        self.pos += len;
        Some(ch)
    }
}

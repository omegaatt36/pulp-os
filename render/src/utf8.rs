// UTF-8 decoding utilities for no_std environments
//
// provides both iterator-based and single-char decoding interfaces
// for processing UTF-8 byte slices without std::str

// result of decoding one UTF-8 sequence at buf[pos]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded {
    // a scalar value and its byte length
    Scalar(char, usize),
    // malformed input; skip this many bytes and emit U+FFFD
    Invalid(usize),
    // buf ends inside a sequence that could still complete: the caller
    // either resumes at pos with more input or, at true EOF, treats the
    // remaining bytes as one invalid sequence
    Incomplete,
}

// decode one UTF-8 sequence at buf[pos], telling a sequence cut off by
// the end of buf apart from malformed input
// panics if pos >= buf.len()
#[inline]
pub fn decode_step(buf: &[u8], pos: usize) -> Decoded {
    let b0 = buf[pos];

    // ASCII fast path
    if b0 < 0x80 {
        return Decoded::Scalar(b0 as char, 1);
    }

    // Determine expected sequence length from lead byte
    let (mut cp, expected) = if b0 < 0xC2 {
        // Stray continuation or overlong two-byte lead
        return Decoded::Invalid(1);
    } else if b0 < 0xE0 {
        ((b0 as u32) & 0x1F, 2)
    } else if b0 < 0xF0 {
        ((b0 as u32) & 0x0F, 3)
    } else if b0 < 0xF5 {
        ((b0 as u32) & 0x07, 4)
    } else {
        // Invalid lead byte
        return Decoded::Invalid(1);
    };

    // Decode continuation bytes that are present
    let avail = expected.min(buf.len() - pos);
    for i in 1..avail {
        let cont = buf[pos + i];
        if cont & 0xC0 != 0x80 {
            // Invalid continuation byte
            return Decoded::Invalid(i);
        }
        if i == 1
            && ((b0 == 0xE0 && cont < 0xA0)
                || (b0 == 0xED && cont >= 0xA0)
                || (b0 == 0xF0 && cont < 0x90)
                || (b0 == 0xF4 && cont >= 0x90))
        {
            // No later byte can make this prefix a valid scalar.
            return Decoded::Invalid(1);
        }
        cp = (cp << 6) | (cont as u32 & 0x3F);
    }

    if avail < expected {
        return Decoded::Incomplete;
    }

    match char::from_u32(cp) {
        Some(ch) => Decoded::Scalar(ch, expected),
        None => Decoded::Invalid(expected),
    }
}

// decode one UTF-8 character at buf[pos]
// returns (char, byte_length); malformed sequences yield '\u{FFFD}', and
// a sequence cut off by the end of buf consumes the rest of buf
// panics if pos >= buf.len()
#[inline]
pub fn decode_utf8_char(buf: &[u8], pos: usize) -> (char, usize) {
    match decode_step(buf, pos) {
        Decoded::Scalar(ch, len) => (ch, len),
        Decoded::Invalid(len) => ('\u{FFFD}', len),
        Decoded::Incomplete => ('\u{FFFD}', buf.len() - pos),
    }
}

// length of the longest prefix of buf that ends on a sequence boundary:
// buf.len() minus an incomplete trailing sequence (at most 3 bytes);
// bytes that can never complete are kept and decode as invalid
//
// for chunked input, decode buf[..complete_prefix_len(buf)] and resume
// the next chunk at that offset; at true EOF decode all of buf instead,
// so a truncated tail yields one U+FFFD rather than stalling
pub fn complete_prefix_len(buf: &[u8]) -> usize {
    let len = buf.len();
    // the lead byte of an incomplete sequence is within the last 3 bytes
    for back in 1..=len.min(3) {
        let pos = len - back;
        if buf[pos] & 0xC0 != 0x80 {
            return match decode_step(buf, pos) {
                Decoded::Incomplete => pos,
                _ => len,
            };
        }
    }
    len
}

// iterator over UTF-8 characters in a byte slice
// invalid sequences yield U+FFFD
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

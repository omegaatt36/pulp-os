// ruby groups of the stripped text: reading them out of a line and placing their parts
//
// The stripper (`smol_epub::html_strip`) writes a base text that has a reading as
//   OPEN base READING reading CLOSE
// (each a two-byte marker). The base may hold bold, italic and heading markers;
// the reading is text only. A group never holds a line break and is always closed,
// so the layout treats it as one unbreakable scalar and the draw loops set it as
// one unit.
//
// A group is as long as the longer of its two parts, measured along the line (a
// row of text, or a column in vertical writing); the shorter part is centred on
// it. The reading is set in its own, smaller pack size beside the base text:
// above it in a row, to the right of it in a column. Both take `room` of the
// cross axis on top of the base's em box, and the line pitch keeps a lead on
// top of that (`min_pitch`) so that a reading is plainly nearer its own base
// than the neighbouring line.

use super::{Style, StyleState};
use pulp_kernel::util::decode_utf8_char;
use smol_epub::html_strip::{
    BOLD_OFF, BOLD_ON, HEADING_OFF, HEADING_ON, ITALIC_OFF, ITALIC_ON, MARKER, RUBY_BASE_MAX,
    RUBY_CLOSE, RUBY_OPEN, RUBY_READING, RUBY_READING_MAX,
};

/// Space between the em box of the base and the em box of its reading.
pub const GAP: u16 = 1;

// longest a well-formed group is: both parts, their markers and the style markers
// a base can hold
const GROUP_MAX: usize = 2 + RUBY_BASE_MAX + 12 + 2 + RUBY_READING_MAX + 2;

/// A ruby group read out of the text.
pub struct Group<'a> {
    // base text with the style markers inside it
    base: &'a [u8],
    reading: &'a [u8],
    /// Bytes of the whole group, from the OPEN marker to the end of the CLOSE marker.
    pub len: usize,
}

pub enum Read<'a> {
    Group(Group<'a>),
    /// The text ends before the group does; read again with more of it.
    Cut,
    /// The marker starts no well-formed group: skip it.
    Stray,
}

/// Read the group whose OPEN marker is at `buf[at..]`.
pub fn read(buf: &[u8], at: usize) -> Read<'_> {
    if buf.get(at..at + 2) != Some(&[MARKER, RUBY_OPEN]) {
        return Read::Stray;
    }
    let base_at = at + 2;
    let mut base_end = None;
    let mut i = base_at;
    loop {
        if i >= buf.len() {
            return Read::Cut;
        }
        if i - at > GROUP_MAX {
            return Read::Stray;
        }
        match buf[i] {
            MARKER => {
                let Some(&tag) = buf.get(i + 1) else {
                    return Read::Cut;
                };
                match tag {
                    RUBY_READING if base_end.is_none() => base_end = Some(i),
                    RUBY_CLOSE => {
                        let (base, reading) = match base_end {
                            Some(end) => (&buf[base_at..end], &buf[end + 2..i]),
                            None => (&buf[base_at..i], &buf[i..i]),
                        };
                        return Read::Group(Group {
                            base,
                            reading: reading.trim_ascii(),
                            len: i + 2 - at,
                        });
                    }
                    BOLD_ON | BOLD_OFF | ITALIC_ON | ITALIC_OFF | HEADING_ON | HEADING_OFF
                        if base_end.is_none() => {}
                    _ => return Read::Stray,
                }
                i += 2;
            }
            b'\n' => return Read::Stray,
            _ => i += 1,
        }
    }
}

impl Group<'_> {
    /// Each scalar of the base with the style it is set in, starting from
    /// `styles`; the style after the base, which the text goes on in.
    pub fn each_base(&self, styles: StyleState, mut f: impl FnMut(char, Style)) -> StyleState {
        let mut styles = styles;
        let mut i = 0;
        while i < self.base.len() {
            if self.base[i] == MARKER && i + 1 < self.base.len() {
                styles.apply_marker(self.base[i + 1]);
                i += 2;
                continue;
            }
            let (ch, n) = decode_utf8_char(self.base, i);
            f(ch, styles.style());
            i += n;
        }
        styles
    }

    /// Each scalar of the reading, which has no style but its size.
    pub fn each_reading(&self, mut f: impl FnMut(char)) {
        let mut i = 0;
        while i < self.reading.len() {
            let (ch, n) = decode_utf8_char(self.reading, i);
            f(ch);
            i += n;
        }
    }

    pub fn has_reading(&self) -> bool {
        !self.reading.is_empty()
    }

    /// First scalar of the base, which decides the line breaking before the group.
    pub fn first(&self) -> char {
        let mut first = None;
        self.each_base(StyleState::from_flags(0), |ch, _| {
            first.get_or_insert(ch);
        });
        first.unwrap_or('\u{fffd}')
    }
}

/// How the two parts of a group lie along the line: its length, and where the
/// base and the reading start in it.
pub struct Span {
    pub len: u32,
    pub base_at: u32,
    pub reading_at: u32,
}

pub fn span(base_len: u32, reading_len: u32) -> Span {
    let len = base_len.max(reading_len);
    Span {
        len,
        base_at: (len - base_len) / 2,
        reading_at: (len - reading_len) / 2,
    }
}

/// Cross-axis space a reading takes beside the em box of its base.
pub const fn room(ruby_px: u16) -> u16 {
    ruby_px + GAP
}

/// Cross-axis extent of a base and its reading together.
pub const fn content(body_px: u16, ruby_px: u16) -> u16 {
    body_px + room(ruby_px)
}

/// Smallest line pitch (row height, column width) of text with readings: the
/// two together and half a reading's size of lead between one line and the next.
pub const fn min_pitch(body_px: u16, ruby_px: u16) -> u16 {
    content(body_px, ruby_px) + ruby_px / 2
}

/// Row: distance from the top of the line to the horizontal baseline of the
/// reading. Its em box is the top of the line's `room`; the base's follows.
pub const fn row_reading_baseline(ruby_px: u16) -> i32 {
    super::vertical::em_ascent(ruby_px)
}

/// Column: the centre lines of the base and of the reading, given the right
/// edge of the column's content. The column holds the base's em box and the
/// reading's, side by side, centred in its pitch.
pub fn column_centers(col_right: i32, pitch: u16, body_px: u16, ruby_px: u16) -> (i32, i32) {
    let content = i32::from(content(body_px, ruby_px));
    // glyph boxes hang from `center - px / 2`, so the centres follow from the left edges
    let base_left = col_right - (i32::from(pitch) - content) / 2 - content;
    let reading_left = base_left + i32::from(body_px) + i32::from(GAP);
    (
        base_left + i32::from(body_px) / 2,
        reading_left + i32::from(ruby_px) / 2,
    )
}

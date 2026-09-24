// line and page wrapping of reader text over a proportional font
//
// input is one chunk of the reader's text stream: UTF-8 with the
// smol-epub html_strip markup ([marker, code] style pairs and
// [marker, img_ref, len, path..] image references). output is one page
// of LineSpans, byte ranges into the chunk, and the byte count consumed:
// the next page's offset, always on a scalar and markup boundary
//
// breaks: at spaces / NBSP (the space stays at the end of the line
// unless it overflows itself, then it is dropped), after a soft hyphen,
// and between scalars where line_break::break_between allows it (CJK,
// with the clreq prohibition rules). when no break exists within the
// line, it breaks before the overflowing scalar. newlines end a line and
// are dropped, with a CR before them
//
// chunk contract (utf8::complete_prefix_len): before EOF only the
// complete prefix is laid out and a scalar or markup sequence cut by the
// chunk end is left for the next chunk; at EOF everything is laid out,
// so a truncated tail is one U+FFFD

use crate::line_break::break_between;
use crate::utf8::{complete_prefix_len, decode_utf8_char};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Regular,
    Bold,
    Italic,
    Heading,
}

// one laid-out line: buf[start..start + len], or an image line
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineSpan {
    pub start: u16,
    pub len: u16,
    // style the line starts in (the renderer applies the markers inside
    // the line itself), or FLAG_IMAGE
    pub flags: u8,
    // block-quote depth
    pub indent: u8,
}

impl LineSpan {
    pub const EMPTY: Self = Self {
        start: 0,
        len: 0,
        flags: 0,
        indent: 0,
    };

    pub const FLAG_BOLD: u8 = 1 << 0;
    pub const FLAG_ITALIC: u8 = 1 << 1;
    pub const FLAG_HEADING: u8 = 1 << 2;
    pub const FLAG_IMAGE: u8 = 1 << 3;

    #[inline]
    pub fn is_image(&self) -> bool {
        self.flags & Self::FLAG_IMAGE != 0
    }

    // first line of an image: start/len hold the image path
    #[inline]
    pub fn is_image_origin(&self) -> bool {
        self.is_image() && self.len > 0
    }

    pub fn style(&self) -> Style {
        if self.flags & Self::FLAG_HEADING != 0 {
            Style::Heading
        } else if self.flags & Self::FLAG_BOLD != 0 {
            Style::Bold
        } else if self.flags & Self::FLAG_ITALIC != 0 {
            Style::Italic
        } else {
            Style::Regular
        }
    }
}

// markup byte codes; the caller passes the stream format's constants
// (smol_epub::html_strip) so they stay defined in one place
#[derive(Clone, Copy, Debug)]
pub struct Markup {
    pub marker: u8,
    pub img_ref: u8,
    pub bold_on: u8,
    pub bold_off: u8,
    pub italic_on: u8,
    pub italic_off: u8,
    pub heading_on: u8,
    pub heading_off: u8,
    pub quote_on: u8,
    pub quote_off: u8,
}

// glyph metrics source. fallible so a measurer can read an SD-resident
// font pack (holding its reader) or prepared per-page metrics
pub trait Measure {
    type Error;
    // horizontal advance of ch in style, in px. A single-face font may
    // intentionally use the same metrics for every style.
    fn advance(&mut self, ch: char, style: Style) -> Result<u32, Self::Error>;
    fn line_height(&self, style: Style) -> u16;
}

pub struct WrapParams<'a> {
    pub markup: Markup,
    pub max_lines: usize,
    pub max_width_px: u32,
    // width lost per block-quote level
    pub indent_px: u32,
    // pre-scanned heights of the chunk's images, in order; 0 or missing
    // entries use default_img_h
    pub img_heights: &'a [u16],
    pub default_img_h: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wrapped {
    // bytes laid out; the next page starts here
    pub consumed: usize,
    pub line_count: usize,
    // style and quote state at `consumed`, for a page that begins there
    pub next_state: WrapState,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct WrapState {
    pub flags: u8,
    pub indent: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageProgressError {
    Stalled,
    BeyondEnd,
}

// A chunk can end before a page fills, especially when its last UTF-8
// scalar is incomplete. Continue while unread bytes remain, regardless
// of the number of lines emitted by wrap.
pub fn next_page_offset(
    offset: usize,
    consumed: usize,
    total: usize,
) -> Result<Option<usize>, PageProgressError> {
    let next = offset
        .checked_add(consumed)
        .ok_or(PageProgressError::BeyondEnd)?;
    if next > total {
        return Err(PageProgressError::BeyondEnd);
    }
    if next == offset && next < total {
        return Err(PageProgressError::Stalled);
    }
    Ok((next < total).then_some(next))
}

// style state at a byte position
#[derive(Clone, Copy, Default)]
struct Pen {
    bold: bool,
    italic: bool,
    heading: bool,
    indent: u8,
}

impl Pen {
    fn from_state(state: WrapState) -> Self {
        Self {
            bold: state.flags & LineSpan::FLAG_BOLD != 0,
            italic: state.flags & LineSpan::FLAG_ITALIC != 0,
            heading: state.flags & LineSpan::FLAG_HEADING != 0,
            indent: state.indent,
        }
    }

    fn state(&self) -> WrapState {
        WrapState {
            flags: self.flags(),
            indent: self.indent,
        }
    }

    #[inline]
    fn style(&self) -> Style {
        if self.heading {
            Style::Heading
        } else if self.bold {
            Style::Bold
        } else if self.italic {
            Style::Italic
        } else {
            Style::Regular
        }
    }

    #[inline]
    fn flags(&self) -> u8 {
        (self.bold as u8) | ((self.italic as u8) << 1) | ((self.heading as u8) << 2)
    }

    // the pen a text line starts with
    fn of_line(span: &LineSpan) -> Self {
        Pen {
            bold: span.flags & LineSpan::FLAG_BOLD != 0,
            italic: span.flags & LineSpan::FLAG_ITALIC != 0,
            heading: span.flags & LineSpan::FLAG_HEADING != 0,
            indent: span.indent,
        }
    }

    // apply the code of a [marker, code] pair; unknown codes are no-ops
    fn apply(&mut self, code: u8, mk: &Markup) {
        match code {
            c if c == mk.bold_on => self.bold = true,
            c if c == mk.bold_off => self.bold = false,
            c if c == mk.italic_on => self.italic = true,
            c if c == mk.italic_off => self.italic = false,
            c if c == mk.heading_on => self.heading = true,
            c if c == mk.heading_off => self.heading = false,
            c if c == mk.quote_on => self.indent = self.indent.saturating_add(1),
            c if c == mk.quote_off => self.indent = self.indent.saturating_sub(1),
            _ => {}
        }
    }
}

// the glyph measured and drawn for every wrap space (' ' and NBSP)
const SPACE: char = ' ';

// what the text (non-markup) bytes at one position are to layout
enum Text {
    // ends the line; dropped
    Newline,
    // CR, other controls, DEL, stray continuation bytes: zero width,
    // never measured or drawn
    Invisible,
    // zero-width break opportunity, never measured or drawn
    SoftHyphen,
    // ' ' or NBSP: measured and drawn as SPACE
    Space,
    // any other scalar, measured and drawn as itself
    Scalar(char),
}

// classify buf[i] (buf already cut to the laid-out prefix) and its length
fn scan_text(buf: &[u8], i: usize) -> (Text, usize) {
    let b = buf[i];
    if b == b'\n' {
        return (Text::Newline, 1);
    }
    if b < 0x20 || b == 0x7F || (0x80..0xC0).contains(&b) {
        return (Text::Invisible, 1);
    }
    let (ch, len) = if b < 0x80 {
        (b as char, 1)
    } else {
        decode_utf8_char(buf, i)
    };
    let text = match ch {
        '\u{00AD}' => Text::SoftHyphen,
        c if is_wrap_space(c) => Text::Space,
        c => Text::Scalar(c),
    };
    (text, len)
}

// Reconstruct the pen at a returned page boundary. The scanner can have
// looked beyond that boundary to find a line break, so its final pen is not
// necessarily the pen the next page must start with.
fn state_at(buf: &[u8], at: usize, initial: WrapState, mk: Markup) -> WrapState {
    let mut pen = Pen::from_state(initial);
    let mut i = 0;
    while i < at {
        if buf[i] == mk.marker && i + 1 < at {
            let code = buf[i + 1];
            if code == mk.img_ref && i + 2 < at {
                let path_len = buf[i + 2] as usize;
                if path_len > 0 && i + 3 + path_len <= at {
                    i += 3 + path_len;
                    continue;
                }
            }
            pen.apply(code, &mk);
            i += 2;
        } else {
            i += 1;
        }
    }
    pen.state()
}

// the glyphs a laid-out text line draws, in order, each with the style
// wrap measured it in: exactly the (char, style) pairs wrap passed to
// Measure::advance for the line's bytes, so drawing and page preparation
// see what layout saw. image lines yield nothing
pub fn line_glyphs<'a>(buf: &'a [u8], span: &LineSpan, markup: Markup) -> LineGlyphs<'a> {
    let line = if span.is_image() {
        &[]
    } else {
        let start = span.start as usize;
        &buf[start..start + span.len as usize]
    };
    LineGlyphs {
        line,
        i: 0,
        pen: Pen::of_line(span),
        markup,
    }
}

// Empty newline/control spans can occupy a line without drawing a page.
// Images count as content even when their continuation span has no bytes.
pub fn page_has_content(buf: &[u8], lines: &[LineSpan], markup: Markup) -> bool {
    lines
        .iter()
        .any(|span| span.is_image() || line_glyphs(buf, span, markup).any(|(ch, _)| ch != SPACE))
}

pub struct LineGlyphs<'a> {
    line: &'a [u8],
    i: usize,
    pen: Pen,
    markup: Markup,
}

impl Iterator for LineGlyphs<'_> {
    type Item = (char, Style);

    fn next(&mut self) -> Option<(char, Style)> {
        while self.i < self.line.len() {
            let i = self.i;
            if self.line[i] == self.markup.marker {
                // a lone marker ends the text only at EOF (wrap skips it)
                if let Some(&code) = self.line.get(i + 1) {
                    self.pen.apply(code, &self.markup);
                    self.i += 2;
                } else {
                    self.i += 1;
                }
                continue;
            }
            let (text, len) = scan_text(self.line, i);
            self.i += len;
            match text {
                Text::Space => return Some((SPACE, self.pen.style())),
                Text::Scalar(ch) => return Some((ch, self.pen.style())),
                Text::Newline | Text::Invisible | Text::SoftHyphen => {}
            }
        }
        None
    }
}

struct Out<'l> {
    lines: &'l mut [LineSpan],
    max: usize,
    count: usize,
}

impl Out<'_> {
    // true once the page is full
    #[inline]
    fn push(&mut self, span: LineSpan) -> bool {
        if self.count < self.max {
            self.lines[self.count] = span;
            self.count += 1;
        }
        self.count >= self.max
    }

    #[inline]
    fn text(&mut self, buf: &[u8], start: usize, end: usize, pen: Pen) -> bool {
        let end = if end > start && buf[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };
        self.push(LineSpan {
            start: start as u16,
            len: (end - start) as u16,
            flags: pen.flags(),
            indent: pen.indent,
        })
    }
}

#[inline]
fn is_wrap_space(ch: char) -> bool {
    matches!(ch, ' ' | '\u{00A0}')
}

// lay out one page of buf into lines; eof tells whether buf ends at the
// end of the text. spans are u16, so buf must be at most 65,535 bytes.
pub fn wrap<M: Measure>(
    buf: &[u8],
    eof: bool,
    measure: &mut M,
    p: &WrapParams<'_>,
    lines: &mut [LineSpan],
) -> Result<Wrapped, M::Error> {
    wrap_impl(buf, eof, measure, p, lines, WrapState::default(), false)
}

// Stream-aware wrapping: withhold the last unfinished line before EOF so a
// punctuation scalar in the next chunk can join it. `initial` is the state
// stored beside this page's byte offset. A zero-line result can consume only
// non-rendering bytes; the caller must refill the same page, not index it.
pub fn wrap_stream<M: Measure>(
    buf: &[u8],
    eof: bool,
    measure: &mut M,
    p: &WrapParams<'_>,
    lines: &mut [LineSpan],
    initial: WrapState,
) -> Result<Wrapped, M::Error> {
    wrap_impl(buf, eof, measure, p, lines, initial, true)
}

fn wrap_impl<M: Measure>(
    buf: &[u8],
    eof: bool,
    measure: &mut M,
    p: &WrapParams<'_>,
    lines: &mut [LineSpan],
    initial: WrapState,
    hold_tail: bool,
) -> Result<Wrapped, M::Error> {
    debug_assert!(buf.len() <= u16::MAX as usize);
    let mk = p.markup;
    let width_for = |indent: u8| p.max_width_px.saturating_sub(p.indent_px * indent as u32);
    let mut limit = if eof {
        buf.len()
    } else {
        complete_prefix_len(buf)
    };
    let max = p.max_lines.min(lines.len());
    let mut out = Out {
        lines,
        max,
        count: 0,
    };

    // style state at the scan position
    let mut pen = Pen::from_state(initial);
    // the current line: first byte, starting style, width used, whether
    // anything measured is on it yet
    let mut line_start = 0usize;
    let mut line_pen = pen;
    let mut cursor: u32 = 0;
    let mut has_content = false;
    let mut first_content = None;
    let mut max_w = width_for(line_pen.indent);
    // last break opportunity on the line: where the next line would
    // start, the width before it, and the style there
    let mut brk = 0usize;
    let mut brk_x: u32 = 0;
    let mut brk_pen = pen;
    // last measured scalar, None after a space, soft hyphen, line start
    let mut prev: Option<char> = None;
    let mut img_idx = 0usize;

    macro_rules! start_line {
        ($at:expr, $pen:expr, $x:expr, $content:expr) => {{
            line_start = $at;
            line_pen = $pen;
            cursor = $x;
            has_content = $content;
            first_content = if $content { Some($at) } else { None };
            max_w = width_for(line_pen.indent);
            brk = line_start;
            brk_x = 0;
            brk_pen = line_pen;
        }};
    }

    macro_rules! done {
        ($at:expr) => {
            return Ok(Wrapped {
                consumed: $at,
                line_count: out.count,
                next_state: state_at(buf, $at, initial, mk),
            })
        };
    }

    if max == 0 {
        done!(0);
    }

    let mut i = 0;
    while i < limit {
        let b = buf[i];

        if b == mk.marker {
            // a sequence cut by the chunk end waits for the next chunk
            if i + 1 >= limit {
                if !eof {
                    limit = i;
                    break;
                }
                i += 1;
                continue;
            }
            let code = buf[i + 1];
            if code == mk.img_ref {
                if i + 2 >= limit && !eof {
                    limit = i;
                    break;
                }
                if i + 2 < limit {
                    let path_len = buf[i + 2] as usize;
                    let path_start = i + 3;
                    if path_len > 0 && path_start + path_len > limit && !eof {
                        limit = i;
                        break;
                    }
                    if path_len > 0 && path_start + path_len <= limit {
                        if line_start < i && out.text(buf, line_start, i, line_pen) {
                            done!(i);
                        }
                        let line_h = measure.line_height(Style::Regular).max(1);
                        let img_h = match p.img_heights.get(img_idx) {
                            Some(&h) if h > 0 => h,
                            _ => p.default_img_h,
                        };
                        img_idx += 1;
                        // enough lines to cover the image height
                        let img_lines = img_h.div_ceil(line_h).max(1) as usize;
                        let mut full = out.push(LineSpan {
                            start: path_start as u16,
                            len: path_len as u16,
                            flags: LineSpan::FLAG_IMAGE,
                            indent: 0,
                        });
                        for _ in 1..img_lines {
                            if full {
                                break;
                            }
                            full = out.push(LineSpan {
                                flags: LineSpan::FLAG_IMAGE,
                                ..LineSpan::EMPTY
                            });
                        }
                        i = path_start + path_len;
                        start_line!(i, pen, 0, false);
                        prev = None;
                        if full {
                            done!(i);
                        }
                        continue;
                    }
                }
            }

            pen.apply(code, &mk);
            // markers before the line's first glyph set its style
            if !has_content {
                line_pen = pen;
                max_w = width_for(pen.indent);
            }
            i += 2;
            continue;
        }

        let (text, len) = scan_text(&buf[..limit], i);
        let ch = match text {
            Text::Newline => {
                let full = out.text(buf, line_start, i, line_pen);
                start_line!(i + 1, pen, 0, false);
                prev = None;
                if full {
                    done!(i + 1);
                }
                i += 1;
                continue;
            }
            Text::Invisible => {
                i += 1;
                continue;
            }
            Text::SoftHyphen => {
                brk = i + len;
                brk_x = cursor;
                brk_pen = pen;
                prev = None;
                i += len;
                continue;
            }
            Text::Space => SPACE,
            Text::Scalar(ch) => ch,
        };

        if matches!(text, Text::Space) {
            if !has_content {
                first_content = Some(i);
            }
            cursor += measure.advance(ch, pen.style())?;
            has_content = true;
            brk = i + len;
            brk_x = cursor;
            brk_pen = pen;
            prev = None;
            if cursor > max_w {
                // the overflowing space itself is dropped
                let full = out.text(buf, line_start, i, line_pen);
                start_line!(i + len, pen, 0, false);
                if full {
                    done!(i + len);
                }
            }
            i += len;
            continue;
        }

        if let Some(pc) = prev
            && break_between(pc, ch)
        {
            brk = i;
            brk_x = cursor;
            brk_pen = pen;
        }

        let adv = measure.advance(ch, pen.style())?;
        cursor += adv;
        if !has_content {
            first_content = Some(i);
        }
        let had_content = has_content;
        has_content = true;
        // a scalar wider than the whole line stays on it alone
        if cursor > max_w && had_content {
            if brk > line_start {
                let full = out.text(buf, line_start, brk, line_pen);
                start_line!(brk, brk_pen, cursor - brk_x, true);
                if full {
                    done!(line_start);
                }
                // text carried from brk is itself longer than the line
                if cursor > max_w && i > line_start {
                    let full = out.text(buf, line_start, i, line_pen);
                    start_line!(i, pen, adv, true);
                    if full {
                        done!(i);
                    }
                }
            } else {
                // no legal break on the line: break before this scalar
                let full = out.text(buf, line_start, i, line_pen);
                start_line!(i, pen, adv, true);
                if full {
                    done!(i);
                }
            }
        }
        prev = Some(ch);
        i += len;
    }

    if hold_tail && !eof {
        // Skip the non-rendering prefix of the pending line; its markers
        // are captured in next_state. This guarantees progress even when
        // 8 KiB of controls precedes one glyph at the boundary.
        let resume = first_content.unwrap_or(limit);
        if resume > 0 {
            done!(resume);
        }
        // A line that occupies the whole bounded input has no place to
        // carry it. Commit it at the chunk edge as an emergency break.
    }

    if line_start < limit && (!hold_tail || has_content) {
        let end = if buf[limit - 1] == b'\r' {
            limit - 1
        } else {
            limit
        };
        if end > line_start {
            out.text(buf, line_start, end, line_pen);
        }
    }
    done!(limit)
}

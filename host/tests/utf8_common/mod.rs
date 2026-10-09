// Shared helpers of the utf8_* test files: a TXT book on a virtual card, the
// page walks, and the text invariants. Nothing here lays text out or decodes
// it for the reader: expected text comes from std (`str::from_utf8`,
// `String::from_utf8_lossy`, `char`), never from the code under test.
#![allow(dead_code)]

use pulp_host::fonts::{FontSet, Style};
use pulp_host::reader::{Action, CHARS_PER_LINE, Phase, Rig};
use pulp_host::render::{Framebuffer, WIDTH, render_full};
use pulp_host::storage::VirtualStorage;

pub const BOOK: &str = "BOOK.TXT";
pub const FFFD: char = '\u{FFFD}';

// (book font index, reading theme index): the smallest, a middle and the largest font
pub const CONFIGS: [(u8, u8); 3] = [(0, 0), (2, 1), (4, 3)];

// one 2-byte, one 3-byte and one 4-byte scalar (0xC3 0xA9 / 0xE8 0x87 0xBA / 0xF0 0xA0 0xAE 0xB7)
pub const E_ACUTE: &str = "\u{e9}";
pub const TAI: &str = "\u{81fa}";
pub const YOSHI: &str = "\u{20bb7}";
// 3-byte CJK plus CJK punctuation, with one 2-byte and one 4-byte scalar
pub const MIXED_UNIT: &str =
    "\u{e9}\u{81fa}\u{7063}\u{300c}\u{7e41}\u{9ad4}\u{4e2d}\u{6587}\u{300d}\u{ff0c}\u{20bb7}";

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }
}

// valid prose: paragraphs of 3..=60 pieces (2-, 3-, 4-byte and ASCII scalars, words and
// spaces) separated by "\n"; at least `min_bytes` long
pub fn prose(seed: u64, min_bytes: usize) -> String {
    const PIECES: [&str; 17] = [
        "\u{e9}",
        "\u{e8}",
        "\u{81fa}",
        "\u{7063}",
        "\u{300c}",
        "\u{7e41}",
        "\u{9ad4}",
        "\u{4e2d}",
        "\u{6587}",
        "\u{300d}",
        "\u{ff0c}",
        "\u{20bb7}",
        "a",
        "bc",
        " ",
        "word",
        "\u{ff1f}",
    ];
    let mut rng = Lcg(seed);
    let mut out = String::new();
    while out.len() < min_bytes {
        let n = 3 + rng.below(58);
        for _ in 0..n {
            out.push_str(PIECES[rng.below(PIECES.len())]);
        }
        out.push('\n');
    }
    out
}

pub fn repeat_to(unit: &str, min_bytes: usize) -> String {
    unit.repeat(min_bytes.div_ceil(unit.len()))
}

pub fn padded(pad: usize, text: &str) -> Vec<u8> {
    let mut v = vec![b'a'; pad];
    v.extend_from_slice(text.as_bytes());
    v
}

// ---------------------------------------------------------------------------
// rigs and walks
// ---------------------------------------------------------------------------

pub fn card(data: &[u8]) -> VirtualStorage {
    let s = VirtualStorage::memory_with(&[(BOOK, data)]);
    s.ensure_pulp_dir()
        .expect("card has _PULP/ the way the firmware boots it");
    s
}

pub fn fresh_rig(data: &[u8], font: u8, theme: u8) -> Rig {
    let mut r = Rig::new(card(data));
    r.configure(font, theme);
    r
}

pub fn open_book(data: &[u8], font: u8, theme: u8) -> Rig {
    let mut r = fresh_rig(data, font, theme);
    r.open(BOOK);
    r
}

// the monospace layout path (no Regular font data)
pub fn open_mono(data: &[u8]) -> Rig {
    let mut r = fresh_rig(data, 2, 1);
    r.open_monospace(BOOK);
    r
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub index: usize,
    pub offset: u32,
    pub lines: Vec<Vec<u8>>,
}

pub fn snapshot(r: &Rig) -> Page {
    let index = r.page();
    let offsets = r.page_offsets();
    assert!(
        index < offsets.len(),
        "current page {index} is in the page table"
    );
    Page {
        index,
        offset: offsets[index],
        lines: r.lines(),
    }
}

// Press(Next) until the page stops changing
pub fn walk_forward(r: &mut Rig) -> Vec<Page> {
    assert_eq!(r.phase(), Phase::Ready, "book is open");
    let mut out = vec![snapshot(r)];
    loop {
        let before = r.page();
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready, "page after {before}");
        if r.page() == before {
            return out;
        }
        assert_eq!(r.page(), before + 1, "Next moves exactly one page");
        out.push(snapshot(r));
        assert!(out.len() < 3000, "walk_forward runaway");
    }
}

// Press(Prev) until the page stops changing; result is in visiting order (last page first)
pub fn walk_back(r: &mut Rig) -> Vec<Page> {
    let mut out = vec![snapshot(r)];
    loop {
        let before = r.page();
        r.press(Action::Prev);
        assert_eq!(r.phase(), Phase::Ready);
        if r.page() == before {
            return out;
        }
        assert_eq!(r.page() + 1, before, "Prev moves exactly one page");
        out.push(snapshot(r));
        assert!(out.len() < 3000, "walk_back runaway");
    }
}

// ---------------------------------------------------------------------------
// text invariants
// ---------------------------------------------------------------------------

// A wrapper may drop the space that overflows a line and consumes line
// terminators, so text is compared modulo ASCII space / CR / LF.
pub fn strip_ws(b: &[u8]) -> Vec<u8> {
    b.iter()
        .copied()
        .filter(|c| !matches!(c, b' ' | b'\r' | b'\n'))
        .collect()
}

pub fn text_of(lines: &[Vec<u8>]) -> Vec<u8> {
    lines.iter().flat_map(|l| l.iter().copied()).collect()
}

pub fn all_lines(pages: &[Page]) -> Vec<Vec<u8>> {
    pages.iter().flat_map(|p| p.lines.iter().cloned()).collect()
}

fn is_continuation(b: u8) -> bool {
    b & 0xC0 == 0x80
}

// Valid `data` (RFC 3629): every page, line and the whole book keep each scalar
// whole and the text unchanged.
//  (1) the page table starts at 0, increases strictly and every page starts on a scalar start;
//  (2) page i's text is the file's text between its offset and the next page's offset, so
//      no scalar is split over two pages;
//  (3) the text of the whole book equals the file's text byte for byte;
//  (4) every line is valid UTF-8 without U+FFFD (a cut scalar is one or the other);
//  (5) no page has more lines than the page capacity.
pub fn check_whole_scalars(r: &Rig, pages: &[Page], data: &[u8], label: &str) {
    assert_eq!(r.phase(), Phase::Ready, "{label}");
    assert!(
        std::str::from_utf8(data).is_ok(),
        "{label}: the fixture is valid UTF-8"
    );
    let offs = r.page_offsets();
    assert_eq!(
        offs.len(),
        pages.len(),
        "{label}: page table == pages visited"
    );
    assert_eq!(offs[0], 0, "{label}: first page starts at byte 0");
    for w in offs.windows(2) {
        assert!(
            w[0] < w[1],
            "{label}: page offsets strictly increase: {offs:?}"
        );
    }
    for (i, p) in pages.iter().enumerate() {
        let o = p.offset as usize;
        assert!(
            o < data.len().max(1),
            "{label}: page {i} starts inside the file"
        );
        assert!(
            o == 0 || !is_continuation(data[o]),
            "{label}: page {i} starts inside a scalar (byte {o})"
        );
        let end = pages.get(i + 1).map_or(data.len(), |n| n.offset as usize);
        assert_eq!(
            strip_ws(&text_of(&p.lines)),
            strip_ws(&data[o..end]),
            "{label}: page {i} text != file bytes [{o}..{end})"
        );
        assert!(
            p.lines.len() <= r.max_lines(),
            "{label}: page {i} has {} lines > {}",
            p.lines.len(),
            r.max_lines()
        );
    }
    assert_eq!(
        strip_ws(&text_of(&all_lines(pages))),
        strip_ws(data),
        "{label}: book text lost, duplicated or altered"
    );
    for (i, p) in pages.iter().enumerate() {
        for l in &p.lines {
            let s = std::str::from_utf8(l).unwrap_or_else(|e| {
                panic!(
                    "{label}: page {i}: line is not valid UTF-8 ({e}): {:?}",
                    String::from_utf8_lossy(l)
                )
            });
            assert!(!s.contains(FFFD), "{label}: page {i}: U+FFFD in line {s:?}");
        }
    }
}

// monospace: a line holds at most CHARS_PER_LINE columns, one column per scalar
pub fn check_mono_width(pages: &[Page], label: &str) {
    for (i, p) in pages.iter().enumerate() {
        for l in &p.lines {
            let cols = String::from_utf8_lossy(l).chars().count();
            assert!(
                cols <= CHARS_PER_LINE,
                "{label}: page {i}: line of {cols} columns > {CHARS_PER_LINE}: {:?}",
                String::from_utf8_lossy(l)
            );
        }
    }
}

// Position inside a scalar of byte `b` of valid `data`: Some((scalar length, index in
// scalar)) when `b` is a continuation byte, None when it starts a scalar.
pub fn inner_class(data: &[u8], b: usize) -> Option<(usize, usize)> {
    if b >= data.len() || !is_continuation(data[b]) {
        return None;
    }
    let mut s = b;
    while is_continuation(data[s]) {
        s -= 1;
    }
    let len = match data[s] {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    };
    Some((len, b - s))
}

// every (scalar length, index) with index >= 1: where a boundary can cut a scalar
pub const INNER_CLASSES: [(usize, usize); 6] = [(2, 1), (3, 1), (3, 2), (4, 1), (4, 2), (4, 3)];

// the byte positions at which the reader's reads of the book began or ended
pub fn read_boundaries(r: &Rig) -> Vec<usize> {
    let mut v = Vec::new();
    for x in r.storage().read_log() {
        if x.path == BOOK {
            v.push(x.offset as usize);
            v.push(x.offset as usize + x.returned);
        }
    }
    v
}

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

// The ink of line `i` of the drawn page against the line's expected text: the last
// visible scalar's glyph has ink at or right of the x its advance puts it at, and nothing
// is inked beyond the line's total advance. Supported native glyphs and U+FFFD
// keep the existing table; unsupported valid scalars use the optional no-pack
// missing box advance at the literal body pixel size.
pub fn line_ink(
    r: &Rig,
    frame: &Framebuffer,
    font: u8,
    i: usize,
    text: &str,
) -> Result<(), String> {
    let fs = FontSet::for_size(font);
    let body_px = [16u32, 19, 23, 28, 35][font as usize];
    let advance = |c| {
        if c == FFFD || fs.font(Style::Regular).has_glyph(c) {
            fs.advance(c, Style::Regular) as u32
        } else {
            body_px
        }
    };
    let (y0, h, margin) = (
        r.text_y() as u32,
        r.font_line_h() as u32,
        r.text_margin() as u32,
    );
    let Some((last_idx, last)) = text.char_indices().rev().find(|(_, c)| !c.is_whitespace()) else {
        return Ok(());
    };
    let origin = margin + text[..last_idx].chars().map(advance).sum::<u32>();
    let total = origin + advance(last);
    // the rows of line i, below the few rows under the header that carry other ink
    let (top, bottom) = (y0 + 3 + i as u32 * h, y0 + 3 + (i as u32 + 1) * h);
    let mut max_x: Option<u32> = None;
    for y in top..bottom {
        for x in 0..WIDTH {
            if frame.is_black(x, y as u16) {
                max_x = Some(max_x.map_or(x as u32, |m| m.max(x as u32)));
            }
        }
    }
    let Some(max_x) = max_x else {
        return Err(format!("line {i} {text:?} drew nothing"));
    };
    if max_x < origin {
        return Err(format!(
            "line {i} {text:?}: last scalar {last:?} (x {origin}..{total}) is not drawn, rightmost ink at x {max_x}"
        ));
    }
    if max_x > total + 2 {
        return Err(format!(
            "line {i} {text:?}: ink at x {max_x} beyond the line's advance {total}"
        ));
    }
    Ok(())
}

// Draw the current page with the real ReaderApp::draw. Every laid-out line must show all
// its scalars (`from_utf8_lossy` of the reader's line is the expected text).
pub fn check_drawn(r: &Rig, font: u8, label: &str) {
    let frame = render_full(&|s| r.draw(s)).frame;
    for (i, line) in r.lines().iter().enumerate() {
        if let Err(e) = line_ink(r, &frame, font, i, &String::from_utf8_lossy(line)) {
            panic!("{label}: {e}");
        }
    }
}

// Draw the current page and compare line `i` with the text it must show.
pub fn drawn_line_matches(r: &Rig, font: u8, i: usize, text: &str) -> Result<(), String> {
    let frame = render_full(&|s| r.draw(s)).frame;
    line_ink(r, &frame, font, i, text)
}

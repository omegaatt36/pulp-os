// page glyph preparation (R10, R11): layout -> prepare -> draw
//
// oracle: every expected read count, byte offset, budget boundary and
// pixel below is derived by hand from the planted fixture (common::Pack,
// written from docs/font-pack.txt §2/§4/§5) and the Deg270 rotation rule
//   logical (lx, ly) -> physical (px, py) = (ly, 479 - lx)
// nothing is copied from running the implementation
//
// fixture pack (pixel size 16, line_height 18, ascent 16, §2 bitmaps
// back to back in index order, bitmap_off = 512 + 6 * 16 = 608):
//   cp       glyph        w x h   §5 len  section off  advance
//   U+0020   ' '          0 x 0   0       0            5
//   U+0041   'A'          7 x 9   9       0            8
//   U+0042   'B'          9 x 4   2*4=8   9            10
//   U+0078   'x'          5 x 3   3       17           6
//   U+25A1   '□' fallback 3 x 3   3       20           4
//   U+4E2D   '中'         15 x 16 2*16=32 23           16
// 6 records = 96 B: one index sector, so every find() of a code point
// >= U+0020 is exactly one read_at(512, 96); each non-empty bitmap is
// one read_at(608 + off, len)

mod common;

use std::cell::Cell;

use common::strip::{Frame, render_full, render_partial};
use common::*;
use pulp_render::font_pack::{FontPack, IoError, LookupError};
use pulp_render::layout::{LineSpan, Markup, Measure, Style, WrapParams, wrap};
use pulp_render::page::{DrawError, PackMeasure, PageGeometry, PageGlyphs, PrepareError};
use pulp_render::panel::Rotation;
use pulp_render::strip::StripBuffer;

const M: u8 = 0x01;
const MARKUP: Markup = Markup {
    marker: M,
    img_ref: b'P',
    bold_on: b'B',
    bold_off: b'b',
    italic_on: b'I',
    italic_off: b'i',
    heading_on: b'H',
    heading_off: b'h',
    quote_on: b'Q',
    quote_off: b'q',
};

const BITMAP_OFF: u32 = 608;
const INDEX_READ: (u32, usize) = (512, 96);

// planted tiny glyph, stride 1, MSB = leftmost column
//   row 0: x=0        #....
//   row 1: x=1,2      .##..
//   row 2: x=4        ....#
fn x_glyph() -> Glyph {
    Glyph {
        cp: 0x78,
        width: 5,
        height: 3,
        advance: 6,
        offset_x: 1,
        offset_y: -3,
        bitmap: vec![0b1000_0000, 0b0110_0000, 0b0000_1000],
    }
}

fn box_glyph() -> Glyph {
    Glyph {
        cp: 0x25A1,
        width: 3,
        height: 3,
        advance: 4,
        offset_x: 0,
        offset_y: -3,
        bitmap: vec![0xE0, 0xA0, 0xE0],
    }
}

fn glyphs() -> Vec<Glyph> {
    vec![
        Glyph {
            advance: 5,
            ..sized(0x20, 0, 0, 0)
        },
        sized(0x41, 1, 7, 9),
        Glyph {
            advance: 10,
            ..sized(0x42, 3, 9, 4)
        },
        x_glyph(),
        box_glyph(),
        sized(0x4E2D, 2, 15, 16),
    ]
}

fn fixture() -> (TestReader, FontPack) {
    let built = Pack::new(16, 0x25A1, glyphs()).build();
    assert_eq!(built.bitmap_off, BITMAP_OFF, "fixture precondition");
    assert_eq!(built.offsets, [0, 0, 9, 17, 20, 23], "fixture precondition");
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 16).unwrap();
    r.reset_counts();
    (r, pack)
}

fn params(max_lines: usize) -> WrapParams<'static> {
    WrapParams {
        markup: MARKUP,
        max_lines,
        max_width_px: 1000,
        indent_px: 10,
        img_heights: &[],
        default_img_h: 36,
    }
}

// layout with the pack itself (PackMeasure), as the firmware sequences it
fn layout(pack: &FontPack, r: &mut TestReader, text: &[u8]) -> Vec<LineSpan> {
    let mut lines = vec![LineSpan::EMPTY; 8];
    let out = wrap(
        text,
        true,
        &mut PackMeasure::new(pack, r),
        &params(8),
        &mut lines,
    )
    .unwrap();
    assert_eq!(out.consumed, text.len(), "whole fixture fits one page");
    lines.truncate(out.line_count);
    lines
}

// left 10, top 20, pitch = pack line_height 18, baseline = top + ascent 16
const GEOM: PageGeometry = PageGeometry {
    left: 10,
    top: 20,
    line_height: 18,
    ascent: 16,
    indent_px: 10,
};

// logical pixels of glyph g drawn with its pen at (cx, baseline):
// bitmap bit (x, y) set -> (cx + offset_x + x, baseline + offset_y + y)
fn glyph_pixels(g: &Glyph, cx: i32, baseline: i32) -> Vec<(i32, i32)> {
    let stride = (g.width as usize).div_ceil(8);
    let mut out = Vec::new();
    for y in 0..g.height as usize {
        for x in 0..g.width as usize {
            if g.bitmap[y * stride + x / 8] & (0x80 >> (x % 8)) != 0 {
                out.push((
                    cx + g.offset_x as i32 + x as i32,
                    baseline + g.offset_y as i32 + y as i32,
                ));
            }
        }
    }
    out
}

// Deg270: logical (lx, ly) -> physical (ly, 479 - lx), sorted row-major
// like Frame::black_pixels
fn to_physical(logical: &[(i32, i32)]) -> Vec<(u16, u16)> {
    let mut p: Vec<(u16, u16)> = logical
        .iter()
        .map(|&(lx, ly)| (ly as u16, (479 - lx) as u16))
        .collect();
    p.sort_by_key(|&(x, y)| (y, x));
    p.dedup();
    p
}

// R10 fixture: two lines; markers, soft hyphen, CR/LF are not drawn,
// NBSP is laid out (and so drawn) as ' ', 가 and 나 are absent -> □
//   line 0: A 中 A ' ' 가 SHY B [bold] B B [/bold]   then CR LF
//   line 1: 中 나 x NBSP A
// distinct drawn scalars: A 中 ' ' 가 B x 나 = 7 -> 7 index reads
// distinct resolved bitmaps (non-empty): A 9, 中 32, B 8, x 3, □ 3 (for
// both 가 and 나; ' ' is 0 B, never read) = 5 bitmap reads, 55 B
const R10_TEXT: &str = "A中A 가\u{AD}B\u{1}BB\u{1}b\r\n中나x\u{A0}A";

type Big = PageGlyphs<16, 256>;

#[test]
fn prepare_reads_each_distinct_glyph_once() {
    let (mut r, pack) = fixture();
    let lines = layout(&pack, &mut r, R10_TEXT.as_bytes());
    assert_eq!(lines.len(), 2, "one line per paragraph at width 1000");
    r.reset_counts();

    let mut cache = Big::new();
    cache
        .prepare(&pack, &mut r, R10_TEXT.as_bytes(), &lines, MARKUP)
        .unwrap();

    assert_eq!(cache.glyph_count(), 7);
    assert_eq!(cache.bytes_used(), 55);
    assert_eq!(r.reads, 7 + 5, "7 index-sector reads + 5 bitmap reads");
    let index_reads = r.log.iter().filter(|&&e| e == INDEX_READ).count();
    assert_eq!(index_reads, 7);
    let mut bitmap_reads: Vec<(u32, usize)> = r
        .log
        .iter()
        .copied()
        .filter(|&(off, _)| off >= BITMAP_OFF)
        .collect();
    bitmap_reads.sort();
    // A @0 9 B, B @9 8 B, x @17 3 B, □ @20 3 B, 中 @23 32 B
    assert_eq!(
        bitmap_reads,
        [
            (BITMAP_OFF, 9),
            (BITMAP_OFF + 9, 8),
            (BITMAP_OFF + 17, 3),
            (BITMAP_OFF + 20, 3),
            (BITMAP_OFF + 23, 32),
        ]
    );
}

#[test]
fn draw_performs_no_font_reads() {
    let (mut r, pack) = fixture();
    let text = R10_TEXT.as_bytes();
    let lines = layout(&pack, &mut r, text);
    let mut cache = Big::new();
    cache.prepare(&pack, &mut r, text, &lines, MARKUP).unwrap();

    let reads = r.reads;
    let bytes = r.bytes_read;
    let log = r.log.clone();
    let calls = Cell::new(0usize);
    let draw = |s: &mut StripBuffer| {
        calls.set(calls.get() + 1);
        cache.draw(s, text, &lines, MARKUP, &GEOM).unwrap();
    };

    let full = render_full(Rotation::Deg270, &draw);
    // 480 physical rows / 40 per strip
    assert_eq!(calls.get(), 12);
    assert!(!full.black_pixels().is_empty(), "the page drew something");

    // logical window x 0..100, y 0..60 covers both lines' glyphs
    render_partial(&Frame::blank(), Rotation::Deg270, 0, 0, 100, 60, &draw)
        .expect("non-empty window");
    assert!(calls.get() > 12, "partial pass drew at least once");

    assert_eq!(r.reads, reads);
    assert_eq!(r.bytes_read, bytes);
    assert_eq!(r.log, log);
}

// line 0 "x" at pen (10, 20 + 16): gx = 10 + 1 = 11, gy = 36 - 3 = 33
//   logical (11,33) (12,34) (13,34) (15,35)
//   -> physical (33,468) (34,467) (34,466) (35,464)
// line 1 "[quote]x", indent 1: pen (10 + 10, 20 + 18 + 16) = (20, 54)
//   gx 21, gy 51: logical (21,51) (22,52) (23,52) (25,53)
//   -> physical (51,458) (52,457) (52,456) (53,454)
#[test]
fn planted_glyph_lands_on_hand_derived_pixels() {
    let (mut r, pack) = fixture();
    let text = "x\n\u{1}Qx".as_bytes();
    let lines = layout(&pack, &mut r, text);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].indent, 1);
    let mut cache = Big::new();
    cache.prepare(&pack, &mut r, text, &lines, MARKUP).unwrap();

    let frame = render_full(Rotation::Deg270, &|s: &mut StripBuffer| {
        cache.draw(s, text, &lines, MARKUP, &GEOM).unwrap()
    });
    let mut want = vec![
        (33, 468),
        (34, 467),
        (34, 466),
        (35, 464),
        (51, 458),
        (52, 457),
        (52, 456),
        (53, 454),
    ];
    want.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(frame.black_pixels(), want);
}

// every cached bitmap is the planted one: "xA中 가" on one line, pens
// from the planted advances: x @10, A @16, 中 @24, ' ' @40, 가 (□) @45;
// baseline 36. expected pixels come from the fixture bitmaps directly
#[test]
fn cached_bitmaps_draw_the_planted_glyphs() {
    let (mut r, pack) = fixture();
    let text = "xA中 가".as_bytes();
    let lines = layout(&pack, &mut r, text);
    let mut cache = Big::new();
    cache.prepare(&pack, &mut r, text, &lines, MARKUP).unwrap();

    let frame = render_full(Rotation::Deg270, &|s: &mut StripBuffer| {
        cache.draw(s, text, &lines, MARKUP, &GEOM).unwrap()
    });
    let g = glyphs();
    let mut logical = Vec::new();
    logical.extend(glyph_pixels(&g[3], 10, 36));
    logical.extend(glyph_pixels(&g[1], 16, 36));
    logical.extend(glyph_pixels(&g[5], 24, 36));
    logical.extend(glyph_pixels(&g[4], 45, 36));
    assert_eq!(frame.black_pixels(), to_physical(&logical));
}

// PackMeasure: layout sees the resolved metrics (R4): present glyphs
// their own advance, absent ones the fallback's, one face for all styles
#[test]
fn pack_measure_uses_resolved_advances() {
    let (mut r, pack) = fixture();
    let mut m = PackMeasure::new(&pack, &mut r);
    assert_eq!(m.advance('A', Style::Regular), Ok(8));
    assert_eq!(m.advance('A', Style::Bold), Ok(8));
    assert_eq!(m.advance('中', Style::Heading), Ok(16));
    assert_eq!(m.advance('가', Style::Regular), Ok(4));
    assert_eq!(m.line_height(Style::Regular), 18);
}

fn prepare_text<const N: usize, const B: usize>(
    cache: &mut PageGlyphs<N, B>,
    text: &str,
) -> Result<(), PrepareError> {
    let (mut r, pack) = fixture();
    let lines = layout(&pack, &mut r, text.as_bytes());
    cache.prepare(&pack, &mut r, text.as_bytes(), &lines, MARKUP)
}

fn assert_not_drawable<const N: usize, const B: usize>(cache: &PageGlyphs<N, B>, text: &str) {
    let lines = [LineSpan {
        start: 0,
        len: text.len() as u16,
        flags: 0,
        indent: 0,
    }];
    let mut strip = StripBuffer::new();
    assert!(!cache.is_ready());
    assert_eq!(cache.glyph_count(), 0);
    assert_eq!(cache.bytes_used(), 0);
    assert_eq!(
        cache.draw(&mut strip, text.as_bytes(), &lines, MARKUP, &GEOM),
        Err(DrawError::NotPrepared)
    );
}

// glyph budget 3: "AB中" is exactly 3 distinct scalars; "AB中x" is 4
#[test]
fn glyph_budget_boundary() {
    let mut cache = PageGlyphs::<3, 256>::new();
    assert_eq!(prepare_text(&mut cache, "AB中BA中"), Ok(()));
    assert!(cache.is_ready());
    assert_eq!(cache.glyph_count(), 3);

    let mut cache = PageGlyphs::<3, 256>::new();
    assert_eq!(
        prepare_text(&mut cache, "AB中x"),
        Err(PrepareError::TooManyGlyphs { limit: 3 })
    );
    assert_not_drawable(&cache, "AB中x");
}

// byte budget: "A中" needs 9 + 32 = 41 B
#[test]
fn byte_budget_boundary() {
    let mut cache = PageGlyphs::<8, 41>::new();
    assert_eq!(prepare_text(&mut cache, "A中A中"), Ok(()));
    assert_eq!(cache.bytes_used(), 41);

    let mut cache = PageGlyphs::<8, 40>::new();
    assert_eq!(
        prepare_text(&mut cache, "A中"),
        Err(PrepareError::TooManyBytes { limit: 40 })
    );
    assert_not_drawable(&cache, "A中");
}

// bytes are budgeted per resolved bitmap: 가, 나 and □ itself all draw
// the one 3-byte fallback bitmap, and ' ' costs nothing, so 3 B suffice;
// each distinct scalar still takes a glyph slot (4 here)
#[test]
fn byte_budget_counts_shared_fallback_once() {
    let mut cache = PageGlyphs::<4, 3>::new();
    assert_eq!(prepare_text(&mut cache, "가 나\u{25A1}"), Ok(()));
    assert_eq!(cache.glyph_count(), 4);
    assert_eq!(cache.bytes_used(), 3);
}

// a failed prepare also discards the previous page: never draw stale text
#[test]
fn failed_prepare_discards_the_previous_page() {
    let mut cache = PageGlyphs::<3, 256>::new();
    assert_eq!(prepare_text(&mut cache, "A"), Ok(()));
    assert!(cache.is_ready());
    assert_eq!(
        prepare_text(&mut cache, "ABx中"),
        Err(PrepareError::TooManyGlyphs { limit: 3 })
    );
    assert_not_drawable(&cache, "A");
}

#[test]
fn read_failure_is_a_prepare_error() {
    let (mut r, pack) = fixture();
    let text = "AB".as_bytes();
    let lines = layout(&pack, &mut r, text);
    r.fail_read = Some(IoError::Io);
    let mut cache = Big::new();
    assert_eq!(
        cache.prepare(&pack, &mut r, text, &lines, MARKUP),
        Err(PrepareError::Lookup(LookupError::Io))
    );
    assert_not_drawable(&cache, "AB");
}

// drawing text the cache was not prepared for is refused, not guessed
#[test]
fn draw_refuses_unprepared_scalars() {
    let mut cache = Big::new();
    assert_eq!(prepare_text(&mut cache, "A"), Ok(()));
    let lines = [LineSpan {
        start: 0,
        len: 2,
        flags: 0,
        indent: 0,
    }];
    let mut strip = StripBuffer::new();
    assert_eq!(
        cache.draw(&mut strip, b"AB", &lines, MARKUP, &GEOM),
        Err(DrawError::NotInCache('B'))
    );
}

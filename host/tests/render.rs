// onepage-host-validation / render regression -- production strip + glyph rendering into a software
// framebuffer, portrait artifacts, full-frame vs stitched-strip equality.
// (shared production logic shared production algorithm, firmware builds firmware stays buildable, portrait artifacts portrait artifact,
// strip equality strip equality)
//
// Run: scripts/host-test.sh --test render
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// Every pixel in this file comes out of the firmware's own drawing path:
//   * `StripBuffer` (kernel/src/drivers/strip.rs over board-logic's StripCore):
//     begin_strip / begin_window / logical_window / strip_count /
//     max_rows_for_width and its embedded-graphics `DrawTarget<Color = BinaryColor>`
//     impl (BinaryColor::On == black ink, Off == white);
//   * `FontSet::{draw_char, draw_str, draw_bytes}` (src/fonts: glyph blit);
//   * the real `ReaderApp::draw(&self, &mut StripBuffer)` through `Rig`.
// No rasterising / glyph blitting / paging is reimplemented in pulp-host or in
// this file. The tests only compute EXPECTED pixels, independently: from
// geometry (rectangles, single pixels) and from the public glyph tables
// (`BitmapFont::resolve` -> `BitmapGlyph` metrics + packed MSB-first,
// row-major, `ceil(w/8)` bytes-per-row bitmap, bit 1 = ink).
//
// host/src/lib.rs must expose the new PUBLIC module `pulp_host::render`:
//
//   pub const WIDTH: u16 = 480;      // portrait logical width
//   pub const HEIGHT: u16 = 800;     // portrait logical height
//
//   pub struct Framebuffer;          // the 480x800 portrait page, monochrome
//   impl Framebuffer {
//       // pixel (x, y) in PORTRAIT coordinates: origin top-left, x to the right,
//       // y downward, 0 <= x < 480, 0 <= y < 800. true == black. Panics when out
//       // of range.
//       pub fn is_black(&self, x: u16, y: u16) -> bool;
//       // number of black pixels
//       pub fn black_count(&self) -> usize;
//       // the PBM artifact (see below) as bytes; pure function of the pixels
//       pub fn to_pbm(&self) -> Vec<u8>;
//       // writes exactly `to_pbm()` to `path` (creates / truncates the file;
//       // the caller owns the directory)
//       pub fn write_pbm(&self, path: &std::path::Path) -> std::io::Result<()>;
//   }
//
//   // one draw pass == one window of the production StripBuffer: the region the
//   // closure observed through `strip.logical_window()` for that pass
//   // (logical / portrait coordinates)
//   #[derive(Clone, Copy, PartialEq, Eq, Debug)]
//   pub struct Pass { pub x: u16, pub y: u16, pub w: u16, pub h: u16 }
//
//   pub struct Render { pub frame: Framebuffer, pub passes: Vec<Pass> }
//
//   // Firmware full-refresh path: for idx in 0..StripBuffer::strip_count() {
//   //   strip.begin_strip(Rotation::Deg270, idx); clear it to white (what the
//   //   firmware does before drawing a strip); draw(&mut strip); copy the
//   //   strip's rows into the framebuffer }, `draw` invoked exactly once per
//   //   strip, in order. The page is portrait (Deg270). `passes` has one entry
//   //   per strip (in the order drawn).
//   pub fn render_stitched(draw: &dyn Fn(&mut StripBuffer)) -> Render;
//
//   // Host "full-frame" reference (the firmware has no such path: one 4 KB
//   // strip can never hold the 48 KB page). Defined as: the whole 480x800
//   // portrait page rendered through the SAME production StripBuffer, but tiled
//   // with `begin_window` windows of a DIFFERENT shape than the firmware strips
//   // (e.g. full-logical-width bands of `max_rows_for_width` rows), each tile
//   // cleared to white, `draw` invoked once per tile, tiles copied into the
//   // framebuffer. It is NOT allowed to rasterise on its own, and it must not
//   // be the same partition as `render_stitched` (tests assert that).
//   pub fn render_full(draw: &dyn Fn(&mut StripBuffer)) -> Render;
//
//   (`StripBuffer` is the existing `pulp_host::drivers::strip::StripBuffer`.)
//
// host/src/reader.rs: two more public methods on the existing `Rig`:
//   Rig::draw(&self, strip: &mut StripBuffer)
//        forwards to the real `App::draw` of the ReaderApp (the current page /
//        TOC / error screen). Pure: must not change the Rig's state.
//   Rig::text_y(&self) -> u16
//        the reader's current top of the text area in px (production
//        `ReaderApp::text_y` = TEXT_Y + theme vertical margin).
//
// PBM artifact (Framebuffer::to_pbm / write_pbm), binary PBM "P4":
//   * bytes 0.. : the ASCII header  b"P4\n480 800\n"  (exactly, 11 bytes)
//   * then 800 rows * 60 bytes = 48000 bytes of raster, row-major, top row
//     first. Within a byte the MSB is the LEFTMOST pixel. 480 is a multiple of
//     8, so there are no row-padding bits. Bit 1 == BLACK, bit 0 == WHITE (PBM
//     convention). NOTE the firmware's strip bytes use the opposite sense
//     (1 == white, 0 == black): the artifact writer must invert; a blank page
//     is therefore 48000 bytes of 0x00.
//   * deterministic: the same pixels always produce the same bytes (no
//     timestamp / comment line / metadata).
//
// Text band used by the reader tests (all derived from public data):
//   x in [0, 480), y in [text_y, text_y + text_area_h)
//   text_y        == 24 + reading_theme(theme).margin_v   (TEXT_Y = HEADER_Y 6 +
//                    HEADER_H 16 + 2; kernel::config::READING_THEMES)
//   text_margin   == reading_theme(theme).margin_h
//   text_w        == 480 - 2 * text_margin
//   text_y + text_area_h == 796        (SCREEN_H 800 - 4 bottom pad)
// Rows above the band are chrome (title / status); rows below are the progress
// strip. The reader tests do not constrain chrome pixels.
// ============================================================================

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

use pulp_host::drivers::strip::{STRIP_ROWS, StripBuffer};
use pulp_host::fonts::bitmap::BitmapFont;
use pulp_host::fonts::{FontSet, Style, chrome_font};
use pulp_host::kernel::config::reading_theme;
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{Framebuffer, HEIGHT, Pass, Render, WIDTH, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;
use pulp_host::utf8::Utf8Iter;

const W: i32 = 480;
const H: i32 = 800;
const PBM_HEADER: &[u8] = b"P4\n480 800\n";
const BOOK: &str = "BOOK.TXT";
const NUM_FONTS: u8 = 5;
const NUM_THEMES: u8 = 4;
const STYLES: [Style; 4] = [Style::Regular, Style::Bold, Style::Italic, Style::Heading];

type Cells = HashSet<(i32, i32)>;
type Renderer = fn(&dyn Fn(&mut StripBuffer)) -> Render;
const RENDERERS: [(&str, Renderer); 2] = [("stitched", render_stitched), ("full", render_full)];

// ---------------------------------------------------------------------------
// drawing + expectation helpers (expected pixels are computed from geometry /
// public glyph tables only)
// ---------------------------------------------------------------------------

fn fill_rect(s: &mut StripBuffer, x: i32, y: i32, w: u32, h: u32) {
    let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(s);
}

fn put_pixel(s: &mut StripBuffer, x: i32, y: i32) {
    let _ = Pixel(Point::new(x, y), BinaryColor::On).draw(s);
}

fn rect_cells(out: &mut Cells, x: i32, y: i32, w: i32, h: i32) {
    for yy in y.max(0)..(y + h).min(H) {
        for xx in x.max(0)..(x + w).min(W) {
            out.insert((xx, yy));
        }
    }
}

fn pixel_cell(out: &mut Cells, x: i32, y: i32) {
    if (0..W).contains(&x) && (0..H).contains(&y) {
        out.insert((x, y));
    }
}

// cells inked by one glyph, from the public tables: left = cx + offset_x,
// top = baseline + offset_y, bitmap row-major MSB-first, ceil(w/8) bytes per row.
// Not clipped. Returns the advance.
fn glyph_cells(font: &BitmapFont, ch: char, cx: i32, baseline: i32, out: &mut Cells) -> u8 {
    let r = font.resolve(ch);
    let g = r.glyph;
    let (w, h) = (g.width as usize, g.height as usize);
    let row_bytes = w.div_ceil(8);
    for row in 0..h {
        for col in 0..w {
            let byte = r.bitmaps[g.bitmap_offset as usize + row * row_bytes + col / 8];
            if (byte >> (7 - col % 8)) & 1 == 1 {
                out.insert((cx + g.offset_x as i32 + col as i32, baseline + g.offset_y as i32 + row as i32));
            }
        }
    }
    g.advance
}

fn clip(cells: &mut Cells) {
    cells.retain(|&(x, y)| (0..W).contains(&x) && (0..H).contains(&y));
}

fn frame_cells(fb: &Framebuffer) -> Cells {
    let mut c = Cells::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if fb.is_black(x, y) {
                c.insert((x as i32, y as i32));
            }
        }
    }
    c
}

fn sorted(it: impl Iterator<Item = (i32, i32)>) -> Vec<(i32, i32)> {
    let mut v: Vec<_> = it.collect();
    v.sort_unstable();
    v
}

fn assert_cells(name: &str, fb: &Framebuffer, want: &Cells) {
    let got = frame_cells(fb);
    if got == *want {
        return;
    }
    let extra = sorted(got.difference(want).copied());
    let missing = sorted(want.difference(&got).copied());
    panic!(
        "{name}: frame differs from the expected pixels: {} unexpected black (first {:?}), {} missing black (first {:?}); got {} black, want {}",
        extra.len(),
        &extra[..extra.len().min(8)],
        missing.len(),
        &missing[..missing.len().min(8)],
        got.len(),
        want.len()
    );
}

fn assert_same(name: &str, a: &Framebuffer, b: &Framebuffer) {
    if a.to_pbm() == b.to_pbm() {
        return;
    }
    let (ca, cb) = (frame_cells(a), frame_cells(b));
    let only_a = sorted(ca.difference(&cb).copied());
    let only_b = sorted(cb.difference(&ca).copied());
    panic!(
        "{name}: frames differ: {} pixels only in the first (first {:?}), {} only in the second (first {:?})",
        only_a.len(),
        &only_a[..only_a.len().min(8)],
        only_b.len(),
        &only_b[..only_b.len().min(8)]
    );
}

// independent PBM encoder straight from the format definition
fn pbm_of(cells: &Cells) -> Vec<u8> {
    let mut out = PBM_HEADER.to_vec();
    let mut data = vec![0u8; 800 * 60];
    for &(x, y) in cells {
        data[y as usize * 60 + x as usize / 8] |= 0x80 >> (x % 8);
    }
    out.extend_from_slice(&data);
    out
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pulp-host-render-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

// internal pass edges (x and y) of a set of passes
fn internal_edges(passes: &[Pass]) -> (Vec<i32>, Vec<i32>) {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for p in passes {
        for e in [p.x as i32, p.x as i32 + p.w as i32] {
            if 0 < e && e < W {
                xs.push(e);
            }
        }
        for e in [p.y as i32, p.y as i32 + p.h as i32] {
            if 0 < e && e < H {
                ys.push(e);
            }
        }
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    (xs, ys)
}

fn assert_exact_tiling(name: &str, passes: &[Pass]) {
    let mut cover = vec![0u8; (W * H) as usize];
    for p in passes {
        assert!(p.w > 0 && p.h > 0, "{name}: empty pass {p:?}");
        assert!(
            p.x as i32 + p.w as i32 <= W && p.y as i32 + p.h as i32 <= H,
            "{name}: pass {p:?} leaves the 480x800 page"
        );
        for y in p.y as i32..p.y as i32 + p.h as i32 {
            for x in p.x as i32..p.x as i32 + p.w as i32 {
                cover[(y * W + x) as usize] += 1;
            }
        }
    }
    let bad = cover.iter().filter(|&&c| c != 1).count();
    assert_eq!(bad, 0, "{name}: {bad} page pixels are not covered by exactly one pass");
}

// a window of the strip buffer must fit its capacity: physical rows (the
// logical thin side under Deg270) <= max_rows_for_width(physical width)
fn assert_within_capacity(name: &str, passes: &[Pass]) {
    for p in passes {
        let fits = p.w <= StripBuffer::max_rows_for_width(p.h) || p.h <= StripBuffer::max_rows_for_width(p.w);
        assert!(fits, "{name}: pass {p:?} exceeds max_rows_for_width ({} / {})", StripBuffer::max_rows_for_width(p.h), StripBuffer::max_rows_for_width(p.w));
    }
}

// ---------------------------------------------------------------------------
// reader fixtures (deterministic prose, English + a few Latin-1 / punctuation)
// ---------------------------------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }
}

const WORDS: &[&str] = &[
    "the", "lighthouse", "keeper", "climbed", "spiral", "stairs", "each", "evening", "harbour", "below", "turned", "colour", "of", "old",
    "pewter", "she", "read", "letter", "twice", "folded", "along", "its", "creases", "where", "no", "one", "would", "look", "market",
    "smelled", "tar", "oranges", "rope", "happy", "rain", "drummed", "tin", "roof", "hour", "softened", "whisper", "stopped", "patience",
    "caf\u{e9}", "na\u{ef}ve", "r\u{e9}sum\u{e9}", "it\u{2019}s", "\u{2014}", "\u{201c}quoted\u{201d}", "Wolf", "Hymn", "gypsy", "quiz",
];

fn prose(seed: u64, bytes: usize) -> Vec<u8> {
    let mut rng = Lcg(seed);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut size = 0usize;
    while size < bytes {
        let n = 3 + rng.below(70);
        let p: Vec<&str> = (0..n).map(|_| WORDS[rng.below(WORDS.len())]).collect();
        let p = p.join(" ");
        size += p.len() + 2;
        paragraphs.push(p);
    }
    paragraphs.join("\n\n").into_bytes()
}

fn rig_for(data: &[u8], font: u8, theme: u8) -> Rig {
    let card = VirtualStorage::memory_with(&[(BOOK, data)]);
    card.ensure_pulp_dir().expect("card has _PULP/ the way the firmware boots it");
    let mut r = Rig::new(card);
    r.configure(font, theme);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready, "book font {font} theme {theme} must open to a page");
    r
}

fn goto_page(r: &mut Rig, n: usize) {
    while r.page() < n {
        let before = r.page();
        r.press(Action::Next);
        assert_eq!(r.page(), before + 1, "Next must advance one page (fixture long enough)");
    }
}

fn goto_last(r: &mut Rig) {
    for _ in 0..4000 {
        let p = r.page();
        r.press(Action::Next);
        if r.page() == p {
            return;
        }
    }
    panic!("never reached the last page");
}

fn shot(r: &Rig) -> Render {
    render_stitched(&|s| r.draw(s))
}

// The chrome (title / status) is drawn with `chrome_font()` in a region that starts at
// HEADER_Y = 6 and is only 16 px tall, but the chrome font's line is taller, so its
// glyph ink may spill below the region: nothing of the chrome can reach below
// HEADER_Y + chrome_font().line_height. Rows at / below that are page text only.
fn chrome_bottom() -> i32 {
    6 + chrome_font().line_height as i32
}

// the whole text band: [text_y, text_y + text_area_h)
fn text_band(r: &Rig) -> (i32, i32) {
    let top = r.text_y() as i32;
    (top, top + r.text_area_h() as i32)
}

// the part of the band no chrome ink can reach
fn strict_band(r: &Rig) -> (i32, i32) {
    let (top, bot) = text_band(r);
    (top.max(chrome_bottom()), bot)
}

fn below(cells: &Cells, top: i32) -> Cells {
    cells.iter().copied().filter(|&(_, y)| y >= top).collect()
}

// expected ink of lines [from, to) of the CURRENT page with line 0's baseline at b0
// and the line pitch == font_line_h, pen starting at text_margin, restricted to
// the text band
fn page_text_cells(r: &Rig, font: u8, b0: i32, from: usize, to: usize) -> Cells {
    let f = FontSet::for_size(font).font(Style::Regular);
    let lines = r.lines();
    let mut cells = Cells::new();
    for (i, line) in lines.iter().enumerate().take(to).skip(from) {
        let mut pen = r.text_margin() as i32;
        let base = b0 + i as i32 * r.font_line_h() as i32;
        for ch in Utf8Iter::new(line) {
            pen += glyph_cells(f, ch, pen, base, &mut cells) as i32;
        }
    }
    let (top, bot) = text_band(r);
    cells.retain(|&(x, y)| (0..W).contains(&x) && y >= top && y < bot);
    cells
}

// black pixels of the whole text band (chrome spill possible in its first rows)
fn band_cells_loose(r: &Rig, fb: &Framebuffer) -> Cells {
    let (top, bot) = text_band(r);
    let mut c = frame_cells(fb);
    c.retain(|&(_, y)| y >= top && y < bot);
    c
}

// black pixels of the chrome-free part of the band
fn band_cells(r: &Rig, fb: &Framebuffer) -> Cells {
    let (top, bot) = strict_band(r);
    let mut c = frame_cells(fb);
    c.retain(|&(_, y)| y >= top && y < bot);
    c
}

// ===========================================================================
// portrait artifacts -- portrait artifact: size, header, bit sense, determinism
// ===========================================================================

#[test]
fn blank_page_is_white_for_both_renderers() {
    for (name, render) in RENDERERS {
        let r = render(&|_| {});
        assert_eq!(r.frame.black_count(), 0, "{name}: nothing drawn, so no black pixel");
        assert!(!r.frame.is_black(0, 0) && !r.frame.is_black(479, 799) && !r.frame.is_black(240, 400));
    }
}

#[test]
fn framebuffer_constants_are_the_portrait_page() {
    assert_eq!((WIDTH, HEIGHT), (480, 800));
}

#[test]
#[should_panic]
fn is_black_rejects_x_out_of_range() {
    let r = render_stitched(&|_| {});
    let _ = r.frame.is_black(480, 0);
}

#[test]
#[should_panic]
fn is_black_rejects_y_out_of_range() {
    let r = render_stitched(&|_| {});
    let _ = r.frame.is_black(0, 800);
}

#[test]
fn pbm_file_has_header_and_exact_size() {
    let dir = tmp_dir("pbm-size");
    let path = dir.join("blank.pbm");
    render_stitched(&|_| {}).frame.write_pbm(&path).expect("write_pbm");
    let bytes = std::fs::read(&path).expect("artifact exists");
    assert_eq!(PBM_HEADER.len(), 11);
    assert_eq!(&bytes[..PBM_HEADER.len()], PBM_HEADER, "P4 header with fixed 480 800");
    assert_eq!(bytes.len(), PBM_HEADER.len() + 800 * 60, "raster is 800 rows x ceil(480/8) bytes");
    assert!(bytes[PBM_HEADER.len()..].iter().all(|&b| b == 0), "blank page: every raster byte 0x00 (0 = white)");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pbm_file_is_exactly_to_pbm() {
    let dir = tmp_dir("pbm-bytes");
    let path = dir.join("page.pbm");
    let r = render_stitched(&|s| {
        fill_rect(s, 10, 10, 50, 20);
        put_pixel(s, 479, 799);
    });
    r.frame.write_pbm(&path).expect("write_pbm");
    assert_eq!(std::fs::read(&path).unwrap(), r.frame.to_pbm());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pbm_write_truncates_an_existing_file() {
    let dir = tmp_dir("pbm-trunc");
    let path = dir.join("page.pbm");
    std::fs::write(&path, vec![0xAAu8; 100_000]).unwrap();
    render_stitched(&|_| {}).frame.write_pbm(&path).expect("write_pbm");
    assert_eq!(std::fs::read(&path).unwrap().len(), PBM_HEADER.len() + 800 * 60);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pbm_is_msb_first_with_one_meaning_black() {
    // (x, y) -> (raster byte index, byte value) by the PBM definition
    let cases: [((i32, i32), usize, u8); 7] = [
        ((0, 0), 0, 0x80),
        ((7, 0), 0, 0x01),
        ((8, 0), 1, 0x80),
        ((479, 0), 59, 0x01),
        ((0, 1), 60, 0x80),
        ((0, 799), 799 * 60, 0x80),
        ((479, 799), 799 * 60 + 59, 0x01),
    ];
    for ((x, y), idx, val) in cases {
        let r = render_stitched(&|s| put_pixel(s, x, y));
        let pbm = r.frame.to_pbm();
        assert_eq!(&pbm[..PBM_HEADER.len()], PBM_HEADER);
        let data = &pbm[PBM_HEADER.len()..];
        assert_eq!(data[idx], val, "pixel ({x},{y}) must land in raster byte {idx} as {val:#04x}");
        let others = data.iter().enumerate().filter(|&(i, &b)| i != idx && b != 0).count();
        assert_eq!(others, 0, "pixel ({x},{y}): no other raster byte may be set");
    }
}

#[test]
fn pbm_equals_independent_encoding_of_the_pixels() {
    let mut want = Cells::new();
    rect_cells(&mut want, 13, 37, 101, 211);
    rect_cells(&mut want, 300, 600, 180, 200); // touches right + bottom edge
    for (x, y) in [(0, 0), (479, 0), (0, 799), (479, 799), (3, 5), (476, 7)] {
        pixel_cell(&mut want, x, y);
    }
    let r = render_stitched(&|s| {
        fill_rect(s, 13, 37, 101, 211);
        fill_rect(s, 300, 600, 180, 200);
        for (x, y) in [(0, 0), (479, 0), (0, 799), (479, 799), (3, 5), (476, 7)] {
            put_pixel(s, x, y);
        }
    });
    assert!(r.frame.to_pbm() == pbm_of(&want), "to_pbm must equal an independent MSB-first, 1 = black encoding of the drawn pixels");
}

#[test]
fn pbm_bits_agree_with_is_black() {
    let r = render_stitched(&|s| {
        fill_rect(s, 5, 9, 33, 17);
        for i in 0..200 {
            put_pixel(s, (i * 7) % 480, (i * 13) % 800);
        }
    });
    let pbm = r.frame.to_pbm();
    let data = &pbm[PBM_HEADER.len()..];
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let bit = (data[y * 60 + x / 8] >> (7 - x % 8)) & 1 == 1;
            assert_eq!(bit, r.frame.is_black(x as u16, y as u16), "pixel ({x},{y})");
        }
    }
    assert_eq!(r.frame.black_count(), frame_cells(&r.frame).len());
}

#[test]
fn portrait_orientation_single_pixels_are_not_mirrored_or_transposed() {
    // asymmetric points: a mirror in x, in y, or a transpose moves every one of them
    let pts = [(0, 0), (479, 0), (0, 799), (479, 799), (3, 5), (476, 7), (11, 790), (468, 794), (1, 0), (0, 1), (240, 400), (100, 700)];
    for (name, render) in RENDERERS {
        for (x, y) in pts {
            let r = render(&move |s: &mut StripBuffer| put_pixel(s, x, y));
            let mut want = Cells::new();
            pixel_cell(&mut want, x, y);
            assert_cells(&format!("{name} pixel ({x},{y})"), &r.frame, &want);
        }
    }
}

#[test]
fn off_screen_pixels_are_ignored() {
    for (name, render) in RENDERERS {
        let r = render(&|s: &mut StripBuffer| {
            for (x, y) in [(-1, 0), (0, -1), (480, 0), (0, 800), (-100, -100), (1000, 1000), (479, 800), (480, 799)] {
                put_pixel(s, x, y);
            }
        });
        assert_eq!(r.frame.black_count(), 0, "{name}: off-screen pixels draw nothing");
    }
}

#[test]
fn solid_rectangles_are_pixel_exact_including_borders() {
    let rects: [(i32, i32, u32, u32); 8] = [
        (13, 37, 101, 211),
        (0, 0, 1, 1),
        (479, 799, 1, 1),
        (0, 0, 480, 800), // whole page
        (0, 0, 480, 1),
        (0, 799, 480, 1),
        (0, 0, 1, 800),
        (479, 0, 1, 800),
    ];
    for (name, render) in RENDERERS {
        for (x, y, w, h) in rects {
            let r = render(&move |s: &mut StripBuffer| fill_rect(s, x, y, w, h));
            let mut want = Cells::new();
            rect_cells(&mut want, x, y, w as i32, h as i32);
            assert_cells(&format!("{name} rect ({x},{y}) {w}x{h}"), &r.frame, &want);
        }
    }
}

#[test]
fn rectangles_partly_off_screen_are_clipped() {
    let rects: [(i32, i32, u32, u32); 4] = [(-10, -10, 30, 30), (470, 790, 40, 40), (-5, 100, 20, 5), (400, -3, 100, 10)];
    for (name, render) in RENDERERS {
        for (x, y, w, h) in rects {
            let r = render(&move |s: &mut StripBuffer| fill_rect(s, x, y, w, h));
            let mut want = Cells::new();
            rect_cells(&mut want, x, y, w as i32, h as i32);
            assert_cells(&format!("{name} clipped rect ({x},{y}) {w}x{h}"), &r.frame, &want);
        }
    }
}

#[test]
fn rectangles_have_the_stated_orientation() {
    // tall thin vs wide thin: a transpose swaps them
    for (name, render) in RENDERERS {
        let r = render(&|s: &mut StripBuffer| {
            fill_rect(s, 20, 30, 4, 300); // tall
            fill_rect(s, 100, 500, 300, 4); // wide
        });
        let mut want = Cells::new();
        rect_cells(&mut want, 20, 30, 4, 300);
        rect_cells(&mut want, 100, 500, 300, 4);
        assert_cells(&format!("{name} tall+wide"), &r.frame, &want);
    }
}

// ---- glyph blit through the production FontSet ----------------------------

const GLYPH_CHARS: [char; 8] = ['H', 'g', '.', 'W', 'i', ' ', '\u{e9}', '\u{2014}'];

#[test]
fn single_glyph_pixels_match_the_public_bitmap_for_every_font_and_style() {
    let (cx, base) = (100, 300);
    for size in 0..NUM_FONTS {
        let fs = FontSet::for_size(size);
        for style in STYLES {
            for ch in GLYPH_CHARS {
                let font = fs.font(style);
                let mut want = Cells::new();
                let advance = glyph_cells(font, ch, cx, base, &mut want);
                clip(&mut want);
                let got_adv = Cell::new(0u8);
                // full-path equality is checked for every combination on the stitched
                // path and (below) on the full path for the extreme sizes
                let r = render_stitched(&|s: &mut StripBuffer| got_adv.set(fs.draw_char(s, ch, style, cx, base)));
                assert_eq!(got_adv.get(), advance, "size {size} {style:?} {ch:?}: draw_char returns the glyph advance");
                assert_cells(&format!("size {size} {style:?} {ch:?}"), &r.frame, &want);
            }
        }
    }
    for size in [0u8, NUM_FONTS - 1] {
        let fs = FontSet::for_size(size);
        for ch in ['H', 'g', '\u{e9}'] {
            let mut want = Cells::new();
            glyph_cells(fs.font(Style::Regular), ch, cx, base, &mut want);
            let r = render_full(&|s: &mut StripBuffer| {
                fs.draw_char(s, ch, Style::Regular, cx, base);
            });
            assert_cells(&format!("full size {size} {ch:?}"), &r.frame, &want);
        }
    }
}

#[test]
fn glyph_position_is_cursor_plus_offset_x_and_baseline_plus_offset_y() {
    // 'H' has ink, so any 1 px shift of cx or baseline moves the expected set
    let fs = FontSet::for_size(2);
    let f = fs.font(Style::Regular);
    for (cx, base) in [(50, 100), (51, 100), (50, 101), (200, 400), (301, 799 - 4)] {
        let mut want = Cells::new();
        glyph_cells(f, 'H', cx, base, &mut want);
        clip(&mut want);
        let r = render_stitched(&|s: &mut StripBuffer| {
            fs.draw_char(s, 'H', Style::Regular, cx, base);
        });
        assert!(!want.is_empty());
        assert_cells(&format!("'H' at cx {cx} baseline {base}"), &r.frame, &want);
    }
}

#[test]
fn glyphs_clipped_at_the_page_edges() {
    let fs = FontSet::for_size(3);
    let f = fs.font(Style::Regular);
    for (cx, base) in [(-6, 300), (470, 300), (100, 4), (100, 798), (-3, 2), (476, 799)] {
        let mut want = Cells::new();
        glyph_cells(f, 'W', cx, base, &mut want);
        clip(&mut want);
        let r = render_stitched(&|s: &mut StripBuffer| {
            fs.draw_char(s, 'W', Style::Regular, cx, base);
        });
        assert_cells(&format!("clipped 'W' at ({cx},{base})"), &r.frame, &want);
    }
}

#[test]
fn string_glyphs_advance_by_each_glyphs_advance() {
    let fs = FontSet::for_size(1);
    let f = fs.font(Style::Regular);
    let text = "Hg.W i";
    let (cx, base) = (30, 200);
    let mut want = Cells::new();
    let mut pen = cx;
    for ch in text.chars() {
        pen += glyph_cells(f, ch, pen, base, &mut want) as i32;
    }
    clip(&mut want);
    let end = Cell::new(0);
    let r = render_stitched(&|s: &mut StripBuffer| end.set(fs.draw_str(s, text, Style::Regular, cx, base)));
    assert_eq!(end.get(), pen, "draw_str returns the final pen x");
    assert_cells("draw_str", &r.frame, &want);
}

#[test]
fn utf8_bytes_decode_to_extended_glyphs() {
    let fs = FontSet::for_size(2);
    let f = fs.font(Style::Regular);
    let bytes = "caf\u{e9} \u{2014} x".as_bytes();
    let (cx, base) = (20, 150);
    let mut want = Cells::new();
    let mut pen = cx;
    for ch in Utf8Iter::new(bytes) {
        pen += glyph_cells(f, ch, pen, base, &mut want) as i32;
    }
    clip(&mut want);
    let end = Cell::new(0);
    let r = render_stitched(&|s: &mut StripBuffer| end.set(fs.draw_bytes(s, bytes, Style::Regular, cx, base)));
    assert_eq!(end.get(), pen);
    assert_cells("draw_bytes", &r.frame, &want);
}

// ---- reader page (the real ReaderApp::draw) ---------------------------------

#[test]
fn reader_geometry_matches_theme_and_screen_constants() {
    let book = prose(7, 6000);
    for theme in 0..NUM_THEMES {
        let r = rig_for(&book, 2, theme);
        let t = reading_theme(theme);
        assert_eq!(r.text_margin(), t.margin_h, "theme {theme} margin_h");
        assert_eq!(r.text_y(), 24 + t.margin_v, "theme {theme}: TEXT_Y 24 + margin_v");
        assert_eq!(r.text_w(), 480 - 2 * t.margin_h as u32, "theme {theme} text width");
        assert_eq!(r.text_y() + r.text_area_h(), 796, "theme {theme}: text area ends 4 px above the page bottom");
    }
}

#[test]
fn reader_page_ink_is_exactly_the_laid_out_glyphs_over_every_font_and_theme() {
    let book = prose(0x5eed, 16_000);
    for font in 0..NUM_FONTS {
        for theme in 0..NUM_THEMES {
            let mut r = rig_for(&book, font, theme);
            for page in [0usize, 3] {
                goto_page(&mut r, page);
                let fb = shot(&r).frame;
                let actual = band_cells(&r, &fb);
                let loose = band_cells_loose(&r, &fb);
                let lines = r.lines();
                assert!(!lines.is_empty() && !actual.is_empty(), "font {font} theme {theme} page {page}: a text page has ink");
                let first = lines.iter().position(|l| !l.is_empty()).expect("a non-blank line");
                let (top, _) = text_band(&r);
                let (ink_top, _) = strict_band(&r);
                // line 0's baseline is unknown to the test (reader layout detail); it must
                // be within the first two line boxes, and then: every glyph pixel is black
                // in the whole band, and the chrome-free part of the band is EXACTLY the
                // glyph composite with the firmware's pitch font_line_h
                let found = (top..=top + 2 * r.font_line_h() as i32).find(|&b0| {
                    if !page_text_cells(&r, font, b0, first, first + 1).is_subset(&loose) {
                        return false;
                    }
                    let want = page_text_cells(&r, font, b0, 0, lines.len());
                    want.is_subset(&loose) && below(&want, ink_top) == actual
                });
                assert!(
                    found.is_some(),
                    "font {font} theme {theme} page {page}: band ink is not the Regular-glyph composite of the {} laid-out lines at x = margin {}, pitch {}",
                    lines.len(),
                    r.text_margin(),
                    r.font_line_h()
                );
            }
        }
    }
}

#[test]
fn single_line_page_has_ink_only_in_its_line_and_inside_the_margins() {
    let (font, theme) = (2u8, 1u8);
    let r = rig_for(b"Hello, World", font, theme);
    assert_eq!(r.lines(), vec![b"Hello, World".to_vec()]);
    let fb = shot(&r).frame;
    let actual = band_cells(&r, &fb);
    let loose = band_cells_loose(&r, &fb);
    assert!(!actual.is_empty(), "the line is drawn");
    let (top, _) = text_band(&r);
    let (ink_top, _) = strict_band(&r);
    let found = (top..=top + 2 * r.font_line_h() as i32).find(|&b0| {
        let want = page_text_cells(&r, font, b0, 0, 1);
        want.is_subset(&loose) && below(&want, ink_top) == actual
    });
    let b0 = found.expect("band ink == glyph composite of the single line");
    // everything lies in the first line box (+ its descent): below that the band is white
    let max_y = actual.iter().map(|&(_, y)| y).max().unwrap();
    assert!(max_y < b0 + FontSet::for_size(font).font(Style::Regular).line_height as i32, "no ink below the single line");
    let (min_x, max_x) = (actual.iter().map(|&(x, _)| x).min().unwrap(), actual.iter().map(|&(x, _)| x).max().unwrap());
    assert!(min_x >= r.text_margin() as i32 && max_x < 480 - r.text_margin() as i32, "ink [{min_x}, {max_x}] inside the margins");
}

#[test]
fn text_pages_keep_the_side_margins_white_inside_the_text_band() {
    let book = prose(0x5eed, 16_000);
    for (font, theme) in [(0u8, 0u8), (2, 1), (4, 3), (1, 2), (3, 0)] {
        let mut r = rig_for(&book, font, theme);
        for page in [0usize, 2] {
            goto_page(&mut r, page);
            let fb = shot(&r).frame;
            let band = band_cells(&r, &fb);
            assert!(!band.is_empty(), "font {font} theme {theme} page {page}: non-blank text band");
            let m = r.text_margin() as i32;
            let inside = band.iter().filter(|&&(x, _)| x >= m && x < 480 - m).count();
            assert_eq!(inside, band.len(), "font {font} theme {theme} page {page}: ink only in [{m}, {})", 480 - m);
            // and the page as a whole is not blank
            assert!(fb.black_count() >= band.len());
        }
    }
}

#[test]
fn reader_page_artifact_is_a_valid_portrait_pbm() {
    let book = prose(11, 9000);
    let r = rig_for(&book, 2, 1);
    let dir = tmp_dir("page-pbm");
    let path = dir.join("page0.pbm");
    let shot = shot(&r);
    shot.frame.write_pbm(&path).expect("write_pbm");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..PBM_HEADER.len()], PBM_HEADER);
    assert_eq!(bytes.len(), PBM_HEADER.len() + 48_000);
    let black_bits: u32 = bytes[PBM_HEADER.len()..].iter().map(|b| b.count_ones()).sum();
    assert_eq!(black_bits as usize, shot.frame.black_count());
    assert!(black_bits > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rendering_the_same_page_twice_is_byte_identical() {
    let book = prose(0x5eed, 12_000);
    let mut r = rig_for(&book, 2, 1);
    for page in [0usize, 2] {
        goto_page(&mut r, page);
        let a = shot(&r).frame.to_pbm();
        let b = shot(&r).frame.to_pbm();
        assert!(a == b, "page {page}: two renders differ");
        let dir = tmp_dir("determinism");
        let (p1, p2) = (dir.join("a.pbm"), dir.join("b.pbm"));
        shot(&r).frame.write_pbm(&p1).unwrap();
        shot(&r).frame.write_pbm(&p2).unwrap();
        assert!(std::fs::read(&p1).unwrap() == std::fs::read(&p2).unwrap(), "page {page}: two artifacts differ");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // a freshly built rig on the same data / page renders the same bytes (no hidden global state)
    let again = {
        let mut r2 = rig_for(&book, 2, 1);
        goto_page(&mut r2, 2);
        shot(&r2).frame.to_pbm()
    };
    assert!(again == shot(&r).frame.to_pbm(), "an independent rig on the same page renders different bytes");
}

#[test]
fn rendering_does_not_change_the_rig() {
    let book = prose(0x5eed, 12_000);
    let mut r = rig_for(&book, 2, 1);
    goto_page(&mut r, 2);
    let before = (r.page(), r.total_pages(), r.lines(), r.page_offsets(), r.phase());
    for _ in 0..3 {
        let _ = shot(&r);
        let _ = render_full(&|s| r.draw(s));
    }
    let after = (r.page(), r.total_pages(), r.lines(), r.page_offsets(), r.phase());
    assert!(before == after, "render must not touch the reader state");
    assert_eq!(r.page(), 2);
}

#[test]
fn different_pages_render_differently() {
    let book = prose(0x5eed, 12_000);
    let mut r = rig_for(&book, 2, 1);
    let p0 = shot(&r).frame;
    r.press(Action::Next);
    assert_eq!(r.page(), 1);
    let p1 = shot(&r).frame;
    r.press(Action::Next);
    let p2 = shot(&r).frame;
    assert!(p0.to_pbm() != p1.to_pbm(), "page 0 and page 1 look identical");
    assert!(p1.to_pbm() != p2.to_pbm(), "page 1 and page 2 look identical");
    assert!(p0.to_pbm() != p2.to_pbm());
    // the text bands differ (not just a page number in the chrome)
    let (b0, b1) = (band_cells(&r, &p0), band_cells(&r, &p1));
    assert!(b0 != b1, "the text band of page 0 and page 1 is the same");
}

// ===========================================================================
// strip equality -- strip equality: full-frame == stitched strips, strips really are strips
// ===========================================================================

#[test]
fn stitched_render_is_really_made_of_the_firmware_strips() {
    let seen = RefCell::new(Vec::new());
    let calls = Cell::new(0usize);
    let r = render_stitched(&|s: &mut StripBuffer| {
        let w = s.logical_window();
        seen.borrow_mut().push((w.x, w.y, w.w, w.h));
        calls.set(calls.get() + 1);
    });
    let strips = StripBuffer::strip_count() as usize;
    assert_eq!(strips, 480 / STRIP_ROWS as usize, "firmware strips: the 480 panel rows in STRIP_ROWS bands");
    assert!(strips > 1, "a page is several strips, never one");
    assert_eq!(r.passes.len(), strips, "one pass per strip");
    assert_eq!(calls.get(), strips, "draw invoked exactly once per strip");
    let observed: Vec<Pass> = seen.borrow().iter().map(|&(x, y, w, h)| Pass { x, y, w, h }).collect();
    assert_eq!(observed, r.passes, "reported passes are the windows draw actually saw, in order");
    assert_exact_tiling("stitched", &r.passes);
    assert_within_capacity("stitched", &r.passes);
    for p in &r.passes {
        assert_eq!((p.w.min(p.h), p.w.max(p.h)), (STRIP_ROWS, 800), "every strip is STRIP_ROWS x 800 (one physical band): {p:?}");
    }
}

#[test]
fn full_render_is_a_different_exact_tiling_within_buffer_capacity() {
    let seen = RefCell::new(Vec::new());
    let r = render_full(&|s: &mut StripBuffer| {
        let w = s.logical_window();
        seen.borrow_mut().push((w.x, w.y, w.w, w.h));
    });
    let observed: Vec<Pass> = seen.borrow().iter().map(|&(x, y, w, h)| Pass { x, y, w, h }).collect();
    assert_eq!(observed, r.passes, "reported passes are the windows draw actually saw, in order");
    assert!(r.passes.len() > 1, "the 48 KB page can never be one 4 KB window");
    assert_exact_tiling("full", &r.passes);
    assert_within_capacity("full", &r.passes);
    let stitched = render_stitched(&|_| {});
    assert!(r.passes != stitched.passes, "'full' must be a different partition than the firmware strips, else equality is vacuous");
    let strip_shape = |p: &Pass| (p.w.min(p.h), p.w.max(p.h)) == (STRIP_ROWS, 800);
    assert!(!r.passes.iter().all(strip_shape), "'full' tiles must not all be firmware-strip shaped");
}

#[test]
fn every_pass_contributes_a_marker_pixel() {
    // a renderer that skips / duplicates / mis-places any one pass loses or moves a marker
    for (name, render) in RENDERERS {
        let probe = render(&|_| {});
        let markers: Vec<(i32, i32)> = probe.passes.iter().map(|p| (p.x as i32 + p.w as i32 / 2, p.y as i32 + p.h as i32 / 2)).collect();
        // first and last pixel of every pass too (the pass corners)
        let mut corners: Vec<(i32, i32)> = Vec::new();
        for p in &probe.passes {
            corners.push((p.x as i32, p.y as i32));
            corners.push((p.x as i32 + p.w as i32 - 1, p.y as i32 + p.h as i32 - 1));
        }
        let mut want = Cells::new();
        for &(x, y) in markers.iter().chain(&corners) {
            pixel_cell(&mut want, x, y);
        }
        let r = render(&|s: &mut StripBuffer| {
            for &(x, y) in markers.iter().chain(&corners) {
                put_pixel(s, x, y);
            }
        });
        assert_eq!(r.passes, probe.passes, "{name}: the tiling does not depend on the content");
        assert_cells(&format!("{name} markers"), &r.frame, &want);
    }
}

#[test]
fn blank_full_equals_stitched() {
    let (a, b) = (render_full(&|_| {}), render_stitched(&|_| {}));
    assert_same("blank", &a.frame, &b.frame);
    assert_eq!(a.frame.black_count(), 0);
}

#[test]
fn whole_page_black_full_equals_stitched() {
    let draw = |s: &mut StripBuffer| fill_rect(s, 0, 0, 480, 800);
    let (a, b) = (render_full(&draw), render_stitched(&draw));
    assert_same("black page", &a.frame, &b.frame);
    assert_eq!(a.frame.black_count(), 480 * 800);
}

// edges of BOTH renderers' tiles: any content that crosses any of them is a
// strip-boundary case for at least one of the two paths
fn all_edges() -> (Vec<i32>, Vec<i32>) {
    let mut passes = render_stitched(&|_| {}).passes;
    passes.extend(render_full(&|_| {}).passes);
    internal_edges(&passes)
}

#[test]
fn rectangles_straddling_every_tile_boundary_are_identical_and_correct() {
    let (xs, ys) = all_edges();
    assert!(xs.len() + ys.len() >= 11, "at least the firmware strip boundaries: x {xs:?} y {ys:?}");
    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    for (k, &e) in xs.iter().enumerate() {
        rects.push((e - 3, 10 + (k as i32 * 61) % 700, 6, 40)); // 3 px either side of the edge
        rects.push((e - 1, 0, 1, 800)); // 1 px column just left of the edge, full height
        rects.push((e, 0, 1, 800)); // 1 px column just right of the edge
    }
    for (k, &e) in ys.iter().enumerate() {
        rects.push((10 + (k as i32 * 37) % 400, e - 3, 40, 6));
        rects.push((0, e - 1, 480, 1));
        rects.push((0, e, 480, 1));
    }
    // straddling an x edge and a y edge at once
    if let (Some(&ex), Some(&ey)) = (xs.get(xs.len() / 2), ys.get(ys.len() / 2)) {
        rects.push((ex - 20, ey - 20, 40, 40));
    }
    let mut want = Cells::new();
    for &(x, y, w, h) in &rects {
        rect_cells(&mut want, x, y, w, h);
    }
    let draw = |s: &mut StripBuffer| {
        for &(x, y, w, h) in &rects {
            fill_rect(s, x, y, w as u32, h as u32);
        }
    };
    let (full, stitched) = (render_full(&draw), render_stitched(&draw));
    assert_cells("stitched boundary rects", &stitched.frame, &want);
    assert_cells("full boundary rects", &full.frame, &want);
    assert_same("boundary rects", &full.frame, &stitched.frame);
}

#[test]
fn single_pixels_on_both_sides_of_every_tile_boundary() {
    let (xs, ys) = all_edges();
    let mut pts: Vec<(i32, i32)> = Vec::new();
    for &e in &xs {
        for dx in [-2, -1, 0, 1] {
            for y in [0, 1, 399, 400, 798, 799] {
                pts.push((e + dx, y));
            }
        }
    }
    for &e in &ys {
        for dy in [-2, -1, 0, 1] {
            for x in [0, 1, 239, 240, 478, 479] {
                pts.push((x, e + dy));
            }
        }
    }
    let mut want = Cells::new();
    for &(x, y) in &pts {
        pixel_cell(&mut want, x, y);
    }
    let draw = |s: &mut StripBuffer| {
        for &(x, y) in &pts {
            put_pixel(s, x, y);
        }
    };
    let (full, stitched) = (render_full(&draw), render_stitched(&draw));
    assert_cells("stitched boundary pixels", &stitched.frame, &want);
    assert_cells("full boundary pixels", &full.frame, &want);
}

#[test]
fn glyphs_cut_by_a_tile_boundary_are_identical_and_correct() {
    let (xs, ys) = all_edges();
    let fs = FontSet::for_size(4);
    let f = fs.font(Style::Regular);
    let g = f.resolve('H').glyph;
    let (gw, gh) = (g.width as i32, g.height as i32);
    assert!(gw > 4 && gh > 4);
    // (cx, baseline) so that the edge runs through the middle of the glyph
    let mut placements: Vec<(i32, i32)> = Vec::new();
    for (k, &e) in xs.iter().enumerate() {
        placements.push((e - g.offset_x as i32 - gw / 2, 120 + (k as i32 * 53) % 600));
    }
    for (k, &e) in ys.iter().enumerate() {
        placements.push((20 + (k as i32 * 71) % 380, e - g.offset_y as i32 - gh / 2));
    }
    let mut want = Cells::new();
    for &(cx, base) in &placements {
        glyph_cells(f, 'H', cx, base, &mut want);
    }
    clip(&mut want);
    let draw = |s: &mut StripBuffer| {
        for &(cx, base) in &placements {
            fs.draw_char(s, 'H', Style::Regular, cx, base);
        }
    };
    let (full, stitched) = (render_full(&draw), render_stitched(&draw));
    assert_cells("stitched cut glyphs", &stitched.frame, &want);
    assert_cells("full cut glyphs", &full.frame, &want);
    assert_same("cut glyphs", &full.frame, &stitched.frame);
}

#[test]
fn seeded_scatter_of_rectangles_and_pixels_full_equals_stitched_equals_geometry() {
    let mut rng = Lcg(0xC61);
    let mut rects = Vec::new();
    for _ in 0..80 {
        let (x, y) = (rng.below(520) as i32 - 20, rng.below(840) as i32 - 20);
        let (w, h) = (1 + rng.below(120) as i32, 1 + rng.below(120) as i32);
        rects.push((x, y, w, h));
    }
    let pixels: Vec<(i32, i32)> = (0..400).map(|_| (rng.below(500) as i32 - 10, rng.below(820) as i32 - 10)).collect();
    let mut want = Cells::new();
    for &(x, y, w, h) in &rects {
        rect_cells(&mut want, x, y, w, h);
    }
    for &(x, y) in &pixels {
        pixel_cell(&mut want, x, y);
    }
    let draw = |s: &mut StripBuffer| {
        for &(x, y, w, h) in &rects {
            fill_rect(s, x, y, w as u32, h as u32);
        }
        for &(x, y) in &pixels {
            put_pixel(s, x, y);
        }
    };
    let (full, stitched) = (render_full(&draw), render_stitched(&draw));
    assert_cells("stitched scatter", &stitched.frame, &want);
    assert_cells("full scatter", &full.frame, &want);
}

#[test]
fn reader_pages_full_equals_stitched_for_every_font_and_theme() {
    let book = prose(0x5eed, 16_000);
    for font in 0..NUM_FONTS {
        for theme in 0..NUM_THEMES {
            let mut r = rig_for(&book, font, theme);
            let mut targets = vec![0usize, 3];
            targets.push(usize::MAX); // last page (partly filled)
            for target in targets {
                if target == usize::MAX {
                    goto_last(&mut r);
                } else {
                    goto_page(&mut r, target);
                }
                let stitched = render_stitched(&|s| r.draw(s));
                let full = render_full(&|s| r.draw(s));
                let what = format!("font {font} theme {theme} page {}", r.page());
                assert!(stitched.frame.black_count() > 0, "{what}: not blank");
                assert_same(&what, &full.frame, &stitched.frame);
            }
        }
    }
}

#[test]
fn reader_page_ink_crosses_strip_boundaries() {
    // guards against an equality that only holds because nothing was drawn near a boundary:
    // the stitched page has ink on both sides of every internal strip edge
    let book = prose(0x5eed, 12_000);
    let r = rig_for(&book, 2, 1);
    let page = shot(&r);
    let (xs, _) = internal_edges(&page.passes);
    assert!(xs.len() >= 11);
    let cells = frame_cells(&page.frame);
    for &e in &xs {
        // columns in the margin may be blank; require ink in the 16 px on each side instead
        let strip_left = cells.iter().filter(|&&(x, _)| x >= e - 16 && x < e).count();
        let strip_right = cells.iter().filter(|&&(x, _)| x >= e && x < e + 16).count();
        assert!(strip_left > 0 && strip_right > 0, "edge x={e}: ink on both sides ({strip_left}/{strip_right})");
    }
}

// Vertical writing: the package declares it, the reader sets text in columns.
// Layout and drawing are the production ReaderApp's; the packs are the literal
// ones of `cjk_support` (every glyph an 8x3 pattern, advance px + 1), so a
// column's geometry can be read off the page pixel for pixel.
mod cjk_support;

use cjk_support::{BODY, SIZES, install, pack_of, rows, text_lines};
use pulp_board_logic::chapter_ring::RingConfig;
use pulp_board_logic::image_lru::LruConfig;
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, Run,
    TocItem, Writing, build_epub_writing,
};
use pulp_host::fonts::vertical::{
    Setting, em_ascent, presentation_form, setting, short_number_len, turned_origin,
};
use pulp_host::fonts::{FontSet, Style};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{Framebuffer, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

const BOOK: &str = "V.EPUB";
const PX: u16 = BODY[2];
const VERTICAL: Writing<'static> = Writing {
    language: "ja",
    page_progression: Some("rtl"),
};

// column 0 of every fixture page is the chapter's own heading ("章", set at
// the heading size), column 1 the blank line between blocks; the paragraph
// starts in column 2
const FIRST: usize = 2;

fn alphabet(extra: &str) -> Vec<char> {
    let mut c: Vec<char> = "臺灣繁體中文章".chars().chain(extra.chars()).collect();
    c.sort_unstable();
    c.dedup();
    c
}

fn epub(blocks: Vec<Block>, images: Vec<ImageSpec>, writing: &Writing<'_>) -> Vec<u8> {
    build_epub_writing(
        &EpubSpec {
            version: EpubVersion::V3,
            title: "直排".into(),
            author: "Fixture".into(),
            identifier: "vertical-fixture".into(),
            chapters: vec![Chapter {
                title: "章".into(),
                blocks,
            }],
            toc: vec![TocItem {
                title: "章".into(),
                chapter: 0,
                children: vec![],
            }],
            images,
            cover: None,
            compression: Compression::Stored,
            numeric_entities: false,
        },
        writing,
    )
    .unwrap()
}

fn open_book(bytes: &[u8], packs: &str) -> Rig {
    let card = VirtualStorage::memory_with(&[(BOOK, bytes)]);
    card.ensure_pulp_dir().unwrap();
    for px in SIZES {
        install(&card, px, &pack_of(px, false, alphabet(packs)));
    }
    let mut r = Rig::new(card);
    r.configure(2, 0);
    r.open(BOOK);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    r
}

fn open(text: &str, packs: &str, writing: &Writing<'_>) -> Rig {
    let blocks = vec![Block::Paragraph(vec![Run::Text(text.into())])];
    open_book(&epub(blocks, vec![], writing), packs)
}

// where column `i` has its centre line, and the em box of a glyph in it
struct Grid {
    right: i32,
    pitch: i32,
    top: i32,
}

impl Grid {
    fn of(r: &Rig) -> Self {
        let pitch = i32::from(r.font_line_h());
        let grid_w = pitch * r.max_lines() as i32;
        Grid {
            right: i32::from(r.text_margin()) + (r.text_w() as i32 + grid_w) / 2,
            pitch,
            top: i32::from(r.text_y()),
        }
    }
    fn center(&self, column: usize) -> i32 {
        self.right - self.pitch * column as i32 - self.pitch / 2
    }
    fn em_left(&self, column: usize) -> i32 {
        self.center(column) - i32::from(PX) / 2
    }
    fn em_top(&self, row: usize) -> i32 {
        self.top + i32::from(PX) * row as i32
    }
}

fn is_ink(f: &Framebuffer, x: i32, y: i32, w: usize, bm: &[u8], h: usize) -> bool {
    let stride = w.div_ceil(8);
    (0..h).all(|dy| {
        (0..w).all(|dx| {
            let ink = bm[dy * stride + dx / 8] & (0x80 >> (dx % 8)) != 0;
            f.is_black((x + dx as i32) as u16, (y + dy as i32) as u16) == ink
        })
    })
}

// the literal glyph of `c` set upright in `row` of `column`: the pack's bearing
// is (0, -3) and its baseline hangs `em_ascent` below the em box top
fn assert_upright(f: &Framebuffer, g: &Grid, column: usize, row: usize, c: char) {
    let (x, y) = (g.em_left(column), g.em_top(row) + em_ascent(PX) - 3);
    assert!(
        is_ink(f, x, y, 8, &rows(PX, c), 3),
        "{c:?} upright at column {column}, row {row} (x={x}, y={y})"
    );
}

// the 8x3 glyph of `c` as the 3x8 bitmap a quarter turn clockwise makes of it
fn turned(c: char) -> [u8; 8] {
    let src = rows(PX, c);
    let mut out = [0u8; 8];
    for (y, row) in src.iter().enumerate() {
        for x in 0..8 {
            if row & (0x80 >> x) != 0 {
                out[x] |= 0x80 >> (2 - y);
            }
        }
    }
    out
}

fn assert_turned(f: &Framebuffer, g: &Grid, column: usize, row: usize, c: char) {
    let (x, y) = turned_origin(g.em_left(column), g.em_top(row), PX, 0, -3, 3);
    assert!(
        is_ink(f, x, y, 3, &turned(c), 8),
        "{c:?} turned at column {column}, row {row} (x={x}, y={y})"
    );
}

#[test]
fn rtl_spine_and_a_cjk_language_make_a_book_vertical() {
    let cases: [(&str, Option<&str>, bool); 9] = [
        ("ja", Some("rtl"), true),
        ("zh-TW", Some("rtl"), true),
        ("ko-KR", Some("rtl"), true),
        ("JA", Some("rtl"), true),
        ("zh_Hant", Some("rtl"), true),
        ("en", Some("rtl"), false),
        ("ja", Some("ltr"), false),
        ("ja", Some("default"), false),
        ("ja", None, false),
    ];
    for (language, page_progression, want) in cases {
        let writing = Writing {
            language,
            page_progression,
        };
        let r = open("臺灣", "", &writing);
        assert_eq!(r.vertical(), want, "{language} {page_progression:?}");
    }
}

#[test]
fn a_horizontal_book_after_a_vertical_one_lays_out_in_lines_again() {
    let text = "臺灣繁體中文".repeat(100);
    let mut r = open(&text, "", &VERTICAL);
    assert!(r.vertical());
    let bytes = epub(
        vec![Block::Paragraph(vec![Run::Text(text)])],
        vec![],
        &Writing::LATIN,
    );
    r.exit();
    let mut r2 = open_book(&bytes, "");
    assert!(!r2.vertical());
    // lines run the text width, a page holds as many of them as the height fits
    assert_eq!(
        r2.max_lines(),
        usize::from(r2.text_area_h() / r2.font_line_h()).min(37)
    );
    r2.exit();
}

#[test]
fn a_column_is_as_tall_as_the_text_area_and_a_page_has_text_width_over_pitch_columns() {
    let r = open(&"臺灣繁體中文".repeat(100), "", &VERTICAL);
    assert_eq!(
        r.max_lines(),
        (r.text_w() / u32::from(r.font_line_h())) as usize
    );
    let lines = text_lines(&r);
    assert_eq!(lines.len(), r.max_lines(), "the page is full");
    assert_eq!((lines[0].as_str(), lines[1].as_str()), ("章", ""));
    // every glyph advances one em down the column, whatever the pack says is
    // its horizontal advance (px + 1 here)
    let per_column = usize::from(r.text_area_h() / PX);
    for (i, line) in lines.iter().enumerate().skip(FIRST) {
        assert_eq!(line.chars().count(), per_column, "column {i}");
    }
}

#[test]
fn a_column_ends_above_the_button_labels_drawn_along_the_bottom_edge() {
    let r = open(&"臺灣繁體中文".repeat(100), "", &VERTICAL);
    // BUTTON_BAR_H: the labels' tab (22) and the inset below it (4)
    let bar_top: u16 = 800 - 26;
    assert!(
        r.text_y() + r.text_area_h() <= bar_top,
        "text band ends at {}, the labels start at {bar_top}",
        r.text_y() + r.text_area_h()
    );
    let g = Grid::of(&r);
    let rows = i32::from(r.text_area_h() / PX);
    assert!(
        g.em_top(rows as usize) <= i32::from(bar_top),
        "the last glyph of a column is clear of the labels"
    );
}

#[test]
fn glyphs_are_centred_on_their_column_and_columns_run_right_to_left() {
    let r = open(&"臺灣繁體中文".repeat(100), "", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    let lines = text_lines(&r);
    for column in [FIRST, FIRST + 1, r.max_lines() - 1] {
        for (row, c) in lines[column].chars().take(3).enumerate() {
            assert_upright(&f, &g, column, row, c);
        }
    }
    assert!(
        g.center(FIRST + 1) < g.center(FIRST),
        "the second column is to the left"
    );
}

#[test]
fn a_bracket_the_pack_has_no_vertical_form_for_is_turned_inside_its_em_box() {
    let r = open("臺「體", "「", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    assert_upright(&f, &g, FIRST, 0, '臺');
    assert_turned(&f, &g, FIRST, 1, '「');
    assert_upright(&f, &g, FIRST, 2, '體');
}

#[test]
fn a_bracket_the_pack_has_a_vertical_form_for_is_set_with_that_glyph_upright() {
    let r = open("臺「體", "「\u{FE41}", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    assert_upright(&f, &g, FIRST, 1, '\u{FE41}');
    let (x, y) = (g.em_left(FIRST), g.em_top(1) + em_ascent(PX) - 3);
    assert!(!is_ink(&f, x, y, 8, &rows(PX, '「'), 3));
}

#[test]
fn a_comma_without_a_vertical_form_stays_upright_not_turned_on_its_side() {
    let r = open("臺，體", "，", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    assert_upright(&f, &g, FIRST, 1, '，');
}

#[test]
fn a_missing_form_does_not_draw_the_missing_glyph_box() {
    // `；` has no form in this pack: the page must show the glyph of `；`, not
    // the box a pack answers for a codepoint it lacks
    let r = open("臺；體", "；", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    assert_upright(&f, &g, FIRST, 1, '；');
}

#[test]
fn strips_and_full_bands_agree_on_a_page_of_columns() {
    let r = open(
        &"臺7灣12繁123體2026中文「」，".repeat(60),
        "「」，\u{FE41}",
        &VERTICAL,
    );
    let full = render_full(&|s| r.draw(s)).frame;
    let stitched = render_stitched(&|s| r.draw(s)).frame;
    assert_eq!(full.to_pbm(), stitched.to_pbm());
    assert!(full.black_count() > 0);
}

fn assert_sideways_digit(f: &Framebuffer, g: &Grid, column: usize, top: i32, digit: char) {
    let font = FontSet::for_size(2).font(Style::Regular);
    let r = font.resolve(digit);
    let glyph = r.glyph;
    let (x, y) = turned_origin(
        g.em_left(column),
        top,
        PX,
        glyph.offset_x.into(),
        glyph.offset_y.into(),
        glyph.height.into(),
    );
    let stride = usize::from(glyph.width).div_ceil(8);
    for sy in 0..usize::from(glyph.height) {
        for sx in 0..usize::from(glyph.width) {
            let ink = r.bitmaps[glyph.bitmap_offset as usize + sy * stride + sx / 8]
                & (0x80 >> (sx % 8))
                != 0;
            assert_eq!(
                f.is_black(
                    (x + i32::from(glyph.height) - 1 - sy as i32) as u16,
                    (y + sx as i32) as u16
                ),
                ink,
                "sideways {digit} at column {column}, top {top}"
            );
        }
    }
}

// Compare the upright cell with an independent horizontal render of the same
// digits. The font's digit bearings stay inside its advance box.
fn assert_number(f: &Framebuffer, g: &Grid, column: usize, row: usize, digits: &str) {
    let fonts = FontSet::for_size(2);
    let font = fonts.font(Style::Regular);
    let width: i32 = digits.chars().map(|c| i32::from(font.advance(c))).sum();
    let above = digits
        .chars()
        .map(|c| i32::from(font.glyph(c).offset_y))
        .min()
        .unwrap();
    let below = digits
        .chars()
        .map(|c| i32::from(font.glyph(c).offset_y) + i32::from(font.glyph(c).height))
        .max()
        .unwrap();
    for c in digits.chars() {
        let glyph = font.glyph(c);
        assert!(glyph.offset_x >= 0);
        assert!(i32::from(glyph.offset_x) + i32::from(glyph.width) <= i32::from(glyph.advance));
    }
    let horizontal = render_full(&|s| {
        fonts.draw_str(s, digits, Style::Regular, 0, -above);
    })
    .frame;
    let (w, h) = (width.min(i32::from(PX)), below - above);
    assert!(h <= i32::from(PX));
    let (left, top) = (g.em_left(column), g.em_top(row));
    let (pad_x, pad_y) = ((i32::from(PX) - w) / 2, (i32::from(PX) - h) / 2);
    for dy in 0..i32::from(PX) {
        for dx in 0..i32::from(PX) {
            let ink = (pad_x..pad_x + w).contains(&dx)
                && (pad_y..pad_y + h).contains(&dy)
                && horizontal.is_black(((dx - pad_x) * width / w) as u16, (dy - pad_y) as u16);
            assert_eq!(
                f.is_black((left + dx) as u16, (top + dy) as u16),
                ink,
                "upright {digits} at column {column}, row {row}: ({dx}, {dy})"
            );
        }
    }
}

#[test]
fn short_numbers_are_horizontal_in_one_em_and_long_numbers_stay_sideways() {
    let r = open("臺7灣12體123中", "", &VERTICAL);
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    assert_upright(&f, &g, FIRST, 0, '臺');
    assert_number(&f, &g, FIRST, 1, "7");
    assert_upright(&f, &g, FIRST, 2, '灣');
    assert_number(&f, &g, FIRST, 3, "12");
    assert_upright(&f, &g, FIRST, 4, '體');
    let font = FontSet::for_size(2).font(Style::Regular);
    let mut y = g.em_top(5);
    for c in "123".chars() {
        assert_sideways_digit(&f, &g, FIRST, y, c);
        y += i32::from(font.advance(c));
    }
    assert!(is_ink(
        &f,
        g.em_left(FIRST),
        y + em_ascent(PX) - 3,
        8,
        &rows(PX, '中'),
        3
    ));
}

#[test]
fn a_short_number_moves_whole_to_the_next_column_or_page() {
    let sample = open("臺", "", &VERTICAL);
    let rows_per_column = usize::from(sample.text_area_h() / PX);
    for columns in [1, sample.max_lines() - FIRST] {
        let prefix = "臺".repeat(rows_per_column * columns);
        let mut r = open(&format!("{prefix}12灣"), "", &VERTICAL);
        if columns > 1 {
            r.press(Action::Next);
            r.prepare_render();
            assert_eq!(r.page(), 1);
        }
        let column = if columns == 1 { FIRST + 1 } else { 0 };
        assert!(text_lines(&r)[column].starts_with("12灣"));
        let g = Grid::of(&r);
        let f = render_full(&|s| r.draw(s)).frame;
        assert_number(&f, &g, column, 0, "12");
        assert_upright(&f, &g, column, 1, '灣');
    }
}

#[test]
fn the_tail_of_a_long_number_stays_sideways_across_columns_and_pages() {
    let sample = open("臺", "", &VERTICAL);
    let rows_per_column = usize::from(sample.text_area_h() / PX);
    let digit_advance = FontSet::for_size(2).advance('1', Style::Regular);
    let remaining = usize::from(sample.text_area_h()) - (rows_per_column - 1) * usize::from(PX);
    let fitting = remaining / usize::from(digit_advance);
    for columns in [1, sample.max_lines() - FIRST] {
        let prefix = "臺".repeat(rows_per_column * columns - 1);
        let digits = "1".repeat(fitting + 2);
        assert!(digits.len() >= 3);
        let mut r = open(&format!("{prefix}{digits}灣"), "", &VERTICAL);
        if columns > 1 {
            r.press(Action::Next);
            r.prepare_render();
            assert_eq!(r.page(), 1);
        }
        let column = if columns == 1 { FIRST + 1 } else { 0 };
        assert!(text_lines(&r)[column].starts_with("11灣"));
        let g = Grid::of(&r);
        let f = render_full(&|s| r.draw(s)).frame;
        assert_sideways_digit(&f, &g, column, g.top, '1');
        assert_sideways_digit(&f, &g, column, g.top + i32::from(digit_advance), '1');
        assert!(is_ink(
            &f,
            g.em_left(column),
            g.top + 2 * i32::from(digit_advance) + em_ascent(PX) - 3,
            8,
            &rows(PX, '灣'),
            3
        ));
        let stitched = render_stitched(&|s| r.draw(s)).frame;
        assert_eq!(f.to_pbm(), stitched.to_pbm());
        if columns > 1 {
            r.press(Action::Prev);
            r.press(Action::Next);
            r.prepare_render();
            assert_eq!(f.to_pbm(), render_full(&|s| r.draw(s)).frame.to_pbm());
        }
    }
}

#[test]
fn short_number_detection_is_bounded_to_complete_ascii_runs() {
    for (text, at, continued, want) in [
        ("7", 0, false, 1),
        ("12年", 0, false, 2),
        ("123", 0, false, 0),
        ("1234", 2, false, 0),
        ("12", 0, true, 0),
        ("１２", 0, false, 0),
        ("A12B", 1, false, 2),
        ("1.2", 2, false, 1),
        ("", 0, false, 0),
    ] {
        assert_eq!(
            short_number_len(text.as_bytes(), at, continued),
            want,
            "{text:?}"
        );
    }
    assert_eq!(short_number_len(&vec![b'1'; 100_000], 0, false), 0);
}

#[test]
fn horizontal_numbers_keep_their_original_glyphs_and_advances() {
    let r = open("臺12灣", "", &Writing::LATIN);
    let fonts = FontSet::for_size(2);
    let x = i32::from(r.text_margin()) + i32::from(PX) + 1;
    let baseline = i32::from(r.text_y())
        + FIRST as i32 * i32::from(r.font_line_h())
        + i32::from(r.font_ascent());
    let expected = render_full(&|s| {
        fonts.draw_str(s, "12", Style::Regular, x, baseline);
    })
    .frame;
    let actual = render_full(&|s| r.draw(s)).frame;
    let width = i32::from(fonts.advance('1', Style::Regular))
        + i32::from(fonts.advance('2', Style::Regular));
    for y in baseline - i32::from(PX)..baseline + 1 {
        for x in x..x + width {
            assert_eq!(
                actual.is_black(x as u16, y as u16),
                expected.is_black(x as u16, y as u16)
            );
        }
    }
    assert!(is_ink(
        &actual,
        x + width,
        baseline - 3,
        8,
        &rows(PX, '灣'),
        3
    ));
}

#[test]
fn numbers_in_ruby_keep_the_groups_existing_scalar_layout() {
    let blocks = vec![Block::Paragraph(vec![
        Run::Ruby {
            base: "12".into(),
            reading: "臺".into(),
        },
        Run::Text("灣".into()),
    ])];
    let r = open_book(&epub(blocks, vec![], &VERTICAL), "");
    let g = Grid::of(&r);
    let f = render_full(&|s| r.draw(s)).frame;
    let font = FontSet::for_size(2).font(Style::Regular);
    let mut y = g.top;
    for c in "12".chars() {
        assert_sideways_digit(&f, &g, FIRST, y, c);
        y += i32::from(font.advance(c));
    }
    assert!(is_ink(
        &f,
        g.em_left(FIRST),
        y + em_ascent(PX) - 3,
        8,
        &rows(PX, '灣'),
        3
    ));
}

#[test]
fn a_persisted_page_index_remembers_a_long_numbers_continuation() {
    let sample = open("臺", "", &VERTICAL);
    let rows = usize::from(sample.text_area_h() / PX);
    let columns = sample.max_lines() - FIRST;
    let digit_advance = FontSet::for_size(2).advance('1', Style::Regular);
    let remaining = usize::from(sample.text_area_h()) - (rows - 1) * usize::from(PX);
    let digits = "1".repeat(remaining / usize::from(digit_advance) + 2);
    let text = format!("{}{digits}灣", "臺".repeat(rows * columns - 1));
    let mut first = open(&text, "", &VERTICAL);
    first.exit();
    first.set_profile(RingConfig::HR8, LruConfig::OFF, true);
    first.open(BOOK);
    assert_eq!(first.index_counts().1, 1, "the index was stored");
    first.press(Action::Next);
    first.prepare_render();
    assert_eq!(first.page(), 1);
    let expected = render_full(&|s| first.draw(s)).frame;
    first.exit();
    let mut again = Rig::new(first.into_storage());
    again.configure(2, 0);
    again.set_profile(RingConfig::HR8, LruConfig::OFF, true);
    again.open(BOOK);
    assert_eq!(again.index_counts().0, 1, "the stored index was loaded");
    again.press(Action::Next);
    again.prepare_render();
    assert_eq!(again.page(), 1);
    assert_eq!(
        expected.to_pbm(),
        render_full(&|s| again.draw(s)).frame.to_pbm()
    );
}

// a portrait picture, wider than the box an inline image is given
fn picture(path: &str) -> ImageSpec {
    ImageSpec {
        path: path.into(),
        kind: ImageKind::PngGray1,
        width: 512,
        height: 768,
        pattern: Pattern::Black,
    }
}

fn paragraph(t: &str) -> Block {
    Block::Paragraph(vec![Run::Text(t.into())])
}

// The book as the reader sees it from its second visit on: the images are
// cached, so the first page it shows has its picture decoded.
fn open_warm(bytes: &[u8]) -> Rig {
    let mut r = open_book(bytes, "");
    for _ in 0..400 {
        if !r.has_bg_work() {
            break;
        }
        r.idle(1);
    }
    assert!(!r.has_bg_work(), "the book is cached");
    r.exit();
    r.open(BOOK);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    r
}

// columns an inline image reserves, and the box it is decoded to fit
fn image_columns(r: &Rig) -> usize {
    (r.text_w() * 60 / 100).div_ceil(u32::from(r.font_line_h())) as usize
}

// (first column, columns) of the images on the page
fn image_runs(r: &Rig) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = vec![];
    for (i, l) in r.line_infos().iter().enumerate() {
        if l.image_origin {
            runs.push((i, 1));
        } else if l.image {
            runs.last_mut()
                .expect("a reserved column follows its image")
                .1 += 1;
        }
    }
    runs
}

#[test]
fn an_image_in_a_vertical_book_reserves_columns_and_the_text_around_it_stays() {
    let with = epub(
        vec![paragraph("臺灣"), Block::Image(0), paragraph("繁體")],
        vec![picture("images/a.png")],
        &VERTICAL,
    );
    let without = epub(
        vec![paragraph("臺灣"), paragraph("繁體")],
        vec![],
        &VERTICAL,
    );
    let with = open_book(&with, "");
    let without = open_book(&without, "");
    assert!(with.vertical());
    let n = image_columns(&with);
    assert!(n >= 2, "an inline image is wider than a column");
    let runs = image_runs(&with);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1, n);
    // the text of the chapter is the same: what the image takes from a page
    // goes on to the next
    let text = |mut r: Rig| -> Vec<String> {
        let mut all = vec![];
        loop {
            let infos = r.line_infos();
            all.extend(
                text_lines(&r)
                    .into_iter()
                    .zip(infos)
                    .filter(|(t, l)| !l.image && !t.is_empty())
                    .map(|(t, _)| t),
            );
            let before = r.page();
            r.press(Action::Next);
            r.prepare_render();
            if r.page() == before {
                return all;
            }
        }
    };
    assert_eq!(text(with), text(without));
}

#[test]
fn a_decoded_image_is_drawn_in_the_columns_it_reserved() {
    // the first page of an open defers its image, so the picture is on a later one
    let bytes = epub(
        vec![
            paragraph(&"臺灣繁體中文".repeat(200)),
            Block::Image(0),
            paragraph("繁體"),
        ],
        vec![picture("images/a.png")],
        &VERTICAL,
    );
    let mut r = open_warm(&bytes);
    while image_runs(&r).is_empty() {
        let before = r.page();
        r.press(Action::Next);
        r.prepare_render();
        assert_ne!(r.page(), before, "the book has an image page");
    }
    let (w, h) = r.page_image().expect("the page's image is decoded");
    // it fits the box of an inline image: 60% of the text width, a whole column
    assert!(u32::from(w) <= r.text_w() * 60 / 100, "width {w}");
    assert!(h <= r.text_area_h(), "height {h}");
    let g = Grid::of(&r);
    let (at, n) = image_runs(&r)[0];
    let (left, width) = (g.right - g.pitch * (at + n) as i32, g.pitch * n as i32);
    let x = left + (width - i32::from(w)) / 2;
    let y = g.top + (i32::from(r.text_area_h()) - i32::from(h)) / 2;
    let f = render_full(&|s| r.draw(s)).frame;
    // the picture is black: its rectangle is all ink, and the columns of the
    // text on either side hold none of it
    let black = |x0: i32, y0: i32, x1: i32, y1: i32| {
        (x0..x1)
            .flat_map(|x| (y0..y1).map(move |y| (x, y)))
            .filter(|&(x, y)| f.is_black(x as u16, y as u16))
            .count()
    };
    let area = usize::from(w) * usize::from(h);
    assert_eq!(black(x, y, x + i32::from(w), y + i32::from(h)), area);
    assert_eq!(black(left, g.top, x, g.top + i32::from(r.text_area_h())), 0);
    assert_eq!(
        black(
            x + i32::from(w),
            g.top,
            left + width,
            g.top + i32::from(r.text_area_h())
        ),
        0
    );
    let stitched = render_stitched(&|s| r.draw(s)).frame;
    assert_eq!(f.to_pbm(), stitched.to_pbm(), "strips and bands agree");
}

#[test]
fn an_image_is_never_cut_by_the_page_edge_and_a_page_holds_one() {
    let long = "臺灣繁體中文".repeat(40);
    let mut blocks = vec![];
    for i in 0..5 {
        blocks.push(paragraph(&long[..long.len() / (1 + i % 3)]));
        blocks.push(Block::Image(i));
        blocks.push(Block::Image((i + 1) % 5));
    }
    let images = (0..5)
        .map(|i| picture(&format!("images/{i}.png")))
        .collect();
    let mut r = open_book(&epub(blocks, images, &VERTICAL), "");
    let n = image_columns(&r);
    let mut seen = 0;
    for _ in 0..r.total_pages().max(1) * 2 {
        let runs = image_runs(&r);
        assert!(runs.len() <= 1, "page {}: {runs:?}", r.page());
        for &(at, len) in &runs {
            assert_eq!(len, n, "page {}: the image is whole", r.page());
            assert!(at + len <= r.max_lines());
        }
        seen += runs.len();
        let before = r.page();
        r.press(Action::Next);
        r.prepare_render();
        if r.page() == before {
            break;
        }
    }
    assert_eq!(seen, 10, "every image of the book has its page");
}

#[test]
fn closing_brackets_and_stops_still_never_start_a_column() {
    let text = "臺灣繁體中文，".repeat(120);
    let r = open(&text, "，", &VERTICAL);
    cjk_support::assert_kinsoku(&text_lines(&r));
}

#[test]
fn policy_ideographs_are_upright_brackets_have_forms_and_marks_turn() {
    assert!(matches!(setting('臺'), Setting::Upright));
    assert!(matches!(setting('あ'), Setting::Upright));
    assert!(matches!(setting('ー'), Setting::Turned));
    assert!(matches!(setting('～'), Setting::Turned));
    assert!(matches!(
        setting('「'),
        Setting::Form {
            form: '\u{FE41}',
            turn_if_absent: true
        }
    ));
    assert!(matches!(
        setting('，'),
        Setting::Form {
            form: '\u{FE10}',
            turn_if_absent: false
        }
    ));
    assert_eq!(presentation_form('」'), Some('\u{FE42}'));
    assert_eq!(presentation_form('臺'), None);
    assert_eq!(presentation_form('A'), None);
}

#[test]
fn policy_every_form_is_a_distinct_vertical_presentation_scalar() {
    let sources = "，、。：；…‥—–（）｛｝〔〕【】《》〈〉「」『』";
    let mut forms: Vec<char> = sources
        .chars()
        .map(|c| presentation_form(c).unwrap_or_else(|| panic!("{c} has no form")))
        .collect();
    assert!(
        forms
            .iter()
            .all(|&f| matches!(f as u32, 0xFE10..=0xFE19 | 0xFE30..=0xFE48))
    );
    forms.sort_unstable();
    let n = forms.len();
    forms.dedup();
    assert_eq!(forms.len(), n);
}

#[test]
fn policy_em_box_geometry() {
    // the baseline is seven eighths of the em below its top, rounded
    assert_eq!(em_ascent(16), 14);
    assert_eq!(em_ascent(23), 20);
    assert_eq!(em_ascent(35), 31);
    // a turned glyph keeps its baseline em_ascent from the box edge; the
    // top-left of a 3-row glyph with bearing (0, -3) in a 23 px box at (100, 200)
    assert_eq!(
        turned_origin(100, 200, 23, 0, -3, 3),
        (100 + 23 - 20 + 3 - 3, 200)
    );
    // its horizontal bearing becomes the offset down the column
    assert_eq!(turned_origin(0, 0, 23, 5, -3, 3).1, 5);
}

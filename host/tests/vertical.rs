// Vertical writing: the package declares it, the reader sets text in columns.
// Layout and drawing are the production ReaderApp's; the packs are the literal
// ones of `cjk_support` (every glyph an 8x3 pattern, advance px + 1), so a
// column's geometry can be read off the page pixel for pixel.
mod cjk_support;

use cjk_support::{BODY, SIZES, install, pack_of, rows, text_lines};
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, Run,
    TocItem, Writing, build_epub_writing,
};
use pulp_host::fonts::vertical::{Setting, em_ascent, presentation_form, setting, turned_origin};
use pulp_host::reader::{Phase, Rig};
use pulp_host::render::{Framebuffer, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;
use smol_epub::html_strip::{IMG_REF, MARKER};

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
        &"臺灣繁體中文「」，".repeat(60),
        "「」，\u{FE41}",
        &VERTICAL,
    );
    let full = render_full(&|s| r.draw(s)).frame;
    let stitched = render_stitched(&|s| r.draw(s)).frame;
    assert_eq!(full.to_pbm(), stitched.to_pbm());
    assert!(full.black_count() > 0);
}

#[test]
fn an_image_in_a_vertical_book_reserves_nothing_and_the_text_around_it_stays() {
    let image = ImageSpec {
        path: "images/a.png".into(),
        kind: ImageKind::PngGray1,
        width: 64,
        height: 64,
        pattern: Pattern::Black,
    };
    let paragraph = |t: &str| Block::Paragraph(vec![Run::Text(t.into())]);
    let with = epub(
        vec![paragraph("臺灣"), Block::Image(0), paragraph("繁體")],
        vec![image],
        &VERTICAL,
    );
    let without = epub(
        vec![paragraph("臺灣"), paragraph("繁體")],
        vec![],
        &VERTICAL,
    );
    let with = open_book(&with, "");
    assert!(with.vertical());
    assert!(with.line_infos().iter().all(|l| !l.image));
    // the reference stays in the text as a column of its own, which is blank:
    // its path (Latin text) is not drawn
    let lines = with.lines();
    let at = lines
        .iter()
        .position(|l| l.starts_with(&[MARKER, IMG_REF]))
        .expect("the image reference is in the page text");
    let g = Grid::of(&with);
    let f = render_full(&|s| with.draw(s)).frame;
    let (x0, x1) = (g.center(at) - g.pitch / 2, g.center(at) + g.pitch / 2);
    let ink = (x0..x1)
        .flat_map(|x| (0..800).map(move |y| (x, y)))
        .filter(|&(x, y)| f.is_black(x as u16, y))
        .count();
    assert_eq!(ink, 0, "column {at} is blank");
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

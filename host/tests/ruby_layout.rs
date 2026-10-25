// Ruby readings in the production reader: a book with `<ruby>` elements is laid out and drawn by
// the real ReaderApp over literal packs (every glyph an 8x3 pattern, advance px + 1), so the
// position of a reading is read off the page pixel for pixel. A reading is set in the ruby pack
// of the book font size (12 px for size 2), beside the base text set in the body pack (23 px).
mod cjk_support;

use cjk_support::{BODY, SIZES, install, pack_of, rows};
use pulp_board_logic::chapter_ring::RingConfig;
use pulp_board_logic::image_lru::LruConfig;
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, Writing, build_epub_writing,
};
use pulp_host::fonts::cjk::RUBY_PIXELS;
use pulp_host::fonts::ruby;
use pulp_host::fonts::vertical::em_ascent;
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{Framebuffer, render_full};
use pulp_host::storage::VirtualStorage;

const BOOK: &str = "R.EPUB";
const PX: u16 = BODY[2];
const RUBY_PX: u16 = RUBY_PIXELS[2];
// advance of a glyph of these literal packs: px + 1
const ADV: u32 = PX as u32 + 1;
const RUBY_ADV: u32 = RUBY_PX as u32 + 1;
const HORIZONTAL: Writing<'static> = Writing {
    language: "ja",
    page_progression: None,
};
const VERTICAL: Writing<'static> = Writing {
    language: "ja",
    page_progression: Some("rtl"),
};
// the chapter's heading is line 0 and the blank line between blocks line 1
const FIRST: usize = 2;

fn ruby_run(base: &str, reading: &str) -> Run {
    Run::Ruby {
        base: base.into(),
        reading: reading.into(),
    }
}

fn text(s: &str) -> Run {
    Run::Text(s.into())
}

fn epub(runs: Vec<Run>, writing: &Writing<'_>) -> Vec<u8> {
    build_epub_writing(
        &EpubSpec {
            version: EpubVersion::V3,
            title: "ルビ".into(),
            author: "Fixture".into(),
            identifier: "ruby-fixture".into(),
            chapters: vec![Chapter {
                title: "章".into(),
                blocks: vec![Block::Paragraph(runs)],
            }],
            toc: vec![TocItem {
                title: "章".into(),
                chapter: 0,
                children: vec![],
            }],
            images: vec![],
            cover: None,
            compression: Compression::Stored,
            numeric_entities: false,
        },
        writing,
    )
    .unwrap()
}

fn alphabet() -> Vec<char> {
    let mut c: Vec<char> = "臺灣繁體中文章かんじほ".chars().collect();
    c.sort_unstable();
    c.dedup();
    c
}

struct Opened {
    ruby: bool,
    pack: bool,
    width: Option<u32>,
}

fn open_with(bytes: &[u8], o: Opened) -> Rig {
    let card = VirtualStorage::memory_with(&[(BOOK, bytes)]);
    card.ensure_pulp_dir().unwrap();
    for px in SIZES {
        install(&card, px, &pack_of(px, false, alphabet()));
    }
    if o.pack {
        for px in RUBY_PIXELS {
            install(&card, px, &pack_of(px, false, alphabet()));
        }
    }
    let mut r = Rig::new(card);
    r.configure(2, 0);
    r.set_ruby(o.ruby);
    if let Some(width) = o.width {
        r.set_text_width(width);
    }
    r.open(BOOK);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    r
}

fn open(runs: Vec<Run>, writing: &Writing<'_>, ruby: bool) -> Rig {
    open_with(
        &epub(runs, writing),
        Opened {
            ruby,
            pack: true,
            width: None,
        },
    )
}

fn frame(r: &Rig) -> Framebuffer {
    render_full(&|s| r.draw(s)).frame
}

fn is_ink(f: &Framebuffer, x: i32, y: i32, px: u16, c: char) -> bool {
    let bm = rows(px, c);
    (0..3).all(|dy| {
        (0..8).all(|dx| {
            let ink = bm[dy] & (0x80 >> dx) != 0;
            f.is_black((x + dx) as u16, (y + dy as i32) as u16) == ink
        })
    })
}

// the page's lines with every ruby group spelled `[base|reading]`
fn lines(r: &Rig) -> Vec<String> {
    r.lines()
        .into_iter()
        .map(|line| {
            let mut out = Vec::new();
            let mut i = 0;
            while i < line.len() {
                if line[i] == 1 && i + 1 < line.len() {
                    match line[i + 1] {
                        b'R' => out.push(b'['),
                        b'T' => out.push(b'|'),
                        b'r' => out.push(b']'),
                        _ => {}
                    }
                    i += 2;
                } else {
                    out.push(line[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).unwrap()
        })
        .collect()
}

// Row i of a horizontal page: its top, and the baseline the body text sits on.
struct Rows {
    left: i32,
    top: i32,
    pitch: i32,
    ascent: i32,
}

impl Rows {
    fn of(r: &Rig) -> Self {
        Rows {
            left: i32::from(r.text_margin()),
            top: i32::from(r.text_y()),
            pitch: i32::from(r.font_line_h()),
            ascent: i32::from(r.font_ascent()),
        }
    }
    fn line_top(&self, line: usize) -> i32 {
        self.top + self.pitch * line as i32
    }
    // top of the 3 rows of ink of a body glyph (bearing -3 from the baseline)
    fn base_y(&self, line: usize) -> i32 {
        self.line_top(line) + self.ascent - 3
    }
    // the same of a reading: its em box is the top of the line
    fn reading_y(&self, line: usize) -> i32 {
        self.line_top(line) + em_ascent(RUBY_PX) - 3
    }
}

#[test]
fn readings_asked_for_make_every_line_tall_enough_for_them() {
    let runs = vec![ruby_run("臺", "ほ")];
    let off = open(runs.clone(), &HORIZONTAL, false);
    let on = open(runs, &HORIZONTAL, true);
    assert!(off.font_line_h() < ruby::min_pitch(PX, RUBY_PX));
    assert_eq!(on.font_line_h(), ruby::min_pitch(PX, RUBY_PX));
    assert!(on.max_lines() < off.max_lines());
    // the text sits lower in the line by the room the readings take
    assert_eq!(
        on.font_ascent(),
        off.font_ascent() + ruby::room(RUBY_PX),
        "the room is above the base"
    );
}

#[test]
fn a_reading_is_set_centred_above_its_base() {
    let r = open(vec![ruby_run("臺", "ほ")], &HORIZONTAL, true);
    let g = Rows::of(&r);
    let f = frame(&r);
    assert_eq!(lines(&r)[FIRST], "[臺|ほ]");
    // the base fills the group (24 wide), the reading (13 wide) is centred on it
    assert!(is_ink(&f, g.left, g.base_y(FIRST), PX, '臺'), "base");
    let at = g.left + ((ADV - RUBY_ADV) / 2) as i32;
    assert!(
        is_ink(&f, at, g.reading_y(FIRST), RUBY_PX, 'ほ'),
        "reading at x={at}, y={}",
        g.reading_y(FIRST)
    );
}

#[test]
fn the_longer_part_of_a_group_sets_its_length_and_the_text_goes_on_after_it() {
    // a reading of 4 glyphs is 52 wide, the base 24: the base is centred under it
    let r = open(
        vec![ruby_run("臺", "かんじほ"), text("灣")],
        &HORIZONTAL,
        true,
    );
    let g = Rows::of(&r);
    let f = frame(&r);
    assert_eq!(lines(&r)[FIRST], "[臺|かんじほ]灣");
    let group = 4 * RUBY_ADV;
    assert!(is_ink(
        &f,
        g.left + ((group - ADV) / 2) as i32,
        g.base_y(FIRST),
        PX,
        '臺'
    ));
    for (i, c) in "かんじほ".chars().enumerate() {
        assert!(
            is_ink(
                &f,
                g.left + (i as u32 * RUBY_ADV) as i32,
                g.reading_y(FIRST),
                RUBY_PX,
                c
            ),
            "reading glyph {i}"
        );
    }
    assert!(
        is_ink(&f, g.left + group as i32, g.base_y(FIRST), PX, '灣'),
        "the text after the group starts where it ends"
    );
}

#[test]
fn a_group_whose_base_is_longer_than_its_reading_centres_the_reading() {
    let r = open(vec![ruby_run("臺灣體", "かん")], &HORIZONTAL, true);
    let g = Rows::of(&r);
    let f = frame(&r);
    let group = 3 * ADV;
    let reading = 2 * RUBY_ADV;
    for (i, c) in "臺灣體".chars().enumerate() {
        assert!(is_ink(
            &f,
            g.left + (i as u32 * ADV) as i32,
            g.base_y(FIRST),
            PX,
            c
        ));
    }
    let at = g.left + ((group - reading) / 2) as i32;
    assert!(is_ink(&f, at, g.reading_y(FIRST), RUBY_PX, 'か'));
    assert!(is_ink(
        &f,
        at + RUBY_ADV as i32,
        g.reading_y(FIRST),
        RUBY_PX,
        'ん'
    ));
}

#[test]
fn a_group_is_never_split_by_a_line_break() {
    // ten glyphs fill a line of 10 x 24; nine plain ones leave room for one more, not for
    // a group of two
    let bytes = epub(
        vec![text(&"臺".repeat(9)), ruby_run("灣體", "かん"), text("中")],
        &HORIZONTAL,
    );
    let r = open_with(
        &bytes,
        Opened {
            ruby: true,
            pack: true,
            width: Some(10 * ADV),
        },
    );
    let l = lines(&r);
    assert_eq!(
        l[FIRST], "臺臺臺臺臺臺臺臺臺",
        "the group moves to the next line whole"
    );
    assert_eq!(l[FIRST + 1], "[灣體|かん]中");
}

#[test]
fn a_group_wider_than_the_remaining_line_because_of_its_reading_moves_whole() {
    // the base is one glyph (it would fit) but its reading of 4 is 52 wide (it does not)
    let bytes = epub(
        vec![
            text(&"臺".repeat(9)),
            ruby_run("灣", "かんじほ"),
            text("中"),
        ],
        &HORIZONTAL,
    );
    let r = open_with(
        &bytes,
        Opened {
            ruby: true,
            pack: true,
            width: Some(10 * ADV),
        },
    );
    let l = lines(&r);
    assert_eq!(l[FIRST], "臺臺臺臺臺臺臺臺臺");
    assert_eq!(l[FIRST + 1], "[灣|かんじほ]中");
}

#[test]
fn readings_not_asked_for_are_not_drawn_and_take_no_room() {
    let with = open(
        vec![text("臺"), ruby_run("灣", "ほ"), text("體")],
        &HORIZONTAL,
        false,
    );
    let without = open(vec![text("臺灣體")], &HORIZONTAL, false);
    assert_eq!(with.font_line_h(), without.font_line_h());
    assert_eq!(with.font_ascent(), without.font_ascent());
    assert_eq!(
        frame(&with).to_pbm(),
        frame(&without).to_pbm(),
        "the page is the one of the plain text"
    );
}

#[test]
fn readings_without_their_pack_are_left_out_of_the_layout_and_the_page() {
    let runs = vec![text("臺"), ruby_run("灣", "かんじほ"), text("體")];
    let bytes = epub(runs, &HORIZONTAL);
    let r = open_with(
        &bytes,
        Opened {
            ruby: true,
            pack: false,
            width: None,
        },
    );
    let plain = open_with(
        &epub(vec![text("臺灣體")], &HORIZONTAL),
        Opened {
            ruby: true,
            pack: false,
            width: None,
        },
    );
    assert_eq!(r.font_line_h(), plain.font_line_h());
    assert_eq!(
        frame(&r).to_pbm(),
        frame(&plain).to_pbm(),
        "no reading is drawn, none lengthens the line"
    );
}

#[test]
fn every_group_of_a_long_chapter_is_laid_out_whole_on_exactly_one_page() {
    let mut runs = Vec::new();
    for _ in 0..300 {
        runs.push(text("中文"));
        runs.push(ruby_run("臺灣", "かん"));
    }
    let mut r = open(runs, &HORIZONTAL, true);
    let mut groups = 0;
    loop {
        for line in lines(&r) {
            let (open, bar, close) = (
                line.matches('[').count(),
                line.matches('|').count(),
                line.matches(']').count(),
            );
            assert_eq!((open, bar), (close, close), "{line:?}");
            groups += close;
        }
        let last = r.page() + 1 >= r.total_pages();
        if last {
            break;
        }
        r.press(Action::Next);
        r.prepare_render();
    }
    assert!(r.total_pages() > 3, "the chapter spans pages");
    assert_eq!(groups, 300);
}

fn card_with_packs(bytes: &[u8]) -> VirtualStorage {
    let card = VirtualStorage::memory_with(&[(BOOK, bytes)]);
    card.ensure_pulp_dir().unwrap();
    for px in SIZES.into_iter().chain(RUBY_PIXELS) {
        install(&card, px, &pack_of(px, false, alphabet()));
    }
    card
}

#[test]
fn turning_ruby_on_while_a_book_is_open_lays_it_out_again() {
    let mut r = open(vec![ruby_run("臺", "ほ")], &HORIZONTAL, false);
    let pitch = r.font_line_h();
    let g = Rows::of(&r);
    let at = g.left + ((ADV - RUBY_ADV) / 2) as i32;
    assert!(
        !is_ink(&frame(&r), at, g.reading_y(FIRST), RUBY_PX, 'ほ'),
        "no reading yet"
    );

    // Settings -> Ruby -> back to the reader
    r.suspend();
    r.set_ruby(true);
    r.resume();
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    assert!(r.font_line_h() > pitch, "the lines take room for readings");
    let g = Rows::of(&r);
    assert!(
        is_ink(&frame(&r), at, g.reading_y(FIRST), RUBY_PX, 'ほ'),
        "the reading is set"
    );

    // and off again
    r.suspend();
    r.set_ruby(false);
    r.resume();
    r.prepare_render();
    assert_eq!(r.font_line_h(), pitch);
}

#[test]
fn a_stored_page_index_serves_only_the_layout_it_was_made_for() {
    let mut runs = Vec::new();
    for _ in 0..200 {
        runs.push(text("中文臺灣"));
        runs.push(ruby_run("體", "か"));
    }
    let bytes = epub(runs, &HORIZONTAL);
    let open_book = |card: VirtualStorage, ruby: bool| {
        let mut r = Rig::new(card);
        r.configure(2, 0);
        r.set_ruby(ruby);
        r.set_profile(RingConfig::HR8, LruConfig::OFF, true);
        r.open(BOOK);
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Ready);
        r
    };

    let mut plain = open_book(card_with_packs(&bytes), false);
    assert_eq!(
        plain.index_counts(),
        (0, 1, 0),
        "stored for the plain layout"
    );
    let plain_pages = plain.page_offsets();
    plain.exit();

    let mut again = open_book(plain.into_storage(), false);
    assert_eq!(again.index_counts(), (1, 0, 0), "and served to it");
    again.exit();

    let mut with = open_book(again.into_storage(), true);
    assert_eq!(with.index_counts().0, 0, "the plain index is not served");
    assert_ne!(
        with.page_offsets(),
        plain_pages,
        "the pages are other pages"
    );
    with.exit();

    let again = open_book(with.into_storage(), true);
    assert_eq!(
        again.index_counts().0,
        1,
        "the layout with readings has its own"
    );
}

// where column `i` of a vertical page has its right edge and the top of its glyphs
struct Columns {
    right: i32,
    pitch: i32,
    top: i32,
}

impl Columns {
    fn of(r: &Rig) -> Self {
        let pitch = i32::from(r.font_line_h());
        let grid_w = pitch * r.max_lines() as i32;
        Columns {
            right: i32::from(r.text_margin()) + (r.text_w() as i32 + grid_w) / 2,
            pitch,
            top: i32::from(r.text_y()),
        }
    }
    fn col_right(&self, column: usize) -> i32 {
        self.right - self.pitch * column as i32
    }
}

#[test]
fn in_a_column_a_reading_is_set_right_of_its_base() {
    let r = open(vec![ruby_run("臺", "ほ")], &VERTICAL, true);
    assert!(r.vertical());
    assert_eq!(r.font_line_h(), ruby::min_pitch(PX, RUBY_PX));
    let c = Columns::of(&r);
    let f = frame(&r);
    assert_eq!(lines(&r)[FIRST], "[臺|ほ]");
    let (base_c, reading_c) =
        ruby::column_centers(c.col_right(FIRST), r.font_line_h(), PX, RUBY_PX);
    // upright glyphs: the em box hangs from `center - px / 2`, the baseline 7/8 down it; a
    // glyph advances one em down a column, and the shorter part of the group (the reading,
    // 12 against 23) is centred along it
    let base_x = base_c - i32::from(PX) / 2;
    let base_y = c.top + em_ascent(PX) - 3;
    assert!(is_ink(&f, base_x, base_y, PX, '臺'), "base");
    let reading_x = reading_c - i32::from(RUBY_PX) / 2;
    let reading_y = c.top + i32::from(PX - RUBY_PX) / 2 + em_ascent(RUBY_PX) - 3;
    assert!(
        is_ink(&f, reading_x, reading_y, RUBY_PX, 'ほ'),
        "reading at ({reading_x}, {reading_y})"
    );
}

// Unicode scalars at layout, page, read and draw boundaries: TXT through the real
// ReaderApp (proportional layout and the monospace layout), with the boundary moved over
// every byte alignment by padding the text with ASCII.
//
// Run: cargo test-host --test utf8_boundary
//
// Oracle: RFC 3629 (a scalar is 1..=4 bytes; `str::from_utf8` is the independent judge of
// validity) and the invariants of the text itself: laying a book out must not change its
// text, must not cut a scalar between two lines or two pages, and must not replace a
// scalar of valid text by U+FFFD. Buffer sizes and widths come from the production
// constants (`PAGE_BUF`, `CHARS_PER_LINE`) re-exported by pulp_host::reader. No expected
// value is taken from what the reader currently produces.

mod utf8_common;

use std::collections::BTreeSet;

use pulp_host::reader::{Action, CHARS_PER_LINE, PAGE_BUF, Phase, Rig};
use utf8_common::*;

// a book of at least three read buffers, so reads, page starts and EOF all sit at
// different alignments inside the text
const MIN_BYTES: usize = 3 * PAGE_BUF + 500;
// ASCII padding 0..PADS moves every boundary over every alignment of the 12-byte unit
// (lcm of 2, 3, 4) and then some
const PADS: usize = 26;

fn kinds(min: usize) -> Vec<(&'static str, String)> {
    vec![
        ("e-acute run", repeat_to(E_ACUTE, min)),
        ("tai run", repeat_to(TAI, min)),
        ("yoshi run", repeat_to(YOSHI, min)),
        ("mixed unit run", repeat_to(MIXED_UNIT, min)),
        ("prose", prose(7, min)),
    ]
}

// ---------------------------------------------------------------------------
// proportional layout
// ---------------------------------------------------------------------------

// Every pad value shifts where read buffers, pages and lines fall inside the scalars.
// The reads' own boundaries are recorded: the sweep must have put a read boundary
// inside a 2-, 3- and 4-byte scalar at every inner byte position, or it proved nothing.
#[test]
fn scalars_stay_whole_at_every_pad_offset_in_proportional_layout() {
    let mut cut = BTreeSet::new();
    for (name, text) in kinds(MIN_BYTES) {
        for pad in 0..PADS {
            let data = padded(pad, &text);
            for (font, theme) in CONFIGS {
                let mut r = open_book(&data, font, theme);
                let pages = walk_forward(&mut r);
                let label = format!("{name}, pad {pad}, font {font}, theme {theme}");
                assert!(pages.len() >= 4, "{label}: only {} pages", pages.len());
                check_whole_scalars(&r, &pages, &data, &label);
                for b in read_boundaries(&r) {
                    cut.extend(inner_class(&data, b));
                }
            }
        }
    }
    let missing: Vec<_> = INNER_CLASSES.iter().filter(|c| !cut.contains(c)).collect();
    assert!(
        missing.is_empty(),
        "the sweep never put a read boundary at (scalar length, byte index) {missing:?}"
    );
}

// the book is laid out the same whichever way it is walked: backward and by jumps show
// the pages that walking forward showed, and each page is made of whole scalars
#[test]
fn backward_walk_and_jumps_show_the_same_whole_scalar_pages() {
    for pad in 0..12 {
        let data = padded(pad, &prose(100 + pad as u64, 6 * PAGE_BUF));
        let mut r = open_book(&data, 2, 1);
        let fwd = walk_forward(&mut r);
        let label = format!("pad {pad}");
        check_whole_scalars(&r, &fwd, &data, &label);
        assert!(fwd.len() > 12, "{label}: {} pages", fwd.len());

        let mut back = walk_back(&mut r);
        back.reverse();
        assert_eq!(
            back, fwd,
            "{label}: walking back shows the pages walking forward showed"
        );

        r.press(Action::NextJump);
        let k = r.page();
        assert!(k > 1, "{label}: a jump moves more than one page");
        assert_eq!(snapshot(&r), fwd[k], "{label}: NextJump lands on page {k}");
        r.press(Action::NextJump);
        assert_eq!(
            snapshot(&r),
            fwd[(2 * k).min(fwd.len() - 1)],
            "{label}: second jump"
        );
        r.press(Action::PrevJump);
        assert_eq!(
            snapshot(&r),
            fwd[k],
            "{label}: PrevJump returns to page {k}"
        );
    }
}

// a book reopened at a saved position resumes on a page that is made of whole scalars
// and is the page that was shown when it was saved
#[test]
fn reopening_at_a_saved_position_resumes_on_the_same_whole_scalar_page() {
    for pad in 0..12 {
        let data = padded(pad, &prose(200 + pad as u64, 4 * PAGE_BUF));
        let mut r = open_book(&data, 2, 1);
        let fwd = walk_forward(&mut r);
        let label = format!("pad {pad}");
        for target in [1, fwd.len() / 2, fwd.len() - 1] {
            while r.page() > target {
                r.press(Action::Prev);
            }
            while r.page() < target {
                r.press(Action::Next);
            }
            r.save_position();
            r.bookmarks_flush();
            r.exit();
            let mut again = Rig::new(r.into_storage());
            again.configure(2, 1);
            again.open(BOOK);
            assert_eq!(
                again.phase(),
                Phase::Ready,
                "{label}: reopen at page {target}"
            );
            let shown = snapshot(&again);
            assert_eq!(shown, fwd[target], "{label}: resumed page {target}");
            for l in &shown.lines {
                assert!(
                    std::str::from_utf8(l).is_ok(),
                    "{label}: resumed line is not UTF-8"
                );
            }
            r = again;
        }
    }
}

// a read that returns fewer bytes than asked, ending anywhere (inside a scalar too):
// the reader reports an error or shows a prefix of the book made of whole scalars; the
// half scalar is never shown as garbage
#[test]
fn a_short_read_ending_inside_a_scalar_never_shows_a_half_scalar() {
    let data = prose(300, 2 * PAGE_BUF).into_bytes();
    let healthy = {
        let mut r = open_book(&data, 2, 1);
        let p = walk_forward(&mut r);
        p[1].offset as usize
    };
    let full = strip_ws(&data);
    let cuts = (1..=64)
        .chain(healthy - 12..=healthy + 12)
        .chain(PAGE_BUF - 8..=PAGE_BUF + 8);
    let mut cut_inside = 0;
    for k in cuts {
        let mut r = fresh_rig(&data, 2, 1);
        r.storage().inject_short_read(BOOK, 1, k);
        r.open(BOOK);
        assert_eq!(
            r.storage().pending_injections(),
            0,
            "k {k}: the injection fired"
        );
        if r.phase() == Phase::Error {
            continue;
        }
        assert_eq!(r.phase(), Phase::Ready, "k {k}");
        cut_inside += inner_class(&data, k).is_some() as usize;
        let mut pages = vec![snapshot(&r)];
        for _ in 0..6 {
            let before = r.page();
            r.press(Action::Next);
            if r.phase() != Phase::Ready || r.page() == before {
                break;
            }
            pages.push(snapshot(&r));
        }
        for p in &pages {
            for l in &p.lines {
                let s = std::str::from_utf8(l).unwrap_or_else(|e| {
                    panic!(
                        "k {k}: page {} line is not UTF-8 ({e}): {:?}",
                        p.index,
                        String::from_utf8_lossy(l)
                    )
                });
                assert!(
                    !s.contains(FFFD),
                    "k {k}: page {} shows U+FFFD: {s:?}",
                    p.index
                );
            }
        }
        let shown = strip_ws(&text_of(&all_lines(&pages)));
        assert!(
            full.starts_with(&shown),
            "k {k}: the shown text is not a prefix of the book"
        );
    }
    assert!(
        cut_inside >= 10,
        "only {cut_inside} cuts fell inside a scalar"
    );
}

// ---------------------------------------------------------------------------
// monospace layout
// ---------------------------------------------------------------------------

// the monospace path is the one entered: ASCII (one byte per column) wraps at exactly
// CHARS_PER_LINE columns, which proportional layout of a narrow glyph does not
#[test]
fn monospace_layout_wraps_ascii_at_exactly_chars_per_line() {
    let text = "i".repeat(3 * CHARS_PER_LINE + 7);
    let r = open_mono(text.as_bytes());
    let lens: Vec<usize> = r.lines().iter().map(Vec::len).collect();
    assert_eq!(lens, [CHARS_PER_LINE, CHARS_PER_LINE, CHARS_PER_LINE, 7]);
}

#[test]
fn monospace_layout_keeps_every_scalar_whole_at_every_pad_offset() {
    // 2- and 4-byte scalars never divide the 51-column line evenly, 3-byte ones do:
    // the pad moves the cut over every byte position of every unit
    for (name, text) in kinds(2 * PAGE_BUF) {
        for pad in 0..CHARS_PER_LINE + 12 {
            let data = padded(pad, &text);
            let mut r = open_mono(&data);
            let pages = walk_forward(&mut r);
            let label = format!("monospace {name}, pad {pad}");
            check_whole_scalars(&r, &pages, &data, &label);
            check_mono_width(&pages, &label);
        }
    }
}

// a line of only multi-byte scalars: whatever a column count means for them, the line
// holds more than one scalar, never more than CHARS_PER_LINE, and each is whole
#[test]
fn monospace_layout_of_multibyte_runs_fills_lines_with_whole_scalars() {
    for unit in [E_ACUTE, TAI, YOSHI] {
        let text = unit.repeat(5 * CHARS_PER_LINE);
        let r = open_mono(text.as_bytes());
        let lines = r.lines();
        assert!(lines.len() >= 2, "{unit:?}: {} lines", lines.len());
        for l in &lines[..lines.len() - 1] {
            let s =
                std::str::from_utf8(l).unwrap_or_else(|e| panic!("{unit:?}: line not UTF-8 ({e})"));
            let n = s.chars().count();
            assert!(
                (2..=CHARS_PER_LINE).contains(&n),
                "{unit:?}: {n} scalars on a full line"
            );
            assert!(s.chars().all(|c| c.to_string() == unit), "{unit:?}: {s:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

// ReaderApp::draw draws every scalar of every laid-out line, on each page of the book
#[test]
fn drawn_pages_show_every_scalar_at_every_pad_offset() {
    for (name, text) in kinds(2 * PAGE_BUF) {
        for pad in 0..16 {
            let data = padded(pad, &text);
            for (font, theme) in [(2u8, 1u8), (0, 0)] {
                let mut r = open_book(&data, font, theme);
                let mut pages = 0;
                loop {
                    check_drawn(
                        &r,
                        font,
                        &format!("{name}, pad {pad}, font {font}, page {}", r.page()),
                    );
                    pages += 1;
                    let before = r.page();
                    r.press(Action::Next);
                    if r.page() == before {
                        break;
                    }
                }
                assert!(pages >= 3, "{name}: {pages} pages");
            }
        }
    }
}

// a single line of n scalars, n = 1.. up to a full line: the line is drawn whole whatever
// its byte length (so a fixed-size copy of the line cannot cut it); at the smallest font
// a full line is 56 scalars, up to 224 bytes
#[test]
fn a_line_of_every_length_is_drawn_whole() {
    for (font, theme, max_n) in [(2u8, 1u8, 36), (0, 0, 56)] {
        for unit in [E_ACUTE, TAI, YOSHI] {
            for n in 1..=max_n {
                for pad in 0..4 {
                    let data = padded(pad, &unit.repeat(n));
                    let r = open_book(&data, font, theme);
                    check_drawn(&r, font, &format!("{unit:?} x {n}, pad {pad}, font {font}"));
                }
            }
        }
    }
}

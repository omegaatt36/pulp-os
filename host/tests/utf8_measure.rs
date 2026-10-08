// A malformed subpart counts as one U+FFFD everywhere text is measured or drawn: the
// layout of a book holding bad bytes is the layout of the same book with each maximal
// subpart written as U+FFFD, and so is the picture of its page, in the proportional layout
// and in the plain-font layout (no Regular font data, `Rig::open_monospace`).
//
// Run: cargo test-host --test utf8_measure
//
// Oracle: the replacement policy (one U+FFFD per maximal subpart, as std's
// `from_utf8_lossy` counts them; the hand counts are checked against std below). A book
// with the subpart replaced by U+FFFD is the reference: its lines, scalar for scalar, and
// its page rows must be the ones of the book with the bad bytes. Nothing is read from the
// code under test other than the two books being compared with each other.

mod utf8_common;

use pulp_host::reader::Rig;
use pulp_host::render::render_full;
use pulp_host::utf8::Utf8Iter;
use utf8_common::*;

// (name, bytes, U+FFFD under maximal-subpart replacement; counted by hand)
const BAD: &[(&str, &[u8], usize)] = &[
    ("stray continuation 80", &[0x80], 1),
    ("stray continuation BF", &[0xBF], 1),
    ("two stray continuations", &[0x80, 0x80], 2),
    ("overlong C0 80", &[0xC0, 0x80], 2),
    ("overlong E0 80 80", &[0xE0, 0x80, 0x80], 3),
    ("overlong F0 80 80 80", &[0xF0, 0x80, 0x80, 0x80], 4),
    ("surrogate ED A0 80", &[0xED, 0xA0, 0x80], 3),
    ("above U+10FFFF F4 90 80 80", &[0xF4, 0x90, 0x80, 0x80], 4),
    ("cut 3-byte E8 87", &[0xE8, 0x87], 1),
    ("cut 4-byte F0 A0 AE", &[0xF0, 0xA0, 0xAE], 1),
    ("cut 2-byte C3", &[0xC3], 1),
    ("byte F8", &[0xF8], 1),
    ("byte FF", &[0xFF], 1),
];

#[test]
fn hand_counts_agree_with_std() {
    for (name, seq, n) in BAD {
        assert_eq!(
            String::from_utf8_lossy(seq).matches(FFFD).count(),
            *n,
            "{name}"
        );
    }
}

// the book with the bad bytes at `at`, and the reference with U+FFFD in their place
fn pair(text: &str, at: usize, seq: &[u8], n: usize) -> (Vec<u8>, Vec<u8>) {
    let (head, tail) = text.as_bytes().split_at(at);
    let mut bad = head.to_vec();
    bad.extend_from_slice(seq);
    bad.extend_from_slice(tail);
    let mut reference = head.to_vec();
    reference.extend_from_slice("\u{FFFD}".repeat(n).as_bytes());
    reference.extend_from_slice(tail);
    (bad, reference)
}

fn open_layout(data: &[u8], mono: bool, font: u8, theme: u8) -> Rig {
    let mut r = fresh_rig(data, font, theme);
    if mono {
        r.open_monospace(BOOK);
    } else {
        r.open(BOOK);
    }
    r
}

// every page's lines, each decoded by the production decoder
fn laid_out(r: &mut Rig) -> Vec<Vec<String>> {
    walk_forward(r)
        .iter()
        .map(|p| {
            p.lines
                .iter()
                .map(|l| Utf8Iter::new(l).collect::<String>())
                .collect()
        })
        .collect()
}

// the char boundaries of `text` in its first 160 bytes: wherever the first lines end
fn early_boundaries(text: &str) -> Vec<usize> {
    (0..160).filter(|&i| text.is_char_boundary(i)).collect()
}

fn sweep_layout(mono: bool) {
    let text = prose(41, 5000);
    for (font, theme) in [(2u8, 1u8), (0, 0)] {
        for (name, seq, n) in BAD {
            for at in early_boundaries(&text) {
                let (bad, reference) = pair(&text, at, seq, *n);
                let got = laid_out(&mut open_layout(&bad, mono, font, theme));
                let want = laid_out(&mut open_layout(&reference, mono, font, theme));
                assert_eq!(
                    got, want,
                    "monospace {mono}, font {font}: {name} at byte {at}: layout differs from U+FFFD's"
                );
            }
        }
    }
}

// where each line breaks, and the width a line is measured at, are those of U+FFFD
#[test]
fn bad_bytes_are_laid_out_like_replacement_characters() {
    sweep_layout(false);
}

#[test]
fn bad_bytes_are_laid_out_like_replacement_characters_in_monospace() {
    sweep_layout(true);
}

// a stray continuation byte after a run of ASCII of every length: the line it falls on
// breaks exactly where the line with U+FFFD does (the text is one word per line end, so
// the break moves with a width that is off by one cell)
#[test]
fn a_stray_continuation_byte_measures_as_one_replacement_cell_at_every_line_fill() {
    for (font, theme) in [(2u8, 1u8), (0, 0), (4, 3)] {
        for fill in 0..120usize {
            for word in ["", "w", "\u{81fa}"] {
                let text = format!("{}{}", "a".repeat(fill), word.repeat(3));
                let (bad, reference) = pair(&format!("{text} tail text"), text.len(), &[0x80], 1);
                let got = laid_out(&mut open_layout(&bad, false, font, theme));
                let want = laid_out(&mut open_layout(&reference, false, font, theme));
                assert_eq!(got, want, "font {font}: {fill} x 'a' then {word:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the picture
// ---------------------------------------------------------------------------

// the rows of the text area of the drawn page as bytes of the PBM (1 = ink); the header and
// the status line, which show the file's position, are left out
fn text_rows(r: &Rig) -> Vec<u8> {
    let pbm = render_full(&|s| r.draw(s)).frame.to_pbm();
    let body = &pbm[b"P4\n480 800\n".len()..];
    let row = 480 / 8;
    let (top, h) = (r.text_y() as usize, r.text_area_h() as usize);
    body[top * row..(top + h) * row].to_vec()
}

fn sweep_draw(mono: bool) {
    let text = prose(43, 3000);
    // the first line and its neighbours: bad bytes at the start, middle and end of lines
    for (name, seq, n) in BAD {
        for at in early_boundaries(&text).into_iter().step_by(3) {
            let (bad, reference) = pair(&text, at, seq, *n);
            let got = text_rows(&open_layout(&bad, mono, 2, 1));
            let want = text_rows(&open_layout(&reference, mono, 2, 1));
            assert!(
                want.iter().any(|&b| b != 0),
                "{name} at byte {at}: the reference page draws nothing"
            );
            assert!(
                got == want,
                "monospace {mono}: {name} at byte {at}: the page with bad bytes is not drawn like the page with U+FFFD"
            );
        }
    }
}

// the line holding bad bytes is drawn (it does not vanish, whole or in part), a cell per
// maximal subpart, the valid text around it where it is without them
#[test]
fn a_page_with_bad_bytes_is_drawn_like_the_page_with_replacement_characters() {
    sweep_draw(false);
}

#[test]
fn a_page_with_bad_bytes_is_drawn_like_the_page_with_replacement_characters_in_monospace() {
    sweep_draw(true);
}

// plain-font layout, a line of only valid text and one bad byte in the middle of it
#[test]
fn a_plain_font_line_with_a_bad_byte_is_not_blank() {
    for (name, seq, _) in BAD {
        let mut data = b"before ".to_vec();
        data.extend_from_slice(seq);
        data.extend_from_slice(b" after");
        let r = open_layout(&data, true, 2, 1);
        assert!(
            text_rows(&r).iter().any(|&b| b != 0),
            "{name}: the line vanished"
        );
    }
}

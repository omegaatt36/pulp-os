// Malformed UTF-8 in a TXT book, at every line / page / read position: the reader shows
// U+FFFD for each malformed sequence, never invalid UTF-8, and leaves the valid text
// around it alone.
//
// Run: cargo test-host --test utf8_malformed
//
// Policy: a malformed sequence is replaced when the text is consumed (Utf8Iter,
// decode_utf8_char, drawing), one U+FFFD per maximal subpart (std's
// `String::from_utf8_lossy`); the page buffer keeps the file's raw bytes. So the tests
// look at the page buffer (`page_buffer_*`: the laid-out lines hold the file's bytes and
// each line decodes, through the production decoder, to the lossy text of its bytes), the
// text as displayed (`Utf8Iter` over each line) and the pixels the real draw puts on
// screen (`drawn_*`).
//
// Oracle: RFC 3629 for what is malformed (a stray continuation byte, a lead byte that is
// not a lead byte (C0, C1, F5..FF), an overlong form, a surrogate, a value above U+10FFFF,
// a sequence cut short) and std's `String::from_utf8_lossy` for the replacement count:
// one U+FFFD for each maximal prefix of a valid sequence and one for every other bad
// byte. The hand counts in the table below are checked against std first.

mod utf8_common;

use pulp_host::reader::PAGE_BUF;
use pulp_host::utf8::Utf8Iter;
use utf8_common::*;

// (name, bytes, U+FFFD under maximal-subpart replacement; counted by hand)
const BAD: &[(&str, &[u8], usize)] = &[
    ("stray continuation 80", &[0x80], 1),
    ("stray continuation BF", &[0xBF], 1),
    ("two stray continuations", &[0x80, 0x80], 2),
    ("overlong C0 80", &[0xC0, 0x80], 2),
    ("overlong C1 BF", &[0xC1, 0xBF], 2),
    ("overlong E0 80 80", &[0xE0, 0x80, 0x80], 3),
    ("overlong E0 9F BF", &[0xE0, 0x9F, 0xBF], 3),
    ("overlong F0 80 80 80", &[0xF0, 0x80, 0x80, 0x80], 4),
    ("overlong F0 8F BF BF", &[0xF0, 0x8F, 0xBF, 0xBF], 4),
    ("surrogate ED A0 80", &[0xED, 0xA0, 0x80], 3),
    ("surrogate ED BF BF", &[0xED, 0xBF, 0xBF], 3),
    ("above U+10FFFF F4 90 80 80", &[0xF4, 0x90, 0x80, 0x80], 4),
    ("lead F5 80 80 80", &[0xF5, 0x80, 0x80, 0x80], 4),
    ("cut 2-byte lead C3", &[0xC3], 1),
    ("cut 3-byte E8 87", &[0xE8, 0x87], 1),
    ("cut 3-byte lead E8", &[0xE8], 1),
    ("cut 4-byte F0 A0 AE", &[0xF0, 0xA0, 0xAE], 1),
    ("cut 4-byte F0 A0", &[0xF0, 0xA0], 1),
    ("byte F8", &[0xF8], 1),
    ("byte FB", &[0xFB], 1),
    ("byte FC", &[0xFC], 1),
    ("byte FE", &[0xFE], 1),
    ("byte FF", &[0xFF], 1),
    (
        "five-byte form F8 88 80 80 80",
        &[0xF8, 0x88, 0x80, 0x80, 0x80],
        5,
    ),
];

// the table is the oracle's input: its hand counts must agree with std before they are used
#[test]
fn hand_counts_agree_with_std_maximal_subpart_replacement() {
    for (name, seq, n) in BAD {
        assert!(
            std::str::from_utf8(seq).is_err(),
            "{name}: the sequence is malformed"
        );
        let lossy = String::from_utf8_lossy(seq);
        assert_eq!(lossy.matches(FFFD).count(), *n, "{name}");
        assert_eq!(lossy.chars().count(), *n, "{name}: nothing but U+FFFD");
    }
}

// ---------------------------------------------------------------------------
// the invariants of one book
// ---------------------------------------------------------------------------

fn collapse_runs(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c == FFFD && out.ends_with(FFFD) {
            continue;
        }
        out.push(c);
    }
    out
}

fn without_ws(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\r' | '\n'))
        .collect()
}

fn fffd_in(s: &str) -> usize {
    s.matches(FFFD).count()
}

// one line decoded by the production decoder
fn decode_line(l: &[u8]) -> String {
    Utf8Iter::new(l).collect()
}

// the book as displayed: each line decoded on its own (a line is drawn on its own)
fn displayed(pages: &[Page]) -> String {
    pages
        .iter()
        .flat_map(|p| &p.lines)
        .map(|l| decode_line(l))
        .collect()
}

// The page buffer keeps the file's bytes (laying out does not change the content), and
// each line decodes to the lossy text of that line's own bytes.
fn check_buffer(pages: &[Page], data: &[u8], label: &str) {
    assert_eq!(
        strip_ws(&text_of(&all_lines(pages))),
        strip_ws(data),
        "{label}: the page buffer's bytes differ from the file's"
    );
    for (i, p) in pages.iter().enumerate() {
        for l in &p.lines {
            assert_eq!(
                decode_line(l),
                String::from_utf8_lossy(l),
                "{label}: page {i}: line {l:02X?} decodes differently from its lossy text"
            );
        }
    }
}

// Any replacement policy: `shown` equals the lossy decoding of the file once runs of
// U+FFFD are merged and ASCII space / CR / LF are ignored (the text around the bad bytes
// is intact), and holds between `min` and `max` U+FFFD.
fn check_replaced(shown: &str, data: &[u8], min: usize, max: usize, label: &str) {
    let want = String::from_utf8_lossy(data);
    assert_eq!(
        without_ws(&collapse_runs(shown)),
        without_ws(&collapse_runs(&want)),
        "{label}: text around the malformed bytes changed"
    );
    let n = fffd_in(shown);
    assert!(
        (min..=max).contains(&n),
        "{label}: {n} U+FFFD shown, want {min}..={max}"
    );
}

// ---------------------------------------------------------------------------
// the sequence at every position around a line break, a page break and a read boundary
// ---------------------------------------------------------------------------

fn base_text() -> String {
    prose(500, 2 * PAGE_BUF)
}

// the char boundaries of `text` within 14 bytes of `at`
fn boundaries_near(text: &str, at: usize) -> Vec<usize> {
    (at.saturating_sub(14)..=(at + 14).min(text.len()))
        .filter(|&i| text.is_char_boundary(i))
        .collect()
}

fn with_inserted(text: &str, at: usize, seq: &[u8]) -> Vec<u8> {
    let mut v = text.as_bytes()[..at].to_vec();
    v.extend_from_slice(seq);
    v.extend_from_slice(&text.as_bytes()[at..]);
    v
}

// every char boundary near the first line break, the first page break and the read size
// of the clean book laid out `mono` or proportionally
fn insertion_points(text: &str, mono: bool) -> Vec<usize> {
    let mut r = if mono {
        open_mono(text.as_bytes())
    } else {
        open_book(text.as_bytes(), 2, 1)
    };
    let first_line = r.lines()[0].len();
    let first_page = walk_forward(&mut r)[1].offset as usize;
    let mut v = boundaries_near(text, first_line);
    v.extend(boundaries_near(text, first_page));
    v.extend(boundaries_near(text, PAGE_BUF));
    v.sort_unstable();
    v.dedup();
    assert!(v.len() > 30, "{} insertion points", v.len());
    v
}

fn open_layout(data: &[u8], mono: bool) -> pulp_host::reader::Rig {
    if mono {
        open_mono(data)
    } else {
        open_book(data, 2, 1)
    }
}

// the text before and after one malformed sequence is intact wherever the sequence falls
fn sweep_text_around(mono: bool) {
    let text = base_text();
    let points = insertion_points(&text, mono);
    for (name, seq, _) in BAD {
        for &at in &points {
            let data = with_inserted(&text, at, seq);
            let mut r = open_layout(&data, mono);
            let pages = walk_forward(&mut r);
            let label = format!("monospace {mono}: {name} at byte {at}");
            // one malformed sequence: at least one U+FFFD, at most one per byte
            check_replaced(&displayed(&pages), &data, 1, seq.len(), &label);
        }
    }
}

#[test]
fn text_around_a_malformed_sequence_is_intact_at_every_break_position() {
    sweep_text_around(false);
}

#[test]
fn text_around_a_malformed_sequence_is_intact_at_every_break_position_in_monospace() {
    sweep_text_around(true);
}

// the page buffer holds the file's bytes and decodes line by line to the lossy text,
// whichever break the malformed sequence falls on
fn sweep_page_buffer(mono: bool) {
    let text = base_text();
    let points = insertion_points(&text, mono);
    for (name, seq, _) in BAD {
        for &at in &points {
            let data = with_inserted(&text, at, seq);
            let mut r = open_layout(&data, mono);
            let pages = walk_forward(&mut r);
            let label = format!("monospace {mono}: {name} at byte {at}");
            check_buffer(&pages, &data, &label);
            check_replaced(&displayed(&pages), &data, 1, seq.len(), &label);
        }
    }
}

#[test]
fn page_buffer_keeps_the_file_bytes_and_decodes_to_lossy_text_at_every_break_position() {
    sweep_page_buffer(false);
}

#[test]
fn page_buffer_keeps_the_file_bytes_and_decodes_to_lossy_text_in_monospace() {
    sweep_page_buffer(true);
}

// Replacement count of the decoded line: one U+FFFD for each maximal subpart, as std counts
// them. A short book (one page) with the sequence between valid text of every width,
// shifted over every alignment.
#[test]
fn decoded_page_line_has_one_fffd_per_maximal_subpart() {
    let pre = "\u{81fa}\u{7063}\u{300c}";
    let post = "\u{300d}\u{7e41}\u{9ad4}\u{e9}\u{20bb7}";
    for (name, seq, n) in BAD {
        for pad in 0..12 {
            let mut data = padded(pad, pre);
            data.extend_from_slice(seq);
            data.extend_from_slice(post.as_bytes());
            for mono in [false, true] {
                let r = open_layout(&data, mono);
                let label = format!("{name}, pad {pad}, monospace {mono}");
                let page = snapshot(&r);
                check_buffer(std::slice::from_ref(&page), &data, &label);
                let shown = displayed(std::slice::from_ref(&page));
                let want = format!("{}{pre}{}{post}", "a".repeat(pad), "\u{FFFD}".repeat(*n));
                assert_eq!(without_ws(&shown), without_ws(&want), "{label}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the pixels: ReaderApp::draw of a one-line book with a malformed sequence in it
// ---------------------------------------------------------------------------

fn one_line_book(pad: usize, seq: &[u8]) -> (Vec<u8>, String, String) {
    let pre = "\u{81fa}\u{7063}\u{300c}";
    let post = "\u{300d}\u{7e41}\u{9ad4}\u{e9}\u{20bb7}";
    let mut data = padded(pad, pre);
    data.extend_from_slice(seq);
    data.extend_from_slice(post.as_bytes());
    (data, format!("{}{pre}", "a".repeat(pad)), post.to_string())
}

// any replacement policy: the line drawn is the valid text with 1..=len(seq) replacement
// cells where the malformed sequence is, and the valid text on both sides in place
#[test]
fn drawn_line_shows_replacement_cells_and_the_valid_text_around_a_malformed_sequence() {
    for (name, seq, _) in BAD {
        for pad in 0..12 {
            let (data, pre, post) = one_line_book(pad, seq);
            let r = open_book(&data, 2, 1);
            let ok = (1..=seq.len()).any(|k| {
                let want = format!("{pre}{}{post}", "\u{FFFD}".repeat(k));
                drawn_line_matches(&r, 2, 0, &want).is_ok()
            });
            assert!(
                ok,
                "{name}, pad {pad}: the drawn line fits no 1..={} replacement cells",
                seq.len()
            );
        }
    }
}

// the maximal-subpart count of replacement cells
#[test]
fn drawn_line_has_one_replacement_cell_per_maximal_subpart() {
    for (name, seq, n) in BAD {
        for pad in 0..12 {
            let (data, pre, post) = one_line_book(pad, seq);
            let r = open_book(&data, 2, 1);
            let want = format!("{pre}{}{post}", "\u{FFFD}".repeat(*n));
            if let Err(e) = drawn_line_matches(&r, 2, 0, &want) {
                panic!("{name}, pad {pad}: want {n} replacement cells: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the file ends inside a scalar, at every position around the read size
// ---------------------------------------------------------------------------

fn cut_points() -> Vec<usize> {
    let mut v = Vec::new();
    for k in 1..=3 {
        v.extend(k * PAGE_BUF - 8..=k * PAGE_BUF + 8);
    }
    v
}

// A file cut at byte L: when L falls inside a scalar the book ends with the cut scalar's
// valid prefix, which is one maximal subpart: shown as exactly one U+FFFD; the text before
// it is intact.
fn sweep_eof(mono: bool) {
    let text = prose(600, 3 * PAGE_BUF + 100);
    let mut inside = 0;
    for len in cut_points() {
        let data = &text.as_bytes()[..len];
        let cut = std::str::from_utf8(data).is_err();
        inside += cut as usize;
        for (font, theme) in if mono { vec![(2, 1)] } else { CONFIGS.to_vec() } {
            let mut r = fresh_rig(data, font, theme);
            if mono {
                r.open_monospace(BOOK);
            } else {
                r.open(BOOK);
            }
            let pages = walk_forward(&mut r);
            let label = format!("monospace {mono}: file of {len} bytes, font {font}");
            let (min, max) = if cut { (1, 1) } else { (0, 0) };
            let shown = displayed(&pages);
            check_replaced(&shown, data, min, max, &label);
            if cut {
                assert!(
                    shown.trim_end().ends_with(FFFD),
                    "{label}: the cut scalar is not shown as U+FFFD"
                );
            }
        }
    }
    assert!(
        inside >= 10,
        "only {inside} cut points fell inside a scalar"
    );
}

#[test]
fn a_file_ending_inside_a_scalar_ends_with_a_replacement_character() {
    sweep_eof(false);
}

#[test]
fn a_file_ending_inside_a_scalar_ends_with_a_replacement_character_in_monospace() {
    sweep_eof(true);
}

#[test]
fn page_buffer_of_a_file_ending_inside_a_scalar_keeps_the_bytes_and_decodes_to_one_fffd() {
    let text = prose(600, 3 * PAGE_BUF + 100);
    for len in cut_points() {
        let data = &text.as_bytes()[..len];
        for mono in [false, true] {
            let mut r = open_layout(data, mono);
            let pages = walk_forward(&mut r);
            let label = format!("monospace {mono}: file of {len} bytes");
            check_buffer(&pages, data, &label);
            let cut = std::str::from_utf8(data).is_err();
            let (min, max) = if cut { (1, 1) } else { (0, 0) };
            check_replaced(&displayed(&pages), data, min, max, &label);
        }
    }
}

// every prefix of a short book, drawn: a cut scalar is one replacement cell (a valid
// prefix of a scalar is one maximal subpart), everything before it is drawn as is
#[test]
fn drawn_prefix_of_a_book_cut_inside_a_scalar_ends_with_one_replacement_cell() {
    let book = format!("a{E_ACUTE}{TAI}{YOSHI}b");
    for len in 1..=book.len() {
        let data = &book.as_bytes()[..len];
        let r = open_book(data, 2, 1);
        let want = String::from_utf8_lossy(data).into_owned();
        if let Err(e) = drawn_line_matches(&r, 2, 0, &want) {
            panic!("prefix of {len} bytes, want {want:?}: {e}");
        }
    }
}

// A page is laid out from the bytes the read actually returned, never from what an earlier,
// larger read left in the buffer behind them. A file ending inside a scalar is the case
// where the leftovers matter: if the layout looked past the end of the read, bytes of an
// earlier page could "complete" the cut scalar into a character the file does not hold.
//
// Run: scripts/host-test.sh --test utf8_residue
//
// Oracle: the text of the book is the file's bytes, and a sequence cut off by the end of
// the file is one maximal subpart, shown as exactly one U+FFFD (std `from_utf8_lossy`).
// The files are runs of one scalar width with ASCII padding in front, so that for some
// padding the bytes just past the end of the last read, in the buffer of the page before,
// are continuation bytes that would complete the cut lead byte. The sweep checks that
// such a case occurs (`leftovers_would_complete_*`), so the test does not pass vacuously.

mod utf8_common;

use pulp_host::reader::PAGE_BUF;
use pulp_host::utf8::Utf8Iter;
use utf8_common::*;

// proper prefixes of 2-, 3- and 4-byte scalars: a cut file ends in one of these
const TAILS: [&[u8]; 6] = [
    &[0xC3],
    &[0xE8],
    &[0xE8, 0x87],
    &[0xF0],
    &[0xF0, 0xA0],
    &[0xF0, 0xA0, 0xAE],
];

fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

fn runs() -> Vec<(&'static str, String)> {
    let min = 3 * PAGE_BUF;
    vec![
        ("e-acute run", repeat_to(E_ACUTE, min)),
        ("tai run", repeat_to(TAI, min)),
        ("yoshi run", repeat_to(YOSHI, min)),
        ("mixed run", repeat_to(MIXED_UNIT, min)),
    ]
}

fn decoded(lines: &[Vec<u8>]) -> String {
    lines
        .iter()
        .map(|l| Utf8Iter::new(l).collect::<String>())
        .collect()
}

fn without_ws(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\r' | '\n'))
        .collect()
}

// true when the bytes that follow the last page's text in the buffer of the page before it
// are continuation bytes that would complete the cut lead byte at the end of the file
// (the leftovers: file[start of earlier page + length of last read ..])
fn leftovers_complete(data: &[u8], pages: &[Page], tail_len: usize) -> bool {
    let n = pages.len();
    if n < 2 {
        return false;
    }
    let last = pages[n - 1].offset as usize;
    let before = pages[n - 2].offset as usize;
    let read = data.len() - last; // bytes the last read returned
    let at = before + read;
    let need = match data[data.len() - tail_len] {
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        _ => 3,
    } - (tail_len - 1);
    at + need <= data.len() && data[at..at + need].iter().all(|&b| is_cont(b))
}

fn sweep(mono: bool) -> usize {
    let mut completing = 0;
    for (name, body) in runs() {
        for pad in 0..14 {
            for tail in TAILS {
                let mut data = padded(pad, &body);
                data.extend_from_slice(tail);
                let mut r = if mono {
                    open_mono(&data)
                } else {
                    open_book(&data, 2, 1)
                };
                let pages = walk_forward(&mut r);
                let label = format!("monospace {mono}: {name}, pad {pad}, tail {tail:02X?}");
                assert!(pages.len() >= 3, "{label}: {} pages", pages.len());
                let lines = all_lines(&pages);
                assert_eq!(
                    strip_ws(&text_of(&lines)),
                    strip_ws(&data),
                    "{label}: the page buffer's bytes differ from the file's"
                );
                // the whole book shown: the file's text, the cut scalar as one U+FFFD
                assert_eq!(
                    without_ws(&decoded(&lines)),
                    without_ws(&String::from_utf8_lossy(&data)),
                    "{label}"
                );
                completing += leftovers_complete(&data, &pages, tail.len()) as usize;
            }
        }
    }
    completing
}

#[test]
fn a_file_ending_inside_a_scalar_is_not_completed_by_leftover_buffer_bytes() {
    sweep(false);
}

#[test]
fn a_file_ending_inside_a_scalar_is_not_completed_by_leftover_buffer_bytes_in_monospace() {
    sweep(true);
}

// the sweep reaches the situation it is about: for some file the leftovers do complete
// the cut scalar (a bytes-past-the-read layout would show a character that is not there)
#[test]
fn leftovers_would_complete_the_cut_scalar_in_some_files_of_the_sweep() {
    assert!(sweep(false) >= 10, "proportional layout");
    assert!(sweep(true) >= 10, "monospace layout");
}

// ---------------------------------------------------------------------------
// the width of the cut scalar
// ---------------------------------------------------------------------------

// A scalar with a glyph of its own (é, em dash) is wider than U+FFFD. If leftover bytes
// completed a cut lead byte while the line was measured, the last line would be measured
// with the wider glyph and could break where the file's own text does not. The layout of
// a file ending in a cut scalar is that of the same file ending in U+FFFD (the cut is one
// maximal subpart): the same lines, scalar for scalar, on the same number of pages.
const EM_DASH: &str = "\u{2014}";

fn laid_out(data: &[u8], font: u8, theme: u8) -> Vec<Vec<String>> {
    let mut r = open_book(data, font, theme);
    walk_forward(&mut r)
        .iter()
        .map(|p| {
            p.lines
                .iter()
                .map(|l| Utf8Iter::new(l).collect::<String>())
                .collect()
        })
        .collect()
}

#[test]
fn a_cut_scalar_is_laid_out_like_a_replacement_character_at_every_last_line_fill() {
    // (scalar run, tails that cut a scalar of it: lead byte(s) of the same scalar)
    let cases: [(&str, &[&[u8]]); 2] =
        [(E_ACUTE, &[&[0xC3]]), (EM_DASH, &[&[0xE2], &[0xE2, 0x80]])];
    for (font, theme) in [(2u8, 1u8), (0, 0)] {
        let probe = open_book(b"x", font, theme);
        let (text_w, max_lines) = (probe.text_w(), probe.max_lines());
        for (unit, tails) in cases {
            let adv = pulp_host::fonts::FontSet::for_size(font).advance(
                unit.chars().next().unwrap(),
                pulp_host::fonts::Style::Regular,
            ) as u32;
            let per_line = (text_w / adv) as usize;
            // more than two pages, then every last-line fill twice over
            let base = (2 * max_lines + 1) * per_line;
            for count in base..base + 2 * per_line {
                for pad in 0..2 {
                    for tail in tails {
                        let mut cut = padded(pad, &unit.repeat(count));
                        cut.extend_from_slice(tail);
                        let mut fffd = padded(pad, &unit.repeat(count));
                        fffd.extend_from_slice("\u{FFFD}".as_bytes());
                        assert_eq!(
                            laid_out(&cut, font, theme),
                            laid_out(&fffd, font, theme),
                            "font {font}: {unit:?} x {count}, pad {pad}, tail {tail:02X?}"
                        );
                    }
                }
            }
        }
    }
}

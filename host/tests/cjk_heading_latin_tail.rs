//! R10 boundary policy: "heading 狀態持續到 closing marker，不因頁界或 nested
//! bold/italic 而結束。" A heading opened by 0x01 'H' and closed by 0x01 'h' keeps
//! heading style on every page before the closing marker, including pages that hold
//! only Latin text after a CJK start.
//!
//! Observables (all derived from the requirement and literal pack metrics):
//! * Latin static glyphs are wider in heading style than in body style, so the number
//!   of Latin chars that fit a narrow line distinguishes the two styles. Reference
//!   capacities are measured on independent oracle rigs (Latin-only heading, Latin-only
//!   body) that do not involve a CJK start or a page boundary inside a CJK heading.
//! * CJK glyphs come from the literal pack: heading size-index 0 is 23px, body 16px,
//!   and the pack bitmap is rows(px, c), so the drawn patch identifies the style.
mod cjk_support;

use cjk_support::{BOOK, assert_patch, rig, rows, text_lines};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::render_full;

const WIDTH: u32 = 2 * 24;
const LATIN: usize = 600;

fn chars_per_line(line: &str) -> usize {
    line.chars().count()
}

/// Latin chars per full line in the given style, measured on a rig that contains only
/// that style, on its first page.
fn latin_capacity(heading: bool) -> usize {
    let mut input = Vec::new();
    if heading {
        input.extend_from_slice(&[1, b'H']);
    }
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    if heading {
        input.extend_from_slice(b"\x01h\n");
    }
    let r = rig(&input, 0, WIDTH);
    chars_per_line(&text_lines(&r)[0])
}

fn caps() -> (usize, usize) {
    let (h, b) = (latin_capacity(true), latin_capacity(false));
    assert!(
        h >= 1 && h < b,
        "fixture: heading Latin ({h}) must be wider than body ({b})"
    );
    (h, b)
}

/// Visit every page from the current one forward until Next no longer advances.
fn walk(r: &mut Rig, mut visit: impl FnMut(&Rig, usize)) {
    loop {
        let page = r.page();
        visit(r, page);
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready);
        if r.page() == page {
            break;
        }
    }
}

/// Heading: short CJK run, then a Latin-only run, closing marker after the run.
fn latin_tail_input() -> (Vec<u8>, u32) {
    let mut input = vec![1, b'H'];
    input.extend_from_slice("臺".as_bytes());
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    let heading_end = input.len() as u32;
    input.extend_from_slice(b"\x01h\nBody.");
    (input, heading_end)
}

/// Every Latin-only line of the visible page must hold exactly `cap` chars, except the
/// last line of the page (it may be a partial line only at end of text).
fn assert_latin_page_capacity(r: &Rig, cap: usize, what: &str) {
    let lines = text_lines(r);
    let n = lines.len();
    assert!(n > 1, "{what}: page has lines");
    for (i, l) in lines.iter().enumerate().take(n - 1) {
        assert!(
            l.chars().all(|c| c == 'A'),
            "{what}: latin-only line {i}: {l:?}"
        );
        assert_eq!(
            chars_per_line(l),
            cap,
            "{what}: line {i} {l:?} not in heading style"
        );
    }
}

#[test]
fn pure_latin_heading_resets_style_at_page_start() {
    // Characterization (spec R10, scoped): with no CJK glyph window earlier in the chapter,
    // heading style is reset at every page start, as in the original English behaviour.
    let (h, b) = caps();
    let mut input = vec![1, b'H'];
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    let end = input.len() as u32;
    input.extend_from_slice(b"\x01h\n");
    let mut r = rig(&input, 0, WIDTH);
    assert_eq!(
        chars_per_line(&text_lines(&r)[0]),
        h,
        "page 0 opens the heading"
    );
    let mut pages = 0;
    walk(&mut r, |r, p| {
        if p > 0
            && r.page_offsets()[p] < end
            && r.page_offsets().get(p + 1).is_some_and(|&o| o <= end)
        {
            pages += 1;
            assert_latin_page_capacity(r, b, &format!("pure-Latin continuation page {p}"));
        }
    });
    assert!(pages >= 3);
}

#[test]
fn latin_only_pages_inside_open_cjk_heading_keep_heading_style() {
    let (h, _) = caps();
    let (input, heading_end) = latin_tail_input();
    let mut r = rig(&input, 0, WIDTH);
    let mut checked = 0;
    walk(&mut r, |r, p| {
        let start = r.page_offsets()[p];
        if p == 0 || start >= heading_end {
            return;
        }
        // Page begins and (when its end is known) ends before the closing marker.
        let ends_before_close = r
            .page_offsets()
            .get(p + 1)
            .is_some_and(|&o| o <= heading_end);
        if !ends_before_close {
            return;
        }
        assert!(text_lines(r).concat().chars().all(|c| c == 'A'));
        assert_latin_page_capacity(r, h, &format!("page {p} at raw offset {start}"));
        checked += 1;
    });
    assert!(
        checked >= 3,
        "heading continues over at least three Latin-only pages, got {checked}"
    );
}

#[test]
fn late_cjk_in_open_heading_is_drawn_at_heading_size_after_latin_only_pages() {
    let mut input = vec![1, b'H'];
    input.extend_from_slice("臺".as_bytes());
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    let tail_start = input.len() as u32;
    input.extend_from_slice("臺".repeat(400).as_bytes());
    let heading_end = input.len() as u32;
    input.extend_from_slice(b"\x01h\nBody.");
    let mut r = rig(&input, 0, WIDTH);
    let mut latin_only_before = 0;
    let mut seen = false;
    walk(&mut r, |r, p| {
        let start = r.page_offsets()[p];
        let text = text_lines(r).concat();
        if start >= heading_end {
            return;
        }
        if p > 0 && text.chars().all(|c| c == 'A') {
            latin_only_before += 1;
        }
        // A page that starts inside the pure CJK tail and is still in the heading.
        if start >= tail_start && !seen && text.chars().all(|c| c == '臺') && !text.is_empty() {
            seen = true;
            assert!(
                latin_only_before >= 2,
                "Latin-only pages precede the CJK tail"
            );
            assert_patch(r, r.text_margin(), 8, &rows(23, '臺'), 3);
        }
    });
    assert!(seen, "fixture reaches a pure CJK page inside the heading");
}

#[test]
fn navigation_and_bookmark_restore_keep_heading_style_on_latin_only_page() {
    let (h, _) = caps();
    let (input, heading_end) = latin_tail_input();
    let mut r = rig(&input, 0, WIDTH);
    r.press(Action::Next);
    assert_eq!(r.page(), 1);
    let second = r.lines();
    let second_offset = r.page_offsets()[1];
    assert!(second_offset < heading_end);
    assert_latin_page_capacity(&r, h, "page 1 first visit");
    r.press(Action::Next);
    assert_eq!(r.page(), 2);
    assert!(
        r.page_offsets()[2] < heading_end,
        "heading spans at least three pages"
    );
    assert_latin_page_capacity(&r, h, "page 2");
    r.press(Action::Prev);
    assert_eq!(r.page(), 1);
    assert_eq!(r.lines(), second);
    assert_latin_page_capacity(&r, h, "page 1 after Prev");

    r.save_position();
    r.bookmarks_flush();
    let mut reboot = Rig::new(r.into_storage());
    reboot.configure(0, 0);
    reboot.set_text_width(WIDTH);
    reboot.open(BOOK);
    assert_eq!(reboot.phase(), Phase::Ready);
    assert_eq!(reboot.page_offsets()[reboot.page()], second_offset);
    assert_eq!(reboot.lines(), second);
    assert_latin_page_capacity(&reboot, h, "restored page");
    reboot.press(Action::Next);
    assert_latin_page_capacity(&reboot, h, "page after restore");
}

#[test]
fn body_style_resumes_after_closing_marker() {
    let (h, b) = caps();
    let mut input = vec![1, b'H'];
    input.extend_from_slice("臺".as_bytes());
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    input.extend_from_slice(b"\x01h\n");
    let body_start = input.len() as u32;
    input.extend_from_slice("A".repeat(LATIN).as_bytes());
    input.push(b'\n');
    let cjk_start = input.len() as u32;
    input.extend_from_slice("臺".repeat(400).as_bytes());
    let mut r = rig(&input, 0, WIDTH);
    let (mut heading_pages, mut body_latin, mut body_cjk) = (0, 0, 0);
    walk(&mut r, |r, p| {
        let start = r.page_offsets()[p];
        let known_end = r.page_offsets().get(p + 1).copied();
        let text = text_lines(r).concat();
        if p > 0 && known_end.is_some_and(|e| e <= body_start) {
            heading_pages += 1;
            assert_latin_page_capacity(r, h, &format!("heading page {p}"));
        } else if start >= body_start && known_end.is_some_and(|e| e <= cjk_start) {
            body_latin += 1;
            assert_latin_page_capacity(r, b, &format!("body page {p}"));
        } else if start >= cjk_start && !text.is_empty() && text.chars().all(|c| c == '臺') {
            body_cjk += 1;
            assert_patch(r, r.text_margin(), 8, &rows(16, '臺'), 3);
        }
    });
    assert!(
        heading_pages >= 3 && body_latin >= 1 && body_cjk >= 1,
        "{heading_pages} {body_latin} {body_cjk}"
    );
}

// ---- Drawn output of the heading line that carries the closing marker ----
//
// R10 (scoped): heading style persists until the closing marker, so the text before the
// marker on its own line must be DRAWN in heading style, and only text after the marker
// in body style. Observable: the pixel strip of one text line (full line height, text
// width), compared with reference rigs that draw the same visible text as a pure heading
// line and as a pure body line. No marker occurs in either reference.

fn line_strip(r: &Rig, line: usize) -> Vec<bool> {
    let f = render_full(&|s| r.draw(s)).frame;
    let top = r.text_y() + line as u16 * r.font_line_h();
    let mut v = Vec::new();
    for y in top..top + r.font_line_h() {
        for x in r.text_margin()..r.text_margin() + WIDTH as u16 {
            v.push(f.is_black(x, y));
        }
    }
    v
}

/// Strip of `text` drawn alone on line 0, as heading or as body.
fn reference_strip(text: &str, heading: bool) -> Vec<bool> {
    let mut input = Vec::new();
    if heading {
        input.extend_from_slice(&[1, b'H']);
    }
    input.extend_from_slice(text.as_bytes());
    input.extend_from_slice(b"\nA");
    let r = rig(&input, 0, WIDTH);
    assert_eq!(
        text_lines(&r)[0],
        text,
        "fixture: reference line holds the text"
    );
    line_strip(&r, 0)
}

fn has_marker(line: &[u8]) -> bool {
    line.windows(2).any(|w| w == [1, b'h'])
}

/// Walk to the page holding the closing marker; every heading line of that page, the
/// marker line included, must be drawn like the reference heading line of the same text,
/// and the line after the marker like the reference body line.
fn assert_closing_page_drawn_in_heading_style(input: &[u8], what: &str) -> String {
    let mut r = rig(input, 0, WIDTH);
    let mut marker_line_text = None;
    walk(&mut r, |r, p| {
        let raw = r.lines();
        let Some(n) = raw.iter().position(|l| has_marker(l)) else {
            return;
        };
        assert!(p > 0, "{what}: marker page must be a continuation page");
        let text = text_lines(r);
        for (i, t) in text.iter().enumerate().take(n + 1) {
            if t.trim().is_empty() {
                continue;
            }
            let heading = reference_strip(t, true);
            assert_ne!(
                heading,
                reference_strip(t, false),
                "fixture: styles differ for {t:?}"
            );
            assert!(
                line_strip(r, i) == heading,
                "{what}: page {p} line {i} {t:?} (marker line is {n}) is not drawn in heading style"
            );
        }
        marker_line_text = Some(text[n].clone());
    });
    marker_line_text.unwrap_or_else(|| panic!("{what}: no page shows the closing marker"))
}

#[test]
fn closing_marker_line_is_drawn_in_heading_style_on_latin_only_continuation_page() {
    let (input, _) = latin_tail_input();
    let t = assert_closing_page_drawn_in_heading_style(&input, "single-letter run");
    assert!(!t.is_empty());
}

#[test]
fn text_after_closing_marker_is_drawn_in_body_style() {
    let (input, heading_end) = latin_tail_input();
    let mut r = rig(&input, 0, WIDTH);
    let mut seen = false;
    walk(&mut r, |r, p| {
        let text = text_lines(r);
        let Some(i) = text.iter().position(|t| t == "Body.") else {
            return;
        };
        assert!(
            p > 0 && r.page_offsets()[p] < heading_end,
            "Body. shares the marker page"
        );
        assert!(
            line_strip(r, i) == reference_strip("Body.", false),
            "line after the marker is not drawn in body style"
        );
        seen = true;
    });
    assert!(seen);
}

#[test]
fn closing_marker_line_with_spaces_in_latin_words_is_drawn_in_heading_style() {
    // Words so that line breaks land on spaces inside the carried heading; suffixes move the
    // marker line around those breaks. At least one variant puts a space on the marker line.
    let mut with_space = 0;
    for suffix in ["", " ", "A", "A ", " B", "A B", "AA B", "A BB", "B A "] {
        let mut input = vec![1, b'H'];
        input.extend_from_slice("臺".as_bytes());
        input.extend_from_slice("AAA BBB ".repeat(75).as_bytes());
        input.extend_from_slice(suffix.as_bytes());
        input.extend_from_slice(b"\x01h\nBody.");
        let t = assert_closing_page_drawn_in_heading_style(&input, &format!("suffix {suffix:?}"));
        if t.contains(' ') {
            with_space += 1;
        }
    }
    assert!(with_space >= 1, "no variant put a space on the marker line");
}

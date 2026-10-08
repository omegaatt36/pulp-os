// paging regression -- UTF-8 + paging through the production ReaderApp:
// previous/next paging on English TXT fixtures.
//
// Run: cargo test-host --test paging
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// Everything below is reached through the real ReaderApp: the production
// src/apps/reader/* sources (paging.rs: wrap_lines_counted, page_forward /
// page_backward, jump_*, preindex_all_pages, trim_trailing_cr ...) are
// `#[path]`-included and driven the way the firmware's app manager and
// scheduler do (on_enter -> run `background` until the page is ready ->
// on_event). Only the hardware edge may be shimmed; no layout / wrapping /
// paging logic may be reimplemented in pulp-host. The model is the archived
// reader-regression/os/src/rig.rs (`Rig`) with the in-memory FakeFs
// replaced by `pulp_host::storage::VirtualStorage`.
//
// host/src/lib.rs must expose these PUBLIC modules (besides the storage regression `error` and
// `storage`, and the `utf8` of tests/utf8.rs):
//
//   pulp_host::fonts    -- the firmware's own src/fonts (build.rs-generated glyph
//                          data; same set the ReaderApp wraps with).
//                          Used here ONLY to measure text width:
//                            FontSet::for_size(idx: u8) -> FontSet        (Copy)
//                            FontSet::advance(&self, ch: char, style: Style) -> u8
//                            Style::Regular            (TXT is laid out Regular)
//
//   pulp_host::reader   -- pub use of the production
//                            board::action::Action      with variants
//                                Next, Prev, NextJump, PrevJump
//                          and
//   pub enum Phase { Loading, Ready, Toc, Error }      // Debug, Clone, Copy, PartialEq, Eq
//                          (Action must be Debug + Clone + Copy as well)
//                          `Ready`   = a page is up (what ReaderApp::State::Ready is)
//                          `Error`   = the error page is up (State::Error)
//                          `Loading` = still working (any other state)
//                          `Toc`     = EPUB table of contents (unused by paging regression)
//
//   pub struct Rig          // drives one real ReaderApp over one VirtualStorage
//
//   Rig::new(storage: VirtualStorage) -> Rig
//        Takes ownership of the card. Boots the way the firmware does before any
//        app runs (kernel built over the storage, bookmark cache loaded). The
//        tests create a card with `VirtualStorage::memory_with(..)` or
//        `VirtualStorage::host_dir(..)` and call `ensure_pulp_dir()` first.
//   Rig::storage(&self) -> &VirtualStorage
//        The same card, for read_log / read_count / inject_* / reset_reads.
//   Rig::configure(&mut self, book_font: u8, theme: u8)
//        AppManager::propagate_fonts: book font size index (0..5) and reading
//        theme index (0..4), before `open`.
//   Rig::open(&mut self, name: &str)
//        set the open-file message, on_enter, then run `background` until the
//        phase is no longer `Loading` (bounded: panics with a message that
//        contains "did not settle" if it never leaves Loading -- that is a test
//        failure by design).
//   Rig::press(&mut self, a: Action)
//        on_event(ActionEvent::Press(a)) then settle exactly like `open`
//        (so lazy indexing triggered by a page turn has completed on return).
//   Rig::phase(&self) -> Phase
//   Rig::error_kind(&self) -> Option<ErrorKind>
//        kind of the error currently shown (Some only in Phase::Error)
//   Rig::page(&self) -> usize             // 0-based current page
//   Rig::total_pages(&self) -> usize      // pages in the page table so far
//   Rig::fully_indexed(&self) -> bool     // page table complete
//   Rig::page_offsets(&self) -> Vec<u32>  // start byte offset of every page in
//                                         // the page table: len == total_pages()
//   Rig::lines(&self) -> Vec<Vec<u8>>
//        the current page, one entry per laid-out line, RAW bytes exactly as in
//        the reader's page buffer (no decoding, no marker stripping; TXT has no
//        markers). Line terminators (LF / CRLF) are not part of a line.
//   Rig::max_lines(&self) -> usize        // lines per page (capacity)
//   Rig::text_w(&self) -> u32             // text area width in px
//   Rig::text_margin(&self) -> u16        // left/right margin in px
//   Rig::font_line_h(&self) -> u16        // line height in px
//   Rig::text_area_h(&self) -> u16        // text area height in px
//
// What the tests treat as the observable result of a failure:
//   * the reader never panics and never stays in `Loading` forever;
//   * the failure is `Phase::Error` with `error_kind()` == Some(kind), or the
//     reader stays `Ready` and everything it shows is correct (see the fault
//     tests for exactly what "correct" means; the open questions are listed in
//     the paging regression report, not guessed here).
//
// Expected values never come from running the code under test. They are:
//   (a) invariants of the text itself (characters conserved modulo the
//       whitespace a wrapper may drop, UTF-8 well-formedness per RFC 3629,
//       fixtures built so that line/page counts are known by construction);
//   (b) the public glyph advance widths, used only as the measuring stick for
//       "no line is wider than the text area";
//   (c) the VirtualStorage read log as the measure of how much was read.
// ============================================================================

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use pulp_host::error::ErrorKind;
use pulp_host::fonts::{FontSet, Style};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::storage::{ReadOutcome, StorageOp, VirtualStorage};

const BOOK: &str = "BOOK.TXT";
// portrait panel width in px (OnePage 480x800 portrait; the text area is this minus both margins)
const SCREEN_W: u32 = 480;
// (book font index, reading theme index): corners and a few interior points of the 5x4 grid
const CONFIGS: [(u8, u8); 5] = [(0, 0), (2, 1), (4, 3), (1, 2), (3, 0)];

// ---------------------------------------------------------------------------
// fixtures (deterministic, generated here, no network / no external files)
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

const ASCII_WORDS: &[&str] = &[
    "the", "lighthouse", "keeper", "climbed", "spiral", "stairs", "each", "evening", "harbour", "below", "turned", "colour", "of", "old",
    "pewter", "she", "read", "letter", "twice", "folded", "along", "its", "creases", "where", "no", "one", "would", "look", "market",
    "smelled", "tar", "oranges", "rope", "happy", "rain", "drummed", "tin", "roof", "hour", "softened", "whisper", "stopped", "patience",
    "habit", "ledger", "barrels", "signatures", "tide", "ferry", "crossing", "cliffs", "A", "I", "of", "and", "to",
];

// every word has 2- or 3-byte UTF-8 characters (Latin-1 letters and typographic punctuation)
const MULTIBYTE_WORDS: &[&str] = &[
    "caf\u{e9}",
    "na\u{ef}ve",
    "\u{dc}ber",
    "se\u{f1}or",
    "\u{c5}ngstr\u{f6}m",
    "r\u{e9}sum\u{e9}",
    "co\u{f6}perate",
    "stra\u{df}e",
    "\u{2014}",
    "\u{201c}quoted\u{201d}",
    "it\u{2019}s",
    "wait\u{2026}",
    "\u{20ac}5",
    "\u{2018}single\u{2019}",
    "\u{ab}guillemets\u{bb}",
    "20\u{b0}C",
    "\u{201c}\u{2014}\u{201d}",
];

struct Book {
    bytes: Vec<u8>,
    // the source paragraphs (each one source line, no terminator)
    paragraphs: Vec<String>,
}

// English-ish prose: paragraphs of 3..=90 single-space-separated words, ~45 % of them
// with multi-byte characters; paragraphs are separated by a blank line (`eol eol`);
// no terminator after the last paragraph. Every word is far narrower than any text area.
fn prose(seed: u64, bytes: usize, eol: &str) -> Book {
    let mut rng = Lcg(seed);
    let mut paragraphs = Vec::new();
    let mut size = 0usize;
    while size < bytes {
        let n = 3 + rng.below(88);
        let mut p = String::new();
        for i in 0..n {
            if i > 0 {
                p.push(' ');
            }
            if rng.below(100) < 45 {
                p.push_str(MULTIBYTE_WORDS[rng.below(MULTIBYTE_WORDS.len())]);
            } else {
                p.push_str(ASCII_WORDS[rng.below(ASCII_WORDS.len())]);
            }
        }
        size += p.len() + 2 * eol.len();
        paragraphs.push(p);
    }
    let sep = format!("{eol}{eol}");
    Book { bytes: paragraphs.join(sep.as_str()).into_bytes(), paragraphs }
}

// n short lines "line 0000", "line 0001", ... joined by `eol`; known by construction:
// the expected layout is exactly these lines.
fn numbered(n: usize, eol: &str, trailing_eol: bool) -> (Vec<u8>, Vec<Vec<u8>>) {
    let lines: Vec<Vec<u8>> = (0..n).map(|i| format!("line {i:04}").into_bytes()).collect();
    (join_lines(&lines, eol, trailing_eol), lines)
}

// like `numbered`, but every 5th line (i % 5 == 3, never the last) is empty
fn numbered_with_blanks(n: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let lines: Vec<Vec<u8>> = (0..n)
        .map(|i| if i % 5 == 3 && i != n - 1 { Vec::new() } else { format!("row {i:04}").into_bytes() })
        .collect();
    (join_lines(&lines, "\n", false), lines)
}

fn join_lines(lines: &[Vec<u8>], eol: &str, trailing_eol: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(eol.as_bytes());
        }
        out.extend_from_slice(l);
    }
    if trailing_eol {
        out.extend_from_slice(eol.as_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// rig helpers
// ---------------------------------------------------------------------------

fn card(data: &[u8]) -> VirtualStorage {
    let s = VirtualStorage::memory_with(&[(BOOK, data)]);
    s.ensure_pulp_dir().expect("card has _PULP/ the way the firmware boots it");
    s
}

fn fresh_rig(data: &[u8], font: u8, theme: u8) -> Rig {
    let mut r = Rig::new(card(data));
    r.configure(font, theme);
    r
}

fn open_book(data: &[u8], font: u8, theme: u8) -> Rig {
    let mut r = fresh_rig(data, font, theme);
    r.open(BOOK);
    r
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Page {
    index: usize,
    offset: u32,
    lines: Vec<Vec<u8>>,
}

fn snapshot(r: &Rig) -> Page {
    let index = r.page();
    let offsets = r.page_offsets();
    assert_eq!(offsets.len(), r.total_pages(), "page_offsets covers the page table");
    assert!(index < offsets.len(), "current page {index} is inside the page table ({})", offsets.len());
    Page { index, offset: offsets[index], lines: r.lines() }
}

// Press(Next) until the page stops changing; every Next moves exactly one page.
fn walk_forward(r: &mut Rig) -> Vec<Page> {
    assert_eq!(r.phase(), Phase::Ready, "book is open");
    let mut out = vec![snapshot(r)];
    assert_eq!(out[0].index, 0, "a book opens on page 0");
    loop {
        let before = r.page();
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready);
        if r.page() == before {
            return out;
        }
        assert_eq!(r.page(), before + 1, "Next moves exactly one page");
        out.push(snapshot(r));
        assert!(out.len() < 2000, "walk_forward runaway");
    }
}

fn walk_back(r: &mut Rig) -> Vec<Page> {
    let mut out = vec![snapshot(r)];
    loop {
        let before = r.page();
        r.press(Action::Prev);
        assert_eq!(r.phase(), Phase::Ready);
        if r.page() == before {
            return out;
        }
        assert_eq!(r.page() + 1, before, "Prev moves exactly one page");
        out.push(snapshot(r));
        assert!(out.len() < 2000, "walk_back runaway");
    }
}

fn all_lines(pages: &[Page]) -> Vec<Vec<u8>> {
    pages.iter().flat_map(|p| p.lines.iter().cloned()).collect()
}

// ---------------------------------------------------------------------------
// text helpers (std only: independent of the production decoder / wrapper)
// ---------------------------------------------------------------------------

// A wrapper may drop the space that overflows a line and consumes line
// terminators, so text is compared modulo ASCII space / CR / LF.
fn strip_ws(b: &[u8]) -> Vec<u8> {
    b.iter().copied().filter(|c| !matches!(c, b' ' | b'\r' | b'\n')).collect()
}

fn text_of(lines: &[Vec<u8>]) -> Vec<u8> {
    lines.iter().flat_map(|l| l.iter().copied()).collect()
}

fn width(font: u8, line: &[u8]) -> u32 {
    let fs = FontSet::for_size(font);
    std::str::from_utf8(line)
        .unwrap_or_else(|e| panic!("line is not valid UTF-8 ({e}): {:?}", String::from_utf8_lossy(line)))
        .chars()
        .map(|c| fs.advance(c, Style::Regular) as u32)
        .sum()
}

fn ceil_div(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

// ---------------------------------------------------------------------------
// the invariants every well-formed English book must satisfy
// ---------------------------------------------------------------------------

// `data` must be valid UTF-8 and contain none of TAB / NBSP / soft hyphen.
fn check_well_formed(r: &Rig, pages: &[Page], data: &[u8], font: u8, label: &str) {
    assert!(r.fully_indexed(), "{label}: walked to the end, page table complete");
    assert_eq!(r.total_pages(), pages.len(), "{label}: page table == pages visited");
    let max = r.max_lines();
    let text_w = r.text_w();

    // (1) page table: starts at 0, strictly increasing, inside the file, on character boundaries
    let offs = r.page_offsets();
    assert_eq!(offs[0], 0, "{label}: first page starts at byte 0");
    for w in offs.windows(2) {
        assert!(w[0] < w[1], "{label}: page offsets strictly increase: {offs:?}");
    }
    for (i, p) in pages.iter().enumerate() {
        assert_eq!(p.index, i, "{label}: page numbers are consecutive");
        assert_eq!(p.offset, offs[i]);
        let o = p.offset as usize;
        assert!(o < data.len().max(1), "{label}: page {i} starts inside the file");
        assert!(o == 0 || (data[o] & 0xC0) != 0x80, "{label}: page {i} starts in the middle of a UTF-8 character (byte {o})");
    }

    // (2) every page's text is exactly the file's text between its offset and the next page's offset
    for (i, p) in pages.iter().enumerate() {
        let end = if i + 1 < pages.len() { pages[i + 1].offset as usize } else { data.len() };
        assert_eq!(
            strip_ws(&text_of(&p.lines)),
            strip_ws(&data[p.offset as usize..end]),
            "{label}: page {i} text != file bytes [{}..{end})",
            p.offset
        );
    }

    // (3) text conservation over the whole book
    assert_eq!(strip_ws(&text_of(&all_lines(pages))), strip_ws(data), "{label}: text lost, duplicated or altered");

    // (4) geometry of every line / page
    for (i, p) in pages.iter().enumerate() {
        assert!(p.lines.len() <= max, "{label}: page {i} has {} lines > capacity {max}", p.lines.len());
        for l in &p.lines {
            assert!(!l.contains(&b'\r') && !l.contains(&b'\n'), "{label}: page {i}: terminator inside a line {:?}", String::from_utf8_lossy(l));
            let w = width(font, l); // also asserts the line is valid UTF-8 (no cut character)
            assert!(w <= text_w, "{label}: page {i}: line {:?} is {w}px > {text_w}px", String::from_utf8_lossy(l));
        }
    }
}

// whole pages: every page but the last is full, the last has 1..=max lines,
// and total_pages = ceil(total_lines / max_lines)
fn check_page_fill(r: &Rig, pages: &[Page], label: &str) {
    let max = r.max_lines();
    let total_lines: usize = pages.iter().map(|p| p.lines.len()).sum();
    for p in &pages[..pages.len() - 1] {
        assert_eq!(p.lines.len(), max, "{label}: page {} is not full", p.index);
    }
    let last = pages.last().unwrap().lines.len();
    assert!((1..=max).contains(&last), "{label}: last page has {last} lines (capacity {max})");
    assert_eq!(pages.len(), ceil_div(total_lines, max), "{label}: {total_lines} lines at {max} per page");
}

// ---------------------------------------------------------------------------
// geometry
// ---------------------------------------------------------------------------

#[test]
fn text_geometry_is_consistent_for_every_font_and_theme() {
    let mut prev_line_h = [0u16; 4];
    for font in 0..5u8 {
        for theme in 0..4u8 {
            let r = open_book(b"x", font, theme);
            let label = format!("font {font} theme {theme}");
            let margin = r.text_margin() as u32;
            assert_eq!(r.text_w(), SCREEN_W - 2 * margin, "{label}: text width is the screen minus both margins");
            let (max, lh, area) = (r.max_lines() as u32, r.font_line_h() as u32, r.text_area_h() as u32);
            assert!(max >= 1, "{label}");
            assert!(max * lh <= area, "{label}: {max} lines x {lh}px must fit {area}px");
            assert!((max + 1) * lh > area, "{label}: capacity is the largest line count that fits ({max} x {lh} vs {area})");
            assert!(r.font_line_h() >= prev_line_h[theme as usize], "{label}: larger font never has a shorter line");
            prev_line_h[theme as usize] = r.font_line_h();
        }
    }
}

// ---------------------------------------------------------------------------
// text conservation, UTF-8, width, page table
// ---------------------------------------------------------------------------

#[test]
fn english_prose_pages_conserve_the_text_and_tile_the_file() {
    let book = prose(1, 30_000, "\n");
    for (font, theme) in CONFIGS {
        let mut r = open_book(&book.bytes, font, theme);
        let pages = walk_forward(&mut r);
        assert!(pages.len() > 4, "font {font} theme {theme}: a 30 kB book spans many pages");
        check_well_formed(&r, &pages, &book.bytes, font, &format!("prose font {font} theme {theme}"));
    }
}

#[test]
fn prose_page_count_is_ceil_of_lines_over_page_capacity() {
    let book = prose(2, 30_000, "\n");
    for (font, theme) in CONFIGS {
        let mut r = open_book(&book.bytes, font, theme);
        let pages = walk_forward(&mut r);
        let total_lines: usize = pages.iter().map(|p| p.lines.len()).sum();
        assert!(total_lines > 300, "a long document: {total_lines} lines");
        check_page_fill(&r, &pages, &format!("prose font {font} theme {theme}"));
    }
}

// Greedy word wrap: a line that does not end its paragraph ended because the next
// word did not fit. For consecutive non-empty lines k, k+1 of one paragraph:
//   width(line k without trailing spaces) + width(' ') + width(first word of line k+1) > text width
#[test]
fn wrapping_fills_each_line_until_the_next_word_would_not_fit() {
    let book = prose(3, 30_000, "\n");
    let mut ends = HashSet::new();
    let mut acc = 0usize;
    for p in &book.paragraphs {
        acc += strip_ws(p.as_bytes()).len();
        ends.insert(acc);
    }
    for (font, theme) in CONFIGS {
        let mut r = open_book(&book.bytes, font, theme);
        let lines = all_lines(&walk_forward(&mut r));
        let fs = FontSet::for_size(font);
        let space = fs.advance(' ', Style::Regular) as u32;
        let text_w = r.text_w();
        let mut acc = 0usize;
        let mut checked = 0;
        for k in 0..lines.len() - 1 {
            acc += strip_ws(&lines[k]).len();
            let (a, b) = (&lines[k], &lines[k + 1]);
            if strip_ws(a).is_empty() || strip_ws(b).is_empty() || ends.contains(&acc) {
                continue; // blank line, or a paragraph really ends here
            }
            let cur = std::str::from_utf8(a).unwrap().trim_end_matches(' ');
            let next = std::str::from_utf8(b).unwrap().trim_start_matches(' ');
            let word = next.split(' ').next().unwrap();
            let need = width(font, cur.as_bytes()) + space + width(font, word.as_bytes());
            assert!(need > text_w, "font {font} theme {theme}: line {k} {cur:?} would also have fit {word:?} ({need}px <= {text_w}px)");
            checked += 1;
        }
        assert!(checked > 200, "font {font} theme {theme}: only {checked} wrapped line pairs were checked");
    }
}

#[test]
fn crlf_book_lays_out_exactly_like_the_lf_book_and_no_cr_survives() {
    let lf = prose(4, 20_000, "\n");
    let crlf = prose(4, 20_000, "\r\n");
    assert!(crlf.bytes.len() > lf.bytes.len());
    for (font, theme) in [(2u8, 1u8), (0, 0)] {
        let mut a = open_book(&lf.bytes, font, theme);
        let mut b = open_book(&crlf.bytes, font, theme);
        let (pa, pb) = (walk_forward(&mut a), walk_forward(&mut b));
        check_well_formed(&b, &pb, &crlf.bytes, font, "crlf");
        assert_eq!(pa.len(), pb.len(), "font {font} theme {theme}");
        for (x, y) in pa.iter().zip(&pb) {
            assert_eq!(x.lines, y.lines, "font {font} theme {theme} page {}", x.index);
        }
    }
}

// the character-level stress: long unbreakable runs of one multi-byte character are
// split mid-token, every file size / start phase moves where read chunks and page
// buffers fall inside a character
#[test]
fn multibyte_characters_are_never_cut_by_line_page_or_read_chunk_boundaries() {
    let runs: Vec<(String, String)> = vec![
        ("e-acute".into(), "\u{e9}".repeat(6000)),
        ("em-dash".into(), "\u{2014}".repeat(4000)),
        ("ellipsis".into(), "\u{2026}".repeat(4000)),
        ("left-quote".into(), "\u{201c}".repeat(4000)),
        ("euro".into(), "\u{20ac}".repeat(4000)),
        ("ascii-W".into(), "W".repeat(3000)),
    ];
    for (name, run) in &runs {
        for pad in 0..16usize {
            let data = format!("{}\n{run}\nend of run", "a".repeat(pad)).into_bytes();
            let mut r = open_book(&data, 2, 1);
            let pages = walk_forward(&mut r);
            let label = format!("{name} pad {pad}");
            check_well_formed(&r, &pages, &data, 2, &label);
            let all = String::from_utf8(text_of(&all_lines(&pages))).unwrap();
            let ch = run.chars().next().unwrap();
            assert_eq!(all.chars().filter(|&c| c == ch).count(), run.chars().count(), "{label}: character count");
            assert!(!all.contains('\u{FFFD}'), "{label}: no replacement characters in valid text");
            let lines_with_run = all_lines(&pages).iter().filter(|l| l.starts_with(ch.encode_utf8(&mut [0; 4]).as_bytes())).count();
            assert!(lines_with_run >= 5, "{label}: the run must be split over several lines, got {lines_with_run}");
        }
    }
}

#[test]
fn dense_multibyte_prose_survives_every_start_phase() {
    for pad in 0..16usize {
        let book = prose(100 + pad as u64, 20_000, "\n");
        let mut data = "a".repeat(pad).into_bytes();
        data.push(b'\n');
        data.extend_from_slice(&book.bytes);
        for (font, theme) in [(2u8, 1u8), (4, 3)] {
            let mut r = open_book(&data, font, theme);
            let pages = walk_forward(&mut r);
            check_well_formed(&r, &pages, &data, font, &format!("pad {pad} font {font} theme {theme}"));
        }
    }
}

// Damaged bytes (RFC 3629 violations) must become U+FFFD, never a panic, never lost text.
// (Overlong forms are pinned in tests/utf8.rs, kept out here so this test is about the reader.)
// Every bad sequence is followed by a space, so the expectation does not depend on how
// many bytes of an invalid sequence a decoder resynchronises over.
#[test]
fn damaged_utf8_becomes_replacement_characters_and_the_surrounding_text_survives() {
    let bad: [(&str, &[u8]); 6] = [
        ("stray continuation", &[0x80]),
        ("0xFF", &[0xFF]),
        ("surrogate ED A0 80", &[0xED, 0xA0, 0x80]),
        ("above U+10FFFF F4 90 80 80", &[0xF4, 0x90, 0x80, 0x80]),
        ("truncated E4 B8 mid-file", &[0xE4, 0xB8]),
        ("truncated E4 B8 at EOF", &[0xE4, 0xB8]),
    ];
    let filler = prose(7, 1_500, "\n");
    for pad in 0..8usize {
        let mut data = "a".repeat(pad).into_bytes();
        let mut expect = "a".repeat(pad).into_bytes();
        for (i, (_, seq)) in bad.iter().enumerate() {
            let word = format!("\nmark{i} ");
            data.extend_from_slice(word.as_bytes());
            expect.extend_from_slice(word.as_bytes());
            data.extend_from_slice(seq);
            if i + 1 < bad.len() {
                data.push(b' ');
                data.extend_from_slice(filler.bytes.as_slice());
                expect.extend_from_slice(filler.bytes.as_slice());
            }
        }
        let mut r = open_book(&data, 2, 1);
        assert_eq!(r.phase(), Phase::Ready, "pad {pad}: damaged text still opens");
        let pages = walk_forward(&mut r);
        assert_eq!(r.phase(), Phase::Ready);
        let shown = String::from_utf8_lossy(&text_of(&all_lines(&pages))).into_owned();
        let replacements = shown.chars().filter(|&c| c == '\u{FFFD}').count();
        assert!(replacements >= bad.len(), "pad {pad}: {replacements} U+FFFD for {} damaged sequences", bad.len());
        let kept: Vec<u8> = strip_ws(shown.replace('\u{FFFD}', "").as_bytes());
        assert_eq!(kept, strip_ws(&expect), "pad {pad}: text around the damage must be intact");
        // every page boundary is still a character boundary or damaged byte, never a panic path
        let offs = r.page_offsets();
        assert!(offs.windows(2).all(|w| w[0] < w[1]), "pad {pad}: offsets increase");
    }
}

// ---------------------------------------------------------------------------
// exact line / page counts by construction
// ---------------------------------------------------------------------------

fn capacity(font: u8, theme: u8) -> usize {
    open_book(b"x", font, theme).max_lines()
}

#[test]
fn short_lines_page_as_exactly_ceil_n_over_capacity() {
    for (font, theme) in [(0u8, 0u8), (2, 1), (4, 3)] {
        let max = capacity(font, theme);
        assert!(max >= 10);
        let counts = [1, max - 1, max, max + 1, 2 * max - 1, 2 * max, 2 * max + 1, 3 * max - 1, 3 * max, 7 * max + 3];
        for n in counts {
            for (eol, trailing) in [("\n", false), ("\r\n", false), ("\n", true), ("\r\n", true)] {
                let (data, expected) = numbered(n, eol, trailing);
                let mut r = open_book(&data, font, theme);
                let pages = walk_forward(&mut r);
                let label = format!("font {font} theme {theme}: {n} lines, eol {eol:?}, trailing {trailing}");
                assert_eq!(pages.len(), ceil_div(n, max), "{label}");
                assert_eq!(all_lines(&pages), expected, "{label}: every source line is one displayed line, in order");
                for p in &pages[..pages.len() - 1] {
                    assert_eq!(p.lines.len(), max, "{label}: page {} is full", p.index);
                }
                assert_eq!(pages.last().unwrap().lines.len(), n - (pages.len() - 1) * max, "{label}: remainder on the last page");
                assert!(r.fully_indexed());
                assert_eq!(r.total_pages(), pages.len(), "{label}");
            }
        }
    }
}

#[test]
fn a_long_short_line_document_has_the_page_count_of_its_line_count() {
    let (data, expected) = numbered(700, "\n", false);
    let mut r = open_book(&data, 0, 0);
    let max = r.max_lines();
    let pages = walk_forward(&mut r);
    assert_eq!(pages.len(), ceil_div(700, max));
    assert_eq!(all_lines(&pages), expected);
    check_well_formed(&r, &pages, &data, 0, "700 lines");
}

#[test]
fn blank_lines_are_kept_as_empty_lines_in_place() {
    for (font, theme) in [(0u8, 0u8), (2, 1)] {
        let max = capacity(font, theme);
        let (data, expected) = numbered_with_blanks(3 * max + 2);
        let mut r = open_book(&data, font, theme);
        let pages = walk_forward(&mut r);
        assert_eq!(all_lines(&pages), expected, "font {font} theme {theme}");
        assert_eq!(pages.len(), 4);
    }
}

#[test]
fn empty_and_one_character_files() {
    let mut r = open_book(b"", 2, 1);
    assert_eq!(r.phase(), Phase::Ready, "an empty file opens");
    assert_eq!(r.page(), 0);
    assert_eq!(r.total_pages(), 1);
    assert!(r.fully_indexed());
    assert!(r.lines().is_empty(), "nothing to show");
    r.press(Action::Next);
    r.press(Action::Prev);
    assert_eq!((r.phase(), r.page(), r.total_pages()), (Phase::Ready, 0, 1));

    let mut r = open_book(b"x", 2, 1);
    assert_eq!(r.lines(), vec![b"x".to_vec()]);
    assert_eq!(walk_forward(&mut r).len(), 1);
    assert_eq!(r.total_pages(), 1);

    let r = open_book("\u{e9}".as_bytes(), 2, 1);
    assert_eq!(r.lines(), vec!["\u{e9}".as_bytes().to_vec()], "a lone two-byte character is one intact line");
}

// ---------------------------------------------------------------------------
// previous / next
// ---------------------------------------------------------------------------

#[test]
fn forward_n_then_backward_n_returns_to_page_zero_with_identical_content() {
    let book = prose(5, 30_000, "\n");
    for (font, theme) in CONFIGS {
        let mut r = open_book(&book.bytes, font, theme);
        let first = snapshot(&r);
        let fwd = walk_forward(&mut r);
        let n = fwd.len();
        assert!(n > 4);
        let mut back = walk_back(&mut r);
        assert_eq!(back.len(), n, "font {font} theme {theme}: same number of pages going back");
        back.reverse();
        assert_eq!(fwd, back, "font {font} theme {theme}: the same pages, with the same text and offsets, in reverse");
        assert_eq!(r.page(), 0);
        assert_eq!(snapshot(&r), first, "page 0 is unchanged after the round trip");
        // and once more forward: identical to the first walk
        assert_eq!(walk_forward(&mut r), fwd, "font {font} theme {theme}: second pass");
    }
}

#[test]
fn page_numbers_are_monotonic_and_content_is_a_function_of_the_page_only() {
    let book = prose(6, 30_000, "\n");
    let mut reference = open_book(&book.bytes, 2, 1);
    let want = walk_forward(&mut reference);
    let total = want.len();
    assert!(total > 20);

    // a fresh reader (page table built lazily as pages are turned) walked by a
    // deterministic random Next/Prev sequence must show exactly the reference pages
    let mut r = open_book(&book.bytes, 2, 1);
    let mut rng = Lcg(42);
    let mut model = 0usize;
    for step in 0..600 {
        let next = rng.below(100) < 65;
        if next {
            r.press(Action::Next);
            model = (model + 1).min(total - 1);
        } else {
            r.press(Action::Prev);
            model = model.saturating_sub(1);
        }
        assert_eq!(r.phase(), Phase::Ready, "step {step}");
        assert_eq!(r.page(), model, "step {step}: page number follows Next/Prev with clamping");
        let got = snapshot(&r);
        assert_eq!(got, want[model], "step {step}: page {model} shows the same text and offset however it was reached");
        assert!(r.total_pages() > model && r.total_pages() <= total, "step {step}: table size {}", r.total_pages());
    }
}

#[test]
fn first_page_has_no_previous_and_last_page_has_no_next() {
    let book = prose(8, 30_000, "\n");
    let mut r = open_book(&book.bytes, 2, 1);
    let first = snapshot(&r);
    for a in [Action::Prev, Action::PrevJump, Action::Prev] {
        r.press(a);
        assert_eq!(r.phase(), Phase::Ready);
        assert_eq!(snapshot(&r), first, "{a:?} on page 0 stays on page 0");
    }
    let pages = walk_forward(&mut r);
    let last = pages.last().unwrap().clone();
    assert_eq!(last.index, pages.len() - 1);
    for a in [Action::Next, Action::NextJump, Action::Next] {
        r.press(a);
        assert_eq!(r.phase(), Phase::Ready);
        assert_eq!(snapshot(&r), last, "{a:?} on the last page stays on the last page");
        assert_eq!(r.total_pages(), pages.len(), "the page table does not grow past the end");
    }
}

#[test]
fn jumps_move_a_fixed_distance_clamp_at_both_ends_and_show_the_right_page() {
    let book = prose(9, 30_000, "\n");
    let mut r = open_book(&book.bytes, 2, 1);
    let want = walk_forward(&mut r); // index everything
    let last = want.len() - 1;
    assert!(last >= 25, "need a book of at least 26 pages, got {}", want.len());
    walk_back(&mut r);
    assert_eq!(r.page(), 0);

    r.press(Action::NextJump);
    let k = r.page();
    // existing behaviour (pre-port baseline suite, reader-regression navigation.rs): a jump is 10 pages
    assert_eq!(k, 10, "NextJump distance");
    assert_eq!(snapshot(&r), want[k]);
    r.press(Action::NextJump);
    assert_eq!(snapshot(&r), want[2 * k], "same distance again");
    r.press(Action::PrevJump);
    assert_eq!(snapshot(&r), want[k]);
    r.press(Action::PrevJump);
    assert_eq!(snapshot(&r), want[0]);
    r.press(Action::PrevJump);
    assert_eq!(snapshot(&r), want[0], "PrevJump clamps at page 0");

    // walk to the end by jumps: always lands on a real page, ends clamped on the last one
    let mut guard = 0;
    while r.page() != last {
        let before = r.page();
        r.press(Action::NextJump);
        assert_eq!(r.page(), (before + k).min(last), "NextJump from {before}");
        assert_eq!(snapshot(&r), want[r.page()]);
        guard += 1;
        assert!(guard < 100);
    }
    r.press(Action::NextJump);
    assert_eq!(snapshot(&r), want[last], "NextJump clamps at the last page");
    r.press(Action::PrevJump);
    assert_eq!(r.page(), last.saturating_sub(k));
    assert_eq!(snapshot(&r), want[last.saturating_sub(k)]);
}

// ---------------------------------------------------------------------------
// storage: same book through both backends, read volume, open failures
// ---------------------------------------------------------------------------

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pulp-host-paging-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("_PULP")).unwrap();
        Self(p)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn memory_card_and_host_directory_card_page_identically() {
    let book = prose(10, 30_000, "\n");
    let root = TempRoot::new();
    std::fs::write(root.0.join(BOOK), &book.bytes).unwrap();
    let mut host = Rig::new(VirtualStorage::host_dir(&root.0));
    host.configure(2, 1);
    host.open(BOOK);
    let mut mem = open_book(&book.bytes, 2, 1);
    let (h, m) = (walk_forward(&mut host), walk_forward(&mut mem));
    check_well_formed(&host, &h, &book.bytes, 2, "host dir");
    assert_eq!(h, m);
}

fn book_reads(r: &Rig) -> Vec<pulp_host::storage::ReadRecord> {
    r.storage().read_log().into_iter().filter(|x| x.path == BOOK).collect()
}

fn bytes_returned(recs: &[pulp_host::storage::ReadRecord]) -> usize {
    recs.iter().map(|x| x.returned).sum()
}

#[test]
fn a_page_turn_reads_a_small_part_of_the_book_not_the_whole_file() {
    // ~160 kB at the smallest font: > 60 pages, one page is ~2 kB of text
    let book = prose(11, 160_000, "\n");
    let size = book.bytes.len();
    let mut r = open_book(&book.bytes, 0, 0);
    let limit = size / 4;
    assert!(book_reads(&r).iter().all(|x| x.outcome == ReadOutcome::Ok), "healthy card: every read succeeds");

    let mut pages = 1;
    loop {
        r.storage().reset_reads();
        let before = r.page();
        r.press(Action::Next);
        if r.page() == before {
            break;
        }
        pages += 1;
        let got = bytes_returned(&book_reads(&r));
        assert!(got < limit, "turning to page {} read {got} bytes of a {size} byte book (limit {limit})", r.page());
    }
    assert!(pages > 60, "book spans {pages} pages");

    // coming back over already indexed pages is just as cheap
    for _ in 0..pages - 1 {
        r.storage().reset_reads();
        r.press(Action::Prev);
        let got = bytes_returned(&book_reads(&r));
        assert!(got < limit, "going back to page {} read {got} bytes of a {size} byte book (limit {limit})", r.page());
    }
    assert_eq!(r.page(), 0);
}

#[test]
fn reads_of_the_book_stay_inside_the_file_and_are_all_accounted_for() {
    let book = prose(12, 40_000, "\n");
    let size = book.bytes.len();
    let mut r = open_book(&book.bytes, 2, 1);
    walk_forward(&mut r);
    let recs = book_reads(&r);
    assert!(recs.len() >= 3, "a 40 kB book is not read in fewer than 3 reads, got {}", recs.len());
    assert_eq!(r.storage().read_count(), r.storage().read_log().len());
    for x in &recs {
        assert_eq!(x.outcome, ReadOutcome::Ok);
        assert!(x.offset as usize <= size, "read at {} beyond the {size} byte file", x.offset);
        assert!(x.returned <= x.requested);
        assert!(x.returned <= size - x.offset as usize, "a read cannot return bytes past EOF");
    }
}

#[test]
fn opening_a_missing_book_is_an_error_page_with_open_file() {
    let s = VirtualStorage::memory();
    s.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(s);
    r.configure(2, 1);
    r.open("GONE.TXT");
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::OpenFile));
}

// ---------------------------------------------------------------------------
// fault injection
// ---------------------------------------------------------------------------

// one walk that tolerates failure: stops at the end of the book, on the first
// non-Ready phase, or when a press does not move the page
fn walk_tolerant(r: &mut Rig) -> Vec<Page> {
    let mut out = Vec::new();
    if r.phase() != Phase::Ready {
        return out;
    }
    out.push(snapshot(r));
    for _ in 0..2000 {
        let before = r.page();
        r.press(Action::Next);
        if r.phase() != Phase::Ready || r.page() == before {
            return out;
        }
        assert_eq!(r.page(), before + 1, "Next moves exactly one page");
        out.push(snapshot(r));
    }
    panic!("walk_tolerant runaway");
}

// the reads of the book made by open + a full walk on a healthy card, plus the pages
fn pristine(data: &[u8], font: u8, theme: u8) -> (Vec<pulp_host::storage::ReadRecord>, Vec<Page>) {
    let mut r = open_book(data, font, theme);
    let pages = walk_forward(&mut r);
    (book_reads(&r), pages)
}

fn sample_indices(len: usize) -> Vec<usize> {
    let mut v = vec![0, 1, len / 2, len - 2, len - 1];
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn a_read_error_while_opening_is_an_error_page_not_a_panic() {
    let book = prose(13, 30_000, "\n");
    // the injected kind reaches the error page (pre-port baseline: ReadFailed -> ReadFailed)
    let mut r = fresh_rig(&book.bytes, 2, 1);
    r.storage().inject_error(StorageOp::Read, BOOK, 1, ErrorKind::ReadFailed);
    r.open(BOOK);
    assert_eq!(r.storage().pending_injections(), 0, "the injection fired during open");
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::ReadFailed));

    for kind in [ErrorKind::SeekFailed, ErrorKind::OpenFile] {
        let mut r = fresh_rig(&book.bytes, 2, 1);
        r.storage().inject_error(StorageOp::Read, BOOK, 1, kind);
        r.open(BOOK);
        assert_eq!(r.storage().pending_injections(), 0, "{kind:?}: fired");
        assert_eq!(r.phase(), Phase::Error, "{kind:?}");
        assert!(r.error_kind().is_some(), "{kind:?}: the error page names a kind");
    }
}

#[test]
fn a_read_error_during_paging_never_shows_wrong_content() {
    let book = prose(14, 100_000, "\n");
    let (log, want) = pristine(&book.bytes, 0, 0);
    assert!(log.len() >= 3 && want.len() > 30);
    for j in sample_indices(log.len()) {
        let mut r = fresh_rig(&book.bytes, 0, 0);
        r.storage().inject_error(StorageOp::Read, BOOK, j + 1, ErrorKind::ReadFailed);
        r.open(BOOK);
        let pages = walk_tolerant(&mut r);
        assert_eq!(r.storage().pending_injections(), 0, "read #{}: the injection must fire (same read sequence as the healthy run)", j + 1);
        match r.phase() {
            Phase::Error => assert_eq!(r.error_kind(), Some(ErrorKind::ReadFailed), "read #{}", j + 1),
            Phase::Ready => {}
            other => panic!("read #{}: unexpected phase {other:?}", j + 1),
        }
        for p in &pages {
            assert_eq!(p, &want[p.index], "read #{}: page {} shown while a read failed is wrong", j + 1, p.index);
        }
        assert!(pages.len() <= want.len());
    }
}

// A transient read error must not be mistaken for the end of the book: once the card
// answers again, the reader is either in the error state or can still reach every page.
#[test]
fn a_transient_read_error_is_not_mistaken_for_the_end_of_the_book() {
    let book = prose(15, 100_000, "\n");
    let (log, want) = pristine(&book.bytes, 0, 0);
    assert!(log.len() >= 3 && want.len() > 30);
    for j in sample_indices(log.len()) {
        let mut r = fresh_rig(&book.bytes, 0, 0);
        r.storage().inject_error(StorageOp::Read, BOOK, j + 1, ErrorKind::ReadFailed);
        r.open(BOOK);
        let mut pages = walk_tolerant(&mut r);
        if r.phase() == Phase::Error {
            continue; // reported as an error: acceptable
        }
        // the walk stopped early on a Ready reader: try once more, the fault is gone
        if pages.len() < want.len() {
            r.press(Action::Next);
            if r.phase() == Phase::Error {
                continue; // the retry surfaced an error: acceptable
            }
            if r.page() == pages.len() {
                pages.extend(walk_tolerant(&mut r)); // first element is the page just reached
            }
        }
        assert_eq!(pages, want, "read #{}: every page of the book is reachable after a transient error", j + 1);
    }
}

// A short read (fewer bytes than asked, not at EOF) must not corrupt what is shown:
// the reader reports an error, shows the whole book, or shows a correct prefix of it.
// The cut is placed at an ASCII byte so the test stays about the reader, not about a
// half character in the card's data.
#[test]
fn a_short_read_never_corrupts_the_text_that_is_shown() {
    let book = prose(16, 100_000, "\n");
    let data = &book.bytes;
    let (log, want) = pristine(data, 0, 0);
    assert!(log.len() >= 3, "{} reads", log.len());
    let full = strip_ws(data);
    let mut tried = 0;
    for j in sample_indices(log.len()) {
        let rec = &log[j];
        if rec.returned < 2 {
            continue;
        }
        let Some(k) = (1..=(rec.returned / 2).max(1)).rev().find(|k| data[rec.offset as usize + k] < 0x80) else {
            continue;
        };
        tried += 1;
        let mut r = fresh_rig(data, 0, 0);
        r.storage().inject_short_read(BOOK, j + 1, k);
        r.open(BOOK);
        let pages = walk_tolerant(&mut r);
        assert_eq!(r.storage().pending_injections(), 0, "read #{}: the injection must fire", j + 1);
        match r.phase() {
            Phase::Error => assert!(r.error_kind().is_some(), "read #{}", j + 1),
            Phase::Ready => {
                for p in &pages {
                    // every character shown is a whole one
                    for l in &p.lines {
                        assert!(std::str::from_utf8(l).is_ok(), "read #{}: page {} line cut inside a character", j + 1, p.index);
                    }
                }
                let shown = strip_ws(&text_of(&all_lines(&pages)));
                assert!(full.starts_with(&shown), "read #{}: shown text is not a prefix of the book (a gap or garbage appeared)", j + 1);
                assert!(pages.len() <= want.len(), "read #{}", j + 1);
            }
            other => panic!("read #{}: unexpected phase {other:?}", j + 1),
        }
    }
    assert!(tried >= 3, "only {tried} short-read samples were usable");
}

// An EPUB chapter whose text reaches the end of the page buffer: the page is laid out up to
// the last whole scalar of the read and the rest is left for the page that starts there; a
// scalar cut by the end of the buffer is never shown, as half a character or as U+FFFD.
// Only the end of the chapter's own text is the end of the text.
//
// Run: scripts/host-test.sh --test utf8_epub
//
// How the chapter reaches the buffer end: U+00AD (soft hyphen) has no width, so a run of
// them stays on one line however long it is. A paragraph "run of soft hyphens, then CJK
// text" fills the 8192-byte buffer with a few lines of the text, well under the page's
// line capacity, so the layout meets the buffer end inside the text. The number of soft
// hyphens and the ASCII padding in front move that end over every byte alignment of 2-,
// 3- and 4-byte scalars of the CJK text.
//
// Oracle: RFC 3629 (a line of valid text is valid UTF-8; `str::from_utf8` judges) and the
// text of the chapter itself: what the pages show, markers and white space aside, is the
// beginning of the chapter's text in whole scalars, none repeated, none skipped.

mod utf8_common;

use std::sync::{Mutex, MutexGuard};

use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
};
use pulp_host::reader::{Action, PAGE_BUF, Phase, Rig};
use pulp_host::storage::VirtualStorage;
use smol_epub::html_strip::MARKER;
use utf8_common::*;

const SHY: &str = "\u{ad}";

// the image worker behind the reader is one task process-wide: one reader at a time
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn epub(text: &str, numeric_entities: bool) -> Vec<u8> {
    build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Buffer end".to_string(),
        author: "Test Author".to_string(),
        identifier: "urn:pulp-os:fixture:buffer-end".to_string(),
        chapters: vec![Chapter {
            title: "One".to_string(),
            blocks: vec![Block::Paragraph(vec![Run::Text(text.to_string())])],
        }],
        toc: vec![TocItem {
            title: "One".to_string(),
            chapter: 0,
            children: vec![],
        }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities,
    })
    .expect("a one-chapter text EPUB builds")
}

// the text of the page's lines: markers removed, as raw bytes
fn page_bytes(r: &Rig) -> Vec<u8> {
    let mut out = Vec::new();
    for l in r.line_infos().iter().filter(|l| !l.image) {
        let b = &l.bytes;
        let mut i = 0;
        while i < b.len() {
            if b[i] == MARKER {
                i += 2;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

// Open the book and walk every page of the chapter; each page's text must be valid UTF-8
// with no U+FFFD, and the pages together a prefix of `expected` (white space aside).
fn check_chapter(name: &str, bytes: &[u8], expected: &str, label: &str) {
    let card = VirtualStorage::memory_with(&[(name, bytes)]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open(name);
    assert_eq!(r.phase(), Phase::Ready, "{label}");
    let mut shown = String::new();
    for _ in 0..50 {
        let text = page_bytes(&r);
        let page = std::str::from_utf8(&text).unwrap_or_else(|e| {
            panic!(
                "{label}: page {} text is not valid UTF-8 ({e}): ...{:02X?}",
                r.page(),
                &text[text.len().saturating_sub(6)..]
            )
        });
        assert!(
            !page.contains(FFFD),
            "{label}: page {} shows U+FFFD",
            r.page()
        );
        shown.extend(page.chars().filter(|c| !c.is_whitespace()));
        let before = (r.chapter(), r.page());
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready, "{label}");
        if (r.chapter(), r.page()) == before {
            break;
        }
    }
    let want: String = expected.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        want.starts_with(&shown),
        "{label}: the pages' text is not the beginning of the chapter's text"
    );
    assert!(
        shown.chars().count() > 3000,
        "{label}: only {} scalars shown, the sweep did not reach the buffer end",
        shown.chars().count()
    );
}

// The soft hyphens leave `PAGE_BUF - 14 - 2 * shy` bytes of CJK text in the page buffer,
// 14 being the heading's markers and the title's bytes in front. m is swept so that the
// CJK part is 0..= ~2400 bytes: fewer than 20 lines (the page's capacity) up to a page that
// ends on its line count just before the buffer end.
#[test]
fn a_chapter_page_reaching_the_end_of_the_buffer_never_shows_a_cut_scalar() {
    let _serial = serial();
    let units = [("tai", TAI), ("yoshi", YOSHI), ("e-acute", E_ACUTE)];
    for (name, unit) in units {
        for pad in 0..4 {
            for m in (PAGE_BUF / 2 - 1300..PAGE_BUF / 2 - 4).step_by(7) {
                let text = format!(
                    "{}{}{}",
                    "a".repeat(pad),
                    SHY.repeat(m),
                    repeat_to(unit, 3000)
                );
                for entities in [false, true] {
                    let bytes = epub(&text, entities);
                    let label = format!("{name}, pad {pad}, {m} soft hyphens, entities {entities}");
                    check_chapter("BOOK.EPU", &bytes, &format!("One{text}"), &label);
                }
            }
        }
    }
}

// one soft hyphen more at a time over a stretch where the buffer end moves through every
// alignment: the same, step 1
#[test]
fn a_chapter_page_ending_at_every_byte_of_a_scalar_never_shows_a_cut_scalar() {
    let _serial = serial();
    let m0 = (PAGE_BUF - 700) / 2;
    for (name, unit) in [("tai", TAI), ("yoshi", YOSHI), ("e-acute", E_ACUTE)] {
        for pad in 0..4 {
            for m in m0..m0 + 12 {
                let text = format!(
                    "{}{}{}",
                    "a".repeat(pad),
                    SHY.repeat(m),
                    repeat_to(unit, 3000)
                );
                let bytes = epub(&text, false);
                let label = format!("{name}, pad {pad}, {m} soft hyphens");
                check_chapter("BOOK.EPU", &bytes, &format!("One{text}"), &label);
            }
        }
    }
}

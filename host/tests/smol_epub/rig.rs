// End to end: the real ReaderApp opens EPUBs whose title, TOC labels and chapters are
// Traditional Chinese, and what it holds (smol-epub's parsers feeding the reader's page
// buffer) is the text that went into the book.
//
// Run: cargo test-host --test smol_rig
//
// Oracle: the strings written into the archive (written here as `char` code points and
// ASCII), `String::from_utf8_lossy` for anything with malformed bytes, and the HTML5 numeric
// reference rules for the references. The page text is read from `line_infos` with the style
// markers removed and white space ignored (line breaks are the layout's business).

use crate::smol_common;

use std::sync::{Mutex, MutexGuard};

use pulp_host::ErrorKind;
use pulp_host::fixtures::{RawEntry, build_zip};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::storage::VirtualStorage;
use smol_common::*;
use smol_epub::epub::TOC_TITLE_CAP;
use smol_epub::html_strip::MARKER;

// the image worker behind the reader is one task process-wide: one reader at a time
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// "Taiwan traditional Chinese" in corner quotes, comma, and one 2- and one 4-byte scalar
const TITLE: &str = "\u{81fa}\u{7063}\u{300c}\u{7e41}\u{9ad4}\u{4e2d}\u{6587}\u{300d}";
const AUTHOR: &str = "\u{ff0c}\u{e9}\u{20bb7}";
const LABEL_1: &str = "\u{7b2c}\u{4e00}\u{7ae0}";
const LABEL_2: &str = "\u{7b2c}\u{4e8c}\u{7ae0}";
const PARA_1: &str =
    "\u{81fa}\u{7063}\u{300c}\u{7e41}\u{9ad4}\u{4e2d}\u{6587}\u{300d}\u{ff0c}\u{20bb7}\u{e9}";
const PARA_2: &str = "\u{4e2d}\u{6587}\u{ff0c}\u{7e41}\u{9ad4}\u{3002}";

fn open(book: &Book) -> Rig {
    let card = VirtualStorage::memory_with(&[("BOOK.EPU", &build_book(book))]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("BOOK.EPU");
    r
}

// the text of the current page: markers removed, as bytes
fn page_bytes(r: &Rig) -> Vec<u8> {
    let mut out = Vec::new();
    for l in r.line_infos().iter().filter(|l| !l.image) {
        let b = &l.bytes;
        let mut i = 0;
        while i < b.len() {
            if b[i] == MARKER {
                i += 2;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    out
}

// the text of every page of the chapter (up to 60 pages), concatenated
fn chapter_bytes(r: &mut Rig) -> Vec<u8> {
    let mut all = Vec::new();
    for _ in 0..60 {
        all.extend(page_bytes(r));
        let before = (r.chapter(), r.page());
        r.press(Action::Next);
        if (r.chapter(), r.page()) == before {
            break;
        }
    }
    all
}

fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// two chapters (the first is `body_1` after a first paragraph break), Chinese metadata and TOC
fn chinese_book(ver: Ver, deflate: bool, chapter_1: &[u8]) -> Book {
    let mut b = Book::new(ver, vec![p_doc(chapter_1), p_doc(PARA_2.as_bytes())]);
    b.title = TITLE.as_bytes().to_vec();
    b.creator = AUTHOR.as_bytes().to_vec();
    b.toc_titles = vec![LABEL_1.as_bytes().to_vec(), LABEL_2.as_bytes().to_vec()];
    b.deflate = deflate;
    b
}

#[test]
fn a_chinese_epub_shows_its_title_author_toc_and_chapter_text() {
    let _serial = serial();
    for ver in [Ver::V2, Ver::V3] {
        for deflate in [false, true] {
            let label = format!("{ver:?}, deflate {deflate}");
            // chapter 1: plain text, then the same scalars as numeric references
            let refs: String = PARA_2
                .chars()
                .map(|c| format!("&#x{:X};", c as u32))
                .collect();
            let body_1 = format!("{PARA_1}</p><p>{refs}");
            let book = chinese_book(ver, deflate, body_1.as_bytes());
            let mut r = open(&book);
            assert_eq!(r.phase(), Phase::Ready, "{label}");
            assert_eq!(r.epub_title(), TITLE, "{label}: title");
            assert_eq!(r.epub_author(), AUTHOR, "{label}: author");
            assert_eq!(
                r.toc_entries(),
                vec![(LABEL_1.to_string(), 0), (LABEL_2.to_string(), 1)],
                "{label}: TOC"
            );
            let shown = chapter_bytes(&mut r);
            let shown = std::str::from_utf8(&shown)
                .unwrap_or_else(|e| panic!("{label}: chapter text is not valid UTF-8 ({e})"));
            assert!(!shown.contains(FFFD), "{label}: U+FFFD on a page");
            assert_eq!(
                squeeze(shown)
                    .chars()
                    .take(squeeze(PARA_1).chars().count() + squeeze(PARA_2).chars().count())
                    .collect::<String>(),
                squeeze(&format!("{PARA_1}{PARA_2}")),
                "{label}: chapter 1 text"
            );
        }
    }
}

#[test]
fn a_toc_label_longer_than_its_field_is_shown_cut_on_a_scalar_boundary() {
    let _serial = serial();
    // 48 / 3 = 16 whole scalars fit; one ASCII byte in front moves the cut inside a scalar
    for pad in 0..3 {
        let long = format!("{}{}", "a".repeat(pad), TAI.repeat(30));
        for ver in [Ver::V2, Ver::V3] {
            let mut book = Book::new(ver, vec![p_doc(PARA_1.as_bytes())]);
            book.title = TITLE.as_bytes().to_vec();
            book.toc_titles = vec![long.clone().into_bytes()];
            let r = open(&book);
            assert_eq!(r.phase(), Phase::Ready);
            let toc = r.toc_entries();
            assert_eq!(toc.len(), 1, "{ver:?} pad {pad}");
            assert_eq!(
                toc[0].0,
                fitting_prefix(&long, TOC_TITLE_CAP),
                "{ver:?} pad {pad}: the label the reader shows"
            );
        }
    }
}

#[test]
fn a_numeric_reference_to_no_scalar_shows_one_replacement_character_on_the_page() {
    let _serial = serial();
    let body = "a&#xD800;b&#x110000;c&#0;d&#x81FA;e&#xDFFF;f";
    let mut book = Book::new(Ver::V3, vec![p_doc(body.as_bytes())]);
    book.toc_titles = vec![b"x".to_vec()];
    let mut r = open(&book);
    assert_eq!(r.phase(), Phase::Ready);
    let page = chapter_bytes(&mut r);
    // consumer-side replacement (lossy decoding of whatever reached the page) must give
    // exactly one U+FFFD per reference
    assert_eq!(
        String::from_utf8_lossy(&page),
        "a\u{FFFD}b\u{FFFD}c\u{FFFD}d\u{81fa}e\u{FFFD}f"
    );
}

#[test]
fn malformed_bytes_in_a_chapter_show_as_replacement_characters_on_the_page() {
    let _serial = serial();
    let raw: &[u8] = b"a\x80b\xC0\x80c\xED\xA0\x80d\xF4\x90\x80\x80e\xE8\x87";
    let mut book = Book::new(Ver::V3, vec![p_doc(raw)]);
    book.toc_titles = vec![b"x".to_vec()];
    let mut r = open(&book);
    assert_eq!(r.phase(), Phase::Ready);
    let page = chapter_bytes(&mut r);
    assert_eq!(String::from_utf8_lossy(&page), String::from_utf8_lossy(raw));
}

#[test]
fn a_title_and_toc_label_with_malformed_bytes_are_shown_with_replacement_characters() {
    let _serial = serial();
    let raw: &[u8] = b"a\xE8\x87b\x80c";
    let want = String::from_utf8_lossy(raw).into_owned();
    for ver in [Ver::V2, Ver::V3] {
        let mut book = Book::new(ver, vec![p_doc(PARA_1.as_bytes())]);
        book.title = raw.to_vec();
        book.creator = raw.to_vec();
        book.toc_titles = vec![raw.to_vec()];
        let r = open(&book);
        assert_eq!(r.phase(), Phase::Ready, "{ver:?}");
        assert_eq!(r.epub_title(), want, "{ver:?}: title");
        assert_eq!(r.epub_author(), want, "{ver:?}: author");
        assert_eq!(r.toc_entries(), vec![(want.clone(), 0)], "{ver:?}: TOC");
    }
}

// a 257-entry archive whose only chapter is entry 257 (pinned in smol_zip_limit): the reader
// reports it rather than showing an empty book
#[test]
fn an_epub_with_its_chapter_past_the_256_entry_limit_known_limit_is_reported_as_a_parse_failure() {
    let _serial = serial();
    let book = Book::new(Ver::V3, vec![p_doc(PARA_1.as_bytes())]);
    let mut e = book_entries(&book);
    let ch = e.pop().unwrap();
    for i in 0..257 - 5 {
        e.push(RawEntry {
            name: format!("OEBPS/pad/f{i:03}.txt"),
            data: b"x".to_vec(),
            deflate: false,
        });
    }
    e.push(ch);
    let card = VirtualStorage::memory_with(&[("BOOK.EPU", &build_zip(&e))]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("BOOK.EPU");
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::ParseFailed));
}

// Titles cut to a fixed capacity: a TITLES.BIN line, a directory entry, a bookmark list
// entry, and the title stored in an EPUB's cache header. The cap is moved over every byte
// alignment of 2-, 3- and 4-byte scalars by padding the title with ASCII.
//
// Run: cargo test-host --test utf8_title
//
// Oracle: a title cut to N bytes must stay valid UTF-8 (RFC 3629; `str::from_utf8`
// judges), must be a prefix of the original title in whole scalars, must not be empty
// while the original is not (an unreadable title makes the listing fall back to the file
// name), and must hold no U+FFFD. The capacities are the production constants
// (`dir_entry::TITLE_CAP`, `smol_epub::cache::TITLE_CAP`). The stronger rule "the longest
// whole-scalar prefix that fits" is pinned apart (`*_keeps_the_longest_prefix_that_fits`).

mod utf8_common;

use std::sync::Mutex;

use pulp_host::dir_entry::{DirEntry, PULP_DIR, TITLE_CAP, TITLES_FILE, title_line};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::drivers::storage::save_title;
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
};
use pulp_host::kernel::bookmarks::BmListEntry;
use pulp_host::kernel::dir_cache::DirCache;
use pulp_host::reader::{Phase, Rig};
use pulp_host::storage::VirtualStorage;
use utf8_common::*;

const UNITS: [&str; 4] = [E_ACUTE, TAI, YOSHI, MIXED_UNIT];

// padding 0..PADS moves the cut over every byte alignment of every unit
const PADS: usize = 16;

// `pad` ASCII bytes, then `unit` repeated, at least `min` bytes long
fn long_title(pad: usize, unit: &str, min: usize) -> String {
    format!("{}{}", "a".repeat(pad), repeat_to(unit, min))
}

// the longest prefix of `s` on a scalar boundary that is at most `cap` bytes
fn fitting_prefix(s: &str, cap: usize) -> &str {
    let mut n = cap.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

// the title as stored in `bytes` must read back as a whole-scalar prefix of `title`
fn check_prefix(shown: &[u8], title: &str, label: &str) {
    let s = std::str::from_utf8(shown)
        .unwrap_or_else(|e| panic!("{label}: stored title is not valid UTF-8 ({e}): {shown:02X?}"));
    assert!(!s.is_empty(), "{label}: the title was lost");
    assert!(!s.contains(FFFD), "{label}: U+FFFD in {s:?}");
    assert!(
        title.starts_with(s),
        "{label}: {s:?} is not a prefix of {title:?}"
    );
}

// ---------------------------------------------------------------------------
// TITLES.BIN line
// ---------------------------------------------------------------------------

fn line_title(filename: &str, title: &str) -> Vec<u8> {
    let mut line = [0u8; 128];
    let n = title_line(filename, title, &mut line)
        .expect("a 12-byte name and a capped title fit 128 bytes");
    let line = &line[..n];
    assert!(line.ends_with(b"\n"), "a line ends with LF");
    let body = &line[..n - 1];
    let tab = body
        .iter()
        .position(|&b| b == b'\t')
        .expect("name TAB title");
    assert_eq!(
        &body[..tab],
        filename.as_bytes(),
        "the line starts with the file name"
    );
    body[tab + 1..].to_vec()
}

#[test]
fn titles_file_line_cuts_a_long_title_on_a_scalar_boundary() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let stored = line_title("BOOK.EPU", &title);
            assert!(
                stored.len() <= TITLE_CAP,
                "{unit:?} pad {pad}: {} bytes > cap {TITLE_CAP}",
                stored.len()
            );
            check_prefix(&stored, &title, &format!("title line, {unit:?}, pad {pad}"));
        }
    }
}

#[test]
fn titles_file_line_keeps_the_longest_prefix_that_fits() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            assert_eq!(
                line_title("BOOK.EPU", &title),
                fitting_prefix(&title, TITLE_CAP).as_bytes(),
                "{unit:?}, pad {pad}"
            );
        }
    }
}

#[test]
fn titles_file_line_keeps_a_title_that_fits_whole() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = fitting_prefix(&long_title(pad, unit, TITLE_CAP), TITLE_CAP).to_string();
            assert_eq!(
                line_title("BOOK.EPU", &title),
                title.as_bytes(),
                "{unit:?}, pad {pad}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// directory entry and bookmark list entry
// ---------------------------------------------------------------------------

#[test]
fn directory_entry_title_is_cut_on_a_scalar_boundary() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let mut e = DirEntry::EMPTY;
            e.set_title(title.as_bytes());
            let label = format!("dir entry, {unit:?}, pad {pad}");
            assert!(e.has_real_title(), "{label}: the entry has a title");
            check_prefix(e.display_name().as_bytes(), &title, &label);
        }
    }
}

#[test]
fn directory_entry_title_keeps_the_longest_prefix_that_fits() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let mut e = DirEntry::EMPTY;
            e.set_title(title.as_bytes());
            assert_eq!(
                e.display_name(),
                fitting_prefix(&title, TITLE_CAP),
                "{unit:?}, pad {pad}"
            );
        }
    }
}

#[test]
fn bookmark_list_entry_title_is_cut_on_a_scalar_boundary() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let mut e = BmListEntry::EMPTY;
            e.set_title(title.as_bytes());
            check_prefix(
                e.display_name().as_bytes(),
                &title,
                &format!("bookmark entry, {unit:?}, pad {pad}"),
            );
        }
    }
}

#[test]
fn bookmark_list_entry_title_keeps_the_longest_prefix_that_fits() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let mut e = BmListEntry::EMPTY;
            e.set_title(title.as_bytes());
            assert_eq!(
                e.display_name(),
                fitting_prefix(&title, TITLE_CAP),
                "{unit:?}, pad {pad}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// the directory cache: a title saved to TITLES.BIN and loaded back
// ---------------------------------------------------------------------------

fn listed_title(title: &str) -> String {
    let card = VirtualStorage::memory_with(&[("BOOK.EPU", b"x")]);
    card.ensure_pulp_dir().unwrap();
    let sd = SdStorage::new(card);
    save_title(&sd, "BOOK.EPU", title).expect("title saved");
    let mut cache = DirCache::new();
    cache.ensure_loaded(&sd).expect("directory loaded");
    let mut page = [DirEntry::EMPTY; 4];
    let got = cache.page(0, &mut page);
    assert_eq!(got.count, 1, "one book on the card");
    page[0].display_name().to_string()
}

#[test]
fn a_long_title_saved_and_listed_is_a_scalar_prefix_of_the_title() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let listed = listed_title(&title);
            assert_ne!(
                listed, "BOOK.EPU",
                "listed, {unit:?}, pad {pad}: the listing fell back to the file name"
            );
            check_prefix(
                listed.as_bytes(),
                &title,
                &format!("listed, {unit:?}, pad {pad}"),
            );
        }
    }
}

#[test]
fn a_title_stored_by_the_directory_cache_is_a_scalar_prefix_of_the_title() {
    for unit in UNITS {
        for pad in 0..PADS {
            let title = long_title(pad, unit, TITLE_CAP + 8);
            let card = VirtualStorage::memory_with(&[("BOOK.EPU", b"x")]);
            card.ensure_pulp_dir().unwrap();
            let sd = SdStorage::new(card);
            let mut cache = DirCache::new();
            cache.ensure_loaded(&sd).expect("directory loaded");
            cache.set_entry_title(0, title.as_bytes());
            let mut page = [DirEntry::EMPTY; 4];
            cache.page(0, &mut page);
            let listed = page[0].display_name();
            assert_ne!(
                listed, "BOOK.EPU",
                "set_entry_title, {unit:?}, pad {pad}: the listing fell back to the file name"
            );
            check_prefix(
                listed.as_bytes(),
                &title,
                &format!("set_entry_title, {unit:?}, pad {pad}"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// an EPUB's title through the reader: shown, saved to TITLES.BIN, stored in the cache header
// ---------------------------------------------------------------------------

// the image worker behind the reader is one task process-wide: one reader at a time
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn epub_with_title(title: &str) -> Vec<u8> {
    build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: title.to_string(),
        author: "Test Author".to_string(),
        identifier: "urn:pulp-os:fixture:title".to_string(),
        chapters: vec![Chapter {
            title: "One".to_string(),
            blocks: vec![Block::Paragraph(vec![Run::Text(
                "Some text of the chapter.".to_string(),
            )])],
        }],
        toc: vec![TocItem {
            title: "One".to_string(),
            chapter: 0,
            children: vec![],
        }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .expect("a title within smol-epub's capacity builds")
}

// the longest title the fixture builds (smol-epub's own capacity); the cache header's
// capacity is smaller, so every title in 49..=64 bytes is cut there
fn epub_title(pad: usize, unit: &str) -> String {
    let t = long_title(pad, unit, smol_epub::epub::TITLE_CAP);
    fitting_prefix(&t, smol_epub::epub::TITLE_CAP).to_string()
}

struct Opened {
    // the title the reader shows on the first open
    shown: String,
    // _PULP/TITLES.BIN after the first open
    titles_file: Vec<u8>,
    // the 128-byte header of the book's cache file, parsed by smol-epub's own parser
    header: smol_epub::cache::CacheHeader,
}

// Open the EPUB, let it cache, close; then open it again: the reads of the second open
// name the cache file in _PULP (the one whose first read is its 128-byte header).
fn open_and_cache(title: &str) -> Opened {
    let card = VirtualStorage::memory_with(&[("BOOK.EPU", &epub_with_title(title))]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("BOOK.EPU");
    assert_eq!(r.phase(), Phase::Ready, "the book opens");
    let shown = r.epub_title();
    for _ in 0..400 {
        if !r.has_bg_work() {
            break;
        }
        r.idle(1);
    }
    r.exit();
    let card = r.into_storage();

    let mut buf = vec![0u8; 1024];
    let n = card
        .read_file_chunk_in_dir(PULP_DIR, TITLES_FILE, 0, &mut buf)
        .expect("TITLES.BIN exists after an EPUB was opened");
    buf.truncate(n);

    card.reset_reads();
    let mut again = Rig::new(card);
    again.configure(2, 1);
    again.open("BOOK.EPU");
    let file = again
        .storage()
        .read_log()
        .into_iter()
        .find(|x| {
            x.path.starts_with("_PULP/_")
                && x.offset == 0
                && x.returned == smol_epub::cache::HEADER_SIZE
        })
        .expect("the reopened book reads its cache header")
        .path;
    let mut hdr = [0u8; smol_epub::cache::HEADER_SIZE];
    let name = &file["_PULP/".len()..];
    again
        .storage()
        .read_chunk_in_pulp(name, 0, &mut hdr)
        .expect("cache header readable");
    let header = smol_epub::cache::parse_v3_header(&hdr).expect("a v3 cache header");
    Opened {
        shown,
        titles_file: buf,
        header,
    }
}

#[test]
fn an_epub_title_that_fits_is_shown_and_saved_whole() {
    let _serial = serial();
    for unit in UNITS {
        for pad in 0..PADS {
            let title = epub_title(pad, unit);
            let o = open_and_cache(&title);
            let label = format!("{unit:?}, pad {pad}");
            assert_eq!(o.shown, title, "{label}: the title the reader shows");
            let want = format!("BOOK.EPU\t{title}\n");
            assert_eq!(o.titles_file, want.as_bytes(), "{label}: TITLES.BIN line");
        }
    }
}

// the cache header holds the title in a fixed field smaller than the title: what is
// stored must read back as a title
#[test]
fn the_title_in_an_epub_cache_header_is_a_scalar_prefix_of_the_title() {
    let _serial = serial();
    for unit in UNITS {
        for pad in 0..PADS {
            let title = epub_title(pad, unit);
            let hdr = open_and_cache(&title).header;
            let label = format!("cache header, {unit:?}, pad {pad}");
            // title_str() is "" when the stored bytes are not valid UTF-8
            check_prefix(hdr.title_str().as_bytes(), &title, &label);
            assert_eq!(
                hdr.title_len as usize,
                hdr.title_str().len(),
                "{label}: every stored byte is valid"
            );
        }
    }
}

#[test]
fn the_title_in_an_epub_cache_header_keeps_the_longest_prefix_that_fits() {
    let _serial = serial();
    for unit in UNITS {
        for pad in 0..PADS {
            let title = epub_title(pad, unit);
            let hdr = open_and_cache(&title).header;
            assert_eq!(
                hdr.title_str(),
                fitting_prefix(&title, smol_epub::cache::TITLE_CAP),
                "{unit:?}, pad {pad}"
            );
        }
    }
}

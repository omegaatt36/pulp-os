// The 256-entry limit of smol-epub's ZIP index, characterised (pinned as observed, not
// endorsed): what an archive with more entries than `ZipIndex` holds does.
//
// Run: scripts/host-test.sh --test smol_zip_limit
//
// Observed behaviour (unfixed vendored smol-epub, upstream 832609d):
//   * `parse_central_directory` returns Ok whatever the entry count; the index keeps the FIRST
//     `MAX_ENTRIES` (256) entries in archive order and silently ignores the rest. There is no
//     error and no flag the caller can read; the only trace is a `log::warn!` inside smol-epub.
//   * Up to and including 256 entries every entry is found and read.
//   * An entry past the 256th is `find() == None`: a spine chapter there is silently dropped
//     from the spine (if other chapters resolve) or, when none resolves, the OPF parse fails
//     with "epub: spine is empty after resolution". An OPF / TOC past the limit is not found.
// Tests named `known_limit` pin the behaviours a user would call losing content.
//
// Oracle: the archives are built entry by entry here (names, bytes, order), so which entry
// sits at which index is known from the construction; `ZipIndex::MAX_ENTRIES` is the public
// constant.

mod smol_common;

use pulp_host::fixtures::{RawEntry, build_zip};
use smol_common::*;
use smol_epub::zip::{MAX_ENTRIES, ZipIndex};

const FILLER: &str = "OEBPS/pad/f";

fn filler_entry(i: usize) -> RawEntry {
    RawEntry {
        name: format!("{FILLER}{i:03}.txt"),
        data: format!("pad {i}").into_bytes(),
        deflate: false,
    }
}

// where the book's parts sit among the filler entries
#[derive(Clone, Copy, Debug)]
enum Layout {
    // mimetype, container, OPF, TOC, chapters, then filler
    BookFirst,
    // mimetype, container, OPF, TOC, filler, then the chapters
    ChaptersLast,
    // mimetype, container, filler, then OPF, TOC, chapters
    PackageLast,
}

// `n` entries in all; `chapters` chapters, each "<p>chapter k</p>" with a CJK mark
fn archive(n: usize, layout: Layout, chapters: usize) -> Vec<u8> {
    let docs = (1..=chapters)
        .map(|k| p_doc(format!("chapter {k} \u{81fa}").as_bytes()))
        .collect();
    let mut book = Book::new(Ver::V3, docs);
    book.toc_titles = vec![b"first".to_vec()];
    let mut e = book_entries(&book);
    let parts = e.len();
    let filler: Vec<RawEntry> = (0..n - parts).map(filler_entry).collect();
    let out = match layout {
        Layout::BookFirst => [e, filler].concat(),
        Layout::ChaptersLast => {
            let ch = e.split_off(4);
            [e, filler, ch].concat()
        }
        Layout::PackageLast => {
            let tail = e.split_off(2);
            [e, filler, tail].concat()
        }
    };
    assert_eq!(out.len(), n);
    build_zip(&out)
}

fn index_of(bytes: &[u8]) -> ZipIndex {
    open_zip(bytes).expect("central directory of a well-formed archive is accepted")
}

#[test]
fn the_index_capacity_is_256_entries() {
    assert_eq!(MAX_ENTRIES, 256);
}

#[test]
fn zip_with_255_entries_opens_and_every_entry_is_found_and_read() {
    let bytes = archive(255, Layout::BookFirst, 1);
    let zip = index_of(&bytes);
    assert_eq!(zip.count(), 255);
    for i in 0..255 - 5 {
        let name = format!("{FILLER}{i:03}.txt");
        assert!(zip.find(&name).is_some(), "{name}");
        assert_eq!(
            entry_bytes(&bytes, &zip, &name).unwrap(),
            format!("pad {i}").into_bytes()
        );
    }
    let p = parse(&bytes);
    assert_eq!((p.spine.len(), p.toc.len()), (1, 1));
}

#[test]
fn zip_with_256_entries_opens_and_every_entry_is_found_and_read() {
    for layout in [Layout::BookFirst, Layout::ChaptersLast, Layout::PackageLast] {
        let bytes = archive(256, layout, 1);
        let zip = index_of(&bytes);
        assert_eq!(zip.count(), 256, "{layout:?}");
        for i in 0..256 - 5 {
            let name = format!("{FILLER}{i:03}.txt");
            assert_eq!(
                entry_bytes(&bytes, &zip, &name).unwrap(),
                format!("pad {i}").into_bytes(),
                "{layout:?} {name}"
            );
        }
        let p = parse(&bytes);
        assert_eq!((p.spine.len(), p.toc.len()), (1, 1), "{layout:?}");
        let out = route_stream(&bytes, &p.zip, &chapter_name(0));
        assert_eq!(plain(&out), "chapter 1 \u{81fa}".as_bytes(), "{layout:?}");
    }
}

#[test]
fn zip_with_257_entries_known_limit_keeps_the_first_256_and_ignores_the_last_without_error() {
    let bytes = archive(257, Layout::BookFirst, 1);
    // Ok: no error for the extra entry
    let zip = index_of(&bytes);
    assert_eq!(zip.count(), 256, "the index holds MAX_ENTRIES");
    assert!(
        zip.find(&format!("{FILLER}{:03}.txt", 255 - 5)).is_some(),
        "entry 256 (index 255) is the last kept"
    );
    assert!(
        zip.find(&format!("{FILLER}{:03}.txt", 256 - 5)).is_none(),
        "entry 257 is not indexed"
    );
}

#[test]
fn zip_with_300_entries_known_limit_keeps_the_first_256_and_ignores_the_rest_without_error() {
    let bytes = archive(300, Layout::BookFirst, 1);
    let zip = index_of(&bytes);
    assert_eq!(zip.count(), 256);
    for i in 0..256 - 5 {
        assert!(
            zip.find(&format!("{FILLER}{i:03}.txt")).is_some(),
            "filler {i}"
        );
    }
    for i in 256 - 5..300 - 5 {
        assert!(
            zip.find(&format!("{FILLER}{i:03}.txt")).is_none(),
            "filler {i} is past the limit"
        );
    }
    // a book whose parts all sit in the first 256 entries opens and reads as usual
    let p = parse(&bytes);
    assert_eq!((p.spine.len(), p.toc.len()), (1, 1));
    let out = route_stream(&bytes, &p.zip, &chapter_name(0));
    assert_eq!(plain(&out), "chapter 1 \u{81fa}".as_bytes());
}

#[test]
fn zip_with_257_entries_known_limit_chapter_past_the_limit_fails_to_open_the_book() {
    let bytes = archive(257, Layout::ChaptersLast, 1);
    let zip = index_of(&bytes);
    assert!(
        zip.find(&chapter_name(0)).is_none(),
        "the chapter is entry 257"
    );
    let err = try_parse(&bytes)
        .err()
        .expect("the spine resolves to nothing");
    assert_eq!(err, "opf: epub: spine is empty after resolution");
}

#[test]
fn zip_with_257_entries_known_limit_second_chapter_past_the_limit_is_dropped_from_the_spine_silently()
 {
    let bytes = archive(257, Layout::ChaptersLast, 2);
    let zip = index_of(&bytes);
    // chapters are entries 256 and 257 (index 255, 256): the first is kept, the second not
    assert!(zip.find(&chapter_name(0)).is_some());
    assert!(zip.find(&chapter_name(1)).is_none());
    let p = parse(&bytes);
    assert_eq!(
        p.spine.len(),
        1,
        "a two-chapter spine silently became one chapter"
    );
}

#[test]
fn zip_with_300_entries_known_limit_opf_past_the_limit_is_not_found() {
    let bytes = archive(300, Layout::PackageLast, 1);
    let zip = index_of(&bytes);
    assert_eq!(zip.count(), 256);
    assert!(zip.find("META-INF/container.xml").is_some());
    assert!(zip.find("OEBPS/content.opf").is_none());
    assert!(zip.find("OEBPS/nav.xhtml").is_none());
    assert!(zip.find(&chapter_name(0)).is_none());
    let err = try_parse(&bytes).err().expect("no OPF in the index");
    assert_eq!(err, "OEBPS/content.opf: not in index");
}

#[test]
fn zip_with_300_entries_index_is_replaced_not_extended_by_a_second_parse() {
    // `parse_central_directory` replaces what an index held: parsing 300 after 255 gives 256
    let small = archive(255, Layout::BookFirst, 1);
    let big = archive(300, Layout::BookFirst, 1);
    let cd = |b: &[u8]| {
        let (off, size) = ZipIndex::parse_eocd(b, b.len() as u32).unwrap();
        b[off as usize..(off + size) as usize].to_vec()
    };
    let mut zip = ZipIndex::new();
    zip.parse_central_directory(&cd(&small)).unwrap();
    assert_eq!(zip.count(), 255);
    zip.parse_central_directory(&cd(&big)).unwrap();
    assert_eq!(zip.count(), 256);
}

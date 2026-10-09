//! Task 5 behaviour of the real reader over the virtual card: the previous /
//! current / next chapter working set, the persistent page index, the
//! decoded-image LRU, source identity and capacity-vs-EOF indexing.
//!
//! The host build is the X4 board (single 96 KiB chapter, no image cache); the
//! HR8 policy is selected explicitly with `Rig::set_profile`, the same
//! `RingConfig` / `LruConfig` values the C61 picks for a validated 8 MiB part.
use pulp_board_logic::chapter_ring::{HR8_SINGLE_MAX_BYTES, RingConfig};
use pulp_board_logic::image_lru::LruConfig;
use pulp_board_logic::page_index::{
    BANK_BODY_INSTALLED, BANK_BODY_USED, BANK_HEADING_INSTALLED, BANK_HEADING_USED, HEADER_BYTES,
    header_banks, record_name,
};
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, Run,
    TocItem, build_epub,
};
use pulp_host::reader::{
    Action, PAGE_BUF, Phase, QA_FONT_SIZE, QA_NEXT_CHAPTER, QA_PREV_CHAPTER, Rig,
};
use pulp_host::storage::{ReadOutcome, StorageOp, VirtualStorage};

const BOOK: &str = "WORKSET.EPU";

// The image worker of the production reader is one task behind process-global
// channels: tests that run a reader take this lock for their whole duration.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn paragraph(chapter: usize, para: usize, words: usize) -> String {
    // every chapter has the same length; its letter makes the text differ
    let letter = char::from(b'a' + (chapter % 26) as u8);
    (0..words)
        .map(|w| format!("{letter}{para:02}{w:03}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn spec(chapters: usize, paras: usize, words: usize, salt: char) -> EpubSpec {
    EpubSpec {
        version: EpubVersion::V3,
        title: "Working set".into(),
        author: "Fixture".into(),
        identifier: "urn:pulp:workset".into(),
        chapters: (0..chapters)
            .map(|c| Chapter {
                title: format!("Chapter {c}"),
                blocks: (0..paras)
                    .map(|p| {
                        let mut text = paragraph(c, p, words);
                        // same length, other bytes: a replacement of the book
                        text.replace_range(0..1, &salt.to_string());
                        Block::Paragraph(vec![Run::Text(text)])
                    })
                    .collect(),
            })
            .collect(),
        toc: (0..chapters)
            .map(|c| TocItem {
                title: format!("Chapter {c}"),
                chapter: c,
                children: vec![],
            })
            .collect(),
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    }
}

fn epub(chapters: usize, paras: usize, words: usize, salt: char) -> Vec<u8> {
    build_epub(&spec(chapters, paras, words, salt)).unwrap()
}

fn card(bytes: &[u8]) -> VirtualStorage {
    let card = VirtualStorage::memory_with(&[(BOOK, bytes)]);
    card.ensure_pulp_dir().unwrap();
    card
}

// (ring, image LRU, persist page indexes): the C61 HR8 profile stores them
fn hr8() -> (RingConfig, LruConfig, bool) {
    (RingConfig::HR8, LruConfig::HR8, true)
}

// the X4 profile: single chapter, no image cache, no stored page indexes
fn small() -> (RingConfig, LruConfig, bool) {
    (RingConfig::SMALL, LruConfig::OFF, false)
}

fn open_with(card: VirtualStorage, profile: (RingConfig, LruConfig, bool)) -> Rig {
    let mut r = Rig::new(card);
    r.configure(2, 0);
    r.set_profile(profile.0, profile.1, profile.2);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    r
}

// run background ticks until the book's chapters are cached and `done` holds
fn idle_until(r: &mut Rig, what: &str, done: impl Fn(&Rig) -> bool) {
    for _ in 0..3000 {
        if done(r) {
            return;
        }
        r.idle(1);
    }
    panic!("never reached: {what}");
}

fn quiet(r: &mut Rig) {
    idle_until(r, "caching done", |r| !r.has_bg_work());
    // a few more ticks: neighbor reads start after the page settled
    r.idle(100);
}

fn cache_reads(r: &Rig) -> usize {
    let file = format!("_PULP/{}", r.cache_file());
    r.storage()
        .read_log()
        .iter()
        .filter(|rec| rec.path == file)
        .count()
}

fn page_text(r: &Rig) -> Vec<Vec<u8>> {
    r.lines()
}

// every page of the current chapter: its lines
fn chapter_pages(r: &mut Rig) -> Vec<Vec<Vec<u8>>> {
    while r.page() > 0 {
        r.press(Action::Prev);
    }
    let mut pages = vec![page_text(r)];
    let ch = r.chapter();
    loop {
        r.press(Action::Next);
        if r.chapter() != ch {
            // stepped into the next chapter: come back to its first page
            r.quick_trigger(QA_PREV_CHAPTER);
            break;
        }
        if r.page() == pages.len() {
            pages.push(page_text(r));
        } else {
            break;
        }
    }
    pages
}

// --- warm navigation -----------------------------------------------------

#[test]
fn a_warmed_next_chapter_opens_without_reading_the_card() {
    let _serial = serial();
    let bytes = epub(6, 3, 60, 'x');
    let mut r = open_with(card(&bytes), hr8());
    quiet(&mut r);
    assert_eq!(r.chapter(), 0);
    assert_eq!(r.ring_resident(), [0, 1], "current and next are resident");

    let before = cache_reads(&r);
    r.quick_trigger(QA_NEXT_CHAPTER);
    assert_eq!((r.chapter(), r.phase()), (1, Phase::Ready));
    assert_eq!(
        cache_reads(&r),
        before,
        "chapter 1 came from the ring: no read of the text cache"
    );
    quiet(&mut r);
    assert_eq!(r.ring_resident(), [0, 1, 2], "previous, current, next");

    // and the way back is warm as well
    let before = cache_reads(&r);
    r.quick_trigger(QA_PREV_CHAPTER);
    assert_eq!(r.chapter(), 0);
    assert_eq!(
        cache_reads(&r),
        before,
        "the previous chapter stayed resident"
    );
}

#[test]
fn the_single_chapter_profile_reads_the_card_on_every_chapter_change() {
    let _serial = serial();
    let bytes = epub(6, 3, 60, 'x');
    let mut r = open_with(card(&bytes), small());
    quiet(&mut r);
    assert_eq!(r.ring_resident(), [0], "neighbors are off");
    let before = cache_reads(&r);
    r.quick_trigger(QA_NEXT_CHAPTER);
    assert!(cache_reads(&r) > before, "chapter 1 is read from the card");
    assert_eq!(r.ring_resident(), [1], "the old chapter left the window");
}

#[test]
fn warm_navigation_shows_the_same_pages_as_streaming() {
    let _serial = serial();
    let bytes = epub(5, 4, 70, 'x');
    let mut warm = open_with(card(&bytes), hr8());
    let mut cold = open_with(card(&bytes), small());
    for ch in 0..5u16 {
        quiet(&mut warm);
        quiet(&mut cold);
        assert_eq!(warm.chapter(), ch);
        assert_eq!(cold.chapter(), ch);
        assert_eq!(
            chapter_pages(&mut warm),
            chapter_pages(&mut cold),
            "chapter {ch}: pages"
        );
        assert_eq!(
            warm.page_offsets(),
            cold.page_offsets(),
            "chapter {ch}: offsets"
        );
        assert_eq!(warm.fully_indexed(), cold.fully_indexed());
        if ch < 4 {
            warm.quick_trigger(QA_NEXT_CHAPTER);
            cold.quick_trigger(QA_NEXT_CHAPTER);
        }
    }
}

#[test]
fn a_jump_drops_what_is_no_longer_adjacent() {
    let _serial = serial();
    let bytes = epub(9, 2, 60, 'x');
    let mut r = open_with(card(&bytes), hr8());
    quiet(&mut r);
    for _ in 0..6 {
        r.quick_trigger(QA_NEXT_CHAPTER);
    }
    assert_eq!(r.chapter(), 6);
    let resident = r.ring_resident();
    assert!(resident.contains(&6));
    assert!(
        resident.iter().all(|&c| (5..=7).contains(&c)),
        "only the window around 6 is kept: {resident:?}"
    );
    quiet(&mut r);
    assert_eq!(r.ring_resident(), [5, 6, 7]);
}

#[test]
fn a_ring_that_holds_two_chapters_evicts_a_neighbor_for_the_current_one() {
    let _serial = serial();
    let bytes = epub(6, 3, 60, 'x');
    // find the chapter size on a probing run, then budget for two of them
    let size = {
        let mut r = open_with(card(&bytes), hr8());
        quiet(&mut r);
        r.ring_used() / r.ring_resident().len()
    };
    assert!(size > 100);
    let cfg = RingConfig {
        budget: 2 * size + size / 2,
        single_max: size + 64,
        neighbors: true,
    };
    let mut r = open_with(card(&bytes), (cfg, LruConfig::OFF, true));
    quiet(&mut r);
    r.quick_trigger(QA_NEXT_CHAPTER);
    quiet(&mut r);
    assert_eq!(r.chapter(), 1);
    assert!(
        r.ring_used() <= cfg.budget,
        "{} > {}",
        r.ring_used(),
        cfg.budget
    );
    let resident = r.ring_resident();
    assert!(
        resident.contains(&1),
        "the current chapter is always kept: {resident:?}"
    );
    assert_eq!(
        resident.len(),
        2,
        "two of the three window chapters fit: {resident:?}"
    );
}

#[test]
fn chapters_over_the_single_bound_stream_and_look_the_same() {
    let _serial = serial();
    let bytes = epub(4, 4, 80, 'x');
    let size = {
        let mut r = open_with(card(&bytes), hr8());
        quiet(&mut r);
        r.ring_used() / r.ring_resident().len()
    };
    let refuse = RingConfig {
        budget: 3 * size,
        single_max: size / 2,
        neighbors: true,
    };
    let mut streamed = open_with(card(&bytes), (refuse, LruConfig::OFF, true));
    let mut resident = open_with(card(&bytes), hr8());
    for ch in 0..4u16 {
        quiet(&mut streamed);
        quiet(&mut resident);
        assert_eq!(
            streamed.ring_resident(),
            [] as [u16; 0],
            "cap refusal: nothing is kept"
        );
        assert_eq!(streamed.ring_used(), 0);
        assert_eq!(
            chapter_pages(&mut streamed),
            chapter_pages(&mut resident),
            "chapter {ch}"
        );
        assert_eq!(streamed.page_offsets(), resident.page_offsets());
        if ch < 3 {
            streamed.quick_trigger(QA_NEXT_CHAPTER);
            resident.quick_trigger(QA_NEXT_CHAPTER);
        }
    }
}

#[test]
fn a_neighbor_whose_read_fails_is_never_published() {
    let _serial = serial();
    let bytes = epub(5, 3, 60, 'x');
    let mut r = open_with(card(&bytes), hr8());
    // the chapters must be cached before neighbor reads begin; fail the first
    // read of the cache file after that
    // every read of the text cache fails from now on; the neighbor job is the
    // reader of that file while chapter 0 is on screen
    assert_eq!(r.ring_resident(), [0]);
    let file = format!("_PULP/{}", r.cache_file());
    for nth in 1..=64 {
        r.storage().inject_error(
            StorageOp::Read,
            &file,
            nth,
            pulp_host::ErrorKind::ReadFailed,
        );
    }
    r.idle(100);
    assert!(
        r.storage()
            .read_log()
            .iter()
            .any(|rec| rec.path == file && matches!(rec.outcome, ReadOutcome::ErrorInjected(_))),
        "the neighbor job did read, and failed"
    );
    assert!(!r.warm_pending(), "the failed job was dropped");
    assert!(
        !r.ring_resident().contains(&1),
        "nothing was published for chapter 1"
    );
    assert_eq!(r.phase(), Phase::Ready);
    // navigation still works: the chapter is read the ordinary way
    r.storage().clear_injections();
    r.quick_trigger(QA_NEXT_CHAPTER);
    assert_eq!((r.chapter(), r.phase()), (1, Phase::Ready));
    assert!(r.ring_resident().contains(&1));
}

#[test]
fn a_neighbor_job_in_flight_when_the_reader_moves_on_cannot_publish() {
    let _serial = serial();
    // chapters of about 45 KB: three bounded steps each
    let bytes = epub(4, 8, 800, 'x');
    let mut r = open_with(card(&bytes), hr8());
    idle_until(&mut r, "a neighbor job is part way", |r| {
        r.warm_pending() && r.ring_resident() == [0]
    });
    // one more tick reads the first step of chapter 1; it is not done yet
    r.idle(1);
    assert!(r.warm_pending(), "chapter 1 is only part read");
    assert_eq!(
        r.ring_resident(),
        [0],
        "nothing is published before the last byte"
    );
    // the reader moves to that very chapter: its text is loaded for the page,
    // the half-read job must not publish a second, stale copy
    r.quick_trigger(QA_NEXT_CHAPTER);
    assert_ne!(
        r.warm_chapter(),
        Some(1),
        "no job left for the chapter now shown"
    );
    let used = r.ring_used();
    r.idle(3);
    assert_eq!(r.chapter(), 1);
    assert!(r.ring_resident().contains(&1));
    // the same chapter is in the ring once: the budget counts it once
    let mut cold = open_with(card(&bytes), small());
    cold.quick_trigger(QA_NEXT_CHAPTER);
    assert_eq!(page_text(&r), page_text(&cold));
    assert!(used > 0);
}

// --- source identity -------------------------------------------------------

#[test]
fn a_replaced_book_with_the_same_name_and_size_is_not_served_from_the_old_cache() {
    let _serial = serial();
    let a = epub(3, 3, 60, 'a');
    let b = epub(3, 3, 60, 'b');
    assert_eq!(a.len(), b.len(), "same name, same archive size");
    assert_ne!(a, b);

    let mut r = open_with(card(&a), hr8());
    quiet(&mut r);
    let source_a = r.source_id();
    let dir_a = r.cache_dir();
    let lines_a = r.lines();
    r.exit();
    let storage = r.into_storage();

    // the user replaces the file (same name, same size) and opens it again
    storage.write_file(BOOK, &b).unwrap();
    let mut r = open_with(storage, hr8());
    quiet(&mut r);
    assert_ne!(r.source_id(), source_a, "the identity follows the content");
    assert_ne!(
        r.cache_dir(),
        dir_a,
        "images and page indexes are not shared"
    );
    assert_ne!(r.lines(), lines_a, "the new book's text, not the old cache");
    assert!(
        r.lines().iter().any(|l| l.starts_with(b"b")),
        "{:?}",
        r.lines()
            .iter()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_same_book_keeps_its_identity_and_cache_across_opens() {
    let _serial = serial();
    let bytes = epub(3, 3, 60, 'a');
    let mut r = open_with(card(&bytes), small());
    quiet(&mut r);
    let id = r.source_id();
    let dir = r.cache_dir();
    r.exit();
    let storage = r.into_storage();
    storage.reset_reads();
    let mut r = open_with(storage, small());
    assert_eq!((r.source_id(), r.cache_dir()), (id, dir));
    // a valid text cache is reused: the chapters are not decompressed again
    let zip_reads = r
        .storage()
        .read_log()
        .iter()
        .filter(|rec| rec.path == BOOK && rec.offset > 4096)
        .count();
    let _ = &mut r;
    assert!(zip_reads < 20, "{zip_reads}");
}

// --- persistent page index ---------------------------------------------------

fn record_path(r: &Rig, chapter: u16) -> (String, String) {
    let name = record_name(chapter);
    (r.cache_dir(), String::from_utf8(name.to_vec()).unwrap())
}

fn read_record(storage: &VirtualStorage, dir: &str, name: &str) -> Vec<u8> {
    let size = storage.file_size_in_pulp_subdir(dir, name).unwrap() as usize;
    let mut buf = vec![0u8; size];
    let n = storage
        .read_chunk_in_pulp_subdir(dir, name, 0, &mut buf)
        .unwrap();
    assert_eq!(n, size);
    buf
}

// open `bytes` on `storage`, return what the first chapter looks like
struct Opened {
    offsets: Vec<u32>,
    pages: Vec<Vec<Vec<u8>>>,
    counts: (u16, u16, u16),
    dir: String,
    storage: VirtualStorage,
}

fn open_chapter0(storage: VirtualStorage, font: u8) -> Opened {
    let mut r = Rig::new(storage);
    r.configure(font, 0);
    r.set_profile(RingConfig::HR8, LruConfig::OFF, true);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    quiet(&mut r);
    // what opening the chapter did, before any paging touches the next one
    let counts = r.index_counts();
    let offsets = r.page_offsets();
    let pages = chapter_pages(&mut r);
    let opened = Opened {
        offsets,
        pages,
        counts,
        dir: r.cache_dir(),
        storage: VirtualStorage::memory(),
    };
    r.exit();
    Opened {
        storage: r.into_storage(),
        ..opened
    }
}

#[test]
fn a_stored_page_index_is_reused_and_matches_regeneration() {
    let _serial = serial();
    let bytes = epub(2, 5, 90, 'x');
    let first = open_chapter0(card(&bytes), 2);
    assert!(first.offsets.len() > 2, "a multi-page chapter");
    assert_eq!(first.counts, (0, 1, 0), "laid out once and stored");
    let name = String::from_utf8(record_name(0).to_vec()).unwrap();
    assert!(
        first
            .storage
            .optional_file_size_in_pulp_subdir(&first.dir, &name)
            .unwrap()
            .is_some()
    );

    let second = open_chapter0(first.storage, 2);
    assert_eq!(second.counts, (1, 0, 0), "read back, not laid out again");
    assert_eq!(second.offsets, first.offsets);
    assert_eq!(second.pages, first.pages, "every page is identical");

    // regression parity: a card with no stored records gives the same result
    let fresh = open_chapter0(card(&bytes), 2);
    assert_eq!(fresh.offsets, second.offsets);
    assert_eq!(fresh.pages, second.pages);
}

#[test]
fn another_font_size_does_not_use_the_record_and_replaces_it() {
    let _serial = serial();
    let bytes = epub(2, 5, 90, 'x');
    let medium = open_chapter0(card(&bytes), 2);
    let large = open_chapter0(medium.storage, 3);
    assert_eq!(
        large.counts,
        (0, 1, 1),
        "stale record refused, new one stored"
    );
    assert_ne!(large.offsets, medium.offsets);
    let expected = open_chapter0(card(&bytes), 3);
    assert_eq!(large.offsets, expected.offsets, "regenerated, not guessed");
    // the record now belongs to the large size
    let again = open_chapter0(large.storage, 3);
    assert_eq!(again.counts, (1, 0, 0));
    assert_eq!(again.offsets, expected.offsets);
}

#[test]
fn corrupt_truncated_and_foreign_records_fall_back_to_regeneration() {
    let _serial = serial();
    let bytes = epub(2, 5, 90, 'x');
    let good = open_chapter0(card(&bytes), 2);
    let name = String::from_utf8(record_name(0).to_vec()).unwrap();
    let original = read_record(&good.storage, &good.dir, &name);
    assert!(original.len() > HEADER_BYTES + 12);

    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    for at in [
        0,
        9,
        HEADER_BYTES - 1,
        HEADER_BYTES,
        HEADER_BYTES + 7,
        original.len() - 1,
    ] {
        let mut v = original.clone();
        v[at] ^= 0x40;
        cases.push(("bit flip", v));
    }
    cases.push((
        "cut in the payload",
        original[..original.len() - 20].to_vec(),
    ));
    cases.push(("footer missing", original[..original.len() - 16].to_vec()));
    cases.push(("header only", original[..HEADER_BYTES].to_vec()));
    cases.push(("shorter than a header", original[..10].to_vec()));
    cases.push(("empty", Vec::new()));
    let mut longer = original.clone();
    longer.extend_from_slice(&[0; 7]);
    cases.push(("trailing bytes", longer));
    cases.push(("garbage", vec![0xA5; original.len()]));

    let mut storage = good.storage;
    for (what, damaged) in cases {
        storage
            .write_in_pulp_subdir(&good.dir, &name, &damaged)
            .unwrap();
        let again = open_chapter0(storage, 2);
        assert_eq!(again.offsets, good.offsets, "{what}: regenerated offsets");
        assert_eq!(again.pages, good.pages, "{what}: regenerated pages");
        assert_eq!(again.counts.0, 0, "{what}: nothing was loaded");
        assert_eq!(again.counts.1, 1, "{what}: a valid record was stored again");
        storage = again.storage;
        assert_eq!(
            read_record(&storage, &good.dir, &name),
            original,
            "{what}: repaired"
        );
    }
    // and the repaired record is used
    assert_eq!(open_chapter0(storage, 2).counts, (1, 0, 0));
}

#[test]
fn a_replaced_book_never_finds_the_old_books_page_index() {
    let _serial = serial();
    let a = epub(2, 5, 90, 'a');
    let b = epub(2, 5, 90, 'b');
    assert_eq!(a.len(), b.len());
    let first = open_chapter0(card(&a), 2);
    let storage = first.storage;
    storage.write_file(BOOK, &b).unwrap();
    let second = open_chapter0(storage, 2);
    assert_eq!(second.counts.0, 0, "nothing of book A is loaded for book B");
    assert_ne!(second.dir, first.dir);
}

// --- capacity versus end of text --------------------------------------------

#[test]
fn a_full_page_table_is_a_truncation_and_stores_no_index() {
    let _serial = serial();
    // one chapter of about 700 KB: more than 512 pages at any font size
    let bytes = epub(1, 160, 4000, 'x');
    let mut r = Rig::new(card(&bytes));
    r.configure(2, 0);
    r.set_profile(RingConfig::HR8, LruConfig::OFF, true);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    // the chapter is over the single bound: it streams, so the table fills page
    // by page as the reader pages forward
    assert!(r.ring_resident().is_empty());
    for _ in 0..600 {
        r.press(Action::Next);
    }
    assert_eq!(r.total_pages(), 512, "the small profile's table");
    assert!(
        r.truncated(),
        "more text follows the last page of the table"
    );
    assert!(!r.fully_indexed(), "capacity truncation is not completion");
    let (dir, name) = record_path(&r, 0);
    assert_eq!(
        r.storage()
            .optional_file_size_in_pulp_subdir(&dir, &name)
            .unwrap(),
        None,
        "a truncated index is never stored"
    );
    assert_eq!(r.index_counts().1, 0);
}

// the stored records of every chapter of the fixtures, whatever their name
fn stored_records(r: &Rig, chapters: u16) -> Vec<u16> {
    (0..chapters)
        .filter(|&c| {
            let (dir, name) = record_path(r, c);
            r.storage()
                .optional_file_size_in_pulp_subdir(&dir, &name)
                .unwrap()
                .is_some()
        })
        .collect()
}

#[test]
fn the_small_profile_never_creates_or_reads_a_page_index() {
    let _serial = serial();
    let bytes = epub(3, 5, 90, 'x');

    // a whole book read on the X4 profile leaves no PG*.IDX behind
    let mut r = open_with(card(&bytes), small());
    quiet(&mut r);
    let pages = chapter_pages(&mut r);
    assert!(pages.len() > 2, "multi-page chapters");
    for _ in 0..40 {
        r.press(Action::Next);
    }
    quiet(&mut r);
    assert_eq!(stored_records(&r, 3), Vec::<u16>::new());
    assert_eq!(r.index_counts(), (0, 0, 0));
    r.exit();
    // and a second open does not look for one either
    let mut r = open_with(r.into_storage(), small());
    quiet(&mut r);
    assert_eq!(r.index_counts(), (0, 0, 0));
    assert_eq!(stored_records(&r, 3), Vec::<u16>::new());

    // a record that exists (written by an HR8 open) is neither read nor
    // rewritten nor removed on the small profile
    let first = open_chapter0(card(&bytes), 2);
    assert_eq!(first.counts, (0, 1, 0));
    let name = String::from_utf8(record_name(0).to_vec()).unwrap();
    let original = read_record(&first.storage, &first.dir, &name);
    first.storage.reset_reads();
    let mut r = open_with(first.storage, small());
    quiet(&mut r);
    assert_eq!(r.index_counts(), (0, 0, 0), "not loaded, not rejected");
    assert!(
        !r.storage()
            .read_log()
            .iter()
            .any(|rec| rec.path.ends_with(".IDX")),
        "the record is not even opened"
    );
    let (dir, name) = record_path(&r, 0);
    assert_eq!(read_record(r.storage(), &dir, &name), original);
}

#[test]
fn the_small_profile_shows_a_full_table_as_complete_like_before() {
    let _serial = serial();
    let bytes = epub(1, 160, 4000, 'x');
    let reach_cap = |r: &mut Rig| {
        for _ in 0..600 {
            r.press(Action::Next);
        }
        assert_eq!(r.total_pages(), 512);
        // internal semantics are the same on every profile
        assert!(r.truncated());
        assert!(!r.fully_indexed());
    };

    // X4 profile: the counters and the progress figure behave as they did when
    // a full table was reported as the end of the index
    let mut r = open_with(card(&bytes), small());
    reach_cap(&mut r);
    assert!(r.index_complete(), "status line shows N/512");
    assert_eq!(r.page(), 511, "last page of the table");
    assert_eq!(r.progress_pct(), 100, "end of the table is end of progress");
    r.press(Action::Prev);
    assert!(r.index_complete());
    assert!(r.progress_pct() < 100);

    // a profile with the large table keeps the distinction: the figure is not
    // 100% while text remains behind the table
    let mut r = open_with(card(&bytes), (RingConfig::HR8, LruConfig::OFF, true));
    reach_cap(&mut r);
    assert!(!r.index_complete(), "status line shows pN");
    assert!(r.progress_pct() < 100);
}

#[test]
fn a_chapter_read_to_its_end_is_complete_and_stored() {
    let _serial = serial();
    // over the single bound but well inside the table
    let single = HR8_SINGLE_MAX_BYTES;
    let bytes = epub(1, 25, 1200, 'x');
    let mut r = Rig::new(card(&bytes));
    r.configure(2, 0);
    r.set_profile(
        RingConfig {
            budget: single,
            single_max: 2048,
            neighbors: true,
        },
        LruConfig::OFF,
        true,
    );
    r.open(BOOK);
    assert!(r.ring_resident().is_empty(), "streams from the card");
    for _ in 0..2000 {
        if r.fully_indexed() {
            break;
        }
        r.press(Action::Next);
    }
    assert!(r.fully_indexed());
    assert!(!r.truncated());
    assert_eq!(
        r.index_counts().1,
        1,
        "the index of a streamed chapter is stored at its end"
    );
    // next open loads it
    let storage = {
        r.exit();
        r.into_storage()
    };
    let mut r = Rig::new(storage);
    r.configure(2, 0);
    r.set_profile(
        RingConfig {
            budget: single,
            single_max: 2048,
            neighbors: true,
        },
        LruConfig::OFF,
        true,
    );
    r.open(BOOK);
    assert_eq!(r.index_counts().0, 1);
    assert!(
        r.fully_indexed(),
        "the whole table is there before any paging"
    );
}

// --- layout identity ---------------------------------------------------------

// A one-glyph PFNT v1 fixture with explicit identity and horizontal metrics.
fn fallback_pack(px: u16, id: u64, advance: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PFNT");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&px.to_le_bytes());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&(px + 4).to_le_bytes());
    bytes.extend_from_slice(&px.to_le_bytes());
    for n in [1u32, 44, 22, 66, 3, 69] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for n in [u32::from('一'), 0, 3] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for n in [advance, 0, (-3i16) as u16, 8, 3] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x81, 0x81, 0x81]);
    bytes
}

fn install_fallback(card: &VirtualStorage, px: u16, id: u64, advance: u16) {
    card.ensure_pulp_dir().unwrap();
    card.ensure_pulp_subdir("FONTS").unwrap();
    card.write_in_pulp_subdir(
        "FONTS",
        &format!("F{px:05}.PFN"),
        &fallback_pack(px, id, advance),
    )
    .unwrap();
    pulp_board_logic::font_index::FONT_SOURCE.bump();
}

#[test]
fn a_loaded_index_retains_later_fallback_banks_across_suspend() {
    let _serial = serial();
    for heading in [false, true] {
        let px = if heading { 23 } else { 16 };
        let bank = if heading {
            BANK_HEADING_USED | BANK_HEADING_INSTALLED
        } else {
            BANK_BODY_USED | BANK_BODY_INSTALLED
        };
        let latin = "alpha beta gamma delta epsilon. ".repeat(450);
        let latin_len = latin.len();
        let cjk = "一".repeat(2000);
        let mut book = spec(1, 0, 0, 'x');
        book.chapters[0].blocks = vec![
            Block::Paragraph(vec![Run::Text(latin)]),
            if heading {
                Block::Heading(cjk)
            } else {
                Block::Paragraph(vec![Run::Text(cjk)])
            },
        ];
        let bytes = build_epub(&book).unwrap();
        let storage = card(&bytes);
        install_fallback(&storage, px, 111, 17);
        let open = |storage| {
            let mut r = Rig::new(storage);
            r.configure(0, 0);
            r.set_profile(RingConfig::HR8, LruConfig::OFF, true);
            r.open(BOOK);
            assert_eq!(r.phase(), Phase::Ready);
            r
        };
        let mut first = open(storage);
        assert_eq!(first.index_counts(), (0, 1, 0));
        let original = first.page_offsets();
        first.exit();

        let mut loaded = open(first.into_storage());
        assert_eq!(loaded.index_counts(), (1, 0, 0));
        assert_eq!(loaded.page_offsets(), original);
        assert!(
            original[1] as usize + PAGE_BUF < latin_len,
            "the visible and next-page read windows are entirely Latin"
        );
        assert!(
            loaded
                .lines()
                .iter()
                .all(|line| !line.windows(3).any(|s| s == "一".as_bytes())),
            "the visible Latin prefix must not discover the later bank"
        );
        loaded.suspend();
        install_fallback(loaded.storage(), px, 222, 33);
        loaded.resume();
        assert_eq!(loaded.phase(), Phase::Ready);
        let resumed = loaded.page_offsets();
        assert_ne!(
            resumed, original,
            "{px}px replacement invalidates loaded offsets"
        );
        assert_eq!(
            loaded.index_counts(),
            (1, 1, 1),
            "the stale record is rejected and rebuilt"
        );
        let (dir, name) = record_path(&loaded, 0);
        let stored = read_record(loaded.storage(), &dir, &name);
        assert_eq!(
            header_banks(&stored[..HEADER_BYTES]).unwrap() & bank,
            bank,
            "the replacement record retains this chapter's bank participation"
        );

        // A clean card provides the layout oracle, without reusing a record.
        let fresh_storage = card(&bytes);
        install_fallback(&fresh_storage, px, 222, 33);
        let fresh = open(fresh_storage);
        assert_eq!(
            resumed,
            fresh.page_offsets(),
            "{px}px resume agrees with fresh layout"
        );
        assert_eq!(loaded.lines(), fresh.lines());
    }
}

#[test]
fn a_geometry_change_while_suspended_invalidates_the_layout() {
    let _serial = serial();
    let bytes = epub(2, 5, 90, 'x');
    let mut r = Rig::new(card(&bytes));
    r.configure(2, 0);
    r.open(BOOK);
    quiet(&mut r);
    let before = r.page_offsets();
    let width = r.text_w();
    r.suspend();
    // the settings app switched to a theme with wider margins
    r.configure(2, 3);
    r.resume();
    assert_eq!(r.phase(), Phase::Ready);
    assert_ne!(r.text_w(), width, "the theme moved the text column");
    let relaid = r.page_offsets();
    let mut fresh = Rig::new(card(&bytes));
    fresh.configure(2, 3);
    fresh.open(BOOK);
    quiet(&mut fresh);
    assert_eq!(
        relaid,
        fresh.page_offsets(),
        "pages follow the new geometry"
    );
    assert_ne!(relaid, before);
}

#[test]
fn font_size_cycle_keeps_the_resident_chapter_and_replaces_the_stored_index() {
    let _serial = serial();
    let bytes = epub(3, 5, 90, 'x');
    let mut r = open_with(card(&bytes), hr8());
    quiet(&mut r);
    let resident = r.ring_resident();
    let before = cache_reads(&r);
    r.quick_cycle(QA_FONT_SIZE, 3);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.ring_resident(), resident);
    assert_eq!(
        cache_reads(&r),
        before,
        "re-layout works from the resident text"
    );
}

// --- decoded images -----------------------------------------------------------

fn image_book() -> Vec<u8> {
    let para = |s: usize| {
        Block::Paragraph(vec![Run::Text(
            (0..6)
                .map(|i| paragraph(s, i, 12))
                .collect::<Vec<_>>()
                .join(" "),
        )])
    };
    build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Plates".into(),
        author: "Fixture".into(),
        identifier: "urn:pulp:plates".into(),
        chapters: vec![Chapter {
            title: "Plate".into(),
            blocks: vec![
                Block::Heading("Study".into()),
                Block::Image(0),
                para(0),
                para(1),
                para(2),
                para(3),
                para(4),
                para(5),
                para(6),
            ],
        }],
        toc: vec![],
        images: vec![ImageSpec {
            path: "images/plate.png".into(),
            kind: ImageKind::PngGray8,
            width: 64,
            height: 48,
            pattern: Pattern::Black,
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap()
}

fn image_reads(r: &Rig) -> usize {
    let dir = format!("_PULP/{}/", r.cache_dir());
    r.storage()
        .read_log()
        .iter()
        .filter(|rec| rec.path.starts_with(&dir) && rec.path.ends_with(".BIN"))
        .count()
}

fn goto_image_page(r: &mut Rig) {
    for _ in 0..40 {
        if r.page_image().is_some() {
            return;
        }
        r.press(Action::Next);
        r.press(Action::Prev);
        if r.page_image().is_some() {
            return;
        }
        r.press(Action::Next);
    }
    panic!("no image page");
}

#[test]
fn a_revisited_image_page_is_served_from_the_lru() {
    let _serial = serial();
    let bytes = image_book();
    let mut r = open_with(card(&bytes), hr8());
    quiet(&mut r);
    r.press(Action::Prev);
    assert_eq!(r.page(), 0);
    goto_image_page(&mut r);
    let shown = r.page_image().unwrap();
    assert!(
        r.lru_len() >= 1 && r.lru_used() > 0,
        "the image was remembered"
    );
    let image_page = r.page();

    // leave the page and come back: no read of any image file of the book
    r.press(Action::Next);
    r.press(Action::Next);
    let before = image_reads(&r);
    while r.page() > image_page {
        r.press(Action::Prev);
    }
    assert_eq!(r.page(), image_page);
    assert_eq!(r.page_image(), Some(shown));
    assert_eq!(image_reads(&r), before, "served from the LRU, not the card");
}

#[test]
fn without_a_cache_profile_the_revisit_reads_the_card() {
    let _serial = serial();
    let bytes = image_book();
    let mut r = open_with(card(&bytes), small());
    quiet(&mut r);
    goto_image_page(&mut r);
    assert_eq!(r.lru_len(), 0);
    let image_page = r.page();
    r.press(Action::Next);
    let before = image_reads(&r);
    r.press(Action::Prev);
    assert_eq!(r.page(), image_page);
    assert!(image_reads(&r) > before);
}

#[test]
fn an_image_over_the_lru_item_bound_is_not_kept_and_the_page_still_shows_it() {
    let _serial = serial();
    let bytes = image_book();
    let tiny = LruConfig {
        budget: 4096,
        item_max: 64,
    };
    let mut r = open_with(card(&bytes), (RingConfig::SMALL, tiny, false));
    quiet(&mut r);
    goto_image_page(&mut r);
    assert!(r.page_image().is_some());
    assert_eq!(
        (r.lru_len(), r.lru_used()),
        (0, 0),
        "refused, not truncated"
    );
    let image_page = r.page();
    r.press(Action::Next);
    let before = image_reads(&r);
    r.press(Action::Prev);
    assert_eq!(r.page(), image_page);
    assert!(r.page_image().is_some());
    assert!(
        image_reads(&r) > before,
        "the revisit falls back to the card cache"
    );
}

#[test]
fn a_replaced_book_does_not_see_the_old_books_decoded_image() {
    let _serial = serial();
    let a = image_book();
    let mut r = open_with(card(&a), hr8());
    quiet(&mut r);
    goto_image_page(&mut r);
    assert!(r.lru_len() >= 1);
    r.exit();
    let storage = r.into_storage();
    // another book of the same name: the LRU belongs to the open book only
    let b = epub(2, 3, 60, 'b');
    storage.write_file(BOOK, &b).unwrap();
    let r = open_with(storage, hr8());
    assert_eq!(r.lru_len(), 0);
    assert_eq!(r.lru_used(), 0);
}

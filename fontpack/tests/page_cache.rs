mod common;
mod page_cache_support;

use core::mem::{size_of, size_of_val};

use common::{GOLDEN, GOLDEN_INFO, golden_glyphs, put_u16};
use page_cache_support::{Control, SourceError, open};
use pulp_fontpack::{
    CacheStorageError, FontError, Metrics, PackError, PageCache, PageGlyphSlot, PreparationError,
};

type BorrowedCache<'a> = PageCache<&'a mut [PageGlyphSlot], &'a mut [u8]>;

fn budget(slots: &[PageGlyphSlot], bitmap: &[u8]) -> usize {
    size_of::<BorrowedCache<'_>>() + size_of_val(slots) + bitmap.len()
}

fn assert_empty(cache: &BorrowedCache<'_>) {
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.info(), None);
    for c in ['A', '\u{20BB7}', 'B', char::MAX] {
        assert!(
            cache.get(c).is_none(),
            "{c:?} must not expose an old or partial page"
        );
    }
}

fn bitmap_reads(control: &Control) -> Vec<(u64, usize)> {
    control
        .reads
        .borrow()
        .iter()
        .copied()
        .filter(|&(offset, _)| offset >= 132)
        .collect()
}

#[test]
fn storage_budget_includes_reserved_metadata_bitmap_and_cache_object() {
    let mut slots = [PageGlyphSlot::default(); 3];
    let mut bitmap = [0u8; 6];
    let needed = budget(&slots, &bitmap);
    assert!(needed > bitmap.len() + size_of::<BorrowedCache<'_>>());
    match PageCache::new(&mut slots[..], &mut bitmap[..], needed - 1) {
        Err(error) => assert_eq!(
            error,
            CacheStorageError::BudgetExceeded {
                needed,
                budget: needed - 1
            }
        ),
        Ok(_) => panic!("a cache larger than its budget was accepted"),
    }
    let cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    assert_eq!(cache.storage_bytes(), needed);
    assert_empty(&cache);
}

#[test]
fn preparation_loads_distinct_page_glyphs_and_retrieval_needs_no_source() {
    let mut slots = [PageGlyphSlot::default(); 3];
    let mut bitmap = [0u8; 6];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    cache
        .prepare(&mut reader, &['A', '\u{20BB7}', char::MAX])
        .unwrap();
    let unique_reads = control.reads.borrow()[1..].to_vec();
    let before_duplicates = control.reads.borrow().len();
    cache
        .prepare(
            &mut reader,
            &['A', '\u{20BB7}', 'A', char::MAX, '\u{20BB7}'],
        )
        .unwrap();
    assert_eq!(
        &control.reads.borrow()[before_duplicates..],
        unique_reads,
        "duplicate codepoints add no I/O"
    );
    assert_eq!(cache.len(), 3);
    assert_eq!(cache.info(), Some(GOLDEN_INFO));
    assert_eq!(
        bitmap_reads(&control),
        [(132, 2), (135, 4), (132, 2), (135, 4)]
    );
    let before = control.reads.borrow().len();
    control.disabled.set(true);
    drop(reader);
    for _strip in 0..4 {
        for (c, metrics, bytes) in golden_glyphs()
            .into_iter()
            .filter(|(c, _, _)| *c != '\u{FFFF}')
        {
            let glyph = cache.get(c).expect("prepared glyph");
            assert_eq!(glyph.metrics, metrics);
            assert_eq!(glyph.bitmap, bytes);
        }
        assert!(cache.get('B').is_none());
        assert!(cache.get('\u{FFFF}').is_none());
    }
    assert_eq!(control.reads.borrow().len(), before);
    assert_eq!(cache.storage_bytes(), needed);
}

#[test]
fn missing_glyph_is_prepared_as_the_size_specific_hollow_square() {
    let mut slots = [PageGlyphSlot::default(); 1];
    let mut bitmap = [0u8; 24];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    cache.prepare(&mut reader, &['B', 'B']).unwrap();
    assert_eq!(cache.len(), 1);
    let glyph = cache.get('B').unwrap();
    assert_eq!(
        glyph.metrics,
        Metrics {
            advance: 16,
            offset_x: 2,
            offset_y: -12,
            width: 12,
            height: 12
        }
    );
    assert_eq!(
        glyph.bitmap,
        [
            0xFF, 0xF0, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10,
            0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0xFF, 0xF0,
        ]
    );
    assert!(
        bitmap_reads(&control).is_empty(),
        "synthetic glyphs must not read bitmap data"
    );
    assert!(
        control.reads.borrow().len() <= 4,
        "one header plus at most one three-probe lookup"
    );
}

#[test]
fn metadata_capacity_failure_hides_old_and_partial_pages_and_allows_retry() {
    let mut slots = [PageGlyphSlot::default(); 1];
    let mut bitmap = [0u8; 6];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    cache.prepare(&mut reader, &['A']).unwrap();
    let first_reads = control.reads.borrow()[1..].to_vec();
    let before = control.reads.borrow().len();
    assert_eq!(
        cache.prepare(&mut reader, &['A', '\u{20BB7}']),
        Err(PreparationError::MetadataFull {
            needed: 2,
            capacity: 1
        })
    );
    assert_empty(&cache);
    assert_eq!(
        &control.reads.borrow()[before..],
        first_reads,
        "read A only, then reject the second slot"
    );
    cache.prepare(&mut reader, &['\u{20BB7}']).unwrap();
    assert_eq!(
        cache.get('\u{20BB7}').unwrap().bitmap,
        [0xFF, 0x80, 0, 0x80]
    );
    assert_eq!(cache.storage_bytes(), needed);
}

#[test]
fn bitmap_capacity_failure_does_not_read_overflowing_glyph_and_allows_retry() {
    let mut slots = [PageGlyphSlot::default(); 2];
    let mut bitmap = [0u8; 5];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    cache.prepare(&mut reader, &[char::MAX]).unwrap();
    assert_eq!(
        cache.prepare(&mut reader, &['A', '\u{20BB7}']),
        Err(PreparationError::BitmapFull {
            needed: 6,
            capacity: 5
        })
    );
    assert_empty(&cache);
    assert_eq!(bitmap_reads(&control), [(132, 2)]);
    cache.prepare(&mut reader, &['\u{20BB7}']).unwrap();
    assert_eq!(
        cache.get('\u{20BB7}').unwrap().bitmap,
        [0xFF, 0x80, 0, 0x80]
    );
    assert_eq!(cache.storage_bytes(), needed);
}

#[test]
fn lookup_and_bitmap_io_failures_hide_pages_and_are_recoverable() {
    // The supplementary glyph's index is at 88 and its bitmap starts at 135.
    for failing_offset in [88, 135] {
        let mut slots = [PageGlyphSlot::default(); 2];
        let mut bitmap = [0u8; 6];
        let needed = budget(&slots, &bitmap);
        let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
        let (mut reader, control) = open(&GOLDEN);
        cache.prepare(&mut reader, &[char::MAX]).unwrap();
        control.fail_offset.set(Some(failing_offset));
        assert_eq!(
            cache.prepare(&mut reader, &['A', '\u{20BB7}']),
            Err(PreparationError::Font(FontError::Io(SourceError::Injected)))
        );
        assert_empty(&cache);
        let last = control.reads.borrow().last().copied().unwrap();
        assert_eq!(
            last.0, failing_offset,
            "failure ends preparation without retry"
        );
        control.fail_offset.set(None);
        cache.prepare(&mut reader, &['A', '\u{20BB7}']).unwrap();
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get('A').unwrap().bitmap, [0x3C, 0x42]);
        assert_eq!(
            cache.get('\u{20BB7}').unwrap().bitmap,
            [0xFF, 0x80, 0, 0x80]
        );
    }
}

#[test]
fn corrupted_record_is_a_preparation_failure_and_a_replacement_reader_recovers() {
    let mut bytes = GOLDEN;
    put_u16(&mut bytes, 88 + 18, 8); // Declared four-byte bitmap cannot be 8x2.
    let (mut reader, _) = open(&bytes);
    let mut slots = [PageGlyphSlot::default(); 1];
    let mut bitmap = [0u8; 4];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    assert_eq!(
        cache.prepare(&mut reader, &['\u{20BB7}']),
        Err(PreparationError::Font(FontError::Corrupt(
            PackError::GlyphSizeMismatch { index: 2 }
        )))
    );
    assert_empty(&cache);
    let (mut healthy, _) = open(&GOLDEN);
    cache.prepare(&mut healthy, &['\u{20BB7}']).unwrap();
    assert_eq!(
        cache.get('\u{20BB7}').unwrap().bitmap,
        [0xFF, 0x80, 0, 0x80]
    );
}

#[test]
fn replacement_page_records_new_font_identity_and_empty_page_needs_no_io() {
    let mut slots = [PageGlyphSlot::default(); 1];
    let mut bitmap = [0u8; 4];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut first, _) = open(&GOLDEN);
    cache.prepare(&mut first, &['A']).unwrap();
    let mut second_pack = GOLDEN;
    put_u16(&mut second_pack, 6, 23);
    second_pack[8..16].copy_from_slice(&99u64.to_le_bytes());
    put_u16(&mut second_pack, 88 + 12, 23);
    let (mut second, control) = open(&second_pack);
    cache.prepare(&mut second, &['\u{20BB7}']).unwrap();
    assert_eq!(
        cache.info(),
        Some(pulp_fontpack::FontInfo {
            pixel_size: 23,
            font_id: 99,
            ..GOLDEN_INFO
        })
    );
    assert_eq!(cache.get('\u{20BB7}').unwrap().metrics.advance, 23);
    assert!(cache.get('A').is_none());
    let before = control.reads.borrow().len();
    cache.prepare(&mut second, &[]).unwrap();
    assert_eq!(cache.len(), 0);
    assert!(cache.get('\u{20BB7}').is_none());
    assert_eq!(cache.info().unwrap().font_id, 99);
    assert_eq!(control.reads.borrow().len(), before);
}

#[test]
fn located_preparation_equals_searching_one_and_skips_the_index_probes() {
    let page = ['A', 'B', '\u{20BB7}', char::MAX];
    let mut slots_a = [PageGlyphSlot::default(); 4];
    let mut bitmap_a = [0u8; 40];
    let needed = budget(&slots_a, &bitmap_a);
    let mut searched = PageCache::new(&mut slots_a[..], &mut bitmap_a[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    searched.prepare(&mut reader, &page).unwrap();
    let searching_reads = control.reads.borrow().len();

    // lookups made earlier, as a caller that kept them would hand them back
    let (mut reader, control) = open(&GOLDEN);
    let kept: Vec<_> = page.iter().map(|&c| (c, reader.find(c).unwrap())).collect();
    control.reads.borrow_mut().clear();
    let mut slots_b = [PageGlyphSlot::default(); 4];
    let mut bitmap_b = [0u8; 40];
    let mut located = PageCache::new(&mut slots_b[..], &mut bitmap_b[..], needed).unwrap();
    // rebuild each ref from its kept position, as the firmware does
    let rebuilt = |c: char| {
        kept.iter().find(|(k, _)| *k == c).map(|(_, found)| {
            found.map(|g| pulp_fontpack::GlyphRef::from_located(g.metrics, g.bitmap_offset()))
        })
    };
    located
        .prepare_located(&mut reader, &page, rebuilt)
        .unwrap();

    assert_eq!(located.len(), searched.len());
    assert_eq!(located.info(), searched.info());
    for c in page.iter().copied().chain(['C']) {
        assert_eq!(
            located.get(c),
            searched.get(c),
            "{c:?} differs between located and searching preparation"
        );
    }
    let located_reads = control.reads.borrow().clone();
    assert!(
        located_reads.len() < searching_reads,
        "{} reads located, {} searching",
        located_reads.len(),
        searching_reads
    );
    assert!(
        located_reads.iter().all(|&(offset, _)| offset >= 132),
        "a located preparation only reads bitmaps, got {located_reads:?}"
    );
}

#[test]
fn a_located_ref_outside_this_pack_is_rejected_not_read() {
    let mut slots = [PageGlyphSlot::default(); 1];
    let mut bitmap = [0u8; 8];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    let metrics = Metrics {
        advance: 1,
        offset_x: 0,
        offset_y: 0,
        width: 8,
        height: 1,
    };
    let foreign = pulp_fontpack::GlyphRef::from_located(metrics, 1_000_000);
    control.reads.borrow_mut().clear();
    let error = cache
        .prepare_located(&mut reader, &['A'], |_| Some(Some(foreign)))
        .unwrap_err();
    assert_eq!(
        error,
        PreparationError::Font(FontError::Corrupt(PackError::BadLayout))
    );
    assert!(
        control.reads.borrow().is_empty(),
        "no read outside the file"
    );
    assert_eq!(cache.len(), 0);
}

#[test]
fn a_preparation_in_pieces_equals_the_whole_one_and_stays_invisible_until_done() {
    let page = ['A', 'B', '\u{20BB7}', 'A', char::MAX];
    let mut slots_a = [PageGlyphSlot::default(); 4];
    let mut bitmap_a = [0u8; 40];
    let needed = budget(&slots_a, &bitmap_a);
    let mut whole = PageCache::new(&mut slots_a[..], &mut bitmap_a[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    whole.prepare(&mut reader, &page).unwrap();
    let whole_reads = control.reads.borrow().clone();

    let mut slots_b = [PageGlyphSlot::default(); 4];
    let mut bitmap_b = [0u8; 40];
    let mut pieces = PageCache::new(&mut slots_b[..], &mut bitmap_b[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    pieces.begin();
    let mut calls = 0;
    loop {
        calls += 1;
        // one glyph per call: `more` says stop every time it is asked
        let done = pieces
            .resume_located(&mut reader, &page, |_| None, || false)
            .unwrap();
        if done {
            break;
        }
        assert_empty(&pieces);
        assert_eq!(pieces.filled(), calls, "one more glyph per call");
        // the firmware's lists are distinct; after a duplicate `resumes` is false
        assert_eq!(pieces.resumes(&page), calls <= 3);
    }
    assert_eq!(
        calls,
        page.len(),
        "every list entry, duplicates included, is a step"
    );
    assert_eq!(
        *control.reads.borrow(),
        whole_reads,
        "no glyph is read twice"
    );
    assert_eq!(pieces.len(), whole.len());
    assert_eq!(pieces.info(), whole.info());
    for c in page.iter().copied().chain(['C']) {
        assert_eq!(pieces.get(c), whole.get(c), "{c:?}");
    }
}

#[test]
fn only_the_unfinished_preparation_of_the_same_list_resumes() {
    let page = ['A', 'B', '\u{20BB7}'];
    let mut slots = [PageGlyphSlot::default(); 3];
    let mut bitmap = [0u8; 30];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, _) = open(&GOLDEN);
    assert!(!cache.resumes(&page), "nothing started");
    cache.begin();
    assert!(!cache.resumes(&page), "nothing taken yet");
    assert!(
        !cache
            .resume_located(&mut reader, &page, |_| None, || false)
            .unwrap()
    );
    assert!(cache.resumes(&page));
    assert!(
        cache.resumes(&['A', 'B', 'A', 'B']),
        "the taken prefix matches"
    );
    assert!(!cache.resumes(&['B', 'A', '\u{20BB7}']), "another order");
    assert!(!cache.resumes(&['A']), "nothing left to take");
    assert!(
        cache
            .resume_located(&mut reader, &page, |_| None, || true)
            .unwrap()
    );
    assert!(!cache.resumes(&page), "a published page is not resumable");
    assert_eq!(cache.len(), 3);
}

#[test]
fn a_failure_between_pieces_hides_the_partial_page_and_a_new_begin_recovers() {
    let page = ['A', 'B', '\u{20BB7}'];
    let mut slots = [PageGlyphSlot::default(); 3];
    let mut bitmap = [0u8; 30];
    let needed = budget(&slots, &bitmap);
    let mut cache = PageCache::new(&mut slots[..], &mut bitmap[..], needed).unwrap();
    let (mut reader, control) = open(&GOLDEN);
    cache.begin();
    assert!(
        !cache
            .resume_located(&mut reader, &page, |_| None, || false)
            .unwrap()
    );
    control.disabled.set(true);
    let error = cache
        .resume_located(&mut reader, &page, |_| None, || true)
        .unwrap_err();
    assert!(matches!(error, PreparationError::Font(FontError::Io(_))));
    assert_empty(&cache);
    control.disabled.set(false);
    cache.begin();
    assert!(
        cache
            .resume_located(&mut reader, &page, |_| None, || true)
            .unwrap()
    );
    assert_eq!(cache.len(), 3);
}

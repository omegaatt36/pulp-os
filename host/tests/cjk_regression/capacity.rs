//! Large installed packs exercise the reader's metadata and bitmap ceilings.
use crate::cjk_support;

use pulp_fontpack::{Metrics, PackReader, ReadAt, bitmap_size};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fonts::{FontSet, Style, cjk, cjk::CjkState};
use pulp_host::kernel::Kernel;
use pulp_host::{ErrorKind, Result};
use std::cell::Cell;
use std::rc::Rc;

const RECORDS: u32 = 12_665;
const VISIBLE: u32 = 700;
const PX: u16 = 16;
const INDEX_END: u32 = 44 + 22 * RECORDS;

// Spread the requested scalars through the index, rather than one short interval.
fn text() -> String {
    (0..VISIBLE)
        .map(|i| char::from_u32(0x4e00 + 17 * i).unwrap())
        .collect()
}

fn metrics(i: u32, large: bool) -> Metrics {
    Metrics {
        advance: PX + (i % 3) as u16,
        offset_x: -((i % 2) as i16),
        offset_y: -16,
        width: if large { 32 } else { 16 },
        height: if large { 32 } else { 16 },
    }
}

// Literal v1 wire data keeps the fixture independent of the pack builder.
fn pack(large: bool) -> Vec<u8> {
    let bitmap_len = bitmap_size(metrics(0, large).width, metrics(0, large).height);
    let region = bitmap_len * RECORDS;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PFNT");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&PX.to_le_bytes());
    bytes.extend_from_slice(&0x1234_5678u64.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&PX.to_le_bytes());
    for n in [
        RECORDS,
        44,
        22 * RECORDS,
        INDEX_END,
        region,
        INDEX_END + region,
    ] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for i in 0..RECORDS {
        for n in [0x4e00 + i, bitmap_len * i, bitmap_len] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        let m = metrics(i, large);
        for n in [
            m.advance,
            m.offset_x as u16,
            m.offset_y as u16,
            m.width,
            m.height,
        ] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
    }
    for i in 0..RECORDS {
        for j in 0..bitmap_len {
            bytes.push((i.wrapping_mul(37) + j.wrapping_mul(13)) as u8);
        }
    }
    bytes
}

struct Counted<'a> {
    bytes: &'a [u8],
    reads: Rc<Cell<usize>>,
}
impl ReadAt for Counted<'_> {
    type Error = pulp_fontpack::OutOfRange;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> core::result::Result<(), Self::Error> {
        if offset >= 44 && offset < u64::from(INDEX_END) {
            self.reads.set(self.reads.get() + 1);
        }
        let mut bytes = self.bytes;
        bytes.read_at(offset, buf)
    }
}

thread_local! {
    static CLOCK: Cell<u64> = const { Cell::new(0) };
}
fn tick() -> u64 {
    CLOCK.with(|clock| {
        let now = clock.get();
        clock.set(now + 1);
        now
    })
}

fn run_steps(
    state: &mut CjkState,
    sliced: bool,
    mut step: impl FnMut(&mut CjkState) -> Result<()>,
) -> usize {
    let mut pauses = 0;
    loop {
        if sliced {
            state.arm_slice();
        }
        let result = step(state);
        state.disarm_slice();
        match result {
            Ok(()) => return pauses,
            Err(e) if cjk::is_slice_end(&e) => {
                pauses += 1;
                assert!(pauses < 1_000, "preparation must converge");
            }
            Err(e) => panic!("preparation failed: {e:?}"),
        }
    }
}

fn large_pack_page(sliced: bool) {
    let bytes = pack(false);
    let text = text();
    let chars: Vec<_> = text.chars().collect();
    assert_eq!(chars.len(), VISIBLE as usize);
    assert_eq!(chars.len() * 32, 22_400);
    let card = cjk_support::card(b"", false);
    cjk_support::install(&card, PX, &bytes);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    state.set_pace(tick, 3);
    let latin = FontSet::for_size(0);

    let reads = Rc::new(Cell::new(0));
    let mut reference = PackReader::open(
        Counted {
            bytes: &bytes,
            reads: reads.clone(),
        },
        bytes.len() as u64,
    )
    .unwrap();
    let located: Vec<_> = chars
        .iter()
        .map(|&ch| reference.find(ch).unwrap().unwrap())
        .collect();
    let plain_reads = reads.get();

    let stage_pauses = run_steps(&mut state, sliced, |state| {
        state.stage_metrics(&mut kernel.handle(), text.as_bytes(), latin, PX, 23)
    });
    for (&ch, glyph) in chars.iter().zip(&located) {
        assert_eq!(
            state.view(latin, PX, 23).advance(ch, Style::Regular),
            glyph.metrics.advance
        );
    }
    let stage_log = kernel.sd().card.read_log();
    let index_reads: Vec<_> = stage_log
        .iter()
        .filter(|r| r.offset >= 44 && r.offset < INDEX_END)
        .collect();
    assert!(
        index_reads.len() * 3 < plain_reads,
        "cached index reads={} plain={plain_reads}",
        index_reads.len()
    );
    // The terminal interval is batched, so caching must also reduce bytes read.
    let index_bytes: usize = index_reads.iter().map(|r| r.returned).sum();
    assert!(
        index_bytes < plain_reads * 22,
        "cached bytes={index_bytes} plain={}",
        plain_reads * 22
    );
    assert_eq!(kernel.sd().card.open_count(), stage_pauses + 1);

    kernel.sd().card.reset_reads();
    state
        .mark_visible(text.as_bytes(), Style::Regular, latin, PX, 23)
        .unwrap();
    let bitmap_pauses = run_steps(&mut state, sliced, |state| {
        let result = state.prepare_visible(&mut kernel.handle(), PX, 23);
        if result.is_err() {
            assert!(
                chars.iter().all(|&ch| state.get(PX, ch).is_none()),
                "paused page must stay unpublished"
            );
        }
        result
    });
    let bitmap_log = kernel.sd().card.read_log();
    assert!(
        bitmap_log
            .iter()
            .all(|r| r.offset == 0 || r.offset >= INDEX_END),
        "bitmap preparation must reuse located glyphs"
    );
    assert_eq!(
        bitmap_log.iter().filter(|r| r.offset >= INDEX_END).count(),
        VISIBLE as usize
    );
    assert_eq!(kernel.sd().card.open_count(), bitmap_pauses + 1);
    for (&ch, glyph) in chars.iter().zip(&located) {
        let mut bitmap = vec![0; bitmap_size(glyph.metrics.width, glyph.metrics.height) as usize];
        let want = reference.read_bitmap(glyph, &mut bitmap).unwrap();
        let got = state.get(PX, ch).unwrap();
        assert_eq!(
            (got.metrics, got.bitmap),
            (glyph.metrics, want),
            "glyph {ch:?}"
        );
    }
    if sliced {
        assert!(
            stage_pauses > 200 && bitmap_pauses > 200,
            "short slices must reopen many readers"
        );
    } else {
        assert_eq!((stage_pauses, bitmap_pauses), (0, 0));
    }
    eprintln!(
        "700 scalars sliced={sliced}: cached index reads={} bytes={index_bytes}; plain reads={plain_reads}; stage pauses={stage_pauses}, bitmap pauses={bitmap_pauses}",
        index_reads.len()
    );
}

#[test]
fn seven_hundred_visible_glyphs_equal_the_plain_reader() {
    large_pack_page(false);
}

#[test]
fn seven_hundred_visible_glyphs_keep_the_index_cache_across_short_slices() {
    large_pack_page(true);
}

#[test]
fn bitmap_over_budget_unpublishes_the_page_and_recovers() {
    let bytes = pack(true);
    let card = cjk_support::card(b"", false);
    cjk_support::install(&card, PX, &bytes);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    let latin = FontSet::for_size(0);
    let text = text();
    let first = text.chars().next().unwrap().to_string();
    state
        .prepare_text(&mut kernel.handle(), first.as_bytes(), latin, PX, 23)
        .unwrap();
    assert!(state.get(PX, first.chars().next().unwrap()).is_some());
    assert!(VISIBLE * 128 > 64 * 1024);
    let error = state
        .prepare_text(&mut kernel.handle(), text.as_bytes(), latin, PX, 23)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BufferTooSmall);
    assert!(
        text.chars().all(|ch| state.get(PX, ch).is_none()),
        "old and partial pages must stay unpublished"
    );
    state
        .prepare_text(&mut kernel.handle(), first.as_bytes(), latin, PX, 23)
        .unwrap();
    assert!(state.get(PX, first.chars().next().unwrap()).is_some());
}

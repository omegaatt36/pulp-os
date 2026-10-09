//! sd-read-latency (R3, R4): the pack is opened once per role and scalars are
//! looked up once per session, with output identical to the per-read path.
//!
//! The reference ("old path") is the fontpack reader over a plain byte slice
//! with one `read_at` per probe and a fresh reader per phase, which is what
//! the firmware did with one open per read. The new path runs the real
//! `CjkState` against the virtual card, whose read log and open counter show
//! what reached the storage layer.
mod cjk_support;

use cjk_support::{alphabet, card, install, pack, path};
use pulp_fontpack::{
    FontInfo, Glyph, PackReader, PageCache, PageGlyphSlot, ReadAt, bitmap_size,
    missing_glyph_metrics, render_missing_glyph,
};
use pulp_host::ErrorKind;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fonts::{FontSet, Style, cjk, cjk::CjkState};
use pulp_host::kernel::Kernel;
use pulp_host::storage::{StorageOp, VirtualStorage};
use smol_epub::html_strip::{HEADING_OFF, HEADING_ON, MARKER};
use std::cell::Cell;
use std::rc::Rc;

const BODY: u16 = 16;
const HEAD: u16 = 23;
const INDEX_START: u32 = 44;

/// Body text with a scalar the pack lacks ('未'), then a heading run.
fn window() -> String {
    let mut t = String::from("臺灣繁體未A中文𠮷，。");
    t.push(MARKER as char);
    t.push(HEADING_ON as char);
    t.push_str("臺未中");
    t.push(MARKER as char);
    t.push(HEADING_OFF as char);
    t.push_str("灣");
    t
}

fn distinct(text: &str) -> Vec<char> {
    let mut v: Vec<char> = text.chars().filter(|&c| c as u32 > 0x7f).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Old path: per-read `ReadAt` over a byte slice, counted.
struct Counted<'a> {
    bytes: &'a [u8],
    reads: Rc<Cell<usize>>,
}
impl ReadAt for Counted<'_> {
    type Error = pulp_fontpack::OutOfRange;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.reads.set(self.reads.get() + 1);
        let mut bytes = self.bytes;
        bytes.read_at(offset, buf)
    }
}

type RefCache = PageCache<Vec<PageGlyphSlot>, Vec<u8>>;

/// What the old code did for one role: a reader for the metrics lookup and a
/// second one for the page cache (each `open` read the header), `find` for
/// every scalar in both. Returns the prepared cache and the read count.
fn old_role(bytes: &[u8], chars: &[char]) -> (RefCache, usize) {
    let reads = Rc::new(Cell::new(0));
    let src = |reads: &Rc<Cell<usize>>| Counted {
        bytes,
        reads: reads.clone(),
    };
    let mut stage = PackReader::open(src(&reads), bytes.len() as u64).unwrap();
    for &c in chars {
        stage.find(c).unwrap();
    }
    let mut reader = PackReader::open(src(&reads), bytes.len() as u64).unwrap();
    let mut cache = PageCache::new(
        vec![PageGlyphSlot::default(); chars.len()],
        vec![0u8; 64 * chars.len()],
        usize::MAX,
    )
    .unwrap();
    cache.prepare(&mut reader, chars).unwrap();
    (cache, reads.get())
}

fn kernel(card: VirtualStorage) -> Kernel {
    Kernel::new(SdStorage::new(card))
}

fn latin() -> FontSet {
    FontSet::for_size(0)
}

fn prepare(state: &mut CjkState, k: &mut Kernel, text: &str) -> pulp_host::Result<()> {
    state.prepare_text(&mut k.handle(), text.as_bytes(), latin(), BODY, HEAD)
}

fn glyph<'a>(state: &'a CjkState, px: u16, c: char) -> Glyph<'a> {
    state
        .get(px, c)
        .unwrap_or_else(|| panic!("{c:?} at {px}px is not prepared"))
}

fn hollow(px: u16) -> (pulp_fontpack::Metrics, Vec<u8>) {
    let info = FontInfo {
        pixel_size: px,
        font_id: 0,
        line_height: px,
        ascent: px,
    };
    let m = missing_glyph_metrics(&info);
    let mut bitmap = vec![0; bitmap_size(m.width, m.height) as usize];
    render_missing_glyph(&info, &mut bitmap).unwrap();
    (m, bitmap)
}

#[test]
fn prepared_glyphs_and_advances_equal_the_per_read_reference() {
    let text = window();
    for wide in [false, true] {
        let card = card(b"", false);
        for px in [BODY, HEAD] {
            install(&card, px, &pack(px, wide));
        }
        let mut k = kernel(card);
        let mut state = CjkState::new();
        prepare(&mut state, &mut k, &text).unwrap();
        let view = state.view(latin(), BODY, HEAD);

        let heading: Vec<char> = distinct("臺未中");
        let body: Vec<char> = distinct(&text);
        for (px, sty, chars) in [
            (BODY, Style::Regular, body),
            (HEAD, Style::Heading, heading),
        ] {
            let (reference, _) = old_role(&pack(px, wide), &chars);
            for &c in &chars {
                let want = reference.get(c).unwrap();
                assert_eq!(glyph(&state, px, c), want, "{c:?} at {px}px (wide={wide})");
                assert_eq!(
                    view.advance(c, sty),
                    want.metrics.advance,
                    "advance of {c:?} at {px}px (wide={wide})"
                );
            }
        }
        // the scalar the pack lacks is the hollow box, at both sizes
        for px in [BODY, HEAD] {
            let (m, bitmap) = hollow(px);
            let g = glyph(&state, px, '未');
            assert_eq!((g.metrics, g.bitmap), (m, &bitmap[..]));
        }
    }
}

#[test]
fn one_open_per_role_replaces_one_open_per_read() {
    let text = window();
    let card = card(b"", false);
    for px in [BODY, HEAD] {
        install(&card, px, &pack(px, false));
    }
    let mut k = kernel(card);
    let mut state = CjkState::new();
    prepare(&mut state, &mut k, &text).unwrap();
    let storage = k.sd().card.read_log();
    let new_reads = storage.len();
    let new_opens = k.sd().card.open_count();

    let body = distinct(&text);
    let heading = distinct("臺未中");
    let old_reads =
        old_role(&pack(BODY, false), &body).1 + old_role(&pack(HEAD, false), &heading).1;

    eprintln!(
        "reads old={old_reads} (one open each) new={new_reads} in {new_opens} opens, \
         {} scalars",
        body.len() + heading.len()
    );
    // stage + prepare, two banks
    assert_eq!(new_opens, 4);
    assert!(
        new_reads < old_reads,
        "new path must issue fewer pack reads: {new_reads} vs {old_reads}"
    );
    assert!(
        new_opens < old_reads,
        "opens must drop from one per read to one per bank and phase"
    );
    // no scalar is searched twice: every index probe belongs to the staging phase
    // (one probe sequence per scalar), the prepare phase reads bitmaps only
    let index_end = INDEX_START + 22 * alphabet().len() as u32;
    let probes = storage
        .iter()
        .filter(|r| r.offset >= INDEX_START && r.offset < index_end)
        .count();
    let per_scalar_ceiling = (alphabet().len() as f64).log2().ceil() as usize + 1;
    assert!(
        probes <= (body.len() + heading.len()) * per_scalar_ceiling,
        "{probes} index probes for {} scalars",
        body.len() + heading.len()
    );
}

#[test]
fn a_repeated_window_reads_no_glyph_from_the_pack() {
    let text = window();
    let card = card(b"", false);
    for px in [BODY, HEAD] {
        install(&card, px, &pack(px, false));
    }
    let mut k = kernel(card);
    let mut state = CjkState::new();
    prepare(&mut state, &mut k, &text).unwrap();
    let first: Vec<_> = distinct(&text)
        .into_iter()
        .map(|c| {
            (
                c,
                glyph(&state, BODY, c).metrics,
                glyph(&state, BODY, c).bitmap.to_vec(),
            )
        })
        .collect();

    // staging again: nothing is looked up, nothing is opened
    k.sd().card.reset_reads();
    state
        .stage_metrics(&mut k.handle(), text.as_bytes(), latin(), BODY, HEAD)
        .unwrap();
    assert_eq!(
        k.sd().card.read_count(),
        0,
        "staging a known window reads the pack"
    );
    assert_eq!(
        k.sd().card.open_count(),
        0,
        "staging a known window opens the pack"
    );

    // preparing the same page again keeps its glyphs: only each bank's header is checked
    k.sd().card.reset_reads();
    prepare(&mut state, &mut k, &text).unwrap();
    let log = k.sd().card.read_log();
    assert!(
        log.iter()
            .all(|r| r.offset == 0 && r.requested == INDEX_START as usize),
        "a repeated page reads more than the headers: {log:?}"
    );
    assert!(log.len() <= 2);
    for (c, m, bitmap) in first {
        let g = glyph(&state, BODY, c);
        assert_eq!((g.metrics, g.bitmap), (m, &bitmap[..]));
    }
}

#[test]
fn returning_to_an_earlier_page_reads_bitmaps_but_searches_nothing() {
    let card = card(b"", false);
    install(&card, BODY, &pack(BODY, true));
    let mut k = kernel(card);
    let mut state = CjkState::new();
    let a = "臺灣繁體";
    let b = "中文𠮷，。";
    prepare(&mut state, &mut k, a).unwrap();
    let want: Vec<_> = distinct(a)
        .into_iter()
        .map(|c| {
            (
                c,
                glyph(&state, BODY, c).metrics,
                glyph(&state, BODY, c).bitmap.to_vec(),
            )
        })
        .collect();
    prepare(&mut state, &mut k, b).unwrap();
    assert!(
        state.get(BODY, '臺').is_none(),
        "the new page replaces the old one"
    );

    k.sd().card.reset_reads();
    prepare(&mut state, &mut k, a).unwrap();
    let index_end = INDEX_START + 22 * alphabet().len() as u32;
    let log = k.sd().card.read_log();
    assert!(
        log.iter()
            .all(|r| r.offset < INDEX_START || r.offset >= index_end),
        "returning to a page probed the index: {log:?}"
    );
    assert_eq!(log.len(), 1 + 4, "header plus one bitmap read per scalar");
    for (c, m, bitmap) in want {
        let g = glyph(&state, BODY, c);
        assert_eq!((g.metrics, g.bitmap), (m, &bitmap[..]));
    }
}

#[test]
fn metrics_kept_across_windows_do_not_leak_into_the_current_one() {
    let card = card(b"", false);
    install(&card, BODY, &pack(BODY, true));
    let mut k = kernel(card);
    let mut state = CjkState::new();
    let stage = |state: &mut CjkState, k: &mut Kernel, t: &str| {
        state.stage_metrics(&mut k.handle(), t.as_bytes(), latin(), BODY, HEAD)
    };
    stage(&mut state, &mut k, "臺灣").unwrap();
    assert!(state.has_fallback() && state.uses_bank(BODY));
    // Latin only: as before, nothing needs the bank
    stage(&mut state, &mut k, "hello").unwrap();
    assert!(
        !state.has_fallback(),
        "kept entries must not report a fallback"
    );
    assert!(!state.uses_bank(BODY));
    // the kept entry is not part of the new window
    stage(&mut state, &mut k, "體").unwrap();
    let view = state.view(latin(), BODY, HEAD);
    assert_eq!(view.advance('體', Style::Regular), 301);
    assert_eq!(
        view.advance('臺', Style::Regular),
        hollow(BODY).0.advance,
        "an unstaged scalar is the fallback box, as before"
    );
    state.begin_visible();
    assert!(
        state
            .mark_visible("臺".as_bytes(), Style::Regular, latin(), BODY, HEAD)
            .is_err()
    );
    // ... and returning to it costs no lookup
    k.sd().card.reset_reads();
    stage(&mut state, &mut k, "臺灣體").unwrap();
    assert_eq!(k.sd().card.read_count(), 0);
}

#[test]
fn a_full_metric_budget_drops_kept_entries_instead_of_failing_a_window_that_fits() {
    // no pack installed: preparation needs no I/O, only the metric table
    let mut k = kernel(card(b"", false));
    let mut state = CjkState::new();
    let run =
        |from: u32, n: u32| -> String { (from..from + n).filter_map(char::from_u32).collect() };
    let stage = |state: &mut CjkState, k: &mut Kernel, t: &str| {
        state.stage_metrics(&mut k.handle(), t.as_bytes(), latin(), BODY, HEAD)
    };
    // 16 KiB of 20-byte entries is 819; two windows of 500 do not fit together
    stage(&mut state, &mut k, &run(0x4e00, 500)).unwrap();
    stage(&mut state, &mut k, &run(0x5000, 500)).unwrap();
    stage(&mut state, &mut k, &run(0x4e00, 500)).unwrap();
    let err = stage(&mut state, &mut k, &run(0x6000, 900)).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::BufferTooSmall,
        "a window over the budget still fails"
    );
}

#[test]
fn absent_pack_prepares_hollow_boxes_without_touching_the_card() {
    let mut k = kernel(card(b"", false));
    let mut state = CjkState::new();
    prepare(&mut state, &mut k, &window()).unwrap();
    for (px, chars) in [(BODY, "臺灣未"), (HEAD, "臺未")] {
        let (m, bitmap) = hollow(px);
        for c in distinct(chars) {
            let g = glyph(&state, px, c);
            assert_eq!((g.metrics, g.bitmap), (m, &bitmap[..]), "{c:?} at {px}px");
        }
    }
    assert_eq!(k.sd().card.read_count(), 0);
    assert_eq!(k.sd().card.open_count(), 0);
    let id = cjk::bank_identity(&mut k.handle(), BODY).unwrap();
    assert!(!id.installed);
    assert_eq!(id.font_id, 0);
    assert_eq!(id.pixel_size, BODY);
    assert_eq!(k.sd().card.read_count(), 0);
}

#[test]
fn damaged_packs_fail_as_invalid_data_and_publish_nothing() {
    let good = pack(BODY, false);
    let mut bad_magic = good.clone();
    bad_magic[0] ^= 0xff;
    let mut bad_version = good.clone();
    bad_version[4] = 9;
    let cases: [(&str, Vec<u8>); 6] = [
        ("truncated", good[..good.len() - 1].to_vec()),
        ("shorter than the header", good[..10].to_vec()),
        ("empty", Vec::new()),
        ("bad magic", bad_magic),
        ("other version", bad_version),
        ("other pixel size", pack(19, false)),
    ];
    for (name, bytes) in cases {
        // the reference refuses the same bytes at the same point
        if name != "other pixel size" {
            assert!(
                PackReader::open(&bytes[..], bytes.len() as u64).is_err(),
                "{name}: reference reader accepted it"
            );
        }
        let card = card(b"", false);
        install(&card, BODY, &bytes);
        let mut k = kernel(card);
        let mut state = CjkState::new();
        let err = prepare(&mut state, &mut k, "臺灣").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData, "{name}");
        assert!(
            state.get(BODY, '臺').is_none(),
            "{name}: nothing is published"
        );
        let err = cjk::bank_identity(&mut k.handle(), BODY)
            .err()
            .expect("identity of a damaged pack");
        assert_eq!(err.kind(), ErrorKind::InvalidData, "{name}: identity");
        // the failure is recoverable: the repaired pack prepares
        install(&k.sd().card, BODY, &good);
        prepare(&mut state, &mut k, "臺灣").unwrap();
        assert!(state.get(BODY, '灣').is_some(), "{name}: after repair");
    }
}

#[test]
fn a_read_error_in_the_middle_of_a_pack_is_recoverable_and_closes_cleanly() {
    let card = card(b"", false);
    install(&card, BODY, &pack(BODY, false));
    let mut k = kernel(card);
    let mut state = CjkState::new();
    let text = "臺灣繁體中文";
    for (nth, op) in [
        (1, StorageOp::Read),
        (3, StorageOp::Read),
        (9, StorageOp::Read),
    ] {
        k.sd()
            .card
            .inject_error(op, &path(BODY), nth, ErrorKind::ReadFailed);
        let err = prepare(&mut state, &mut k, text).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ReadFailed, "read {nth}");
        assert!(
            state.get(BODY, '臺').is_none(),
            "read {nth}: nothing is published"
        );
        k.sd().card.clear_injections();
        prepare(&mut state, &mut k, text).unwrap();
        assert!(
            state.get(BODY, '體').is_some(),
            "read {nth}: retry succeeds"
        );
        state.clear();
    }
}

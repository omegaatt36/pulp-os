//! sd-read-latency (R2, R5, R6): a long CJK preparation runs in bounded slices
//! with the same result as one call, ends every slice with the pack closed, and
//! turns a card that vanishes into a recoverable error.
//!
//! Slices run on a fake clock: every reading advances it by a fixed step, so a
//! slice of N steps ends after a fixed number of glyphs and the heartbeat
//! cadence is exact.
mod cjk_support;

use cjk_support::{BOOK, alphabet, card, install, pack, path, rig};
use pulp_fontpack::Metrics;
use pulp_host::ErrorKind;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fixtures::{Spec, standard, standard_card};
use pulp_host::fonts::{FontSet, cjk, cjk::CjkState};
use pulp_host::kernel::Kernel;
use pulp_host::reader::{Phase, Rig};
use pulp_host::storage::{StorageOp, VirtualStorage};
use smol_epub::html_strip::{HEADING_OFF, HEADING_ON, MARKER};
use std::cell::Cell;
use std::sync::{Mutex, Once};

const BODY: u16 = 16;
const HEAD: u16 = 23;

thread_local! {
    static CLOCK: Cell<u64> = const { Cell::new(0) };
    static STEP: Cell<u64> = const { Cell::new(1) };
}

/// The fake clock: each reading returns the time and moves it on by the step.
fn fake_now() -> u64 {
    CLOCK.with(|c| {
        let now = c.get();
        c.set(now + STEP.with(Cell::get));
        now
    })
}

fn set_clock(start: u64, step: u64) {
    CLOCK.with(|c| c.set(start));
    STEP.with(|s| s.set(step));
}

fn peek_clock() -> u64 {
    CLOCK.with(Cell::get)
}

type Logged = (std::thread::ThreadId, u64, String);
static LOG: Mutex<Vec<Logged>> = Mutex::new(Vec::new());

struct Capture;
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LOG.lock().unwrap_or_else(|e| e.into_inner()).push((
            std::thread::current().id(),
            peek_clock(),
            record.args().to_string(),
        ));
    }
    fn flush(&self) {}
}

fn capture_log() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log::set_logger(&Capture).expect("no other logger in this test binary");
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// Body text with a scalar the pack lacks ('未') and a heading run.
fn window() -> String {
    let mut t = String::from("臺灣繁體未A中文𠮷，。、？！）」』】（「『【");
    t.push(MARKER as char);
    t.push(HEADING_ON as char);
    t.push_str("臺未中文");
    t.push(MARKER as char);
    t.push(HEADING_OFF as char);
    t.push('灣');
    t
}

fn kernel_with_packs() -> Kernel {
    let card = card(b"", false);
    for px in [BODY, HEAD] {
        install(&card, px, &pack(px, false));
    }
    Kernel::new(SdStorage::new(card))
}

fn prepare(state: &mut CjkState, k: &mut Kernel, text: &str) -> pulp_host::Result<()> {
    state.prepare_text(
        &mut k.handle(),
        text.as_bytes(),
        FontSet::for_size(0),
        BODY,
        HEAD,
    )
}

/// What the state would draw: every scalar of the pack and the missing one,
/// at both sizes.
fn drawn(state: &CjkState) -> Vec<(u16, char, Option<(Metrics, Vec<u8>)>)> {
    let mut chars = alphabet();
    chars.push('未');
    [BODY, HEAD]
        .into_iter()
        .flat_map(|px| chars.iter().map(move |&c| (px, c)))
        .map(|(px, c)| {
            let g = state.get(px, c).map(|g| (g.metrics, g.bitmap.to_vec()));
            (px, c, g)
        })
        .collect()
}

/// Run `prepare` the way the reader's background step does: arm a slice, call,
/// and on a slice end hand control to `between` (the scheduler: keys, card
/// detect, a panel refresh). Returns the slices it took.
fn sliced(
    state: &mut CjkState,
    k: &mut Kernel,
    text: &str,
    mut between: impl FnMut(&mut CjkState, &mut Kernel),
) -> pulp_host::Result<usize> {
    let mut slices = 0;
    loop {
        state.arm_slice();
        let result = prepare(state, k, text);
        state.disarm_slice();
        // nothing may stay open across the point where the scheduler runs
        assert_eq!(k.sd().card.held_handles(), 0, "pack open after a call");
        match result {
            Ok(()) => return Ok(slices),
            Err(e) if cjk::is_slice_end(&e) => {
                slices += 1;
                assert!(slices < 10_000, "preparation does not converge");
                between(state, k);
            }
            Err(e) => return Err(e),
        }
    }
}

fn unsliced(text: &str) -> (CjkState, Kernel) {
    let mut k = kernel_with_packs();
    let mut state = CjkState::new();
    prepare(&mut state, &mut k, text).unwrap();
    (state, k)
}

fn pack_reads(card: &VirtualStorage) -> Vec<(u32, usize)> {
    let mut reads: Vec<_> = card
        .read_log()
        .iter()
        .filter(|r| r.path.contains("FONTS"))
        .map(|r| (r.offset, r.requested))
        // the header is read again by every open; glyph reads must not repeat
        .filter(|&(offset, _)| offset >= 44)
        .collect();
    reads.sort_unstable();
    reads
}

#[test]
fn a_sliced_preparation_equals_the_unsliced_one_and_returns_control_between_slices() {
    let text = window();
    let (reference, ref_kernel) = unsliced(&text);
    let want = drawn(&reference);
    assert!(want.iter().any(|(_, c, g)| *c == '臺' && g.is_some()));

    set_clock(0, 1);
    let mut k = kernel_with_packs();
    let mut state = CjkState::new();
    // three glyphs per slice: the first reading arms, each glyph reads once more
    state.set_pace(fake_now, 3);
    let mut polled = 0;
    let slices = sliced(&mut state, &mut k, &text, |state, _| {
        polled += 1;
        // a paused preparation shows nothing of the page being made
        assert!(
            drawn(state).iter().all(|(_, _, g)| g.is_none()),
            "a paused preparation published glyphs"
        );
        let (done, total) = state.progress();
        assert!(done < total && total > 0);
        assert!(state.remaining_us() > 0);
    })
    .unwrap();

    assert!(slices >= 6, "{slices} slices: the work was not cut up");
    assert_eq!(polled, slices, "the hook ran once per slice end");
    assert_eq!(
        drawn(&state),
        want,
        "glyphs and metrics differ from one call"
    );
    assert_eq!(
        state
            .view(FontSet::for_size(0), BODY, HEAD)
            .advance('臺', cjk_style()),
        reference
            .view(FontSet::for_size(0), BODY, HEAD)
            .advance('臺', cjk_style())
    );
    // every glyph is read exactly once however many slices there were
    assert_eq!(
        pack_reads(&k.sd().card),
        pack_reads(&ref_kernel.sd().card),
        "slicing repeated or skipped a pack read"
    );
}

fn cjk_style() -> pulp_host::fonts::Style {
    pulp_host::fonts::Style::Regular
}

#[test]
fn without_an_armed_slice_nothing_is_cut_off() {
    let text = window();
    let (reference, _) = unsliced(&text);
    set_clock(0, 1_000_000);
    let mut k = kernel_with_packs();
    let mut state = CjkState::new();
    // a clock that is always past any slice, never armed
    state.set_pace(fake_now, 1);
    prepare(&mut state, &mut k, &text).unwrap();
    assert_eq!(drawn(&state), drawn(&reference));
}

#[test]
fn a_slice_stays_within_its_read_budget() {
    let text = window();
    set_clock(0, 1);
    let mut k = kernel_with_packs();
    let mut state = CjkState::new();
    state.set_pace(fake_now, 3);
    let per_glyph_probes = (alphabet().len() as f64).log2().ceil() as usize + 1;
    let mut seen = 0;
    sliced(&mut state, &mut k, &text, |_, k| {
        let reads = k.sd().card.read_count() - seen;
        seen += reads;
        // up to two headers (a slice can span both banks) + three glyphs of probes
        assert!(
            reads <= 3 + 3 * per_glyph_probes,
            "{reads} reads in one slice"
        );
    })
    .unwrap();
}

#[test]
fn a_paused_preparation_logs_a_heartbeat_at_least_every_five_seconds() {
    capture_log();
    let text = window();
    // 0.5 s per reading and a one-reading slice: a glyph per slice, ~1.5 s each
    set_clock(10_000_000, 500_000);
    let mut k = kernel_with_packs();
    let mut state = CjkState::new();
    state.set_pace(fake_now, 1);
    let me = std::thread::current().id();
    let slices = sliced(&mut state, &mut k, &text, |_, _| {}).unwrap();
    assert!(slices >= 20);

    let beats: Vec<u64> = LOG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(t, _, m)| *t == me && m.starts_with("cjk: "))
        .map(|(_, at, _)| *at)
        .collect();
    assert!(
        beats.len() >= 5,
        "{} heartbeats in ~{} s",
        beats.len(),
        slices * 3 / 2
    );
    for pair in beats.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(gap >= 4_000_000, "heartbeats {gap} us apart: log spam");
        assert!(gap <= 5_000_000, "{gap} us without a heartbeat");
    }
    let line = LOG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(t, _, m)| *t == me && m.starts_with("cjk: "))
        .map(|(_, _, m)| m.clone())
        .unwrap();
    assert!(
        line.contains("glyphs") && line.contains('/'),
        "heartbeat carries done/total: {line}"
    );
}

#[test]
fn a_card_that_fails_at_any_read_gives_a_recoverable_error_and_a_clean_retry() {
    let text = window();
    let (reference, ref_kernel) = unsliced(&text);
    let want = drawn(&reference);
    let total_reads = ref_kernel.sd().card.read_count();
    assert!(total_reads > 30);

    let mut failed = 0;
    // the card goes at each pack read in turn: inside a lookup slice, inside a
    // bitmap slice, at a header
    for nth in 1..=total_reads + 8 {
        set_clock(0, 1);
        let mut k = kernel_with_packs();
        let mut state = CjkState::new();
        state.set_pace(fake_now, 3);
        k.sd()
            .card
            .inject_error(StorageOp::Read, &path(BODY), nth, ErrorKind::NoCard);
        // (reads of the 23 px bank are not injected: the sweep is over the body bank)
        match sliced(&mut state, &mut k, &text, |_, _| {}) {
            Ok(_) => assert_eq!(drawn(&state), want, "read {nth} was not reached"),
            Err(e) => {
                failed += 1;
                assert_eq!(e.kind(), ErrorKind::NoCard, "read {nth}");
                assert!(
                    drawn(&state).iter().all(|(_, _, g)| g.is_none()),
                    "read {nth}: a failed preparation published glyphs"
                );
                k.sd().card.clear_injections();
                // card back: the same state prepares the page
                sliced(&mut state, &mut k, &text, |_, _| {}).unwrap();
                assert_eq!(drawn(&state), want, "read {nth}: retry differs");
            }
        }
        assert_eq!(k.sd().card.held_handles(), 0, "read {nth}: pack left open");
    }
    assert!(failed > 20, "only {failed} of the reads were failed");
}

#[test]
fn a_card_removed_between_slices_is_no_card_not_a_panic_and_reinsertion_resumes() {
    let text = window();
    let (reference, _) = unsliced(&text);
    let want = drawn(&reference);
    // remove the card after the n-th slice for every n, pause or not
    for after in 1..=6 {
        set_clock(0, 1);
        let mut k = kernel_with_packs();
        let mut state = CjkState::new();
        state.set_pace(fake_now, 3);
        let mut slices = 0;
        let result = sliced(&mut state, &mut k, &text, |_, k| {
            slices += 1;
            if slices == after {
                k.set_card_present(false);
            }
        });
        let e = result.expect_err("a removed card cannot finish the page");
        assert_eq!(e.kind(), ErrorKind::NoCard, "removed after slice {after}");
        assert!(drawn(&state).iter().all(|(_, _, g)| g.is_none()));
        assert_eq!(k.sd().card.held_handles(), 0);

        k.set_card_present(true);
        sliced(&mut state, &mut k, &text, |_, _| {}).unwrap();
        assert_eq!(drawn(&state), want, "reinserted after slice {after}");
    }
}

// ---- the reader: loading indication, polling, removal --------------------

const WIDTH: u32 = 200;

fn cjk_book() -> Vec<u8> {
    let mut t = String::new();
    for _ in 0..4 {
        t.extend(alphabet().into_iter().filter(|&c| c != 'A'));
        t.push('\n');
    }
    t.into_bytes()
}

fn reader_reference(text: &[u8]) -> (Vec<Vec<u8>>, Vec<u32>, usize) {
    let r = rig(text, 0, WIDTH);
    (r.lines(), r.page_offsets(), r.total_pages())
}

fn paced_rig(text: &[u8]) -> Rig {
    // 0.2 s per reading, 0.5 s slices: three glyphs a slice, seconds of work
    set_clock(0, 200_000);
    let mut r = Rig::new(card(text, true));
    r.configure(0, 0);
    r.set_text_width(WIDTH);
    r.set_cjk_pace(fake_now, 500_000);
    r
}

#[test]
fn opening_a_cjk_book_announces_the_wait_once_and_never_refreshes_between_slices() {
    let text = cjk_book();
    let (lines, offsets, pages) = reader_reference(&text);

    let mut r = paced_rig(&text);
    r.enter(BOOK);
    let mut ticks = 0;
    let mut noticed_at = None;
    while r.phase() == Phase::Loading {
        r.idle(1);
        ticks += 1;
        assert!(ticks < 300, "the book never opened");
        assert_eq!(r.storage().held_handles(), 0, "tick {ticks}: pack open");
        if r.phase() != Phase::Loading {
            break;
        }
        match noticed_at {
            None if r.loading_message().starts_with("Glyphs") => {
                noticed_at = Some(ticks);
                // the scheduler refreshes the panel here, before the next slice
                assert!(r.has_redraw(), "the indication was not requested");
                r.take_redraw();
            }
            None => {}
            Some(_) => {
                assert!(
                    !r.has_redraw(),
                    "tick {ticks}: a refresh was requested between slices"
                );
                assert!(r.loading_message().starts_with("Glyphs"));
            }
        }
    }
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(
        noticed_at,
        Some(1),
        "indication before the first slice's successor"
    );
    assert!(ticks >= 4, "{ticks} ticks: the preparation was not sliced");
    assert!(!r.loading_active(), "the indication is cleared on the page");
    assert_eq!(
        r.lines(),
        lines,
        "page content differs from the unsliced open"
    );
    assert_eq!(r.page_offsets(), offsets, "page breaks differ");
    assert_eq!(r.total_pages(), pages);
}

#[test]
fn leaving_a_book_between_slices_leaves_nothing_behind() {
    let text = cjk_book();
    let (lines, offsets, _) = reader_reference(&text);
    let mut r = paced_rig(&text);
    r.enter(BOOK);
    for _ in 0..50 {
        if r.loading_message().starts_with("Glyphs") {
            break;
        }
        r.idle(1);
    }
    assert!(
        r.loading_message().starts_with("Glyphs"),
        "no glyph indication, state {:?}",
        r.phase()
    );
    r.idle(2);
    // Back: the app manager exits the reader mid-preparation
    r.exit();
    assert_eq!(r.storage().held_handles(), 0);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.lines(), lines);
    assert_eq!(r.page_offsets(), offsets);
}

#[test]
fn a_card_removed_while_a_book_opens_is_an_error_page_and_reopening_works() {
    let text = cjk_book();
    let (lines, offsets, _) = reader_reference(&text);
    let mut r = paced_rig(&text);
    r.enter(BOOK);
    for _ in 0..50 {
        if r.loading_message().starts_with("Glyphs") {
            break;
        }
        r.idle(1);
    }
    assert!(
        r.loading_message().starts_with("Glyphs"),
        "no glyph indication, state {:?}",
        r.phase()
    );
    r.take_redraw();
    r.set_sd_ok(false);
    r.idle(1);
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::NoCard));
    assert!(!r.loading_active(), "the indication outlived the failure");
    assert!(r.has_redraw(), "the error page was not requested");
    assert_eq!(r.storage().held_handles(), 0);

    r.set_sd_ok(true);
    r.exit();
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.lines(), lines);
    assert_eq!(r.page_offsets(), offsets);
}

#[test]
fn a_pack_read_failing_mid_slice_while_a_book_opens_is_an_error_page() {
    let text = cjk_book();
    let mut r = paced_rig(&text);
    r.storage()
        .inject_error(StorageOp::Read, &path(BODY), 12, ErrorKind::ReadFailed);
    r.enter(BOOK);
    let mut ticks = 0;
    while r.phase() == Phase::Loading {
        r.idle(1);
        ticks += 1;
        assert!(ticks < 300);
        assert_eq!(r.storage().held_handles(), 0);
    }
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::ReadFailed));
    assert!(!r.loading_active());
    r.storage().clear_injections();
    r.exit();
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
}

// ---- R6: English text never touches the pack -----------------------------

fn pack_activity(card: &VirtualStorage) -> usize {
    card.read_log()
        .iter()
        .filter(|r| r.path.contains("FONTS"))
        .count()
        + card.open_count()
}

#[test]
fn opening_english_text_reads_and_opens_no_pack_even_with_packs_installed() {
    // positive control: the same card does hit the pack for CJK text
    let probe = rig(&cjk_book(), 0, WIDTH);
    assert!(
        pack_activity(probe.storage()) > 0,
        "the probe sees pack I/O"
    );

    // Latin-1, typographic quotes, dashes and ellipsis of the standard set
    let epub = standard()
        .into_iter()
        .find(|f| matches!(f.spec, Spec::Epub(_)))
        .expect("the standard set has an EPUB")
        .name;
    for name in ["UTF8.TXT", "PLAINLF.TXT", epub] {
        let card = standard_card();
        for px in cjk_support::SIZES {
            install(&card, px, &pack(px, false));
        }
        let mut r = Rig::new(card);
        r.configure(2, 0);
        r.open(name);
        assert_eq!(r.phase(), Phase::Ready, "{name}");
        r.idle(40);
        r.press(pulp_host::reader::Action::Next);
        assert_eq!(
            pack_activity(r.storage()),
            0,
            "{name}: English text reached the font pack"
        );
        assert_eq!(r.storage().open_count(), 0, "{name}: pack opened");
    }
}

//! Owned SD font preparation. Layout and draw borrow only prepared RAM data.
use super::{FontSet, Style, StyleState, bitmap::BitmapFont};
use crate::{
    drivers::{storage::SubdirFile, strip::StripBuffer},
    error::{Error, ErrorKind, Result},
    kernel::{BigBuf, BufClass, FONT_GLYPHS_PSRAM_BYTES, KernelHandle},
};
use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of;
use embedded_graphics::pixelcolor::BinaryColor;
use pulp_fontpack::{
    FontError, FontInfo, Glyph, GlyphRef, IndexCache, Metrics, PACK_DIR, PackReader, PageCache,
    PageGlyphSlot, ReadAt, bitmap_size, missing_glyph_metrics, pack_file_name,
};
use smol_epub::html_strip::{IMG_REF, MARKER};

pub const BODY_PIXELS: [u16; 5] = [16, 19, 23, 28, 35];
pub const HEADING_PIXELS: [u16; 5] = [23, 27, 32, 38, 46];
// Body state: persistent internal metrics <=16 KiB (growth can temporarily
// hold old and new vectors, <=32 KiB), two 700-slot tables (16,800 bytes each
// on the 32-bit boards), scratch
// <=4 KiB. Bitmap bytes use the explicit FontGlyphs board budget, in PSRAM on
// C61.
// Auxiliary surfaces are body role only: <=4 KiB metrics, 170 slots (~4 KiB)
// and <=32 KiB bitmaps each. Alive together with the reader are its title, its
// TOC and the manager overlay; Files, Home and Settings clear their surfaces
// on exit and suspend, so they never add to those three.
// Pack index caches (see `IndexBanks`) are heap, one per pixel size a state
// looks up in, at most `INDEX_BANKS` per state, allocated on first use and freed
// by `clear`: 4 KiB each for the reader, 508 bytes each for an auxiliary surface.
// Board allocation bound. The 64-bit host uses the same slot counts with
// larger pointer-sized metadata to simulate preparation, not board heap use.
pub const CJK_STORAGE_BUDGET: usize = 336 * 1024;
const METRIC_BUDGET: usize = 16 * 1024;
const SLOT_BUDGET: usize = 700 * size_of::<PageGlyphSlot>();
const BITMAP_BUDGET: usize = 64 * 1024;
const CACHE_BUDGET: usize = SLOT_BUDGET + BITMAP_BUDGET + size_of::<OwnedCache>();
const SCRATCH_BUDGET: usize = 4 * 1024;
const AUX_SURFACES: usize = 3;
const AUX_METRIC_BUDGET: usize = 4 * 1024;
// 170 slots is what 4 KiB holds with the 24-byte slot of the 32-bit targets;
// counting slots gives the 64-bit host the same ceiling.
const AUX_SLOT_BUDGET: usize = 170 * size_of::<PageGlyphSlot>();
const AUX_BITMAP_BUDGET: usize = 32 * 1024;
const AUX_CACHE_BUDGET: usize = AUX_SLOT_BUDGET + AUX_BITMAP_BUDGET + size_of::<OwnedCache>();
type OwnedCache = PageCache<Box<[PageGlyphSlot]>, BigBuf>;
type OwnedIndex = IndexCache<Box<[u32]>>;
// Levels of the pack index search kept in RAM. 10 levels is 1023 four-byte
// nodes; the stretch left below them (12665 records / 1024) is read in one go.
const INDEX_LEVELS: u32 = 10;
const AUX_INDEX_LEVELS: u32 = 7;
// body and heading pixel sizes
const INDEX_BANKS: usize = 2;
const INDEX_BUDGET: usize = INDEX_BANKS * OwnedIndex::nodes_for_levels(INDEX_LEVELS) * 4;
const AUX_INDEX_BUDGET: usize = INDEX_BANKS * OwnedIndex::nodes_for_levels(AUX_INDEX_LEVELS) * 4;
const _: () = {
    if size_of::<usize>() == 4 {
        assert!(size_of::<PageGlyphSlot>() == 24);
        assert!(
            2 * METRIC_BUDGET
                + 2 * CACHE_BUDGET
                + SCRATCH_BUDGET
                + INDEX_BUDGET
                + size_of::<CjkState>()
                + AUX_SURFACES
                    * (AUX_METRIC_BUDGET
                        + AUX_CACHE_BUDGET
                        + AUX_INDEX_BUDGET
                        + size_of::<CjkState>())
                <= CJK_STORAGE_BUDGET
        );
    }
};
const _: () =
    assert!(2 * BITMAP_BUDGET + AUX_SURFACES * AUX_BITMAP_BUDGET <= FONT_GLYPHS_PSRAM_BYTES);

fn failure(kind: ErrorKind) -> Error {
    Error::from_kind(kind).with_source("font")
}
fn font_error(e: FontError<Error>) -> Error {
    match e {
        FontError::Io(e) => e.with_source("font"),
        _ => failure(ErrorKind::InvalidData),
    }
}
fn buffer<T: Clone>(len: usize, value: T, budget: usize) -> Result<Box<[T]>> {
    if len.checked_mul(size_of::<T>()).is_none_or(|n| n > budget) {
        return Err(failure(ErrorKind::BufferTooSmall));
    }
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| failure(ErrorKind::OutOfMemory))?;
    if v.capacity() * size_of::<T>() > budget {
        return Err(failure(ErrorKind::BufferTooSmall));
    }
    v.resize(len, value);
    Ok(v.into_boxed_slice())
}
/// SD pack read counters of the `sd-metrics` measurement build. Without the
/// feature every method is an empty inline function: the default image keeps
/// no counters, no clock reads and no log lines.
#[cfg(feature = "sd-metrics")]
mod stats {
    use crate::kernel::uptime_us;

    pub type Tick = u64;
    // End of the previous log line (after logging), low 32 bits of the µs clock (load/store
    // only: the X4 core has no atomic read-modify-write)
    static LAST_END: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static LOG_US: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    pub(super) fn log_us() -> u32 {
        LOG_US.load(core::sync::atomic::Ordering::Relaxed)
    }
    pub struct Stats {
        began: u64,
        opens: u32,
        reads: u32,
        bytes: u32,
        read_us: u64,
        open_us: u64,
        max_us: u32,
        scalars: u32,
        lookups: u32,
    }
    impl Stats {
        pub fn new() -> Self {
            Self {
                began: uptime_us(),
                opens: 0,
                reads: 0,
                bytes: 0,
                read_us: 0,
                open_us: 0,
                max_us: 0,
                scalars: 0,
                lookups: 0,
            }
        }
        /// `since` is taken before the dir walk and file open that precede the closure.
        pub fn opened(&mut self, since: Tick) {
            self.opens += 1;
            self.open_us += uptime_us().saturating_sub(since);
        }
        pub fn tick(&self) -> Tick {
            uptime_us()
        }
        pub fn read(&mut self, since: Tick, bytes: usize) {
            let us = uptime_us().saturating_sub(since);
            self.reads += 1;
            self.bytes = self.bytes.saturating_add(bytes as u32);
            self.read_us += us;
            self.max_us = self.max_us.max(us.min(u64::from(u32::MAX)) as u32);
        }
        /// Distinct scalars this preparation needs.
        pub fn scalars(&mut self, n: usize) {
            self.scalars = self.scalars.saturating_add(n as u32);
        }
        /// Scalars looked up in the pack, as against kept from an earlier window.
        pub fn lookups(&mut self, n: usize) {
            self.lookups = self.lookups.saturating_add(n as u32);
        }
        pub fn log(&self, op: &str) {
            use core::sync::atomic::Ordering::Relaxed;
            // Time after the previous line finished logging, including reader
            // work and scheduler/display time; reader-sd separates those spans.
            let gap_us = (self.began as u32).wrapping_sub(LAST_END.load(Relaxed));
            let log_start = uptime_us();
            log::info!(
                "cjk-sd op={} gap_after_log_us={} opens={} reads={} bytes={} open_us={} read_us={} max_read_us={} wall_us={} scalars={} lookups={}",
                op,
                gap_us,
                self.opens,
                self.reads,
                self.bytes,
                self.open_us,
                self.read_us,
                self.max_us,
                log_start.saturating_sub(self.began),
                self.scalars,
                self.lookups,
            );
            let end = uptime_us();
            LOG_US.store(
                LOG_US
                    .load(Relaxed)
                    .wrapping_add(end.saturating_sub(log_start) as u32),
                Relaxed,
            );
            LAST_END.store(end as u32, Relaxed);
        }
    }
}
#[cfg(not(feature = "sd-metrics"))]
mod stats {
    pub type Tick = ();
    pub struct Stats;
    impl Stats {
        #[inline(always)]
        pub fn new() -> Self {
            Self
        }
        #[inline(always)]
        pub fn opened(&mut self, _since: Tick) {}
        #[inline(always)]
        pub fn tick(&self) -> Tick {}
        #[inline(always)]
        pub fn read(&mut self, _since: Tick, _bytes: usize) {}
        #[inline(always)]
        pub fn scalars(&mut self, _n: usize) {}
        #[inline(always)]
        pub fn lookups(&mut self, _n: usize) {}
        #[inline(always)]
        pub fn log(&self, _op: &str) {}
    }
}
#[cfg(feature = "sd-metrics")]
pub(crate) fn measurement_log_us() -> u32 {
    stats::log_us()
}

use stats::Stats;

enum Source<'a, 's> {
    // The pack file, opened once for the whole role (see `with_pack`).
    Installed {
        file: &'a mut SubdirFile<'s>,
        stats: &'a mut Stats,
    },
    // A validated zero-glyph pack drives the same pure missing-glyph cache
    // preparation as an installed pack that lacks a requested scalar.
    Empty([u8; pulp_fontpack::HEADER_LEN]),
}
impl ReadAt for Source<'_, '_> {
    type Error = Error;
    fn read_at(&mut self, offset: u64, mut buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Empty(header) => {
                let mut bytes = &header[..];
                bytes
                    .read_at(offset, buf)
                    .map_err(|_| failure(ErrorKind::InvalidData))
            }
            Self::Installed { file, stats } => {
                let mut at = u32::try_from(offset).map_err(|_| failure(ErrorKind::InvalidData))?;
                while !buf.is_empty() {
                    let began = stats.tick();
                    let n = file.read_at(at, buf)?;
                    stats.read(began, n);
                    if n == 0 || n > buf.len() {
                        return Err(failure(ErrorKind::ReadFailed));
                    }
                    at = at
                        .checked_add(n as u32)
                        .ok_or(failure(ErrorKind::InvalidData))?;
                    buf = &mut buf[n..];
                }
                Ok(())
            }
        }
    }
}
fn empty_header(px: u16) -> [u8; pulp_fontpack::HEADER_LEN] {
    let mut raw = [0; pulp_fontpack::HEADER_LEN];
    raw[..4].copy_from_slice(&pulp_fontpack::MAGIC);
    raw[4..6].copy_from_slice(&pulp_fontpack::FORMAT_VERSION.to_le_bytes());
    raw[6..8].copy_from_slice(&px.to_le_bytes());
    raw[16..18].copy_from_slice(&px.to_le_bytes());
    raw[18..20].copy_from_slice(&px.to_le_bytes());
    // With zero records and zero bitmap bytes, index, bitmap and EOF coincide.
    for offset in [24, 32, 40] {
        raw[offset..offset + 4].copy_from_slice(&(pulp_fontpack::HEADER_LEN as u32).to_le_bytes());
    }
    raw
}
/// Run `f` on a validated reader of the `px` bank. Every index probe and
/// bitmap read uses the same positioned file handle. Storage may retain that
/// handle after the call; its borrow and SD transactions end before returning.
/// `f` also learns whether a pack is installed. An absent pack is the validated
/// empty pack.
fn with_pack<T>(
    k: &mut KernelHandle<'_>,
    px: u16,
    stats: &mut Stats,
    f: impl FnOnce(&mut PackReader<Source<'_, '_>>, bool) -> Result<T>,
) -> Result<T> {
    let name = pack_file_name(px);
    let opening = stats.tick();
    k.with_app_subdir_file(PACK_DIR, name.as_str(), |file| {
        let (source, len, installed) = match file {
            Some(file) => {
                let len = file.len()?;
                stats.opened(opening);
                (Source::Installed { file, stats }, u64::from(len), true)
            }
            None => (
                Source::Empty(empty_header(px)),
                pulp_fontpack::HEADER_LEN as u64,
                false,
            ),
        };
        let mut reader = PackReader::open(source, len).map_err(font_error)?;
        if reader.info().pixel_size != px {
            return Err(failure(ErrorKind::InvalidData));
        }
        f(&mut reader, installed)
    })
    .map_err(|e| e.with_source("font"))
}
/// Validated installed bank identity; an absent optional bank has font ID zero.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BankIdentity {
    pub pixel_size: u16,
    pub font_id: u64,
    pub installed: bool,
}

pub fn bank_identity(k: &mut KernelHandle<'_>, px: u16) -> Result<BankIdentity> {
    let mut stats = Stats::new();
    let identity = with_pack(k, px, &mut stats, |reader, installed| {
        Ok(BankIdentity {
            pixel_size: px,
            font_id: reader.info().font_id,
            installed,
        })
    });
    stats.log("bank");
    identity
}

/// One scalar of a bank. Entries outlive the window that created them so a
/// repeated scalar is never looked up again; `NEEDED` marks the current window.
/// The scalar and its state bits share one word so an entry stays 20 bytes:
/// the metric budget counts entries.
#[derive(Clone, Copy)]
struct Entry {
    px: u16,
    metrics: Metrics,
    word: u32,
    // bitmap position in the pack, valid when FOUND
    bitmap_offset: u32,
}
const _: () = assert!(size_of::<Entry>() <= 20);
const CH_BITS: u32 = 0x1f_ffff;
const VISIBLE: u32 = 1 << 31;
const NEEDED: u32 = 1 << 30;
const RESOLVED: u32 = 1 << 29;
const FOUND: u32 = 1 << 28;
impl Entry {
    fn new(px: u16, ch: char) -> Self {
        Self {
            px,
            metrics: Metrics {
                advance: 0,
                offset_x: 0,
                offset_y: 0,
                width: 0,
                height: 0,
            },
            word: ch as u32 | NEEDED,
            bitmap_offset: 0,
        }
    }
    fn ch(&self) -> char {
        char::from_u32(self.word & CH_BITS).unwrap_or('\u{fffd}')
    }
    fn key(&self) -> (u16, char) {
        (self.px, self.ch())
    }
    fn has(&self, bit: u32) -> bool {
        self.word & bit != 0
    }
    fn needed(&self) -> bool {
        self.has(NEEDED)
    }
    fn visible(&self) -> bool {
        self.has(VISIBLE)
    }
    fn pending(&self) -> bool {
        self.needed() && !self.has(RESOLVED)
    }
    fn begin_window(&mut self) {
        self.word &= !(NEEDED | VISIBLE);
    }
    fn resolve(&mut self, found: Option<GlyphRef>, info: &FontInfo) {
        self.word |= RESOLVED;
        match found {
            Some(g) => {
                self.word |= FOUND;
                self.metrics = g.metrics;
                self.bitmap_offset = g.bitmap_offset();
            }
            None => self.metrics = missing_glyph_metrics(info),
        }
    }
    fn metrics(&self) -> Option<Metrics> {
        self.has(RESOLVED).then_some(self.metrics)
    }
    /// The earlier pack lookup, when there is one (`Some(None)`: absent).
    fn located(&self) -> Option<Option<GlyphRef>> {
        self.has(RESOLVED).then(|| {
            self.has(FOUND)
                .then(|| GlyphRef::from_located(self.metrics, self.bitmap_offset))
        })
    }
}

/// Mark `key` as needed by the current window, adding it when new. At the
/// ceiling the entries kept from earlier windows are dropped once and the add
/// retried, so a window that fit alone still fits.
fn note(metrics: &mut Vec<Entry>, budget: usize, key: (u16, char)) -> Result<()> {
    let mut purged = false;
    loop {
        let index = match metrics.binary_search_by_key(&key, Entry::key) {
            Ok(i) => {
                metrics[i].word |= NEEDED;
                return Ok(());
            }
            Err(index) => index,
        };
        if (metrics.len() + 1) * size_of::<Entry>() > budget {
            if purged || metrics.iter().all(Entry::needed) {
                return Err(failure(ErrorKind::BufferTooSmall));
            }
            metrics.retain(Entry::needed);
            purged = true;
            continue;
        }
        if metrics.len() == metrics.capacity() {
            let ceiling = budget / size_of::<Entry>();
            let capacity = metrics.capacity();
            let target = capacity.saturating_mul(2).max(8).min(ceiling);
            if metrics.try_reserve_exact(target - metrics.len()).is_err() {
                return Err(failure(ErrorKind::OutOfMemory));
            }
            if metrics.capacity() * size_of::<Entry>() > budget {
                return Err(failure(ErrorKind::BufferTooSmall));
            }
        }
        metrics.insert(index, Entry::new(key.0, key.1));
        return Ok(());
    }
}

/// Time slicing of a preparation that can run for seconds. The reader arms
/// a slice before each background step; staging and bitmap reading then stop
/// after the glyph that crosses its end and return `SLICE_END`, with everything
/// done so far kept. The same call again resumes, and the finished result is
/// the one an unsliced call produces. With no slice armed nothing is cut off.
///
/// A pause always falls between two glyphs, outside `with_pack`: its storage
/// borrow and SD transactions have ended when the caller gets control back.
/// A retained file handle does not hold the SPI bus, so polling keys or
/// refreshing the panel cannot overlap a font read.
pub const SLICE_US: u32 = 60_000;
/// A paused preparation logs its progress at least this often.
const HEARTBEAT_US: u64 = 4_000_000;
const SLICE_END_TAG: &str = "cjk slice";
const SLICE_END: Error = Error::new(ErrorKind::Other, SLICE_END_TAG);
/// Whether `e` is the end of a slice rather than a failure.
pub fn is_slice_end(e: &Error) -> bool {
    e.kind() == ErrorKind::Other && e.source_tag() == SLICE_END_TAG
}
fn clock_us() -> u64 {
    crate::kernel::uptime_us()
}

#[derive(Clone, Copy)]
struct Pace {
    now: fn() -> u64,
    slice_us: u32,
    // a slice is armed; it starts counting at the first glyph of the step, so
    // the page read and other work before it do not use up its time
    armed: bool,
    began: Option<u64>,
    beat_at: u64,
    // of the paused phase: glyphs done and wanted, microseconds still to go
    progress: (u32, u32),
    left_us: u64,
}
impl Pace {
    const fn new() -> Self {
        Self {
            now: clock_us,
            slice_us: SLICE_US,
            armed: false,
            began: None,
            beat_at: 0,
            progress: (0, 0),
            left_us: 0,
        }
    }
    fn start(&mut self) {
        if self.armed && self.began.is_none() {
            self.began = Some((self.now)());
        }
    }
    fn expired(&self) -> bool {
        self.began
            .is_some_and(|b| (self.now)().saturating_sub(b) >= u64::from(self.slice_us))
    }
    /// End the slice: record how far the phase is, log a heartbeat when one is
    /// due, and hand back the error that tells the caller to come again.
    fn pause(&mut self, phase: &str, did: usize, left: usize, total: usize) -> Error {
        let now = (self.now)();
        let per_glyph = now.saturating_sub(self.began.unwrap_or(now)) / did.max(1) as u64;
        self.left_us = per_glyph.saturating_mul(left as u64);
        self.progress = (total.saturating_sub(left) as u32, total as u32);
        if now >= self.beat_at {
            log::info!(
                "cjk: {} {}/{} glyphs, about {} ms left",
                phase,
                self.progress.0,
                self.progress.1,
                self.left_us / 1000
            );
            self.beat_at = now.saturating_add(HEARTBEAT_US);
        }
        SLICE_END
    }
}

/// The pack index caches of one state, keyed by pixel size. A cache outlives the
/// slices of a preparation (the reader's `with_pack` calls) and everything else
/// until `clear`. It also checks the pack it is used with (font ID, pixel size,
/// record count, bitmap size) and empties itself when that differs. Without
/// memory for one, lookups search the pack file the plain way.
struct IndexBanks {
    banks: [Option<(u16, OwnedIndex)>; INDEX_BANKS],
    next: usize,
}
impl IndexBanks {
    const fn new() -> Self {
        Self {
            banks: [None, None],
            next: 0,
        }
    }
    fn bank(&mut self, px: u16, levels: u32) -> Option<&mut OwnedIndex> {
        let at = match self
            .banks
            .iter()
            .position(|b| b.as_ref().is_some_and(|(p, _)| *p == px))
        {
            Some(at) => at,
            None => {
                let at = self
                    .banks
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(self.next);
                self.next = (at + 1) % INDEX_BANKS;
                // release a replaced bank first, avoiding an old+new peak
                self.banks[at] = None;
                let words = OwnedIndex::nodes_for_levels(levels);
                let nodes = buffer(words, 0u32, words * size_of::<u32>()).ok()?;
                self.banks[at] = Some((px, IndexCache::new(nodes)));
                at
            }
        };
        self.banks[at].as_mut().map(|(_, cache)| cache)
    }
}

pub struct CjkState {
    metrics: Vec<Entry>,
    index: IndexBanks,
    index_levels: u32,
    body: Option<(u16, OwnedCache)>,
    heading: Option<(u16, OwnedCache)>,
    // The roles are drawable only when a whole preparation has finished: a paused
    // one leaves a finished body role beside an unfinished heading role.
    ready: bool,
    pace: Pace,
    heading_role: bool,
    metric_budget: usize,
    slot_budget: usize,
    bitmap_budget: usize,
}
impl CjkState {
    pub const fn new() -> Self {
        Self {
            metrics: Vec::new(),
            index: IndexBanks::new(),
            index_levels: INDEX_LEVELS,
            body: None,
            heading: None,
            ready: false,
            pace: Pace::new(),
            heading_role: true,
            metric_budget: METRIC_BUDGET,
            slot_budget: SLOT_BUDGET,
            bitmap_budget: BITMAP_BUDGET,
        }
    }
    /// Auxiliary visible labels have smaller, independent allocation ceilings
    /// and prepare the body role only.
    pub const fn auxiliary() -> Self {
        Self {
            metrics: Vec::new(),
            index: IndexBanks::new(),
            index_levels: AUX_INDEX_LEVELS,
            body: None,
            heading: None,
            ready: false,
            pace: Pace::new(),
            heading_role: false,
            metric_budget: AUX_METRIC_BUDGET,
            slot_budget: AUX_SLOT_BUDGET,
            bitmap_budget: AUX_BITMAP_BUDGET,
        }
    }
    pub fn clear(&mut self) {
        self.metrics = Vec::new();
        self.index = IndexBanks::new();
        self.body = None;
        self.heading = None;
        self.ready = false;
    }
    /// Start a slice (see `SLICE_US`). Until `disarm_slice`, staging and
    /// `prepare_visible` may return the slice-end error (`is_slice_end`).
    pub fn arm_slice(&mut self) {
        self.pace.armed = true;
        self.pace.began = None;
    }
    pub fn disarm_slice(&mut self) {
        self.pace.armed = false;
        self.pace.began = None;
    }
    /// Another clock and slice length, for tests that need a slice to end at
    /// a chosen glyph.
    pub fn set_pace(&mut self, now: fn() -> u64, slice_us: u32) {
        self.pace.now = now;
        self.pace.slice_us = slice_us;
    }
    /// The clock the slices run on, in microseconds.
    pub fn now_us(&self) -> u64 {
        (self.pace.now)()
    }
    /// Glyphs done and wanted by the phase the last slice ended in.
    pub fn progress(&self) -> (u32, u32) {
        self.pace.progress
    }
    /// What the last slice's rate says the phase still needs, in microseconds.
    pub fn remaining_us(&self) -> u64 {
        self.pace.left_us
    }
    pub fn stage_metrics(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.stage_metrics_with_flags(k, text, latin, body_px, heading_px, 0)
    }
    /// Stage a window whose opening style markers occurred on an earlier page.
    /// Bits 0, 1 and 2 are bold, italic and heading respectively.
    pub fn stage_metrics_with_flags(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
        initial_flags: u8,
    ) -> Result<()> {
        // Entries from earlier windows stay (resolved, so never looked up
        // again) but no longer count as part of this window.
        for e in &mut self.metrics {
            e.begin_window();
        }
        let mut collection = Ok(());
        scan(text, initial_flags, |ch, sty| {
            if collection.is_ok() && needs(latin, ch, sty) {
                let key = (pixel_size(sty, body_px, heading_px), ch);
                collection = note(&mut self.metrics, self.metric_budget, key);
            }
        });
        collection?;
        let mut stats = Stats::new();
        #[cfg(feature = "sd-metrics")]
        stats.scalars(self.metrics.iter().filter(|e| e.needed()).count());
        let result = self.resolve_pending(k, &mut stats);
        stats.log("stage");
        result
    }
    /// Look every not yet resolved scalar of the window up, one pack open per
    /// pixel size (and per slice). Ends the slice, when one is armed and runs
    /// out, after the scalar that crossed it.
    fn resolve_pending(&mut self, k: &mut KernelHandle<'_>, stats: &mut Stats) -> Result<()> {
        let total = self.metrics.iter().filter(|e| e.needed()).count();
        let mut left = self.metrics.iter().filter(|e| e.pending()).count();
        let mut did = 0;
        let mut start = 0;
        self.pace.start();
        while start < self.metrics.len() {
            let px = self.metrics[start].px;
            let end = start
                + self.metrics[start..]
                    .iter()
                    .take_while(|e| e.px == px)
                    .count();
            let group = &mut self.metrics[start..end];
            start = end;
            if !group.iter().any(Entry::pending) {
                continue;
            }
            let pace = &self.pace;
            let (index, levels) = (&mut self.index, self.index_levels);
            let (looked_up, paused) = with_pack(k, px, stats, |reader, installed| {
                let info = reader.info();
                // a pack that is not installed has no index to cache
                let mut cache = installed.then(|| index.bank(px, levels)).flatten();
                let mut n = 0;
                for e in group.iter_mut().filter(|e| e.pending()) {
                    let found = match cache.as_deref_mut() {
                        Some(cache) => reader.find_cached(e.ch(), cache),
                        None => reader.find(e.ch()),
                    }
                    .map_err(font_error)?;
                    e.resolve(found, &info);
                    n += 1;
                    if n < left && pace.expired() {
                        return Ok((n, true));
                    }
                }
                Ok((n, false))
            })?;
            stats.lookups(looked_up);
            left -= looked_up;
            did += looked_up;
            if paused {
                return Err(self.pace.pause("lookup", did, left, total));
            }
        }
        Ok(())
    }
    pub fn begin_visible(&mut self) {
        for e in &mut self.metrics {
            e.word &= !VISIBLE;
        }
    }
    pub fn mark_visible(
        &mut self,
        text: &[u8],
        initial: Style,
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.mark_visible_with_flags(text, style_flags(initial), latin, body_px, heading_px)
    }
    pub fn mark_visible_with_flags(
        &mut self,
        text: &[u8],
        initial_flags: u8,
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        let mut valid = true;
        scan(text, initial_flags, |ch, sty| {
            if needs(latin, ch, sty) {
                let key = (pixel_size(sty, body_px, heading_px), ch);
                match self.metrics.binary_search_by_key(&key, Entry::key) {
                    Ok(i) if self.metrics[i].needed() => self.metrics[i].word |= VISIBLE,
                    _ => valid = false,
                }
            }
        });
        if valid {
            Ok(())
        } else {
            Err(failure(ErrorKind::InvalidData))
        }
    }
    pub fn prepare_visible(
        &mut self,
        k: &mut KernelHandle<'_>,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        let mut stats = Stats::new();
        let result = self.prepare_roles(k, body_px, heading_px, &mut stats);
        stats.log("prepare");
        result
    }
    fn prepare_roles(
        &mut self,
        k: &mut KernelHandle<'_>,
        body_px: u16,
        heading_px: u16,
        stats: &mut Stats,
    ) -> Result<()> {
        // Unpublish both roles before any fallible work; errors cannot expose
        // either an old page or an otherwise successful partial preparation.
        self.ready = false;
        let result = self.fill_roles(k, body_px, heading_px, stats);
        match &result {
            Ok(()) => self.ready = true,
            // paused: the unfinished cache is kept and stays unpublished
            Err(e) if is_slice_end(e) => {}
            Err(_) => {
                self.body = None;
                self.heading = None;
            }
        }
        result
    }
    fn fill_roles(
        &mut self,
        k: &mut KernelHandle<'_>,
        body_px: u16,
        heading_px: u16,
        stats: &mut Stats,
    ) -> Result<()> {
        self.pace.start();
        for (role, px) in [(0, body_px), (1, heading_px)] {
            let slot = if role == 0 {
                &mut self.body
            } else {
                &mut self.heading
            };
            if role == 1 && (px == body_px || !self.heading_role) {
                *slot = None;
                continue;
            }
            let count = self
                .metrics
                .iter()
                .filter(|e| e.px == px && e.visible())
                .count();
            if count == 0 {
                *slot = None;
                continue;
            }
            let mut chars = buffer(count, '\0', SCRATCH_BUDGET)?;
            let mut bitmap_len = 0usize;
            for (i, e) in self
                .metrics
                .iter()
                .filter(|e| e.px == px && e.visible())
                .enumerate()
            {
                chars[i] = e.ch();
                let m = e.metrics().ok_or(failure(ErrorKind::InvalidData))?;
                bitmap_len = bitmap_len
                    .checked_add(bitmap_size(m.width, m.height) as usize)
                    .ok_or(failure(ErrorKind::BufferTooSmall))?;
            }
            let needed = size_of::<OwnedCache>() + count * size_of::<PageGlyphSlot>() + bitmap_len;
            let cache_budget = self.slot_budget + self.bitmap_budget + size_of::<OwnedCache>();
            if needed > cache_budget || bitmap_len > self.bitmap_budget {
                return Err(failure(ErrorKind::BufferTooSmall));
            }
            stats.scalars(count);
            // The same scalars as the page still held: no glyph is read again.
            // The pack header is still validated; clearing for a bank identity
            // change remains the caller's lifecycle responsibility.
            if let Some((_, cache)) = slot.as_ref().filter(|(p, _)| *p == px)
                && cache.len() == count
                && chars.iter().all(|&c| cache.get(c).is_some())
            {
                with_pack(k, px, stats, |_, _| Ok(()))?;
                continue;
            }
            // A paused preparation of this very list continues where it stopped;
            // anything else starts over in the storage of the page it replaces.
            let mut cache = match slot.take() {
                Some((p, cache)) if p == px && cache.resumes(&chars) => cache,
                previous => {
                    let (mut slots, mut bitmaps) = previous
                        .map(|(_, cache)| cache.into_storage())
                        .unwrap_or_else(|| (Box::default(), BigBuf::empty()));
                    if slots.len() < count {
                        // Release replaced storage first, avoiding an old+new peak.
                        drop(slots);
                        slots = buffer(count, PageGlyphSlot::default(), self.slot_budget)?;
                    }
                    if bitmaps.len() < bitmap_len {
                        drop(bitmaps);
                        bitmaps = BigBuf::zeroed(BufClass::FontGlyphs, bitmap_len)
                            .map_err(|_| failure(ErrorKind::OutOfMemory))?;
                    }
                    if bitmaps.len() > self.bitmap_budget {
                        return Err(failure(ErrorKind::BufferTooSmall));
                    }
                    let mut cache = PageCache::new(slots, bitmaps, cache_budget)
                        .map_err(|_| failure(ErrorKind::BufferTooSmall))?;
                    cache.begin();
                    cache
                }
            };
            let first = cache.filled();
            // Staging already located these scalars; only bitmaps are read.
            let (metrics, pace) = (&self.metrics, &self.pace);
            let finished = with_pack(k, px, stats, |reader, _| {
                cache
                    .resume_located(
                        reader,
                        &chars,
                        |ch| {
                            let i = metrics.binary_search_by_key(&(px, ch), Entry::key).ok()?;
                            metrics[i].located()
                        },
                        || !pace.expired(),
                    )
                    .map_err(|e| match e {
                        pulp_fontpack::PreparationError::Font(e) => font_error(e),
                        _ => failure(ErrorKind::BufferTooSmall),
                    })
            })?;
            let (done, did) = (cache.filled(), cache.filled() - first);
            *slot = Some((px, cache));
            if !finished {
                return Err(self.pace.pause("bitmap", did, count - done, count));
            }
        }
        Ok(())
    }
    pub fn prepare_text(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.stage_metrics(k, text, latin, body_px, heading_px)?;
        self.mark_visible(text, Style::Regular, latin, body_px, heading_px)?;
        self.prepare_visible(k, body_px, heading_px)
    }
    pub fn view(&self, latin: FontSet, body_px: u16, heading_px: u16) -> LayoutFonts<'_> {
        LayoutFonts {
            latin,
            state: self,
            body_px,
            heading_px,
        }
    }
    pub fn get(&self, px: u16, ch: char) -> Option<Glyph<'_>> {
        if !self.ready {
            return None;
        }
        self.body
            .iter()
            .chain(self.heading.iter())
            .find(|(size, _)| *size == px)?
            .1
            .get(ch)
    }
    pub fn has_fallback(&self) -> bool {
        self.metrics.iter().any(Entry::needed)
    }
    /// Whether the staged layout window depends on this fallback bank.
    pub fn uses_bank(&self, px: u16) -> bool {
        self.metrics.iter().any(|e| e.px == px && e.needed())
    }
}
impl Default for CjkState {
    fn default() -> Self {
        Self::new()
    }
}
pub struct LayoutFonts<'a> {
    latin: FontSet,
    state: &'a CjkState,
    body_px: u16,
    heading_px: u16,
}
pub type PreparedFonts<'a> = LayoutFonts<'a>;
impl LayoutFonts<'_> {
    pub fn font(&self, sty: Style) -> &'static BitmapFont {
        self.latin.font(sty)
    }
    pub fn line_height(&self, sty: Style) -> u16 {
        if super::font_data::HAS_REGULAR {
            self.latin.line_height(sty)
        } else {
            18
        }
    }
    pub fn has_fallback(&self) -> bool {
        self.state.has_fallback()
    }
    pub fn advance(&self, ch: char, sty: Style) -> u16 {
        if !needs(self.latin, ch, sty) {
            return if super::font_data::HAS_REGULAR {
                self.latin.advance(ch, sty).into()
            } else {
                9
            };
        }
        let key = (pixel_size(sty, self.body_px, self.heading_px), ch);
        self.state
            .metrics
            .binary_search_by_key(&key, Entry::key)
            .ok()
            .and_then(|i| {
                Some(&self.state.metrics[i])
                    .filter(|e| e.needed())?
                    .metrics()
            })
            .unwrap_or_else(|| missing_glyph_metrics(&fallback_info(key.0)))
            .advance
    }

    pub fn draw_char(
        &self,
        s: &mut StripBuffer,
        ch: char,
        sty: Style,
        x: i32,
        baseline: i32,
    ) -> u16 {
        self.draw_char_fg(s, ch, sty, BinaryColor::On, x, baseline)
    }
    pub fn draw_char_fg(
        &self,
        s: &mut StripBuffer,
        ch: char,
        sty: Style,
        fg: BinaryColor,
        x: i32,
        baseline: i32,
    ) -> u16 {
        if !needs(self.latin, ch, sty) {
            if !super::font_data::HAS_REGULAR {
                use embedded_graphics::{
                    mono_font::{MonoTextStyle, ascii::FONT_9X18},
                    prelude::*,
                    text::Text,
                };
                let mut bytes = [0; 4];
                let style = MonoTextStyle::new(&FONT_9X18, fg);
                let _ =
                    Text::new(ch.encode_utf8(&mut bytes), Point::new(x, baseline), style).draw(s);
                return 9;
            }
            return self
                .latin
                .font(sty)
                .draw_char_fg(s, ch, fg, x, baseline)
                .into();
        }
        let px = pixel_size(sty, self.body_px, self.heading_px);
        let Some(g) = self.state.get(px, ch) else {
            // An invariant failure remains drawable without reaching a source.
            // Preparation validates visible keys before the reader publishes Ready.
            use embedded_graphics::{
                prelude::*,
                primitives::{PrimitiveStyle, Rectangle},
            };
            let m = missing_glyph_metrics(&fallback_info(px));
            let _ = Rectangle::new(
                Point::new(x + i32::from(m.offset_x), baseline + i32::from(m.offset_y)),
                Size::new(m.width.into(), m.height.into()),
            )
            .into_styled(PrimitiveStyle::with_stroke(fg, 1))
            .draw(s);
            return m.advance;
        };
        let m = g.metrics;
        s.blit_1bpp(
            g.bitmap,
            0,
            m.width.into(),
            m.height.into(),
            usize::from(m.width).div_ceil(8),
            x + i32::from(m.offset_x),
            baseline + i32::from(m.offset_y),
            fg == BinaryColor::On,
        );
        m.advance
    }
}
fn fallback_info(pixel_size: u16) -> pulp_fontpack::FontInfo {
    pulp_fontpack::FontInfo {
        pixel_size,
        font_id: 0,
        line_height: pixel_size,
        ascent: pixel_size,
    }
}
fn pixel_size(sty: Style, body: u16, heading: u16) -> u16 {
    if sty == Style::Heading { heading } else { body }
}
fn needs(latin: FontSet, ch: char, sty: Style) -> bool {
    ch >= ' '
        && ch != '\u{fffd}'
        && ch != '\u{ad}'
        && ch != '\u{a0}'
        && !latin.font(sty).has_glyph(ch)
}
fn style_flags(style: Style) -> u8 {
    match style {
        Style::Regular => 0,
        Style::Bold => 1,
        Style::Italic => 2,
        Style::Heading => 4,
    }
}
fn scan(text: &[u8], initial_flags: u8, mut f: impl FnMut(char, Style)) {
    let mut styles = StyleState::from_flags(initial_flags);
    let mut i = 0;
    while i < text.len() {
        if text[i] == MARKER && i + 1 < text.len() {
            if text[i + 1] == IMG_REF
                && i + 2 < text.len()
                && i + 3 + text[i + 2] as usize <= text.len()
            {
                i += 3 + text[i + 2] as usize;
                continue;
            }
            styles.apply_marker(text[i + 1]);
            i += 2;
            continue;
        }
        let (ch, n) = pulp_kernel::util::decode_utf8_char(text, i);
        f(ch, styles.style());
        i += n;
    }
}

/// Temporary, bounded collection of unsupported visible label scalars.
/// Latin glyphs never cause font storage work.
pub struct VisibleText {
    bytes: Vec<u8>,
    error: Option<Error>,
}
impl VisibleText {
    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            error: None,
        }
    }
    pub fn add(&mut self, text: &str, font: &'static BitmapFont, heading: bool) {
        use smol_epub::html_strip::{HEADING_OFF, HEADING_ON};
        let mut marked = false;
        for ch in text.chars().filter(|&ch| ch >= ' ' && !font.has_glyph(ch)) {
            if !marked {
                self.append(&[MARKER, if heading { HEADING_ON } else { HEADING_OFF }]);
                marked = true;
            }
            let mut raw = [0; 4];
            self.append(ch.encode_utf8(&mut raw).as_bytes());
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        if self.bytes.len() + bytes.len() > 4096 {
            self.error = Some(failure(ErrorKind::BufferTooSmall));
            return;
        }
        if self.bytes.try_reserve_exact(bytes.len()).is_err() {
            self.error = Some(failure(ErrorKind::OutOfMemory));
            return;
        }
        if self.bytes.capacity() > 4096 {
            self.error = Some(failure(ErrorKind::BufferTooSmall));
            return;
        }
        self.bytes.extend_from_slice(bytes);
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        if let Some(e) = self.error {
            Err(e)
        } else {
            Ok(&self.bytes)
        }
    }
}

/// One app's bounded label caches; reset on exit/suspend, never shared with
/// background work in another app. Preparation errors remain drawable.
pub struct SurfaceFonts {
    state: CjkState,
    size: u8,
    // (size, length, FNV-1a) of the last successful prepare. A collision can
    // only keep a stale glyph set drawable as missing-glyph boxes; draw never
    // reaches storage and indexes only prepared data.
    prepared: Option<(u8, usize, u32)>,
    pub error: Option<Error>,
}
impl SurfaceFonts {
    pub const fn new() -> Self {
        Self {
            state: CjkState::auxiliary(),
            size: 0,
            prepared: None,
            error: None,
        }
    }
    pub fn set_size(&mut self, idx: u8) {
        self.size = if idx < 5 { idx } else { 1 };
    }
    pub fn clear(&mut self) {
        self.state.clear();
        self.prepared = None;
        self.error = None;
    }
    pub fn prepare(&mut self, k: &mut KernelHandle<'_>, text: &VisibleText) {
        let idx = usize::from(self.size);
        let key = text
            .bytes()
            .ok()
            .map(|bytes| (self.size, bytes.len(), smol_epub::cache::fnv1a(bytes)));
        if key.is_some() && key == self.prepared && self.error.is_none() {
            return;
        }
        let result = text.bytes().and_then(|bytes| {
            self.state.prepare_text(
                k,
                bytes,
                FontSet::for_size(self.size),
                BODY_PIXELS[idx],
                HEADING_PIXELS[idx],
            )
        });
        self.error = result.err();
        if self.error.is_some() {
            self.state.clear();
        }
        self.prepared = key.filter(|_| self.error.is_none());
    }
    pub fn view(&self) -> PreparedFonts<'_> {
        let idx = usize::from(self.size);
        self.state.view(
            FontSet::for_size(self.size),
            BODY_PIXELS[idx],
            HEADING_PIXELS[idx],
        )
    }
}

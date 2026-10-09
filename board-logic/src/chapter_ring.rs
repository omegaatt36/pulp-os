// Previous / current / next chapter working set, with a byte budget.
//
// HAL-free and allocation-free: the ring owns opaque payloads (`T` is a
// `BigBuf` in the firmware, a `Vec<u8>` in the tests) and only does the
// accounting, the identity checks and the eviction policy.
//
// Identity. A slot is found by its whole `ChapterKey`: the persistent
// `SourceId` of the book, the chapter number and the position and size of the
// chapter text in the book's cache file. Two chapters of the same size, the same
// chapter of a replaced book, or a rebuilt cache file with other offsets never
// alias; looking up by length alone (what the single-chapter buffer did) cannot
// promise that.
//
// Window. The ring keeps the chapters `current - 1 ..= current + 1` of one
// source (only `current` when neighbors are disabled). `retarget` evicts what
// left the window, so moving to the next chapter keeps the old current one as
// the new previous chapter and, if it was warmed, finds the new current chapter
// resident.
//
// Pending work. A neighbor is read in bounded steps by a `WarmJob` and
// published only when every byte arrived. The ring carries a transient
// `generation` that changes whenever the window moves or the source changes; a
// job remembers the generation it began in, so a job that finishes after the
// reader navigated, changed book or reset publishes nothing.

use crate::memory::{KIB, MIB, PSRAM_LARGE_CHAPTER_TEXT_BYTES, PSRAM_LARGE_MIN_BYTES, PsramStatus};
use crate::source_id::SourceId;

/// Largest chapter text kept in RAM on X4, HR2 and degraded builds (the size
/// the single-chapter buffer always had).
pub const SMALL_SINGLE_MAX_BYTES: usize = 98_304;
/// HR8: largest chapter text of a slot.
pub const HR8_SINGLE_MAX_BYTES: usize = 256 * KIB;
/// HR8: three full slots. The rest of the 1 MiB chapter class is headroom for
/// the inflate window, the strip buffers and the page prefetch buffer.
pub const HR8_BUDGET_BYTES: usize = 3 * HR8_SINGLE_MAX_BYTES;

const _: () = assert!(HR8_SINGLE_MAX_BYTES == 256 * KIB);
const _: () = assert!(HR8_BUDGET_BYTES + 256 * KIB <= PSRAM_LARGE_CHAPTER_TEXT_BYTES);
const _: () = assert!(PSRAM_LARGE_CHAPTER_TEXT_BYTES == MIB);

/// Bytes one background tick reads of a neighbor chapter.
pub const WARM_CHUNK_BYTES: usize = 16 * KIB;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RingConfig {
    /// Sum of the resident chapter texts.
    pub budget: usize,
    /// Largest single chapter text; a bigger chapter streams from the card.
    pub single_max: usize,
    /// Keep (and warm) the previous and next chapter.
    pub neighbors: bool,
}

impl RingConfig {
    pub const SMALL: RingConfig = RingConfig {
        budget: SMALL_SINGLE_MAX_BYTES,
        single_max: SMALL_SINGLE_MAX_BYTES,
        neighbors: false,
    };
    pub const HR8: RingConfig = RingConfig {
        budget: HR8_BUDGET_BYTES,
        single_max: HR8_SINGLE_MAX_BYTES,
        neighbors: true,
    };

    /// HR8 only on a validated part of at least `PSRAM_LARGE_MIN_BYTES`.
    pub const fn for_status(status: PsramStatus) -> RingConfig {
        match status {
            PsramStatus::Ready { bytes } if bytes >= PSRAM_LARGE_MIN_BYTES => Self::HR8,
            _ => Self::SMALL,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChapterKey {
    pub source: SourceId,
    pub chapter: u16,
    /// Offset of the chapter text in the book's cache file.
    pub offset: u32,
    /// Byte size of the chapter text.
    pub size: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A zero-sized chapter has nothing to keep.
    Empty,
    /// Larger than `single_max`.
    TooLarge,
    /// Does not fit next to the chapters that must stay.
    OverBudget,
    /// Not one of the three chapters around the current one.
    OutOfWindow,
    /// A neighbor while `neighbors` is off.
    NeighborsDisabled,
    /// Key of another book than the ring is aimed at.
    WrongSource,
    /// A warm job that began before the window moved or the source changed.
    Stale,
    /// A warm job that has not read every byte.
    Incomplete,
}

struct Slot<T> {
    key: ChapterKey,
    value: T,
}

pub struct ChapterRing<T> {
    cfg: RingConfig,
    source: SourceId,
    current: u16,
    generation: u32,
    used: usize,
    slots: [Option<Slot<T>>; 3],
}

impl<T> ChapterRing<T> {
    pub const fn new(cfg: RingConfig) -> Self {
        Self {
            cfg,
            source: SourceId::NONE,
            current: 0,
            generation: 0,
            used: 0,
            slots: [const { None }; 3],
        }
    }

    pub const fn config(&self) -> RingConfig {
        self.cfg
    }

    /// Transient: changes whenever the window moves, the source changes or the
    /// ring is cleared. Never stored.
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    pub const fn used(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn source(&self) -> SourceId {
        self.source
    }

    pub const fn current(&self) -> u16 {
        self.current
    }

    fn evict_where(&mut self, mut gone: impl FnMut(&ChapterKey) -> bool) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|s| gone(&s.key)) {
                let s = slot.take().expect("checked");
                self.used -= s.key.size as usize;
            }
        }
    }

    fn in_window(&self, chapter: u16) -> bool {
        if self.cfg.neighbors {
            chapter.abs_diff(self.current) <= 1
        } else {
            chapter == self.current
        }
    }

    /// Aim the ring at `chapter` of `source`. A new source drops everything; a
    /// new chapter drops what left the window. Either bumps the generation, so
    /// work that was pending for the old aim cannot publish.
    pub fn retarget(&mut self, source: SourceId, chapter: u16) {
        if source != self.source {
            self.clear();
            self.source = source;
            self.current = chapter;
            return;
        }
        if chapter != self.current {
            self.current = chapter;
            self.generation = self.generation.wrapping_add(1);
            let (neighbors, current) = (self.cfg.neighbors, chapter);
            self.evict_where(|k| {
                if neighbors {
                    k.chapter.abs_diff(current) > 1
                } else {
                    k.chapter != current
                }
            });
        }
    }

    /// Drop every chapter and forget the source.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.used = 0;
        self.source = SourceId::NONE;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether `key` could be kept now (without evicting a chapter that has to
    /// stay).
    pub fn admit(&self, key: &ChapterKey) -> Result<(), Refusal> {
        self.check(key, false)
    }

    fn check(&self, key: &ChapterKey, evict_neighbors: bool) -> Result<(), Refusal> {
        if key.source != self.source || self.source.is_none() {
            return Err(Refusal::WrongSource);
        }
        if key.size == 0 {
            return Err(Refusal::Empty);
        }
        let size = key.size as usize;
        if size > self.cfg.single_max {
            return Err(Refusal::TooLarge);
        }
        if !self.in_window(key.chapter) {
            return Err(if self.cfg.neighbors {
                Refusal::OutOfWindow
            } else {
                Refusal::NeighborsDisabled
            });
        }
        // bytes that stay: everything but the slot this key replaces and, for
        // the current chapter, the neighbors it may push out
        let current = key.chapter == self.current;
        let stays: usize = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.key.chapter != key.chapter)
            .filter(|_| !(evict_neighbors && current))
            .map(|s| s.key.size as usize)
            .sum();
        if stays + size > self.cfg.budget {
            return Err(Refusal::OverBudget);
        }
        Ok(())
    }

    pub fn get(&self, key: &ChapterKey) -> Option<&T> {
        self.slots
            .iter()
            .flatten()
            .find(|s| s.key == *key)
            .map(|s| &s.value)
    }

    pub fn contains(&self, key: &ChapterKey) -> bool {
        self.get(key).is_some()
    }

    /// Resident chapter numbers, for diagnostics and tests.
    pub fn resident(&self) -> [Option<u16>; 3] {
        let mut out = [None; 3];
        for (o, s) in out.iter_mut().zip(self.slots.iter()) {
            *o = s.as_ref().map(|s| s.key.chapter);
        }
        out
    }

    /// Keep `value` as the text of `key`. A resident chapter of the same number
    /// (another key: the cache was rebuilt) is replaced; the current chapter
    /// pushes neighbors out if it has to, a neighbor never evicts anything.
    pub fn insert(&mut self, key: ChapterKey, value: T) -> Result<(), (Refusal, T)> {
        if let Err(r) = self.check(&key, true) {
            return Err((r, value));
        }
        let current = key.chapter == self.current;
        let replaced = key.chapter;
        self.evict_where(|k| k.chapter == replaced);
        if self.used + key.size as usize > self.cfg.budget && current {
            // the current chapter outranks the neighbors; farthest first would
            // need distances, with one slot each the order is next, then previous
            let cur = self.current;
            self.evict_where(|k| k.chapter > cur);
            if self.used + key.size as usize > self.cfg.budget {
                self.evict_where(|k| k.chapter < cur);
            }
        }
        debug_assert!(self.used + key.size as usize <= self.cfg.budget);
        let free = self
            .slots
            .iter()
            .position(Option::is_none)
            .expect("three slots for a three chapter window");
        self.used += key.size as usize;
        self.slots[free] = Some(Slot { key, value });
        Ok(())
    }

    /// Take a chapter out (the caller wants to own or mutate it).
    pub fn take(&mut self, key: &ChapterKey) -> Option<T> {
        let i = self
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.key == *key))?;
        let s = self.slots[i].take().expect("positioned");
        self.used -= s.key.size as usize;
        Some(s.value)
    }

    /// The next neighbor worth reading in the background: the next chapter
    /// first (forward reading is the common direction), then the previous one.
    /// `lookup(chapter)` gives the key of a chapter, or `None` when it is not
    /// in the cache yet. Chapters that are resident, empty, too large or do not
    /// fit are skipped.
    pub fn next_warm(
        &self,
        chapter_count: u16,
        mut lookup: impl FnMut(u16) -> Option<ChapterKey>,
    ) -> Option<WarmJob> {
        if !self.cfg.neighbors || self.source.is_none() {
            return None;
        }
        let cur = self.current;
        let candidates = [cur.checked_add(1), cur.checked_sub(1)];
        for ch in candidates.into_iter().flatten() {
            if ch >= chapter_count {
                continue;
            }
            let Some(key) = lookup(ch) else { continue };
            if key.chapter != ch || self.contains(&key) {
                continue;
            }
            if self.check(&key, false).is_ok() {
                return Some(WarmJob {
                    key,
                    generation: self.generation,
                    filled: 0,
                });
            }
        }
        None
    }

    /// Publish a completely read neighbor. Refused when the window moved or the
    /// source changed since the job began, when bytes are missing, and by the
    /// admission rules of `insert`.
    pub fn publish(&mut self, job: &WarmJob, value: T) -> Result<(), (Refusal, T)> {
        if job.generation != self.generation {
            return Err((Refusal::Stale, value));
        }
        if !job.is_complete() {
            return Err((Refusal::Incomplete, value));
        }
        // a neighbor must fit without pushing anything out
        if let Err(r) = self.check(&job.key, false) {
            return Err((r, value));
        }
        self.insert(job.key, value)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WarmError {
    /// The device returned nothing before the chapter was complete.
    ShortRead,
    /// More bytes than were asked for.
    Overrun,
}

/// A bounded, resumable read of one neighbor chapter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WarmJob {
    key: ChapterKey,
    generation: u32,
    filled: u32,
}

impl WarmJob {
    pub const fn key(&self) -> &ChapterKey {
        &self.key
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }

    pub const fn filled(&self) -> usize {
        self.filled as usize
    }

    pub const fn is_complete(&self) -> bool {
        self.filled == self.key.size
    }

    /// Next read: byte offset inside the chapter text and length, at most
    /// `max_chunk`. `None` when complete.
    pub fn next_read(&self, max_chunk: usize) -> Option<(u32, usize)> {
        let left = (self.key.size - self.filled) as usize;
        if left == 0 || max_chunk == 0 {
            return None;
        }
        Some((self.filled, left.min(max_chunk)))
    }

    /// Record that `got` bytes of a read of `asked` arrived. Returns whether the
    /// chapter is now complete. A zero-byte read is an error, never a silent
    /// end: the chapter would otherwise be published short.
    pub fn record(&mut self, got: usize, asked: usize) -> Result<bool, WarmError> {
        if got == 0 {
            return Err(WarmError::ShortRead);
        }
        if got > asked || got > (self.key.size - self.filled) as usize {
            return Err(WarmError::Overrun);
        }
        self.filled += got as u32;
        Ok(self.is_complete())
    }
}

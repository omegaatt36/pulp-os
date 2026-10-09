// SD sector read cache (write-through), the logic under the volume manager.
//
// HAL-free and allocation-free: the cache owns two caller-provided storage
// blocks (tags + sector contents) and talks to the card through the small
// `SectorDevice` trait, so the firmware (`kernel/src/drivers/sdcard.rs`, PSRAM
// `StorageCache` class backing) and the host tests (a recording fake card)
// run the very same code.
//
// Policy, in one paragraph. The cache is set associative: `WAYS` ways per set,
// `lba % sets` picks the set. Every way is `Probation` (seen once), `Protected`
// (seen again while cached) or empty. A new sector always enters `Probation`;
// the victim is an empty way, else the least recently used `Probation` way. A
// hit on a `Probation` way promotes it; at most `PROTECTED_WAYS` ways of a
// set are protected (promoting past that demotes the set's least recently used
// protected way). So a long sequential scan, whose sectors are never read
// twice, only rotates through the probation ways and cannot evict the FAT and
// directory sectors that were read repeatedly.
//
// Coherence. Write-through, no dirty data: before the device write the
// addressed cached sectors are made unreadable (`Stale*`); on success they are
// refreshed with the written bytes (and keep their class), on any error,
// including a partial multi-sector write, they are dropped. A failed read never
// publishes anything: misses are inserted only after the whole request
// succeeded. A card change is a new `SectorCache` (the firmware builds one per
// mount); `invalidate_all` is available for the same purpose.

use crate::memory::{
    KIB, MemClass, PSRAM_LARGE_MIN_BYTES, PsramStatus, Region, charge, class_limit,
};

pub const SECTOR_BYTES: usize = 512;
pub type Sector = [u8; SECTOR_BYTES];

/// Ways per set.
pub const WAYS: usize = 4;
/// Most protected ways of one set (the rest rotate as probation).
pub const PROTECTED_WAYS: usize = 2;
/// Alignment the backing blocks are requested with (the PSRAM allocator uses
/// at least 16, see `memory::ALLOC_GRANULE`).
pub const BACKING_ALIGN: usize = 16;

/// Cache size on a standard (HR2) part and on a large (HR8) part; smaller or
/// unavailable PSRAM disables the cache.
pub const STANDARD_CACHE_BYTES: usize = 64 * KIB;
pub const LARGE_CACHE_BYTES: usize = 128 * KIB;
/// Below this the cache is not worth its overhead and stays off.
pub const MIN_CACHE_BYTES: usize = 64 * KIB;
/// `plan` never sizes a cache above this, whatever it is asked for.
pub const MAX_PLAN_BYTES: usize = 16 * 1024 * KIB;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum State {
    Empty,
    Probation,
    Protected,
    // being rewritten: kept so a successful write can refresh the slot, but
    // never served
    StaleProbation,
    StaleProtected,
}

/// Tag and recency of one cache way (contents live in the sector block).
#[derive(Copy, Clone, Debug)]
pub struct Meta {
    tag: u32,
    stamp: u32,
    state: State,
}

impl Meta {
    pub const EMPTY: Meta = Meta {
        tag: 0,
        stamp: 0,
        state: State::Empty,
    };
}

/// Bytes of one `Meta` entry.
pub const META_BYTES: usize = core::mem::size_of::<Meta>();

/// Runtime cache size for a PSRAM state: 128 KiB on HR8, 64 KiB on HR2, never
/// above the profile's `StorageCache` class limit, 0 (cache off) when PSRAM is
/// absent, degraded or not initialised.
pub const fn sd_cache_budget(status: PsramStatus) -> usize {
    match status {
        PsramStatus::Ready { bytes } => {
            let limit = class_limit(status, Region::Psram, MemClass::StorageCache);
            let want = if bytes >= PSRAM_LARGE_MIN_BYTES {
                LARGE_CACHE_BYTES
            } else {
                STANDARD_CACHE_BYTES
            };
            let size = if limit < want { limit } else { want };
            if size < MIN_CACHE_BYTES { 0 } else { size }
        }
        _ => 0,
    }
}

/// Shape of a cache that fits a byte budget.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub sets: usize,
}

impl Geometry {
    pub const fn entries(&self) -> usize {
        self.sets * WAYS
    }

    /// Bytes charged to the allocation class, both blocks, with the
    /// allocator's rounding. `None` on overflow.
    pub fn charged_bytes(&self) -> Option<usize> {
        let n = self.entries();
        let data = charge(n.checked_mul(SECTOR_BYTES)?, BACKING_ALIGN).ok()?;
        let meta = charge(n.checked_mul(META_BYTES)?, BACKING_ALIGN).ok()?;
        data.checked_add(meta)
    }
}

/// Largest geometry whose total charge (tags and contents) stays within
/// `budget_bytes`. `None` if not even one set fits.
pub fn plan(budget_bytes: usize) -> Option<Geometry> {
    // keeps every size computation below far from overflow
    let budget_bytes = budget_bytes.min(MAX_PLAN_BYTES);
    let mut sets = budget_bytes / ((SECTOR_BYTES + META_BYTES) * WAYS);
    while sets > 0 {
        let g = Geometry { sets };
        if g.charged_bytes().is_some_and(|b| b <= budget_bytes) {
            return Some(g);
        }
        sets -= 1;
    }
    None
}

/// Counters, all saturating. A sector request counts one `hit` or one `miss`
/// per sector; sectors after a failed device read are not looked at.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SectorCacheStats {
    /// Read requests that went through the cache.
    pub read_requests: u32,
    /// Sectors served from the cache.
    pub hits: u32,
    /// Sectors that had to come from the card.
    pub misses: u32,
    /// Contiguous device reads issued for misses.
    pub device_reads: u32,
    /// Sectors asked of the card by those reads.
    pub device_read_sectors: u32,
    /// Sectors published after a fully successful request.
    pub insertions: u32,
    /// Valid sectors dropped to make room.
    pub evictions: u32,
    /// Probation to protected.
    pub promotions: u32,
    /// Protected to probation (set was full of protected ways).
    pub demotions: u32,
    /// Sectors dropped for coherence (failed write, `invalidate_*`).
    pub invalidations: u32,
    /// Device reads that failed (nothing published for that request).
    pub read_failures: u32,
    /// Write requests.
    pub write_requests: u32,
    /// Cached sectors refreshed by a successful write.
    pub write_updates: u32,
    /// Device writes that failed (addressed sectors invalidated).
    pub write_failures: u32,
    /// Requests whose sector range overflows the 32-bit sector space; passed
    /// to the device untouched.
    pub bypassed: u32,
}

fn bump(counter: &mut u32, by: usize) {
    *counter = counter.saturating_add(by.min(u32::MAX as usize) as u32);
}

/// A card as the cache sees it. `Elem` is the caller's sector container (the
/// firmware uses `embedded_sdmmc::Block`, which is not `Copy` and has no fixed
/// layout guarantee, so access goes through `bytes`/`bytes_mut`).
pub trait SectorDevice {
    type Elem;
    type Error;
    fn bytes(e: &Self::Elem) -> &Sector;
    fn bytes_mut(e: &mut Self::Elem) -> &mut Sector;
    fn read(&mut self, dst: &mut [Self::Elem], start: u32) -> Result<(), Self::Error>;
    fn write(&mut self, src: &[Self::Elem], start: u32) -> Result<(), Self::Error>;
}

/// `count` sectors from `start` stay inside the 32-bit sector space.
fn range_fits(start: u32, count: usize) -> bool {
    (start as u64) + (count as u64) <= (1u64 << 32)
}

/// Bounded set-associative sector cache over caller-owned storage `M` (tags)
/// and `D` (contents).
pub struct SectorCache<M, D> {
    meta: M,
    data: D,
    sets: usize,
    tick: u32,
    stats: SectorCacheStats,
}

impl<M, D> SectorCache<M, D>
where
    M: AsRef<[Meta]> + AsMut<[Meta]>,
    D: AsRef<[Sector]> + AsMut<[Sector]>,
{
    /// `None` unless both blocks are non-empty, equally long and a whole
    /// number of sets. All entries start empty (the contents block is left as
    /// provided: it is never read before a tag says so).
    pub fn new(mut meta: M, data: D) -> Option<Self> {
        let n = meta.as_ref().len();
        if n == 0 || n % WAYS != 0 || data.as_ref().len() != n {
            return None;
        }
        meta.as_mut().fill(Meta::EMPTY);
        Some(Self {
            meta,
            data,
            sets: n / WAYS,
            tick: 0,
            stats: SectorCacheStats::default(),
        })
    }

    pub fn stats(&self) -> SectorCacheStats {
        self.stats
    }

    /// Sectors the cache can hold.
    pub fn capacity(&self) -> usize {
        self.sets * WAYS
    }

    /// Sectors currently readable from the cache.
    pub fn len(&self) -> usize {
        self.meta
            .as_ref()
            .iter()
            .filter(|m| matches!(m.state, State::Probation | State::Protected))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Is `lba` readable from the cache (no counters, no recency change).
    pub fn contains(&self, lba: u32) -> bool {
        self.find(lba).is_some()
    }

    /// Is `lba` cached in the protected class.
    pub fn is_protected(&self, lba: u32) -> bool {
        self.find(lba)
            .is_some_and(|i| self.meta.as_ref()[i].state == State::Protected)
    }

    fn base(&self, lba: u32) -> usize {
        (lba as usize % self.sets) * WAYS
    }

    fn find(&self, lba: u32) -> Option<usize> {
        let base = self.base(lba);
        (base..base + WAYS).find(|&i| {
            let m = &self.meta.as_ref()[i];
            m.tag == lba && matches!(m.state, State::Probation | State::Protected)
        })
    }

    fn find_stale(&self, lba: u32) -> Option<usize> {
        let base = self.base(lba);
        (base..base + WAYS).find(|&i| {
            let m = &self.meta.as_ref()[i];
            m.tag == lba && matches!(m.state, State::StaleProbation | State::StaleProtected)
        })
    }

    fn next_tick(&mut self) -> u32 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn age(&self, i: usize) -> u32 {
        self.tick.wrapping_sub(self.meta.as_ref()[i].stamp)
    }

    // a hit: refresh recency, promote a probation way
    fn touch(&mut self, idx: usize) {
        let now = self.next_tick();
        self.meta.as_mut()[idx].stamp = now;
        if self.meta.as_ref()[idx].state != State::Probation {
            return;
        }
        let base = idx - idx % WAYS;
        let protected = (base..base + WAYS)
            .filter(|&i| self.meta.as_ref()[i].state == State::Protected)
            .count();
        if protected >= PROTECTED_WAYS {
            let oldest = (base..base + WAYS)
                .filter(|&i| self.meta.as_ref()[i].state == State::Protected)
                .max_by_key(|&i| self.age(i));
            if let Some(o) = oldest {
                self.meta.as_mut()[o].state = State::Probation;
                bump(&mut self.stats.demotions, 1);
            }
        }
        self.meta.as_mut()[idx].state = State::Protected;
        bump(&mut self.stats.promotions, 1);
    }

    // publish one sector into probation, evicting if the set is full
    fn insert(&mut self, lba: u32, bytes: &Sector) {
        let base = self.base(lba);
        let ways = base..base + WAYS;
        let victim = ways
            .clone()
            .find(|&i| self.meta.as_ref()[i].state == State::Empty)
            .or_else(|| {
                ways.clone()
                    .filter(|&i| self.meta.as_ref()[i].state == State::Probation)
                    .max_by_key(|&i| self.age(i))
            })
            .or_else(|| {
                ways.filter(|&i| self.meta.as_ref()[i].state == State::Protected)
                    .max_by_key(|&i| self.age(i))
            });
        // only stale ways left cannot happen outside a write; skip then
        let Some(v) = victim else { return };
        if self.meta.as_ref()[v].state != State::Empty {
            bump(&mut self.stats.evictions, 1);
        }
        let now = self.next_tick();
        self.meta.as_mut()[v] = Meta {
            tag: lba,
            stamp: now,
            state: State::Probation,
        };
        self.data.as_mut()[v] = *bytes;
        bump(&mut self.stats.insertions, 1);
    }

    /// Drop everything (card change). Counters are kept.
    pub fn invalidate_all(&mut self) {
        let mut dropped = 0usize;
        for m in self.meta.as_mut() {
            if m.state != State::Empty {
                dropped += 1;
                *m = Meta::EMPTY;
            }
        }
        bump(&mut self.stats.invalidations, dropped);
    }

    /// Drop the cached sectors of `[start, start + count)`; the part of the
    /// range beyond the 32-bit sector space is ignored.
    pub fn invalidate_range(&mut self, start: u32, count: usize) {
        let covered = covered(start, count);
        let mut dropped = 0usize;
        for k in 0..covered {
            let lba = start + k as u32;
            let base = self.base(lba);
            for i in base..base + WAYS {
                let m = &mut self.meta.as_mut()[i];
                if m.tag == lba && m.state != State::Empty {
                    *m = Meta::EMPTY;
                    dropped += 1;
                }
            }
        }
        bump(&mut self.stats.invalidations, dropped);
    }

    /// Read `dst.len()` sectors from `start`: cached sectors are copied,
    /// contiguous missing runs come from the device in one call each. Misses
    /// are published only if the whole request succeeded.
    pub fn read<Dev: SectorDevice>(
        &mut self,
        dev: &mut Dev,
        dst: &mut [Dev::Elem],
        start: u32,
    ) -> Result<(), Dev::Error> {
        let n = dst.len();
        if n == 0 {
            return Ok(());
        }
        if !range_fits(start, n) {
            bump(&mut self.stats.bypassed, 1);
            return dev.read(dst, start);
        }
        bump(&mut self.stats.read_requests, 1);

        let mut i = 0;
        while i < n {
            let lba = start + i as u32;
            if let Some(idx) = self.find(lba) {
                *Dev::bytes_mut(&mut dst[i]) = self.data.as_ref()[idx];
                self.touch(idx);
                bump(&mut self.stats.hits, 1);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < n && self.find(start + j as u32).is_none() {
                j += 1;
            }
            bump(&mut self.stats.misses, j - i);
            bump(&mut self.stats.device_reads, 1);
            bump(&mut self.stats.device_read_sectors, j - i);
            if let Err(e) = dev.read(&mut dst[i..j], lba) {
                // nothing of this request is published
                bump(&mut self.stats.read_failures, 1);
                return Err(e);
            }
            i = j;
        }

        // complete: publish what was not cached (a sector of this request that
        // was a hit and got evicted meanwhile is simply inserted again)
        for (k, e) in dst.iter().enumerate() {
            let lba = start + k as u32;
            if self.find(lba).is_none() {
                self.insert(lba, Dev::bytes(e));
            }
        }
        Ok(())
    }

    /// Write-through. Addressed cached sectors are made unreadable first;
    /// success refreshes them with the written bytes, any error drops them.
    pub fn write<Dev: SectorDevice>(
        &mut self,
        dev: &mut Dev,
        src: &[Dev::Elem],
        start: u32,
    ) -> Result<(), Dev::Error> {
        let n = src.len();
        if n == 0 {
            return Ok(());
        }
        bump(&mut self.stats.write_requests, 1);
        let covered = covered(start, n);
        if covered < n {
            bump(&mut self.stats.bypassed, 1);
        }

        for k in 0..covered {
            if let Some(i) = self.find(start + k as u32) {
                let m = &mut self.meta.as_mut()[i];
                m.state = if m.state == State::Protected {
                    State::StaleProtected
                } else {
                    State::StaleProbation
                };
            }
        }

        match dev.write(src, start) {
            Ok(()) => {
                for (k, e) in src.iter().enumerate().take(covered) {
                    if let Some(i) = self.find_stale(start + k as u32) {
                        self.data.as_mut()[i] = *Dev::bytes(e);
                        let now = self.next_tick();
                        let m = &mut self.meta.as_mut()[i];
                        m.stamp = now;
                        m.state = if m.state == State::StaleProtected {
                            State::Protected
                        } else {
                            State::Probation
                        };
                        bump(&mut self.stats.write_updates, 1);
                    }
                }
                Ok(())
            }
            Err(e) => {
                let mut dropped = 0usize;
                for k in 0..covered {
                    if let Some(i) = self.find_stale(start + k as u32) {
                        self.meta.as_mut()[i] = Meta::EMPTY;
                        dropped += 1;
                    }
                }
                bump(&mut self.stats.invalidations, dropped);
                bump(&mut self.stats.write_failures, 1);
                Err(e)
            }
        }
    }
}

/// How many sectors of `[start, start + count)` lie inside the 32-bit space.
fn covered(start: u32, count: usize) -> usize {
    let room = (1u64 << 32) - start as u64;
    (count as u64).min(room) as usize
}

/// Build a cache for `budget_bytes` with caller-provided fallible backing.
/// `None` (use the device directly) if the budget is too small or either
/// allocation fails; nothing is left allocated in that case beyond what the
/// failing closure already returned (a successful first block is dropped).
pub fn build_cache<M, D, E>(
    budget_bytes: usize,
    alloc_meta: impl FnOnce(usize) -> Result<M, E>,
    alloc_data: impl FnOnce(usize) -> Result<D, E>,
) -> Option<SectorCache<M, D>>
where
    M: AsRef<[Meta]> + AsMut<[Meta]>,
    D: AsRef<[Sector]> + AsMut<[Sector]>,
{
    let g = plan(budget_bytes)?;
    let n = g.entries();
    let meta = alloc_meta(n).ok()?;
    let data = alloc_data(n).ok()?;
    SectorCache::new(meta, data)
}

/// A device with an optional cache in front; `None` is exact passthrough.
pub struct CachedDevice<Dev, M, D> {
    dev: Dev,
    cache: Option<SectorCache<M, D>>,
}

impl<Dev, M, D> CachedDevice<Dev, M, D>
where
    Dev: SectorDevice,
    M: AsRef<[Meta]> + AsMut<[Meta]>,
    D: AsRef<[Sector]> + AsMut<[Sector]>,
{
    pub fn new(dev: Dev, cache: Option<SectorCache<M, D>>) -> Self {
        Self { dev, cache }
    }

    pub fn read(&mut self, dst: &mut [Dev::Elem], start: u32) -> Result<(), Dev::Error> {
        match &mut self.cache {
            Some(c) => c.read(&mut self.dev, dst, start),
            None => self.dev.read(dst, start),
        }
    }

    pub fn write(&mut self, src: &[Dev::Elem], start: u32) -> Result<(), Dev::Error> {
        match &mut self.cache {
            Some(c) => c.write(&mut self.dev, src, start),
            None => self.dev.write(src, start),
        }
    }

    pub fn device(&self) -> &Dev {
        &self.dev
    }

    pub fn cache(&self) -> Option<&SectorCache<M, D>> {
        self.cache.as_ref()
    }

    pub fn cache_mut(&mut self) -> Option<&mut SectorCache<M, D>> {
        self.cache.as_mut()
    }

    pub fn stats(&self) -> Option<SectorCacheStats> {
        self.cache.as_ref().map(|c| c.stats())
    }
}

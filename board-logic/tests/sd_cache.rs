// SD sector cache against an instrumented fake card (the production
// `pulp_board_logic::sd_cache`, no stand-in).

use pulp_board_logic::memory::{
    MemClass, PSRAM_LARGE_MIN_BYTES, PSRAM_STORAGE_CACHE_BYTES, PsramFault, PsramStatus, Region,
    class_limit,
};
use pulp_board_logic::sd_cache::{
    CachedDevice, LARGE_CACHE_BYTES, META_BYTES, Meta, PROTECTED_WAYS, SECTOR_BYTES,
    STANDARD_CACHE_BYTES, Sector, SectorCache, SectorCacheStats, SectorDevice, WAYS, build_cache,
    plan, sd_cache_budget,
};
use std::collections::HashMap;

// like embedded_sdmmc::Block: Clone, not Copy
#[derive(Clone)]
struct Blk(Sector);

#[derive(Debug, PartialEq, Eq)]
struct Fail;

fn content(lba: u32, generation: u8) -> Sector {
    let mut s = [0u8; SECTOR_BYTES];
    for (i, b) in s.iter_mut().enumerate() {
        *b = (lba as u8)
            .wrapping_mul(31)
            .wrapping_add(i as u8)
            .wrapping_add(generation);
    }
    s[..4].copy_from_slice(&lba.to_le_bytes());
    s
}

struct Fake {
    capacity: u64,
    generation: u8,
    written: HashMap<u32, Sector>,
    reads: Vec<(u32, usize)>,
    writes: Vec<(u32, usize)>,
    fail_read_at: Option<u32>,
    fail_write_after: Option<usize>,
}

impl Fake {
    fn new() -> Self {
        Self::with_capacity(1 << 20)
    }
    fn with_capacity(capacity: u64) -> Self {
        Fake {
            capacity,
            generation: 0,
            written: HashMap::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            fail_read_at: None,
            fail_write_after: None,
        }
    }
    fn truth(&self, lba: u32) -> Sector {
        self.written
            .get(&lba)
            .copied()
            .unwrap_or_else(|| content(lba, self.generation))
    }
}

impl SectorDevice for Fake {
    type Elem = Blk;
    type Error = Fail;
    fn bytes(e: &Blk) -> &Sector {
        &e.0
    }
    fn bytes_mut(e: &mut Blk) -> &mut Sector {
        &mut e.0
    }
    fn read(&mut self, dst: &mut [Blk], start: u32) -> Result<(), Fail> {
        self.reads.push((start, dst.len()));
        if start as u64 + dst.len() as u64 > self.capacity {
            return Err(Fail);
        }
        for (k, d) in dst.iter_mut().enumerate() {
            let lba = start + k as u32;
            if self.fail_read_at == Some(lba) {
                // what a half-done transfer leaves in the caller's buffer
                for rest in dst[k..].iter_mut() {
                    rest.0 = [0xEE; SECTOR_BYTES];
                }
                return Err(Fail);
            }
            d.0 = self.truth(lba);
        }
        Ok(())
    }
    fn write(&mut self, src: &[Blk], start: u32) -> Result<(), Fail> {
        self.writes.push((start, src.len()));
        for (k, s) in src.iter().enumerate() {
            let lba = start as u64 + k as u64;
            if lba >= self.capacity || self.fail_write_after == Some(k) {
                return Err(Fail);
            }
            self.written.insert(lba as u32, s.0);
        }
        Ok(())
    }
}

type Cache = SectorCache<Vec<Meta>, Vec<Sector>>;

fn cache(sets: usize) -> Cache {
    SectorCache::new(
        vec![Meta::EMPTY; sets * WAYS],
        vec![[0u8; SECTOR_BYTES]; sets * WAYS],
    )
    .unwrap()
}

fn blocks(n: usize) -> Vec<Blk> {
    vec![Blk([0; SECTOR_BYTES]); n]
}

fn rd(c: &mut Cache, f: &mut Fake, start: u32, n: usize) -> Result<Vec<Blk>, Fail> {
    let mut v = blocks(n);
    c.read(f, &mut v, start)?;
    Ok(v)
}

fn assert_truth(f: &Fake, v: &[Blk], start: u32) {
    for (k, b) in v.iter().enumerate() {
        assert_eq!(
            b.0,
            f.truth(start + k as u32),
            "sector {}",
            start + k as u32
        );
    }
}

#[test]
fn single_sector_miss_then_hit() {
    let mut f = Fake::new();
    let mut c = cache(4);
    let v = rd(&mut c, &mut f, 10, 1).unwrap();
    assert_truth(&f, &v, 10);
    let s = c.stats();
    assert_eq!(
        (s.read_requests, s.hits, s.misses, s.insertions),
        (1, 0, 1, 1)
    );
    assert_eq!((s.device_reads, s.device_read_sectors), (1, 1));

    let v = rd(&mut c, &mut f, 10, 1).unwrap();
    assert_truth(&f, &v, 10);
    assert_eq!(f.reads, vec![(10, 1)]);
    let s = c.stats();
    assert_eq!(
        (s.read_requests, s.hits, s.misses, s.insertions),
        (2, 1, 1, 1)
    );
    assert_eq!(s.promotions, 1);
    assert!(c.is_protected(10));
}

#[test]
fn multi_block_miss_is_one_device_read_and_later_partial_hit() {
    let mut f = Fake::new();
    let mut c = cache(16);
    let v = rd(&mut c, &mut f, 100, 8).unwrap();
    assert_truth(&f, &v, 100);
    assert_eq!(f.reads, vec![(100, 8)]);
    assert_eq!(c.len(), 8);

    // 104..108 cached, 108..112 not: one device read for the tail only
    let v = rd(&mut c, &mut f, 104, 8).unwrap();
    assert_truth(&f, &v, 104);
    assert_eq!(f.reads, vec![(100, 8), (108, 4)]);
    let s = c.stats();
    assert_eq!((s.hits, s.misses, s.insertions), (4, 12, 12));
    assert_eq!(s.device_read_sectors, 12);

    // fully cached: no device traffic
    let v = rd(&mut c, &mut f, 100, 12).unwrap();
    assert_truth(&f, &v, 100);
    assert_eq!(f.reads.len(), 2);
}

#[test]
fn holes_split_into_separate_runs_in_order() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 2, 1).unwrap();
    rd(&mut c, &mut f, 5, 1).unwrap();
    f.reads.clear();
    let v = rd(&mut c, &mut f, 0, 8).unwrap();
    assert_truth(&f, &v, 0);
    assert_eq!(f.reads, vec![(0, 2), (3, 2), (6, 2)]);
    assert_eq!(c.len(), 8);
}

#[test]
fn request_wider_than_the_cache_and_set_wraparound() {
    let mut f = Fake::new();
    let mut c = cache(2); // 8 ways
    let v = rd(&mut c, &mut f, 0, 20).unwrap();
    assert_truth(&f, &v, 0);
    assert_eq!(f.reads, vec![(0, 20)]);
    assert_eq!(c.len(), c.capacity());
    let s = c.stats();
    assert_eq!(s.insertions, 20);
    assert_eq!(s.evictions, 12);
    // whatever is cached is correct
    for lba in 0..20u32 {
        if c.contains(lba) {
            let v = rd(&mut c, &mut f, lba, 1).unwrap();
            assert_truth(&f, &v, lba);
        }
    }
}

#[test]
fn sequential_scan_does_not_wash_out_protected_sectors() {
    let mut f = Fake::new();
    let mut c = cache(4);
    // two "metadata" sectors read repeatedly
    for _ in 0..2 {
        rd(&mut c, &mut f, 100, 1).unwrap();
        rd(&mut c, &mut f, 101, 1).unwrap();
    }
    assert!(c.is_protected(100) && c.is_protected(101));
    // a one-shot scan far bigger than the cache
    for lba in 1000..1400u32 {
        rd(&mut c, &mut f, lba, 1).unwrap();
    }
    let before = f.reads.len();
    assert!(c.is_protected(100) && c.is_protected(101));
    rd(&mut c, &mut f, 100, 1).unwrap();
    rd(&mut c, &mut f, 101, 1).unwrap();
    assert_eq!(f.reads.len(), before, "protected sectors survived the scan");
    assert!(c.stats().evictions >= 400 - 14);

    // the same through one big request
    rd(&mut c, &mut f, 5000, 300).unwrap();
    assert!(c.is_protected(100) && c.is_protected(101));
}

#[test]
fn protected_class_is_bounded_per_set_and_demotes_lru() {
    let mut f = Fake::new();
    let mut c = cache(4); // sectors 0,4,8,12,16 share set 0
    assert_eq!(PROTECTED_WAYS, 2);
    for lba in [0u32, 4, 8] {
        rd(&mut c, &mut f, lba, 1).unwrap();
        rd(&mut c, &mut f, lba, 1).unwrap();
    }
    let s = c.stats();
    assert_eq!((s.promotions, s.demotions), (3, 1));
    assert!(
        !c.is_protected(0) && c.contains(0),
        "oldest protected demoted"
    );
    assert!(c.is_protected(4) && c.is_protected(8));

    // fills the empty way, then the next one evicts the LRU probation way (0)
    rd(&mut c, &mut f, 12, 1).unwrap();
    assert_eq!(c.stats().evictions, 0);
    rd(&mut c, &mut f, 16, 1).unwrap();
    assert_eq!(c.stats().evictions, 1);
    assert!(!c.contains(0) && c.contains(12) && c.contains(16));
    assert!(c.is_protected(4) && c.is_protected(8));
}

#[test]
fn probation_victim_is_least_recently_used() {
    let mut f = Fake::new();
    let mut c = cache(1); // one set, 4 ways
    for lba in 0..4u32 {
        rd(&mut c, &mut f, lba, 1).unwrap();
    }
    // touching 0 promotes it; 1 is then the oldest probation sector
    rd(&mut c, &mut f, 0, 1).unwrap();
    rd(&mut c, &mut f, 4, 1).unwrap();
    assert!(!c.contains(1) && c.contains(0) && c.contains(2) && c.contains(3) && c.contains(4));
}

#[test]
fn failed_read_publishes_nothing_and_leaves_no_garbage() {
    let mut f = Fake::new();
    let mut c = cache(16);
    f.fail_read_at = Some(5);
    assert!(rd(&mut c, &mut f, 0, 8).is_err());
    for lba in 0..8u32 {
        assert!(!c.contains(lba));
    }
    let s = c.stats();
    assert_eq!(
        (s.insertions, s.read_failures, s.misses, s.device_reads),
        (0, 1, 8, 1)
    );

    f.fail_read_at = None;
    let v = rd(&mut c, &mut f, 0, 8).unwrap();
    assert_truth(&f, &v, 0);
    assert!(v.iter().all(|b| b.0[7] != 0xEE || b.0 == f.truth(0)));
    assert_eq!(c.stats().insertions, 8);
}

#[test]
fn failure_in_a_later_run_keeps_earlier_hits_but_publishes_no_miss() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 0, 2).unwrap();
    f.reads.clear();
    f.fail_read_at = Some(4);
    // 0,1 hit; 2..6 missing and fails midway
    assert!(rd(&mut c, &mut f, 0, 6).is_err());
    assert_eq!(f.reads, vec![(2, 4)]);
    assert!(c.contains(0) && c.contains(1));
    for lba in 2..6u32 {
        assert!(!c.contains(lba));
    }
    let s = c.stats();
    assert_eq!((s.insertions, s.hits, s.read_failures), (2, 2, 1));

    f.fail_read_at = None;
    let v = rd(&mut c, &mut f, 0, 6).unwrap();
    assert_truth(&f, &v, 0);
    assert_eq!(f.reads, vec![(2, 4), (2, 4)]);
}

#[test]
fn successful_write_refreshes_the_cached_sector_and_keeps_its_class() {
    let mut f = Fake::new();
    let mut c = cache(4);
    rd(&mut c, &mut f, 7, 1).unwrap();
    rd(&mut c, &mut f, 7, 1).unwrap();
    assert!(c.is_protected(7));
    let new = Blk([0x5A; SECTOR_BYTES]);
    c.write(&mut f, std::slice::from_ref(&new), 7).unwrap();
    assert_eq!(f.writes, vec![(7, 1)]);
    assert!(c.is_protected(7));
    let reads = f.reads.len();
    let v = rd(&mut c, &mut f, 7, 1).unwrap();
    assert_eq!(v[0].0, [0x5A; SECTOR_BYTES]);
    assert_eq!(f.reads.len(), reads, "served from the refreshed entry");
    let s = c.stats();
    assert_eq!(
        (s.write_requests, s.write_updates, s.write_failures),
        (1, 1, 0)
    );
    assert_eq!(s.invalidations, 0);
}

#[test]
fn write_does_not_insert_uncached_sectors() {
    let mut f = Fake::new();
    let mut c = cache(4);
    let w = vec![Blk([1; SECTOR_BYTES]); 3];
    c.write(&mut f, &w, 40).unwrap();
    assert!(c.is_empty());
    assert_eq!(c.stats().insertions, 0);
    let v = rd(&mut c, &mut f, 40, 3).unwrap();
    assert!(v.iter().all(|b| b.0 == [1; SECTOR_BYTES]));
}

#[test]
fn failed_write_invalidates_and_next_read_sees_the_card() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 10, 1).unwrap();
    f.fail_write_after = Some(0);
    let w = [Blk([9; SECTOR_BYTES])];
    assert_eq!(c.write(&mut f, &w, 10), Err(Fail));
    assert!(!c.contains(10));
    let s = c.stats();
    assert_eq!(
        (s.write_failures, s.invalidations, s.write_updates),
        (1, 1, 0)
    );
    f.fail_write_after = None;
    let v = rd(&mut c, &mut f, 10, 1).unwrap();
    assert_truth(&f, &v, 10);
}

#[test]
fn partial_multi_sector_write_failure_invalidates_every_addressed_sector() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 8, 8).unwrap(); // 8..16 cached
    f.fail_write_after = Some(2);
    let w = vec![Blk([0xA5; SECTOR_BYTES]); 4];
    assert_eq!(c.write(&mut f, &w, 10), Err(Fail)); // 10,11 hit the card
    for lba in 10..14u32 {
        assert!(!c.contains(lba), "sector {lba}");
    }
    assert!(c.contains(8) && c.contains(9) && c.contains(14) && c.contains(15));
    assert_eq!(c.stats().invalidations, 4);
    f.fail_write_after = None;
    let v = rd(&mut c, &mut f, 8, 8).unwrap();
    assert_truth(&f, &v, 8);
    assert_eq!(v[2].0, [0xA5; SECTOR_BYTES]);
    assert_eq!(v[4].0, content(12, 0));
}

#[test]
fn multi_sector_write_updates_only_cached_sectors() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 21, 1).unwrap();
    let w = vec![Blk([0x33; SECTOR_BYTES]); 4];
    c.write(&mut f, &w, 20).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c.stats().write_updates, 1);
    let reads = f.reads.len();
    let v = rd(&mut c, &mut f, 21, 1).unwrap();
    assert_eq!(v[0].0, [0x33; SECTOR_BYTES]);
    assert_eq!(f.reads.len(), reads);
}

#[test]
fn fresh_cache_after_card_change_serves_the_new_card() {
    let mut old_card = Fake::new();
    let mut c = cache(8);
    rd(&mut c, &mut old_card, 0, 16).unwrap();
    rd(&mut c, &mut old_card, 3, 1).unwrap();

    // another card with other bytes; the firmware builds a new cache on mount
    let mut new_card = Fake::new();
    new_card.generation = 7;
    let mut fresh = cache(8);
    assert!(fresh.is_empty());
    assert_eq!(fresh.stats(), SectorCacheStats::default());
    let v = rd(&mut fresh, &mut new_card, 0, 16).unwrap();
    assert_truth(&new_card, &v, 0);
    assert_ne!(v[3].0, content(3, 0));

    // invalidate_all gives the same result for a cache that is kept
    c.invalidate_all();
    assert!(c.is_empty());
    assert_eq!(c.stats().invalidations, 16);
    let v = rd(&mut c, &mut new_card, 0, 16).unwrap();
    assert_truth(&new_card, &v, 0);
}

#[test]
fn invalidate_range_drops_only_the_range_and_counts() {
    let mut f = Fake::new();
    let mut c = cache(16);
    rd(&mut c, &mut f, 0, 10).unwrap();
    c.invalidate_range(3, 4);
    assert_eq!(c.stats().invalidations, 4);
    for lba in 0..10u32 {
        assert_eq!(c.contains(lba), !(3..7).contains(&lba));
    }
    c.invalidate_range(u32::MAX, 5); // beyond the sector space: ignored, no panic
    c.invalidate_range(0, 0);
    assert_eq!(c.stats().invalidations, 4);
    c.invalidate_all();
    assert_eq!(c.stats().invalidations, 4 + 6);
}

#[test]
fn zero_length_and_sector_space_overflow() {
    let mut f = Fake::with_capacity(1 << 32);
    let mut c = cache(4);
    c.read(&mut f, &mut [], 5).unwrap();
    c.write(&mut f, &[], 5).unwrap();
    assert!(f.reads.is_empty() && f.writes.is_empty());
    assert_eq!(c.stats(), SectorCacheStats::default());

    // the very last sector is fine
    let v = rd(&mut c, &mut f, u32::MAX, 1).unwrap();
    assert_eq!(v[0].0, content(u32::MAX, 0));
    assert!(c.contains(u32::MAX));

    // one past the end: handed to the card untouched, nothing published
    assert!(rd(&mut c, &mut f, u32::MAX, 2).is_err());
    assert_eq!(f.reads.last(), Some(&(u32::MAX, 2)));
    let s = c.stats();
    assert_eq!((s.bypassed, s.read_requests, s.insertions), (1, 1, 1));
    assert!(rd(&mut c, &mut f, u32::MAX - 1, 100).is_err());
    assert_eq!(c.stats().bypassed, 2);
    assert_eq!(c.len(), 1);
}

#[test]
fn write_across_the_end_of_the_sector_space_invalidates_the_valid_part() {
    let mut f = Fake::with_capacity(1 << 32);
    let mut c = cache(4);
    rd(&mut c, &mut f, u32::MAX - 1, 2).unwrap();
    let w = vec![Blk([8; SECTOR_BYTES]); 4];
    assert_eq!(c.write(&mut f, &w, u32::MAX - 1), Err(Fail));
    assert!(!c.contains(u32::MAX - 1) && !c.contains(u32::MAX));
    let s = c.stats();
    assert_eq!((s.bypassed, s.write_failures, s.invalidations), (1, 1, 2));
    // the two sectors the card did take are visible
    let v = rd(&mut c, &mut f, u32::MAX - 1, 2).unwrap();
    assert!(v.iter().all(|b| b.0 == [8; SECTOR_BYTES]));
}

#[test]
fn counters_for_a_scripted_session() {
    let mut f = Fake::new();
    let mut c = cache(4);
    rd(&mut c, &mut f, 0, 4).unwrap(); // 4 misses, 1 device read, 4 insertions
    rd(&mut c, &mut f, 0, 4).unwrap(); // 4 hits, 4 promotions (4 sets, 1 each)
    c.write(&mut f, &[Blk([1; SECTOR_BYTES])], 2).unwrap(); // 1 update
    f.fail_write_after = Some(0);
    let _ = c.write(&mut f, &[Blk([2; SECTOR_BYTES])], 3); // 1 failure, 1 invalidation
    f.fail_write_after = None;
    f.fail_read_at = Some(9);
    let _ = rd(&mut c, &mut f, 9, 1); // miss + read failure
    f.fail_read_at = None;
    assert_eq!(
        c.stats(),
        SectorCacheStats {
            read_requests: 3,
            hits: 4,
            misses: 5,
            device_reads: 2,
            device_read_sectors: 5,
            insertions: 4,
            evictions: 0,
            promotions: 4,
            demotions: 0,
            invalidations: 1,
            read_failures: 1,
            write_requests: 2,
            write_updates: 1,
            write_failures: 1,
            bypassed: 0,
        }
    );
}

#[test]
fn rejects_malformed_backing() {
    let ok = |m: usize, d: usize| {
        SectorCache::new(vec![Meta::EMPTY; m], vec![[0u8; SECTOR_BYTES]; d]).is_some()
    };
    assert!(ok(8, 8));
    assert!(!ok(0, 0));
    assert!(!ok(6, 6), "not a whole number of sets");
    assert!(!ok(8, 4), "tags and contents differ");
}

#[test]
fn plan_stays_within_budget_with_every_field_counted() {
    for budget in [
        STANDARD_CACHE_BYTES,
        96 * 1024,
        LARGE_CACHE_BYTES,
        100_000,
        LARGE_CACHE_BYTES - 1,
    ] {
        let g = plan(budget).unwrap();
        let n = g.entries();
        assert_eq!(n % WAYS, 0);
        let raw = n * (SECTOR_BYTES + META_BYTES);
        assert!(raw <= g.charged_bytes().unwrap());
        assert!(g.charged_bytes().unwrap() <= budget, "budget {budget}");
        // no wasted set: one more set would not fit
        let bigger = pulp_board_logic::sd_cache::Geometry { sets: g.sets + 1 };
        assert!(bigger.charged_bytes().unwrap() > budget);
        assert!(
            n * SECTOR_BYTES * 100 / budget >= 95,
            "budget {budget}: {n} entries"
        );
    }
    assert!(plan(0).is_none());
    assert!(plan(SECTOR_BYTES).is_none());
    // absurd budgets are clamped, not overflowed
    let g = plan(usize::MAX).unwrap();
    assert!(g.charged_bytes().unwrap() <= pulp_board_logic::sd_cache::MAX_PLAN_BYTES);
}

#[test]
fn runtime_profile_picks_64_or_128_kib_and_never_exceeds_the_class_limit() {
    fn ready(bytes: usize) -> PsramStatus {
        PsramStatus::Ready { bytes }
    }
    assert_eq!(
        sd_cache_budget(ready(PSRAM_LARGE_MIN_BYTES)),
        LARGE_CACHE_BYTES
    );
    assert_eq!(
        sd_cache_budget(ready(2 * 1024 * 1024)),
        STANDARD_CACHE_BYTES
    );
    assert_eq!(sd_cache_budget(ready(1024 * 1024)), STANDARD_CACHE_BYTES);
    assert_eq!(sd_cache_budget(PsramStatus::NotInitialised), 0);
    assert_eq!(
        sd_cache_budget(PsramStatus::Degraded(PsramFault::NotDetected)),
        0
    );
    assert_eq!(
        sd_cache_budget(PsramStatus::Degraded(PsramFault::BadWindow)),
        0
    );
    for status in [
        ready(PSRAM_LARGE_MIN_BYTES),
        ready(2 * 1024 * 1024),
        ready(1024 * 1024),
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
    ] {
        let budget = sd_cache_budget(status);
        let limit = class_limit(status, Region::Psram, MemClass::StorageCache);
        assert!(budget <= limit, "{status:?}");
        assert!(budget <= PSRAM_STORAGE_CACHE_BYTES);
        if let Some(g) = plan(budget) {
            assert!(g.charged_bytes().unwrap() <= limit, "{status:?}");
            assert!(budget >= 64 * 1024);
        } else {
            assert_eq!(budget, 0);
        }
    }
}

#[test]
fn allocation_failure_means_no_cache_and_every_block_is_requested_for_the_plan() {
    let budget = LARGE_CACHE_BYTES;
    let n = plan(budget).unwrap().entries();

    let asked = std::cell::RefCell::new(Vec::new());
    let c: Option<Cache> = build_cache(
        budget,
        |len| {
            asked.borrow_mut().push(("meta", len));
            Ok::<_, ()>(vec![Meta::EMPTY; len])
        },
        |len| {
            asked.borrow_mut().push(("data", len));
            Ok::<_, ()>(vec![[0u8; SECTOR_BYTES]; len])
        },
    );
    assert_eq!(c.unwrap().capacity(), n);
    assert_eq!(*asked.borrow(), vec![("meta", n), ("data", n)]);

    // tag block refused: the contents block is never requested
    let mut data_asked = false;
    let c: Option<Cache> = build_cache(
        budget,
        |_| Err(()),
        |len| {
            data_asked = true;
            Ok(vec![[0u8; SECTOR_BYTES]; len])
        },
    );
    assert!(c.is_none() && !data_asked);

    // contents block refused
    let c: Option<Cache> = build_cache(budget, |len| Ok(vec![Meta::EMPTY; len]), |_| Err(()));
    assert!(c.is_none());

    // disabled budget: nothing requested
    let c: Option<Cache> = build_cache(
        0,
        |_| -> Result<Vec<Meta>, ()> { panic!("no allocation expected") },
        |_| -> Result<Vec<Sector>, ()> { panic!("no allocation expected") },
    );
    assert!(c.is_none());
}

#[test]
fn without_a_cache_the_device_sees_every_request_unchanged() {
    let mut d: CachedDevice<Fake, Vec<Meta>, Vec<Sector>> = CachedDevice::new(Fake::new(), None);
    let mut v = blocks(3);
    d.read(&mut v, 9).unwrap();
    d.read(&mut v, 9).unwrap();
    d.read(&mut [], 4).unwrap();
    d.write(&v, 12).unwrap();
    d.write(&[], 13).unwrap();
    assert_eq!(d.device().reads, vec![(9, 3), (9, 3), (4, 0)]);
    assert_eq!(d.device().writes, vec![(12, 3), (13, 0)]);
    assert!(d.stats().is_none() && d.cache().is_none());
}

#[test]
fn cached_device_routes_through_the_cache() {
    let cache_ = cache(4);
    let mut d = CachedDevice::new(Fake::new(), Some(cache_));
    let mut v = blocks(2);
    d.read(&mut v, 3).unwrap();
    d.read(&mut v, 3).unwrap();
    assert_eq!(d.device().reads, vec![(3, 2)]);
    assert_eq!(d.stats().unwrap().hits, 2);
    d.write(&v, 3).unwrap();
    assert_eq!(d.stats().unwrap().write_updates, 2);
    d.cache_mut().unwrap().invalidate_all();
    assert!(d.cache().unwrap().is_empty());
}

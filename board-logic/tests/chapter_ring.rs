// Chapter working set: keys, window, budget, warm jobs and stale publication.
use pulp_board_logic::chapter_ring::{
    ChapterKey, ChapterRing, HR8_BUDGET_BYTES, HR8_SINGLE_MAX_BYTES, Refusal, RingConfig,
    SMALL_SINGLE_MAX_BYTES, WarmError,
};
use pulp_board_logic::memory::{MIB, PsramFault, PsramStatus};
use pulp_board_logic::source_id::SourceId;

const BOOK_A: SourceId = SourceId::from_raw(0xA);
const BOOK_B: SourceId = SourceId::from_raw(0xB);

fn key(source: SourceId, chapter: u16, size: u32) -> ChapterKey {
    ChapterKey {
        source,
        chapter,
        offset: 1000 + u32::from(chapter) * 100_000,
        size,
    }
}

fn hr8() -> ChapterRing<Vec<u8>> {
    let mut ring = ChapterRing::new(RingConfig::HR8);
    ring.retarget(BOOK_A, 5);
    ring
}

#[test]
fn profile_is_hr8_only_on_a_validated_large_part() {
    let ready = |bytes| PsramStatus::Ready { bytes };
    assert_eq!(RingConfig::for_status(ready(8 * MIB)), RingConfig::HR8);
    for status in [
        ready(2 * MIB),
        ready(8 * MIB - 1),
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
    ] {
        assert_eq!(RingConfig::for_status(status), RingConfig::SMALL);
    }
    assert_eq!(RingConfig::SMALL.single_max, SMALL_SINGLE_MAX_BYTES);
    assert!(!RingConfig::SMALL.neighbors);
    assert_eq!(RingConfig::HR8.single_max, 256 * 1024);
    assert!(HR8_BUDGET_BYTES + 256 * 1024 <= MIB);
}

#[test]
fn same_sized_chapters_and_books_never_alias() {
    let mut ring = hr8();
    let k5 = key(BOOK_A, 5, 4000);
    let k6 = key(BOOK_A, 6, 4000);
    ring.insert(k5, vec![5; 4000]).unwrap();
    ring.insert(k6, vec![6; 4000]).unwrap();
    assert_eq!(ring.get(&k5).unwrap()[0], 5);
    assert_eq!(ring.get(&k6).unwrap()[0], 6);
    // same chapter and size, rebuilt cache file at another offset
    let moved = ChapterKey { offset: 7, ..k5 };
    assert!(ring.get(&moved).is_none());
    // same chapter number and size in another book
    assert!(ring.get(&key(BOOK_B, 5, 4000)).is_none());
    assert_eq!(
        ring.insert(key(BOOK_B, 5, 4000), vec![0; 4000])
            .unwrap_err()
            .0,
        Refusal::WrongSource
    );
}

#[test]
fn window_is_previous_current_next_and_slides() {
    let mut ring = hr8();
    for ch in [4u16, 5, 6] {
        ring.insert(key(BOOK_A, ch, 1000), vec![ch as u8; 1000])
            .unwrap();
    }
    assert_eq!(ring.used(), 3000);
    assert_eq!(
        ring.insert(key(BOOK_A, 7, 1000), vec![]).unwrap_err().0,
        Refusal::OutOfWindow
    );
    // forward: 4 leaves, 5 becomes the previous chapter, 6 is current
    ring.retarget(BOOK_A, 6);
    assert_eq!(ring.used(), 2000);
    assert!(ring.contains(&key(BOOK_A, 5, 1000)));
    assert!(ring.contains(&key(BOOK_A, 6, 1000)));
    assert!(!ring.contains(&key(BOOK_A, 4, 1000)));
    // a jump drops everything that is not adjacent
    ring.retarget(BOOK_A, 40);
    assert_eq!(ring.used(), 0);
    assert!(ring.is_empty());
}

#[test]
fn a_new_source_drops_everything() {
    let mut ring = hr8();
    ring.insert(key(BOOK_A, 5, 1000), vec![1; 1000]).unwrap();
    ring.retarget(BOOK_B, 5);
    assert_eq!(ring.used(), 0);
    assert!(ring.get(&key(BOOK_A, 5, 1000)).is_none());
    ring.insert(key(BOOK_B, 5, 1000), vec![2; 1000]).unwrap();
    assert_eq!(ring.get(&key(BOOK_B, 5, 1000)).unwrap()[0], 2);
}

#[test]
fn single_chapter_bound_and_cap_refusal() {
    let mut ring = hr8();
    let fits = key(BOOK_A, 5, HR8_SINGLE_MAX_BYTES as u32);
    let over = key(BOOK_A, 6, HR8_SINGLE_MAX_BYTES as u32 + 1);
    assert_eq!(ring.admit(&fits), Ok(()));
    assert_eq!(ring.admit(&over), Err(Refusal::TooLarge));
    assert_eq!(ring.admit(&key(BOOK_A, 6, 0)), Err(Refusal::Empty));
    let (r, back) = ring.insert(over, vec![9]).unwrap_err();
    assert_eq!((r, back), (Refusal::TooLarge, vec![9]));
    assert!(ring.is_empty());
    ring.insert(fits, vec![0; 1]).unwrap();
    assert_eq!(ring.used(), HR8_SINGLE_MAX_BYTES);
}

#[test]
fn budget_is_a_byte_sum_and_the_current_chapter_outranks_neighbors() {
    // budget for two full-size chapters, not three
    let cfg = RingConfig {
        budget: 2 * 1000,
        single_max: 1000,
        neighbors: true,
    };
    let mut ring: ChapterRing<u8> = ChapterRing::new(cfg);
    ring.retarget(BOOK_A, 5);
    ring.insert(key(BOOK_A, 6, 1000), 6).unwrap();
    ring.insert(key(BOOK_A, 4, 1000), 4).unwrap();
    assert_eq!(ring.used(), 2000);
    // current does not fit beside two neighbors: next goes first, then previous
    ring.insert(key(BOOK_A, 5, 1000), 5).unwrap();
    assert_eq!(ring.used(), 2000);
    assert!(ring.contains(&key(BOOK_A, 5, 1000)));
    assert!(ring.contains(&key(BOOK_A, 4, 1000)));
    assert!(!ring.contains(&key(BOOK_A, 6, 1000)));
    // a neighbor never evicts anything
    assert_eq!(
        ring.insert(key(BOOK_A, 6, 1000), 6).unwrap_err().0,
        Refusal::OverBudget
    );
    assert_eq!(ring.used(), 2000);
}

#[test]
fn small_profile_keeps_only_the_current_chapter() {
    let mut ring: ChapterRing<u8> = ChapterRing::new(RingConfig::SMALL);
    ring.retarget(BOOK_A, 5);
    assert_eq!(
        ring.insert(key(BOOK_A, 6, 100), 0).unwrap_err().0,
        Refusal::NeighborsDisabled
    );
    ring.insert(key(BOOK_A, 5, SMALL_SINGLE_MAX_BYTES as u32), 5)
        .unwrap();
    assert_eq!(
        ring.admit(&key(BOOK_A, 5, SMALL_SINGLE_MAX_BYTES as u32 + 1)),
        Err(Refusal::TooLarge)
    );
    // moving on drops the old chapter even though the budget would be free
    ring.retarget(BOOK_A, 6);
    assert_eq!(ring.used(), 0);
    ring.insert(key(BOOK_A, 6, SMALL_SINGLE_MAX_BYTES as u32), 6)
        .unwrap();
    assert!(ring.next_warm(10, |c| Some(key(BOOK_A, c, 10))).is_none());
}

fn sizes(c: u16) -> Option<ChapterKey> {
    Some(key(BOOK_A, c, 40_000 + u32::from(c)))
}

#[test]
fn warm_reads_next_then_previous_in_bounded_steps_and_publishes_when_complete() {
    let mut ring = hr8();
    ring.insert(key(BOOK_A, 5, 40_005), vec![5; 1]).unwrap();
    let mut job = ring.next_warm(20, sizes).unwrap();
    assert_eq!(job.key().chapter, 6);
    let mut steps = 0;
    let mut got = vec![0u8; 0];
    while let Some((off, len)) = job.next_read(16 * 1024) {
        assert!(len <= 16 * 1024);
        assert_eq!(off as usize, got.len());
        got.resize(got.len() + len, 6);
        let done = job.record(len, len).unwrap();
        steps += 1;
        assert_eq!(done, job.is_complete());
    }
    assert_eq!(steps, 3);
    assert_eq!(got.len(), 40_006);
    // not resident until published
    assert!(!ring.contains(job.key()));
    ring.publish(&job, got).unwrap();
    assert!(ring.contains(job.key()));
    // the next warm target is the previous chapter
    assert_eq!(ring.next_warm(20, sizes).unwrap().key().chapter, 4);
}

#[test]
fn warm_skips_missing_resident_oversized_and_out_of_range_chapters() {
    let mut ring = hr8();
    // next not in the cache yet: previous is chosen
    let job = ring
        .next_warm(20, |c| (c != 6).then(|| key(BOOK_A, c, 100)))
        .unwrap();
    assert_eq!(job.key().chapter, 4);
    // next too large, previous resident
    ring.insert(key(BOOK_A, 4, 100), vec![]).unwrap();
    let big = |c| Some(key(BOOK_A, c, HR8_SINGLE_MAX_BYTES as u32 + 1));
    assert!(ring.next_warm(20, big).is_none());
    // the last chapter has no next
    ring.retarget(BOOK_A, 19);
    assert!(
        ring.next_warm(20, |c| Some(key(BOOK_A, c, 100)))
            .unwrap()
            .key()
            .chapter
            == 18
    );
    ring.retarget(BOOK_A, 0);
    assert_eq!(
        ring.next_warm(20, |c| Some(key(BOOK_A, c, 100)))
            .unwrap()
            .key()
            .chapter,
        1
    );
    assert!(ring.next_warm(1, |c| Some(key(BOOK_A, c, 100))).is_none());
}

#[test]
fn incomplete_and_short_jobs_never_publish() {
    let mut ring = hr8();
    let mut job = ring.next_warm(20, sizes).unwrap();
    let (_, len) = job.next_read(1000).unwrap();
    assert_eq!(job.record(len, len), Ok(false));
    assert_eq!(
        ring.publish(&job, vec![0; 1000]).unwrap_err().0,
        Refusal::Incomplete
    );
    // the device returns nothing: an error, not a silent end
    assert_eq!(job.record(0, 100), Err(WarmError::ShortRead));
    assert_eq!(job.record(101, 100), Err(WarmError::Overrun));
    assert_eq!(job.filled(), 1000);
    assert!(ring.is_empty());
}

#[test]
fn a_job_from_before_navigation_or_a_book_change_is_stale() {
    let mut ring = hr8();
    let finish = |job: &mut pulp_board_logic::chapter_ring::WarmJob| {
        let n = job.key().size as usize;
        job.record(n, n).unwrap();
    };
    // window moved
    let mut job = ring.next_warm(20, sizes).unwrap();
    finish(&mut job);
    ring.retarget(BOOK_A, 6);
    assert_eq!(ring.publish(&job, vec![1]).unwrap_err().0, Refusal::Stale);
    // book replaced under the same chapter number
    let mut job = ring.next_warm(20, sizes).unwrap();
    finish(&mut job);
    ring.retarget(BOOK_B, 6);
    assert_eq!(ring.publish(&job, vec![1]).unwrap_err().0, Refusal::Stale);
    // cleared (reader reset)
    ring.retarget(BOOK_A, 6);
    let mut job = ring.next_warm(20, sizes).unwrap();
    finish(&mut job);
    ring.clear();
    ring.retarget(BOOK_A, 6);
    assert_eq!(ring.publish(&job, vec![1]).unwrap_err().0, Refusal::Stale);
    assert!(ring.is_empty());
    // a job begun after the last change publishes
    let mut job = ring.next_warm(20, sizes).unwrap();
    finish(&mut job);
    ring.publish(&job, vec![1]).unwrap();
}

#[test]
fn take_returns_the_payload_and_releases_the_bytes() {
    let mut ring = hr8();
    let k = key(BOOK_A, 5, 10);
    ring.insert(k, vec![7; 10]).unwrap();
    assert_eq!(ring.take(&key(BOOK_A, 5, 11)), None);
    assert_eq!(ring.take(&k), Some(vec![7; 10]));
    assert_eq!(ring.used(), 0);
    assert!(ring.take(&k).is_none());
}

#[test]
fn replacing_a_chapter_of_another_key_swaps_it_and_keeps_the_sum_exact() {
    let mut ring = hr8();
    ring.insert(key(BOOK_A, 5, 100), vec![1; 100]).unwrap();
    ring.insert(key(BOOK_A, 5, 300), vec![2; 300]).unwrap();
    assert_eq!(ring.used(), 300);
    assert_eq!(ring.len(), 1);
    assert!(ring.get(&key(BOOK_A, 5, 100)).is_none());
}

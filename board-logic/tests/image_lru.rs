// Decoded-image LRU: identity, eviction order, budget and disabled profiles.
use pulp_board_logic::image_lru::{ImageKey, ImageLru, LruConfig, LruRefusal, MAX_ENTRIES};
use pulp_board_logic::memory::{MIB, PsramFault, PsramStatus};
use pulp_board_logic::source_id::SourceId;

const BOOK: SourceId = SourceId::from_raw(1);

fn key(path: u64, w: u16, h: u16) -> ImageKey {
    ImageKey {
        source: BOOK,
        path,
        max_w: w,
        max_h: h,
    }
}

fn cache(budget: usize, item_max: usize) -> ImageLru<u32> {
    let mut lru = ImageLru::new(LruConfig { budget, item_max });
    lru.retarget(BOOK);
    lru
}

#[test]
fn only_a_validated_large_part_enables_the_cache() {
    assert_eq!(
        LruConfig::for_status(PsramStatus::Ready { bytes: 8 * MIB }),
        LruConfig::HR8
    );
    for status in [
        PsramStatus::Ready { bytes: 2 * MIB },
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
    ] {
        assert_eq!(LruConfig::for_status(status), LruConfig::OFF);
    }
    let mut off: ImageLru<u32> = ImageLru::new(LruConfig::OFF);
    off.retarget(BOOK);
    assert_eq!(
        off.insert(key(1, 1, 1), 10, 0).unwrap_err().0,
        LruRefusal::Disabled
    );
    // a full-screen 800 x 480 bitmap fits the HR8 item bound
    assert!(48_000 <= LruConfig::HR8.item_max);
}

#[test]
fn key_covers_source_path_and_actual_decode_geometry() {
    let mut lru = cache(1000, 1000);
    lru.insert(key(1, 784, 192), 100, 1).unwrap();
    lru.insert(key(1, 784, 464), 100, 2).unwrap();
    lru.insert(key(2, 784, 192), 100, 3).unwrap();
    assert_eq!(lru.get(&key(1, 784, 192)), Some(&1));
    assert_eq!(lru.get(&key(1, 784, 464)), Some(&2));
    assert_eq!(lru.get(&key(2, 784, 192)), Some(&3));
    // another width, e.g. after a theme change, is a miss
    assert_eq!(lru.get(&key(1, 700, 192)), None);
    // another book is a miss even for the same path hash
    let other = ImageKey {
        source: SourceId::from_raw(2),
        ..key(1, 784, 192)
    };
    assert_eq!(lru.get(&other), None);
    assert_eq!(
        lru.insert(other, 10, 9).unwrap_err().0,
        LruRefusal::WrongSource
    );
}

#[test]
fn least_recently_used_goes_first_and_get_refreshes() {
    let mut lru = cache(300, 100);
    for p in 1..=3 {
        lru.insert(key(p, 1, 1), 100, p as u32).unwrap();
    }
    assert_eq!(lru.used(), 300);
    // touch 1: 2 is now the oldest
    assert_eq!(lru.get(&key(1, 1, 1)), Some(&1));
    assert_eq!(lru.insert(key(4, 1, 1), 100, 4), Ok(1));
    assert!(lru.contains(&key(1, 1, 1)));
    assert!(!lru.contains(&key(2, 1, 1)));
    assert!(lru.contains(&key(3, 1, 1)));
    assert!(lru.contains(&key(4, 1, 1)));
    assert_eq!(lru.used(), 300);
}

#[test]
fn a_big_insert_evicts_several_and_an_impossible_one_changes_nothing() {
    let mut lru = cache(300, 300);
    for p in 1..=3 {
        lru.insert(key(p, 1, 1), 100, p as u32).unwrap();
    }
    assert_eq!(lru.insert(key(9, 1, 1), 250, 9), Ok(3));
    assert_eq!(lru.len(), 1);
    assert_eq!(lru.used(), 250);
    let (why, back) = lru.insert(key(10, 1, 1), 301, 10).unwrap_err();
    assert_eq!((why, back), (LruRefusal::TooLarge, 10));
    assert_eq!(lru.used(), 250);
    assert!(lru.contains(&key(9, 1, 1)));
    assert_eq!(
        lru.insert(key(11, 1, 1), 0, 0).unwrap_err().0,
        LruRefusal::Empty
    );
}

#[test]
fn item_bound_is_enforced_below_the_budget() {
    let mut lru = cache(1000, 100);
    assert_eq!(
        lru.insert(key(1, 1, 1), 101, 0).unwrap_err().0,
        LruRefusal::TooLarge
    );
    assert!(lru.is_empty());
}

#[test]
fn entry_count_is_bounded_and_reinserting_a_key_replaces_it() {
    let mut lru = cache(1_000_000, 1000);
    for p in 0..MAX_ENTRIES as u64 + 3 {
        lru.insert(key(p, 1, 1), 10, p as u32).unwrap();
    }
    assert_eq!(lru.len(), MAX_ENTRIES);
    assert!(!lru.contains(&key(0, 1, 1)));
    assert!(lru.contains(&key(MAX_ENTRIES as u64 + 2, 1, 1)));
    let before = lru.used();
    lru.insert(key(5, 1, 1), 40, 55).unwrap();
    assert_eq!(lru.len(), MAX_ENTRIES);
    assert_eq!(lru.used(), before - 10 + 40);
    assert_eq!(lru.get(&key(5, 1, 1)), Some(&55));
}

#[test]
fn another_book_empties_the_cache() {
    let mut lru = cache(1000, 1000);
    lru.insert(key(1, 1, 1), 100, 1).unwrap();
    lru.retarget(BOOK);
    assert_eq!(lru.len(), 1);
    lru.retarget(SourceId::from_raw(2));
    assert!(lru.is_empty());
    assert_eq!(lru.used(), 0);
}

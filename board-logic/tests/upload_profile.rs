// Wi-Fi upload buffer profile and write-batching rules (the production
// `pulp_board_logic::upload`), against the memory budget of each part. Expected
// sizes come from the Task 4 contract (RX 16-32 KiB, TX 4-8 KiB, work 16-32 KiB
// on HR8; the small profile everywhere else), not from the implementation.

use pulp_board_logic::memory::{
    MemClass, MemError, MemoryBudget, PSRAM_HW_BYTES, PSRAM_LARGE_MIN_BYTES, PSRAM_MAX_BYTES,
    PsramFault, PsramStatus, Region,
};
use pulp_board_logic::upload::{
    FLUSH_SLICE_BYTES, HTTP_FIXED_MAX_BYTES, MIN_WORK_BYTES, NetProfile, acquire, next_file_len,
    profile_order, sector_aligned,
};

const KIB: usize = 1024;

fn hr8() -> PsramStatus {
    PsramStatus::Ready {
        bytes: PSRAM_MAX_BYTES,
    }
}

fn hr2() -> PsramStatus {
    PsramStatus::Ready {
        bytes: PSRAM_HW_BYTES,
    }
}

#[test]
fn profiles_have_the_contract_sizes() {
    assert_eq!(
        NetProfile::SMALL,
        NetProfile {
            rx: 2048,
            tx: 1536,
            work: 2048
        }
    );
    assert_eq!(NetProfile::LARGE.rx, 16 * KIB);
    assert_eq!(NetProfile::LARGE.tx, 4 * KIB);
    assert_eq!(NetProfile::LARGE.work, 16 * KIB);
    assert_eq!(NetProfile::LARGE.total(), 36 * KIB);
    assert_eq!(NetProfile::SMALL.total(), 5632);
    assert!((16 * KIB..=32 * KIB).contains(&NetProfile::LARGE.rx));
    assert!((4 * KIB..=8 * KIB).contains(&NetProfile::LARGE.tx));
    assert!((16 * KIB..=32 * KIB).contains(&NetProfile::LARGE.work));
}

#[test]
fn only_an_hr8_part_prefers_the_large_profile() {
    assert_eq!(
        profile_order(hr8()),
        &[NetProfile::LARGE, NetProfile::SMALL]
    );
    assert_eq!(
        profile_order(PsramStatus::Ready {
            bytes: PSRAM_LARGE_MIN_BYTES
        }),
        &[NetProfile::LARGE, NetProfile::SMALL]
    );
    for status in [
        PsramStatus::Ready {
            bytes: PSRAM_LARGE_MIN_BYTES - 1,
        },
        hr2(),
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
    ] {
        assert_eq!(profile_order(status), &[NetProfile::SMALL], "{status:?}");
    }
}

#[test]
fn acquire_takes_the_first_profile_that_is_granted_and_asks_once() {
    let mut asked = Vec::new();
    let (profile, token) = acquire(hr8(), |p| {
        asked.push(p);
        Ok::<_, ()>(p.total())
    })
    .unwrap();
    assert_eq!(profile, NetProfile::LARGE);
    assert_eq!(token, NetProfile::LARGE.total());
    assert_eq!(asked, vec![NetProfile::LARGE]);
}

#[test]
fn a_refused_large_block_falls_back_to_the_small_one() {
    let mut asked = Vec::new();
    let (profile, ()) = acquire(hr8(), |p| {
        asked.push(p);
        if p == NetProfile::LARGE {
            Err("oom")
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(profile, NetProfile::SMALL);
    assert_eq!(asked, vec![NetProfile::LARGE, NetProfile::SMALL]);
}

#[test]
fn a_session_fails_only_when_the_small_block_is_refused_too() {
    let mut asked = 0;
    let err = acquire(hr8(), |p| {
        asked += 1;
        Err::<(), _>(if p == NetProfile::LARGE {
            "large"
        } else {
            "small"
        })
    })
    .unwrap_err();
    assert_eq!(asked, 2);
    assert_eq!(err, "small", "the error of the last attempt");

    let mut asked = 0;
    let err = acquire(PsramStatus::NotInitialised, |_| {
        asked += 1;
        Err::<(), _>("oom")
    })
    .unwrap_err();
    assert_eq!((asked, err), (1, "oom"));
}

// one block per profile, charged to NetScratch like the firmware does
fn reserve_block(budget: &mut MemoryBudget, p: NetProfile) -> Result<(), MemError> {
    budget
        .reserve(MemClass::NetScratch, HTTP_FIXED_MAX_BYTES + p.total(), 16)
        .map(drop)
}

#[test]
fn each_part_gets_the_profile_its_net_scratch_limit_admits() {
    // HR8: the class admits the large block, even with the fixed part at its cap
    let mut b = MemoryBudget::for_build(true);
    b.set_status(hr8()).unwrap();
    let (p, ()) = acquire(hr8(), |p| reserve_block(&mut b, p)).unwrap();
    assert_eq!(p, NetProfile::LARGE);

    // HR2: the large block exceeds the 32 KiB class and is refused; small fits
    let mut b = MemoryBudget::for_build(true);
    b.set_status(hr2()).unwrap();
    assert!(matches!(
        reserve_block(&mut b, NetProfile::LARGE),
        Err(MemError::ClassLimit { .. })
    ));
    let (p, ()) = acquire(hr2(), |p| reserve_block(&mut b, p)).unwrap();
    assert_eq!(p, NetProfile::SMALL);

    // degraded PSRAM: the internal class limit still admits the small block
    let degraded = PsramStatus::Degraded(PsramFault::NotDetected);
    let mut b = MemoryBudget::for_build(true);
    b.set_status(degraded).unwrap();
    assert_eq!(
        b.class_limit(Region::Internal, MemClass::NetScratch),
        16 * KIB
    );
    let (p, ()) = acquire(degraded, |p| reserve_block(&mut b, p)).unwrap();
    assert_eq!(p, NetProfile::SMALL);
}

#[test]
fn work_buffers_hold_the_delimiter_holdback_and_two_sectors() {
    // CRLF-- + the longest accepted boundary (120) + the two deciding bytes
    assert_eq!(MIN_WORK_BYTES, 2 * 512 + 120 + 4 + 2);
    for p in [NetProfile::SMALL, NetProfile::LARGE] {
        assert!(p.work >= MIN_WORK_BYTES, "{p:?}");
        // a full window always has at least one whole sector of payload
        assert!(sector_aligned(p.work - (120 + 4)) >= 512, "{p:?}");
    }
}

#[test]
fn flush_slices_are_whole_sectors_of_at_most_4_kib() {
    assert_eq!(FLUSH_SLICE_BYTES, 4096);
    assert_eq!(FLUSH_SLICE_BYTES % 512, 0);
}

#[test]
fn sector_aligned_rounds_down_to_whole_sectors() {
    for (n, want) in [
        (0, 0),
        (1, 0),
        (511, 0),
        (512, 512),
        (513, 512),
        (1023, 512),
        (1024, 1024),
        (4096 + 511, 4096),
    ] {
        assert_eq!(sector_aligned(n), want, "{n}");
    }
}

#[test]
fn file_length_never_passes_the_fat_bound() {
    assert_eq!(next_file_len(0, 0, u32::MAX), Some(0));
    assert_eq!(next_file_len(0, 4096, u32::MAX), Some(4096));
    assert_eq!(next_file_len(u32::MAX - 10, 10, u32::MAX), Some(u32::MAX));
    assert_eq!(next_file_len(u32::MAX - 10, 11, u32::MAX), None);
    assert_eq!(next_file_len(u32::MAX, 1, u32::MAX), None);
    // a request that does not even fit u32 (64-bit hosts)
    #[cfg(target_pointer_width = "64")]
    assert_eq!(next_file_len(0, u32::MAX as usize + 1, u32::MAX), None);
    // a lower card-specific bound
    assert_eq!(next_file_len(90, 10, 100), Some(100));
    assert_eq!(next_file_len(91, 10, 100), None);
}

use pulp_board_logic::memory::{
    ALLOC_GRANULE, C61_RAM_LEN, INTERNAL_HEAP_MAIN_BYTES_WIFI, INTERNAL_HEAP_RECLAIMED_BYTES,
    MemClass, MemError, MemoryBudget, PsramFault, PsramStatus, Region, STACK_MIN_BYTES,
    STATIC_RAM_MAX_BYTES, internal_heap_bytes, internal_heap_main_bytes,
};

// Measured statics of the C61 firmware (specs/changes/archive/onepage-wifi-upload/baseline.md).
const OFFLINE_STATICS_BYTES: usize = 180_932;
const WIFI_STATICS_EXTRA_BYTES: usize = 68_340;
const OFFLINE_HEAP_MAIN_BYTES: usize = 96 * 1024;

/// Statics of the wifi build once its main heap is shrunk. The measured wifi
/// statics still contain the full 96 KiB offline main heap.
fn wifi_statics_with_shrunk_heap() -> usize {
    OFFLINE_STATICS_BYTES + WIFI_STATICS_EXTRA_BYTES
        - (OFFLINE_HEAP_MAIN_BYTES - INTERNAL_HEAP_MAIN_BYTES_WIFI)
}

#[test]
fn offline_budget_is_unchanged() {
    assert_eq!(internal_heap_main_bytes(false), 96 * 1024);
    assert_eq!(STACK_MIN_BYTES, 48 * 1024);
    assert_eq!(INTERNAL_HEAP_RECLAIMED_BYTES, 64_000);
}

#[test]
fn wifi_variant_main_heap_is_52_kib() {
    assert_eq!(INTERNAL_HEAP_MAIN_BYTES_WIFI, 52 * 1024);
    assert_eq!(
        internal_heap_main_bytes(true),
        INTERNAL_HEAP_MAIN_BYTES_WIFI
    );
}

#[test]
fn internal_heap_is_main_plus_reclaimed_for_both_variants() {
    for wifi in [false, true] {
        assert_eq!(
            internal_heap_bytes(wifi),
            internal_heap_main_bytes(wifi) + INTERNAL_HEAP_RECLAIMED_BYTES,
            "wifi = {wifi}"
        );
    }
}

#[test]
fn wifi_statics_with_shrunk_heap_fit_the_static_budget() {
    assert!(
        wifi_statics_with_shrunk_heap() <= STATIC_RAM_MAX_BYTES,
        "statics {} exceed budget {}",
        wifi_statics_with_shrunk_heap(),
        STATIC_RAM_MAX_BYTES
    );
}

#[test]
fn wifi_stack_headroom_meets_the_minimum() {
    let stack_headroom = C61_RAM_LEN - wifi_statics_with_shrunk_heap();
    assert!(
        stack_headroom >= STACK_MIN_BYTES,
        "stack headroom {stack_headroom} below minimum {STACK_MIN_BYTES}"
    );
}

#[test]
fn wifi_without_heap_shrink_would_exceed_the_static_budget() {
    // Control: proves the shrink is necessary and the budget check is not vacuous.
    let unshrunk = OFFLINE_STATICS_BYTES + WIFI_STATICS_EXTRA_BYTES;
    assert!(
        unshrunk > STATIC_RAM_MAX_BYTES,
        "unshrunk wifi statics {unshrunk} unexpectedly fit budget {STATIC_RAM_MAX_BYTES}"
    );
}

#[test]
fn wifi_main_heap_is_smaller_and_granule_aligned() {
    assert!(INTERNAL_HEAP_MAIN_BYTES_WIFI < internal_heap_main_bytes(false));
    assert_eq!(INTERNAL_HEAP_MAIN_BYTES_WIFI % ALLOC_GRANULE, 0);
}

// Capacity expectations come from the build contract, independently of the
// helper functions whose wiring these reservations exercise.
#[test]
fn build_budget_enforces_aggregate_internal_capacity_across_classes() {
    for status in [
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
        PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        },
    ] {
        for (wifi, capacity) in [(true, 52 * 1024 + 64_000), (false, 96 * 1024 + 64_000)] {
            let ctx = format!("wifi {wifi}, status {status:?}");
            let mut budget = MemoryBudget::for_build(wifi);
            budget.set_status(status).unwrap();
            assert_eq!(
                budget.pool_limit(Region::Internal),
                capacity,
                "{ctx}: capacity"
            );
            let mut remaining = capacity;
            let mut reservations = Vec::new();
            for class in MemClass::ALL {
                let bytes = budget.class_limit(Region::Internal, class).min(remaining);
                if bytes == 0 {
                    continue;
                }
                let reservation = budget
                    .reserve_in(Region::Internal, class, bytes, 16)
                    .unwrap();
                assert_eq!(reservation.region, Region::Internal, "{ctx}: region");
                reservations.push((class, bytes, reservation));
                remaining -= bytes;
                if remaining == 0 {
                    break;
                }
            }
            assert_eq!(remaining, 0, "{ctx}: classes must permit exact pool fit");
            assert!(
                reservations.len() > 1,
                "{ctx}: boundary must span multiple classes"
            );
            assert_eq!(
                budget.pool_used(Region::Internal),
                capacity,
                "{ctx}: exact fit"
            );
            let spare_class = MemClass::ALL
                .into_iter()
                .find(|&class| {
                    budget
                        .class_limit(Region::Internal, class)
                        .saturating_sub(budget.used(Region::Internal, class))
                        >= 16
                })
                .expect("at least one class must have room beyond the pool capacity");
            let before: Vec<_> = MemClass::ALL
                .into_iter()
                .map(|class| budget.used(Region::Internal, class))
                .collect();
            assert!(
                matches!(
                    budget.reserve_in(Region::Internal, spare_class, 16, 16),
                    Err(MemError::PoolExhausted { .. })
                ),
                "{ctx}: reject beyond aggregate pool"
            );
            assert_eq!(
                budget.pool_used(Region::Internal),
                capacity,
                "{ctx}: rejected allocation accounting"
            );
            let after: Vec<_> = MemClass::ALL
                .into_iter()
                .map(|class| budget.used(Region::Internal, class))
                .collect();
            assert_eq!(after, before, "{ctx}: rejection changed class accounting");
            let (class, bytes, reservation) = reservations.pop().unwrap();
            budget.release(reservation).unwrap();
            assert_eq!(
                budget.pool_used(Region::Internal),
                capacity - bytes,
                "{ctx}: release accounting"
            );
            let refill = budget
                .reserve_in(Region::Internal, class, bytes, 16)
                .unwrap();
            assert_eq!(
                budget.pool_used(Region::Internal),
                capacity,
                "{ctx}: refill exact fit"
            );
            budget.release(refill).unwrap();
            for (_, _, reservation) in reservations {
                budget.release(reservation).unwrap();
            }
            assert_eq!(
                budget.pool_used(Region::Internal),
                0,
                "{ctx}: released all reservations"
            );
        }
    }
}

#[test]
fn offline_budget_accepts_internal_reservations_above_wifi_ceiling() {
    let wifi_ceiling = 52 * 1024 + 64_000;
    let target = wifi_ceiling + 16;
    let mut budget = MemoryBudget::for_build(false);
    let mut remaining = target;
    let mut classes = 0;
    for class in MemClass::ALL {
        let bytes = budget.class_limit(Region::Internal, class).min(remaining);
        if bytes == 0 {
            continue;
        }
        budget
            .reserve_in(Region::Internal, class, bytes, 16)
            .unwrap();
        remaining -= bytes;
        classes += 1;
        if remaining == 0 {
            break;
        }
    }
    assert_eq!(remaining, 0);
    assert!(classes > 1);
    assert_eq!(budget.pool_used(Region::Internal), target);
    assert!(budget.pool_used(Region::Internal) > wifi_ceiling);
}

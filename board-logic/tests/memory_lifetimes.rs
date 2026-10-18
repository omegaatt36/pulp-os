use pulp_board_logic::memory::{
    AllocationKind as Kind, KIB, MIB, MemClass, MemoryBudget, PsramStatus, Region,
};

fn budget() -> MemoryBudget {
    let mut b = MemoryBudget::new();
    b.set_status(PsramStatus::Ready { bytes: 8 * MIB }).unwrap();
    b
}

#[test]
fn shared_cache_borrows_unused_class_targets_without_consuming_scratch() {
    let mut b = budget();
    let text = b
        .reserve_in_with_kind(
            Region::Psram,
            MemClass::ChapterText,
            Kind::Cache,
            3 * MIB,
            16,
        )
        .unwrap();
    let images = b
        .reserve_in_with_kind(Region::Psram, MemClass::ImageData, Kind::Cache, 2 * MIB, 16)
        .unwrap();
    assert!(
        b.reserve_in_with_kind(Region::Psram, MemClass::StorageCache, Kind::Cache, 16, 16)
            .is_err()
    );
    let work = b
        .reserve_in_with_kind(
            Region::Psram,
            MemClass::ImageData,
            Kind::Scratch,
            1500 * KIB,
            16,
        )
        .unwrap();
    let page = b
        .reserve_in_with_kind(Region::Psram, MemClass::FontGlyphs, Kind::Resident, MIB, 16)
        .unwrap();
    for r in [text, images, work, page] {
        b.release(r).unwrap();
    }
    assert_eq!(b.pool_used(Region::Psram), 0);
    for kind in Kind::ALL {
        assert_eq!(b.kind_used(kind), 0);
    }
}

#[test]
fn working_set_limit_is_independent_of_reclaimable_cache() {
    let mut b = budget();
    let resident = b
        .reserve_in_with_kind(Region::Psram, MemClass::FontGlyphs, Kind::Resident, MIB, 16)
        .unwrap();
    assert!(
        b.reserve_in_with_kind(Region::Psram, MemClass::ZipToc, Kind::Resident, 16, 16)
            .is_err()
    );
    assert!(
        b.reserve_in_with_kind(
            Region::Psram,
            MemClass::ImageData,
            Kind::Scratch,
            2 * MIB,
            16
        )
        .is_err()
    );
    let cached = b
        .reserve_in_with_kind(Region::Psram, MemClass::FontGlyphs, Kind::Cache, MIB, 16)
        .unwrap();
    b.release(cached).unwrap();
    b.release(resident).unwrap();
}

#[test]
fn lifetime_selection_never_allows_dma_isr_or_runtime_in_psram() {
    let mut b = budget();
    for class in [MemClass::DmaBuffer, MemClass::IsrData, MemClass::Runtime] {
        for kind in Kind::ALL {
            assert!(matches!(
                b.reserve_in_with_kind(Region::Psram, class, kind, 4096, 16),
                Err(pulp_board_logic::memory::MemError::RegionForbidden { .. })
            ));
            assert_eq!(b.kind_used(kind), 0);
        }
    }
}

#[test]
fn scratch_diagnostics_follow_the_physical_heap_on_small_and_large_parts() {
    use pulp_board_logic::memory::{HeapKind, external_heap_kind};
    assert_eq!(external_heap_kind(Kind::Scratch, false), HeapKind::Psram);
    assert_eq!(external_heap_kind(Kind::Scratch, true), HeapKind::Scratch);
    for kind in [Kind::Resident, Kind::Cache] {
        for isolated in [false, true] {
            assert_eq!(external_heap_kind(kind, isolated), HeapKind::Psram);
        }
    }
}

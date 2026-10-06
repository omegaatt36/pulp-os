use pulp_board_logic::memory::{
    ExternalClass, INTERNAL_FONT_GLYPHS_BYTES, MemClass, MemError, MemoryBudget,
    PSRAM_CHAPTER_TEXT_BYTES, PSRAM_FONT_GLYPHS_BYTES, PSRAM_HW_BYTES, PSRAM_IMAGE_DATA_BYTES,
    PSRAM_PAGE_TABLE_BYTES, PSRAM_RESERVE_BYTES, PSRAM_ZIP_TOC_BYTES, PsramFault, PsramStatus,
    Region,
};

#[test]
fn ready_font_storage_uses_psram_with_a_shared_class_cap() {
    let mut budget = MemoryBudget::new();
    budget
        .set_status(PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        })
        .unwrap();
    assert_eq!(
        MemClass::from(ExternalClass::FontGlyphs),
        MemClass::FontGlyphs
    );
    assert_eq!(
        MemClass::FontGlyphs.external(),
        Some(ExternalClass::FontGlyphs)
    );
    assert!(MemClass::ALL.contains(&MemClass::FontGlyphs));
    assert_eq!(budget.region_for(MemClass::FontGlyphs), Region::Psram);
    assert_eq!(
        budget.class_limit(Region::Psram, MemClass::FontGlyphs),
        256 * 1024
    );

    let first = budget
        .reserve(MemClass::FontGlyphs, 128 * 1024, 16)
        .unwrap();
    let second = budget
        .reserve(MemClass::FontGlyphs, 128 * 1024, 16)
        .unwrap();
    assert_eq!(first.region, Region::Psram);
    assert_eq!(second.region, Region::Psram);
    assert_eq!(budget.used(Region::Psram, MemClass::FontGlyphs), 256 * 1024);
    assert_eq!(budget.used(Region::Internal, MemClass::FontGlyphs), 0);
    assert_eq!(
        budget.reserve(MemClass::FontGlyphs, 16, 16),
        Err(MemError::ClassLimit {
            class: MemClass::FontGlyphs,
            region: Region::Psram,
            limit: 256 * 1024,
            used: 256 * 1024,
            requested: 16,
        })
    );
    assert_eq!(budget.pool_used(Region::Psram), 256 * 1024);

    budget.release(first).unwrap();
    budget.release(second).unwrap();
    assert_eq!(budget.pool_used(Region::Psram), 0);
    let retry = budget
        .reserve(MemClass::FontGlyphs, 256 * 1024, 16)
        .unwrap();
    budget.release(retry).unwrap();
    assert_eq!(budget.used(Region::Psram, MemClass::FontGlyphs), 0);
}

#[test]
fn degraded_font_storage_refuses_over_budget_then_recovers_after_release() {
    let mut budget = MemoryBudget::new();
    budget
        .set_status(PsramStatus::Degraded(PsramFault::NotDetected))
        .unwrap();
    assert_eq!(budget.region_for(MemClass::FontGlyphs), Region::Internal);
    assert_eq!(
        budget.class_limit(Region::Internal, MemClass::FontGlyphs),
        16 * 1024
    );
    assert_eq!(budget.class_limit(Region::Psram, MemClass::FontGlyphs), 0);

    let page = budget.reserve(MemClass::FontGlyphs, 16 * 1024, 16).unwrap();
    assert_eq!(page.region, Region::Internal);
    assert_eq!(
        budget.reserve(MemClass::FontGlyphs, 16, 16),
        Err(MemError::ClassLimit {
            class: MemClass::FontGlyphs,
            region: Region::Internal,
            limit: 16 * 1024,
            used: 16 * 1024,
            requested: 16,
        })
    );
    assert_eq!(
        budget.used(Region::Internal, MemClass::FontGlyphs),
        16 * 1024
    );
    assert_eq!(budget.pool_used(Region::Internal), 16 * 1024);
    budget.release(page).unwrap();
    assert_eq!(budget.pool_used(Region::Internal), 0);
    let retry = budget.reserve(MemClass::FontGlyphs, 16 * 1024, 16).unwrap();
    budget.release(retry).unwrap();
    assert_eq!(budget.used(Region::Internal, MemClass::FontGlyphs), 0);
}

#[test]
fn font_allowance_is_funded_from_reserve_without_shrinking_existing_classes() {
    assert_eq!(PSRAM_HW_BYTES, 2 * 1024 * 1024);
    assert_eq!(PSRAM_RESERVE_BYTES, 192 * 1024);
    assert_eq!(PSRAM_FONT_GLYPHS_BYTES, 256 * 1024);
    assert_eq!(INTERNAL_FONT_GLYPHS_BYTES, 16 * 1024);
    assert_eq!(PSRAM_CHAPTER_TEXT_BYTES, 768 * 1024);
    assert_eq!(PSRAM_IMAGE_DATA_BYTES, 512 * 1024);
    assert_eq!(PSRAM_PAGE_TABLE_BYTES, 64 * 1024);
    assert_eq!(PSRAM_ZIP_TOC_BYTES, 256 * 1024);
    let all_class_bytes = PSRAM_CHAPTER_TEXT_BYTES
        + PSRAM_IMAGE_DATA_BYTES
        + PSRAM_PAGE_TABLE_BYTES
        + PSRAM_ZIP_TOC_BYTES
        + PSRAM_FONT_GLYPHS_BYTES;
    assert!(all_class_bytes + PSRAM_RESERVE_BYTES <= 2 * 1024 * 1024);

    let mut budget = MemoryBudget::new();
    budget
        .set_status(PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        })
        .unwrap();
    for (class, bytes) in [
        (MemClass::ChapterText, 768 * 1024),
        (MemClass::ImageData, 512 * 1024),
        (MemClass::PageTable, 64 * 1024),
        (MemClass::ZipToc, 256 * 1024),
        (MemClass::FontGlyphs, 256 * 1024),
    ] {
        let reservation = budget.reserve(class, bytes, 16).unwrap();
        assert_eq!(reservation.region, Region::Psram);
    }
    assert_eq!(budget.pool_used(Region::Psram), 1856 * 1024);
    assert_eq!(budget.pool_limit(Region::Psram), 1856 * 1024);
}

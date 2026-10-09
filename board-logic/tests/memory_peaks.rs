use pulp_board_logic::memory::{MemClass, MemoryBudget, PsramStatus, Region};

#[test]
fn peaks_track_simultaneous_charges_and_survive_release() {
    let mut budget = MemoryBudget::new();
    budget
        .set_status(PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        })
        .unwrap();
    assert_eq!(budget.pool_peak(Region::Psram), 0);
    let a = budget.reserve(MemClass::ChapterText, 17, 16).unwrap();
    let b = budget.reserve(MemClass::ImageData, 33, 16).unwrap();
    assert_eq!(budget.peak(Region::Psram, MemClass::ChapterText), 32);
    assert_eq!(budget.peak(Region::Psram, MemClass::ImageData), 48);
    assert_eq!(budget.pool_peak(Region::Psram), 80);
    budget.release(a).unwrap();
    budget.release(b).unwrap();
    assert_eq!(budget.pool_used(Region::Psram), 0);
    assert_eq!(budget.pool_peak(Region::Psram), 80);
    let c = budget.reserve(MemClass::ImageData, 16, 16).unwrap();
    assert_eq!(budget.peak(Region::Psram, MemClass::ImageData), 48);
    assert_eq!(budget.pool_peak(Region::Psram), 80);
    budget.release(c).unwrap();
}

#[test]
fn refused_reservations_do_not_change_peaks() {
    let mut budget = MemoryBudget::new();
    let a = budget.reserve(MemClass::FontGlyphs, 16, 16).unwrap();
    assert!(budget.reserve(MemClass::FontGlyphs, 16 * 1024, 16).is_err());
    assert!(
        budget
            .reserve_in(Region::Psram, MemClass::DmaBuffer, 16, 16)
            .is_err()
    );
    assert!(budget.reserve(MemClass::ImageData, usize::MAX, 16).is_err());
    assert_eq!(budget.pool_peak(Region::Internal), 16);
    assert_eq!(budget.peak(Region::Internal, MemClass::FontGlyphs), 16);
    assert_eq!(budget.pool_peak(Region::Psram), 0);
    budget.release(a).unwrap();
}

#[test]
fn peaks_keep_internal_and_psram_history_separate() {
    let mut budget = MemoryBudget::new();
    let a = budget.reserve(MemClass::FontGlyphs, 32, 16).unwrap();
    budget.release(a).unwrap();
    budget
        .set_status(PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        })
        .unwrap();
    let b = budget.reserve(MemClass::FontGlyphs, 64, 16).unwrap();
    assert_eq!(budget.peak(Region::Internal, MemClass::FontGlyphs), 32);
    assert_eq!(budget.peak(Region::Psram, MemClass::FontGlyphs), 64);
    assert_eq!(budget.pool_peak(Region::Internal), 32);
    assert_eq!(budget.pool_peak(Region::Psram), 64);
    budget.release(b).unwrap();
}

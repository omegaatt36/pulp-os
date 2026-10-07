mod cjk_support;

use pulp_fontpack::PageGlyphSlot;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fonts::{FontSet, cjk::CjkState};
use pulp_host::kernel::Kernel;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static WATCH: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    static ALL: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static DROPPED: Cell<bool> = const { Cell::new(false) };
}
struct TrackingAllocator;
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        WATCH.with(|watch| {
            if ALL.with(Cell::get) || watch.get().is_some_and(|(size, _)| size == layout.size()) {
                ALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        WATCH.with(|watch| {
            if watch
                .get()
                .is_some_and(|(_, address)| address == ptr as usize)
            {
                DROPPED.with(|d| d.set(true));
            }
        });
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

#[test]
fn smaller_pages_reuse_slot_and_bitmap_backing_until_clear() {
    let card = cjk_support::card(b"", true);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    let latin = FontSet::for_size(0);
    state
        .prepare_text(&mut kernel.handle(), "臺灣𠮷".as_bytes(), latin, 16, 23)
        .unwrap();
    let bitmap = state.get(16, '灣').unwrap().bitmap.as_ptr();
    WATCH.with(|w| w.set(Some((2 * size_of::<PageGlyphSlot>(), bitmap as usize))));
    state
        .prepare_text(&mut kernel.handle(), "臺灣".as_bytes(), latin, 16, 23)
        .unwrap();
    let dropped = DROPPED.with(Cell::get);
    let allocations = ALLOCS.with(Cell::get);
    WATCH.with(|w| w.set(None));
    assert!(!dropped, "a smaller page must retain its bitmap allocation");
    assert_eq!(allocations, 0, "a smaller page must reuse reserved slots");
    assert_eq!(state.get(16, '灣').unwrap().bitmap.as_ptr(), bitmap);
    assert!(
        state.get(16, '𠮷').is_none(),
        "old glyphs must become invisible"
    );
    WATCH.with(|w| w.set(Some((usize::MAX, bitmap as usize))));
    state.clear();
    WATCH.with(|w| w.set(None));
    assert!(
        DROPPED.with(Cell::get),
        "lifecycle clear must release backing"
    );
    assert!(state.get(16, '灣').is_none());
}

#[test]
fn first_metrics_window_grows_in_bounded_chunks() {
    let card = cjk_support::card(b"", false);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    let text: String = (0x4e00..0x4e40)
        .map(|cp| char::from_u32(cp).unwrap())
        .collect();
    ALLOCS.with(|n| n.set(0));
    ALL.with(|a| a.set(true));
    let result = state.stage_metrics(
        &mut kernel.handle(),
        text.as_bytes(),
        FontSet::for_size(0),
        16,
        23,
    );
    ALL.with(|a| a.set(false));
    let allocations = ALLOCS.with(Cell::get);
    result.unwrap();
    eprintln!("64 metrics: {allocations} allocations including source lookup");
    assert!(
        allocations <= 16,
        "64 metrics must grow in chunks, got {allocations} allocations"
    );
}

#[test]
fn slot_growth_preserves_large_enough_bitmap_backing() {
    let card = cjk_support::card(b"", true);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    let latin = FontSet::for_size(0);
    state
        .prepare_text(&mut kernel.handle(), "未".as_bytes(), latin, 16, 23)
        .unwrap();
    let bitmap = state.get(16, '未').unwrap().bitmap.as_ptr();
    WATCH.with(|w| w.set(Some((usize::MAX, bitmap as usize))));
    state
        .prepare_text(&mut kernel.handle(), "臺灣𠮷".as_bytes(), latin, 16, 23)
        .unwrap();
    WATCH.with(|w| w.set(None));
    assert!(
        !DROPPED.with(Cell::get),
        "slot growth must keep sufficient bitmap backing"
    );
    assert_eq!(state.get(16, '灣').unwrap().bitmap.as_ptr(), bitmap);
}

#[test]
fn failed_heading_preparation_hides_both_roles() {
    let card = cjk_support::card(b"", true);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let mut state = CjkState::new();
    let latin = FontSet::for_size(0);
    let text = "臺\x01H灣";
    state
        .prepare_text(&mut kernel.handle(), text.as_bytes(), latin, 16, 23)
        .unwrap();
    assert!(state.get(16, '臺').is_some());
    assert!(state.get(23, '灣').is_some());
    cjk_support::install(&kernel.sd().card, 23, b"broken");
    assert!(state.prepare_visible(&mut kernel.handle(), 16, 23).is_err());
    assert!(
        state.get(16, '臺').is_none(),
        "a successful body cannot publish a partial page"
    );
    assert!(
        state.get(23, '灣').is_none(),
        "an old heading cannot survive failure"
    );
}

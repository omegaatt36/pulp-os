use core::num::NonZeroU32;
use pulp_host::kernel::{BufClass, TypedBuf};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static WATCHED: AtomicUsize = AtomicUsize::new(0);
static RELEASED_SIZE: AtomicUsize = AtomicUsize::new(0);
static RELEASED_ALIGN: AtomicUsize = AtomicUsize::new(0);

struct TrackingAllocator;
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if WATCHED.load(Ordering::SeqCst) == ptr as usize {
            RELEASED_SIZE.store(layout.size(), Ordering::SeqCst);
            RELEASED_ALIGN.store(layout.align(), Ordering::SeqCst);
            WATCHED.store(0, Ordering::SeqCst);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(align(128))]
struct Aligned(NonZeroU32, char, bool);

#[test]
fn aligned_elements_are_initialized_from_valid_values_and_mutable() {
    let value = Aligned(NonZeroU32::new(17).unwrap(), '臺', true);
    let mut storage = TypedBuf::filled(BufClass::PageTable, 9, value).unwrap();
    assert_eq!(storage.as_ptr() as usize % 128, 0);
    assert_eq!(storage.as_ref(), &[value; 9]);
    storage[8] = Aligned(NonZeroU32::new(23).unwrap(), '灣', false);
    assert_eq!(storage[8].0.get(), 23);
    assert_eq!(storage[0], value);
    let other = TypedBuf::filled(BufClass::PageTable, 9, value).unwrap();
    assert_ne!(other.as_ptr(), storage.as_ptr());
    drop(storage);
    assert_eq!(other.as_ref(), &[value; 9]);
}

#[test]
fn copy_references_and_zero_sized_elements_do_not_require_zero_validity() {
    let value = 42;
    let refs = TypedBuf::filled(BufClass::ZipToc, 3, &value).unwrap();
    assert!(refs.iter().all(|r| **r == 42));
    assert_eq!(
        TypedBuf::filled(BufClass::PageTable, 7, ()).unwrap().len(),
        7
    );
    assert!(TypedBuf::<Aligned>::empty().is_empty());
    let empty = TypedBuf::filled(BufClass::FontGlyphs, 0, NonZeroU32::new(1).unwrap()).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn impossible_counts_and_layouts_return_errors_without_allocation() {
    assert!(TypedBuf::filled(BufClass::PageTable, usize::MAX, 1u32).is_err());
    assert!(TypedBuf::filled(BufClass::PageTable, isize::MAX as usize, 1u32).is_err());
    assert!(TypedBuf::filled(BufClass::PageTable, usize::MAX, ()).is_err());
    // The same allocator remains usable after rejection.
    assert_eq!(
        TypedBuf::filled(BufClass::PageTable, 2, 99u32)
            .unwrap()
            .as_ref(),
        &[99, 99]
    );
}

#[test]
fn drop_returns_the_owned_block_with_its_original_layout() {
    let value = Aligned(NonZeroU32::new(1).unwrap(), '字', true);
    let storage = TypedBuf::filled(BufClass::FontGlyphs, 5, value).unwrap();
    WATCHED.store(storage.as_ptr() as usize, Ordering::SeqCst);
    drop(storage);
    assert_eq!(WATCHED.load(Ordering::SeqCst), 0);
    assert_eq!(
        RELEASED_SIZE.load(Ordering::SeqCst),
        5 * size_of::<Aligned>()
    );
    assert_eq!(RELEASED_ALIGN.load(Ordering::SeqCst), align_of::<Aligned>());
}

use pulp_board_logic::memory::MemError;
use pulp_host::kernel::bigbuf::{BigBuf, BufClass, BufError, ClassVec};

#[test]
fn buffer_errors_keep_the_allocation_cause() {
    let error = BigBuf::zeroed(BufClass::ImageData, usize::MAX)
        .err()
        .unwrap();
    assert_eq!(error.0, MemError::Overflow);
    assert_eq!(
        BufError(MemError::OutOfMemory).as_str(),
        "memory allocator exhausted"
    );
    use pulp_board_logic::memory::HeapKind;
    assert_eq!(
        BufError(MemError::HeapExhausted {
            heap: HeapKind::Internal
        })
        .as_str(),
        "internal allocator exhausted"
    );
    assert_eq!(
        BufError(MemError::HeapExhausted {
            heap: HeapKind::Scratch
        })
        .as_str(),
        "scratch allocator exhausted"
    );
    assert_eq!(
        BufError(MemError::HeapExhausted {
            heap: HeapKind::Psram
        })
        .as_str(),
        "memory allocator exhausted"
    );
}

#[test]
fn class_vector_grows_without_losing_sorted_entries_and_releases_capacity() {
    let mut values = ClassVec::new(BufClass::FontGlyphs);
    values.try_reserve_exact(2, 0u32).unwrap();
    values.insert(0, 3);
    values.insert(0, 1);
    values.try_reserve_exact(2, 0).unwrap();
    values.insert(1, 2);
    values.insert(3, 4);
    values.retain(|v| v % 2 == 0);
    assert_eq!(&*values, &[2, 4]);
    assert_eq!(values.capacity(), 4);
    values.clear();
    assert_eq!(values.len(), 0);
    assert_eq!(values.capacity(), 0);
}

#[test]
fn decoded_images_share_immutable_pixels_and_survive_cache_eviction() {
    use pulp_host::kernel::work_queue::DecodedImage;
    let mut pixels = BigBuf::zeroed(BufClass::ImageData, 48_000).unwrap();
    pixels[123] = 42;
    let image = DecodedImage::new(800, 480, 100, pixels).unwrap();
    let page = image.clone();
    assert_eq!(image.data.as_ptr(), page.data.as_ptr());
    drop(image);
    assert_eq!(page.data[123], 42);
}

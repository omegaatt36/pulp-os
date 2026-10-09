use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, build_epub,
};
use smol_epub::{jpeg, png, zip::ZipIndex};

const BITMAP_BYTES: usize = 800 / 8 * 480;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static BITMAP_ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct TrackedAllocator;

fn record(size: usize) {
    if size >= BITMAP_BYTES && TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = BITMAP_ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
    }
}

unsafe impl GlobalAlloc for TrackedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackedAllocator = TrackedAllocator;

struct TrackingGuard;

impl TrackingGuard {
    fn start() -> Self {
        BITMAP_ALLOCATIONS.set(0);
        TRACKING.set(true);
        Self
    }
}

impl Drop for TrackingGuard {
    fn drop(&mut self) {
        TRACKING.set(false);
    }
}

fn source(kind: ImageKind) -> Vec<u8> {
    let bytes = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Destination storage".into(),
        author: "Test".into(),
        identifier: "output-allocation".into(),
        chapters: vec![Chapter {
            title: "Image".into(),
            blocks: vec![Block::Image(0)],
        }],
        toc: vec![],
        images: vec![ImageSpec {
            path: "image".into(),
            kind,
            width: 800,
            height: 480,
            pattern: Pattern::White,
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap();
    let (offset, size) = ZipIndex::parse_eocd(&bytes, bytes.len() as u32).unwrap();
    let mut index = ZipIndex::new();
    index
        .parse_central_directory(&bytes[offset as usize..(offset + size) as usize])
        .unwrap();
    let entry = index.entry(index.find("OEBPS/image").unwrap());
    let offset = entry.local_offset as usize;
    let skip = ZipIndex::local_header_data_skip(&bytes[offset..offset + 30]).unwrap() as usize;
    bytes[offset + skip..offset + skip + entry.uncomp_size as usize].to_vec()
}

fn read_at(data: &[u8]) -> impl FnMut(u32, &mut [u8]) -> Result<usize, &'static str> + '_ {
    move |offset, output| {
        let source = data.get(offset as usize..).ok_or("read past end")?;
        let len = source.len().min(output.len());
        output[..len].copy_from_slice(&source[..len]);
        Ok(len)
    }
}

#[test]
fn caller_storage_eliminates_the_full_bitmap_heap_allocation() {
    for kind in [ImageKind::Jpeg, ImageKind::PngGray8] {
        let source = source(kind);
        let compressed = miniz_oxide::deflate::compress_to_vec(&source, 6);
        for route in 0..3 {
            let mut destination = vec![0xA5; BITMAP_BYTES];
            let pointer = destination.as_ptr();
            let storage = destination.as_mut_slice();
            let allocate = move |len| {
                assert_eq!(len, storage.len());
                Ok(storage)
            };
            let guard = TrackingGuard::start();
            let image = match (kind, route) {
                (ImageKind::Jpeg, 0) => {
                    jpeg::decode_jpeg_fit_with_buffer(&source, 800, 480, allocate)
                }
                (ImageKind::Jpeg, 1) => jpeg::decode_jpeg_streaming_with_buffer(
                    read_at(&source),
                    0,
                    source.len() as u32,
                    800,
                    480,
                    allocate,
                ),
                (ImageKind::Jpeg, _) => jpeg::decode_jpeg_deflate_streaming_with_buffer(
                    read_at(&compressed),
                    0,
                    compressed.len() as u32,
                    source.len() as u32,
                    800,
                    480,
                    allocate,
                ),
                (_, 0) => png::decode_png_fit_with_buffer(&source, 800, 480, allocate),
                (_, 1) => png::decode_png_streaming_with_buffer(
                    read_at(&source),
                    0,
                    source.len() as u32,
                    800,
                    480,
                    allocate,
                ),
                (_, _) => png::decode_png_deflate_streaming_with_buffer(
                    read_at(&compressed),
                    0,
                    compressed.len() as u32,
                    800,
                    480,
                    allocate,
                ),
            }
            .unwrap();
            let allocations = BITMAP_ALLOCATIONS.get();
            drop(guard);
            assert_eq!((image.width, image.height, image.stride), (800, 480, 100));
            assert_eq!(image.data.as_ptr(), pointer);
            assert!(image.data.iter().all(|&byte| byte == 0));
            assert_eq!(
                allocations, 0,
                "{kind:?} route {route}: allocated a full-size temporary bitmap"
            );
        }

        // Positive control: the compatibility API must allocate its owned bitmap.
        let guard = TrackingGuard::start();
        let owned = if kind == ImageKind::Jpeg {
            jpeg::decode_jpeg_fit(&source, 800, 480)
        } else {
            png::decode_png_fit(&source, 800, 480)
        }
        .unwrap();
        let allocations = BITMAP_ALLOCATIONS.get();
        drop(guard);
        assert_eq!(owned.data.len(), BITMAP_BYTES);
        assert!(
            allocations > 0,
            "allocation detector did not see the owned bitmap"
        );
    }
}

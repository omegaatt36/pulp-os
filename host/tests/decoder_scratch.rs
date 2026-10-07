use pulp_host::fixtures::*;
use smol_epub::{
    png,
    scratch::{HeapScratch, ScratchStorage},
};
use std::{alloc::Layout, cell::Cell, rc::Rc};

struct Tracked {
    block: HeapScratch,
    live: Rc<Cell<usize>>,
}
unsafe impl ScratchStorage for Tracked {
    fn ptr(&self) -> *mut u8 {
        self.block.ptr()
    }
    fn len(&self) -> usize {
        self.block.len()
    }
}
impl Drop for Tracked {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}
fn image_input(kind: ImageKind) -> Vec<u8> {
    let book = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Scratch".into(),
        author: "Test".into(),
        identifier: "scratch".into(),
        chapters: vec![Chapter {
            title: "One".into(),
            blocks: vec![Block::Image(0)],
        }],
        toc: vec![],
        images: vec![ImageSpec {
            path: "test.png".into(),
            kind,
            width: 40,
            height: 24,
            pattern: Pattern::Checker { cell: 8 },
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap();
    let (off, len) = smol_epub::zip::ZipIndex::parse_eocd(&book, book.len() as u32).unwrap();
    let mut index = smol_epub::zip::ZipIndex::new();
    index
        .parse_central_directory(&book[off as usize..(off + len) as usize])
        .unwrap();
    let entry = index.entry(index.find("OEBPS/test.png").unwrap());
    smol_epub::zip::extract_entry(entry, entry.local_offset, |off, dst| read(&book, off, dst))
        .unwrap()
}
fn input() -> Vec<u8> {
    image_input(ImageKind::PngGray8)
}
fn read(data: &[u8], off: u32, dst: &mut [u8]) -> Result<usize, &'static str> {
    let off = off as usize;
    let n = dst.len().min(data.len().saturating_sub(off));
    dst[..n].copy_from_slice(&data[off..off + n]);
    Ok(n)
}
#[test]
fn injected_scratch_preserves_pixels_and_releases_both_inflate_layers() {
    let data = input();
    let compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
    let expected = png::decode_png_fit(&data, 19, 11).unwrap();
    let live = Rc::new(Cell::new(0));
    let calls = Cell::new(0);
    let allocate = |layout: Layout| {
        calls.set(calls.get() + 1);
        live.set(live.get() + 1);
        Ok(Tracked {
            block: HeapScratch::zeroed(layout)?,
            live: live.clone(),
        })
    };
    let decoded = png::decode_png_deflate_streaming_with_scratch(
        |off, dst| read(&compressed, off, dst),
        0,
        compressed.len() as u32,
        19,
        11,
        |n| Ok(vec![0; n]),
        allocate,
    )
    .unwrap();
    assert_eq!(decoded.data, expected.data);
    assert_eq!(calls.get(), 4);
    assert_eq!(live.get(), 0);
}
#[test]
fn rejection_of_each_large_scratch_block_releases_prior_allocations() {
    let data = input();
    let compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
    for reject in 0..4 {
        let calls = Cell::new(0);
        let live = Rc::new(Cell::new(0));
        let allocate = |layout: Layout| {
            let call = calls.get();
            calls.set(call + 1);
            if call == reject {
                return Err("scratch rejected");
            }
            let block = HeapScratch::zeroed(layout)?;
            live.set(live.get() + 1);
            Ok(Tracked {
                block,
                live: live.clone(),
            })
        };
        let result = png::decode_png_deflate_streaming_with_scratch(
            |off, dst| read(&compressed, off, dst),
            0,
            compressed.len() as u32,
            19,
            11,
            |n| Ok(vec![0; n]),
            allocate,
        );
        assert_eq!(result.err(), Some("scratch rejected"));
        assert_eq!(live.get(), 0);
    }
}

#[test]
fn kernel_scratch_respects_typed_alignment_and_refuses_impossible_allocation() {
    use pulp_host::kernel::{BufClass, bigbuf::DecoderScratch};
    let layout = Layout::from_size_align(113, 64).unwrap();
    let block = DecoderScratch::zeroed(BufClass::ImageData, layout).unwrap();
    assert_eq!(block.ptr() as usize % 64, 0);
    assert_eq!(block.len(), 113);
    assert!(
        unsafe { std::slice::from_raw_parts(block.ptr(), block.len()) }
            .iter()
            .all(|&b| b == 0)
    );
    let impossible = Layout::from_size_align(isize::MAX as usize, 1).unwrap();
    assert!(DecoderScratch::zeroed(BufClass::ImageData, impossible).is_err());
}

#[test]
fn jpeg_outer_inflate_uses_injected_scratch_and_rejects_without_leaks() {
    let data = image_input(ImageKind::Jpeg);
    let compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
    let expected = smol_epub::jpeg::decode_jpeg_fit(&data, 19, 11).unwrap();
    for reject in 0..3 {
        let live = Rc::new(Cell::new(0));
        let calls = Cell::new(0);
        let allocate = |layout: Layout| {
            let call = calls.get();
            calls.set(call + 1);
            if call == reject {
                return Err("scratch rejected");
            }
            let block = HeapScratch::zeroed(layout)?;
            live.set(live.get() + 1);
            Ok(Tracked {
                block,
                live: live.clone(),
            })
        };
        let result = smol_epub::jpeg::decode_jpeg_deflate_streaming_with_scratch(
            |off, dst| read(&compressed, off, dst),
            0,
            compressed.len() as u32,
            data.len() as u32,
            19,
            11,
            |n| Ok(vec![0; n]),
            allocate,
        );
        if reject < 2 {
            assert_eq!(result.err(), Some("scratch rejected"));
        } else {
            assert_eq!(result.unwrap().data, expected.data);
            assert_eq!(calls.get(), 2);
        }
        assert_eq!(live.get(), 0);
    }
}

struct Reader(Vec<u8>);
impl smol_epub::async_io::AsyncReadAt for Reader {
    async fn read_at(&mut self, off: u32, dst: &mut [u8]) -> Result<usize, &'static str> {
        read(&self.0, off, dst)
    }
}
struct Writer(Vec<u8>);
impl smol_epub::async_io::AsyncWriteChunk for Writer {
    async fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}
fn run<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
    }
}
#[test]
fn chapter_inflate_injects_scratch_and_releases_on_success_or_failure() {
    let book = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Scratch".into(),
        author: "Test".into(),
        identifier: "scratch".into(),
        chapters: vec![Chapter {
            title: "One".into(),
            blocks: vec![Block::Paragraph(vec![Run::Text(
                "Chapter scratch test".into(),
            )])],
        }],
        toc: vec![],
        images: vec![],
        cover: None,
        compression: Compression::Deflate,
        numeric_entities: false,
    })
    .unwrap();
    let (off, len) = smol_epub::zip::ZipIndex::parse_eocd(&book, book.len() as u32).unwrap();
    let mut index = smol_epub::zip::ZipIndex::new();
    index
        .parse_central_directory(&book[off as usize..(off + len) as usize])
        .unwrap();
    let entry = index.entry(index.find("OEBPS/text/chapter01.xhtml").unwrap());
    let mut baseline = Writer(vec![]);
    run(smol_epub::async_io::stream_strip_entry_async(
        entry,
        entry.local_offset,
        &mut Reader(book.clone()),
        &mut baseline,
    ))
    .unwrap();
    for reject in 0..3 {
        let live = Rc::new(Cell::new(0));
        let calls = Cell::new(0);
        let allocate = |layout: Layout| {
            let call = calls.get();
            calls.set(call + 1);
            if call == reject {
                return Err("scratch rejected");
            }
            let block = HeapScratch::zeroed(layout)?;
            live.set(live.get() + 1);
            Ok(Tracked {
                block,
                live: live.clone(),
            })
        };
        let mut writer = Writer(vec![]);
        let result = run(smol_epub::async_io::stream_strip_entry_async_with_scratch(
            entry,
            entry.local_offset,
            &mut Reader(book.clone()),
            &mut writer,
            allocate,
        ));
        if reject < 2 {
            assert_eq!(result.err(), Some("scratch rejected"));
            assert!(writer.0.is_empty());
        } else {
            result.unwrap();
            assert_eq!(writer.0, baseline.0);
            assert_eq!(calls.get(), 2);
        }
        assert_eq!(live.get(), 0);
    }
}

#[test]
fn oversized_scratch_storage_preserves_requested_dictionary_size() {
    let data = input();
    let compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
    let expected = png::decode_png_fit(&data, 19, 11).unwrap();
    let allocate = |layout: Layout| {
        HeapScratch::zeroed(Layout::from_size_align(layout.size() + 1, layout.align()).unwrap())
    };
    let result = png::decode_png_deflate_streaming_with_scratch(
        |off, dst| read(&compressed, off, dst),
        0,
        compressed.len() as u32,
        19,
        11,
        |n| Ok(vec![0; n]),
        allocate,
    )
    .unwrap();
    assert_eq!(result.data, expected.data);
}
#[test]
fn toc_fallible_constructor_initializes_entries_without_large_stack_temporary() {
    let toc = smol_epub::epub::EpubToc::try_new().unwrap();
    assert_eq!(toc.count, 0);
    assert!(
        toc.entries
            .iter()
            .all(|e| e.title_len == 0 && e.spine_idx == 0xffff && e.title.iter().all(|&b| b == 0))
    );
}

#[test]
fn buffered_png_and_zip_extraction_inject_scratch_and_propagate_rejection() {
    let data = input();
    let expected = png::decode_png_fit(&data, 19, 11).unwrap();
    let decoded =
        png::decode_png_fit_with_scratch(&data, 19, 11, |n| Ok(vec![0; n]), HeapScratch::zeroed)
            .unwrap();
    assert_eq!(decoded.data, expected.data);
    let rejected = png::decode_png_fit_with_scratch(
        &data,
        19,
        11,
        |n| Ok(vec![0; n]),
        |_| Err::<HeapScratch, _>("scratch rejected"),
    );
    assert_eq!(rejected.err(), Some("scratch rejected"));
    let book = build_zip(&[RawEntry {
        name: "test.png".into(),
        data: data.clone(),
        deflate: true,
    }]);
    let (off, len) = smol_epub::zip::ZipIndex::parse_eocd(&book, book.len() as u32).unwrap();
    let mut index = smol_epub::zip::ZipIndex::new();
    index
        .parse_central_directory(&book[off as usize..(off + len) as usize])
        .unwrap();
    let entry = index.entry(0);
    let extracted = smol_epub::zip::extract_entry_with_scratch(
        entry,
        entry.local_offset,
        |off, dst| read(&book, off, dst),
        HeapScratch::zeroed,
    )
    .unwrap();
    assert_eq!(extracted, data);
    let rejected = smol_epub::zip::extract_entry_with_scratch(
        entry,
        entry.local_offset,
        |off, dst| read(&book, off, dst),
        |_| Err::<HeapScratch, _>("scratch rejected"),
    );
    assert_eq!(rejected.err(), Some("scratch rejected"));
}

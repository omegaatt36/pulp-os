use pulp_host::fixtures::*;
use smol_epub::png::*;
use smol_epub::zip::{ZipIndex, extract_entry};

fn png(kind: ImageKind, pattern: Pattern) -> Vec<u8> {
    let book = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Output".into(),
        author: "Test".into(),
        identifier: "output".into(),
        chapters: vec![Chapter {
            title: "One".into(),
            blocks: vec![Block::Image(0)],
        }],
        toc: vec![],
        images: vec![ImageSpec {
            path: "test.png".into(),
            kind,
            width: 19,
            height: 11,
            pattern,
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap();
    let (offset, size) = ZipIndex::parse_eocd(&book, book.len() as u32).unwrap();
    let mut index = ZipIndex::new();
    index
        .parse_central_directory(&book[offset as usize..(offset + size) as usize])
        .unwrap();
    let entry = index.entry(index.find("OEBPS/test.png").unwrap());
    extract_entry(entry, entry.local_offset, |off, dst| read(&book, off, dst)).unwrap()
}

fn read(data: &[u8], offset: u32, dst: &mut [u8]) -> Result<usize, &'static str> {
    let offset = offset as usize;
    let n = dst.len().min(data.len().saturating_sub(offset));
    dst[..n].copy_from_slice(&data[offset..offset + n]);
    Ok(n)
}

#[test]
fn writes_exact_borrowed_storage_on_all_routes_with_scaling_and_padding() {
    for kind in [
        ImageKind::PngGray1,
        ImageKind::PngGray8,
        ImageKind::PngPalette8,
    ] {
        for pattern in [Pattern::Black, Pattern::White, Pattern::Checker { cell: 1 }] {
            let input = png(kind, pattern);
            let compressed = miniz_oxide::deflate::compress_to_vec(&input, 6);
            for (max_w, max_h, width, height) in [(19, 11, 19, 11), (10, 6, 9, 5)] {
                let stride = (width as usize + 7) / 8;
                let size = stride * height as usize;
                let mut expected = vec![0; size];
                let scale = if width == 19 { 1 } else { 2 };
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let black = match pattern {
                            Pattern::Black => true,
                            Pattern::White => false,
                            Pattern::Checker { cell } => {
                                ((x * scale / cell as usize) + (y * scale / cell as usize)) % 2 == 0
                            }
                            _ => unreachable!(),
                        };
                        if black {
                            expected[y * stride + x / 8] |= 1 << (7 - x % 8);
                        }
                    }
                }
                for route in 0..3 {
                    let mut storage = vec![0xff; size];
                    let pointer = storage.as_ptr();
                    let calls = std::cell::Cell::new(0);
                    let calls_ref = &calls;
                    let bytes = storage.as_mut_slice();
                    let allocate = move |len| {
                        calls_ref.set(calls_ref.get() + 1);
                        assert_eq!(len, size);
                        Ok(bytes)
                    };
                    let decoded = match route {
                        0 => decode_png_fit_with_buffer(&input, max_w, max_h, allocate),
                        1 => decode_png_streaming_with_buffer(
                            |off, dst| read(&input, off, dst),
                            0,
                            input.len() as u32,
                            max_w,
                            max_h,
                            allocate,
                        ),
                        _ => decode_png_deflate_streaming_with_buffer(
                            |off, dst| read(&compressed, off, dst),
                            0,
                            compressed.len() as u32,
                            max_w,
                            max_h,
                            allocate,
                        ),
                    }
                    .unwrap();
                    assert_eq!((decoded.width, decoded.height), (width, height));
                    assert_eq!(decoded.data.as_ptr(), pointer);
                    assert_eq!(decoded.data, expected.as_slice());
                    assert_eq!(calls.get(), 1);
                }
                let old = decode_png_fit(&input, max_w, max_h).unwrap();
                assert_eq!(old.data, expected);
                let old_stored = decode_png_streaming(
                    |off, dst| read(&input, off, dst),
                    0,
                    input.len() as u32,
                    max_w,
                    max_h,
                )
                .unwrap();
                assert_eq!(old_stored.data, expected);
                let old_deflate = decode_png_deflate_streaming(
                    |off, dst| read(&compressed, off, dst),
                    0,
                    compressed.len() as u32,
                    max_w,
                    max_h,
                )
                .unwrap();
                assert_eq!(old_deflate.data, expected);
            }
        }
    }
}

#[test]
fn refuses_wrong_lengths_failed_allocation_and_zero_bounds() {
    let input = png(ImageKind::PngGray8, Pattern::Black);
    let compressed = miniz_oxide::deflate::compress_to_vec(&input, 6);
    for route in 0..3 {
        for delta in [-1isize, 1] {
            let allocate = |len| Ok(vec![0xff; (len as isize + delta) as usize]);
            let result = match route {
                0 => decode_png_fit_with_buffer(&input, 19, 11, allocate),
                1 => decode_png_streaming_with_buffer(
                    |off, dst| read(&input, off, dst),
                    0,
                    input.len() as u32,
                    19,
                    11,
                    allocate,
                ),
                _ => decode_png_deflate_streaming_with_buffer(
                    |off, dst| read(&compressed, off, dst),
                    0,
                    compressed.len() as u32,
                    19,
                    11,
                    allocate,
                ),
            };
            assert_eq!(result.err(), Some("png: output buffer length mismatch"));
        }
        let allocate = |_| Err::<Vec<u8>, _>("caller allocation failed");
        let result = match route {
            0 => decode_png_fit_with_buffer(&input, 19, 11, allocate),
            1 => decode_png_streaming_with_buffer(
                |off, dst| read(&input, off, dst),
                0,
                input.len() as u32,
                19,
                11,
                allocate,
            ),
            _ => decode_png_deflate_streaming_with_buffer(
                |off, dst| read(&compressed, off, dst),
                0,
                compressed.len() as u32,
                19,
                11,
                allocate,
            ),
        };
        assert_eq!(result.err(), Some("caller allocation failed"));
        for (w, h) in [(0, 11), (19, 0)] {
            let allocate =
                |_| -> Result<Vec<u8>, &'static str> { panic!("zero bounds must not allocate") };
            let result = match route {
                0 => decode_png_fit_with_buffer(&input, w, h, allocate),
                1 => decode_png_streaming_with_buffer(
                    |off, dst| read(&input, off, dst),
                    0,
                    input.len() as u32,
                    w,
                    h,
                    allocate,
                ),
                _ => decode_png_deflate_streaming_with_buffer(
                    |off, dst| read(&compressed, off, dst),
                    0,
                    compressed.len() as u32,
                    w,
                    h,
                    allocate,
                ),
            };
            assert_eq!(result.err(), Some("png: zero output bounds"));
        }
    }
}

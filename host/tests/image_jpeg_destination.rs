use pulp_host::fixtures::{Compression, ImageKind, Pattern, Spec, build_epub, standard};
use smol_epub::jpeg::*;

fn jpeg(pattern: Pattern) -> Vec<u8> {
    let mut spec = standard()
        .into_iter()
        .find_map(|f| match f.spec {
            Spec::Epub(s) => Some(s),
            _ => None,
        })
        .unwrap();
    spec.compression = Compression::Stored;
    let image = spec
        .images
        .iter_mut()
        .find(|i| i.kind == ImageKind::Jpeg)
        .unwrap();
    image.width = 40;
    image.height = 24;
    image.pattern = pattern;
    let path = format!("OEBPS/{}", image.path);
    let zip = build_epub(&spec).unwrap();
    let mut pos = 0;
    while zip[pos..pos + 4] == [0x50, 0x4b, 3, 4] {
        let size = u32::from_le_bytes(zip[pos + 18..pos + 22].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes(zip[pos + 26..pos + 28].try_into().unwrap()) as usize;
        let extra = u16::from_le_bytes(zip[pos + 28..pos + 30].try_into().unwrap()) as usize;
        let start = pos + 30 + name_len + extra;
        if &zip[pos + 30..pos + 30 + name_len] == path.as_bytes() {
            return zip[start..start + size].to_vec();
        }
        pos = start + size;
    }
    panic!("fixture JPEG missing");
}

fn read_at<'a>(bytes: &'a [u8]) -> impl FnMut(u32, &mut [u8]) -> Result<usize, &'static str> + 'a {
    move |offset, dst| {
        let source = &bytes[offset as usize..];
        let n = source.len().min(dst.len());
        dst[..n].copy_from_slice(&source[..n]);
        Ok(n)
    }
}

#[test]
fn all_routes_write_dirty_borrowed_storage_without_replacing_it() {
    for pattern in [
        Pattern::Black,
        Pattern::White,
        Pattern::Checker { cell: 8 },
        Pattern::HorizontalGradient,
    ] {
        let bytes = jpeg(pattern);
        for bounds in [(40, 24), (13, 8), (8, 5)] {
            let reference = decode_jpeg_fit(&bytes, bounds.0, bounds.1).unwrap();
            assert_eq!(
                reference.width as usize,
                40 / (40usize
                    .div_ceil(bounds.0 as usize)
                    .max(24usize.div_ceil(bounds.1 as usize)))
            );
            for route in 0..3 {
                let mut storage = vec![0xff; reference.data.len()];
                let ptr = storage.as_ptr();
                let called = std::cell::Cell::new(0);
                let destination = storage.as_mut_slice();
                let calls = &called;
                let expected_len = reference.data.len();
                let allocate = move |len| {
                    calls.set(calls.get() + 1);
                    assert_eq!(len, expected_len);
                    Ok(destination)
                };
                let out = match route {
                    0 => decode_jpeg_fit_with_buffer(&bytes, bounds.0, bounds.1, allocate),
                    1 => decode_jpeg_streaming_with_buffer(
                        read_at(&bytes),
                        0,
                        bytes.len() as u32,
                        bounds.0,
                        bounds.1,
                        allocate,
                    ),
                    _ => {
                        let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 6);
                        decode_jpeg_deflate_streaming_with_buffer(
                            read_at(&compressed),
                            0,
                            compressed.len() as u32,
                            bytes.len() as u32,
                            bounds.0,
                            bounds.1,
                            allocate,
                        )
                    }
                }
                .unwrap();
                assert_eq!(out.data.as_ptr(), ptr);
                assert_eq!(
                    (out.width, out.height, out.stride),
                    (reference.width, reference.height, reference.stride)
                );
                assert_eq!(out.data, reference.data);
                let unused = (8 - out.width as usize % 8) % 8;
                for row in out.data.chunks(out.stride) {
                    assert_eq!(row[out.stride - 1] & ((1u8 << unused) - 1), 0);
                    if pattern == Pattern::White {
                        assert!(row.iter().all(|&b| b == 0));
                    }
                    if pattern == Pattern::Black {
                        for x in 0..out.width as usize {
                            assert_ne!(row[x / 8] & (1 << (7 - x % 8)), 0);
                        }
                    }
                }
                assert_eq!(called.get(), 1);
            }
        }
    }
}

#[test]
fn refusal_wrong_lengths_and_zero_bounds_are_errors() {
    let bytes = jpeg(Pattern::Black);
    let err = decode_jpeg_fit_with_buffer(&bytes, 40, 24, |_| Err::<Vec<u8>, _>("storage refused"))
        .unwrap_err();
    assert_eq!(err, "storage refused");
    for delta in [-1isize, 1] {
        assert!(
            decode_jpeg_fit_with_buffer(&bytes, 40, 24, |len| Ok(vec![
                0;
                len.checked_add_signed(
                    delta
                )
                .unwrap()
            ]))
            .is_err()
        );
    }
    for bounds in [(0, 24), (40, 0), (0, 0)] {
        let allocate = |_| -> Result<Vec<u8>, &'static str> {
            panic!("invalid bounds must not allocate output")
        };
        assert!(decode_jpeg_fit_with_buffer(&bytes, bounds.0, bounds.1, allocate).is_err());
        assert!(
            decode_jpeg_streaming_with_buffer(
                read_at(&bytes),
                0,
                bytes.len() as u32,
                bounds.0,
                bounds.1,
                allocate
            )
            .is_err()
        );
        let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 6);
        assert!(
            decode_jpeg_deflate_streaming_with_buffer(
                read_at(&compressed),
                0,
                compressed.len() as u32,
                bytes.len() as u32,
                bounds.0,
                bounds.1,
                allocate
            )
            .is_err()
        );
        assert!(decode_jpeg_fit(&bytes, bounds.0, bounds.1).is_err());
    }
}

#[test]
fn streaming_routes_propagate_destination_errors_and_keep_legacy_pixels() {
    let bytes = jpeg(Pattern::HorizontalGradient);
    let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 6);
    let mut stored_file = vec![0x55; 37];
    stored_file.extend_from_slice(&bytes);
    let mut deflated_file = vec![0x55; 37];
    deflated_file.extend_from_slice(&compressed);
    let reference = decode_jpeg_fit(&bytes, 13, 8).unwrap();
    let stored =
        decode_jpeg_streaming(read_at(&stored_file), 37, bytes.len() as u32, 13, 8).unwrap();
    let deflated = decode_jpeg_deflate_streaming(
        read_at(&deflated_file),
        37,
        compressed.len() as u32,
        bytes.len() as u32,
        13,
        8,
    )
    .unwrap();
    assert_eq!(stored.data, reference.data);
    assert_eq!(deflated.data, reference.data);
    let refusal = |_| Err::<Vec<u8>, _>("destination unavailable");
    assert_eq!(
        decode_jpeg_streaming_with_buffer(
            read_at(&stored_file),
            37,
            bytes.len() as u32,
            13,
            8,
            refusal
        )
        .unwrap_err(),
        "destination unavailable"
    );
    assert_eq!(
        decode_jpeg_deflate_streaming_with_buffer(
            read_at(&deflated_file),
            37,
            compressed.len() as u32,
            bytes.len() as u32,
            13,
            8,
            refusal
        )
        .unwrap_err(),
        "destination unavailable"
    );
    for delta in [-1isize, 1] {
        let wrong = |len: usize| Ok(vec![0; len.checked_add_signed(delta).unwrap()]);
        assert!(
            decode_jpeg_streaming_with_buffer(
                read_at(&stored_file),
                37,
                bytes.len() as u32,
                13,
                8,
                wrong
            )
            .is_err()
        );
        assert!(
            decode_jpeg_deflate_streaming_with_buffer(
                read_at(&deflated_file),
                37,
                compressed.len() as u32,
                bytes.len() as u32,
                13,
                8,
                wrong
            )
            .is_err()
        );
    }
}

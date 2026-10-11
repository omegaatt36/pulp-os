//! Progressive JPEG decoding.
//!
//! The `*_prog.jpg` fixtures are `jpegtran -progressive` rewrites of the
//! `*_base.jpg` ones: a lossless transcode, so both carry the same
//! coefficients and a correct decode must agree with the baseline decode.
//! Exactly, when one band covers the image; closely, when AC refinement
//! scans are skipped (see the `jpeg` module docs).

use smol_epub::DecodedImage;
use smol_epub::jpeg::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("jpeg_progressive/", $name, ".jpg")).as_slice()
    };
}

fn read_at<'a>(bytes: &'a [u8]) -> impl FnMut(u32, &mut [u8]) -> Result<usize, &'static str> + 'a {
    move |offset, dst| {
        let source = &bytes[(offset as usize).min(bytes.len())..];
        let n = source.len().min(dst.len());
        dst[..n].copy_from_slice(&source[..n]);
        Ok(n)
    }
}

// every route the reader can take to the decoder
fn routes(bytes: &[u8], max_w: u16, max_h: u16) -> [Result<DecodedImage, &'static str>; 3] {
    // stored entry preceded by 7 unrelated bytes, as in a ZIP
    let mut stored = vec![0xA5; 7];
    stored.extend_from_slice(bytes);
    let mut deflated = vec![0x5A; 3];
    let compressed = miniz_oxide::deflate::compress_to_vec(bytes, 6);
    deflated.extend_from_slice(&compressed);
    [
        decode_jpeg_fit(bytes, max_w, max_h),
        decode_jpeg_streaming(read_at(&stored), 7, bytes.len() as u32, max_w, max_h),
        decode_jpeg_deflate_streaming(read_at(&deflated), 3, compressed.len() as u32, max_w, max_h),
    ]
}

fn differing_bits(a: &DecodedImage, b: &DecodedImage) -> usize {
    assert_eq!((a.width, a.height, a.stride), (b.width, b.height, b.stride));
    a.data
        .iter()
        .zip(&b.data)
        .map(|(x, y)| (x ^ y).count_ones() as usize)
        .sum()
}

// Mean absolute difference of the black fraction over 8x8 cells. Bit-exact
// comparison means nothing once the coefficients differ: one luma level
// moves the error diffusion everywhere. The cell density is what the eye
// (and the panel) integrates.
fn density_error(a: &DecodedImage, b: &DecodedImage) -> f64 {
    assert_eq!((a.width, a.height, a.stride), (b.width, b.height, b.stride));
    let (w, h) = (a.width as usize, a.height as usize);
    let density = |im: &DecodedImage, cx: usize, cy: usize| {
        let mut black = 0;
        for y in cy * 8..(cy * 8 + 8).min(h) {
            for x in cx * 8..(cx * 8 + 8).min(w) {
                black += (im.data[y * im.stride + x / 8] >> (7 - x % 8)) & 1;
            }
        }
        black as f64
    };
    let (cells_x, cells_y) = (w / 8, h / 8);
    let mut total = 0.0;
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            total += (density(a, cx, cy) - density(b, cx, cy)).abs() / 64.0;
        }
    }
    total / (cells_x * cells_y) as f64
}

#[test]
fn single_band_progressive_matches_baseline_exactly() {
    for name in ["small", "small444", "gray"] {
        let (base, prog) = match name {
            "small" => (fixture!("small_base"), fixture!("small_prog")),
            "small444" => (fixture!("small444_base"), fixture!("small444_prog")),
            _ => (fixture!("gray_base"), fixture!("gray_prog")),
        };
        for bounds in [(400, 300), (100, 80), (50, 30)] {
            let expected = decode_jpeg_fit(base, bounds.0, bounds.1).unwrap();
            for (i, got) in routes(prog, bounds.0, bounds.1).into_iter().enumerate() {
                let got = got.unwrap();
                let diff = differing_bits(&expected, &got);
                assert_eq!(diff, 0, "{name} {bounds:?} route {i}: bits differ");
            }
        }
    }
}

#[test]
fn restart_intervals_are_honoured_in_every_scan_kind() {
    let expected = decode_jpeg_fit(fixture!("small_base"), 400, 300).unwrap();
    for got in routes(fixture!("small_prog_rst"), 400, 300) {
        assert_eq!(differing_bits(&expected, &got.unwrap()), 0);
    }
}

// 1024x256 4:2:0: one MCU row is 32 KB of coefficients, so the 48 KB band
// holds a single row and the decode takes 16 passes
#[test]
fn multi_band_without_refinement_scans_matches_baseline_exactly() {
    // spectral selection only: nothing is skipped, so the band loop, the
    // early exit and the non-interleaved block grid must be lossless
    let base = fixture!("big_base");
    for bounds in [(1024, 256), (512, 128), (300, 100)] {
        let expected = decode_jpeg_fit(base, bounds.0, bounds.1).unwrap();
        for (i, got) in routes(fixture!("big_spectral"), bounds.0, bounds.1)
            .into_iter()
            .enumerate()
        {
            let diff = differing_bits(&expected, &got.unwrap());
            assert_eq!(diff, 0, "{bounds:?} route {i}: bits differ");
        }
    }
}

#[test]
fn multi_band_with_refinement_scans_tracks_baseline() {
    // successive approximation: the AC refinement scans are skipped, so
    // the last bits of AC precision go, the picture stays
    let base = fixture!("big_base");
    let prog = fixture!("big_prog");
    for bounds in [(1024, 256), (512, 128), (300, 100)] {
        let expected = decode_jpeg_fit(base, bounds.0, bounds.1).unwrap();
        let [slice, stored, deflated] = routes(prog, bounds.0, bounds.1);
        let slice = slice.unwrap();
        // the read source must not change the result
        assert_eq!(
            differing_bits(&stored.unwrap(), &slice),
            0,
            "{bounds:?} stored"
        );
        assert_eq!(
            differing_bits(&deflated.unwrap(), &slice),
            0,
            "{bounds:?} deflate"
        );
        let err = density_error(&expected, &slice);
        assert!(err < 0.02, "{bounds:?}: density error {err:.4}");
    }
}

#[test]
fn damaged_progressive_files_error_or_decode_but_never_panic() {
    for file in [
        fixture!("small_prog"),
        fixture!("small_prog_rst"),
        fixture!("gray_prog"),
        fixture!("big_prog"),
    ] {
        // about 150 truncation points and as many flipped bytes per file
        let step = (file.len() / 150).max(7);
        for len in (0..file.len()).step_by(step) {
            let _ = routes(&file[..len], 64, 64);
        }
        for at in (0..file.len()).step_by(step + 6) {
            let mut bytes = file.to_vec();
            bytes[at] ^= 0x5A;
            let _ = decode_jpeg_fit(&bytes, 64, 64);
        }
    }
}

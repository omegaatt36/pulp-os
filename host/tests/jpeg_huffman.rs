//! Entropy decoding of high bit-rate baseline JPEGs.
//!
//! The fixtures are `cjpeg` encodes of noisy plasma at quality 85-95 (long
//! Huffman codes, 4:2:0 / 4:2:2 / 4:4:4 / grey, restart intervals, optimised
//! tables). Every route and every reduction (output bounds that pick the full,
//! 1/2, 1/4 and 1/8 IDCT) must give the pixels recorded here, so a change to
//! the bit reader or the Huffman decoder cannot alter a single output bit.

use smol_epub::DecodedImage;
use smol_epub::jpeg::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("jpeg_huffman/", $name, ".jpg")).as_slice()
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

fn fnv(image: &DecodedImage) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in image
        .width
        .to_le_bytes()
        .into_iter()
        .chain(image.height.to_le_bytes())
        .chain(image.data.iter().copied())
    {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const BOUNDS: [(u16, u16); 5] = [(512, 384), (300, 200), (200, 150), (100, 70), (60, 40)];

// (fixture, bounds index) -> hash of the decoded bitmap
const GOLDEN: &[(&str, usize, u64)] = &[
    ("hq420", 0, 0xe708c510b1a71f26),
    ("hq420", 1, 0x851da37fe166150a),
    ("hq420", 2, 0xfaa45718866bd03b),
    ("hq420", 3, 0x6f205ba84294d155),
    ("hq420", 4, 0x9824680892570bb2),
    ("hq444_rst", 0, 0x5a2c17ec03f914db),
    ("hq444_rst", 1, 0x8b60ad4982200304),
    ("hq444_rst", 2, 0x1c54ac2ec8c573bc),
    ("hq444_rst", 3, 0x058bbd6d8d40d8f5),
    ("hq444_rst", 4, 0x94b2a108346ed648),
    ("hq_gray", 0, 0xf484f89349c2f54a),
    ("hq_gray", 1, 0x93de35846c1c94ff),
    ("hq_gray", 2, 0xc6b0159f0996797a),
    ("hq_gray", 3, 0x356a6f98db26f312),
    ("hq_gray", 4, 0xc5de8140a5dc3322),
    ("hq422_opt", 0, 0xf75ec89b817ac1ca),
    ("hq422_opt", 1, 0xb150b2182c94a57b),
    ("hq422_opt", 2, 0xf548d09cbfd0f29e),
    ("hq422_opt", 3, 0x22142a7218178571),
    ("hq422_opt", 4, 0x2aa3a31d4bda0d39),
];

#[test]
fn high_bit_rate_decodes_are_bit_exact_on_every_route() {
    let fixtures: [(&str, &[u8]); 4] = [
        ("hq420", fixture!("hq420")),
        ("hq444_rst", fixture!("hq444_rst")),
        ("hq_gray", fixture!("hq_gray")),
        ("hq422_opt", fixture!("hq422_opt")),
    ];
    let mut seen = Vec::new();
    for (name, bytes) in fixtures {
        let mut stored = vec![0xA5; 7];
        stored.extend_from_slice(bytes);
        let mut deflated = vec![0x5A; 3];
        let compressed = miniz_oxide::deflate::compress_to_vec(bytes, 6);
        deflated.extend_from_slice(&compressed);
        for (i, &(w, h)) in BOUNDS.iter().enumerate() {
            let a = decode_jpeg_fit(bytes, w, h).unwrap();
            let b = decode_jpeg_streaming(read_at(&stored), 7, bytes.len() as u32, w, h).unwrap();
            let c =
                decode_jpeg_deflate_streaming(read_at(&deflated), 3, compressed.len() as u32, w, h)
                    .unwrap();
            let hash = fnv(&a);
            assert_eq!(
                hash,
                fnv(&b),
                "{name} {w}x{h}: streaming differs from buffer"
            );
            assert_eq!(hash, fnv(&c), "{name} {w}x{h}: deflate differs from buffer");
            seen.push((name, i, hash));
        }
    }
    if GOLDEN.is_empty() {
        for (name, i, hash) in &seen {
            println!("    (\"{name}\", {i}, {hash:#018x}),");
        }
        panic!("GOLDEN is empty: copy the lines above into it");
    }
    assert_eq!(seen.as_slice(), GOLDEN);
}

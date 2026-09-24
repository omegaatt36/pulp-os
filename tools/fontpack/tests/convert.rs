//! Rasterisation must match build.rs (fontdue, coverage >= 100, MSB-first) and
//! must fail rather than clamp when a metric does not fit its record type (§4).
mod common;

use fontdue::{LineMetrics, Metrics};
use pulp_fontpack::convert::{ConvertError, convert, glyph_from_raster, line_metrics, scan_cmap};
use pulp_fontpack::format::WriteError;
use pulp_render::font_pack::{FontPack, SliceReader};

fn metrics(xmin: i32, ymin: i32, width: usize, height: usize, advance: f32) -> Metrics {
    Metrics {
        xmin,
        ymin,
        width,
        height,
        advance_width: advance,
        ..Metrics::default()
    }
}

#[test]
fn packs_coverage_like_build_rs() {
    // 9x2 glyph; row 0 coverage 100,99,255,0,0,0,0,0,100 -> bits 1010_0000 1000_0000
    // row 1 only x=8 inked -> 0000_0000 1000_0000 (threshold 100, build.rs THRESHOLD)
    #[rustfmt::skip]
    let cov = [
        100, 99, 255, 0, 0, 0, 0, 0, 100,
        0,   0,  0,   0, 0, 0, 0, 0, 255,
    ];
    let g = glyph_from_raster(0x41, &metrics(-2, -3, 9, 2, 10.5), &cov).unwrap();
    assert_eq!(g.bitmap, vec![0xA0, 0x80, 0x00, 0x80]);
    assert_eq!((g.width, g.height), (9, 2));
    // build.rs: advance = (advance_width + 0.5) as u8 -> 11
    assert_eq!(g.advance, 11);
    assert_eq!(g.offset_x, -2);
    // build.rs: offset_y = -ymin - h = 3 - 2 = 1
    assert_eq!(g.offset_y, 1);
    assert_eq!(g.code_point, 0x41);
}

#[test]
fn metric_overflow_is_an_error_not_a_clamp() {
    let field = |r: Result<_, ConvertError>| match r {
        Err(ConvertError::MetricOverflow { field, .. }) => field,
        other => panic!("expected MetricOverflow, got {other:?}"),
    };
    // u8 width/height: 256 does not fit
    assert_eq!(
        field(glyph_from_raster(1, &metrics(0, 0, 256, 1, 1.0), &[0; 256])),
        "width"
    );
    assert_eq!(
        field(glyph_from_raster(1, &metrics(0, 0, 1, 256, 1.0), &[0; 256])),
        "height"
    );
    // i8 offset_x: -129 does not fit
    assert_eq!(
        field(glyph_from_raster(1, &metrics(-129, 0, 1, 1, 1.0), &[0])),
        "offset_x"
    );
    // i8 offset_y = -ymin - h: ymin = -129, h = 1 -> 128 does not fit
    assert_eq!(
        field(glyph_from_raster(1, &metrics(0, -129, 1, 1, 1.0), &[0])),
        "offset_y"
    );
    // u8 advance: round-half-up 255.5 -> 256 does not fit; -1.0 -> -1 does not fit
    assert_eq!(
        field(glyph_from_raster(1, &metrics(0, 0, 1, 1, 255.5), &[0])),
        "advance"
    );
    assert_eq!(
        field(glyph_from_raster(1, &metrics(0, 0, 1, 1, -1.0), &[0])),
        "advance"
    );

    // the boundary values themselves fit
    let g = glyph_from_raster(1, &metrics(-128, -128, 255, 1, 255.4), &[0; 255]).unwrap();
    assert_eq!(
        (g.width, g.offset_x, g.offset_y, g.advance),
        (255, -128, 127, 255)
    );
}

#[test]
fn line_metrics_round_up_like_build_rs_and_fail_on_overflow() {
    let lm = |ascent: f32, new_line_size: f32| LineMetrics {
        ascent,
        descent: 0.0,
        line_gap: 0.0,
        new_line_size,
    };
    // build.rs: line_height = ceil(new_line_size), ascent = ceil(ascent)
    assert_eq!(line_metrics(&lm(22.1, 26.2)).unwrap(), (27, 23));
    assert!(matches!(
        line_metrics(&lm(1.0, 70_000.0)),
        Err(ConvertError::MetricOverflow {
            field: "line_height",
            ..
        })
    ));
    assert!(matches!(
        line_metrics(&lm(-3.0, 20.0)),
        Err(ConvertError::MetricOverflow {
            field: "ascent",
            ..
        })
    ));
}

/// build.rs `rasterize_char` bit packing, restated here as the oracle.
fn build_rs_bits(font: &fontdue::Font, ch: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
    let (m, cov) = font.rasterize(ch, px);
    let row_bytes = m.width.div_ceil(8);
    let mut bits = Vec::new();
    for y in 0..m.height {
        for bx in 0..row_bytes {
            let mut byte = 0u8;
            for bit in 0..8 {
                let x = bx * 8 + bit;
                if x < m.width && cov[y * m.width + x] >= 100 {
                    byte |= 1 << (7 - bit);
                }
            }
            bits.push(byte);
        }
    }
    (m, bits)
}

/// The subset pack, read back through the device loader, holds exactly
/// the build.rs raster of each character (Latin, Han, kana, CJK punctuation).
#[test]
fn subset_pack_matches_build_rs_rasterisation() {
    let ttf = common::subset_ttf();
    let license = common::subset_license();
    let conv = convert(&ttf, 24, 0x25A1, &license).unwrap();
    let mut reader = SliceReader(&conv.pack);
    let pack = FontPack::load(&mut reader, 24).expect("converter output passes the §6 checks");

    let scan = scan_cmap(&ttf).unwrap();
    let want: Vec<u32> = scan.mapped.iter().map(|&(cp, _)| cp).collect();
    assert_eq!(
        conv.code_points, want,
        "every best-cmap code point is packed"
    );
    assert_eq!(pack.glyph_count() as usize, want.len());
    let mut buf = vec![0u8; pack.max_glyph_len()];
    for cp in 0x20..=0x7Eu32 {
        let ch = char::from_u32(cp).unwrap();
        assert!(
            !pack.resolve(&mut reader, ch).unwrap().fallback,
            "ASCII {ch:?}"
        );
    }

    let font = fontdue::Font::from_bytes(ttf.as_slice(), fontdue::FontSettings::default()).unwrap();
    let lm = font.horizontal_line_metrics(24.0).unwrap();
    assert_eq!(pack.line_height(), lm.new_line_size.ceil() as u16);
    assert_eq!(pack.ascent(), lm.ascent.ceil() as u16);

    for ch in [
        'A', 'g', 'W', '?', ' ', '閱', '讀', '，', '「', 'ひ', 'カ', '—', '\u{25A1}',
    ] {
        let (m, bits) = build_rs_bits(&font, ch, 24.0);
        let (res, got) = pack.lookup(&mut reader, ch, &mut buf).unwrap();
        let rec = res.glyph;
        assert!(!res.fallback, "{ch:?} is in the subset");
        assert_eq!(rec.code_point, ch as u32);
        assert_eq!(got, bits.as_slice(), "{ch:?} bitmap");
        assert_eq!(rec.width as usize, m.width, "{ch:?}");
        assert_eq!(rec.height as usize, m.height, "{ch:?}");
        assert_eq!(rec.advance, (m.advance_width + 0.5) as u8, "{ch:?}");
        assert_eq!(rec.offset_x as i32, m.xmin, "{ch:?}");
        assert_eq!(rec.offset_y as i32, -m.ymin - m.height as i32, "{ch:?}");
    }
    assert_eq!(common::license_section(&conv.pack), license.as_slice());
}

/// R4: generation fails when the chosen fallback would draw nothing (U+0020
/// rasterises to a zero-size glyph) or is not in the font at all (Iansui has
/// no Hangul, so U+D55C is absent from the subset too).
#[test]
fn fallback_must_be_present_and_visible() {
    let ttf = common::subset_ttf();
    let license = b"test license\n";
    let mapped = scan_cmap(&ttf).unwrap().mapped;
    assert!(
        mapped.iter().any(|m| m.0 == 0x20),
        "precondition: U+0020 is in the font"
    );
    assert!(
        !mapped.iter().any(|m| m.0 == 0xD55C),
        "precondition: U+D55C is not in the font"
    );
    assert!(matches!(
        convert(&ttf, 16, 0x20, license),
        Err(ConvertError::Write(WriteError::FallbackInvisible(0x20)))
    ));
    assert!(matches!(
        convert(&ttf, 16, 0xD55C, license),
        Err(ConvertError::Write(WriteError::FallbackMissing(0xD55C)))
    ));
}

#[test]
fn malformed_cmap_glyph_index_is_rejected_before_rasterizing() {
    let mut ttf = common::subset_ttf();
    let be16 =
        |bytes: &[u8], off: usize| u16::from_be_bytes(bytes[off..off + 2].try_into().unwrap());
    let be32 =
        |bytes: &[u8], off: usize| u32::from_be_bytes(bytes[off..off + 4].try_into().unwrap());
    let table = |tag: &[u8; 4]| {
        let count = be16(&ttf, 4) as usize;
        (0..count)
            .find_map(|i| {
                let rec = 12 + i * 16;
                (&ttf[rec..rec + 4] == tag).then(|| be32(&ttf, rec + 8) as usize)
            })
            .unwrap()
    };
    let glyph_count = be16(&ttf, table(b"maxp") + 4);
    let cmap = table(b"cmap");
    let subtable = cmap + be32(&ttf, cmap + 8) as usize;
    assert_eq!(be16(&ttf, subtable), 4, "tracked subset uses cmap format 4");
    let segments = be16(&ttf, subtable + 6) as usize / 2;
    let first_delta = subtable + 16 + 4 * segments;
    // First segment covers ASCII U+0020..U+005B with idRangeOffset=0.
    // 0x41 + 235 = glyph 300, beyond this font's 205 glyphs.
    ttf[first_delta..first_delta + 2].copy_from_slice(&235i16.to_be_bytes());
    let scan = scan_cmap(&ttf).unwrap();
    assert!(
        scan.mapped
            .iter()
            .any(|&(cp, gid)| cp == 0x41 && gid >= glyph_count),
        "the damaged cmap must actually expose an out-of-range glyph"
    );

    let result = std::panic::catch_unwind(|| convert(&ttf, 16, 0x25A1, b"license"));
    assert!(matches!(result, Ok(Err(ConvertError::Font(_)))));
}

/// The render acceptance tests load a tracked pack. It is generated output:
/// its correctness comes from the converter's self-verification (shared
/// loader + per-glyph round trip, run again here), and this test keeps it
/// in step with the tracked subset TTF and license, byte for byte.
/// Regenerate with the command in tests/fixtures/README.txt.
#[test]
fn tracked_render_fixture_pack_is_reproducible() {
    let conv = convert(&common::subset_ttf(), 24, 0x25A1, &common::subset_license()).unwrap();
    let tracked = std::fs::read(common::tracked_pack_path()).unwrap();
    assert!(
        conv.pack == tracked,
        "render/tests/fixtures/IANSUI24.PFP is stale ({} bytes tracked, {} generated)",
        tracked.len(),
        conv.pack.len()
    );
}

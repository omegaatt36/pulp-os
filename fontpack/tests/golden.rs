//! Hand-written golden bytes are the format oracle; the builder is checked
//! against them (writer == contract) and the reader is checked against them
//! (reader == contract).
mod common;

use common::*;
use pulp_fontpack::{BuildError, FontInfo, GlyphEntry, Metrics, Pack, build_pack};

#[test]
fn crate_constants_match_the_documented_layout() {
    assert_eq!(pulp_fontpack::MAGIC, *b"PFNT");
    assert_eq!(pulp_fontpack::FORMAT_VERSION, 1);
    assert_eq!(pulp_fontpack::HEADER_LEN, HEADER_LEN);
    assert_eq!(pulp_fontpack::INDEX_RECORD_LEN, RECORD_LEN);
}

#[test]
fn golden_pack_parses_with_the_documented_header_fields() {
    let pack = Pack::parse(&GOLDEN).expect("golden pack is valid");
    let h = pack.header();
    assert_eq!(h.info.pixel_size, 16);
    assert_eq!(h.info.font_id, 0x0123_4567_89AB_CDEF);
    assert_eq!(h.info.line_height, 20);
    assert_eq!(h.info.ascent, 15);
    assert_eq!(h.glyph_count, 4);
    assert_eq!(h.bitmap_len, 7);
    assert_eq!(h.info, GOLDEN_INFO);
}

#[test]
fn golden_pack_lookup_returns_every_documented_glyph() {
    let pack = Pack::parse(&GOLDEN).unwrap();
    for (c, metrics, bitmap) in golden_glyphs() {
        let g = pack.find(c).unwrap_or_else(|| panic!("{c:?} missing"));
        assert_eq!(g.metrics, metrics, "{c:?} metrics");
        assert_eq!(g.bitmap, &bitmap[..], "{c:?} bitmap");
    }
}

#[test]
fn golden_pack_glyph_at_enumerates_in_index_order() {
    let pack = Pack::parse(&GOLDEN).unwrap();
    let expect = golden_glyphs();
    for (i, (c, metrics, bitmap)) in expect.iter().enumerate() {
        let (gc, g) = pack.glyph_at(i as u32).unwrap();
        assert_eq!(gc, *c);
        assert_eq!(g.metrics, *metrics);
        assert_eq!(g.bitmap, &bitmap[..]);
    }
    assert!(pack.glyph_at(4).is_none());
}

#[test]
fn golden_pack_does_not_find_neighbouring_codepoints() {
    let pack = Pack::parse(&GOLDEN).unwrap();
    for cp in [
        0x0u32, 0x40, 0x42, 0xFFFE, 0x1_0000, 0x2_0BB6, 0x2_0BB8, 0x10_FFFE, 0xD7FF, 0xE000,
    ] {
        let c = char::from_u32(cp).unwrap();
        assert!(pack.find(c).is_none(), "U+{cp:04X} must be absent");
    }
}

#[test]
fn golden_four_byte_scalar_is_stored_as_a_full_u32_codepoint() {
    // U+20BB7 occupies bytes [88,92) of the golden pack as B7 0B 02 00.
    assert_eq!(&GOLDEN[88..92], &[0xB7, 0x0B, 0x02, 0x00]);
    assert_eq!(get_u32(&GOLDEN, rec(2, R_CODEPOINT)), 0x20BB7);
    let pack = Pack::parse(&GOLDEN).unwrap();
    let g = pack.find('𠮷').unwrap();
    assert_eq!(g.metrics.offset_y, -2);
    assert_eq!(g.bitmap, &[0xFF, 0x80, 0x00, 0x80]);
}

#[test]
fn builder_output_equals_the_golden_bytes() {
    let built = build_pack(&GOLDEN_INFO, &golden_entries()).unwrap();
    assert_eq!(built.len(), 139);
    assert_eq!(built, GOLDEN.to_vec());
}

#[test]
fn builder_empty_pack_equals_the_golden_empty_bytes() {
    let built = build_pack(&GOLDEN_INFO, &[]).unwrap();
    assert_eq!(built, GOLDEN_EMPTY.to_vec());
}

#[test]
fn empty_pack_is_valid_and_finds_nothing() {
    let pack = Pack::parse(&GOLDEN_EMPTY).expect("a pack with zero glyphs is valid");
    let h = pack.header();
    assert_eq!(h.glyph_count, 0);
    assert_eq!(h.bitmap_len, 0);
    assert_eq!(h.info, GOLDEN_INFO);
    for c in ['\0', 'A', '𠮷', '\u{10FFFF}'] {
        assert!(pack.find(c).is_none());
    }
    assert!(pack.glyph_at(0).is_none());
}

#[test]
fn single_ascii_glyph_pack_roundtrips_through_the_builder() {
    let info = FontInfo {
        pixel_size: 12,
        font_id: 7,
        line_height: 14,
        ascent: 11,
    };
    let m = Metrics {
        advance: 7,
        offset_x: 0,
        offset_y: 9,
        width: 5,
        height: 3,
    };
    // width 5 -> stride 1, height 3 -> 3 bytes
    let entry = GlyphEntry {
        codepoint: 'A',
        metrics: m,
        bitmap: vec![0x70, 0x88, 0xF8],
    };
    let bytes = build_pack(&info, &[entry]).unwrap();
    // 44 header + 22 record + 3 bitmap
    assert_eq!(bytes.len(), 69);
    let pack = Pack::parse(&bytes).unwrap();
    assert_eq!(pack.header().info, info);
    assert_eq!(pack.header().glyph_count, 1);
    let g = pack.find('A').unwrap();
    assert_eq!(g.metrics, m);
    assert_eq!(g.bitmap, &[0x70, 0x88, 0xF8]);
    assert!(pack.find('B').is_none());
    assert!(pack.find('@').is_none());
}

#[test]
fn builder_rejects_duplicate_and_descending_codepoints() {
    let blank = |c: char| GlyphEntry {
        codepoint: c,
        metrics: Metrics {
            advance: 1,
            offset_x: 0,
            offset_y: 0,
            width: 0,
            height: 0,
        },
        bitmap: vec![],
    };
    assert!(matches!(
        build_pack(&GOLDEN_INFO, &[blank('b'), blank('b')]),
        Err(BuildError::NotStrictlyIncreasing { index: 1 })
    ));
    assert!(matches!(
        build_pack(&GOLDEN_INFO, &[blank('a'), blank('c'), blank('b')]),
        Err(BuildError::NotStrictlyIncreasing { index: 2 })
    ));
}

#[test]
fn builder_rejects_bitmap_length_that_does_not_match_the_metrics() {
    let mut entries = golden_entries();
    entries[2].bitmap.push(0); // 9x2 needs exactly 4 bytes
    assert!(matches!(
        build_pack(&GOLDEN_INFO, &entries),
        Err(BuildError::BitmapSizeMismatch { index: 2 })
    ));
    let mut entries = golden_entries();
    entries[3].bitmap = vec![0]; // blank glyph must carry no bytes
    assert!(matches!(
        build_pack(&GOLDEN_INFO, &entries),
        Err(BuildError::BitmapSizeMismatch { index: 3 })
    ));
}

#[test]
fn error_display_tells_version_mismatch_from_corruption() {
    let version = err(&golden_patched(|b| put_u16(b, H_VERSION, 9)));
    let corrupt = err(&golden_patched(|b| b[0] = b'X'));
    let (v, c) = (format!("{version}"), format!("{corrupt}"));
    assert!(!v.is_empty() && !c.is_empty());
    assert_ne!(v, c);
    assert_ne!(version, corrupt);
}

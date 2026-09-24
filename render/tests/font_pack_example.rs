// docs/font-pack.txt §1 CRC check value and §10 worked example
//
// every expected value is copied from the spec text: the hex listing in
// §10, its prose (px 8, lh 10, ascent 8, U+25A1 3x3 adv 4 ox 0 oy -3,
// bitmap E0 A0 E0, index_crc32 0xE2A95D4A, header_crc32 0xB631F690) and
// the §1 check value crc32("123456789") = 0xCBF43926

mod common;

use common::{Glyph, Pack};
use pulp_render::crc32::{Crc32, crc32};
use pulp_render::font_pack::{FontPack, PackGlyph, SliceReader};

// §10 listing, byte for byte
fn worked_example() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&[
        0x50, 0x55, 0x4C, 0x50, 0x46, 0x4F, 0x4E, 0x54, 0x01, 0x00, 0x40, 0x00, 0x08, 0x00, 0x0A,
        0x00, //
        0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xA1, 0x25, 0x00, 0x00, 0x00, 0x02, 0x00,
        0x00, //
        0x10, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x13, 0x02, 0x00,
        0x00, //
        0x04, 0x00, 0x00, 0x00, 0x17, 0x02, 0x00, 0x00, 0x4A, 0x5D, 0xA9, 0xE2, 0x90, 0xF6, 0x31,
        0xB6,
    ]);
    f.extend_from_slice(&[0u8; 448]);
    f.extend_from_slice(&[
        0xA1, 0x25, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x03, 0x04, 0x00, 0xFD, 0x00, 0x00,
        0x00,
    ]);
    f.extend_from_slice(&[0xE0, 0xA0, 0xE0, 0x4F, 0x46, 0x4C, 0x0A]);
    f
}

#[test]
fn crc32_matches_the_iso_hdlc_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(common::crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn crc32_streaming_equals_one_shot() {
    let mut c = Crc32::new();
    c.update(b"1234");
    c.update(b"");
    c.update(b"56789");
    assert_eq!(c.finish(), 0xCBF4_3926);
}

#[test]
fn crc32_reproduces_the_worked_example_checksums() {
    let f = worked_example();
    assert_eq!(f.len(), 535);
    assert_eq!(crc32(&f[0x200..0x210]), 0xE2A9_5D4A);
    assert_eq!(crc32(&f[0..60]), 0xB631_F690);
}

#[test]
fn worked_example_loads_with_documented_fields() {
    let f = worked_example();
    let pack = FontPack::load(&mut SliceReader(&f), 8).expect("§10 example is valid");
    assert_eq!(pack.pixel_size(), 8);
    assert_eq!(pack.line_height(), 10);
    assert_eq!(pack.ascent(), 8);
    assert_eq!(pack.glyph_count(), 1);
    assert_eq!(pack.fallback_cp(), 0x25A1);
    assert_eq!(pack.bitmap_off(), 0x210);
    assert_eq!(pack.max_glyph_len(), 3);

    let expected = PackGlyph {
        code_point: 0x25A1,
        bitmap_offset: 0,
        width: 3,
        height: 3,
        advance: 4,
        offset_x: 0,
        offset_y: -3,
    };
    assert_eq!(pack.fallback_glyph(), expected);

    let mut buf = [0u8; 16];
    let (res, bits) = pack
        .lookup(&mut SliceReader(&f), '\u{25A1}', &mut buf)
        .unwrap();
    assert!(!res.fallback, "U+25A1 is in the pack");
    let g = res.glyph;
    assert_eq!(g, expected);
    assert_eq!(g.bitmap_len(), 3);
    assert_eq!(bits, &[0xE0, 0xA0, 0xE0]);
}

// cross-checks the test-side writer against the spec's own bytes, so the
// fixtures used by the other test files are anchored to an external oracle
#[test]
fn test_builder_reproduces_the_worked_example_bytes() {
    let pack = Pack {
        pixel_size: 8,
        line_height: 10,
        ascent: 8,
        fallback_cp: 0x25A1,
        glyphs: vec![Glyph {
            cp: 0x25A1,
            width: 3,
            height: 3,
            advance: 4,
            offset_x: 0,
            offset_y: -3,
            bitmap: vec![0xE0, 0xA0, 0xE0],
        }],
        license: b"OFL\n".to_vec(),
    };
    assert_eq!(pack.build().bytes, worked_example());
}

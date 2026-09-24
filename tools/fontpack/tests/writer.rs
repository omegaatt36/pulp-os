//! Writer output checked field by field against docs/font-pack.txt §2–§5.
mod common;

use common::{crc32, u16_at, u32_at};
use pulp_fontpack::format::{Glyph, PackInput, WriteError, write_pack};

const LICENSE: &[u8] =
    b"\xEF\xBB\xBFCopyright 2025 Example Authors.\nSIL OPEN FONT LICENSE Version 1.1\n";

fn g(cp: u32, w: u8, h: u8, adv: u8, ox: i8, oy: i8, bitmap: &[u8]) -> Glyph {
    Glyph {
        code_point: cp,
        width: w,
        height: h,
        advance: adv,
        offset_x: ox,
        offset_y: oy,
        bitmap: bitmap.to_vec(),
    }
}

/// Four glyphs chosen to cover: zero-size glyph, a 2-byte row (w=9), the
/// default fallback U+25A1, and a code point above U+FFFF.
fn glyphs() -> Vec<Glyph> {
    vec![
        g(0x20, 0, 0, 6, 0, 0, &[]),
        g(0x41, 9, 2, 10, 1, -2, &[0xFF, 0x80, 0x80, 0x80]),
        g(0x25A1, 3, 3, 4, 0, -3, &[0xE0, 0xA0, 0xE0]),
        g(0x2_0000, 8, 1, 9, -1, -5, &[0x81]),
    ]
}

fn input(glyphs: &[Glyph]) -> PackInput<'_> {
    PackInput {
        pixel_size: 24,
        line_height: 27,
        ascent: 23,
        fallback_cp: 0x25A1,
        glyphs,
        license: LICENSE,
    }
}

#[test]
fn header_fields_follow_spec() {
    let gs = glyphs();
    let b = write_pack(&input(&gs)).unwrap();

    // derivation: n = 4; index_len = 16*4 = 64; bitmap_off = 512+64 = 576;
    // bitmap_len = 0 + 2*2 + 1*3 + 1*1 = 8 (row_bytes*height per §5, nothing to dedup);
    // license_off = 584; file_len = 584 + LICENSE.len()
    let lic_len = LICENSE.len() as u32;
    assert_eq!(&b[0..8], b"PULPFONT");
    assert_eq!(&b[0..8], &[0x50, 0x55, 0x4C, 0x50, 0x46, 0x4F, 0x4E, 0x54]);
    assert_eq!(u16_at(&b, 8), 1, "format_version");
    assert_eq!(u16_at(&b, 10), 64, "header_len");
    assert_eq!(u16_at(&b, 12), 24, "pixel_size");
    assert_eq!(u16_at(&b, 14), 27, "line_height");
    assert_eq!(u16_at(&b, 16), 23, "ascent");
    assert_eq!(u16_at(&b, 18), 0, "reserved");
    assert_eq!(u32_at(&b, 20), 4, "glyph_count");
    assert_eq!(u32_at(&b, 24), 0x25A1, "fallback_cp");
    assert_eq!(u32_at(&b, 28), 512, "index_off");
    assert_eq!(u32_at(&b, 32), 64, "index_len");
    assert_eq!(u32_at(&b, 36), 576, "bitmap_off");
    assert_eq!(u32_at(&b, 40), 8, "bitmap_len");
    assert_eq!(u32_at(&b, 44), 584, "license_off");
    assert_eq!(u32_at(&b, 48), lic_len, "license_len");
    assert_eq!(u32_at(&b, 52), 584 + lic_len, "file_len");
    assert_eq!(
        b.len() as u32,
        584 + lic_len,
        "file_len equals actual length"
    );
    assert_eq!(u32_at(&b, 56), crc32(&b[512..576]), "index_crc32");
    assert_eq!(u32_at(&b, 60), crc32(&b[..60]), "header_crc32");
    assert!(b[64..512].iter().all(|&x| x == 0), "padding is zero");
}

#[test]
fn index_records_are_sorted_and_carry_metrics_and_bitmaps() {
    let gs = glyphs();
    let b = write_pack(&input(&gs)).unwrap();
    let bitmap_off = 576usize;
    let bitmap_len = 8u32;

    let mut prev: Option<u32> = None;
    let mut saw_fallback = false;
    for (i, want) in gs.iter().enumerate() {
        let r = &b[512 + 16 * i..512 + 16 * (i + 1)];
        let cp = u32_at(r, 0);
        if let Some(p) = prev {
            assert!(cp > p, "strictly ascending");
        }
        prev = Some(cp);
        saw_fallback |= cp == 0x25A1;

        assert_eq!(cp, want.code_point);
        assert_eq!(r[8], want.width);
        assert_eq!(r[9], want.height);
        assert_eq!(r[10], want.advance);
        assert_eq!(r[11] as i8, want.offset_x);
        assert_eq!(r[12] as i8, want.offset_y);
        assert_eq!(&r[13..16], &[0, 0, 0], "reserved");

        let off = u32_at(r, 4);
        let len = (want.width as u32).div_ceil(8) * want.height as u32;
        assert!(off + len <= bitmap_len);
        let at = bitmap_off + off as usize;
        assert_eq!(&b[at..at + len as usize], want.bitmap.as_slice());
    }
    assert!(saw_fallback, "fallback_cp is in the index");
}

#[test]
fn license_section_is_byte_exact() {
    let gs = glyphs();
    let b = write_pack(&input(&gs)).unwrap();
    assert_eq!(&b[584..], LICENSE);
}

#[test]
fn rejects_invalid_inputs_instead_of_repairing_them() {
    let gs = glyphs();

    let mut unsorted = gs.clone();
    unsorted.swap(1, 2);
    assert!(matches!(
        write_pack(&input(&unsorted)),
        Err(WriteError::Unsorted { .. })
    ));

    let mut dup = gs.clone();
    dup[1].code_point = 0x20;
    assert!(matches!(
        write_pack(&input(&dup)),
        Err(WriteError::Unsorted { .. })
    ));

    let mut surrogate = gs.clone();
    surrogate[2].code_point = 0xD800;
    surrogate.sort_by_key(|g| g.code_point);
    assert!(matches!(
        write_pack(&PackInput {
            fallback_cp: 0x41,
            ..input(&surrogate)
        }),
        Err(WriteError::InvalidCodePoint(0xD800))
    ));

    assert!(matches!(
        write_pack(&PackInput {
            fallback_cp: 0xFFFD,
            ..input(&gs)
        }),
        Err(WriteError::FallbackMissing(0xFFFD))
    ));

    assert!(matches!(
        write_pack(&PackInput {
            license: b"",
            ..input(&gs)
        }),
        Err(WriteError::EmptyLicense)
    ));

    let mut short = gs.clone();
    short[1].bitmap.pop();
    assert!(matches!(
        write_pack(&input(&short)),
        Err(WriteError::BitmapLength {
            code_point: 0x41,
            ..
        })
    ));

    for px in [7u16, 97] {
        assert!(matches!(
            write_pack(&PackInput {
                pixel_size: px,
                ..input(&gs)
            }),
            Err(WriteError::PixelSize(_))
        ));
    }
    assert!(matches!(
        write_pack(&PackInput {
            line_height: 0,
            ascent: 0,
            ..input(&gs)
        }),
        Err(WriteError::LineHeight)
    ));
    assert!(matches!(
        write_pack(&PackInput {
            ascent: 28,
            ..input(&gs)
        }),
        Err(WriteError::AscentAboveLineHeight { .. })
    ));
    assert!(matches!(
        write_pack(&input(&[])),
        Err(WriteError::GlyphCount(0))
    ));
}

/// R4: the fallback is drawn for every missing glyph, so it must put ink on
/// the page. A zero-size glyph (space) and a sized glyph whose bitmap has no
/// set bit are both invisible and rejected.
#[test]
fn rejects_an_invisible_fallback() {
    let gs = glyphs();
    // glyphs()[0] is U+0020, 0x0 px, empty bitmap
    assert_eq!(
        write_pack(&PackInput {
            fallback_cp: 0x20,
            ..input(&gs)
        }),
        Err(WriteError::FallbackInvisible(0x20))
    );

    // U+25A1 kept at 3x3 but blanked: rows 000 / 000 / 000
    let mut blank = gs.clone();
    blank[2].bitmap = vec![0x00, 0x00, 0x00];
    assert_eq!(
        write_pack(&input(&blank)),
        Err(WriteError::FallbackInvisible(0x25A1))
    );

    // a single set pixel is enough ink
    let mut dot = gs.clone();
    dot[2].bitmap = vec![0x00, 0x40, 0x00];
    assert!(write_pack(&input(&dot)).is_ok());
}

#[test]
fn row_padding_bits_do_not_make_a_fallback_visible() {
    let mut gs = glyphs();
    // The fallback is 3 pixels wide. Low five bits of every row are padding.
    gs[2].bitmap = vec![0x01, 0x01, 0x01];
    assert!(matches!(
        write_pack(&input(&gs)),
        Err(WriteError::FallbackInvisible(0x25A1))
    ));
}

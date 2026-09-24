// random-access glyph lookup by code point (docs/font-pack.txt §2, §4,
// §6 "Runtime lookup"), R3: glyphs beyond byte 65,535
//
// expected metrics, offsets and bitmap bytes are the values the fixture
// planted (common::planted / sized, laid out back to back by
// common::Pack::build) and arithmetic on the §2 layout; nothing is taken
// from running the loader

mod common;

use common::*;
use pulp_render::font_pack::{FontPack, LookupError, PackGlyph};

fn expected(g: &Glyph, offset: u32) -> PackGlyph {
    PackGlyph {
        code_point: g.cp,
        bitmap_offset: offset,
        width: g.width,
        height: g.height,
        advance: g.advance,
        offset_x: g.offset_x,
        offset_y: g.offset_y,
    }
}

fn ch(cp: u32) -> char {
    char::from_u32(cp).expect("fixture code points are scalars")
}

fn assert_glyph(pack: &FontPack, r: &mut TestReader, g: &Glyph, offset: u32) {
    let mut buf = [0u8; 8160];
    let (res, bits) = pack.lookup(r, ch(g.cp), &mut buf).unwrap();
    assert!(!res.fallback, "{:#X} missing", g.cp);
    assert_eq!(res.glyph, expected(g, offset), "{:#X}", g.cp);
    assert_eq!(bits, &g.bitmap[..], "{:#X}", g.cp);
}

// R3, section-relative reading: 9 filler glyphs of 255x255 px (32 row
// bytes x 255 rows = 8,160 B each, §5) push U+20000's bitmap to
// section offset 3 + 9 x 8,160 = 73,443 and file offset
// 512 + 11 x 16 + 73,443 = 74,131, both past 65,535
#[test]
fn r3_glyph_beyond_64k_section_relative_and_absolute() {
    let mut glyphs = vec![sized(0x25A1, 0, 3, 3)];
    glyphs.extend((0..9).map(|i| sized(0x4E00 + i, 100 + i as usize, 255, 255)));
    let target = sized(0x2_0000, 7, 17, 20);
    glyphs.push(target.clone());
    let built = Pack::new(24, 0x25A1, glyphs).build();

    assert_eq!(built.offsets[10], 73_443);
    assert_eq!(built.bitmap_off, 512 + 11 * 16);
    assert_eq!(built.bitmap_off + built.offsets[10], 74_131);

    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 24).unwrap();
    assert_eq!(pack.max_glyph_len(), 8160);
    assert_glyph(&pack, &mut r, &target, 73_443);
    // a filler is also exact, so the target is not read from a stale offset
    assert_glyph(&pack, &mut r, &sized(0x4E08, 108, 255, 255), 3 + 8 * 8160);
}

// R3, absolute reading only: 4,200 records make the index 67,200 B, so
// bitmap_off = 67,712 and every bitmap lies past file byte 65,535 while
// its section-relative offset stays small
#[test]
fn r3_glyph_beyond_64k_absolute_with_small_section_offset() {
    let glyphs: Vec<Glyph> = (0..4200)
        .map(|i| planted(0x3400 + 3 * i as u32, i))
        .collect();
    let built = Pack::new(16, 0x3400, glyphs.clone()).build();
    assert_eq!(built.bitmap_off, 512 + 4200 * 16);
    assert!(built.bitmap_off > 65_535);

    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 16).unwrap();
    for i in [0usize, 1, 2047, 2048, 4100, 4199] {
        assert!(built.offsets[i] <= 65_535, "fixture precondition");
        assert_glyph(&pack, &mut r, &glyphs[i], built.offsets[i]);
    }
}

// 100 records spaced two code points apart: sectors hold records
// 0..=31, 32..=63, 64..=95, 96..=99 (§2, 32 records per 512 B sector)
fn sectored() -> (Vec<Glyph>, Built) {
    let glyphs: Vec<Glyph> = (0..100).map(|i| planted(0x100 + 2 * i as u32, i)).collect();
    let built = Pack::new(12, 0x100 + 2 * 50, glyphs.clone()).build();
    (glyphs, built)
}

#[test]
fn lookup_first_last_and_around_sector_boundaries() {
    let (glyphs, built) = sectored();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    assert_eq!(pack.glyph_count(), 100);
    for i in [0usize, 1, 30, 31, 32, 33, 63, 64, 65, 95, 96, 98, 99] {
        assert_glyph(&pack, &mut r, &glyphs[i], built.offsets[i]);
    }
}

#[test]
fn absent_code_points_are_not_in_pack() {
    let (_, built) = sectored();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    let mut buf = [0u8; 64];
    let absent = [
        0x0,                // below everything
        0xFF,               // just below record 0
        0x101,              // between records 0 and 1
        0x100 + 2 * 31 + 1, // between record 31 and 32 (sector edge)
        0x100 + 2 * 63 + 1, // between record 63 and 64 (sector edge)
        0x100 + 2 * 99 + 1, // just above the last record
        0x10_FFFF,          // top of Unicode
        0xFFFF_FFFF,        // not a scalar at all
    ];
    for cp in absent {
        assert_eq!(pack.find(&mut r, cp).unwrap(), None, "{cp:#X}");
        // R4: the draw path substitutes the fallback (record 50) for a
        // scalar that is absent; 0xFFFF_FFFF cannot be a char at all
        if let Some(c) = char::from_u32(cp) {
            let (res, bits) = pack.lookup(&mut r, c, &mut buf).unwrap();
            assert!(res.fallback, "{cp:#X}");
            assert_eq!(res.glyph.code_point, 0x100 + 2 * 50, "{cp:#X}");
            assert_eq!(bits, &planted(0x100 + 2 * 50, 50).bitmap[..], "{cp:#X}");
        }
    }
}

#[test]
fn fallback_record_is_exposed() {
    let (glyphs, built) = sectored();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    assert_eq!(pack.fallback_cp(), 0x100 + 2 * 50);
    assert_eq!(
        pack.fallback_glyph(),
        expected(&glyphs[50], built.offsets[50])
    );

    let mut buf = [0u8; 64];
    let bits = pack
        .read_bitmap(&mut r, &pack.fallback_glyph(), &mut buf)
        .unwrap();
    assert_eq!(bits, &glyphs[50].bitmap[..]);
}

#[test]
fn zero_size_glyph_has_empty_bitmap() {
    let space = sized(0x20, 0, 0, 0);
    let built = Pack::new(8, 0x25A1, vec![space.clone(), planted(0x25A1, 1)]).build();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 8).unwrap();
    let mut buf = [0u8; 0];
    let (res, bits) = pack.lookup(&mut r, ' ', &mut buf).unwrap();
    assert!(!res.fallback);
    assert_eq!(res.glyph, expected(&space, 0));
    assert!(bits.is_empty());
}

// §2: "a lookup therefore costs one sector read for the index and one or
// two for the bitmap"; with a random-access reader that is at most one
// read_at of <= 512 index bytes plus one read_at of the glyph's bytes
#[test]
fn lookup_reads_are_bounded() {
    let (glyphs, built) = sectored();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    let mut buf = [0u8; 64];
    for i in [0usize, 31, 32, 40, 99] {
        r.reset_counts();
        let (res, _) = pack.lookup(&mut r, ch(glyphs[i].cp), &mut buf).unwrap();
        assert!(!res.fallback);
        let len = glyphs[i].bitmap.len();
        assert!(r.reads <= 2, "record {i}: {} reads", r.reads);
        assert!(
            r.bytes_read <= 512 + len,
            "record {i}: {} bytes",
            r.bytes_read
        );
    }
    for cp in [0xFF, 0x100 + 2 * 31 + 1, 0x10_FFFF] {
        r.reset_counts();
        assert!(pack.find(&mut r, cp).unwrap().is_none());
        assert!(r.reads <= 1 && r.bytes_read <= 512, "{cp:#X}");
    }
}

#[test]
fn runtime_read_failures_are_reported() {
    let (glyphs, built) = sectored();
    let mut r = TestReader::new(built.bytes.clone());
    let pack = FontPack::load(&mut r, 12).unwrap();
    let mut buf = [0u8; 64];

    // §6: after a good load only Io and Truncated (short read) remain
    r.fail_read = Some(pulp_render::font_pack::IoError::Io);
    assert_eq!(
        pack.lookup(&mut r, ch(glyphs[5].cp), &mut buf).err(),
        Some(LookupError::Io)
    );

    // card swapped for a shorter file: the bitmap read comes back short
    let mut short = TestReader::new(built.bytes[..built.bitmap_off as usize].to_vec());
    assert_eq!(
        pack.lookup(&mut short, ch(glyphs[5].cp), &mut buf).err(),
        Some(LookupError::Truncated)
    );
}

#[test]
fn bitmap_buffer_smaller_than_glyph_is_an_error() {
    let (glyphs, built) = sectored();
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    let g = &glyphs[12]; // 13 px wide x 13 rows = 26 B
    let mut buf = vec![0u8; g.bitmap.len() - 1];
    assert_eq!(
        pack.lookup(&mut r, ch(g.cp), &mut buf).err(),
        Some(LookupError::BufferTooSmall)
    );
}

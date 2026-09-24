// docs/font-pack.txt §6: one fixture per named load error, each built
// from a valid pack and corrupted in exactly one place; when the damage
// sits behind a CRC the fixture recomputes that CRC (common::fix_*_crc)
// so the loader reaches the targeted check
//
// expected errors are the §6 table's trigger -> name mapping; a load
// error must leave the caller on built-in fonts (R5, ActiveFont)

mod common;

use common::*;
use pulp_render::font_pack::{ActiveFont, FontPack, IoError, LoadError};

// U+0041, U+25A1 (fallback), U+4E2D at 8 px
fn base() -> Built {
    Pack::new(
        8,
        0x25A1,
        vec![planted(0x41, 1), planted(0x25A1, 2), planted(0x4E2D, 3)],
    )
    .build()
}

// (name, header patch) for table-driven cases
type Case = (&'static str, fn(&mut Vec<u8>));

fn load(bytes: Vec<u8>) -> Result<FontPack, LoadError> {
    FontPack::load(&mut TestReader::new(bytes), 8)
}

fn expect(bytes: Vec<u8>, err: LoadError) {
    assert_eq!(load(bytes).err(), Some(err));
}

fn header_patched(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut b = base().bytes;
    f(&mut b);
    fix_header_crc(&mut b);
    b
}

fn index_patched(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut b = base().bytes;
    f(&mut b);
    fix_index_crc(&mut b);
    b
}

#[test]
fn base_fixture_is_valid() {
    let pack = load(base().bytes).expect("base fixture loads");
    assert_eq!(pack.glyph_count(), 3);
}

#[test]
fn e1_not_found() {
    let mut r = TestReader::new(base().bytes);
    r.fail_size = Some(IoError::NotFound);
    assert_eq!(FontPack::load(&mut r, 8).err(), Some(LoadError::NotFound));
}

#[test]
fn e2_io_on_size_and_on_read() {
    let mut r = TestReader::new(base().bytes);
    r.fail_size = Some(IoError::Io);
    assert_eq!(FontPack::load(&mut r, 8).err(), Some(LoadError::Io));

    let mut r = TestReader::new(base().bytes);
    r.fail_read = Some(IoError::Io);
    assert_eq!(FontPack::load(&mut r, 8).err(), Some(LoadError::Io));
}

#[test]
fn e3_truncated_below_header() {
    let mut b = base().bytes;
    b.truncate(63);
    expect(b, LoadError::Truncated);
    expect(Vec::new(), LoadError::Truncated);
}

#[test]
fn e4_bad_magic() {
    expect(header_patched(|b| b[0] = b'X'), LoadError::BadMagic);
    expect(header_patched(|b| b[7] = b't'), LoadError::BadMagic);
}

#[test]
fn e5_unsupported_version() {
    expect(
        header_patched(|b| put_u16(b, VERSION, 2)),
        LoadError::UnsupportedVersion,
    );
    expect(
        header_patched(|b| put_u16(b, VERSION, 0)),
        LoadError::UnsupportedVersion,
    );
}

#[test]
fn e6_header_checksum_mismatch() {
    // a flipped reserved byte is covered by the CRC (§1)
    let mut b = base().bytes;
    b[RESERVED] ^= 1;
    expect(b, LoadError::HeaderChecksumMismatch);

    let mut b = base().bytes;
    b[HEADER_CRC] ^= 0x80;
    expect(b, LoadError::HeaderChecksumMismatch);
}

#[test]
fn e7_bad_header() {
    let cases: [Case; 9] = [
        ("header_len 63", |b| put_u16(b, HEADER_LEN, 63)),
        ("header_len 65", |b| put_u16(b, HEADER_LEN, 65)),
        ("pixel_size 7", |b| put_u16(b, PIXEL_SIZE, 7)),
        ("pixel_size 97", |b| put_u16(b, PIXEL_SIZE, 97)),
        ("line_height 0", |b| {
            put_u16(b, LINE_HEIGHT, 0);
            put_u16(b, ASCENT, 0);
        }),
        ("ascent > line_height", |b| put_u16(b, ASCENT, 11)),
        ("glyph_count 0", |b| put_u32(b, GLYPH_COUNT, 0)),
        ("glyph_count 65537", |b| put_u32(b, GLYPH_COUNT, 65_537)),
        ("license_len 0", |b| put_u32(b, LICENSE_LEN, 0)),
    ];
    for (name, patch) in cases {
        assert_eq!(
            load(header_patched(patch)).err(),
            Some(LoadError::BadHeader),
            "{name}"
        );
    }
}

#[test]
fn bad_header_bounds_are_inclusive() {
    // pixel_size 8..=96 and glyph_count 1..=65,536 are inclusive (§3):
    // edge values pass #7 and the file fails later, at #8 / #9
    let b = header_patched(|b| put_u16(b, PIXEL_SIZE, 96));
    assert_eq!(FontPack::load(&mut TestReader::new(b), 96).err(), None);
    expect(
        header_patched(|b| put_u32(b, GLYPH_COUNT, 65_536)),
        LoadError::BadLayout,
    );
}

#[test]
fn e8_wrong_pixel_size() {
    assert_eq!(
        FontPack::load(&mut TestReader::new(base().bytes), 9).err(),
        Some(LoadError::WrongPixelSize)
    );
}

#[test]
fn e9_bad_layout() {
    let cases: [Case; 5] = [
        ("index_off", |b| put_u32(b, INDEX_OFF, 1024)),
        ("index_len", |b| bump(b, INDEX_LEN, 16)),
        ("bitmap_off", |b| bump(b, BITMAP_OFF, 1)),
        ("license_off", |b| bump(b, LICENSE_OFF, 1)),
        ("file_len", |b| bump(b, FILE_LEN, 1)),
    ];
    for (name, patch) in cases {
        assert_eq!(
            load(header_patched(patch)).err(),
            Some(LoadError::BadLayout),
            "{name}"
        );
    }
}

// add delta to a stored u32 header field
fn bump(b: &mut [u8], off: usize, delta: u32) {
    let v = get_u32(b, off);
    put_u32(b, off, v + delta);
}

#[test]
fn e9_bad_layout_on_u32_overflow() {
    // stored offsets equal the wrapped sums, so only overflow detection
    // can reject this header
    let b = header_patched(|b| {
        let bitmap_off = get_u32(b, BITMAP_OFF);
        let bitmap_len = u32::MAX - 100;
        let license_off = bitmap_off.wrapping_add(bitmap_len);
        let file_len = license_off.wrapping_add(get_u32(b, LICENSE_LEN));
        put_u32(b, BITMAP_LEN, bitmap_len);
        put_u32(b, LICENSE_OFF, license_off);
        put_u32(b, FILE_LEN, file_len);
    });
    expect(b, LoadError::BadLayout);
}

#[test]
fn e10_truncated_below_file_len() {
    let mut b = base().bytes;
    b.pop();
    expect(b, LoadError::Truncated);

    let mut b = base().bytes;
    b.truncate(100); // whole header, index missing
    expect(b, LoadError::Truncated);
}

#[test]
fn e11_trailing_data() {
    let mut b = base().bytes;
    b.push(0);
    expect(b, LoadError::TrailingData);
}

#[test]
fn e12_index_checksum_mismatch() {
    let mut b = base().bytes;
    b[record_off(0) + 13] ^= 1; // reserved byte in record 0
    expect(b, LoadError::IndexChecksumMismatch);
}

#[test]
fn e13_invalid_code_point() {
    for cp in [0x11_0000u32, 0xD800, 0xDFFF] {
        let b = index_patched(|b| put_u32(b, record_off(2), cp));
        assert_eq!(load(b).err(), Some(LoadError::InvalidCodePoint), "{cp:#X}");
    }
}

#[test]
fn e14_unsorted() {
    // duplicate of the previous record, then strictly descending
    for cp in [0x25A1u32, 0x42] {
        let b = index_patched(|b| put_u32(b, record_off(2), cp));
        assert_eq!(load(b).err(), Some(LoadError::Unsorted), "{cp:#X}");
    }
}

#[test]
fn e15_bitmap_out_of_bounds() {
    let built = base();
    let bitmap_len = get_u32(&built.bytes, BITMAP_LEN);
    let g = planted(0x4E2D, 3);
    let len = bitmap_len_of(&g);
    // one byte past the end, then an offset whose sum only fits in u64
    for off in [bitmap_len - len + 1, u32::MAX] {
        let b = index_patched(|b| put_u32(b, record_off(2) + 4, off));
        assert_eq!(
            load(b).err(),
            Some(LoadError::BitmapOutOfBounds),
            "{off:#X}"
        );
    }
    // exactly at the end is in bounds (§6: error only when sum > bitmap_len)
    let b = index_patched(|b| put_u32(b, record_off(2) + 4, bitmap_len - len));
    assert!(load(b).is_ok());
}

fn bitmap_len_of(g: &Glyph) -> u32 {
    bitmap_len(g.width, g.height) as u32
}

#[test]
fn e16_fallback_missing() {
    expect(
        header_patched(|b| put_u32(b, FALLBACK_CP, 0x25A0)),
        LoadError::FallbackMissing,
    );
}

#[test]
fn index_crc_takes_priority_over_record_checks() {
    // §6: a structural record error with a bad CRC reads as a CRC error
    let mut b = base().bytes;
    put_u32(&mut b, record_off(2), 0x11_0000);
    expect(b, LoadError::IndexChecksumMismatch);
}

#[test]
fn first_record_failure_in_index_order_wins() {
    // record 1 out of bounds, record 2 unsorted: record 1 is first
    let b = index_patched(|b| {
        put_u32(b, record_off(1) + 4, u32::MAX);
        put_u32(b, record_off(2), 0x41);
    });
    expect(b, LoadError::BitmapOutOfBounds);

    // one record both invalid and out of bounds: 13 before 15
    let b = index_patched(|b| {
        put_u32(b, record_off(2), 0x11_0000);
        put_u32(b, record_off(2) + 4, u32::MAX);
    });
    expect(b, LoadError::InvalidCodePoint);
}

#[test]
fn record_failure_takes_priority_over_fallback_missing() {
    let mut b = base().bytes;
    put_u32(&mut b, FALLBACK_CP, 0x25A0);
    put_u32(&mut b, record_off(2), 0x42);
    fix_index_crc(&mut b);
    expect(b, LoadError::Unsorted);
}

#[test]
fn any_load_error_keeps_built_in_rendering() {
    let mut broken = base().bytes;
    broken[0] = 0;
    assert!(matches!(
        ActiveFont::select(load(broken)),
        ActiveFont::BuiltIn
    ));

    let mut r = TestReader::new(base().bytes);
    r.fail_size = Some(IoError::NotFound);
    assert!(matches!(
        ActiveFont::select(FontPack::load(&mut r, 8)),
        ActiveFont::BuiltIn
    ));

    assert!(matches!(
        ActiveFont::select(load(base().bytes)),
        ActiveFont::Pack(_)
    ));
}

#[test]
fn failed_load_keeps_reader_reusable() {
    // a failed load must not leave the reader unusable for the next attempt
    let mut r = TestReader::new(base().bytes);
    assert_eq!(
        FontPack::load(&mut r, 9).err(),
        Some(LoadError::WrongPixelSize)
    );
    assert!(FontPack::load(&mut r, 8).is_ok());
}

#[test]
fn checks_run_in_spec_order() {
    // each fixture fails two adjacent §6 checks; the earlier one wins
    let mut b = base().bytes;
    b[0] = b'X';
    b.truncate(63);
    expect(b, LoadError::Truncated); // #3 before #4

    expect(
        header_patched(|b| {
            b[0] = b'X';
            put_u16(b, VERSION, 2);
        }),
        LoadError::BadMagic,
    ); // #4 before #5

    let mut b = base().bytes;
    put_u16(&mut b, VERSION, 2); // header CRC left stale
    expect(b, LoadError::UnsupportedVersion); // #5 before #6

    let mut b = base().bytes;
    put_u16(&mut b, HEADER_LEN, 65); // header CRC left stale
    expect(b, LoadError::HeaderChecksumMismatch); // #6 before #7

    let b = header_patched(|b| put_u16(b, PIXEL_SIZE, 7));
    assert_eq!(
        FontPack::load(&mut TestReader::new(b), 8).err(),
        Some(LoadError::BadHeader)
    ); // #7 before #8 (7 is both out of range and not the request)

    let b = header_patched(|b| put_u32(b, INDEX_OFF, 1024));
    assert_eq!(
        FontPack::load(&mut TestReader::new(b), 9).err(),
        Some(LoadError::WrongPixelSize)
    ); // #8 before #9

    let mut b = header_patched(|b| put_u32(b, INDEX_OFF, 1024));
    b.pop();
    expect(b, LoadError::BadLayout); // #9 before #10

    let mut b = base().bytes;
    b[record_off(0) + 13] ^= 1;
    b.pop();
    expect(b, LoadError::Truncated); // #10 before #12

    let mut b = base().bytes;
    b[record_off(0) + 13] ^= 1;
    b.push(0);
    expect(b, LoadError::TrailingData); // #11 before #12
}

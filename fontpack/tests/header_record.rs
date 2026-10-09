//! Direct tests of the already-public header and record codecs:
//! `bitmap_size`, `Header::layout`, `Header::decode`,
//! `Record::decode`, `Record::validate`.
//!
//! Every expectation is a literal derived by hand (field
//! offsets, little-endian composition, u32 boundary arithmetic), never read back
//! from the implementation. The u32 boundary numbers used below:
//!
//!   u32::MAX                = 4_294_967_295
//!   195_225_784 * 22        = 4_294_967_248   (+44 = 4_294_967_292  <= MAX)
//!   195_225_785 * 22        = 4_294_967_270   (mul ok, +44 = 4_294_967_314 > MAX)
//!   195_225_786 * 22        = 4_294_967_292   (mul ok, +44 overflows)
//!   195_225_787 * 22        = 4_294_967_314   (mul overflows; wraps to 18)
mod common;

use common::*;
use pulp_fontpack::{FontInfo, Header, Layout, Metrics, PackError, Record, bitmap_size};

// ---------------------------------------------------------------- helpers

/// The eleven header fields, in file order, raw.
#[derive(Clone, Copy)]
struct Raw {
    version: u16,
    pixel_size: u16,
    font_id: u64,
    line_height: u16,
    ascent: u16,
    count: u32,
    index_offset: u32,
    index_len: u32,
    bitmap_offset: u32,
    bitmap_len: u32,
    total_len: u32,
}

impl Raw {
    /// A fully consistent header for `count` records and `bitmap_len` bitmap
    /// bytes (small values only: no overflow).
    fn ok(count: u32, bitmap_len: u32) -> Raw {
        let index_len = count * 22;
        Raw {
            version: 1,
            pixel_size: 0x1234,
            font_id: 0x0102_0304_0506_0708,
            line_height: 0xA1B2,
            ascent: 0xC3D4,
            count,
            index_offset: 44,
            index_len,
            bitmap_offset: 44 + index_len,
            bitmap_len,
            total_len: 44 + index_len + bitmap_len,
        }
    }

    fn bytes(&self) -> [u8; 44] {
        let mut b = [0u8; 44];
        b[0..4].copy_from_slice(b"PFNT");
        put_u16(&mut b, H_VERSION, self.version);
        put_u16(&mut b, H_PIXEL_SIZE, self.pixel_size);
        b[H_FONT_ID..H_FONT_ID + 8].copy_from_slice(&self.font_id.to_le_bytes());
        put_u16(&mut b, H_LINE_HEIGHT, self.line_height);
        put_u16(&mut b, H_ASCENT, self.ascent);
        put_u32(&mut b, H_COUNT, self.count);
        put_u32(&mut b, H_INDEX_OFFSET, self.index_offset);
        put_u32(&mut b, H_INDEX_LEN, self.index_len);
        put_u32(&mut b, H_BITMAP_OFFSET, self.bitmap_offset);
        put_u32(&mut b, H_BITMAP_LEN, self.bitmap_len);
        put_u32(&mut b, H_TOTAL_LEN, self.total_len);
        b
    }

    /// Decode with `file_len == total_len` (the file is exactly as long as it says).
    fn decode(&self) -> Result<Header, PackError> {
        Header::decode(&self.bytes(), self.total_len as u64)
    }
}

fn lay(h: Header) -> Option<(u32, u32, u32)> {
    h.layout()
        .map(|l: Layout| (l.index_len, l.bitmap_offset, l.total_len))
}

fn header(count: u32, bitmap_len: u32) -> Header {
    Header {
        info: FontInfo {
            pixel_size: 1,
            font_id: 2,
            line_height: 3,
            ascent: 4,
        },
        glyph_count: count,
        bitmap_len,
    }
}

fn metrics(width: u16, height: u16) -> Metrics {
    Metrics {
        advance: 0,
        offset_x: 0,
        offset_y: 0,
        width,
        height,
    }
}

fn record(cp: u32, off: u32, len: u32, w: u16, h: u16) -> Record {
    Record {
        codepoint: cp,
        bitmap_offset: off,
        bitmap_len: len,
        metrics: metrics(w, h),
    }
}

/// A record that passes every rule against region_len 2 with prev None.
fn good() -> Record {
    record(0x41, 0, 2, 8, 2)
}

// ------------------------------------------------------------ bitmap_size

#[test]
fn bitmap_size_is_ceil_width_over_8_times_height() {
    // hand-derived: stride = ceil(w/8); size = stride * h
    let cases: [(u16, u16, u32); 19] = [
        (0, 0, 0),
        (0, 9, 0),
        (9, 0, 0),
        (1, 1, 1),
        (7, 1, 1),
        (8, 1, 1),
        (9, 1, 2),
        (15, 1, 2),
        (16, 1, 2),
        (17, 1, 3),
        (7, 7, 7),
        (8, 8, 8),
        (9, 3, 6),
        (15, 2, 4),
        (17, 3, 9),
        (64, 30, 240),
        (23, 23, 69),
        // 65535 -> stride 8192; 8192 * 65535 = 536_862_720, needs u32 arithmetic
        (65535, 65535, 536_862_720),
        (65535, 1, 8192),
    ];
    for (w, h, want) in cases {
        assert_eq!(bitmap_size(w, h), want, "bitmap_size({w}, {h})");
    }
}

// ------------------------------------------------------------ Header::layout

#[test]
fn layout_of_small_headers_is_44_plus_22_per_glyph() {
    // gc 0 / bl 0 -> (0, 44, 44); gc 3 / bl 7 -> (66, 110, 117); golden gc 4 / bl 7 -> (88, 132, 139)
    assert_eq!(lay(header(0, 0)), Some((0, 44, 44)));
    assert_eq!(lay(header(1, 0)), Some((22, 66, 66)));
    assert_eq!(lay(header(3, 7)), Some((66, 110, 117)));
    assert_eq!(lay(header(4, 7)), Some((88, 132, 139)));
    assert_eq!(lay(header(0, 1000)), Some((0, 44, 1044)));
}

#[test]
fn layout_just_below_every_u32_boundary_is_some_with_exact_values() {
    // gc 195_225_784: index_len 4_294_967_248, bitmap_offset 4_294_967_292 (<= MAX)
    // bitmap_len 3 -> total exactly u32::MAX
    assert_eq!(
        lay(header(195_225_784, 0)),
        Some((4_294_967_248, 4_294_967_292, 4_294_967_292))
    );
    assert_eq!(
        lay(header(195_225_784, 3)),
        Some((4_294_967_248, 4_294_967_292, u32::MAX))
    );
    // gc 0: bitmap_offset 44, bitmap_len MAX-44 -> total exactly MAX
    assert_eq!(lay(header(0, u32::MAX - 44)), Some((0, 44, u32::MAX)));
    // gc 1: bitmap_offset 66, bitmap_len MAX-66 -> total exactly MAX
    assert_eq!(lay(header(1, u32::MAX - 66)), Some((22, 66, u32::MAX)));
}

#[test]
fn layout_is_none_when_the_total_overflows_by_one() {
    // total = 44 + 22*gc + bitmap_len, one past u32::MAX
    assert!(header(0, u32::MAX - 43).layout().is_none());
    assert!(header(0, u32::MAX).layout().is_none());
    assert!(header(1, u32::MAX - 65).layout().is_none());
    assert!(header(195_225_784, 4).layout().is_none());
    assert!(header(195_225_784, u32::MAX).layout().is_none());
}

#[test]
fn layout_is_none_when_bitmap_offset_overflows_even_with_empty_bitmap() {
    // 44 + index_len overflows: gc 195_225_785 (index_len 4_294_967_270, +44 = 4_294_967_314)
    // and gc 195_225_786 (index_len 4_294_967_292). In both the multiplication itself fits.
    assert!(header(195_225_785, 0).layout().is_none());
    assert!(header(195_225_786, 0).layout().is_none());
}

#[test]
fn layout_is_none_when_glyph_count_times_22_overflows() {
    // 195_225_787 * 22 = 4_294_967_314 > MAX (wraps to 18; a wrapping implementation would say Some)
    assert!(header(195_225_787, 0).layout().is_none());
    // 2^32 / 22 region and way beyond; 0x0BA2_E8BA * 22 = 2^28 * ... keep to hand-checkable ones
    assert!(header(u32::MAX, 0).layout().is_none());
    // 0x8000_0000 * 22 = 11 * 2^32: wraps to exactly 0, a wrapping implementation reports Some
    assert!(header(0x8000_0000, 0).layout().is_none());
    // 0x1_0000_0000 / 22 is not an integer; 2_147_483_648 (above) wraps to 0, and
    // 390_451_573 * 22 = 8_589_934_606 = 2^33 + 14 wraps to 14
    assert!(header(390_451_573, 0).layout().is_none());
}

// ------------------------------------------------------------ Header::decode

#[test]
fn decode_reads_every_field_from_its_documented_position() {
    // distinct value in each field: a swapped or shifted field shows up
    let r = Raw::ok(3, 10);
    let h = r.decode().expect("consistent header");
    assert_eq!(h.info.pixel_size, 0x1234);
    assert_eq!(h.info.font_id, 0x0102_0304_0506_0708);
    assert_eq!(h.info.line_height, 0xA1B2);
    assert_eq!(h.info.ascent, 0xC3D4);
    assert_eq!(h.glyph_count, 3);
    assert_eq!(h.bitmap_len, 10);
}

#[test]
fn decode_accepts_the_golden_header_alone_and_with_the_rest_following() {
    let want = Header {
        info: GOLDEN_INFO,
        glyph_count: 4,
        bitmap_len: 7,
    };
    assert_eq!(Header::decode(&GOLDEN[..44], 139), Ok(want));
    assert_eq!(Header::decode(&GOLDEN, 139), Ok(want));
    // the 0-glyph header
    let empty = Header {
        info: GOLDEN_INFO,
        glyph_count: 0,
        bitmap_len: 0,
    };
    assert_eq!(Header::decode(&GOLDEN_EMPTY, 44), Ok(empty));
}

#[test]
fn decode_does_not_range_check_the_informational_fields() {
    // pixel_size / line_height / ascent / font_id: any value is accepted
    for (px, lh, asc, id) in [
        (0u16, 0u16, 0u16, 0u64),
        (u16::MAX, u16::MAX, u16::MAX, u64::MAX),
        (1, 0, u16::MAX, 1 << 63),
    ] {
        let mut r = Raw::ok(1, 0);
        r.pixel_size = px;
        r.line_height = lh;
        r.ascent = asc;
        r.font_id = id;
        let h = r.decode().expect("informational fields are unchecked");
        assert_eq!(h.info.pixel_size, px);
        assert_eq!(h.info.line_height, lh);
        assert_eq!(h.info.ascent, asc);
        assert_eq!(h.info.font_id, id);
    }
}

#[test]
fn decode_below_44_bytes_is_too_short_whatever_the_file_len() {
    let good = Raw::ok(0, 0).bytes();
    for n in 0..44 {
        for file_len in [0u64, n as u64, 44, 139, u64::MAX] {
            assert_eq!(
                Header::decode(&good[..n], file_len),
                Err(PackError::TooShort),
                "n={n} file_len={file_len}"
            );
        }
    }
    // TooShort beats BadMagic: 43 bytes of garbage
    assert_eq!(Header::decode(&[0xEE; 43], 43), Err(PackError::TooShort));
    // and 44 bytes is long enough
    assert_ne!(Header::decode(&good, 44), Err(PackError::TooShort));
}

#[test]
fn decode_rejects_each_magic_byte_with_bad_magic() {
    for i in 0..4 {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut b = Raw::ok(0, 0).bytes();
            b[i] ^= flip;
            assert_eq!(
                Header::decode(&b, 44),
                Err(PackError::BadMagic),
                "byte {i} ^ {flip:#x}"
            );
        }
    }
    // lower-case magic is not the magic
    let mut b = Raw::ok(0, 0).bytes();
    b[0..4].copy_from_slice(b"pfnt");
    assert_eq!(Header::decode(&b, 44), Err(PackError::BadMagic));
}

#[test]
fn decode_reports_the_version_found_for_every_version_but_1() {
    for v in [0u16, 2, 3, 0x0100, 0x7FFF, 0x8000, 0xFFFF] {
        let mut r = Raw::ok(2, 4);
        r.version = v;
        assert_eq!(
            r.decode(),
            Err(PackError::UnsupportedVersion { found: v }),
            "version {v}"
        );
    }
}

#[test]
fn decode_error_precedence_is_short_magic_version_length_layout() {
    // damage everything at once, then repair one class at a time in priority order.
    // file_len 7 matches nothing.
    let mut r = Raw::ok(2, 4);
    r.version = 9;
    r.index_len = 1; // layout damage
    r.total_len = 1000; // length damage (file_len below is 7)
    let mut b = r.bytes();
    b[0] = b'X'; // magic damage
    assert_eq!(Header::decode(&b, 7), Err(PackError::BadMagic));

    // magic fixed -> version wins over length and layout
    b[0] = b'P';
    assert_eq!(
        Header::decode(&b, 7),
        Err(PackError::UnsupportedVersion { found: 9 })
    );

    // version fixed -> length mismatch wins over layout damage
    put_u16(&mut b, H_VERSION, 1);
    assert_eq!(Header::decode(&b, 7), Err(PackError::LengthMismatch));

    // file_len now equals total_len -> the layout damage shows
    assert_eq!(Header::decode(&b, 1000), Err(PackError::BadLayout));
}

#[test]
fn decode_length_check_uses_file_len_not_bytes_len() {
    // 139-byte golden: exact file_len ok; one off either side is a length mismatch,
    // whether or not the slice itself is that long
    assert!(Header::decode(&GOLDEN, 139).is_ok());
    assert_eq!(Header::decode(&GOLDEN, 138), Err(PackError::LengthMismatch));
    assert_eq!(Header::decode(&GOLDEN, 140), Err(PackError::LengthMismatch));
    assert_eq!(
        Header::decode(&GOLDEN, 0x8B + (1 << 32)),
        Err(PackError::LengthMismatch)
    );
    assert_eq!(
        Header::decode(&GOLDEN, u64::MAX),
        Err(PackError::LengthMismatch)
    );
    // header-only slice, file_len bigger than the slice: legal when it matches total_len
    assert!(Header::decode(&GOLDEN[..44], 139).is_ok());
    // 2^32 + 139 must not be truncated to 139
    assert_eq!(
        Header::decode(&GOLDEN[..44], (1u64 << 32) + 139),
        Err(PackError::LengthMismatch)
    );
}

#[test]
fn decode_total_len_of_the_header_alone_is_judged_against_file_len() {
    // 0-glyph pack: total_len 44. file_len 45 / 43 mismatch
    assert_eq!(
        Header::decode(&GOLDEN_EMPTY, 45),
        Err(PackError::LengthMismatch)
    );
    assert_eq!(
        Header::decode(&GOLDEN_EMPTY, 43),
        Err(PackError::LengthMismatch)
    );
    assert_eq!(
        Header::decode(&GOLDEN_EMPTY, 0),
        Err(PackError::LengthMismatch)
    );
}

#[test]
fn decode_each_layout_field_inconsistency_alone_is_bad_layout() {
    // every case keeps file_len == total_len field, so the length check passes
    // and the layout check is what fires
    let base = Raw::ok(3, 10); // index_len 66, bitmap_offset 110, total 120
    let cases: Vec<(&str, Raw)> = vec![
        (
            "index_offset 43",
            Raw {
                index_offset: 43,
                ..base
            },
        ),
        (
            "index_offset 45",
            Raw {
                index_offset: 45,
                ..base
            },
        ),
        (
            "index_offset 0",
            Raw {
                index_offset: 0,
                ..base
            },
        ),
        (
            "index_len 65",
            Raw {
                index_len: 65,
                ..base
            },
        ),
        (
            "index_len 67",
            Raw {
                index_len: 67,
                ..base
            },
        ),
        (
            "index_len 0",
            Raw {
                index_len: 0,
                ..base
            },
        ),
        ("count 2", Raw { count: 2, ..base }),
        ("count 4", Raw { count: 4, ..base }),
        ("count 0", Raw { count: 0, ..base }),
        (
            "bitmap_offset 109",
            Raw {
                bitmap_offset: 109,
                ..base
            },
        ),
        (
            "bitmap_offset 111",
            Raw {
                bitmap_offset: 111,
                ..base
            },
        ),
        (
            "bitmap_len 9",
            Raw {
                bitmap_len: 9,
                ..base
            },
        ),
        (
            "bitmap_len 11",
            Raw {
                bitmap_len: 11,
                ..base
            },
        ),
        (
            "bitmap_len 0",
            Raw {
                bitmap_len: 0,
                ..base
            },
        ),
    ];
    for (name, r) in cases {
        assert_eq!(r.decode(), Err(PackError::BadLayout), "{name}");
    }
    // total_len inconsistent with bitmap_offset + bitmap_len, file_len following total_len
    for t in [119u32, 121, 0, 44, 110] {
        let r = Raw {
            total_len: t,
            ..base
        };
        assert_eq!(r.decode(), Err(PackError::BadLayout), "total_len {t}");
    }
    // the consistent one decodes
    assert!(base.decode().is_ok());
}

#[test]
fn decode_layout_overflow_is_damage_not_wraparound() {
    // each header is crafted so that WRAPPING u32 arithmetic would make it look consistent
    // (and file_len == total_len), so only overflow checking rejects it.

    // glyph_count * 22 wraps: 195_225_787 * 22 = 2^32 + 18 -> index_len 18, bitmap_offset 62
    let r = Raw {
        count: 195_225_787,
        index_len: 18,
        bitmap_offset: 62,
        bitmap_len: 0,
        total_len: 62,
        ..Raw::ok(0, 0)
    };
    assert_eq!(r.decode(), Err(PackError::BadLayout));

    // 0x8000_0000 * 22 wraps to exactly 0
    let r = Raw {
        count: 0x8000_0000,
        index_len: 0,
        bitmap_offset: 44,
        bitmap_len: 0,
        total_len: 44,
        ..Raw::ok(0, 0)
    };
    assert_eq!(r.decode(), Err(PackError::BadLayout));

    // index_offset + index_len wraps: 44 + 4_294_967_270 = 2^32 + 18 -> 18
    let r = Raw {
        count: 195_225_785,
        index_len: 4_294_967_270,
        bitmap_offset: 18,
        bitmap_len: 0,
        total_len: 18,
        ..Raw::ok(0, 0)
    };
    assert_eq!(r.decode(), Err(PackError::BadLayout));

    // bitmap_offset + bitmap_len wraps: 44 + u32::MAX = 2^32 + 43 -> 43
    let r = Raw {
        count: 0,
        index_len: 0,
        bitmap_offset: 44,
        bitmap_len: u32::MAX,
        total_len: 43,
        ..Raw::ok(0, 0)
    };
    assert_eq!(r.decode(), Err(PackError::BadLayout));
}

#[test]
fn decode_accepts_headers_at_the_largest_representable_layout() {
    // header only in `bytes`, the file is declared u32::MAX long: all sums are exactly u32::MAX
    let r = Raw {
        count: 0,
        index_len: 0,
        bitmap_offset: 44,
        bitmap_len: u32::MAX - 44,
        total_len: u32::MAX,
        ..Raw::ok(0, 0)
    };
    let h = Header::decode(&r.bytes(), u32::MAX as u64).expect("largest layout is valid");
    assert_eq!((h.glyph_count, h.bitmap_len), (0, u32::MAX - 44));

    let r = Raw {
        count: 195_225_784,
        index_len: 4_294_967_248,
        bitmap_offset: 4_294_967_292,
        bitmap_len: 3,
        total_len: u32::MAX,
        ..Raw::ok(0, 0)
    };
    let h = Header::decode(&r.bytes(), u32::MAX as u64).expect("largest index is valid");
    assert_eq!((h.glyph_count, h.bitmap_len), (195_225_784, 3));
    // the same header one byte shorter on disk is a length mismatch
    assert_eq!(
        Header::decode(&r.bytes(), u32::MAX as u64 - 1),
        Err(PackError::LengthMismatch)
    );
}

#[test]
fn decode_layout_and_header_layout_agree() {
    // whatever decode accepts, layout() of the result reproduces the three derived fields
    for (count, bl) in [(0u32, 0u32), (1, 0), (4, 7), (100, 12345)] {
        let r = Raw::ok(count, bl);
        let h = r.decode().unwrap();
        assert_eq!(
            lay(h),
            Some((r.index_len, r.bitmap_offset, r.total_len)),
            "count {count}"
        );
    }
}

// ------------------------------------------------------------ Record::decode

fn assert_record(
    r: &Record,
    cp: u32,
    off: u32,
    len: u32,
    adv: u16,
    ox: i16,
    oy: i16,
    w: u16,
    h: u16,
) {
    assert_eq!(r.codepoint, cp, "codepoint");
    assert_eq!(r.bitmap_offset, off, "bitmap_offset");
    assert_eq!(r.bitmap_len, len, "bitmap_len");
    assert_eq!(r.metrics.advance, adv, "advance");
    assert_eq!(r.metrics.offset_x, ox, "offset_x");
    assert_eq!(r.metrics.offset_y, oy, "offset_y");
    assert_eq!(r.metrics.width, w, "width");
    assert_eq!(r.metrics.height, h, "height");
}

#[test]
fn record_decode_places_every_field_little_endian() {
    // bytes 1..=22: field values are the little-endian compositions below
    let bytes: Vec<u8> = (1..=22).collect();
    let r = Record::decode(&bytes).expect("22 bytes");
    assert_record(
        &r,
        0x0403_0201,
        0x0807_0605,
        0x0C0B_0A09,
        0x0E0D,
        0x100F,
        0x1211,
        0x1413,
        0x1615,
    );
}

#[test]
fn record_decode_signed_fields_are_twos_complement() {
    // bytes 0xFF, 0xFE, ... 0xEA (descending): each i16 has its sign bit set
    let bytes: Vec<u8> = (0..22u8).map(|i| 0xFF - i).collect();
    let r = Record::decode(&bytes).expect("22 bytes");
    assert_record(
        &r,
        0xFCFD_FEFF,
        0xF8F9_FAFB,
        0xF4F5_F6F7,
        0xF2F3,
        0xF0F1u16 as i16, // -3855
        0xEEEFu16 as i16, // -4369
        0xECED,           // bytes ED, EC little-endian
        0xEAEB,           // bytes EB, EA little-endian
    );
    assert_eq!(r.metrics.offset_x, -3855);
    assert_eq!(r.metrics.offset_y, -4369);
    assert_eq!(r.metrics.width, 0xECED);
    assert_eq!(r.metrics.height, 0xEAEB);
}

#[test]
fn record_decode_each_field_alone_lands_in_that_field_only() {
    // all-zero record with one field set: catches swapped same-width fields
    type Setter = fn(&mut [u8]);
    let cases: [(&str, Setter, [u32; 8]); 8] = [
        (
            "codepoint",
            |b| put_u32(b, R_CODEPOINT, 0x0002_0BB7),
            [0x20BB7, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            "bitmap_offset",
            |b| put_u32(b, R_BITMAP_OFFSET, 0x0100_0001),
            [0, 0x0100_0001, 0, 0, 0, 0, 0, 0],
        ),
        (
            "bitmap_len",
            |b| put_u32(b, R_BITMAP_LEN, 0x8000_0002),
            [0, 0, 0x8000_0002, 0, 0, 0, 0, 0],
        ),
        (
            "advance",
            |b| put_u16(b, R_ADVANCE, 0xABCD),
            [0, 0, 0, 0xABCD, 0, 0, 0, 0],
        ),
        (
            "offset_x",
            |b| put_u16(b, R_OFFSET_X, 0x8001),
            [0, 0, 0, 0, 0x8001, 0, 0, 0],
        ),
        (
            "offset_y",
            |b| put_u16(b, R_OFFSET_Y, 0x7FFE),
            [0, 0, 0, 0, 0, 0x7FFE, 0, 0],
        ),
        (
            "width",
            |b| put_u16(b, R_WIDTH, 0x1234),
            [0, 0, 0, 0, 0, 0, 0x1234, 0],
        ),
        (
            "height",
            |b| put_u16(b, R_HEIGHT, 0x4321),
            [0, 0, 0, 0, 0, 0, 0, 0x4321],
        ),
    ];
    for (name, set, want) in cases {
        let mut b = [0u8; 22];
        set(&mut b);
        let r = Record::decode(&b).expect("22 bytes");
        let got = [
            r.codepoint,
            r.bitmap_offset,
            r.bitmap_len,
            r.metrics.advance as u32,
            r.metrics.offset_x as u16 as u32,
            r.metrics.offset_y as u16 as u32,
            r.metrics.width as u32,
            r.metrics.height as u32,
        ];
        assert_eq!(got, want, "only {name} is set");
    }
    // i16 extremes
    let mut b = [0u8; 22];
    b[R_OFFSET_X..R_OFFSET_X + 2].copy_from_slice(&[0x00, 0x80]);
    b[R_OFFSET_Y..R_OFFSET_Y + 2].copy_from_slice(&[0xFF, 0x7F]);
    let r = Record::decode(&b).unwrap();
    assert_eq!(
        (r.metrics.offset_x, r.metrics.offset_y),
        (i16::MIN, i16::MAX)
    );
}

#[test]
fn record_decode_of_golden_records_matches_the_annotated_bytes() {
    for (i, (c, m, bitmap)) in golden_glyphs().into_iter().enumerate() {
        let start = rec(i, 0);
        let r = Record::decode(&GOLDEN[start..start + 22]).expect("golden record");
        assert_eq!(r.codepoint, c as u32, "record {i}");
        assert_eq!(r.metrics, m, "record {i}");
        assert_eq!(r.bitmap_len as usize, bitmap.len(), "record {i}");
    }
    // bitmap offsets 0, 2, 3, 7 from the annotated golden bytes
    for (i, off) in [0u32, 2, 3, 7].into_iter().enumerate() {
        let start = rec(i, 0);
        let r = Record::decode(&GOLDEN[start..start + 22]).unwrap();
        assert_eq!(r.bitmap_offset, off, "record {i}");
    }
}

#[test]
fn record_decode_needs_22_bytes() {
    let bytes: Vec<u8> = (1..=22).collect();
    for n in 0..22 {
        assert!(Record::decode(&bytes[..n]).is_none(), "{n} bytes");
    }
    assert!(Record::decode(&bytes[..22]).is_some());
}

#[test]
fn record_decode_ignores_bytes_after_the_22nd() {
    let mut bytes: Vec<u8> = (1..=22).collect();
    bytes.extend_from_slice(&[0xFF; 40]);
    let r = Record::decode(&bytes).expect("longer slice");
    assert_record(
        &r,
        0x0403_0201,
        0x0807_0605,
        0x0C0B_0A09,
        0x0E0D,
        0x100F,
        0x1211,
        0x1413,
        0x1615,
    );
    // one extra byte
    let r = Record::decode(&bytes[..23]).unwrap();
    assert_eq!(r.metrics.height, 0x1615);
    // the whole golden file starting at a record: the next records are ignored
    let r = Record::decode(&GOLDEN[rec(0, 0)..]).unwrap();
    assert_eq!(r.codepoint, 0x41);
    assert_eq!(r.metrics.height, 2);
}

// ----------------------------------------------------------- Record::validate

#[test]
fn validate_accepts_a_good_record_and_returns_the_char() {
    assert_eq!(good().validate(0, None, 2), Ok('A'));
    assert_eq!(good().validate(5, Some('@'), 2), Ok('A'));
    // the index argument does not influence success
    assert_eq!(good().validate(u32::MAX, None, 2), Ok('A'));
    // golden records validate against region_len 7 with the right predecessors
    let mut prev = None;
    for (i, (c, _, _)) in golden_glyphs().into_iter().enumerate() {
        let s = rec(i, 0);
        let r = Record::decode(&GOLDEN[s..s + 22]).unwrap();
        assert_eq!(r.validate(i as u32, prev, 7), Ok(c), "record {i}");
        prev = Some(c);
    }
}

#[test]
fn validate_codepoint_must_be_a_unicode_scalar() {
    // (codepoint, valid?): the boundaries around the surrogate block and the top
    let cases: [(u32, bool); 14] = [
        (0, true),
        (0xD7FE, true),
        (0xD7FF, true),
        (0xD800, false),
        (0xD801, false),
        (0xDBFF, false),
        (0xDC00, false),
        (0xDFFE, false),
        (0xDFFF, false),
        (0xE000, true),
        (0x10FFFF, true),
        (0x110000, false),
        (u32::MAX, false),
        (0x8000_0000, false),
    ];
    for (cp, ok) in cases {
        let mut r = good();
        r.codepoint = cp;
        let got = r.validate(9, None, 2);
        if ok {
            assert_eq!(got, Ok(char::from_u32(cp).unwrap()), "U+{cp:X}");
        } else {
            assert_eq!(
                got,
                Err(PackError::InvalidCodepoint { index: 9 }),
                "U+{cp:X}"
            );
        }
    }
}

#[test]
fn validate_order_requires_strictly_greater_than_prev() {
    let at = |cp: u32, prev: Option<char>| {
        let mut r = good();
        r.codepoint = cp;
        r.validate(3, prev, 2)
    };
    let ord = Err(PackError::CodepointOrder { index: 3 });
    // equal / lower: error; higher: ok
    assert_eq!(at(0x41, Some('A')), ord);
    assert_eq!(at(0x41, Some('B')), ord);
    assert_eq!(at(0x41, Some('\u{10FFFF}')), ord);
    assert_eq!(at(0x40, Some('A')), ord);
    assert_eq!(at(0x42, Some('A')), Ok('B'));
    assert_eq!(at(0x41, Some('@')), Ok('A'));
    assert_eq!(at(0x41, Some('\0')), Ok('A'));
    // prev None: anything goes, including U+0000
    assert_eq!(at(0, None), Ok('\0'));
    assert_eq!(at(0, Some('\0')), ord);
    assert_eq!(at(1, Some('\0')), Ok('\u{1}'));
    // across the surrogate gap and at the top
    assert_eq!(at(0xE000, Some('\u{D7FF}')), Ok('\u{E000}'));
    assert_eq!(at(0xD7FF, Some('\u{D7FF}')), ord);
    assert_eq!(at(0x10FFFF, Some('\u{10FFFE}')), Ok('\u{10FFFF}'));
    assert_eq!(at(0x10FFFF, Some('\u{10FFFF}')), ord);
}

#[test]
fn validate_bitmap_range_is_offset_plus_len_at_most_region_len() {
    let at = |off: u32, len: u32, w: u16, h: u16, region: u32| {
        record(0x41, off, len, w, h).validate(4, None, region)
    };
    let rng = Err(PackError::GlyphBitmapRange { index: 4 });
    // 8x2 glyph, len 2
    assert_eq!(at(0, 2, 8, 2, 2), Ok('A')); // ends exactly at region end
    assert_eq!(at(0, 2, 8, 2, 1), rng); // one past
    assert_eq!(at(1, 2, 8, 2, 2), rng); // offset+len = 3 > 2
    assert_eq!(at(1, 2, 8, 2, 3), Ok('A'));
    assert_eq!(at(10, 2, 8, 2, 11), rng);
    assert_eq!(at(9, 2, 8, 2, 11), Ok('A'));
    assert_eq!(at(0, 2, 8, 2, 0), rng);
    // blank glyph: offset may equal region_len, not exceed it
    assert_eq!(at(7, 0, 0, 0, 7), Ok('A'));
    assert_eq!(at(8, 0, 0, 0, 7), rng);
    assert_eq!(at(0, 0, 0, 0, 0), Ok('A'));
    assert_eq!(at(1, 0, 0, 0, 0), rng);
    // u32 limits, exact: offset+len == u32::MAX is representable and fine
    assert_eq!(at(u32::MAX - 1, 1, 8, 1, u32::MAX), Ok('A'));
    assert_eq!(at(u32::MAX, 0, 0, 0, u32::MAX), Ok('A'));
    // offset+len == 2^32 overflows; wrapping would give 0 <= region and pass
    assert_eq!(at(u32::MAX, 1, 8, 1, u32::MAX), rng);
    assert_eq!(at(u32::MAX, 1, 8, 1, 0), rng);
    assert_eq!(at(u32::MAX, 1, 8, 1, 5), rng);
    // a large glyph whose size rule holds: 8192 * 65535 = 536_862_720
    let big = bitmap_size(65535, 65535);
    assert_eq!(big, 536_862_720);
    assert_eq!(at(u32::MAX - big + 1, big, 65535, 65535, u32::MAX), rng);
    assert_eq!(at(u32::MAX - big, big, 65535, 65535, u32::MAX), Ok('A'));
}

#[test]
fn validate_size_rule_is_len_equals_ceil_width_over_8_times_height() {
    let at = |len: u32, w: u16, h: u16| record(0x41, 0, len, w, h).validate(8, None, u32::MAX);
    let sz = Err(PackError::GlyphSizeMismatch { index: 8 });
    // width 0 or height 0: only len 0
    assert_eq!(at(0, 0, 0), Ok('A'));
    assert_eq!(at(0, 0, 5), Ok('A'));
    assert_eq!(at(0, 100, 0), Ok('A'));
    assert_eq!(at(1, 0, 5), sz);
    assert_eq!(at(1, 100, 0), sz);
    assert_eq!(at(1, 0, 0), sz);
    // width 1 / 7 / 8: stride 1
    assert_eq!(at(1, 1, 1), Ok('A'));
    assert_eq!(at(0, 1, 1), sz);
    assert_eq!(at(2, 1, 1), sz);
    assert_eq!(at(3, 7, 3), Ok('A'));
    assert_eq!(at(1, 8, 1), Ok('A'));
    assert_eq!(at(8, 8, 8), Ok('A'));
    assert_eq!(at(7, 8, 8), sz);
    assert_eq!(at(9, 8, 8), sz);
    // width 9: stride 2 (a floor(w/8) implementation would say 1)
    assert_eq!(at(2, 9, 1), Ok('A'));
    assert_eq!(at(1, 9, 1), sz);
    assert_eq!(at(3, 9, 1), sz);
    assert_eq!(at(6, 9, 3), Ok('A'));
    assert_eq!(at(3, 9, 3), sz);
    // width 16 / 17: stride 2 / 3
    assert_eq!(at(2, 16, 1), Ok('A'));
    assert_eq!(at(3, 17, 1), Ok('A'));
    assert_eq!(at(2, 17, 1), sz);
    assert_eq!(at(9, 17, 3), Ok('A'));
    // a stride*height that is not divisible: width 12 height 5 -> 2*5 = 10
    assert_eq!(at(10, 12, 5), Ok('A'));
    assert_eq!(at(5, 12, 5), sz);
    assert_eq!(at(12, 12, 5), sz);
    // the full u16 range needs 32-bit arithmetic
    assert_eq!(at(536_862_720, 65535, 65535), Ok('A'));
    assert_eq!(at(536_862_720 - 1, 65535, 65535), sz);
    // a 16-bit product would wrap: 8192 * 65535 mod 65536 = 57344
    assert_eq!(at(57_344, 65535, 65535), sz);
    // swapping width and height changes the answer: w 8 h 3 -> 3; w 3 h 8 -> 8
    assert_eq!(at(3, 8, 3), Ok('A'));
    assert_eq!(at(3, 3, 8), sz);
    assert_eq!(at(8, 3, 8), Ok('A'));
}

#[test]
fn validate_checks_in_order_codepoint_then_order_then_range_then_size() {
    // a record failing everything, then repaired from the top
    // cp invalid, prev violated, range violated, size violated
    let all_bad = record(0xD800, 100, 3, 8, 2);
    assert_eq!(
        all_bad.validate(6, Some('\u{10FFFF}'), 2),
        Err(PackError::InvalidCodepoint { index: 6 })
    );
    // codepoint valid but not above prev; range and size still bad
    let mut r = all_bad;
    r.codepoint = 0x41;
    assert_eq!(
        r.validate(6, Some('A'), 2),
        Err(PackError::CodepointOrder { index: 6 })
    );
    // order ok; range bad (100+3 > 2) and size bad (3 != 2)
    assert_eq!(
        r.validate(6, Some('@'), 2),
        Err(PackError::GlyphBitmapRange { index: 6 })
    );
    // range ok (region 103), size still bad
    assert_eq!(
        r.validate(6, Some('@'), 103),
        Err(PackError::GlyphSizeMismatch { index: 6 })
    );
    // size repaired too
    r.bitmap_len = 2;
    assert_eq!(r.validate(6, Some('@'), 102), Ok('A'));
    // an out-of-range codepoint is reported even with prev None and everything else fine
    let mut r = good();
    r.codepoint = 0x110000;
    assert_eq!(
        r.validate(0, None, 2),
        Err(PackError::InvalidCodepoint { index: 0 })
    );
}

#[test]
fn validate_carries_the_index_argument_verbatim_in_every_error() {
    for index in [0u32, 1, 7, 65_535, 65_536, 0x00FF_FFFF, u32::MAX] {
        let mut a = good();
        a.codepoint = 0xDFFF;
        assert_eq!(
            a.validate(index, None, 2),
            Err(PackError::InvalidCodepoint { index })
        );
        assert_eq!(
            good().validate(index, Some('A'), 2),
            Err(PackError::CodepointOrder { index })
        );
        assert_eq!(
            good().validate(index, None, 1),
            Err(PackError::GlyphBitmapRange { index })
        );
        let mut c = good();
        c.bitmap_len = 3;
        assert_eq!(
            c.validate(index, None, 3),
            Err(PackError::GlyphSizeMismatch { index })
        );
    }
}

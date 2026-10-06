//! Damaged input matrix. Every case starts from the golden bytes and changes
//! the named field(s); each must be reported as a named error, never accepted
//! and never a panic. Version mismatch and structural damage are distinct
//! error variants.
mod common;

use common::*;
use pulp_fontpack::{Pack, PackError};

fn parse(bytes: &[u8]) -> Result<Pack<'_>, PackError> {
    Pack::parse(bytes)
}

// ---- header ---------------------------------------------------------------

#[test]
fn empty_input_is_too_short() {
    assert_eq!(err(&[]), PackError::TooShort);
}

#[test]
fn input_shorter_than_the_header_is_too_short() {
    for n in [1usize, 3, 4, 43] {
        assert_eq!(err(&GOLDEN[..n]), PackError::TooShort, "len {n}");
    }
}

#[test]
fn short_input_with_a_wrong_magic_is_still_too_short() {
    assert_eq!(err(b"XYZ"), PackError::TooShort);
}

#[test]
fn wrong_magic_is_bad_magic_for_every_flipped_magic_byte() {
    for i in 0..4 {
        let bytes = golden_patched(|b| b[H_MAGIC + i] ^= 0x20);
        assert_eq!(err(&bytes), PackError::BadMagic, "magic byte {i}");
    }
    assert_eq!(err(&[0u8; 139]), PackError::BadMagic);
    assert_eq!(err(&[0u8; 44]), PackError::BadMagic);
}

#[test]
fn version_above_and_below_the_supported_one_is_unsupported_version() {
    for v in [0u16, 2, 3, 0x0100, 0xFFFF] {
        let bytes = golden_patched(|b| put_u16(b, H_VERSION, v));
        assert_eq!(
            err(&bytes),
            PackError::UnsupportedVersion { found: v },
            "version {v}"
        );
    }
}

#[test]
fn version_mismatch_wins_over_structural_damage() {
    // Newer formats may lay the rest out differently, so the version is
    // judged before layout fields.
    let bytes = golden_patched(|b| {
        put_u16(b, H_VERSION, 2);
        put_u32(b, H_COUNT, 0xFFFF_FFFF);
        put_u32(b, H_TOTAL_LEN, 1);
    });
    assert_eq!(err(&bytes), PackError::UnsupportedVersion { found: 2 });
}

#[test]
fn version_mismatch_is_not_reported_for_structural_damage() {
    let damaged = [
        golden_patched(|b| put_u32(b, H_COUNT, 5)),
        golden_patched(|b| put_u32(b, H_TOTAL_LEN, 140)),
        golden_patched(|b| put_u32(b, rec(1, R_CODEPOINT), 0x41)),
        golden_patched(|b| b.truncate(100)),
    ];
    for (i, d) in damaged.iter().enumerate() {
        assert!(
            !matches!(err(d), PackError::UnsupportedVersion { .. }),
            "case {i} misreported as a version mismatch"
        );
    }
}

// ---- total length ---------------------------------------------------------

#[test]
fn total_len_field_that_differs_from_the_input_length_is_a_length_mismatch() {
    for v in [0u32, 44, 138, 140, 0xFFFF_FFFF] {
        let bytes = golden_patched(|b| put_u32(b, H_TOTAL_LEN, v));
        assert_eq!(err(&bytes), PackError::LengthMismatch, "total_len {v}");
    }
}

#[test]
fn trailing_bytes_after_the_declared_end_are_a_length_mismatch() {
    let mut bytes = GOLDEN.to_vec();
    bytes.push(0);
    assert_eq!(err(&bytes), PackError::LengthMismatch);
    bytes.extend_from_slice(&[0u8; 100]);
    assert_eq!(err(&bytes), PackError::LengthMismatch);
}

#[test]
fn declared_regions_larger_than_the_input_are_a_length_mismatch() {
    // Self-consistent header claiming 1000 glyphs, but the file is 139 bytes.
    let bytes = golden_patched(|b| {
        put_u32(b, H_COUNT, 1000);
        put_u32(b, H_INDEX_LEN, 22_000);
        put_u32(b, H_BITMAP_OFFSET, 22_044);
        put_u32(b, H_BITMAP_LEN, 7);
        put_u32(b, H_TOTAL_LEN, 22_051);
    });
    assert_eq!(err(&bytes), PackError::LengthMismatch);
}

// ---- layout ---------------------------------------------------------------

fn assert_layout(case: &str, bytes: &[u8]) {
    assert_eq!(err(bytes), PackError::BadLayout, "{case}");
}

#[test]
fn index_offset_other_than_directly_after_the_header_is_bad_layout() {
    assert_layout(
        "index overlaps header",
        &golden_patched(|b| put_u32(b, H_INDEX_OFFSET, 0)),
    );
    assert_layout(
        "index overlaps header end",
        &golden_patched(|b| put_u32(b, H_INDEX_OFFSET, 40)),
    );
    assert_layout(
        "gap before index",
        &golden_patched(|b| put_u32(b, H_INDEX_OFFSET, 48)),
    );
    assert_layout(
        "index offset huge",
        &golden_patched(|b| put_u32(b, H_INDEX_OFFSET, u32::MAX)),
    );
}

#[test]
fn glyph_count_inconsistent_with_index_len_is_bad_layout() {
    assert_layout(
        "count one too many",
        &golden_patched(|b| put_u32(b, H_COUNT, 5)),
    );
    assert_layout(
        "count one too few",
        &golden_patched(|b| put_u32(b, H_COUNT, 3)),
    );
    assert_layout("count zero", &golden_patched(|b| put_u32(b, H_COUNT, 0)));
    assert_layout(
        "count u32::MAX",
        &golden_patched(|b| put_u32(b, H_COUNT, u32::MAX)),
    );
    // count * 22 = 4_294_967_314 > u32::MAX
    assert_layout(
        "count*22 overflows u32",
        &golden_patched(|b| put_u32(b, H_COUNT, 0x0BA2_E8BC)),
    );
}

#[test]
fn index_len_that_is_not_count_times_record_size_is_bad_layout() {
    for v in [0u32, 66, 87, 89, 110, 0x1000, u32::MAX] {
        assert_layout(
            &format!("index_len {v}"),
            &golden_patched(|b| put_u32(b, H_INDEX_LEN, v)),
        );
    }
}

#[test]
fn bitmap_region_overlapping_the_index_is_bad_layout() {
    // bitmap [130,139): sum still equals total_len, but starts inside the index
    let bytes = golden_patched(|b| {
        put_u32(b, H_BITMAP_OFFSET, 130);
        put_u32(b, H_BITMAP_LEN, 9);
    });
    assert_layout("bitmap overlaps index end", &bytes);
    let bytes = golden_patched(|b| {
        put_u32(b, H_BITMAP_OFFSET, 44);
        put_u32(b, H_BITMAP_LEN, 95);
    });
    assert_layout("bitmap overlaps whole index", &bytes);
    let bytes = golden_patched(|b| {
        put_u32(b, H_BITMAP_OFFSET, 0);
        put_u32(b, H_BITMAP_LEN, 139);
    });
    assert_layout("bitmap overlaps header and index", &bytes);
}

#[test]
fn gap_between_index_and_bitmap_is_bad_layout() {
    let bytes = golden_patched(|b| {
        put_u32(b, H_BITMAP_OFFSET, 134);
        put_u32(b, H_BITMAP_LEN, 5);
    });
    assert_layout("two-byte gap", &bytes);
}

#[test]
fn bitmap_region_running_past_total_len_is_bad_layout() {
    assert_layout(
        "bitmap_len one too long",
        &golden_patched(|b| put_u32(b, H_BITMAP_LEN, 8)),
    );
    assert_layout(
        "bitmap_len one too short",
        &golden_patched(|b| put_u32(b, H_BITMAP_LEN, 6)),
    );
    assert_layout(
        "bitmap_len zero",
        &golden_patched(|b| put_u32(b, H_BITMAP_LEN, 0)),
    );
}

#[test]
fn bitmap_offset_plus_len_overflowing_u32_is_bad_layout() {
    assert_layout(
        "bitmap_len u32::MAX",
        &golden_patched(|b| put_u32(b, H_BITMAP_LEN, u32::MAX)),
    );
    assert_layout(
        "bitmap_offset u32::MAX",
        &golden_patched(|b| put_u32(b, H_BITMAP_OFFSET, u32::MAX)),
    );
    let bytes = golden_patched(|b| {
        put_u32(b, H_BITMAP_OFFSET, 0xFFFF_FFF0);
        put_u32(b, H_BITMAP_LEN, 0x20); // sum wraps to 0x10
    });
    assert_layout("offset + len wraps u32", &bytes);
}

#[test]
fn index_len_overflowing_u32_when_added_to_index_offset_is_bad_layout() {
    let bytes = golden_patched(|b| {
        // 195_225_786 * 22 = 4_294_967_292 = 0xFFFF_FFFC; + index_offset 44 wraps u32
        put_u32(b, H_INDEX_LEN, 0xFFFF_FFFC);
        put_u32(b, H_COUNT, 195_225_786);
    });
    assert_layout("index end wraps u32", &bytes);
}

// ---- truncation -----------------------------------------------------------

#[test]
fn every_proper_prefix_of_a_valid_pack_is_rejected() {
    for n in 0..GOLDEN.len() {
        let e = err(&GOLDEN[..n]);
        let want = if n < 44 {
            PackError::TooShort
        } else {
            PackError::LengthMismatch
        };
        assert_eq!(e, want, "prefix length {n}");
    }
    assert!(parse(&GOLDEN).is_ok());
}

#[test]
fn every_proper_prefix_of_the_empty_pack_is_rejected() {
    for n in 0..GOLDEN_EMPTY.len() {
        assert_eq!(
            err(&GOLDEN_EMPTY[..n]),
            PackError::TooShort,
            "prefix length {n}"
        );
    }
}

#[test]
fn every_proper_prefix_of_a_large_pack_is_rejected() {
    let bytes = pulp_fontpack::build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    let boundaries = [
        0usize, 1, 43, 44, 45, 65, 66, 197, 198, 199, 65_535, 65_536, 66_000,
    ];
    for &n in boundaries.iter().chain(&[bytes.len() - 2, bytes.len() - 1]) {
        assert!(parse(&bytes[..n]).is_err(), "prefix length {n} accepted");
    }
    assert!(parse(&bytes).is_ok());
}

#[test]
fn truncated_input_with_total_len_rewritten_to_match_is_still_rejected() {
    // An attacker (or a buggy writer) fixes up total_len but leaves the other
    // fields: the regions still do not add up.
    for n in 44..GOLDEN.len() {
        let mut bytes = GOLDEN[..n].to_vec();
        put_u32(&mut bytes, H_TOTAL_LEN, n as u32);
        assert_eq!(err(&bytes), PackError::BadLayout, "prefix length {n}");
    }
}

// ---- index records --------------------------------------------------------

#[test]
fn duplicate_codepoint_is_rejected() {
    let bytes = golden_patched(|b| put_u32(b, rec(1, R_CODEPOINT), 0x41));
    assert_eq!(err(&bytes), PackError::CodepointOrder { index: 1 });
}

#[test]
fn descending_codepoints_are_rejected() {
    // swap the first two codepoints: 0xFFFF, 0x41, ...
    let bytes = golden_patched(|b| {
        put_u32(b, rec(0, R_CODEPOINT), 0xFFFF);
        put_u32(b, rec(1, R_CODEPOINT), 0x41);
    });
    assert_eq!(err(&bytes), PackError::CodepointOrder { index: 1 });
    // last record drops below its predecessor (U+20BB7)
    let bytes = golden_patched(|b| put_u32(b, rec(3, R_CODEPOINT), 0x1_0000));
    assert_eq!(err(&bytes), PackError::CodepointOrder { index: 3 });
}

#[test]
fn codepoints_that_are_not_unicode_scalars_are_rejected() {
    // surrogate between 0x41 and 0xFFFF keeps the order intact
    let bytes = golden_patched(|b| put_u32(b, rec(1, R_CODEPOINT), 0xD800));
    assert_eq!(err(&bytes), PackError::InvalidCodepoint { index: 1 });
    let bytes = golden_patched(|b| put_u32(b, rec(1, R_CODEPOINT), 0xDFFF));
    assert_eq!(err(&bytes), PackError::InvalidCodepoint { index: 1 });
    // above U+10FFFF as the last record
    for v in [0x11_0000u32, 0x8000_0000, u32::MAX] {
        let bytes = golden_patched(|b| put_u32(b, rec(3, R_CODEPOINT), v));
        assert_eq!(
            err(&bytes),
            PackError::InvalidCodepoint { index: 3 },
            "U+{v:X}"
        );
    }
}

#[test]
fn glyph_bitmap_range_past_the_bitmap_region_is_rejected() {
    // offset 6 + len 2 = 8 > 7 (size is consistent with a 8x2 glyph)
    let bytes = golden_patched(|b| put_u32(b, rec(0, R_BITMAP_OFFSET), 6));
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 0 });
    // len grows to 8 with height 8 (so stride*h agrees): 0 + 8 > 7
    let bytes = golden_patched(|b| {
        put_u32(b, rec(0, R_BITMAP_LEN), 8);
        put_u16(b, rec(0, R_HEIGHT), 8);
    });
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 0 });
    // zero-length glyph with offset one past the end of the region
    let bytes = golden_patched(|b| put_u32(b, rec(3, R_BITMAP_OFFSET), 8));
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 3 });
    // offset far away
    let bytes = golden_patched(|b| put_u32(b, rec(2, R_BITMAP_OFFSET), 0x1_0000));
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 2 });
}

#[test]
fn glyph_bitmap_offset_plus_len_overflowing_u32_is_rejected() {
    for (off, len) in [
        (0xFFFF_FFFEu32, 2u32),
        (0xFFFF_FFFF, 2),
        (1, 0xFFFF_FFFF),
        (0xFFFF_FFF0, 0x20),
    ] {
        let bytes = golden_patched(|b| {
            put_u32(b, rec(0, R_BITMAP_OFFSET), off);
            put_u32(b, rec(0, R_BITMAP_LEN), len);
        });
        assert_eq!(
            err(&bytes),
            PackError::GlyphBitmapRange { index: 0 },
            "offset {off:#x} len {len:#x}"
        );
    }
}

#[test]
fn maximum_dimensions_do_not_overflow_the_length_check() {
    // 65535 x 65535 needs 8192 * 65535 bytes: far more than the region.
    let bytes = golden_patched(|b| {
        put_u16(b, rec(0, R_WIDTH), 0xFFFF);
        put_u16(b, rec(0, R_HEIGHT), 0xFFFF);
    });
    assert!(matches!(
        err(&bytes),
        PackError::GlyphSizeMismatch { index: 0 }
    ));
    // and with a length that is in range but wrong
    let bytes = golden_patched(|b| {
        put_u16(b, rec(2, R_WIDTH), 0xFFFF);
        put_u16(b, rec(2, R_HEIGHT), 0xFFFF);
        put_u32(b, rec(2, R_BITMAP_LEN), 0xFFFF_FFFF);
    });
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 2 });
}

#[test]
fn bitmap_len_not_equal_to_stride_times_height_is_rejected() {
    // record 0 is 8x2 -> 2 bytes
    for len in [1u32, 3] {
        let bytes = golden_patched(|b| put_u32(b, rec(0, R_BITMAP_LEN), len));
        assert_eq!(
            err(&bytes),
            PackError::GlyphSizeMismatch { index: 0 },
            "len {len}"
        );
    }
    // record 0 width 9 -> stride 2 -> 4 bytes expected, 2 present
    let bytes = golden_patched(|b| put_u16(b, rec(0, R_WIDTH), 9));
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 0 });
    // record 2 is 9x2 -> stride 2 -> 4; width 8 -> 2 expected, 4 present
    let bytes = golden_patched(|b| put_u16(b, rec(2, R_WIDTH), 8));
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 2 });
    // width 17 -> stride 3 -> 6 expected, 4 present
    let bytes = golden_patched(|b| put_u16(b, rec(2, R_WIDTH), 17));
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 2 });
    // height 3 -> 6 expected, 4 present
    let bytes = golden_patched(|b| put_u16(b, rec(2, R_HEIGHT), 3));
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 2 });
}

#[test]
fn blank_glyph_with_a_nonempty_bitmap_is_rejected() {
    // width 0 / height 0 glyph claiming 1 byte (offset 6 + 1 = 7, in range)
    let bytes = golden_patched(|b| {
        put_u32(b, rec(3, R_BITMAP_OFFSET), 6);
        put_u32(b, rec(3, R_BITMAP_LEN), 1);
    });
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 3 });
    // width 8, height 0 with length 1
    let bytes = golden_patched(|b| {
        put_u32(b, rec(3, R_BITMAP_OFFSET), 6);
        put_u32(b, rec(3, R_BITMAP_LEN), 1);
        put_u16(b, rec(3, R_WIDTH), 8);
    });
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 3 });
    // width 0, height 5 with length 1
    let bytes = golden_patched(|b| {
        put_u32(b, rec(3, R_BITMAP_OFFSET), 6);
        put_u32(b, rec(3, R_BITMAP_LEN), 1);
        put_u16(b, rec(3, R_HEIGHT), 5);
    });
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 3 });
}

#[test]
fn nonblank_glyph_with_a_zero_length_bitmap_is_rejected() {
    let bytes = golden_patched(|b| put_u32(b, rec(0, R_BITMAP_LEN), 0));
    assert_eq!(err(&bytes), PackError::GlyphSizeMismatch { index: 0 });
}

#[test]
fn first_bad_glyph_is_the_one_reported() {
    let bytes = golden_patched(|b| {
        put_u32(b, rec(2, R_BITMAP_LEN), 99);
        put_u32(b, rec(1, R_BITMAP_LEN), 99);
    });
    assert_eq!(err(&bytes), PackError::GlyphBitmapRange { index: 1 });
}

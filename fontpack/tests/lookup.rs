//! Unicode range and 16-bit boundary behaviour: scalars beyond U+FFFF, bitmap
//! regions beyond 64 KiB, and packs built so that a u16-truncated codepoint or
//! offset would return a different glyph.
mod common;

use common::*;
use pulp_fontpack::{FontInfo, GlyphEntry, Metrics, Pack, build_pack};

fn one_byte_glyph(codepoint: char, marker: u8) -> GlyphEntry {
    GlyphEntry {
        codepoint,
        metrics: Metrics {
            advance: 8,
            offset_x: 0,
            offset_y: 7,
            width: 8,
            height: 1,
        },
        bitmap: vec![marker],
    }
}

const INFO: FontInfo = FontInfo {
    pixel_size: 8,
    font_id: 1,
    line_height: 9,
    ascent: 7,
};

#[test]
fn scalars_around_the_16_bit_and_unicode_limits_resolve_to_their_own_glyph() {
    // Each marker is unique. If a codepoint were truncated to u16:
    //   U+10041 -> 0x0041, U+10000 -> 0x0000, U+20BB7 -> 0x0BB7
    // and would hit another glyph in this pack.
    let cps: [u32; 8] = [
        0x0, 0x41, 0xBB7, 0xFFFF, 0x1_0000, 0x1_0041, 0x2_0BB7, 0x10_FFFF,
    ];
    let entries: Vec<GlyphEntry> = cps
        .iter()
        .enumerate()
        .map(|(i, &cp)| one_byte_glyph(char::from_u32(cp).unwrap(), 0x10 + i as u8))
        .collect();
    let bytes = build_pack(&INFO, &entries).unwrap();
    let pack = Pack::parse(&bytes).unwrap();
    for (i, &cp) in cps.iter().enumerate() {
        let g = pack
            .find(char::from_u32(cp).unwrap())
            .unwrap_or_else(|| panic!("U+{cp:04X} not found"));
        assert_eq!(
            g.bitmap,
            &[0x10 + i as u8],
            "U+{cp:04X} returned another glyph"
        );
    }
    for absent in [
        0x1u32, 0x42, 0xBB6, 0xBB8, 0xFFFE, 0x1_0001, 0x1_0040, 0x1_0042, 0x1_FFFF, 0x2_0BB6,
        0x2_0BB8, 0x10_FFFE, 0xD7FF, 0xE000,
    ] {
        assert!(
            pack.find(char::from_u32(absent).unwrap()).is_none(),
            "U+{absent:04X} must be absent"
        );
    }
}

#[test]
fn builder_writes_the_full_32_bit_codepoint_for_supplementary_planes() {
    let bytes = build_pack(
        &INFO,
        &[one_byte_glyph('𠮷', 1), one_byte_glyph('\u{10FFFF}', 2)],
    )
    .unwrap();
    assert_eq!(get_u32(&bytes, rec(0, R_CODEPOINT)), 0x2_0BB7);
    assert_eq!(get_u32(&bytes, rec(1, R_CODEPOINT)), 0x10_FFFF);
}

#[test]
fn bitmap_region_over_64k_returns_the_right_bytes_for_every_glyph() {
    let entries = large_offset_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let pack = Pack::parse(&bytes).unwrap();
    let h = pack.header();
    assert_eq!(h.bitmap_len, LARGE_OFFSET_BITMAP_LEN);
    assert!(h.bitmap_len > 65_535);
    assert_eq!(h.glyph_count, 7);
    for (k, e) in entries.iter().enumerate() {
        let g = pack
            .find(e.codepoint)
            .unwrap_or_else(|| panic!("glyph {k} missing"));
        assert_eq!(g.metrics, e.metrics, "glyph {k} metrics");
        assert_eq!(g.bitmap.len(), e.bitmap.len(), "glyph {k} length");
        // Expected bytes come from pattern(), not from the builder's output.
        for (j, &b) in g.bitmap.iter().enumerate() {
            assert_eq!(b, pattern(k as u32, j as u32), "glyph {k} byte {j}");
        }
    }
}

#[test]
fn builder_places_bitmaps_back_to_back_across_the_16_bit_boundary() {
    let bytes = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    for (i, &off) in LARGE_OFFSET_EXPECTED.iter().enumerate() {
        assert_eq!(
            get_u32(&bytes, rec(i, R_BITMAP_OFFSET)),
            off,
            "glyph {i} offset"
        );
    }
    assert_eq!(get_u32(&bytes, H_BITMAP_LEN), LARGE_OFFSET_BITMAP_LEN);
    // header 44 + 7*22 index + bitmap
    assert_eq!(bytes.len(), 44 + 7 * 22 + 66_174);
}

#[test]
fn a_u16_truncated_offset_would_alias_another_glyph_and_the_reader_does_not() {
    let entries = large_offset_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let pack = Pack::parse(&bytes).unwrap();
    // Glyph 2 sits at bitmap offset 65_536, which truncates to 0 in a u16,
    // i.e. to the start of glyph 0.
    let truncated_view = &entries[0].bitmap[0..8];
    let right = &entries[2].bitmap[..];
    assert_ne!(truncated_view, right, "test premise: the two must differ");
    let g = pack.find('\u{100}').unwrap();
    assert_eq!(g.bitmap, right);
    assert_ne!(g.bitmap, truncated_view);
    // Glyph 1 sits at 65_535 (last u16 value) with length 1.
    assert_eq!(pack.find('\u{62}').unwrap().bitmap, &[pattern(1, 0)]);
    // Every glyph past the boundary: offset % 65_536 differs from the real one.
    for k in 2..7 {
        let e = &entries[k];
        assert_eq!(
            pack.find(e.codepoint).unwrap().bitmap,
            &e.bitmap[..],
            "glyph {k}"
        );
    }
}

#[test]
fn glyph_ending_exactly_at_the_end_of_a_large_bitmap_region_is_found() {
    let entries = large_offset_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let pack = Pack::parse(&bytes).unwrap();
    // U+10FFFF: offset 66_134, len 40, ends at 66_174 = bitmap_len.
    let g = pack.find('\u{10FFFF}').unwrap();
    assert_eq!(g.bitmap.len(), 40);
    assert_eq!(g.bitmap, &entries[6].bitmap[..]);
    assert_eq!(g.bitmap[39], pattern(6, 39));
}

#[test]
fn index_region_over_64k_and_bitmap_region_over_half_a_megabyte_resolve_every_glyph() {
    let entries = many_glyph_entries();
    let n = entries.len();
    assert!(n * 22 > 65_535, "premise: index region over 64 KiB");
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    assert!(get_u32(&bytes, H_BITMAP_OFFSET) > 65_535);
    assert!(get_u32(&bytes, H_BITMAP_LEN) as usize == n * 240);
    let pack = Pack::parse(&bytes).unwrap();
    assert_eq!(pack.header().glyph_count as usize, n);
    for (k, e) in entries.iter().enumerate() {
        let g = pack
            .find(e.codepoint)
            .unwrap_or_else(|| panic!("glyph {k} missing"));
        assert_eq!(g.metrics, e.metrics, "glyph {k}");
        assert_eq!(g.bitmap.len(), 240);
        // spot-check first, middle, last byte against the pattern oracle
        for j in [0usize, 119, 239] {
            assert_eq!(
                g.bitmap[j],
                pattern(k as u32, j as u32),
                "glyph {k} byte {j}"
            );
        }
    }
}

#[test]
fn find_agrees_with_a_linear_scan_over_a_sparse_sweep_of_all_scalars() {
    let entries = many_glyph_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let pack = Pack::parse(&bytes).unwrap();
    let present: Vec<char> = entries.iter().map(|e| e.codepoint).collect();
    let mut probes: Vec<u32> = (0..0x11_0000u32).step_by(997).collect();
    for e in &entries {
        let cp = e.codepoint as u32;
        probes.push(cp.saturating_sub(1));
        probes.push(cp + 1);
    }
    probes.push(0x10_FFFF);
    for cp in probes {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        assert_eq!(pack.find(c).is_some(), present.contains(&c), "U+{cp:04X}");
    }
}

#[test]
fn blank_glyph_with_nonzero_height_and_zero_width_is_accepted_with_empty_bitmap() {
    // width 0, height 5: stride 0 -> 0 bytes. Legal.
    let bytes = golden_patched(|b| put_u16(b, rec(3, R_HEIGHT), 5));
    let pack = Pack::parse(&bytes).expect("zero width with height 5 is a valid blank glyph");
    let g = pack.find('\u{10FFFF}').unwrap();
    assert_eq!(g.metrics.height, 5);
    assert_eq!(g.metrics.width, 0);
    assert!(g.bitmap.is_empty());
    // and the mirror: width 8, height 0
    let bytes = golden_patched(|b| put_u16(b, rec(3, R_WIDTH), 8));
    let pack = Pack::parse(&bytes).expect("height 0 with width 8 is a valid blank glyph");
    assert!(pack.find('\u{10FFFF}').unwrap().bitmap.is_empty());
}

#[test]
fn glyph_ranges_may_overlap_and_appear_out_of_offset_order() {
    // record 1 (U+FFFF, len 1) re-pointed at offset 0 -> shares the first byte of glyph 'A'
    let bytes = golden_patched(|b| put_u32(b, rec(1, R_BITMAP_OFFSET), 0));
    let pack = Pack::parse(&bytes).expect("shared bitmap bytes are allowed");
    assert_eq!(pack.find('\u{FFFF}').unwrap().bitmap, &[0x3C]);
    // record 0 re-pointed at offset 3 -> bytes FF 80 (after record 2's start)
    let bytes = golden_patched(|b| put_u32(b, rec(0, R_BITMAP_OFFSET), 3));
    let pack = Pack::parse(&bytes).expect("out-of-order offsets are allowed");
    assert_eq!(pack.find('A').unwrap().bitmap, &[0xFF, 0x80]);
}

#[test]
fn bitmap_bytes_not_referenced_by_any_glyph_are_allowed() {
    // glyph_count 0 but the former index bytes declared as bitmap region:
    // index [44,44), bitmap [44,139).
    let bytes = golden_patched(|b| {
        put_u32(b, H_COUNT, 0);
        put_u32(b, H_INDEX_LEN, 0);
        put_u32(b, H_BITMAP_OFFSET, 44);
        put_u32(b, H_BITMAP_LEN, 95);
    });
    let pack = Pack::parse(&bytes).expect("zero glyphs with a non-empty bitmap region is valid");
    assert_eq!(pack.header().glyph_count, 0);
    assert_eq!(pack.header().bitmap_len, 95);
    assert!(pack.find('A').is_none());
}

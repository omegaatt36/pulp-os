//! Pack content for a Latin font that is always available (Bookerly Regular):
//! parse, header fields, lookup, and bit-exact glyph data against the
//! independent oracle at every default size.
mod common;

use common::*;
use pulp_fontpack::Pack;

#[test]
fn every_default_size_pack_parses_with_its_pixel_size_and_ceil_line_metrics() {
    // header pixel_size is the request; line_height/ascent are ceil of the font's line metrics at that size
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).expect("pack must parse");
        let h = pack.header();
        let (line_height, ascent) = oracle_line_metrics(&font, px);
        assert_eq!(h.info.pixel_size, px);
        assert_eq!(h.info.line_height, line_height, "line_height at {px}px");
        assert_eq!(h.info.ascent, ascent, "ascent at {px}px");
    }
}

#[test]
fn line_height_and_ascent_for_16px_match_hand_computed_ceilings() {
    // fontdue reports ascent 16.912 and new_line_size 21.088 at 16px for this font,
    // so ceil gives ascent 17 and line_height 22 (derived by hand from those two numbers)
    let bytes = bookerly_bytes();
    let out = convert_with(&bytes, &[16], None);
    let h = Pack::parse(pack_bytes(&out, 16)).unwrap().header();
    assert_eq!((h.info.line_height, h.info.ascent), (22, 17));
}

#[test]
fn glyph_count_is_cmap_minus_control_characters_at_every_size() {
    // included set = cmap without char::is_control, identical for every size
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let want = oracle_included(&font).len() as u32;
    assert!(
        want < font.chars().len() as u32,
        "fixture must contain controls"
    );
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        assert_eq!(pack.header().glyph_count, want, "glyph_count at {px}px");
    }
}

#[test]
fn every_included_char_is_found_and_excluded_or_unmapped_chars_are_not() {
    // included -> find hits; controls in the cmap and chars outside the cmap -> find misses
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    let included = oracle_included(&font);
    let excluded: Vec<char> = oracle_cmap(&font)
        .into_iter()
        .filter(|c| c.is_control())
        .collect();
    assert!(excluded.contains(&'\u{8}'), "fixture: Bookerly maps U+0008");
    let unmapped = ['臺', '\u{2A6A5}', '\u{20BB7}', '\u{10FFFF}', '\u{1}'];
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        for &c in &included {
            assert!(
                pack.find(c).is_some(),
                "U+{:04X} missing at {px}px",
                c as u32
            );
        }
        for &c in excluded.iter().chain(unmapped.iter()) {
            assert!(
                pack.find(c).is_none(),
                "U+{:04X} must be absent at {px}px",
                c as u32
            );
        }
    }
}

#[test]
fn glyph_index_order_is_ascending_codepoint_and_covers_exactly_the_included_set() {
    // glyph_at(i) walks the index: the chars come out ascending and equal the oracle list
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let out = convert_with(&bytes, &[23], None);
    let pack = Pack::parse(pack_bytes(&out, 23)).unwrap();
    let walked: Vec<char> = (0..pack.header().glyph_count)
        .map(|i| pack.glyph_at(i).unwrap().0)
        .collect();
    assert_eq!(walked, oracle_included(&font));
}

#[test]
fn space_is_included_as_a_blank_glyph_with_an_advance() {
    // whitespace glyphs are kept: width/height 0, empty bitmap, advance > 0 (a text run needs it)
    let bytes = bookerly_bytes();
    let out = convert_with(&bytes, &[23], None);
    let pack = Pack::parse(pack_bytes(&out, 23)).unwrap();
    let g = pack
        .find(' ')
        .expect("space is in the cmap and not a control");
    assert_eq!((g.metrics.width, g.metrics.height), (0, 0));
    assert!(g.bitmap.is_empty());
    assert!(g.metrics.advance > 0);
}

#[test]
fn glyph_data_matches_independent_rasterisation_for_whole_cmap_at_every_default_size() {
    // metrics + bitmap bytes per glyph equal the oracle's coverage>=100, MSB-first packing
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let included = oracle_included(&font);
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        assert_pack_matches_oracle(&pack, &font, px, &included);
    }
}

#[test]
fn row_padding_bits_beyond_the_glyph_width_are_zero() {
    // convention: bits past `width` in each row's last byte are 0 (keeps output reproducible)
    let bytes = bookerly_bytes();
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        for i in 0..pack.header().glyph_count {
            let (c, g) = pack.glyph_at(i).unwrap();
            let w = g.metrics.width as usize;
            if w % 8 == 0 || g.metrics.height == 0 {
                continue;
            }
            let stride = w.div_ceil(8);
            // low (8 - w%8) bits of the last byte of each row are padding
            let pad_mask = 0xFFu8 >> (w % 8);
            for row in g.bitmap.chunks(stride) {
                assert_eq!(
                    row[stride - 1] & pad_mask,
                    0,
                    "U+{:04X} at {px}px has ink in padding",
                    c as u32
                );
            }
        }
    }
}

#[test]
fn flat_based_capital_sits_on_the_baseline_and_a_descender_hangs_below_it() {
    // offset_y is baseline-to-top (y down): 'H' bottom at the baseline gives offset_y == -height;
    // 'p' bottom is below the baseline, so offset_y + height > 0; both have ink
    let bytes = bookerly_bytes();
    let out = convert_with(&bytes, &[35], None);
    let pack = Pack::parse(pack_bytes(&out, 35)).unwrap();
    let h = pack.find('H').unwrap();
    assert!(h.metrics.height > 0 && h.bitmap.iter().any(|&b| b != 0));
    assert_eq!(h.metrics.offset_y, -(h.metrics.height as i16));
    let p = pack.find('p').unwrap();
    assert!(p.metrics.offset_y + p.metrics.height as i16 > 0);
    assert!(
        p.metrics.offset_y < 0,
        "x-height body of 'p' is above the baseline"
    );
}

#[test]
fn size_255_keeps_values_beyond_the_8_bit_range_unclamped() {
    // at 255px a capital is ~170px tall above the baseline (offset_y < -128) and some glyph exceeds 255
    // px in advance or height; the old 8-bit generator would clamp all of these, the pack must not
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let out = convert_with(&bytes, &[255], None);
    let pack = Pack::parse(pack_bytes(&out, 255)).unwrap();
    assert_eq!(pack.header().info.pixel_size, 255);
    let h = pack.find('H').unwrap();
    assert!(h.metrics.offset_y < -128, "offset_y {}", h.metrics.offset_y);
    assert!(h.metrics.height > 128);
    let included = oracle_included(&font);
    let over_255: Vec<char> = included
        .iter()
        .copied()
        .filter(|&c| {
            let g = oracle_glyph(&font, c, 255);
            g.width > 255 || g.height > 255 || g.advance > 255
        })
        .collect();
    assert!(
        !over_255.is_empty(),
        "fixture: some glyph exceeds 255 at 255px"
    );
    assert_pack_matches_oracle(&pack, &font, 255, &over_255);
    assert_pack_matches_oracle(&pack, &font, 255, &['H', 'W', 'p', 'g', '|']);
}

#[test]
fn big_pack_has_a_bitmap_region_over_64k_and_its_tail_glyph_is_retrievable() {
    // at 46px the Latin cmap alone is > 65,535 bytes of bitmap (oracle sum); the last glyph starts
    // beyond byte 65,535 of the region and must still come back exactly
    let bytes = bookerly_bytes();
    let font = load_font(&bytes);
    let included = oracle_included(&font);
    let out = convert_with(&bytes, &[46], None);
    let pack = Pack::parse(pack_bytes(&out, 46)).unwrap();

    let lens: Vec<usize> = included
        .iter()
        .map(|&c| oracle_glyph(&font, c, 46).bitmap.len())
        .collect();
    let total: usize = lens.iter().sum();
    assert!(total > 65_535, "fixture must exceed 64 KiB, got {total}");
    assert_eq!(pack.header().bitmap_len as usize, total);

    let last = included.len() - 1;
    let tail_start: usize = lens[..last].iter().sum();
    assert!(tail_start > 65_535, "tail glyph must start past 64 KiB");
    let (c, g) = pack.glyph_at(last as u32).unwrap();
    assert_eq!(c, included[last]);
    assert_glyph_matches(c, 46, &g, &oracle_glyph(&font, c, 46));

    // a glyph that straddles the 65,536 boundary, if any, is intact too
    let mut start = 0usize;
    for (i, &len) in lens.iter().enumerate() {
        if len > 0 && start < 65_536 && start + len > 65_536 {
            let (c, g) = pack.glyph_at(i as u32).unwrap();
            assert_glyph_matches(c, 46, &g, &oracle_glyph(&font, c, 46));
        }
        start += len;
    }
}

#[test]
fn packs_for_different_sizes_hold_size_specific_metrics_and_bitmaps() {
    // each pack is self-consistent for its own size: line_height grows with size and 'H' gets taller
    let bytes = bookerly_bytes();
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    let mut prev_line = 0u16;
    let mut prev_h = 0u16;
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        let line = pack.header().info.line_height;
        let h = pack.find('H').unwrap().metrics.height;
        assert!(line > prev_line, "line_height must grow: {px}px");
        assert!(h > prev_h, "'H' must grow: {px}px");
        (prev_line, prev_h) = (line, h);
    }
}

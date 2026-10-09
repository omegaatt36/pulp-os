//! The missing-glyph fallback (`missing_glyph_metrics`, `render_missing_glyph`).
//! Its format rules define it completely, so every expectation below is
//! hand-derived from those rules:
//!
//!   s       = clamp(pixel_size * 3 / 4, 3, 255)        (u32 integer arithmetic)
//!   advance = max(pixel_size, s + 2)
//!   offset_x = (advance - s) / 2,   offset_y = -s,   width = height = s
//!   bitmap  = hollow square, 1 bpp, MSB first, stride ceil(s/8), padding bits 0
mod common;

use common::*;
use pulp_fontpack::{FontInfo, Metrics, bitmap_size, missing_glyph_metrics, render_missing_glyph};

fn info(pixel_size: u16) -> FontInfo {
    FontInfo {
        pixel_size,
        font_id: 0x1122_3344_5566_7788,
        line_height: 30,
        ascent: 24,
    }
}

fn mm(advance: u16, offset_x: i16, offset_y: i16, side: u16) -> Metrics {
    Metrics {
        advance,
        offset_x,
        offset_y,
        width: side,
        height: side,
    }
}

fn render(px: u16) -> (Metrics, Vec<u8>) {
    let m = missing_glyph_metrics(&info(px));
    let mut out = vec![0xFFu8; stride(m.width) * m.height as usize];
    let got = render_missing_glyph(&info(px), &mut out).expect("exact-size buffer suffices");
    assert_eq!(got, m, "render returns missing_glyph_metrics");
    (m, out)
}

/// Pixel (x, y) of a 1-bpp MSB-first bitmap with `stride` bytes per row.
fn bit(bm: &[u8], stride: usize, x: usize, y: usize) -> bool {
    bm[y * stride + x / 8] & (0x80 >> (x % 8)) != 0
}

// ------------------------------------------------------------------ metrics

#[test]
fn metrics_hand_derived_table() {
    // (pixel_size, side s, advance, offset_x); offset_y is always -s
    //  px    s    adv   ox       derivation
    let table: [(u16, u16, u16, i16); 31] = [
        (0, 3, 5, 1),               // 0*3/4=0 -> clamp 3; max(0,5)=5; (5-3)/2=1
        (1, 3, 5, 1),               // 3/4=0 -> 3
        (2, 3, 5, 1),               // 6/4=1 -> 3
        (3, 3, 5, 1),               // 9/4=2 -> 3
        (4, 3, 5, 1),               // 12/4=3 (clamp boundary, exact); max(4,5)=5
        (5, 3, 5, 1),               // 15/4=3; max(5,5)=5
        (6, 4, 6, 1),               // 18/4=4; max(6,6)=6; (6-4)/2=1
        (7, 5, 7, 1),               // 21/4=5; 7; 1
        (8, 6, 8, 1),               // 24/4=6; 8; 1
        (9, 6, 9, 1),               // 27/4=6; max(9,8)=9; (9-6)/2=1
        (10, 7, 10, 1),             // 30/4=7; 10; 3/2=1
        (11, 8, 11, 1),             // 33/4=8; 11; 3/2=1
        (12, 9, 12, 1),             // 36/4=9; 12; 3/2=1
        (13, 9, 13, 2),             // 39/4=9; 13; 4/2=2
        (16, 12, 16, 2),            // contract example: s=12, advance 16, offset_x 2
        (19, 14, 19, 2),            // 57/4=14; 19; 5/2=2
        (23, 17, 23, 3),            // contract example: s=17, advance 23, offset_x 3
        (27, 20, 27, 3),            // 81/4=20; 7/2=3
        (28, 21, 28, 3),            // 84/4=21; 7/2=3
        (32, 24, 32, 4),            // 96/4=24; 8/2=4
        (35, 26, 35, 4),            // 105/4=26; 9/2=4
        (38, 28, 38, 5),            // 114/4=28; 10/2=5
        (46, 34, 46, 6),            // 138/4=34; 12/2=6
        (100, 75, 100, 12),         // 300/4=75; 25/2=12
        (339, 254, 339, 42),        // 1017/4=254 (below the upper clamp); 85/2=42
        (340, 255, 340, 42),        // 1020/4=255 exactly; 85/2=42
        (341, 255, 341, 43),        // 1023/4=255 (floor of 255.75); 86/2=43
        (342, 255, 342, 43),        // 1026/4=256 -> clamp 255; 87/2=43
        (400, 255, 400, 72),        // contract example: 1200/4=300 -> 255; (400-255)/2=72
        (1000, 255, 1000, 372),     // 745/2=372
        (65535, 255, 65535, 32640), // 65280/2=32640 fits i16
    ];
    for (px, s, adv, ox) in table {
        assert_eq!(
            missing_glyph_metrics(&info(px)),
            mm(adv, ox, -(s as i16), s),
            "pixel_size {px}"
        );
    }
}

#[test]
fn metrics_follow_the_contract_formula_for_every_pixel_size() {
    for px in 0..=u16::MAX {
        let s = (px as u32 * 3 / 4).clamp(3, 255);
        let adv = (px as u32).max(s + 2);
        let ox = (adv - s) / 2;
        let m = missing_glyph_metrics(&info(px));
        assert_eq!(m.width as u32, s, "width at {px}");
        assert_eq!(m.height as u32, s, "height at {px}");
        assert_eq!(m.advance as u32, adv, "advance at {px}");
        assert_eq!(m.offset_x as i32, ox as i32, "offset_x at {px}");
        assert_eq!(m.offset_y as i32, -(s as i32), "offset_y at {px}");
        // derived: the box lies inside its advance and sits above the baseline
        assert!(
            m.offset_x >= 0 && m.offset_x as u32 + s <= adv,
            "box inside advance at {px}"
        );
        assert!(m.offset_y < 0);
    }
}

#[test]
fn metrics_depend_only_on_pixel_size() {
    for px in [0u16, 1, 5, 16, 23, 46, 340, 400, u16::MAX] {
        let want = missing_glyph_metrics(&info(px));
        for (font_id, line_height, ascent) in [
            (0u64, 0u16, 0u16),
            (u64::MAX, u16::MAX, u16::MAX),
            (1, 1, 1),
            (0xDEAD_BEEF, 100, 3),
        ] {
            let other = FontInfo {
                pixel_size: px,
                font_id,
                line_height,
                ascent,
            };
            assert_eq!(
                missing_glyph_metrics(&other),
                want,
                "px {px} id {font_id:x}"
            );
        }
    }
}

#[test]
fn the_fallback_differs_between_the_supported_sizes() {
    // Each pixel size has its own fallback box.
    let sizes = [16u16, 19, 23, 27, 28, 32, 35, 38, 46];
    for (i, &a) in sizes.iter().enumerate() {
        for &b in &sizes[i + 1..] {
            assert_ne!(
                missing_glyph_metrics(&info(a)),
                missing_glyph_metrics(&info(b)),
                "{a}px vs {b}px"
            );
        }
    }
}

// ------------------------------------------------------------------- bitmaps

#[test]
fn bitmap_16px_is_the_hand_drawn_12x12_hollow_square() {
    // s = 12, stride 2. Row 0 / row 11: x 0..=11 all ink -> FF, then x 8..=11 = high nibble F0.
    // Rows 1..=10: x=0 -> 0x80 in byte 0; x=11 -> bit (7-3) of byte 1 = 0x10.
    let mut want: Vec<u8> = Vec::new();
    want.extend([0xFF, 0xF0]);
    for _ in 0..10 {
        want.extend([0x80, 0x10]);
    }
    want.extend([0xFF, 0xF0]);
    assert_eq!(want.len(), 24);
    let literal: [u8; 24] = [
        0xFF, 0xF0, //
        0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, //
        0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, //
        0xFF, 0xF0,
    ];
    assert_eq!(want, literal);
    let (m, got) = render(16);
    assert_eq!(m, mm(16, 2, -12, 12));
    assert_eq!(got, literal);
}

#[test]
fn bitmap_23px_is_the_17x17_hollow_square_with_zero_padding_bits() {
    // s = 17, stride 3 (24 bits per row, 7 padding bits).
    // Row 0 / row 16: x 0..=16 -> FF FF and x=16 = bit 7 of byte 2 -> 0x80.
    // Rows 1..=15: x=0 -> 0x80; x=16 -> 0x80 in byte 2; byte 1 empty.
    let mut want: Vec<u8> = Vec::new();
    want.extend([0xFF, 0xFF, 0x80]);
    for _ in 0..15 {
        want.extend([0x80, 0x00, 0x80]);
    }
    want.extend([0xFF, 0xFF, 0x80]);
    assert_eq!(want.len(), 51);
    let (m, got) = render(23);
    assert_eq!(m, mm(23, 3, -17, 17));
    assert_eq!(got, want);
    // the padding bits (x = 17..=23 -> low 7 bits of every third byte) are zero
    for row in got.chunks(3) {
        assert_eq!(row[2] & 0x7F, 0, "padding bits");
    }
}

#[test]
fn bitmap_of_tiny_boxes_is_exact() {
    // s=3 (px 0..=5): rows 111 / 101 / 111 -> E0 A0 E0
    for px in [0u16, 1, 2, 3, 4, 5] {
        let (m, got) = render(px);
        assert_eq!((m.width, m.height), (3, 3), "px {px}");
        assert_eq!(got, [0xE0, 0xA0, 0xE0], "px {px}");
    }
    // s=4 (px 6): 1111 / 1001 / 1001 / 1111
    assert_eq!(render(6).1, [0xF0, 0x90, 0x90, 0xF0]);
    // s=5 (px 7): 11111 / 10001 x3 / 11111
    assert_eq!(render(7).1, [0xF8, 0x88, 0x88, 0x88, 0xF8]);
    // s=7 (px 10): 1111111 / 1000001 x5 / 1111111  (the 8th bit is padding)
    assert_eq!(render(10).1, [0xFE, 0x82, 0x82, 0x82, 0x82, 0x82, 0xFE]);
}

#[test]
fn bitmap_at_the_byte_boundaries_8_9_16_17() {
    // s=8 (px 11): stride 1 exactly, no padding
    assert_eq!(
        render(11).1,
        [0xFF, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0xFF]
    );
    // s=9 (px 12): stride 2; x=8 is bit 7 of byte 1
    let mut want = vec![0xFF, 0x80];
    for _ in 0..7 {
        want.extend([0x80, 0x80]);
    }
    want.extend([0xFF, 0x80]);
    assert_eq!(render(12).1, want);
    // s=16 (px 22: 66/4=16): stride 2; x=15 is bit 0 of byte 1
    let mut want = vec![0xFF, 0xFF];
    for _ in 0..14 {
        want.extend([0x80, 0x01]);
    }
    want.extend([0xFF, 0xFF]);
    let (m, got) = render(22);
    assert_eq!((m.width, m.height), (16, 16));
    assert_eq!(got, want);
}

#[test]
fn bitmap_at_the_clamp_maximum_255() {
    // px 340 and px 400 and px 65535 all have s = 255, stride 32:
    // row 0 / 254: 31 bytes of FF, then x 248..=254 (7 bits) -> 0xFE;
    // other rows: x=0 -> byte 0 = 0x80; x=254 -> bit (7-6) of byte 31 = 0x02.
    let mut want: Vec<u8> = Vec::new();
    let mut border = vec![0xFFu8; 31];
    border.push(0xFE);
    let mut inner = vec![0u8; 32];
    inner[0] = 0x80;
    inner[31] = 0x02;
    want.extend(&border);
    for _ in 0..253 {
        want.extend(&inner);
    }
    want.extend(&border);
    assert_eq!(want.len(), 255 * 32);
    for px in [340u16, 400, u16::MAX] {
        let (m, got) = render(px);
        assert_eq!((m.width, m.height), (255, 255));
        assert_eq!(got, want, "px {px}");
    }
}

#[test]
fn bitmap_matches_the_per_pixel_definition_for_every_size_up_to_420_px() {
    // pixel (x, y) is ink iff x == 0 || y == 0 || x == s-1 || y == s-1; all else (incl. padding) is 0
    for px in 0..=420u16 {
        let (m, got) = render(px);
        let s = m.width as usize;
        let st = s.div_ceil(8);
        assert_eq!(got.len(), st * s, "px {px}");
        for y in 0..s {
            for x in 0..s {
                let ink = x == 0 || y == 0 || x == s - 1 || y == s - 1;
                assert_eq!(bit(&got, st, x, y), ink, "px {px} ({x},{y})");
            }
            for x in s..st * 8 {
                assert!(!bit(&got, st, x, y), "px {px}: padding bit ({x},{y}) set");
            }
        }
    }
}

#[test]
fn bitmap_is_visible_hollow_and_has_exactly_4s_minus_4_ink_pixels() {
    for s in 3..=255usize {
        // smallest pixel size with that side: s = px*3/4  <=>  px = ceil(4s/3); px 5 -> 3 for s=3
        let px = (4 * s).div_ceil(3).max(1) as u16;
        let (m, got) = render(px);
        assert_eq!(m.width as usize, s, "px {px} gives s={s}");
        let ink: u32 = got.iter().map(|b| b.count_ones()).sum();
        assert_eq!(
            ink as usize,
            4 * s - 4,
            "s={s}: ink pixels of a hollow square"
        );
        assert!(got.iter().any(|&b| b != 0), "never blank");
        let st = s.div_ceil(8);
        for y in 1..s - 1 {
            for x in 1..s - 1 {
                assert!(!bit(&got, st, x, y), "s={s}: interior ({x},{y}) is ink");
            }
        }
    }
}

#[test]
fn bitmap_length_equals_bitmap_size_of_the_metrics() {
    for px in [0u16, 7, 16, 23, 46, 340, 400] {
        let m = missing_glyph_metrics(&info(px));
        let need = bitmap_size(m.width, m.height) as usize;
        assert_eq!(need, stride(m.width) * m.height as usize);
        let mut out = vec![0u8; need];
        assert!(
            render_missing_glyph(&info(px), &mut out).is_some(),
            "px {px}"
        );
    }
}

// ----------------------------------------------------------- buffer handling

#[test]
fn render_is_deterministic_and_ignores_the_previous_buffer_content() {
    for px in [0u16, 5, 16, 23, 27, 46, 400] {
        let n = {
            let m = missing_glyph_metrics(&info(px));
            stride(m.width) * m.height as usize
        };
        let want = render(px).1;
        for fill in [0x00u8, 0xFF, 0xA5, 0x01] {
            let mut out = vec![fill; n];
            let m = render_missing_glyph(&info(px), &mut out).unwrap();
            assert_eq!(out, want, "px {px} prefilled with {fill:#x}");
            assert_eq!(m, missing_glyph_metrics(&info(px)));
        }
        let mut again = vec![0x3Cu8; n];
        render_missing_glyph(&info(px), &mut again).unwrap();
        assert_eq!(again, want);
    }
}

#[test]
fn render_depends_only_on_pixel_size() {
    for px in [0u16, 16, 23, 400] {
        let want = render(px);
        let n = want.1.len();
        for (font_id, line_height, ascent) in [(0u64, 0u16, 0u16), (u64::MAX, u16::MAX, u16::MAX)] {
            let other = FontInfo {
                pixel_size: px,
                font_id,
                line_height,
                ascent,
            };
            let mut out = vec![0u8; n];
            let m = render_missing_glyph(&other, &mut out).unwrap();
            assert_eq!((m, out), want, "px {px}");
        }
    }
}

#[test]
fn render_into_a_too_small_buffer_is_none_and_writes_nothing() {
    for px in [0u16, 5, 16, 23, 46, 400] {
        let need = {
            let m = missing_glyph_metrics(&info(px));
            stride(m.width) * m.height as usize
        };
        for have in [0usize, 1, need / 2, need - 1] {
            let mut out = vec![0xC3u8; have];
            assert_eq!(
                render_missing_glyph(&info(px), &mut out),
                None,
                "px {px} have {have}"
            );
            assert!(
                out.iter().all(|&b| b == 0xC3),
                "px {px}: nothing written on None"
            );
        }
    }
    let mut empty: [u8; 0] = [];
    assert_eq!(render_missing_glyph(&info(16), &mut empty), None);
}

#[test]
fn render_into_a_larger_buffer_touches_only_the_bitmap_bytes() {
    for px in [0u16, 16, 23, 46, 340] {
        let want = render(px).1;
        let n = want.len();
        let mut out = vec![0x5Au8; n + 17];
        let m = render_missing_glyph(&info(px), &mut out).unwrap();
        assert_eq!(m, missing_glyph_metrics(&info(px)));
        assert_eq!(&out[..n], &want[..], "px {px}");
        assert!(
            out[n..].iter().all(|&b| b == 0x5A),
            "px {px}: bytes beyond the bitmap changed"
        );
    }
}

#[test]
fn render_succeeds_with_exactly_the_needed_bytes_and_fails_with_one_less() {
    // 16px needs 24, 23px needs 51, 3px box needs 3, 340px needs 8160
    for (px, need) in [
        (16u16, 24usize),
        (23, 51),
        (0, 3),
        (340, 8160),
        (11, 8),
        (12, 18),
    ] {
        let mut ok = vec![0u8; need];
        assert!(
            render_missing_glyph(&info(px), &mut ok).is_some(),
            "px {px} need {need}"
        );
        let mut short = vec![0u8; need - 1];
        assert!(
            render_missing_glyph(&info(px), &mut short).is_none(),
            "px {px} need {need}"
        );
    }
}

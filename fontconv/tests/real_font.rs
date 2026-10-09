//! Iansui-Regular.ttf end to end through the library API (skipped, loudly, when
//! the font file is not on this machine: see `common::iansui_path`).
#[macro_use]
mod common;

use std::path::Path;
use std::sync::OnceLock;

use common::*;
use pulp_fontconv::Output;
use pulp_fontpack::Pack;

struct Real {
    bytes: Vec<u8>,
    font: fontdue::Font,
    out: Output,
}

/// One default-size conversion shared by every test in this binary.
fn real(path: &Path) -> &'static Real {
    static R: OnceLock<Real> = OnceLock::new();
    R.get_or_init(|| {
        let bytes = std::fs::read(path).unwrap();
        let font = load_font(&bytes);
        let out = convert_with(&bytes, &DEFAULT_SIZES, None);
        Real { bytes, font, out }
    })
}

// representative text: Traditional Chinese, full-width punctuation, supplementary-plane
// character, digits, and stroke-dense characters
const SAMPLE: &str = "臺灣「繁體中文」，。、？！（）【】『』𠮷0123456789０１２３龜鬱矗AaZz　";

fn sample_chars() -> Vec<char> {
    let mut v: Vec<char> = SAMPLE.chars().collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn the_font_file_is_the_pinned_release_the_numbers_below_depend_on() {
    // size and sha-256 from `shasum`; cmap size from the font itself via fontdue; 𠮷 present, 𪚥 absent
    let path = require_iansui!();
    let r = real(&path);
    assert_eq!(r.bytes.len() as u64, IANSUI_LEN);
    assert_eq!(sha256_hex(&r.bytes), IANSUI_SHA256);
    assert_eq!(r.font.chars().len(), 12_666);
    assert!(r.font.chars().contains_key(&'\u{20BB7}'));
    assert!(!r.font.chars().contains_key(&'\u{2A6A5}'));
}

#[test]
fn skip_helper_finds_the_font_when_present_and_counts_skips_when_not() {
    // present -> Some path and zero skips; absent -> the helper itself prints SKIPPED and counts it
    let before = skipped_count();
    let found = iansui_path("skip_helper_probe");
    if found.is_some() {
        assert_eq!(
            skipped_count(),
            before,
            "a present font must not count as a skip"
        );
    } else {
        // other tests may skip concurrently, so only a lower bound
        assert!(skipped_count() > before, "an absent font must be counted");
    }
}

#[test]
fn every_default_size_pack_parses_with_pixel_size_and_ceil_line_metrics() {
    // pixel_size = request; line_height/ascent = ceil of fontdue horizontal line metrics at that size
    let path = require_iansui!();
    let r = real(&path);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&r.out, px)).expect("pack must parse");
        let h = pack.header();
        let (line_height, ascent) = oracle_line_metrics(&r.font, px);
        assert_eq!(h.info.pixel_size, px);
        assert_eq!(
            (h.info.line_height, h.info.ascent),
            (line_height, ascent),
            "{px}px"
        );
    }
}

#[test]
fn glyph_count_is_the_cmap_without_control_characters_at_every_size() {
    // 12,666 mapped scalars; U+000D is a control character and is excluded
    let path = require_iansui!();
    let r = real(&path);
    let want = oracle_included(&r.font).len() as u32;
    assert_eq!(want, 12_666 - 1);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&r.out, px)).unwrap();
        assert_eq!(pack.header().glyph_count, want, "{px}px");
    }
}

#[test]
fn rare_supplementary_char_is_found_and_the_unmapped_one_is_not() {
    // 𠮷 (U+20BB7) is in the font; 𪚥 (U+2A6A5) is not; the control U+000D is excluded
    let path = require_iansui!();
    let r = real(&path);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&r.out, px)).unwrap();
        assert!(pack.find('\u{20BB7}').is_some(), "𠮷 at {px}px");
        assert!(pack.find('\u{2A6A5}').is_none(), "𪚥 at {px}px");
        assert!(pack.find('\r').is_none(), "U+000D at {px}px");
    }
}

#[test]
fn every_included_char_is_found_and_non_cmap_chars_are_not() {
    // lookup hit for all 12,665 included chars; misses for chars outside the cmap
    let path = require_iansui!();
    let r = real(&path);
    let pack = Pack::parse(pack_bytes(&r.out, 23)).unwrap();
    for c in oracle_included(&r.font) {
        assert!(pack.find(c).is_some(), "U+{:04X}", c as u32);
    }
    for c in ['\u{2A6A5}', '\u{10FFFF}', '\u{E000}', '\u{1F600}'] {
        if !r.font.chars().contains_key(&c) {
            assert!(pack.find(c).is_none(), "U+{:04X}", c as u32);
        }
    }
}

#[test]
fn representative_chinese_text_matches_independent_rasterisation_at_every_size() {
    // metrics and bitmap bytes of the sample chars equal the oracle, per default size
    let path = require_iansui!();
    let r = real(&path);
    let chars = sample_chars();
    for c in &chars {
        assert!(
            r.font.chars().contains_key(c),
            "sample U+{:04X} must be in the font",
            *c as u32
        );
    }
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&r.out, px)).unwrap();
        assert_pack_matches_oracle(&pack, &r.font, px, &chars);
    }
}

#[test]
fn ideographic_space_is_included_as_a_blank_glyph_with_advance() {
    // U+3000 is in the cmap and is not a control: blank bitmap, advance kept (about one em wide)
    let path = require_iansui!();
    let r = real(&path);
    let pack = Pack::parse(pack_bytes(&r.out, 28)).unwrap();
    let g = pack.find('\u{3000}').expect("U+3000 included");
    assert_eq!((g.metrics.width, g.metrics.height), (0, 0));
    assert!(g.bitmap.is_empty());
    assert!(g.metrics.advance >= 20, "advance {}", g.metrics.advance);
}

#[test]
fn whole_cmap_at_23px_matches_independent_rasterisation_and_the_region_is_over_64k() {
    // every glyph equals the oracle; header bitmap_len = sum of oracle lengths, far above 65,535;
    // the tail glyph starts beyond byte 65,535 and comes back exactly
    let path = require_iansui!();
    let r = real(&path);
    let included = oracle_included(&r.font);
    let pack = Pack::parse(pack_bytes(&r.out, 23)).unwrap();

    let mut total = 0usize;
    let mut last_start = 0usize;
    for &c in &included {
        let o = oracle_glyph(&r.font, c, 23);
        let g = pack.find(c).unwrap_or_else(|| panic!("U+{:04X}", c as u32));
        assert_glyph_matches(c, 23, &g, &o);
        last_start = total;
        total += o.bitmap.len();
    }
    assert!(total > 65_535, "got {total}");
    assert_eq!(pack.header().bitmap_len as usize, total);
    assert!(last_start > 65_535);

    let tail = *included.last().unwrap();
    let (c, g) = pack.glyph_at(included.len() as u32 - 1).unwrap();
    assert_eq!(c, tail);
    assert_glyph_matches(c, 23, &g, &oracle_glyph(&r.font, tail, 23));
}

#[test]
fn largest_size_pack_also_retrieves_its_tail_glyph() {
    // 46px region is several MiB: the last glyph must still be exact (offsets are 32-bit)
    let path = require_iansui!();
    let r = real(&path);
    let included = oracle_included(&r.font);
    let pack = Pack::parse(pack_bytes(&r.out, 46)).unwrap();
    assert!(pack.header().bitmap_len > 1_000_000);
    let tail = *included.last().unwrap();
    assert_pack_matches_oracle(&pack, &r.font, 46, &[tail, *included.first().unwrap()]);
}

#[test]
fn missing_report_lists_only_the_unmapped_requirement() {
    // required 臺灣𠮷 are in the font, 𪚥 is not: missing == [𪚥]
    let path = require_iansui!();
    let r = real(&path);
    let license = license_bytes();
    let out = pulp_fontconv::convert(&pulp_fontconv::Input {
        font: &r.bytes,
        sizes: &[16],
        license: &license,
        upstream_url: IANSUI_UPSTREAM,
        require_chars: Some("臺灣 𠮷\n𪚥"),
    })
    .unwrap();
    assert_eq!(out.missing, vec!['\u{2A6A5}']);
}

#[test]
fn missing_report_is_empty_when_every_required_char_is_in_the_font() {
    // all of the sample text is covered
    let path = require_iansui!();
    let r = real(&path);
    let license = license_bytes();
    let out = pulp_fontconv::convert(&pulp_fontconv::Input {
        font: &r.bytes,
        sizes: &[16],
        license: &license,
        upstream_url: IANSUI_UPSTREAM,
        require_chars: Some(SAMPLE),
    })
    .unwrap();
    assert!(out.missing.is_empty(), "{:?}", out.missing);
}

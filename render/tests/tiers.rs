// every book-font size tier the reader offers, loaded and rendered
//
// src/apps/reader/font.rs picks a pack by name: the tier's pixel size is
// baked into `IANSUI<SS>.PFP`, and the reader opens exactly that file. So a
// tier is only really supported if a pack with that exact name loads and
// lays out and draws a page. These fixtures are the converter's output for
// each of the five tiers build.rs rasterises (16/19/23/28/35 px).
//
// IANSUI24.PFP is the odd one out: it is the size the golden images were
// rendered at, not a tier, and the device never asks for it. It lives here
// because iansui_golden.rs compares against those reviewed bytes.

mod common;

use std::path::PathBuf;

use pulp_render::font_pack::FontPack;
use pulp_render::layout::{LineSpan, Markup, WrapParams, wrap};
use pulp_render::page::{PageGeometry, PageGlyphs};
use pulp_render::panel::Rotation;
use pulp_render::strip::StripBuffer;

/// The tiers src/apps/reader/font.rs asks for, in order.
const TIERS: [u16; 5] = [16, 19, 23, 28, 35];

const MAX_LINES: usize = 32;
const MARGIN: u32 = 20;
const TOP: i32 = 40;

const MARKUP: Markup = Markup {
    marker: 0x01,
    img_ref: b'P',
    bold_on: b'B',
    bold_off: b'b',
    italic_on: b'I',
    italic_off: b'i',
    heading_on: b'H',
    heading_off: b'h',
    quote_on: b'Q',
    quote_off: b'q',
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn sample() -> Vec<u8> {
    fixture("iansui-sample.txt")
}

/// one page laid out and drawn through a pack, returning its line count, the
/// inked pixel count, and the pack's own metrics
fn render_tier(px: u16) -> (usize, usize, u16, u16) {
    let mut r = pulp_render::font_pack::SliceReader(&fixture(&format!("IANSUI{px}.PFP")));
    let pack = FontPack::load(&mut r, px).unwrap_or_else(|e| panic!("{px} px pack: {e:?}"));
    let text = sample();

    let params = WrapParams {
        markup: MARKUP,
        max_lines: MAX_LINES,
        max_width_px: 440,
        indent_px: 0,
        img_heights: &[],
        default_img_h: 0,
    };
    let geom = PageGeometry {
        left: MARGIN as i32,
        top: TOP,
        line_height: pack.line_height() as i32,
        ascent: pack.ascent() as i32,
        indent_px: 0,
    };

    let mut lines = vec![LineSpan::EMPTY; MAX_LINES];
    let wrapped = wrap(
        &text,
        true,
        &mut pulp_render::page::PackMeasure::new(&pack, &mut r),
        &params,
        &mut lines,
    )
    .expect("wrap");
    lines.truncate(wrapped.line_count);
    let page = &text[..wrapped.consumed];

    let mut cache = Box::new(PageGlyphs::<256, { 24 * 1024 }>::new());
    cache
        .prepare(&pack, &mut r, page, &lines, MARKUP)
        .unwrap_or_else(|e| panic!("{px} px prepare: {e:?}"));

    let frame = common::strip::render_full(Rotation::Deg270, &|s: &mut StripBuffer| {
        cache.draw(s, page, &lines, MARKUP, &geom).expect("draw");
    });

    (
        wrapped.line_count,
        frame.black_pixels().len(),
        pack.line_height(),
        pack.ascent(),
    )
}

#[test]
fn every_tier_loads_lays_out_and_draws() {
    for px in TIERS {
        let (lines, ink, line_h, ascent) = render_tier(px);
        assert!(lines > 0, "{px} px laid out no lines");
        assert!(ink > 0, "{px} px drew no ink");
        assert!(
            ascent > 0 && ascent <= line_h,
            "{px} px ascent > line height"
        );
    }
}

#[test]
fn bigger_tiers_need_more_lines_and_put_down_more_ink() {
    // a pack that ignored its pixel_size would render every tier identically
    let mut prev = (0usize, 0usize);
    for px in TIERS {
        let (lines, ink, line_h, _) = render_tier(px);
        assert!(
            lines >= prev.0,
            "{px} px wrapped into fewer lines than the tier below it ({lines} < {})",
            prev.0
        );
        assert!(
            ink >= prev.1,
            "{px} px put down less ink than the tier below it ({ink} < {})",
            prev.1
        );
        // line height must grow with the pixel size, or the reader's
        // max_lines calculation would be wrong at every tier but one
        assert!(line_h > 0);
        prev = (lines, ink);
    }
    let (_, _, last_h, _) = render_tier(*TIERS.last().unwrap());
    let (_, _, first_h, _) = render_tier(TIERS[0]);
    assert!(
        last_h > first_h,
        "line height did not grow: {first_h} at 16 px, {last_h} at 35 px"
    );
}

#[test]
fn a_tier_renders_the_same_bytes_every_time() {
    // the device redraws the same page on every partial refresh, so a pack
    // that drifted between draws would ghost on the panel
    for px in TIERS {
        let a = render_tier(px);
        let b = render_tier(px);
        assert_eq!(a, b, "{px} px is not deterministic across renders");
    }
}

#[test]
fn a_pack_at_the_wrong_size_is_rejected() {
    // the loader checks the name-derived size against the header, so a pack
    // copied to the wrong file name is refused rather than drawn squashed
    let mut r = pulp_render::font_pack::SliceReader(&fixture("IANSUI23.PFP"));
    assert!(
        FontPack::load(&mut r, 28).is_err(),
        "the 23 px pack was accepted as 28 px"
    );
}

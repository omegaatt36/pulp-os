// firmware strip renderer on the host: full-frame vs partial-window
//
// oracle: every expected physical coordinate below is derived by hand
// from the panel rotation contract, not from running the renderer.
// panel is 800x480 physical, strips are 40 physical rows. Deg270 (the
// firmware UI) is portrait, logical 480 wide x 800 tall:
//   logical (lx, ly) -> physical (px, py) = (ly, 479 - lx)
// and a logical region (x, y, w, h) covers physical
//   (y, 480 - x - w, h, w)
// so physical strip boundary rows 39|40 sit at logical x 440|439

mod common;

use common::strip::{Frame, assert_area_eq, render_full, render_partial};
use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::BinaryColor,
    primitives::Rectangle,
};
use pulp_render::panel::{HEIGHT, Rotation, WIDTH, align_partial_region, transform_region};
use pulp_render::strip::StripBuffer;

// planted 5x3 glyph, stride 1, MSB = leftmost column
//   row 0: x=0        #....
//   row 1: x=1,2      .##..
//   row 2: x=4        ....#
// glyph pixel (x, y) lands at logical (gx + x, gy + y)
const GLYPH: [u8; 3] = [0b1000_0000, 0b0110_0000, 0b0000_1000];

fn blit_glyph(s: &mut StripBuffer, gx: i32, gy: i32) {
    s.blit_1bpp(&GLYPH, 0, 5, 3, 1, gx, gy, true);
}

fn fill(s: &mut StripBuffer, x: i32, y: i32, w: u32, h: u32) {
    s.fill_solid(
        &Rectangle::new(Point::new(x, y), Size::new(w, h)),
        BinaryColor::On,
    )
    .unwrap();
}

fn dot(s: &mut StripBuffer, x: i32, y: i32) {
    s.draw_iter([Pixel(Point::new(x, y), BinaryColor::On)])
        .unwrap();
}

// the shared portrait scene, each item annotated with its physical image
fn scene(s: &mut StripBuffer) {
    // R1: logical x 436..=443, y 100..=109 -> px 100..=109, py 36..=43
    //     (crosses strip rows 39|40)
    fill(s, 436, 100, 8, 10);
    // R2: logical x 405..=414, y 300..=303 -> px 300..=303, py 65..=74
    //     (crosses partial chunk boundary 69|70 of window W2)
    fill(s, 405, 300, 10, 4);
    // G1 at (437, 200): lx 437,438,439,441 -> py 42,41,40,38
    //     -> (200,42) (201,41) (201,40) (202,38)
    blit_glyph(s, 437, 200);
    // G2 at (407, 500): lx 407,408,409,411 -> py 72,71,70,68
    //     -> (500,72) (501,71) (501,70) (502,68)
    blit_glyph(s, 407, 500);
    // single pixels: panel corners and both sides of strip rows 39|40
    dot(s, 0, 0); // -> (0, 479)
    dot(s, 479, 799); // -> (799, 0)
    dot(s, 440, 5); // -> (5, 39)
    dot(s, 439, 5); // -> (5, 40)
    // P5: logical (440, 195) -> (195, 39), in W3's left mask padding
    dot(s, 440, 195);
}

fn rect(x0: u16, x1: u16, y0: u16, y1: u16) -> Vec<(u16, u16)> {
    let mut v = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            v.push((x, y));
        }
    }
    v
}

fn sorted(mut v: Vec<(u16, u16)>) -> Vec<(u16, u16)> {
    // row-major, matching Frame::black_pixels
    v.sort_by_key(|&(x, y)| (y, x));
    v.dedup();
    v
}

fn scene_expected() -> Vec<(u16, u16)> {
    let mut v = rect(100, 109, 36, 43);
    v.extend(rect(300, 303, 65, 74));
    v.extend([(200, 42), (201, 41), (201, 40), (202, 38)]);
    v.extend([(500, 72), (501, 71), (501, 70), (502, 68)]);
    v.extend([(0, 479), (799, 0), (5, 39), (5, 40), (195, 39)]);
    sorted(v)
}

#[test]
fn full_frame_matches_hand_derived_portrait_pixels() {
    let full = render_full(Rotation::Deg270, &scene);
    let expected = scene_expected();
    assert_eq!(expected.len(), 80 + 40 + 4 + 4 + 5);
    if full.black_pixels() != expected {
        let path = full.write_pbm("full_frame_portrait");
        panic!(
            "full-frame pixels differ from hand-derived set; frame at {}\n got: {:?}",
            path.display(),
            full.black_pixels()
        );
    }
}

#[test]
fn landscape_generic_blit_crosses_strip_boundary() {
    // Deg0 is the identity: logical (lx, ly) -> physical (lx, ly);
    // exercises the non-270 blit path across strip rows 39|40
    let full = render_full(Rotation::Deg0, &|s: &mut StripBuffer| blit_glyph(s, 10, 38));
    assert_eq!(
        full.black_pixels(),
        vec![(10, 38), (11, 39), (12, 39), (14, 40)]
    );
}

// outside the physical window the partial pass must leave the base frame alone
fn assert_untouched_outside(name: &str, partial: &Frame, base: &Frame, win: (u16, u16, u16, u16)) {
    let (wx, wy, ww, wh) = win;
    let above = (0, 0, WIDTH, wy);
    let below = (0, wy + wh, WIDTH, HEIGHT - wy - wh);
    let left = (0, wy, wx, wh);
    let right = (wx + ww, wy, WIDTH - wx - ww, wh);
    for (x, y, w, h) in [above, below, left, right] {
        assert_area_eq(name, partial, base, x, y, w, h);
    }
}

#[test]
fn aligned_window_cutting_a_rect_matches_full_frame() {
    // W1: logical (436+2, 104, 4, 16) -> physical (104, 480-438-4, 16, 4)
    //     = window x 104..120, y 38..42; byte aligned, no edge masks
    let full = render_full(Rotation::Deg270, &scene);
    let base = Frame::blank();
    let (partial, rs) = render_partial(&base, Rotation::Deg270, 438, 104, 4, 16, &scene).unwrap();

    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (104, 38, 16, 4));
    assert_eq!((rs.left_mask, rs.right_mask), (0, 0));
    assert_area_eq("w1", &partial, &full, 104, 38, 16, 4);
    // R1 clipped to the window: px 104..=109, py 38..=41
    assert_eq!(partial.black_pixels(), rect(104, 109, 38, 41));
    assert_untouched_outside("w1-outside", &partial, &base, (104, 38, 16, 4));
}

#[test]
fn window_clips_glyph_on_both_row_edges() {
    // W4: logical (438, 200, 2, 8) -> physical (200, 480-438-2, 8, 2)
    //     = window x 200..208, y 40..42; G1 keeps (201,40) (201,41),
    //     loses (202,38) above and (200,42) below
    let full = render_full(Rotation::Deg270, &scene);
    let base = Frame::blank();
    let (partial, rs) = render_partial(&base, Rotation::Deg270, 438, 200, 2, 8, &scene).unwrap();

    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (200, 40, 8, 2));
    assert_area_eq("w4", &partial, &full, 200, 40, 8, 2);
    assert_eq!(partial.black_pixels(), vec![(201, 40), (201, 41)]);
    assert_untouched_outside("w4-outside", &partial, &base, (200, 40, 8, 2));
}

#[test]
fn full_width_window_split_into_row_chunks_matches_full_frame() {
    // W2: logical (400, 0, 50, 800) -> physical (0, 480-400-50, 800, 50)
    //     = window x 0..800, y 30..80. 800 px = 100 row bytes, so the
    //     4000-byte strip holds 40 rows: chunks y 30..70 and 70..80,
    //     with R2 and G2 straddling the 69|70 chunk seam and R1, G1,
    //     (5,39)/(5,40) straddling strip rows 39|40
    let full = render_full(Rotation::Deg270, &scene);
    let base = Frame::blank();
    let (partial, rs) = render_partial(&base, Rotation::Deg270, 400, 0, 50, 800, &scene).unwrap();

    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (0, 30, 800, 50));
    assert_eq!(StripBuffer::max_rows_for_width(800), 40);
    assert_area_eq("w2", &partial, &full, 0, 30, 800, 50);
    // everything in the scene except the two corner dots at py 479 and 0
    let expected: Vec<_> = scene_expected()
        .into_iter()
        .filter(|&(_, y)| (30..80).contains(&y))
        .collect();
    assert_eq!(expected.len(), 80 + 40 + 4 + 4 + 3);
    assert_eq!(partial.black_pixels(), expected);
    assert_untouched_outside("w2-outside", &partial, &base, (0, 30, 800, 50));
}

#[test]
fn unaligned_window_matches_full_frame_in_requested_region() {
    // W3: logical (436, 198, 6, 20) -> physical (198, 480-436-6, 20, 6)
    //     = requested x 198..218, y 38..44. RAM writes are byte wide, so
    //     the driver widens to x 192..224 (pw 32) with padding of
    //     6 px left (mask 0b1111_1100) and 6 px right (0b0011_1111)
    let full = render_full(Rotation::Deg270, &scene);
    let base = Frame::blank();
    let (partial, rs) = render_partial(&base, Rotation::Deg270, 436, 198, 6, 20, &scene).unwrap();

    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (192, 38, 32, 6));
    assert_eq!((rs.left_mask, rs.right_mask), (0b1111_1100, 0b0011_1111));
    assert_area_eq("w3", &partial, &full, 198, 38, 20, 6);
    assert_eq!(
        partial.black_pixels_in(198, 38, 20, 6),
        vec![(202, 38), (201, 40), (201, 41), (200, 42)]
    );
    assert_untouched_outside("w3-outside", &partial, &base, (192, 38, 32, 6));
}

#[test]
fn unaligned_window_edge_masks_force_padding_white() {
    // characterizes the driver's edge masks: padding pixels inside the
    // byte-aligned RAM window but outside the requested region are
    // written white even where the scene draws black. P5 at physical
    // (195, 39) is black in the full frame and sits in W3's left padding
    let full = render_full(Rotation::Deg270, &scene);
    let (partial, _) =
        render_partial(&Frame::blank(), Rotation::Deg270, 436, 198, 6, 20, &scene).unwrap();

    assert!(full.is_black(195, 39));
    assert!(!partial.is_black(195, 39));
    assert!(partial.black_pixels_in(192, 38, 6, 6).is_empty());
    assert!(partial.black_pixels_in(218, 38, 6, 6).is_empty());
}

#[test]
fn partial_pass_overwrites_stale_base_only_inside_window() {
    // panel RAM holds a previous full frame (all black); a partial pass of
    // W1 must replace exactly the window with the scene and keep the rest
    let base = render_full(Rotation::Deg270, &|s: &mut StripBuffer| {
        fill(s, 0, 0, 480, 800)
    });
    assert_eq!(base.black_pixels().len(), WIDTH as usize * HEIGHT as usize);

    let full = render_full(Rotation::Deg270, &scene);
    let (partial, _) = render_partial(&base, Rotation::Deg270, 438, 104, 4, 16, &scene).unwrap();

    assert_area_eq("stale-w1", &partial, &full, 104, 38, 16, 4);
    assert_untouched_outside("stale-w1-outside", &partial, &base, (104, 38, 16, 4));
}

#[test]
fn rotated_regions_clip_to_the_panel_before_transforming() {
    assert_eq!(
        transform_region(Rotation::Deg90, 470, 799, 20, 20),
        (0, 470, 1, 10)
    );
    assert_eq!(
        transform_region(Rotation::Deg180, 799, 479, 2, 2),
        (0, 0, 1, 1)
    );
    assert_eq!(
        transform_region(Rotation::Deg270, 479, 799, 2, 2),
        (799, 0, 1, 1)
    );

    let rs = align_partial_region(Rotation::Deg90, 470, 799, 20, 20).unwrap();
    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (0, 470, 8, 10));
    assert_eq!((rs.left_mask, rs.right_mask), (0, 0x7F));
    assert!(align_partial_region(Rotation::Deg90, 480, 0, 1, 1).is_none());
}

#[test]
fn oversized_region_clamps_without_u16_overflow() {
    let rs = align_partial_region(Rotation::Deg0, 0, 0, u16::MAX, u16::MAX).unwrap();
    assert_eq!((rs.px, rs.py, rs.pw, rs.ph), (0, 0, WIDTH, HEIGHT));
    assert_eq!((rs.left_mask, rs.right_mask), (0, 0));
}

#[test]
fn non_byte_aligned_window_uses_a_full_byte_for_its_last_pixel() {
    // A 201-pixel row occupies 26 bytes. Floor division reports 25,
    // so row 160's final pixel used to address byte 4000 in a 4000-byte buffer.
    assert_eq!(StripBuffer::max_rows_for_width(201), 153);
    let mut strip = StripBuffer::new();
    strip.begin_window(Rotation::Deg0, 0, 0, 201, 160);
    assert_eq!(strip.window(), (0, 0, 201, 153));
    dot(&mut strip, 200, 152);
    assert_eq!(strip.data().len(), 26 * 153);
    assert_eq!(strip.data()[26 * 152 + 25], 0x7F);
}

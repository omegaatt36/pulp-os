// StripCore::blit_1bpp_cw90: vertical writing turns Latin and punctuation a
// quarter turn clockwise. Pixels are checked through the same logical ->
// physical map the display uses.
use pulp_board_logic::ssd1677::Rotation;
use pulp_board_logic::strip::StripCore;

fn is_black(core: &StripCore, lx: u16, ly: u16) -> bool {
    let (wx, wy, ww, wh) = core.window();
    let (px, py) = core.to_physical(lx, ly);
    assert!(px >= wx && px < wx + ww && py >= wy && py < wy + wh);
    let rb = core.data().len() / wh as usize;
    let (col, row) = ((px - wx) as usize, (py - wy) as usize);
    core.data()[row * rb + col / 8] & (0x80 >> (col % 8)) == 0
}

// the whole logical page as one window
fn page() -> StripCore<'static> {
    let mut core = StripCore::new();
    // 64 logical rows keep the window inside the strip buffer
    let (px, py, pw, ph) =
        pulp_board_logic::ssd1677::transform_region(Rotation::Deg270, 0, 0, 480, 64);
    core.begin_window(Rotation::Deg270, px, py, pw, ph);
    core.fill_logical_rect(0, 0, 480, 64, false);
    core
}

// 3 wide, 2 tall:   #.#
//                   .##
const BITMAP: [u8; 2] = [0b1010_0000, 0b0110_0000];

#[test]
fn a_quarter_turn_clockwise_moves_pixel_x_y_to_gx_plus_h_minus_1_minus_y_and_gy_plus_x() {
    let mut core = page();
    core.blit_1bpp_cw90(&BITMAP, 0, 3, 2, 1, 10, 20, true);
    // turned bitmap is 2 wide, 3 tall:  row0: .#   (source column 0 read bottom up)
    //                                   row1: ##
    //                                   row2: #.   -> columns are the source rows reversed
    let expect = [
        // (dx, dy): source (x, y) -> (1 - y, x)
        (1, 0), // (0,0)
        (1, 2), // (2,0)
        (0, 1), // (1,1)
        (0, 2), // (2,1)
    ];
    for dy in 0..3u16 {
        for dx in 0..2u16 {
            assert_eq!(
                is_black(&core, 10 + dx, 20 + dy),
                expect.contains(&(dx, dy)),
                "pixel ({dx}, {dy})"
            );
        }
    }
}

#[test]
fn white_blit_clears_what_black_set() {
    let mut core = page();
    core.blit_1bpp_cw90(&BITMAP, 0, 3, 2, 1, 4, 4, true);
    core.blit_1bpp_cw90(&BITMAP, 0, 3, 2, 1, 4, 4, false);
    for y in 4..7 {
        for x in 4..6 {
            assert!(!is_black(&core, x, y));
        }
    }
}

#[test]
fn pixels_outside_the_window_or_the_screen_are_clipped_not_wrapped() {
    let mut core = page();
    // hangs over the left edge and past the bottom of the 64-row window
    core.blit_1bpp_cw90(&BITMAP, 0, 3, 2, 1, -1, 62, true);
    // source (0,0) -> (-1 + 1 - 0, 62) = (0, 62) is the only one on screen and in the window
    // source (2,0) -> (0, 64) is below the window; (1,1) -> (-1, 63) and (2,1) -> (-1, 64) are off screen
    assert!(is_black(&core, 0, 62));
    assert!(!is_black(&core, 1, 62));
    assert!(!is_black(&core, 0, 63));
    assert!(!is_black(&core, 1, 63));
}

#[test]
fn a_bitmap_wholly_outside_the_window_changes_nothing() {
    let mut core = page();
    let before = core.data().to_vec();
    core.blit_1bpp_cw90(&BITMAP, 0, 3, 2, 1, 100, 200, true);
    assert_eq!(core.data(), &before[..]);
}

#[test]
fn a_short_bitmap_slice_is_refused_instead_of_read_out_of_range() {
    let mut core = page();
    let before = core.data().to_vec();
    core.blit_1bpp_cw90(&BITMAP[..1], 0, 3, 2, 1, 10, 20, true);
    assert_eq!(core.data(), &before[..]);
}

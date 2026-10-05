// Portrait page renderer: the firmware's own StripBuffer / glyph blit /
// ReaderApp::draw produce every pixel; this file only schedules windows,
// copies finished strips into a 480x800 page and encodes the PBM artifact.
// No rasterising, glyph blitting or rotation math lives here: a strip's
// pixels are read back through the logical -> physical map the firmware
// itself uses (`StripCore::to_physical`), and a tile is placed with
// board-logic's `transform_region` (the same map the partial-refresh driver
// uses).
use std::io;
use std::path::Path;

use pulp_board_logic::ssd1677::{Rotation, transform_region};

use crate::drivers::strip::StripBuffer;

pub const WIDTH: u16 = 480;
pub const HEIGHT: u16 = 800;

const ROW_BYTES: usize = WIDTH as usize / 8;
const PBM_HEADER: &[u8] = b"P4\n480 800\n";

// firmware page rotation (portrait)
const ROTATION: Rotation = Rotation::Deg270;

// "full-frame" tile height: a full-logical-width band that fits the strip
// buffer (480 px = 480 physical rows of 64 / 8 bytes), and is a different
// shape than a firmware strip (40 physical rows x 800)
const FULL_BAND_ROWS: u16 = 64;

// One draw pass == one window of the production StripBuffer, in logical
// (portrait) coordinates as `StripBuffer::logical_window` reports it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pass {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

// The page. Bits are stored in the firmware strip sense (1 = white, MSB =
// leftmost); `to_pbm` inverts into the PBM convention.
pub struct Framebuffer {
    bits: Vec<u8>,
}

impl Framebuffer {
    fn new() -> Self {
        Self {
            bits: vec![0xFF; ROW_BYTES * HEIGHT as usize],
        }
    }

    fn locate(x: u16, y: u16) -> (usize, u8) {
        assert!(x < WIDTH && y < HEIGHT, "pixel ({x}, {y}) is off the page");
        (y as usize * ROW_BYTES + x as usize / 8, 0x80 >> (x % 8))
    }

    fn set_white(&mut self, x: u16, y: u16, white: bool) {
        let (i, m) = Self::locate(x, y);
        if white {
            self.bits[i] |= m;
        } else {
            self.bits[i] &= !m;
        }
    }

    pub fn is_black(&self, x: u16, y: u16) -> bool {
        let (i, m) = Self::locate(x, y);
        self.bits[i] & m == 0
    }

    pub fn black_count(&self) -> usize {
        self.bits.iter().map(|b| b.count_zeros() as usize).sum()
    }

    // binary PBM (P4): 1 = black, MSB = leftmost, no metadata
    pub fn to_pbm(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PBM_HEADER.len() + self.bits.len());
        out.extend_from_slice(PBM_HEADER);
        out.extend(self.bits.iter().map(|b| !b));
        out
    }

    pub fn write_pbm(&self, path: &Path) -> io::Result<()> {
        std::fs::write(path, self.to_pbm())
    }

    // copy the strip's current window into the page
    fn take(&mut self, strip: &StripBuffer) -> Pass {
        let (wx, wy, _, wh) = strip.window();
        let rb = strip.data().len() / wh as usize;
        let lw = strip.logical_window();
        for ly in lw.y..lw.y + lw.h {
            for lx in lw.x..lw.x + lw.w {
                let (px, py) = strip.to_physical(lx, ly);
                let (col, row) = ((px - wx) as usize, (py - wy) as usize);
                let white = strip.data()[row * rb + col / 8] & (0x80 >> (col % 8)) != 0;
                self.set_white(lx, ly, white);
            }
        }
        Pass {
            x: lw.x,
            y: lw.y,
            w: lw.w,
            h: lw.h,
        }
    }
}

pub struct Render {
    pub frame: Framebuffer,
    pub passes: Vec<Pass>,
}

// Firmware full-refresh path (`write_full_frame` -> `render_strip`): every
// 40-row physical strip begins white, is drawn once, and is streamed out.
pub fn render_stitched(draw: &dyn Fn(&mut StripBuffer)) -> Render {
    let mut strip = StripBuffer::new();
    let mut frame = Framebuffer::new();
    let mut passes = Vec::new();
    for idx in 0..StripBuffer::strip_count() {
        strip.begin_strip(ROTATION, idx);
        draw(&mut strip);
        passes.push(frame.take(&strip));
    }
    Render { frame, passes }
}

// Host "full-frame" reference: the same production StripBuffer, tiled with
// full-width bands instead of the firmware's strips.
pub fn render_full(draw: &dyn Fn(&mut StripBuffer)) -> Render {
    let mut strip = StripBuffer::new();
    let mut frame = Framebuffer::new();
    let mut passes = Vec::new();
    for y in (0..HEIGHT).step_by(FULL_BAND_ROWS as usize) {
        let h = FULL_BAND_ROWS.min(HEIGHT - y);
        let (px, py, pw, ph) = transform_region(ROTATION, 0, y, WIDTH, h);
        strip.begin_window(ROTATION, px, py, pw, ph);
        draw(&mut strip);
        passes.push(frame.take(&strip));
    }
    Render { frame, passes }
}

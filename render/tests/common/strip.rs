// host harness for the firmware strip renderer
//
// drives StripBuffer the way kernel/src/drivers/ssd1677.rs does and
// collects what the driver would stream to panel RAM into a whole-frame
// 1-bit image, so full-frame and partial-window passes can be compared
// without hardware. the loops below mirror write_full_frame and
// write_region_strips; window alignment and edge masks come from the
// shared pulp_render::panel code the driver itself calls

use std::fmt::Write as _;
use std::path::PathBuf;

use pulp_render::panel::{HEIGHT, RenderState, Rotation, WIDTH, align_partial_region};
use pulp_render::strip::{STRIP_COUNT, STRIP_ROWS, StripBuffer};

const ROW_BYTES: usize = WIDTH as usize / 8;

// physical panel RAM image: row-major, MSB-first, 1 = white (panel polarity)
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    bits: Vec<u8>,
}

impl Frame {
    pub fn blank() -> Self {
        Self {
            bits: vec![0xFF; ROW_BYTES * HEIGHT as usize],
        }
    }

    pub fn is_black(&self, px: u16, py: u16) -> bool {
        let byte = self.bits[py as usize * ROW_BYTES + px as usize / 8];
        byte & (0x80 >> (px % 8)) == 0
    }

    // black pixels as physical (x, y), sorted row-major
    pub fn black_pixels(&self) -> Vec<(u16, u16)> {
        self.black_pixels_in(0, 0, WIDTH, HEIGHT)
    }

    pub fn black_pixels_in(&self, x: u16, y: u16, w: u16, h: u16) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        for py in y..y + h {
            for px in x..x + w {
                if self.is_black(px, py) {
                    out.push((px, py));
                }
            }
        }
        out
    }

    // byte-aligned row copy, as a RAM write of `data` into the area
    // (x, y, w, h) with x and w multiples of 8
    fn write_area(&mut self, x: u16, y: u16, w: u16, h: u16, data: &[u8]) {
        let rb = w as usize / 8;
        assert_eq!(x % 8, 0, "RAM area x not byte aligned");
        assert_eq!(data.len(), rb * h as usize, "strip data length mismatch");
        for (row, src) in data.chunks(rb).enumerate() {
            let start = (y as usize + row) * ROW_BYTES + x as usize / 8;
            self.bits[start..start + rb].copy_from_slice(src);
        }
    }

    // binary PBM (P4, 1 = black) of the whole physical frame
    pub fn to_pbm(&self) -> Vec<u8> {
        let mut out = format!("P4\n{WIDTH} {HEIGHT}\n").into_bytes();
        out.extend(self.bits.iter().map(|b| !b));
        out
    }

    // inverse of to_pbm. Goldens use our canonical header and exact body
    // length; reject alternate PBM encodings to catch accidental rewrites
    pub fn from_pbm(pbm: &[u8]) -> Option<Self> {
        let header = format!("P4\n{WIDTH} {HEIGHT}\n").into_bytes();
        let body = pbm.strip_prefix(header.as_slice())?;
        (body.len() == ROW_BYTES * HEIGHT as usize).then(|| Self {
            bits: body.iter().map(|b| !b).collect(),
        })
    }

    // black exactly where the two frames differ
    pub fn diff(&self, other: &Frame) -> Frame {
        let bits = self.bits.iter().zip(&other.bits).map(|(a, b)| !(a ^ b));
        Self {
            bits: bits.collect(),
        }
    }

    // to_pbm under the cargo test scratch dir
    pub fn write_pbm(&self, name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("strip-harness");
        std::fs::create_dir_all(&dir).expect("create PBM dir");
        let path = dir.join(format!("{name}.pbm"));
        std::fs::write(&path, self.to_pbm()).expect("write PBM");
        path
    }
}

// full refresh: one begin_strip + draw per strip, data streamed
// sequentially into a RAM area covering the whole panel
pub fn render_full<F: Fn(&mut StripBuffer)>(rotation: Rotation, draw: &F) -> Frame {
    let mut frame = Frame::blank();
    let mut strip = StripBuffer::new();
    for i in 0..STRIP_COUNT {
        strip.begin_strip(rotation, i);
        draw(&mut strip);
        frame.write_area(0, i * STRIP_ROWS, WIDTH, STRIP_ROWS, strip.data());
    }
    frame
}

// partial refresh of logical region (x, y, w, h) on top of `base`
// (the panel's previous RAM content); None when the region is empty
pub fn render_partial<F: Fn(&mut StripBuffer)>(
    base: &Frame,
    rotation: Rotation,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    draw: &F,
) -> Option<(Frame, RenderState)> {
    let rs = align_partial_region(rotation, x, y, w, h)?;
    let mut frame = base.clone();
    let mut strip = StripBuffer::new();

    let max_rows = StripBuffer::max_rows_for_width(rs.pw);
    let row_bytes = (rs.pw / 8) as usize;
    let needs_mask = rs.left_mask != 0 || rs.right_mask != 0;

    let mut wy = rs.py;
    while wy < rs.py + rs.ph {
        let rows = max_rows.min(rs.py + rs.ph - wy);
        strip.begin_window(rotation, rs.px, wy, rs.pw, rows);
        draw(&mut strip);

        if needs_mask && row_bytes > 0 {
            for row in strip.data_mut().chunks_mut(row_bytes) {
                row[0] |= rs.left_mask;
                row[row.len() - 1] |= rs.right_mask;
            }
        }
        frame.write_area(rs.px, wy, rs.pw, rows, strip.data());
        wy += rows;
    }
    Some((frame, rs))
}

// pixel-exact comparison of a physical area; dumps both frames as PBM
// and lists the first differing pixels on failure
pub fn assert_area_eq(name: &str, a: &Frame, b: &Frame, x: u16, y: u16, w: u16, h: u16) {
    let mut diffs = Vec::new();
    for py in y..y + h {
        for px in x..x + w {
            if a.is_black(px, py) != b.is_black(px, py) {
                diffs.push((px, py, a.is_black(px, py)));
            }
        }
    }
    if diffs.is_empty() {
        return;
    }
    let pa = a.write_pbm(&format!("{name}-a"));
    let pb = b.write_pbm(&format!("{name}-b"));
    let mut msg = format!(
        "{name}: {} pixel(s) differ in area ({x},{y}) {w}x{h}\n  a: {}\n  b: {}\n",
        diffs.len(),
        pa.display(),
        pb.display()
    );
    for (px, py, a_black) in diffs.iter().take(16) {
        let _ = writeln!(msg, "  ({px},{py}) a={} b={}", a_black, !a_black);
    }
    panic!("{msg}");
}

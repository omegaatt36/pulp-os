// Sleep wallpaper: synthetic BMPs through `wallpaper::decode`, the refusals,
// and the strip blit against the logical image.
use pulp_board_logic::memory::{MemClass, MemError, MemoryBudget, PsramFault, PsramStatus};
use pulp_board_logic::ssd1677::Rotation;
use pulp_board_logic::strip::{PHYS_BYTES_PER_ROW, STRIP_COUNT, STRIP_ROWS, StripCore};
use pulp_board_logic::wallpaper::{
    BATCH_BYTES, BmpSource, Error, HEIGHT, MIN_BATCH_BYTES, OUT_BYTES, OUT_STRIDE, ReadError,
    WIDTH, WORK_BYTES, decode, draw_strip, parse_header,
};
use pulp_board_logic::wallpaper::{GRAY_OUT_BYTES, GRAY_WORK_BYTES, decode_gray};

// -- synthetic files ---------------------------------------------------

struct Spec {
    bpp: u16,
    top_down: bool,
    width: i32,
    height: i32,
    compression: u32,
    dib: usize,
    colors_used: u32,
    // BGRA entries
    palette: Vec<[u8; 4]>,
    // extra bytes between the palette and the pixels
    gap: usize,
}

impl Spec {
    fn new(bpp: u16, top_down: bool) -> Self {
        let palette = match bpp {
            1 => vec![[0, 0, 0, 0], [255, 255, 255, 0]],
            8 => (0..=255u8).map(|i| [i, i, i, 0]).collect(),
            _ => Vec::new(),
        };
        Spec {
            bpp,
            top_down,
            width: WIDTH as i32,
            height: if top_down {
                -(HEIGHT as i32)
            } else {
                HEIGHT as i32
            },
            compression: 0,
            dib: 40,
            colors_used: 0,
            palette,
            gap: 0,
        }
    }
}

fn stride_of(bpp: u16, width: usize) -> usize {
    (width * bpp as usize).div_ceil(32) * 4
}

// `px(x, y)` is the pixel at OUTPUT position (x, y): an RGB triple for 24 bit,
// a palette index for 8 bit, 0/1 for 1 bit.
fn build(s: &Spec, px: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let pixel_offset = 14 + s.dib + s.palette.len() * 4 + s.gap;
    let stride = stride_of(s.bpp, s.width.unsigned_abs() as usize);
    let h = s.height.unsigned_abs() as usize;
    let mut f = vec![0u8; pixel_offset + stride * h];
    f[0] = b'B';
    f[1] = b'M';
    let total = f.len() as u32;
    f[2..6].copy_from_slice(&total.to_le_bytes());
    f[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    f[14..18].copy_from_slice(&(s.dib as u32).to_le_bytes());
    f[18..22].copy_from_slice(&s.width.to_le_bytes());
    f[22..26].copy_from_slice(&s.height.to_le_bytes());
    f[26..28].copy_from_slice(&1u16.to_le_bytes());
    f[28..30].copy_from_slice(&s.bpp.to_le_bytes());
    f[30..34].copy_from_slice(&s.compression.to_le_bytes());
    f[46..50].copy_from_slice(&s.colors_used.to_le_bytes());
    let mut at = 14 + s.dib;
    for e in &s.palette {
        f[at..at + 4].copy_from_slice(e);
        at += 4;
    }
    for y in 0..h {
        let stored = if s.top_down { y } else { h - 1 - y };
        let row = &mut f[pixel_offset + stored * stride..][..stride];
        for x in 0..s.width.unsigned_abs() as usize {
            let p = px(x, y);
            match s.bpp {
                24 => row[x * 3..x * 3 + 3].copy_from_slice(&[p[2], p[1], p[0]]),
                8 => row[x] = p[0],
                _ => row[x / 8] |= (p[0] & 1) << (7 - (x & 7)),
            }
        }
    }
    f
}

struct VecSource {
    data: Vec<u8>,
    reads: usize,
    fail_at: Option<usize>,
}

impl VecSource {
    fn new(data: Vec<u8>) -> Self {
        VecSource {
            data,
            reads: 0,
            fail_at: None,
        }
    }
}

impl BmpSource for VecSource {
    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, ReadError> {
        self.reads += 1;
        if self.fail_at == Some(self.reads) {
            return Err(ReadError);
        }
        let off = (offset as usize).min(self.data.len());
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }
}

fn run(file: Vec<u8>, batch: usize) -> Result<Vec<u8>, Error> {
    let mut out = vec![0xA5u8; OUT_BYTES];
    let mut b = vec![0u8; batch];
    decode(&mut VecSource::new(file), &mut out, &mut b)?;
    Ok(out)
}

fn ink(img: &[u8], x: usize, y: usize) -> bool {
    img[y * OUT_STRIDE + x / 8] & (0x80 >> (x & 7)) != 0
}

fn ink_count(img: &[u8]) -> usize {
    img.iter().map(|b| b.count_ones() as usize).sum()
}

// black rectangle on white, odd offsets so byte boundaries are crossed
fn in_rect(x: usize, y: usize) -> bool {
    (13..77).contains(&x) && (20..31).contains(&y) || (400..480).contains(&x) && y >= 790
}

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

fn assert_rect(img: &[u8]) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(ink(img, x, y), in_rect(x, y), "pixel ({x}, {y})");
        }
    }
}

// -- all depths, both orientations --------------------------------------

#[test]
fn b24_black_and_white_both_orientations() {
    for top_down in [false, true] {
        let s = Spec::new(24, top_down);
        let f = build(&s, |x, y| if in_rect(x, y) { BLACK } else { WHITE });
        assert_rect(&run(f, BATCH_BYTES).unwrap());
    }
}

#[test]
fn b24_channel_order_is_bgr() {
    // luminance of pure red 76, green 149, blue 28: ink density 1 - lum / 255
    for (rgb, want) in [
        ([255, 0, 0], 0.70),
        ([0, 255, 0], 0.42),
        ([0, 0, 255], 0.89),
    ] {
        let f = build(&Spec::new(24, false), |_, _| rgb);
        let img = run(f, BATCH_BYTES).unwrap();
        let frac = ink_count(&img) as f64 / (OUT_BYTES * 8) as f64;
        assert!((frac - want).abs() < 0.02, "{rgb:?}: {frac}");
    }
}

#[test]
fn b8_palette_both_orientations() {
    // indices 0 and 2 are black, 1 and 3 white (the file's palette, not the
    // index value, decides)
    for top_down in [false, true] {
        let mut s = Spec::new(8, top_down);
        s.palette = vec![
            [0, 0, 0, 0],
            [255, 255, 255, 0],
            [0, 0, 0, 0],
            [255, 255, 255, 0],
        ];
        s.colors_used = 4;
        let f = build(&s, |x, y| {
            let i = match (in_rect(x, y), x % 2 == 0) {
                (true, true) => 0,
                (true, false) => 2,
                (false, true) => 1,
                (false, false) => 3,
            };
            [i, 0, 0]
        });
        assert_rect(&run(f, BATCH_BYTES).unwrap());
    }
}

#[test]
fn b8_gray_ramp_palette_is_dithered() {
    let f = build(&Spec::new(8, false), |_, _| [128, 0, 0]);
    let img = run(f, BATCH_BYTES).unwrap();
    let frac = ink_count(&img) as f64 / (OUT_BYTES * 8) as f64;
    assert!((0.45..0.55).contains(&frac), "{frac}");
}

#[test]
fn b1_both_orientations_and_palette_polarity() {
    for top_down in [false, true] {
        // palette 0 = black, 1 = white: a set bit is white
        let s = Spec::new(1, top_down);
        let f = build(&s, |x, y| [(!in_rect(x, y)) as u8, 0, 0]);
        assert_rect(&run(f, BATCH_BYTES).unwrap());

        // inverted palette: a set bit is black
        let mut s = Spec::new(1, top_down);
        s.palette = vec![[255, 255, 255, 0], [0, 0, 0, 0]];
        let f = build(&s, |x, y| [in_rect(x, y) as u8, 0, 0]);
        assert_rect(&run(f, BATCH_BYTES).unwrap());
    }
}

#[test]
fn b1_is_copied_not_dithered() {
    // palette entries of mid gray: 100 is black, 200 is white, whatever the
    // neighbours are
    let mut s = Spec::new(1, true);
    s.palette = vec![[100, 100, 100, 0], [200, 200, 200, 0]];
    let f = build(&s, |x, y| [((x + y) & 1) as u8, 0, 0]);
    let img = run(f, BATCH_BYTES).unwrap();
    for y in [0, 1, 399, 799] {
        for x in 0..WIDTH {
            assert_eq!(ink(&img, x, y), (x + y) & 1 == 0, "({x}, {y})");
        }
    }
}

#[test]
fn b1_degenerate_palettes() {
    let mut s = Spec::new(1, false);
    s.palette = vec![[0, 0, 0, 0], [10, 10, 10, 0]];
    let img = run(build(&s, |x, y| [((x ^ y) & 1) as u8, 0, 0]), BATCH_BYTES).unwrap();
    assert_eq!(ink_count(&img), OUT_BYTES * 8);
    s.palette = vec![[255, 255, 255, 0], [250, 250, 250, 0]];
    let img = run(build(&s, |x, y| [((x ^ y) & 1) as u8, 0, 0]), BATCH_BYTES).unwrap();
    assert_eq!(ink_count(&img), 0);
}

// -- header variants -----------------------------------------------------

#[test]
fn v4_header_and_gap_before_the_pixels() {
    for bpp in [1u16, 8, 24] {
        let mut s = Spec::new(bpp, false);
        s.dib = 108;
        s.gap = 38;
        let f = build(&s, |x, y| {
            let on = in_rect(x, y);
            match bpp {
                1 => [(!on) as u8, 0, 0],
                8 => [if on { 0 } else { 255 }, 0, 0],
                _ => {
                    if on {
                        BLACK
                    } else {
                        WHITE
                    }
                }
            }
        });
        assert_rect(&run(f, BATCH_BYTES).unwrap());
    }
}

#[test]
fn rows_are_not_padded_at_480_wide_but_the_stride_is_computed() {
    for (bpp, stride) in [(1u16, 60usize), (8, 480), (24, 1440)] {
        let f = build(&Spec::new(bpp, false), |_, _| BLACK);
        let h = parse_header(&f[..1200.min(f.len())]).unwrap();
        assert_eq!(h.row_stride, stride);
        assert_eq!(h.row_stride % 4, 0);
        assert_eq!(f.len(), h.pixel_offset as usize + stride * HEIGHT);
    }
}

#[test]
fn trailing_bytes_after_the_pixels_are_ignored() {
    let mut f = build(&Spec::new(24, false), |x, y| {
        if in_rect(x, y) { BLACK } else { WHITE }
    });
    f.extend_from_slice(&[0x55; 100]);
    assert_rect(&run(f, BATCH_BYTES).unwrap());
}

// -- batching and dithering ----------------------------------------------

// horizontal ramp, vertical ramp mix, and noise-free
fn ramp(x: usize, y: usize) -> [u8; 3] {
    let v = ((x * 255 / (WIDTH - 1) + y * 255 / (HEIGHT - 1)) / 2) as u8;
    [v, v, v]
}

#[test]
fn result_does_not_depend_on_the_batch_size() {
    for bpp in [1u16, 8, 24] {
        for top_down in [false, true] {
            let f = build(&Spec::new(bpp, top_down), |x, y| {
                let v = ramp(x, y);
                match bpp {
                    1 => [(v[0] > 100) as u8, 0, 0],
                    _ => v,
                }
            });
            let want = run(f.clone(), 64 * 1024).unwrap();
            for batch in [
                MIN_BATCH_BYTES,
                MIN_BATCH_BYTES + 1,
                3000,
                4096,
                BATCH_BYTES,
            ] {
                assert_eq!(
                    run(f.clone(), batch).unwrap(),
                    want,
                    "{bpp} {top_down} {batch}"
                );
            }
        }
    }
}

#[test]
fn flat_gray_levels_dither_to_their_density() {
    for (v, lo, hi) in [
        (0u8, 1.0, 1.0),
        (64, 0.72, 0.78),
        (192, 0.22, 0.28),
        (255, 0.0, 0.0),
    ] {
        let f = build(&Spec::new(24, false), |_, _| [v, v, v]);
        let img = run(f, BATCH_BYTES).unwrap();
        let frac = ink_count(&img) as f64 / (OUT_BYTES * 8) as f64;
        assert!((lo..=hi).contains(&frac), "{v}: {frac}");
    }
}

#[test]
fn ramp_density_follows_the_ramp() {
    // dark on the left: ink density per 60 px column block falls left to right
    let f = build(&Spec::new(24, false), |x, _| {
        let v = (x * 255 / (WIDTH - 1)) as u8;
        [v, v, v]
    });
    let img = run(f, BATCH_BYTES).unwrap();
    let mut last = usize::MAX;
    for block in 0..10 {
        let n = (0..HEIGHT)
            .map(|y| {
                img[y * OUT_STRIDE + block * 6..][..6]
                    .iter()
                    .map(|b| b.count_ones() as usize)
                    .sum::<usize>()
            })
            .sum::<usize>();
        assert!(n < last, "block {block}: {n} !< {last}");
        last = n;
    }
}

#[test]
fn dither_golden() {
    // regression fingerprint of the Floyd-Steinberg output on a diagonal ramp
    let f = build(&Spec::new(24, false), ramp);
    let img = run(f, BATCH_BYTES).unwrap();
    let mut h: u32 = 0x811C_9DC5;
    for &b in &img {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    assert_eq!(img[..6], [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF][..]);
    assert_eq!((h, ink_count(&img)), GOLDEN);
}
const GOLDEN: (u32, usize) = (3514906291, 193072);

#[test]
fn flat_128_first_row_golden() {
    // x0: 128 -> white, e = -127; x1: 128 - 55 = 73 -> black, e = 73; x2: 128 + 31
    // -> white, ...: the first row alternates white, black, white, ...
    let f = build(&Spec::new(24, false), |_, _| [128, 128, 128]);
    let img = run(f, BATCH_BYTES).unwrap();
    assert!(!ink(&img, 0, 0));
    assert!(ink(&img, 1, 0));
    assert_eq!(img[..4], FLAT_128_ROW0);
}
const FLAT_128_ROW0: [u8; 4] = [0x55; 4];

// -- refusals -------------------------------------------------------------

fn bad(mutate: impl Fn(&mut Spec)) -> Result<Vec<u8>, Error> {
    let mut s = Spec::new(24, false);
    mutate(&mut s);
    // geometry may be wrong on purpose: the pixel data is generated for it
    let f = build(&s, |_, _| WHITE);
    run(f, BATCH_BYTES)
}

#[test]
fn wrong_dimensions() {
    assert_eq!(bad(|s| s.width = 481), Err(Error::Dimensions));
    assert_eq!(bad(|s| s.width = 800), Err(Error::Dimensions));
    assert_eq!(bad(|s| s.height = 799), Err(Error::Dimensions));
    assert_eq!(bad(|s| s.height = -799), Err(Error::Dimensions));
    // landscape file of the same pixel count
    assert_eq!(
        bad(|s| {
            s.width = 800;
            s.height = 480;
        }),
        Err(Error::Dimensions)
    );
    assert_eq!(bad(|s| s.width = -480), Err(Error::Dimensions));
    let mut f = build(&Spec::new(24, false), |_, _| WHITE);
    f[22..26].copy_from_slice(&i32::MIN.to_le_bytes());
    assert_eq!(run(f, BATCH_BYTES), Err(Error::Dimensions));
}

#[test]
fn compressed_and_unsupported_depths() {
    assert_eq!(bad(|s| s.compression = 1), Err(Error::Compressed));
    assert_eq!(bad(|s| s.compression = 2), Err(Error::Compressed));
    assert_eq!(bad(|s| s.compression = 3), Err(Error::Compressed));
    for bpp in [2u16, 4, 16, 32] {
        let f = {
            let mut s = Spec::new(24, false);
            s.bpp = bpp;
            let mut f = build(&s, |_, _| WHITE);
            f[28..30].copy_from_slice(&bpp.to_le_bytes());
            f
        };
        assert_eq!(run(f, BATCH_BYTES), Err(Error::Depth), "{bpp}");
    }
}

#[test]
fn malformed_headers() {
    assert_eq!(run(Vec::new(), BATCH_BYTES), Err(Error::NotBmp));
    assert_eq!(run(vec![b'P', b'N', 1, 2], BATCH_BYTES), Err(Error::NotBmp));
    let mut f = build(&Spec::new(24, false), |_, _| WHITE);
    f[0] = b'X';
    assert_eq!(run(f, BATCH_BYTES), Err(Error::NotBmp));
    // OS/2 core header, absurd DIB size
    assert_eq!(bad(|s| s.dib = 12), Err(Error::Header));
    let mut f = build(&Spec::new(24, false), |_, _| WHITE);
    f[14..18].copy_from_slice(&5000u32.to_le_bytes());
    assert_eq!(run(f, BATCH_BYTES), Err(Error::Header));
    // pixel data claimed to start inside the header / palette
    let mut f = build(&Spec::new(8, false), |_, _| [0, 0, 0]);
    f[10..14].copy_from_slice(&60u32.to_le_bytes());
    assert_eq!(run(f, BATCH_BYTES), Err(Error::Header));
    // more colours than the depth has
    let mut s = Spec::new(1, false);
    s.colors_used = 3;
    assert_eq!(
        run(build(&s, |_, _| BLACK), BATCH_BYTES),
        Err(Error::Header)
    );
    // offset overflow
    let mut f = build(&Spec::new(24, false), |_, _| WHITE);
    f[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(run(f, BATCH_BYTES), Err(Error::Header));
}

#[test]
fn truncated_files() {
    for bpp in [1u16, 8, 24] {
        let f = build(&Spec::new(bpp, false), |_, _| BLACK);
        let pixels = f.len() - stride_of(bpp, WIDTH) * HEIGHT;
        // header cut, palette cut, no pixels, last row short by one byte
        for cut in [10usize, 20, 53, 60, pixels - 1, pixels, f.len() - 1] {
            assert_eq!(
                run(f[..cut].to_vec(), BATCH_BYTES),
                Err(Error::Truncated),
                "{bpp} cut {cut}"
            );
        }
        assert_eq!(run(f[..1].to_vec(), BATCH_BYTES), Err(Error::NotBmp));
        // top rows missing in a top-down file
        let f = build(&Spec::new(bpp, true), |_, _| BLACK);
        assert_eq!(
            run(f[..f.len() - 4].to_vec(), BATCH_BYTES),
            Err(Error::Truncated)
        );
    }
}

#[test]
fn read_errors_anywhere_are_reported() {
    let f = build(&Spec::new(24, false), |_, _| WHITE);
    for at in [1usize, 2, 30, 150] {
        let mut src = VecSource::new(f.clone());
        src.fail_at = Some(at);
        let mut out = vec![0u8; OUT_BYTES];
        let mut b = vec![0u8; BATCH_BYTES];
        assert_eq!(decode(&mut src, &mut out, &mut b), Err(Error::Read), "{at}");
    }
}

#[test]
fn buffers_that_are_too_small_are_refused() {
    let f = build(&Spec::new(24, false), |_, _| WHITE);
    let mut out = vec![0u8; OUT_BYTES - 1];
    let mut b = vec![0u8; BATCH_BYTES];
    assert_eq!(
        decode(&mut VecSource::new(f.clone()), &mut out, &mut b),
        Err(Error::Buffer)
    );
    let mut out = vec![0u8; OUT_BYTES];
    let mut b = vec![0u8; MIN_BATCH_BYTES - 1];
    assert_eq!(
        decode(&mut VecSource::new(f), &mut out, &mut b),
        Err(Error::Buffer)
    );
}

#[test]
fn reads_are_batched() {
    let f = build(&Spec::new(24, false), |_, _| WHITE);
    let mut src = VecSource::new(f);
    let mut out = vec![0u8; OUT_BYTES];
    let mut b = vec![0u8; BATCH_BYTES];
    decode(&mut src, &mut out, &mut b).unwrap();
    // header + 800 rows / (8192 / 1440 = 5 rows)
    assert_eq!(src.reads, 1 + HEIGHT.div_ceil(5));
}

// -- strips ---------------------------------------------------------------

fn blit_matches(rotation: Rotation) {
    // asymmetric content so a flipped or transposed blit cannot pass
    let f = build(&Spec::new(24, false), |x, y| {
        if in_rect(x, y) || (x * 7 + y * 3) % 11 == 0 {
            BLACK
        } else {
            WHITE
        }
    });
    let img = run(f, BATCH_BYTES).unwrap();
    let mut core = StripCore::new();
    let mut seen = 0;
    for idx in 0..STRIP_COUNT {
        core.begin_strip(rotation, idx);
        draw_strip(&mut core, &img);
        let (_, wy, _, wh) = core.window();
        assert_eq!(wh, STRIP_ROWS);
        let data = core.data().to_vec();
        for ly in 0..HEIGHT {
            for lx in 0..WIDTH {
                let (px, py) = core.to_physical(lx as u16, ly as u16);
                if py < wy || py >= wy + wh {
                    continue;
                }
                let byte = data[(py - wy) as usize * PHYS_BYTES_PER_ROW + px as usize / 8];
                let black = byte & (0x80 >> (px & 7)) == 0;
                assert_eq!(black, ink(&img, lx, ly), "{rotation:?} ({lx}, {ly})");
                seen += 1;
            }
        }
    }
    assert_eq!(seen, WIDTH * HEIGHT);
}

#[test]
fn strips_show_the_image_in_portrait() {
    blit_matches(Rotation::Deg270);
    blit_matches(Rotation::Deg90);
}

#[test]
fn white_image_leaves_the_strip_white() {
    let img = vec![0u8; OUT_BYTES];
    let mut core = StripCore::new();
    core.begin_strip(Rotation::Deg270, 3);
    draw_strip(&mut core, &img);
    assert!(core.data().iter().all(|&b| b == 0xFF));
}

// -- memory ----------------------------------------------------------------

#[test]
fn work_buffer_fits_the_display_frame_class_and_never_the_internal_heap() {
    assert_eq!(WORK_BYTES, OUT_BYTES + BATCH_BYTES);
    for bytes in [2 * 1024 * 1024, 8 * 1024 * 1024] {
        let mut m = MemoryBudget::new();
        m.set_status(PsramStatus::Ready { bytes }).unwrap();
        let r = m.reserve(MemClass::DisplayFrame, WORK_BYTES, 16).unwrap();
        assert_eq!(r.region, pulp_board_logic::memory::Region::Psram);
        // the partial-refresh snapshot (up to the whole frame) shares the class
        // but is never live while the device goes to sleep
        m.release(r).unwrap();
    }
    // PSRAM fault: the class has no internal budget, so the wallpaper is
    // refused instead of eating the heap
    let mut m = MemoryBudget::new();
    m.set_status(PsramStatus::Degraded(PsramFault::NotDetected))
        .unwrap();
    assert!(matches!(
        m.reserve(MemClass::DisplayFrame, WORK_BYTES, 16),
        Err(MemError::ClassLimit { limit: 0, .. })
    ));
}

// -- 4 gray levels ---------------------------------------------------------

fn run_gray(file: Vec<u8>, batch: usize) -> Result<Vec<u8>, Error> {
    let mut out = vec![0xA5u8; GRAY_OUT_BYTES];
    let mut b = vec![0u8; batch];
    decode_gray(&mut VecSource::new(file), &mut out, &mut b)?;
    Ok(out)
}

fn red(planes: &[u8]) -> &[u8] {
    &planes[..OUT_BYTES]
}

fn bw(planes: &[u8]) -> &[u8] {
    &planes[OUT_BYTES..]
}

// gray level of a pixel from the two ink planes (3 = white .. 0 = black)
fn level(planes: &[u8], x: usize, y: usize) -> u8 {
    match (ink(red(planes), x, y), ink(bw(planes), x, y)) {
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}

#[test]
fn gray_flat_levels_land_in_the_planes_the_waveform_index_says() {
    // level -> (red ink, bw ink); waveform index = 3 - level
    for (value, level_want) in [(0u8, 0u8), (85, 1), (170, 2), (255, 3)] {
        let s = Spec::new(8, false);
        let planes = run_gray(build(&s, |_, _| [value, 0, 0]), BATCH_BYTES).unwrap();
        for (x, y) in [(0, 0), (479, 799), (240, 400), (7, 8)] {
            assert_eq!(
                level(&planes, x, y),
                level_want,
                "value {value} at ({x}, {y})"
            );
        }
        // an exact level has no error to diffuse: the whole image is one level
        let (r, b) = (ink_count(red(&planes)), ink_count(bw(&planes)));
        let full = OUT_BYTES * 8;
        assert_eq!(r, if level_want >= 2 { full } else { 0 }, "value {value}");
        assert_eq!(
            b,
            if level_want == 3 || level_want == 1 {
                full
            } else {
                0
            },
            "value {value}"
        );
    }
}

#[test]
fn gray_between_two_levels_uses_both_and_keeps_the_mean() {
    let s = Spec::new(8, false);
    // 128 is the middle of dark gray (85) and light gray (170)
    let planes = run_gray(build(&s, |_, _| [128, 0, 0]), BATCH_BYTES).unwrap();
    let mut counts = [0usize; 4];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            counts[level(&planes, x, y) as usize] += 1;
        }
    }
    assert_eq!(counts[0] + counts[3], 0, "no black or white: {counts:?}");
    let mean = (counts[1] as f64 * 85.0 + counts[2] as f64 * 170.0) / (WIDTH * HEIGHT) as f64;
    assert!((mean - 128.0).abs() < 2.0, "mean {mean} {counts:?}");
}

#[test]
fn gray_1bit_files_use_their_palette_both_orientations() {
    for top_down in [false, true] {
        let s = Spec::new(1, top_down);
        let planes = run_gray(
            build(&s, |x, y| [if in_rect(x, y) { 0 } else { 1 }, 0, 0]),
            BATCH_BYTES,
        )
        .unwrap();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let want = if in_rect(x, y) { 0 } else { 3 };
                assert_eq!(level(&planes, x, y), want, "({x}, {y}) top_down {top_down}");
            }
        }
    }
}

#[test]
fn gray_orientation_and_batch_size_do_not_change_the_result() {
    let f = |top_down| build(&Spec::new(24, top_down), ramp);
    let a = run_gray(f(false), BATCH_BYTES).unwrap();
    assert_eq!(a, run_gray(f(true), BATCH_BYTES).unwrap());
    assert_eq!(a, run_gray(f(false), MIN_BATCH_BYTES).unwrap());
    assert_eq!(a, run_gray(f(false), 64 * 1024).unwrap());
}

#[test]
fn gray_ramp_is_darker_on_the_dark_side_and_uses_all_four_levels() {
    let planes = run_gray(
        build(&Spec::new(8, false), |x, _| {
            [(x * 255 / (WIDTH - 1)) as u8, 0, 0]
        }),
        BATCH_BYTES,
    )
    .unwrap();
    let mean_level = |x0: usize| -> f64 {
        let mut sum = 0.0;
        for y in 0..HEIGHT {
            for x in x0..x0 + 60 {
                sum += f64::from(level(&planes, x, y));
            }
        }
        sum / (60 * HEIGHT) as f64
    };
    let means: Vec<f64> = (0..8).map(|i| mean_level(i * 60)).collect();
    assert!(means.windows(2).all(|w| w[0] < w[1]), "{means:?}");
    let mut seen = [false; 4];
    for y in (0..HEIGHT).step_by(7) {
        for x in 0..WIDTH {
            seen[level(&planes, x, y) as usize] = true;
        }
    }
    assert_eq!(seen, [true; 4]);
}

#[test]
fn gray_refusals_match_the_one_bit_decoder() {
    assert_eq!(
        run_gray(
            build(&Spec::new(8, false), |_, _| [0, 0, 0]),
            MIN_BATCH_BYTES - 1
        ),
        Err(Error::Buffer)
    );
    let mut small = vec![0u8; GRAY_OUT_BYTES - 1];
    let mut b = vec![0u8; BATCH_BYTES];
    assert_eq!(
        decode_gray(
            &mut VecSource::new(build(&Spec::new(8, false), |_, _| [0, 0, 0])),
            &mut small,
            &mut b
        ),
        Err(Error::Buffer)
    );
    let mut wrong = Spec::new(8, false);
    wrong.width = 481;
    assert_eq!(
        run_gray(build(&wrong, |_, _| [0, 0, 0]), BATCH_BYTES),
        Err(Error::Dimensions)
    );
    let mut cut = build(&Spec::new(8, false), |_, _| [0, 0, 0]);
    cut.truncate(cut.len() - 1);
    assert_eq!(run_gray(cut, BATCH_BYTES), Err(Error::Truncated));
}

#[test]
fn gray_work_buffer_fits_the_display_frame_class_and_never_the_internal_heap() {
    assert_eq!(GRAY_WORK_BYTES, 2 * OUT_BYTES + BATCH_BYTES);
    for bytes in [2 * 1024 * 1024, 8 * 1024 * 1024] {
        let mut m = MemoryBudget::new();
        m.set_status(PsramStatus::Ready { bytes }).unwrap();
        let r = m
            .reserve(MemClass::DisplayFrame, GRAY_WORK_BYTES, 16)
            .unwrap();
        assert_eq!(r.region, pulp_board_logic::memory::Region::Psram);
        m.release(r).unwrap();
    }
    let mut m = MemoryBudget::new();
    m.set_status(PsramStatus::Degraded(PsramFault::NotDetected))
        .unwrap();
    assert!(matches!(
        m.reserve(MemClass::DisplayFrame, GRAY_WORK_BYTES, 16),
        Err(MemError::ClassLimit { limit: 0, .. })
    ));
}

// Sleep wallpaper: `SLEEP.BMP` (480x800, uncompressed BMP) -> 1 bit per pixel,
// or (`decode_gray`) -> 4 gray levels as the two RAM planes of the 4-gray
// waveform (`gray`), see the end of this comment.
//
// HAL-free and allocation-free: the caller owns the 48000 byte output and a
// small read batch, and talks to the card through the `BmpSource` trait, so the
// firmware (SD root file, PSRAM `DisplayFrame` backing) and the host tests (a
// byte vector) run the very same code.
//
// Accepted: BI_RGB, 1-bit (2 palette entries), 8-bit (palette) or 24-bit, exactly
// 480 wide and 800 high, bottom-up (positive height) or top-down (negative).
// Everything else is an `Error`; the firmware turns any `Error` into the text
// sleep screen, so a bad file can never block sleeping.
//
// Output: portrait 480x800 (the logical screen), row-major, 60 bytes per row,
// MSB = leftmost pixel, 1 = INK (black), 0 = white. That is the bitmap layout
// `StripCore::blit_1bpp` takes with `black = true`, see `draw_strip`.
//
// Conversion, in one paragraph. Rows are read from the card in batches of a few
// rows (`batch`), each BMP row is turned into 8-bit luminance (Rec.601 weights,
// 77/150/29 over 256; palette entries are converted once), and 8/24-bit rows are
// reduced to 1 bit with Floyd-Steinberg error diffusion, left to right, top to
// bottom in OUTPUT order (a bottom-up file is read from its last row to its
// first, a batch at a time, and processed in reverse). Only two i16 error rows
// are kept (`cur` incoming, `next` for the row below). A 1-bit file needs no
// diffusion: each palette entry is black (luminance < 128) or white, and the
// bytes are copied or inverted.
//
// 4 gray levels (`decode_gray`): the same reading, but every pixel goes through
// Floyd-Steinberg to the nearest of the four `GRAY_LEVELS` (0 = black .. 3 =
// white), 1-bit files included (palette luminance). The result is two logical
// 480x800 images, one per RAM plane, in the layout `draw_strip` takes: the
// waveform index of a level is `3 - level`, RED plane = high bit, BW plane = low
// bit, and a set bit in these images clears the plane bit (ink), so
//   level 3 (white):      red ink, bw ink      (index 0)
//   level 2 (light gray): red ink, no bw ink   (index 1)
//   level 1 (dark gray):  no red ink, bw ink   (index 2)
//   level 0 (black):      neither              (index 3)

use crate::strip::StripCore;

/// Logical screen (portrait).
pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 800;
/// Bytes per output row (1 bit per pixel).
pub const OUT_STRIDE: usize = WIDTH / 8;
/// Size of the output image.
pub const OUT_BYTES: usize = OUT_STRIDE * HEIGHT;

/// BMP rows are padded to 4 bytes; the widest one (24-bit) is 1440 bytes.
const MAX_ROW_STRIDE: usize = (WIDTH * 3).next_multiple_of(4);
/// File header, the largest DIB header (V5) and a full 256 entry palette: the
/// bytes `decode` reads first.
pub const HEADER_READ_BYTES: usize = FILE_HEADER + MAX_DIB + 256 * 4;
/// The smallest `batch` `decode` accepts: the header read, and one whole row.
pub const MIN_BATCH_BYTES: usize = if HEADER_READ_BYTES > MAX_ROW_STRIDE {
    HEADER_READ_BYTES
} else {
    MAX_ROW_STRIDE
};
/// Batch size the firmware uses: 5 rows of 24-bit, 17 of 8-bit.
pub const BATCH_BYTES: usize = 8 * 1024;
/// One allocation for the firmware: image, then batch.
pub const WORK_BYTES: usize = OUT_BYTES + BATCH_BYTES;
/// The 4 gray output: the RED plane image, then the BW plane image.
pub const GRAY_OUT_BYTES: usize = 2 * OUT_BYTES;
/// One allocation for the firmware in 4 gray: both planes, then batch.
pub const GRAY_WORK_BYTES: usize = GRAY_OUT_BYTES + BATCH_BYTES;
/// What each gray level is taken to look like on a 0..255 scale when the error
/// of a pixel is computed. Evenly spaced; on the panel the lightest level is
/// closer to the next one than to white, so tune these by eye, not by theory.
pub const GRAY_LEVELS: [i16; 4] = [0, 85, 170, 255];

const FILE_HEADER: usize = 14;
const MIN_DIB: usize = 40;
const MAX_DIB: usize = 124;

/// Why a wallpaper was refused. All of them mean "show the text screen".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed to read.
    Read,
    /// The file ended before the header, palette or last pixel row.
    Truncated,
    /// No `BM` signature.
    NotBmp,
    /// Not 480x800.
    Dimensions,
    /// Compression other than BI_RGB.
    Compressed,
    /// Bit depth other than 1, 8 or 24.
    Depth,
    /// Header fields that cannot be right (DIB size, palette, pixel offset).
    Header,
    /// The output or batch buffer is too small.
    Buffer,
}

impl Error {
    pub const fn as_str(self) -> &'static str {
        match self {
            Error::Read => "read error",
            Error::Truncated => "truncated file",
            Error::NotBmp => "not a BMP",
            Error::Dimensions => "not 480x800",
            Error::Compressed => "compressed BMP",
            Error::Depth => "unsupported bit depth",
            Error::Header => "bad header",
            Error::Buffer => "buffer too small",
        }
    }
}

/// A byte source failed (card removed, bus error).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ReadError;

/// Random-access reads of the BMP file.
pub trait BmpSource {
    /// Read up to `buf.len()` bytes at `offset`; fewer (or 0) at end of file.
    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, ReadError>;
}

/// Parsed and validated BMP header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub pixel_offset: u32,
    pub bpp: u16,
    /// Negative height in the file: the first stored row is the top one.
    pub top_down: bool,
    /// Bytes per stored row, padding included.
    pub row_stride: usize,
    /// Luminance of each palette entry (1 and 8 bit); entries the file does
    /// not define are black.
    pub palette_lum: [u8; 256],
}

/// Rec.601 luminance, weights scaled by 256 (white maps to 255).
#[inline]
fn lum(r: u8, g: u8, b: u8) -> u8 {
    ((77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8) as u8
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Validate the start of the file (`head` is what `read_at(0, ..)` returned).
pub fn parse_header(head: &[u8]) -> Result<Header, Error> {
    if head.len() < 2 || head[0] != b'B' || head[1] != b'M' {
        return Err(Error::NotBmp);
    }
    if head.len() < FILE_HEADER + 4 {
        return Err(Error::Truncated);
    }
    let pixel_offset = le32(head, 10);
    let dib = le32(head, 14) as usize;
    // BITMAPINFOHEADER up to BITMAPV5HEADER; the 12 byte OS/2 header is out
    if !(MIN_DIB..=MAX_DIB).contains(&dib) {
        return Err(Error::Header);
    }
    if head.len() < FILE_HEADER + MIN_DIB {
        return Err(Error::Truncated);
    }
    let width = le32(head, 18) as i32;
    let height = le32(head, 22) as i32;
    let bpp = le16(head, 28);
    let compression = le32(head, 30);
    let colors_used = le32(head, 46) as usize;

    if width != WIDTH as i32 || height.unsigned_abs() != HEIGHT as u32 {
        return Err(Error::Dimensions);
    }
    if compression != 0 {
        return Err(Error::Compressed);
    }
    let (row_stride, max_colors) = match bpp {
        1 => (OUT_STRIDE.next_multiple_of(4), 2),
        8 => (WIDTH.next_multiple_of(4), 256),
        24 => (MAX_ROW_STRIDE, 0),
        _ => return Err(Error::Depth),
    };
    if colors_used > max_colors {
        return Err(Error::Header);
    }
    let colors = if max_colors == 0 {
        0
    } else if colors_used == 0 {
        max_colors
    } else {
        colors_used
    };

    let palette_at = FILE_HEADER + dib;
    let palette_end = palette_at + colors * 4;
    if (pixel_offset as usize) < palette_end {
        return Err(Error::Header);
    }
    if head.len() < palette_end {
        return Err(Error::Truncated);
    }
    let mut palette_lum = [0u8; 256];
    for (i, e) in head[palette_at..palette_end].chunks_exact(4).enumerate() {
        // BGRA
        palette_lum[i] = lum(e[2], e[1], e[0]);
    }

    Ok(Header {
        pixel_offset,
        bpp,
        top_down: height < 0,
        row_stride,
        palette_lum,
    })
}

/// Fill `lum_row` with the luminance of one BMP row (1, 8 or 24 bit).
fn row_luminance(row: &[u8], h: &Header, lum_row: &mut [u8; WIDTH]) {
    if h.bpp == 1 {
        for (x, l) in lum_row.iter_mut().enumerate() {
            *l = h.palette_lum[usize::from((row[x / 8] >> (7 - (x & 7))) & 1)];
        }
    } else if h.bpp == 8 {
        for (l, &i) in lum_row.iter_mut().zip(&row[..WIDTH]) {
            *l = h.palette_lum[i as usize];
        }
    } else {
        for (l, px) in lum_row.iter_mut().zip(row[..WIDTH * 3].chunks_exact(3)) {
            // BGR
            *l = lum(px[2], px[1], px[0]);
        }
    }
}

/// One 1-bit BMP row into one output row: the bits whose palette entry is
/// black become ink.
fn row_1bit(row: &[u8], h: &Header, out: &mut [u8]) {
    let ink0 = h.palette_lum[0] < 128;
    let ink1 = h.palette_lum[1] < 128;
    for (o, &b) in out.iter_mut().zip(&row[..OUT_STRIDE]) {
        *o = match (ink0, ink1) {
            (false, false) => 0x00,
            (false, true) => b,
            (true, false) => !b,
            (true, true) => 0xFF,
        };
    }
}

/// Floyd-Steinberg step for one row: `cur` holds the error carried into this
/// row, `next` collects the error for the row below.
fn dither_row(
    lum_row: &[u8; WIDTH],
    cur: &mut [i16; WIDTH],
    next: &mut [i16; WIDTH],
    out: &mut [u8],
) {
    for x in 0..WIDTH {
        let v = (lum_row[x] as i16 + cur[x]).clamp(0, 255);
        let white = v >= 128;
        let e = v - if white { 255 } else { 0 };
        if !white {
            out[x / 8] |= 0x80 >> (x & 7);
        }
        // 7/16 right, 3/16 below left, 5/16 below, 1/16 below right
        if x + 1 < WIDTH {
            cur[x + 1] += e * 7 / 16;
            next[x + 1] += e / 16;
        }
        if x > 0 {
            next[x - 1] += e * 3 / 16;
        }
        next[x] += e * 5 / 16;
    }
}

/// The level (0..4) nearest to `v`.
fn nearest_level(v: i16) -> usize {
    let mut best = 0;
    for l in 1..GRAY_LEVELS.len() {
        if (v - GRAY_LEVELS[l]).abs() < (v - GRAY_LEVELS[best]).abs() {
            best = l;
        }
    }
    best
}

/// Floyd-Steinberg to the four gray levels for one row; sets the ink bit of the
/// pixel in `red` / `bw` (rows of `OUT_STRIDE` bytes) as the table at the top of
/// this file says.
fn gray_row(
    lum_row: &[u8; WIDTH],
    cur: &mut [i16; WIDTH],
    next: &mut [i16; WIDTH],
    red: &mut [u8],
    bw: &mut [u8],
) {
    for x in 0..WIDTH {
        let v = (lum_row[x] as i16 + cur[x]).clamp(0, 255);
        let level = nearest_level(v);
        let e = v - GRAY_LEVELS[level];
        let bit = 0x80 >> (x & 7);
        if level >= 2 {
            red[x / 8] |= bit;
        }
        if level == 3 || level == 1 {
            bw[x / 8] |= bit;
        }
        if x + 1 < WIDTH {
            cur[x + 1] += e * 7 / 16;
            next[x + 1] += e / 16;
        }
        if x > 0 {
            next[x - 1] += e * 3 / 16;
        }
        next[x] += e * 5 / 16;
    }
}

/// Read the file in batches of whole rows and hand every row, in OUTPUT order
/// (top to bottom), to `row_fn(header, y, stored_row)`.
fn for_each_row<S: BmpSource>(
    src: &mut S,
    batch: &mut [u8],
    mut row_fn: impl FnMut(&Header, usize, &[u8]),
) -> Result<(), Error> {
    if batch.len() < MIN_BATCH_BYTES {
        return Err(Error::Buffer);
    }
    let n = src
        .read_at(0, &mut batch[..HEADER_READ_BYTES])
        .map_err(|_| Error::Read)?;
    let h = parse_header(&batch[..n])?;

    let rows_per_batch = batch.len() / h.row_stride;
    if rows_per_batch == 0 {
        return Err(Error::Buffer);
    }

    let mut y = 0;
    while y < HEIGHT {
        let rows = rows_per_batch.min(HEIGHT - y);
        // stored row of the first output row of this batch, lowest stored row
        // first: bottom-up files list the output rows in reverse
        let first = if h.top_down { y } else { HEIGHT - y - rows };
        let at = h
            .pixel_offset
            .checked_add((first * h.row_stride) as u32)
            .ok_or(Error::Header)?;
        let want = rows * h.row_stride;
        let got = src
            .read_at(at, &mut batch[..want])
            .map_err(|_| Error::Read)?;
        if got < want {
            return Err(Error::Truncated);
        }
        for i in 0..rows {
            let stored = if h.top_down { i } else { rows - 1 - i };
            row_fn(
                &h,
                y + i,
                &batch[stored * h.row_stride..(stored + 1) * h.row_stride],
            );
        }
        y += rows;
    }
    Ok(())
}

/// Decode the whole file into `out` (`OUT_BYTES` or more; the first
/// `OUT_BYTES` are written). `batch` is the read buffer, at least
/// `MIN_BATCH_BYTES`; it holds as many whole rows as fit. On `Err` the
/// contents of `out` are meaningless.
pub fn decode<S: BmpSource>(src: &mut S, out: &mut [u8], batch: &mut [u8]) -> Result<(), Error> {
    if out.len() < OUT_BYTES || batch.len() < MIN_BATCH_BYTES {
        return Err(Error::Buffer);
    }
    let out = &mut out[..OUT_BYTES];
    out.fill(0);

    let mut lum_row = [0u8; WIDTH];
    let mut cur = [0i16; WIDTH];
    let mut next = [0i16; WIDTH];
    for_each_row(src, batch, |h, y, row| {
        let dst = &mut out[y * OUT_STRIDE..(y + 1) * OUT_STRIDE];
        if h.bpp == 1 {
            row_1bit(row, h, dst);
        } else {
            row_luminance(row, h, &mut lum_row);
            dither_row(&lum_row, &mut cur, &mut next, dst);
            core::mem::swap(&mut cur, &mut next);
            next.fill(0);
        }
    })
}

/// Like `decode`, to 4 gray levels: `planes` is `GRAY_OUT_BYTES` or more, the
/// RED plane image first, then the BW plane image (see the top of this file).
pub fn decode_gray<S: BmpSource>(
    src: &mut S,
    planes: &mut [u8],
    batch: &mut [u8],
) -> Result<(), Error> {
    if planes.len() < GRAY_OUT_BYTES || batch.len() < MIN_BATCH_BYTES {
        return Err(Error::Buffer);
    }
    let (red, bw) = planes[..GRAY_OUT_BYTES].split_at_mut(OUT_BYTES);
    red.fill(0);
    bw.fill(0);

    let mut lum_row = [0u8; WIDTH];
    let mut cur = [0i16; WIDTH];
    let mut next = [0i16; WIDTH];
    for_each_row(src, batch, |h, y, row| {
        let at = y * OUT_STRIDE..(y + 1) * OUT_STRIDE;
        row_luminance(row, h, &mut lum_row);
        gray_row(
            &lum_row,
            &mut cur,
            &mut next,
            &mut red[at.clone()],
            &mut bw[at],
        );
        core::mem::swap(&mut cur, &mut next);
        next.fill(0);
    })
}

/// Draw the current strip of `image` (as `decode` produced it) into `strip`
/// (after `begin_strip`): ink bits clear the strip's white, so only the dark
/// pixels are touched and `StripCore` does the portrait rotation and clipping.
pub fn draw_strip(strip: &mut StripCore, image: &[u8]) {
    strip.blit_1bpp(image, 0, WIDTH, HEIGHT, OUT_STRIDE, 0, 0, true);
}

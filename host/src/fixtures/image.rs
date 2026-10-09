// PNG and JPEG encoders for the fixture images. Both are synthetic-pattern
// writers, not general encoders: PNG is filter-0 scanlines in one zlib IDAT;
// JPEG is baseline, one component, every 8x8 block a flat colour (DC only),
// so a block's level is exact after the decoder's dequantise + IDCT.

use miniz_oxide::deflate::compress_to_vec_zlib;

use super::zip::{LEVEL, crc32};
use super::{ImageKind, ImageSpec, Pattern};

pub(super) fn encode(im: &ImageSpec) -> Vec<u8> {
    match im.kind {
        ImageKind::Jpeg => jpeg(im),
        _ => png(im),
    }
}

// luminance of pixel (x, y) of a `w` pixel wide image
fn luminance(p: Pattern, x: u32, y: u32, w: u32) -> u8 {
    match p {
        Pattern::Black => 0,
        Pattern::White => 255,
        Pattern::Checker { cell } => {
            let cell = u32::from(cell);
            if (x / cell + y / cell) % 2 == 0 {
                0
            } else {
                255
            }
        }
        Pattern::HorizontalGradient if w > 1 => (x * 255 / (w - 1)) as u8,
        Pattern::HorizontalGradient => 0,
    }
}

// ---- PNG ------------------------------------------------------------------

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
// PLTE: index 0 white, index 1 black
const PALETTE: [u8; 6] = [255, 255, 255, 0, 0, 0];

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn png(im: &ImageSpec) -> Vec<u8> {
    let (w, h) = (u32::from(im.width), u32::from(im.height));
    let (depth, color_type) = match im.kind {
        ImageKind::PngGray1 => (1u8, 0u8),
        ImageKind::PngGray8 => (8, 0),
        _ => (8, 3),
    };

    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0); // filter: None
        match im.kind {
            ImageKind::PngGray1 => {
                for byte_x in (0..w).step_by(8) {
                    let mut byte = 0u8;
                    for x in byte_x..(byte_x + 8).min(w) {
                        let white = luminance(im.pattern, x, y, w) >= 128;
                        byte |= u8::from(white) << (7 - (x - byte_x));
                    }
                    raw.push(byte);
                }
            }
            ImageKind::PngGray8 => raw.extend((0..w).map(|x| luminance(im.pattern, x, y, w))),
            _ => raw.extend((0..w).map(|x| u8::from(luminance(im.pattern, x, y, w) < 128))),
        }
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color_type, 0, 0, 0]);

    let mut out = PNG_SIGNATURE.to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    if color_type == 3 {
        chunk(&mut out, b"PLTE", &PALETTE);
    }
    chunk(&mut out, b"IDAT", &compress_to_vec_zlib(&raw, LEVEL));
    chunk(&mut out, b"IEND", &[]);
    out
}

// ---- JPEG -----------------------------------------------------------------

// DC table: the standard luminance table (categories 0..=11). AC table: the
// only symbol is EOB, since every block carries no AC coefficient.
const DC_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_BITS: [u8; 16] = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const AC_VALUES: [u8; 1] = [0x00];
// flat quantiser 8: the quantised DC of a block is (level - 128) exactly
const QUANT: u8 = 8;

// canonical Huffman code (code, length) of `symbol`
fn huffman_code(bits: &[u8; 16], values: &[u8], symbol: u8) -> (u32, u8) {
    let mut code = 0u32;
    let mut next = 0;
    for len in 1..=16u8 {
        for _ in 0..bits[usize::from(len) - 1] {
            if values[next] == symbol {
                return (code, len);
            }
            code += 1;
            next += 1;
        }
        code <<= 1;
    }
    unreachable!("symbol {symbol} is in the table");
}

// entropy-coded segment writer: MSB first, 0xFF byte-stuffed, padded with 1s
struct Bits {
    bytes: Vec<u8>,
    acc: u32,
    n: u8,
}

impl Bits {
    fn put(&mut self, code: u32, len: u8) {
        self.acc = (self.acc << len) | code;
        self.n += len;
        while self.n >= 8 {
            self.n -= 8;
            self.byte((self.acc >> self.n) as u8);
        }
        self.acc &= (1 << self.n) - 1;
    }

    fn byte(&mut self, b: u8) {
        self.bytes.push(b);
        if b == 0xFF {
            self.bytes.push(0x00);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
        self.bytes
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&(body.len() as u16 + 2).to_be_bytes());
    out.extend_from_slice(body);
}

fn dht(out: &mut Vec<u8>, class_id: u8, bits: &[u8; 16], values: &[u8]) {
    let mut body = vec![class_id];
    body.extend_from_slice(bits);
    body.extend_from_slice(values);
    segment(out, 0xC4, &body);
}

fn jpeg(im: &ImageSpec) -> Vec<u8> {
    let (w, h) = (u32::from(im.width), u32::from(im.height));
    let (bw, bh) = (w / 8, h / 8);

    let (eob, eob_len) = huffman_code(&AC_BITS, &AC_VALUES, 0x00);
    let mut scan = Bits {
        bytes: Vec::new(),
        acc: 0,
        n: 0,
    };
    let mut pred = 0i32;
    for by in 0..bh {
        for bx in 0..bw {
            let level = match im.pattern {
                Pattern::HorizontalGradient => luminance(im.pattern, bx, by, bw),
                _ => luminance(im.pattern, bx * 8, by * 8, w),
            };
            let dc = i32::from(level) - 128;
            let diff = dc - pred;
            pred = dc;
            let size = (32 - diff.unsigned_abs().leading_zeros()) as u8;
            let (code, len) = huffman_code(&DC_BITS, &DC_VALUES, size);
            scan.put(code, len);
            if size > 0 {
                let mask = (1i32 << size) - 1;
                let value = if diff < 0 { diff + mask } else { diff };
                scan.put((value & mask) as u32, size);
            }
            scan.put(eob, eob_len);
        }
    }

    let mut out = vec![0xFF, 0xD8];
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[QUANT; 64]);
    segment(&mut out, 0xDB, &dqt);
    let mut sof = vec![8];
    sof.extend_from_slice(&im.height.to_be_bytes());
    sof.extend_from_slice(&im.width.to_be_bytes());
    sof.extend_from_slice(&[1, 1, 0x11, 0]); // one component, 1x1 sampling, table 0
    segment(&mut out, 0xC0, &sof);
    dht(&mut out, 0x00, &DC_BITS, &DC_VALUES);
    dht(&mut out, 0x10, &AC_BITS, &AC_VALUES);
    segment(&mut out, 0xDA, &[1, 1, 0x00, 0, 63, 0]);
    out.extend_from_slice(&scan.finish());
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

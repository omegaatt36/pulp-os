//! Shared test support. Layout numbers here are literals on purpose: they are
//! the hand-derived format oracle, not imports from the crate under test.
#![allow(dead_code)]

#[cfg(not(feature = "builder"))]
compile_error!(
    "pulp-fontpack tests need the pack builder: run with `--features builder` \
     (scripts/host-test.sh does this)"
);

use pulp_fontpack::{FontInfo, GlyphEntry, Metrics, Pack, PackError};

// ---- header field offsets (bytes) ----
pub const HEADER_LEN: usize = 44;
pub const H_MAGIC: usize = 0;
pub const H_VERSION: usize = 4;
pub const H_PIXEL_SIZE: usize = 6;
pub const H_FONT_ID: usize = 8;
pub const H_LINE_HEIGHT: usize = 16;
pub const H_ASCENT: usize = 18;
pub const H_COUNT: usize = 20;
pub const H_INDEX_OFFSET: usize = 24;
pub const H_INDEX_LEN: usize = 28;
pub const H_BITMAP_OFFSET: usize = 32;
pub const H_BITMAP_LEN: usize = 36;
pub const H_TOTAL_LEN: usize = 40;

// ---- index record field offsets (relative to record start) ----
pub const RECORD_LEN: usize = 22;
pub const R_CODEPOINT: usize = 0;
pub const R_BITMAP_OFFSET: usize = 4;
pub const R_BITMAP_LEN: usize = 8;
pub const R_ADVANCE: usize = 12;
pub const R_OFFSET_X: usize = 14;
pub const R_OFFSET_Y: usize = 16;
pub const R_WIDTH: usize = 18;
pub const R_HEIGHT: usize = 20;

/// Absolute position of field `field` of index record `i`.
pub fn rec(i: usize, field: usize) -> usize {
    HEADER_LEN + RECORD_LEN * i + field
}

pub fn put_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// The error `Pack::parse` reports; panics if the input is accepted.
pub fn err(bytes: &[u8]) -> PackError {
    match Pack::parse(bytes) {
        Ok(_) => panic!("damaged pack was accepted"),
        Err(e) => e,
    }
}

/// A copy of the golden pack with `f` applied.
pub fn golden_patched(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut v = GOLDEN.to_vec();
    f(&mut v);
    v
}

// ---------------------------------------------------------------------------
// Golden pack: hand-written bytes, the format contract's oracle.
//
// Font: pixel_size 16, font_id 0x0123_4567_89AB_CDEF, line_height 20, ascent 15.
// 4 glyphs: U+0041, U+FFFF, U+20BB7, U+10FFFF (the last one is blank).
// Layout: header [0,44) + index 4*22=88 [44,132) + bitmap 7 [132,139).
// All multi-byte fields little-endian.
// ---------------------------------------------------------------------------
pub const GOLDEN: [u8; 139] = [
    // ---- header [0,44) ----
    0x50, 0x46, 0x4E, 0x54, // [0,4)   magic = "PFNT"
    0x01, 0x00, //             [4,6)   format version = 1 (u16)
    0x10, 0x00, //             [6,8)   pixel_size = 16 (u16)
    0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23,
    0x01, // [8,16) font_id = 0x0123456789ABCDEF (u64)
    0x14, 0x00, //             [16,18) line_height = 20 (u16)
    0x0F, 0x00, //             [18,20) ascent = 15 (u16)
    0x04, 0x00, 0x00, 0x00, // [20,24) glyph_count = 4 (u32)
    0x2C, 0x00, 0x00, 0x00, // [24,28) index_offset = 44 (u32)
    0x58, 0x00, 0x00, 0x00, // [28,32) index_len = 88 = 4*22 (u32)
    0x84, 0x00, 0x00, 0x00, // [32,36) bitmap_offset = 132 = 44+88 (u32)
    0x07, 0x00, 0x00, 0x00, // [36,40) bitmap_len = 7 (u32)
    0x8B, 0x00, 0x00, 0x00, // [40,44) total_len = 139 = 132+7 (u32)
    // ---- index record 0 [44,66): U+0041 'A' ----
    0x41, 0x00, 0x00, 0x00, // codepoint = 0x41 (u32)
    0x00, 0x00, 0x00, 0x00, // bitmap_offset = 0 (u32, relative to bitmap region)
    0x02, 0x00, 0x00, 0x00, // bitmap_len = 2 = stride 1 * height 2 (u32)
    0x09, 0x00, //             advance = 9 (u16)
    0x01, 0x00, //             offset_x = 1 (i16)
    0x0C, 0x00, //             offset_y = 12 (i16)
    0x08, 0x00, //             width = 8 (u16)
    0x02, 0x00, //             height = 2 (u16)
    // ---- index record 1 [66,88): U+FFFF ----
    0xFF, 0xFF, 0x00, 0x00, // codepoint = 0xFFFF
    0x02, 0x00, 0x00, 0x00, // bitmap_offset = 2
    0x01, 0x00, 0x00, 0x00, // bitmap_len = 1 = stride 1 * height 1
    0x01, 0x00, //             advance = 1
    0x00, 0x00, //             offset_x = 0
    0x00, 0x00, //             offset_y = 0
    0x01, 0x00, //             width = 1
    0x01, 0x00, //             height = 1
    // ---- index record 2 [88,110): U+20BB7 (4-byte UTF-8 scalar) ----
    0xB7, 0x0B, 0x02, 0x00, // codepoint = 0x00020BB7
    0x03, 0x00, 0x00, 0x00, // bitmap_offset = 3
    0x04, 0x00, 0x00, 0x00, // bitmap_len = 4 = stride ceil(9/8)=2 * height 2
    0x10, 0x00, //             advance = 16
    0x00, 0x00, //             offset_x = 0
    0xFE, 0xFF, //             offset_y = -2 (i16 two's complement)
    0x09, 0x00, //             width = 9
    0x02, 0x00, //             height = 2
    // ---- index record 3 [110,132): U+10FFFF, blank glyph ----
    0xFF, 0xFF, 0x10, 0x00, // codepoint = 0x0010FFFF
    0x07, 0x00, 0x00, 0x00, // bitmap_offset = 7 (= bitmap_len, legal for len 0)
    0x00, 0x00, 0x00, 0x00, // bitmap_len = 0
    0x05, 0x00, //             advance = 5
    0x00, 0x00, //             offset_x = 0
    0x00, 0x00, //             offset_y = 0
    0x00, 0x00, //             width = 0
    0x00, 0x00, //             height = 0
    // ---- bitmap region [132,139) ----
    0x3C, 0x42, //             glyph U+0041: rows 0x3C, 0x42
    0x80, //                   glyph U+FFFF: one row
    0xFF, 0x80, 0x00, 0x80, // glyph U+20BB7: row0 = FF 80, row1 = 00 80
];

/// Expected glyphs of the golden pack, in index order.
pub fn golden_glyphs() -> Vec<(char, Metrics, Vec<u8>)> {
    vec![
        (
            '\u{41}',
            Metrics {
                advance: 9,
                offset_x: 1,
                offset_y: 12,
                width: 8,
                height: 2,
            },
            vec![0x3C, 0x42],
        ),
        (
            '\u{FFFF}',
            Metrics {
                advance: 1,
                offset_x: 0,
                offset_y: 0,
                width: 1,
                height: 1,
            },
            vec![0x80],
        ),
        (
            '\u{20BB7}',
            Metrics {
                advance: 16,
                offset_x: 0,
                offset_y: -2,
                width: 9,
                height: 2,
            },
            vec![0xFF, 0x80, 0x00, 0x80],
        ),
        (
            '\u{10FFFF}',
            Metrics {
                advance: 5,
                offset_x: 0,
                offset_y: 0,
                width: 0,
                height: 0,
            },
            vec![],
        ),
    ]
}

pub const GOLDEN_INFO: FontInfo = FontInfo {
    pixel_size: 16,
    font_id: 0x0123_4567_89AB_CDEF,
    line_height: 20,
    ascent: 15,
};

pub fn golden_entries() -> Vec<GlyphEntry> {
    golden_glyphs()
        .into_iter()
        .map(|(codepoint, metrics, bitmap)| GlyphEntry {
            codepoint,
            metrics,
            bitmap,
        })
        .collect()
}

// Golden empty pack: 0 glyphs, header only, 44 bytes. Same font info.
pub const GOLDEN_EMPTY: [u8; 44] = [
    0x50, 0x46, 0x4E, 0x54, // magic
    0x01, 0x00, //             version 1
    0x10, 0x00, //             pixel_size 16
    0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01, // font_id
    0x14, 0x00, //             line_height 20
    0x0F, 0x00, //             ascent 15
    0x00, 0x00, 0x00, 0x00, // glyph_count 0
    0x2C, 0x00, 0x00, 0x00, // index_offset 44
    0x00, 0x00, 0x00, 0x00, // index_len 0
    0x2C, 0x00, 0x00, 0x00, // bitmap_offset 44
    0x00, 0x00, 0x00, 0x00, // bitmap_len 0
    0x2C, 0x00, 0x00, 0x00, // total_len 44
];

// ---------------------------------------------------------------------------
// Synthetic packs whose bitmap bytes are a function of (glyph index, byte pos),
// so a lookup that lands on the wrong offset returns recognisably wrong bytes.
// ---------------------------------------------------------------------------

/// Byte `j` of glyph number `k`. A multiplicative hash, so slices of two
/// different glyphs (or two offsets of one glyph) do not coincide.
pub fn pattern(k: u32, j: u32) -> u8 {
    let x = k.wrapping_mul(0x9E37_79B1) ^ j.wrapping_mul(0x85EB_CA6B).rotate_left(7);
    let x = x ^ (x >> 15);
    let x = x.wrapping_mul(0x2C1B_3C6D);
    (x ^ (x >> 12)) as u8
}

pub fn stride(width: u16) -> usize {
    (width as usize).div_ceil(8)
}

/// Glyph `k` with the given scalar and size; metrics vary with `k`.
pub fn synth_glyph(k: u32, codepoint: char, width: u16, height: u16) -> GlyphEntry {
    let len = stride(width) * height as usize;
    GlyphEntry {
        codepoint,
        metrics: Metrics {
            advance: (k % 60_000) as u16 + 1,
            offset_x: (k % 5) as i16 - 2,
            offset_y: -((k % 7) as i16),
            width,
            height,
        },
        bitmap: (0..len as u32).map(|j| pattern(k, j)).collect(),
    }
}

pub const SYNTH_INFO: FontInfo = FontInfo {
    pixel_size: 24,
    font_id: 0xFEED_FACE_0BAD_F00D,
    line_height: 30,
    ascent: 24,
};

/// Pack whose bitmap region exceeds 65,535 bytes, with glyphs starting exactly
/// at 0, 65,535 and 65,536 (offsets by cumulative sum of lengths below).
///
/// index  codepoint   w   h   len     bitmap_offset
///   0    U+0061      8   65535 65535        0
///   1    U+0062      8   1     1        65535
///   2    U+0100      8   8     8        65536   <- u16 truncation gives 0
///   3    U+FFFF     16   100   200      65544
///   4    U+10000    24   50    150      65744
///   5    U+20BB7    64   30    240      65894
///   6    U+10FFFF    9   20    40       66134
/// bitmap_len = 66174 > 65535.
pub fn large_offset_entries() -> Vec<GlyphEntry> {
    let specs: [(char, u16, u16); 7] = [
        ('\u{61}', 8, 65535),
        ('\u{62}', 8, 1),
        ('\u{100}', 8, 8),
        ('\u{FFFF}', 16, 100),
        ('\u{10000}', 24, 50),
        ('\u{20BB7}', 64, 30),
        ('\u{10FFFF}', 9, 20),
    ];
    specs
        .iter()
        .enumerate()
        .map(|(k, &(c, w, h))| synth_glyph(k as u32, c, w, h))
        .collect()
}

pub const LARGE_OFFSET_EXPECTED: [u32; 7] = [0, 65535, 65536, 65544, 65744, 65894, 66134];
pub const LARGE_OFFSET_BITMAP_LEN: u32 = 66174;

/// Many glyphs, so that the index region alone exceeds 64 KiB
/// (3000 * 22 = 66,000) and the bitmap region reaches ~700 KB.
/// Codepoints are `i * 351` for i in 0..3000, skipping surrogates; every
/// glyph is 64x30 (240 bytes).
pub fn many_glyph_entries() -> Vec<GlyphEntry> {
    let mut out = Vec::new();
    for i in 0..3000u32 {
        if let Some(c) = char::from_u32(i * 351) {
            let k = out.len() as u32;
            out.push(synth_glyph(k, c, 64, 30));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Deterministic PRNG (xorshift64), no dependency.
// ---------------------------------------------------------------------------
pub struct XorShift64(u64);

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        assert_ne!(seed, 0);
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// Invariants every accepted pack must satisfy, whatever its bytes.
/// Never panics itself on a correct implementation; used by the fuzz tests.
pub fn assert_accepted_pack_is_sound(bytes: &[u8], pack: &Pack<'_>, rng: &mut XorShift64) {
    let h = pack.header();
    let lo = bytes.as_ptr() as usize;
    let hi = lo + bytes.len();
    let mut cps: Vec<char> = Vec::new();
    for i in 0..h.glyph_count {
        let (c, g) = pack.glyph_at(i).expect("glyph_at within glyph_count");
        if let Some(&prev) = cps.last() {
            assert!(c > prev, "accepted pack has non-increasing codepoints");
        }
        cps.push(c);
        let s = g.bitmap;
        if !s.is_empty() {
            let a = s.as_ptr() as usize;
            assert!(a >= lo && a + s.len() <= hi, "bitmap slice outside input");
        }
        assert_eq!(s.len(), stride(g.metrics.width) * g.metrics.height as usize);
        assert_eq!(pack.find(c), Some(g));
    }
    assert!(pack.glyph_at(h.glyph_count).is_none());
    assert!(pack.glyph_at(u32::MAX).is_none());
    for _ in 0..8 {
        let probe = loop {
            if let Some(c) = char::from_u32(rng.below(0x11_0000) as u32) {
                break c;
            }
        };
        let linear = cps.iter().position(|&c| c == probe);
        match (pack.find(probe), linear) {
            (None, None) => {}
            (Some(g), Some(i)) => assert_eq!(pack.glyph_at(i as u32).unwrap().1, g),
            (a, b) => panic!("find/linear scan disagree: {a:?} vs {b:?}"),
        }
    }
}

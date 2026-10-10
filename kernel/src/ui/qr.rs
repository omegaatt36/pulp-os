// qr code widget: encode once, blit scaled modules per strip
//
// no-heap, no dependency encoder for the small symbols the upload screen
// needs: byte mode, ECC level L, versions 1..=4 (single RS block each),
// automatic mask selection with the standard penalty rules. everything
// lives in one fixed bitmap inside `QrSymbol`.
//
// encoding and drawing are split on purpose: the draw closure runs once per
// strip and mask selection scores eight candidates, so the caller encodes
// once and only the finished bitmap is blitted per strip.

use embedded_graphics::{
    pixelcolor::BinaryColor, prelude::*, primitives::PrimitiveStyle, primitives::Rectangle,
};

use crate::drivers::strip::StripBuffer;
use crate::ui::Region;

// version 4 = 33x33 modules, 78 bytes at ECC L; plenty for `http://a.b.c.d/`
const MAX_VERSION: u8 = 4;
const MAX_SIDE: usize = MAX_VERSION as usize * 4 + 17;
const MODULE_BYTES: usize = (MAX_SIDE * MAX_SIDE).div_ceil(8);

// ECC L, versions 1..=4: data and error correction codewords of the one block
const DATA_CW: [usize; MAX_VERSION as usize] = [19, 34, 55, 80];
const ECC_CW: [usize; MAX_VERSION as usize] = [7, 10, 15, 20];
const MAX_CW: usize = 100;
const MAX_ECC: usize = 20;

// quiet zone the standard requires, in modules per side
const QUIET: u16 = 4;

// mask 0..=7 selector, ECC L format bits
const FORMAT_ECC_L: u32 = 1;

pub struct QrSymbol {
    modules: [u8; MODULE_BYTES],
    side: u8,
}

impl QrSymbol {
    // smallest version that holds `text` as bytes, or None past version 4
    pub fn encode(text: &str) -> Option<Self> {
        Self::build(text, None)
    }

    // `forced` pins the mask instead of scoring all eight (tests)
    fn build(text: &str, forced: Option<u32>) -> Option<Self> {
        let data = text.as_bytes();
        // 4 bit mode + 8 bit count leave data_cw - 2 bytes
        let version = (0..MAX_VERSION as usize).find(|&v| data.len() <= DATA_CW[v] - 2)? + 1;
        let data_cw = DATA_CW[version - 1];
        let ecc_cw = ECC_CW[version - 1];

        let mut cw = [0u8; MAX_CW];
        let mut bits = 0usize;
        put_bits(&mut cw, &mut bits, 0b0100, 4);
        put_bits(&mut cw, &mut bits, data.len() as u32, 8);
        for &b in data {
            put_bits(&mut cw, &mut bits, b as u32, 8);
        }
        // terminator (up to 4 zero bits), then pad to a byte boundary
        bits += (data_cw * 8 - bits).min(4);
        bits = bits.div_ceil(8) * 8;
        let mut pad = 0xEC;
        while bits < data_cw * 8 {
            put_bits(&mut cw, &mut bits, pad, 8);
            pad ^= 0xEC ^ 0x11;
        }
        let (data_part, ecc_part) = cw.split_at_mut(data_cw);
        rs_remainder(data_part, &mut ecc_part[..ecc_cw]);

        let mut q = Self {
            modules: [0; MODULE_BYTES],
            side: (version * 4 + 17) as u8,
        };
        q.place_codewords(&cw[..data_cw + ecc_cw], version);

        // keep the lowest penalty; ties go to the lower mask number
        let mut best = (i32::MAX, 0u32);
        for mask in forced.map_or(0..8, |m| m..m + 1) {
            q.apply_mask(mask, version);
            q.draw_function(mask, version);
            let score = q.penalty();
            if score < best.0 {
                best = (score, mask);
            }
            q.apply_mask(mask, version);
        }
        q.apply_mask(best.1, version);
        q.draw_function(best.1, version);
        Some(q)
    }

    pub const fn side(&self) -> u8 {
        self.side
    }

    pub fn is_dark(&self, x: u8, y: u8) -> bool {
        self.get(x as i32, y as i32)
    }

    // largest whole pixel module size that fits symbol and quiet zone in
    // `region`; a fractional scale would round module edges unevenly
    fn scale_for(&self, region: Region) -> u16 {
        region.w.min(region.h) / (self.side as u16 + QUIET * 2)
    }

    // pixel side of the drawn symbol, quiet zone included (0: does not fit)
    pub fn drawn_size(&self, region: Region) -> u16 {
        (self.side as u16 + QUIET * 2) * self.scale_for(region)
    }

    // centred in `region`: white quiet zone, black modules; nothing if the
    // region cannot hold one pixel per module
    pub fn draw(&self, strip: &mut StripBuffer, region: Region) {
        let scale = self.scale_for(region);
        if scale == 0 {
            return;
        }
        let size = self.drawn_size(region);
        let x0 = region.x + (region.w - size) / 2;
        let y0 = region.y + (region.h - size) / 2;
        let frame = Region::new(x0, y0, size, size);
        if !frame.intersects(strip.logical_window()) {
            return;
        }

        // the quiet zone is part of the symbol: without it a decoder cannot
        // find the finder patterns against the rest of the page
        frame
            .to_rect()
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(strip)
            .unwrap();

        let window = strip.logical_window();
        let side = self.side;
        for y in 0..side {
            let py = y0 + (QUIET + y as u16) * scale;
            if !Region::new(x0, py, size, scale).intersects(window) {
                continue;
            }
            // one fill per horizontal run of dark modules
            let mut x = 0;
            while x < side {
                if !self.is_dark(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < side && self.is_dark(x, y) {
                    x += 1;
                }
                Rectangle::new(
                    Point::new((x0 + (QUIET + start as u16) * scale) as i32, py as i32),
                    Size::new((x - start) as u32 * scale as u32, scale as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(strip)
                .unwrap();
            }
        }
    }

    // out of range reads as light, which is how the penalty rules see the
    // border
    fn get(&self, x: i32, y: i32) -> bool {
        let n = self.side as i32;
        if x < 0 || y < 0 || x >= n || y >= n {
            return false;
        }
        let bit = (y * n + x) as usize;
        self.modules[bit / 8] >> (bit % 8) & 1 != 0
    }

    fn set(&mut self, x: i32, y: i32, dark: bool) {
        let n = self.side as i32;
        if x < 0 || y < 0 || x >= n || y >= n {
            return;
        }
        let bit = (y * n + x) as usize;
        if dark {
            self.modules[bit / 8] |= 1 << (bit % 8);
        } else {
            self.modules[bit / 8] &= !(1 << (bit % 8));
        }
    }

    // zigzag over column pairs from the right, skipping the timing column;
    // modules past the last codeword bit (remainder bits) stay light
    fn place_codewords(&mut self, cw: &[u8], version: usize) {
        let n = self.side as i32;
        let total = cw.len() * 8;
        let mut bit = 0;
        let mut right = n - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            let upward = (right + 1) & 2 == 0;
            for v in 0..n {
                let y = if upward { n - 1 - v } else { v };
                for x in [right, right - 1] {
                    if !is_function(x, y, n, version) && bit < total {
                        self.set(x, y, cw[bit / 8] >> (7 - bit % 8) & 1 != 0);
                        bit += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    // flips the data modules the mask selects; applying it twice undoes it
    fn apply_mask(&mut self, mask: u32, version: usize) {
        let n = self.side as i32;
        for y in 0..n {
            for x in 0..n {
                if mask_hit(mask, x as u32, y as u32) && !is_function(x, y, n, version) {
                    let d = self.get(x, y);
                    self.set(x, y, !d);
                }
            }
        }
    }

    // finder, timing, alignment and format modules (overwrites any state)
    fn draw_function(&mut self, mask: u32, version: usize) {
        let n = self.side as i32;
        for i in 0..n {
            self.set(6, i, i % 2 == 0);
            self.set(i, 6, i % 2 == 0);
        }
        for (cx, cy) in [(3, 3), (n - 4, 3), (3, n - 4)] {
            for dy in -4..=4i32 {
                for dx in -4..=4i32 {
                    let dist = dx.abs().max(dy.abs());
                    self.set(cx + dx, cy + dy, dist != 2 && dist != 4);
                }
            }
        }
        if version >= 2 {
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    self.set(n - 7 + dx, n - 7 + dy, dx.abs().max(dy.abs()) != 1);
                }
            }
        }

        // BCH(15,5) over the ECC and mask bits, xored with the fixed mask
        let data = FORMAT_ECC_L << 3 | mask;
        let mut rem = data;
        for _ in 0..10 {
            rem = rem << 1 ^ (rem >> 9) * 0x537;
        }
        let bits = (data << 10 | rem) ^ 0x5412;
        let bit = |i: i32| bits >> i & 1 != 0;
        for i in 0..=5 {
            self.set(8, i, bit(i));
        }
        self.set(8, 7, bit(6));
        self.set(8, 8, bit(7));
        self.set(7, 8, bit(8));
        for i in 9..15 {
            self.set(14 - i, 8, bit(i));
        }
        for i in 0..8 {
            self.set(n - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            self.set(8, n - 15 + i, bit(i));
        }
        // the always-dark module
        self.set(8, n - 8, true);
    }

    // standard penalty: runs, 2x2 blocks, finder-like patterns, dark balance
    fn penalty(&self) -> i32 {
        const FINDER: [bool; 7] = [true, false, true, true, true, false, true];
        let n = self.side as i32;
        let mut score = 0i32;

        for transposed in [false, true] {
            let at = |a: i32, b: i32| {
                if transposed {
                    self.get(a, b)
                } else {
                    self.get(b, a)
                }
            };
            for a in 0..n {
                let mut run = 1;
                for b in 1..n {
                    if at(a, b) == at(a, b - 1) {
                        run += 1;
                    } else {
                        if run >= 5 {
                            score += run - 2;
                        }
                        run = 1;
                    }
                }
                if run >= 5 {
                    score += run - 2;
                }

                // 1011101 with four light modules on either side
                for s in 0..=n - 7 {
                    if (0..7).any(|k| at(a, s + k) != FINDER[k as usize]) {
                        continue;
                    }
                    let light_before = (s - 4..s).all(|b| !at(a, b));
                    let light_after = (s + 7..s + 11).all(|b| !at(a, b));
                    score += 40 * (light_before as i32 + light_after as i32);
                }
            }
        }

        let mut dark = 0i32;
        for y in 0..n {
            for x in 0..n {
                let c = self.get(x, y);
                dark += c as i32;
                if x + 1 < n
                    && y + 1 < n
                    && c == self.get(x + 1, y)
                    && c == self.get(x, y + 1)
                    && c == self.get(x + 1, y + 1)
                {
                    score += 3;
                }
            }
        }
        let total = n * n;
        let k = ((dark * 20 - total * 10).abs() + total - 1) / total - 1;
        score + 10 * k
    }
}

fn put_bits(cw: &mut [u8], len: &mut usize, val: u32, count: usize) {
    for i in (0..count).rev() {
        if val >> i & 1 != 0 {
            cw[*len / 8] |= 0x80 >> (*len % 8);
        }
        *len += 1;
    }
}

fn mask_hit(mask: u32, x: u32, y: u32) -> bool {
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3) % 2 == 0,
        _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
    }
}

// everything but data: finders with separators and format areas, timing
// lines, and the single alignment pattern of versions 2..=4
fn is_function(x: i32, y: i32, n: i32, version: usize) -> bool {
    (x <= 8 && y <= 8)
        || (x >= n - 8 && y <= 8)
        || (x <= 8 && y >= n - 8)
        || x == 6
        || y == 6
        || (version >= 2 && (x - (n - 7)).abs() <= 2 && (y - (n - 7)).abs() <= 2)
}

// GF(256), reducing polynomial 0x11D
fn gf_mul(a: u8, b: u8) -> u8 {
    let mut z = 0u8;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x1D);
        z ^= ((b >> i) & 1) * a;
    }
    z
}

// remainder of data(x) * x^ecc.len() divided by the generator polynomial
fn rs_remainder(data: &[u8], ecc: &mut [u8]) {
    let deg = ecc.len();
    let mut gen_poly = [0u8; MAX_ECC];
    gen_poly[deg - 1] = 1;
    let mut root = 1u8;
    for _ in 0..deg {
        for j in 0..deg {
            gen_poly[j] = gf_mul(gen_poly[j], root);
            if j + 1 < deg {
                gen_poly[j] ^= gen_poly[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }

    ecc.fill(0);
    for &b in data {
        let factor = b ^ ecc[0];
        ecc.copy_within(1.., 0);
        ecc[deg - 1] = 0;
        for (e, &g) in ecc.iter_mut().zip(&gen_poly[..deg]) {
            *e ^= gf_mul(g, factor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(q: &QrSymbol) -> alloc::vec::Vec<alloc::string::String> {
        (0..q.side())
            .map(|y| {
                (0..q.side())
                    .map(|x| if q.is_dark(x, y) { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    // reference rows ('#' dark) come from libqrencode 4.1.1, `qrencode -l L
    // -m 0 -t ASCII -8 <text>`, with the mask it picked; the mask choice is a
    // scoring heuristic that differs between implementations, so the test pins
    // it and compares every module
    fn check(text: &str, mask: u32, expect: &[&str]) {
        let q = QrSymbol::build(text, Some(mask)).unwrap();
        let got = rows(&q);
        assert_eq!(got.len(), expect.len(), "{text:?}");
        for (i, (g, e)) in got.iter().zip(expect).enumerate() {
            assert_eq!(g, e, "row {i} of {text:?}");
        }
    }

    // reads the symbol back the way a scanner would: format info, unmask,
    // zigzag codeword read, Reed-Solomon check, byte-mode payload
    fn decode(q: &QrSymbol) -> alloc::string::String {
        let n = q.side() as i32;
        let version = (n as usize - 17) / 4;
        let bit = |x: i32, y: i32| q.is_dark(x as u8, y as u8) as u32;
        let mut f = 0;
        for i in 0..=5 {
            f |= bit(8, i) << i;
        }
        f |= bit(8, 7) << 6 | bit(8, 8) << 7 | bit(7, 8) << 8;
        for i in 9..15 {
            f |= bit(14 - i, 8) << i;
        }
        let f = (f ^ 0x5412) >> 10;
        assert_eq!(f >> 3, 1, "ecc level L");
        let mask = f & 7;

        let (data_cw, ecc_cw) = (DATA_CW[version - 1], ECC_CW[version - 1]);
        let mut cw = [0u8; MAX_CW];
        let mut k = 0;
        let mut right = n - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for v in 0..n {
                let y = if (right + 1) & 2 == 0 { n - 1 - v } else { v };
                for x in [right, right - 1] {
                    if is_function(x, y, n, version) || k >= (data_cw + ecc_cw) * 8 {
                        continue;
                    }
                    let dark = bit(x, y) != mask_hit(mask, x as u32, y as u32) as u32;
                    cw[k / 8] |= (dark as u8) << (7 - k % 8);
                    k += 1;
                }
            }
            right -= 2;
        }

        let mut ecc = [0u8; MAX_ECC];
        rs_remainder(&cw[..data_cw], &mut ecc[..ecc_cw]);
        assert_eq!(&ecc[..ecc_cw], &cw[data_cw..data_cw + ecc_cw], "ecc");
        assert_eq!(cw[0] >> 4, 0b0100, "byte mode");
        let len = (cw[0] & 15) as usize * 16 + (cw[1] >> 4) as usize;
        let bytes: alloc::vec::Vec<u8> = (0..len)
            .map(|i| (cw[1 + i] << 4) | (cw[2 + i] >> 4))
            .collect();
        alloc::string::String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn matches_libqrencode_21x21_15b() {
        check(
            "http://1.2.3.4/",
            7,
            &[
                "#######..#..#.#######",
                "#.....#.##.#..#.....#",
                "#.###.#.##....#.###.#",
                "#.###.#..#..#.#.###.#",
                "#.###.#.#.....#.###.#",
                "#.....#.#.....#.....#",
                "#######.#.#.#.#######",
                "........####.........",
                "##.#..##.##.#.###.##.",
                "..#.........##..#...#",
                ".....##.#.#.#.#...#.#",
                "###.#..##...###.##.##",
                "#...#.####.#...#.#...",
                "........#.######....#",
                "#######.#..##.#.####.",
                "#.....#..#..##..#...#",
                "#.###.#..#..#..###...",
                "#.###.#.##.######..##",
                "#.###.#...#.#...#.#.#",
                "#.....#.##.##..#.#...",
                "#######.##....#....#.",
            ],
        );
    }

    #[test]
    fn matches_libqrencode_25x25_20b() {
        check(
            "http://192.168.1.50/",
            1,
            &[
                "#######.####.#..#.#######",
                "#.....#.#..#..#...#.....#",
                "#.###.#..#...#....#.###.#",
                "#.###.#....###.#..#.###.#",
                "#.###.#.#..###..#.#.###.#",
                "#.....#.###.#...#.#.....#",
                "#######.#.#.#.#.#.#######",
                "........##...............",
                "###..##.##.##.##.####..##",
                "####.#.##...##.#.##..#.##",
                "##.##.#.###.#..####.###.#",
                ".##..#....###.###.#.##...",
                "#####.#..##..##.###.....#",
                ".#......#.#..#.#..#....##",
                "##...###..##...##...###.#",
                ".....#.##....#....#......",
                "###.###.##.###..#####..#.",
                "........#.#.#..##...#.#.#",
                "#######..#..###.#.#.##..#",
                "#.....#.##.##.#.#...#....",
                "#.###.#..##..#########..#",
                "#.###.#..#...##.#..#####.",
                "#.###.#.#..#....#..##..##",
                "#.....#.#.#..#.##..##....",
                "#######.##.###.##.#..#..#",
            ],
        );
    }

    #[test]
    fn matches_libqrencode_25x25_23b() {
        check(
            "http://192.168.100.100/",
            2,
            &[
                "#######...#.#.#...#######",
                "#.....#.#.#....#..#.....#",
                "#.###.#.....#..#..#.###.#",
                "#.###.#.###.##.##.#.###.#",
                "#.###.#..#...#....#.###.#",
                "#.....#.##.######.#.....#",
                "#######.#.#.#.#.#.#######",
                ".........###...#.........",
                "#####.####.#.#.###.#.#.#.",
                ".#..#..##.#.#.#..#.....#.",
                "#..##.##..#....#..##.#.##",
                "#...#..#....#.#.#...#...#",
                "####.######.##....###.###",
                "#..#.#...#....#......#.#.",
                "#.##..###..##..#.#.#.#.##",
                "#..#...###.#...#.....#..#",
                "#...#.#.#.##.#..#####.#..",
                "........#...#.#.#...###..",
                "#######.###.....#.#.#####",
                "#.....#..#..#..##...##.#.",
                "#.###.#.#.#.##.##########",
                "#.###.#.##...#.##.###.###",
                "#.###.#.#.#####..#....#.#",
                "#.....#.#..#..#.#.####..#",
                "#######.#.###..#.########",
            ],
        );
    }

    #[test]
    fn matches_libqrencode_25x25_25b() {
        check(
            "http://10.20.30.40/upload",
            6,
            &[
                "#######.#..###..#.#######",
                "#.....#..##..##...#.....#",
                "#.###.#...#.##.#..#.###.#",
                "#.###.#.....###...#.###.#",
                "#.###.#..#.#.##...#.###.#",
                "#.....#...#.###.#.#.....#",
                "#######.#.#.#.#.#.#######",
                "........#.##.#.##........",
                "##.##.#..###..#.#.#.....#",
                "..#......#..#..##...####.",
                ".#..#.#...##...#....##..#",
                "...#.#.#.####.#.##...####",
                "...##.#.##.##..#.##.....#",
                "##.###..#....#.#.#.##..#.",
                "##..#.#...####.#..#..####",
                "#..##...##.#..###...#.#.#",
                "#..#####.##.##.######.##.",
                "........######.##...#..#.",
                "#######..#####..#.#.##..#",
                "#.....#..#...##.#...#..##",
                "#.###.#.#.#.#.########...",
                "#.###.#.####.##...##.#.##",
                "#.###.#....#.#..#...#.###",
                "#.....#.#.###.#..####.###",
                "#######.##.##..####..#..#",
            ],
        );
    }

    #[test]
    fn matches_libqrencode_33x33_58b() {
        check(
            "http://192.168.178.123/?name=pulp&session=0123456789abcdef",
            2,
            &[
                "#######..#.....##..#.#.#..#######",
                "#.....#.##.##.....#..##...#.....#",
                "#.###.#..###..#.#....##...#.###.#",
                "#.###.#.##.#.....##.#..##.#.###.#",
                "#.###.#...#.####...#..##..#.###.#",
                "#.....#.#.#..###.##.###...#.....#",
                "#######.#.#.#.#.#.#.#.#.#.#######",
                "............##..#.#.###.#........",
                "#####.#####.#.##.####.#.##.#.#.#.",
                "#..#.#.#.#......#..#.###.###...##",
                "......#.##.##..####..#..#..##....",
                ".#..##...###..###..###.#.#..#.#..",
                ".#.#.##.##.#...#.###..###..##....",
                "##..##.#..#.###.#..#####.###.#.##",
                ".##..###..#..###....###..#.#.#.#.",
                "#.#.....##..##.##.######.####.#..",
                "##...##.##..#.##.###..##....#..#.",
                ".#.#.#.#..#.....#..##..#####.#.##",
                "..##.##.#..##..##...###...####.#.",
                "##.#.#.###.#..#......#.#......#..",
                "####..#..#.#...#.###..###...##.#.",
                "##.##...###.###.#..######.##..###",
                "#.#..##.#....###.#..##...##..###.",
                "#....#....#.##....##.#.##.##..#..",
                "#.###.###.#.#.##.##.#...######..#",
                "........##......#..###.##...#...#",
                "#######.##.##..###..#.###.#.####.",
                "#.....#...##..##...#.#..#...#.#.#",
                "#.###.#.#.##...#.##.#.#.######..#",
                "#.###.#.#...###.##.####.....#.###",
                "#.###.#.##...####.#.#...####.....",
                "#.....#.##..##.#..#.####...##.#..",
                "#######.##..#.#.###...#.##..#..#.",
            ],
        );
    }

    #[test]
    fn matches_libqrencode_33x33_78b() {
        check(
            "http://192.168.178.123/?name=pulp&session=0123456789abcdefghijklmnopqrstuvwxyz",
            2,
            &[
                "#######...##.#..#..#.#.#..#######",
                "#.....#.##.....##.#..##...#.....#",
                "#.###.#..#.#....#....##...#.###.#",
                "#.###.#.#.#..###.##.#..##.#.###.#",
                "#.###.#...##.#..#..#..##..#.###.#",
                "#.....#.##..#..#.##.###...#.....#",
                "#######.#.#.#.#.#.#.#.#.#.#######",
                ".........###..#.#.#.###.#........",
                "#####.###.#..###.####.#.##.#.#.#.",
                ".#........##.#..#..#.###.###...##",
                "#.###.#......#.####..#..#..##....",
                "###.#..#.#.....##..###.#.#..#.#..",
                ".###..##.#...###.###..###..##....",
                ".....#.###.#.#..#..#####.###.#.##",
                "#....##.#...##.#....###..#.#.#.#.",
                "#.#....#......###.######.####.#..",
                "#####.#.##...###.###..##....#..#.",
                "##.#.#.##.##.#..#..##..#####.#.##",
                "##..#.#.#.....###...###...####.#.",
                ".##.#...##.##........#.#......#..",
                "###..##.###..###.###..###...##.#.",
                "##..#....#.#.#..#..######.##..###",
                "#.#.####.#..#.##.#..##...##..###.",
                "#.#.#...#####.#...##.#.##.##..#..",
                "#.########...###.##.#...######..#",
                "........#.##.#..#..###.##...#...#",
                "#######.#.#..#####..#.###.#.####.",
                "#.....#..##.#..#...#.#..#...#.#.#",
                "#.###.#.##...###.##.#.#.######.##",
                "#.###.#.#.##.#..##.####.....#.#..",
                "#.###.#.##..#####.#.#...####...#.",
                "#.....#.##..#.##..#.####...##.#..",
                "#######.#.#..##.###...#.##..#..#.",
            ],
        );
    }

    #[test]
    fn auto_mask_symbols_decode_back() {
        for text in [
            "http://1.2.3.4/",
            "http://192.168.1.50/",
            "http://192.168.100.100/",
            "http://10.20.30.40/upload",
            "http://192.168.178.123/?name=pulp&session=0123456789abcdef",
            "a",
            "caf\u{e9} \u{4e2d}",
        ] {
            assert_eq!(decode(&QrSymbol::encode(text).unwrap()), text);
        }
    }

    #[test]
    fn every_mask_decodes_back() {
        for mask in 0..8 {
            let q = QrSymbol::build("http://192.168.1.50/", Some(mask)).unwrap();
            assert_eq!(decode(&q), "http://192.168.1.50/");
        }
    }

    #[test]
    fn version_follows_length() {
        // byte mode, ECC L capacities: 17, 32, 53, 78
        for (len, side) in [(1, 21), (17, 21), (18, 25), (32, 25), (33, 29), (53, 29)] {
            let text = "a".repeat(len);
            assert_eq!(QrSymbol::encode(&text).unwrap().side(), side, "len {len}");
        }
        assert_eq!(QrSymbol::encode(&"a".repeat(78)).unwrap().side(), 33);
        assert!(QrSymbol::encode(&"a".repeat(79)).is_none());
    }

    #[test]
    fn function_patterns_are_in_place() {
        let q = QrSymbol::encode("http://192.168.1.50/").unwrap();
        let n = q.side();
        // finder corners and centres, always-dark module
        for (x, y) in [(0, 0), (n - 1, 0), (0, n - 1), (3, 3), (8, n - 8)] {
            assert!(q.is_dark(x, y), "({x},{y})");
        }
        assert!(!q.is_dark(7, 0) && !q.is_dark(0, 7));
        // timing line alternates, alignment pattern sits at (n-7, n-7)
        assert!(q.is_dark(6, 8) && !q.is_dark(6, 9) && q.is_dark(8, 6));
        assert!(q.is_dark(n - 7, n - 7) && !q.is_dark(n - 6, n - 7));
    }

    #[test]
    fn reed_solomon_matches_spec_example() {
        // ISO 18004 annex I: "01234567" at version 1-M, 16 data codewords
        // and the 10 ECC codewords the standard lists for them
        let data = [
            0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11,
            0xEC, 0x11,
        ];
        let mut ecc = [0u8; 10];
        rs_remainder(&data, &mut ecc);
        assert_eq!(
            ecc,
            [0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55]
        );
    }
}

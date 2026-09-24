// test-side PFP v1 writer and readers, written from docs/font-pack.txt
// alone (not from the loader or the host converter), so fixtures are an
// independent oracle: every field below cites the spec section it follows
//
// shared by several test crates; each uses a different subset
#![allow(dead_code)]

// host harness for the firmware strip renderer (full vs partial passes)
pub mod strip;

use pulp_render::font_pack::{IoError, PackReader};

// §3 header field offsets
pub const MAGIC: usize = 0;
pub const VERSION: usize = 8;
pub const HEADER_LEN: usize = 10;
pub const PIXEL_SIZE: usize = 12;
pub const LINE_HEIGHT: usize = 14;
pub const ASCENT: usize = 16;
pub const RESERVED: usize = 18;
pub const GLYPH_COUNT: usize = 20;
pub const FALLBACK_CP: usize = 24;
pub const INDEX_OFF: usize = 28;
pub const INDEX_LEN: usize = 32;
pub const BITMAP_OFF: usize = 36;
pub const BITMAP_LEN: usize = 40;
pub const LICENSE_OFF: usize = 44;
pub const LICENSE_LEN: usize = 48;
pub const FILE_LEN: usize = 52;
pub const INDEX_CRC: usize = 56;
pub const HEADER_CRC: usize = 60;

// §2: index starts at 512, 16-byte records
pub const INDEX_START: usize = 512;
pub const RECORD: usize = 16;

// §1: CRC-32/ISO-HDLC, bitwise, reflected poly 0xEDB88320
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

pub fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn get_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

// offset of index record i (§2, §4)
pub fn record_off(i: usize) -> usize {
    INDEX_START + RECORD * i
}

// §3 header_crc32 = CRC32(header[0..60])
pub fn fix_header_crc(buf: &mut [u8]) {
    let crc = crc32(&buf[0..60]);
    put_u32(buf, HEADER_CRC, crc);
}

// §3 index_crc32 = CRC32 over [index_off, index_off + index_len), taken
// from the stored header fields
pub fn fix_index_crc(buf: &mut [u8]) {
    let off = get_u32(buf, INDEX_OFF) as usize;
    let len = get_u32(buf, INDEX_LEN) as usize;
    let crc = crc32(&buf[off..off + len]);
    put_u32(buf, INDEX_CRC, crc);
    fix_header_crc(buf);
}

// §5: row_bytes = ceil(width / 8), len = row_bytes * height
pub fn bitmap_len(w: u8, h: u8) -> usize {
    (w as usize).div_ceil(8) * h as usize
}

#[derive(Clone, Debug)]
pub struct Glyph {
    pub cp: u32,
    pub width: u8,
    pub height: u8,
    pub advance: u8,
    pub offset_x: i8,
    pub offset_y: i8,
    pub bitmap: Vec<u8>,
}

// deterministic planted glyph: metrics and pixels are a function of seed
// only, so a test can recompute the exact values it expects
pub fn planted(cp: u32, seed: usize) -> Glyph {
    let width = 1 + (seed % 20) as u8;
    let height = 1 + (seed % 13) as u8;
    sized(cp, seed, width, height)
}

// planted glyph with explicit size; unused low bits of each row are 0 (§5)
pub fn sized(cp: u32, seed: usize, width: u8, height: u8) -> Glyph {
    let row_bytes = (width as usize).div_ceil(8);
    let spare = row_bytes * 8 - width as usize;
    let mut bitmap = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        for col in 0..row_bytes {
            let mut b = (seed.wrapping_mul(31) ^ row.wrapping_mul(7) ^ col.wrapping_mul(13)) as u8;
            b |= 0x80; // never an all-zero first byte, so a zeroed read is caught
            if col == row_bytes - 1 {
                b &= 0xFFu8 << spare;
            }
            bitmap.push(b);
        }
    }
    Glyph {
        cp,
        width,
        height,
        advance: width.saturating_add(1),
        offset_x: (seed % 5) as i8 - 2,
        offset_y: -(height as i16).min(127) as i8,
        bitmap,
    }
}

pub struct Pack {
    pub pixel_size: u16,
    pub line_height: u16,
    pub ascent: u16,
    pub fallback_cp: u32,
    pub glyphs: Vec<Glyph>,
    pub license: Vec<u8>,
}

pub struct Built {
    pub bytes: Vec<u8>,
    pub bitmap_off: u32,
    // section-relative bitmap_offset planted for each glyph, in order
    pub offsets: Vec<u32>,
}

impl Pack {
    pub fn new(pixel_size: u16, fallback_cp: u32, glyphs: Vec<Glyph>) -> Self {
        Pack {
            pixel_size,
            line_height: pixel_size + 2,
            ascent: pixel_size,
            fallback_cp,
            glyphs,
            license: b"OFL\n".to_vec(),
        }
    }

    // lays out §2 sections: header, zero padding, index, bitmaps packed
    // back to back in index order, license; then fills both CRCs
    pub fn build(&self) -> Built {
        let n = self.glyphs.len();
        let index_len = RECORD * n;
        let bitmap_off = INDEX_START + index_len;
        let mut bitmaps = Vec::new();
        let mut offsets = Vec::with_capacity(n);
        for g in &self.glyphs {
            assert_eq!(g.bitmap.len(), bitmap_len(g.width, g.height), "fixture bug");
            offsets.push(bitmaps.len() as u32);
            bitmaps.extend_from_slice(&g.bitmap);
        }
        let license_off = bitmap_off + bitmaps.len();
        let file_len = license_off + self.license.len();

        let mut b = vec![0u8; bitmap_off];
        b[MAGIC..8].copy_from_slice(b"PULPFONT");
        put_u16(&mut b, VERSION, 1);
        put_u16(&mut b, HEADER_LEN, 64);
        put_u16(&mut b, PIXEL_SIZE, self.pixel_size);
        put_u16(&mut b, LINE_HEIGHT, self.line_height);
        put_u16(&mut b, ASCENT, self.ascent);
        put_u32(&mut b, GLYPH_COUNT, n as u32);
        put_u32(&mut b, FALLBACK_CP, self.fallback_cp);
        put_u32(&mut b, INDEX_OFF, INDEX_START as u32);
        put_u32(&mut b, INDEX_LEN, index_len as u32);
        put_u32(&mut b, BITMAP_OFF, bitmap_off as u32);
        put_u32(&mut b, BITMAP_LEN, bitmaps.len() as u32);
        put_u32(&mut b, LICENSE_OFF, license_off as u32);
        put_u32(&mut b, LICENSE_LEN, self.license.len() as u32);
        put_u32(&mut b, FILE_LEN, file_len as u32);
        for (i, g) in self.glyphs.iter().enumerate() {
            let r = record_off(i);
            put_u32(&mut b, r, g.cp);
            put_u32(&mut b, r + 4, offsets[i]);
            b[r + 8] = g.width;
            b[r + 9] = g.height;
            b[r + 10] = g.advance;
            b[r + 11] = g.offset_x as u8;
            b[r + 12] = g.offset_y as u8;
        }
        b.extend_from_slice(&bitmaps);
        b.extend_from_slice(&self.license);
        assert_eq!(b.len(), file_len);
        fix_index_crc(&mut b);
        Built {
            bytes: b,
            bitmap_off: bitmap_off as u32,
            offsets,
        }
    }
}

// in-memory pack file that counts every read_at call and byte, logs
// each call's (offset, bytes returned), and can be told to fail, as the
// SD layer might
pub struct TestReader {
    pub data: Vec<u8>,
    pub fail_size: Option<IoError>,
    pub fail_read: Option<IoError>,
    pub reads: usize,
    pub bytes_read: usize,
    pub log: Vec<(u32, usize)>,
}

impl TestReader {
    pub fn new(data: Vec<u8>) -> Self {
        TestReader {
            data,
            fail_size: None,
            fail_read: None,
            reads: 0,
            bytes_read: 0,
            log: Vec::new(),
        }
    }

    pub fn reset_counts(&mut self) {
        self.reads = 0;
        self.bytes_read = 0;
        self.log.clear();
    }
}

impl PackReader for TestReader {
    fn size(&mut self) -> Result<u32, IoError> {
        match self.fail_size {
            Some(e) => Err(e),
            None => Ok(self.data.len() as u32),
        }
    }

    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, IoError> {
        if let Some(e) = self.fail_read {
            return Err(e);
        }
        self.reads += 1;
        let start = (offset as usize).min(self.data.len());
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.bytes_read += n;
        self.log.push((offset, n));
        Ok(n)
    }
}

//! Support for the PackReader tests: instrumented `ReadAt` sources, hand-assembled
//! packs and a tiny LCG. Kept in its own module (not appended to
//! `common/mod.rs`) because it names the new reader API: appending it there would
//! stop every older test binary from compiling.
//!
//! Expected values never come from the reader: they come from the builder
//! input, from literals, or from the independent `Pack::parse` oracle.
#![allow(dead_code)]

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pulp_fontpack::{FontInfo, Metrics, PackReader, ReadAt};

use crate::common::*;

/// Every request seen by a `Spy`: `(offset, len)`, in order.
pub type Log = Rc<RefCell<Vec<(u64, usize)>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpyErr {
    /// The k-th read (1-based) was made to fail on purpose.
    Injected(usize),
    /// The source cannot serve that range (outside the bytes it has).
    Unavailable,
}

impl fmt::Display for SpyErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpyErr::Injected(k) => write!(f, "injected failure on read {k}"),
            SpyErr::Unavailable => write!(f, "range unavailable"),
        }
    }
}

/// A `ReadAt` source that logs every request (before anything else), can fail
/// the k-th one, and refuses ranges beyond `len`.
pub struct Spy {
    len: u64,
    fill: Box<dyn Fn(u64, &mut [u8])>,
    log: Log,
    fail_on: Option<usize>,
}

impl Spy {
    /// Serves exactly `data`.
    pub fn bytes(data: Vec<u8>) -> Spy {
        let len = data.len() as u64;
        Spy::sparse(len, move |off, buf| {
            let o = off as usize;
            buf.copy_from_slice(&data[o..o + buf.len()]);
        })
    }

    /// Serves `[0, len)`; `fill(offset, buf)` is only called for in-range requests.
    pub fn sparse(len: u64, fill: impl Fn(u64, &mut [u8]) + 'static) -> Spy {
        Spy {
            len,
            fill: Box::new(fill),
            log: Rc::new(RefCell::new(Vec::new())),
            fail_on: None,
        }
    }

    /// The k-th `read_at` call (1-based, counting from now on) fails with `Injected(k)`.
    pub fn failing_on(mut self, k: usize) -> Spy {
        self.fail_on = Some(k);
        self
    }

    pub fn log(&self) -> Log {
        self.log.clone()
    }
}

impl ReadAt for Spy {
    type Error = SpyErr;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), SpyErr> {
        let k = {
            let mut l = self.log.borrow_mut();
            l.push((offset, buf.len()));
            l.len()
        };
        if self.fail_on == Some(k) {
            return Err(SpyErr::Injected(k));
        }
        match offset.checked_add(buf.len() as u64) {
            Some(end) if end <= self.len => {
                (self.fill)(offset, buf);
                Ok(())
            }
            _ => Err(SpyErr::Unavailable),
        }
    }
}

pub fn reads(log: &Log) -> Vec<(u64, usize)> {
    log.borrow().clone()
}

pub fn read_count(log: &Log) -> usize {
    log.borrow().len()
}

/// Open `bytes` through a `Spy` (file_len = bytes.len()).
pub fn open_spy(bytes: &[u8]) -> (PackReader<Spy>, Log) {
    let spy = Spy::bytes(bytes.to_vec());
    let log = spy.log();
    let r = PackReader::open(spy, bytes.len() as u64).expect("pack must open");
    (r, log)
}

/// Worst-case probe count of a binary search over `n` records: ceil(log2(n+1)).
pub fn max_probes(n: u32) -> usize {
    (32 - n.leading_zeros()) as usize
}

/// The largest scalar below / the smallest scalar above `c`, skipping the surrogate block.
pub fn prev_char(c: char) -> Option<char> {
    let mut v = c as u32;
    loop {
        v = v.checked_sub(1)?;
        if let Some(p) = char::from_u32(v) {
            return Some(p);
        }
    }
}

pub fn next_char(c: char) -> Option<char> {
    let mut v = c as u32;
    loop {
        v += 1;
        if v > 0x10FFFF {
            return None;
        }
        if let Some(n) = char::from_u32(v) {
            return Some(n);
        }
    }
}

// ----------------------------------------------------------------- raw packs

/// One index record, fields exactly as they will be written (no validation).
#[derive(Clone, Copy)]
pub struct RawRec {
    pub cp: u32,
    pub off: u32,
    pub len: u32,
    pub m: Metrics,
}

pub fn m(advance: u16, offset_x: i16, offset_y: i16, width: u16, height: u16) -> Metrics {
    Metrics {
        advance,
        offset_x,
        offset_y,
        width,
        height,
    }
}

/// A record for an 8-bit-aligned glyph `w` x `h` (len = ceil(w/8)*h) at `off`.
pub fn rawrec(cp: u32, off: u32, w: u16, h: u16) -> RawRec {
    RawRec {
        cp,
        off,
        len: (stride(w) * h as usize) as u32,
        m: m((cp % 50) as u16 + 1, -1, 2, w, h),
    }
}

/// header | records as given | `bitmap`, with a header that is consistent with
/// the record count and the bitmap length (so only the records can be damaged).
pub fn raw_pack(info: &FontInfo, recs: &[RawRec], bitmap: &[u8]) -> Vec<u8> {
    raw_pack_with_region(info, recs, bitmap, bitmap.len() as u32)
}

/// As `raw_pack`, but the header declares `bitmap_len` and `bitmap` supplies
/// the (shorter) actual bytes that are emitted (caller truncates the vec).
pub fn raw_pack_with_region(
    info: &FontInfo,
    recs: &[RawRec],
    bitmap: &[u8],
    bitmap_len: u32,
) -> Vec<u8> {
    let n = recs.len();
    let mut v = vec![0u8; HEADER_LEN + RECORD_LEN * n];
    v[0..4].copy_from_slice(b"PFNT");
    put_u16(&mut v, H_VERSION, 1);
    put_u16(&mut v, H_PIXEL_SIZE, info.pixel_size);
    v[H_FONT_ID..H_FONT_ID + 8].copy_from_slice(&info.font_id.to_le_bytes());
    put_u16(&mut v, H_LINE_HEIGHT, info.line_height);
    put_u16(&mut v, H_ASCENT, info.ascent);
    put_u32(&mut v, H_COUNT, n as u32);
    put_u32(&mut v, H_INDEX_OFFSET, 44);
    put_u32(&mut v, H_INDEX_LEN, 22 * n as u32);
    put_u32(&mut v, H_BITMAP_OFFSET, 44 + 22 * n as u32);
    put_u32(&mut v, H_BITMAP_LEN, bitmap_len);
    put_u32(&mut v, H_TOTAL_LEN, 44 + 22 * n as u32 + bitmap_len);
    for (i, r) in recs.iter().enumerate() {
        put_u32(&mut v, rec(i, R_CODEPOINT), r.cp);
        put_u32(&mut v, rec(i, R_BITMAP_OFFSET), r.off);
        put_u32(&mut v, rec(i, R_BITMAP_LEN), r.len);
        put_u16(&mut v, rec(i, R_ADVANCE), r.m.advance);
        put_u16(&mut v, rec(i, R_OFFSET_X), r.m.offset_x as u16);
        put_u16(&mut v, rec(i, R_OFFSET_Y), r.m.offset_y as u16);
        put_u16(&mut v, rec(i, R_WIDTH), r.m.width);
        put_u16(&mut v, rec(i, R_HEIGHT), r.m.height);
    }
    v.extend_from_slice(bitmap);
    v
}

// ----------------------------------------------------------------------- LCG

/// 64-bit linear congruential generator (Knuth MMIX constants); the high bits
/// are the output.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        // n > 0; modulo bias is irrelevant for test inputs
        self.next_u32() % n
    }
    pub fn chance(&mut self, one_in: u32) -> bool {
        self.below(one_in) == 0
    }
    /// Biased towards the edge values a parser trips on.
    pub fn edgy_u32(&mut self) -> u32 {
        const EDGE: [u32; 16] = [
            0,
            1,
            2,
            21,
            22,
            23,
            0xD7FF,
            0xD800,
            0xDFFF,
            0xE000,
            0xFFFF,
            0x1_0000,
            0x10_FFFF,
            0x11_0000,
            0x7FFF_FFFF,
            u32::MAX,
        ];
        if self.chance(2) {
            EDGE[self.below(16) as usize]
        } else {
            self.next_u32()
        }
    }
    pub fn any_char(&mut self) -> char {
        loop {
            let v = if self.chance(3) {
                self.below(0x80)
            } else if self.chance(2) {
                self.below(0x11_0000)
            } else {
                self.edgy_u32() % 0x11_0000
            };
            if let Some(c) = char::from_u32(v) {
                return c;
            }
        }
    }
}

//! Minimal JPEG decoder producing 1-bit Floyd–Steinberg dithered bitmaps.
//!
//! Baseline (SOF0) streams MCU-row-by-row via 4 KB chunked reads; peak RAM
//! ≈ 30 KB. Luminance (Y) channel only — chrominance is Huffman-decoded to
//! advance the bitstream, then discarded.
//!
//! Progressive (SOF2) needs the `progressive-jpeg` feature, off by default;
//! without it such a file is refused. It cannot be decoded in one pass without
//! holding every coefficient of the image, which does not fit the heap. Instead
//! a band of `K` MCU rows of Y coefficients is held at a time and the whole bitstream
//! is re-read from the start once per band (`JpegRead::rewind`). A band
//! budget of [`MAX_BAND_BYTES`] shrinks to whatever the heap allows. When
//! one band covers the image the decode is exact. Otherwise Y AC refinement
//! scans are skipped, because parsing them needs the nonzero map of blocks
//! outside the band; the last bit(s) of AC precision is lost, which is
//! invisible after 1-bit dithering. Chroma scans are never decoded.
//!
//! Output is packed 1-bit MSB-first, row-major — see [`DecodedImage`](crate::DecodedImage).

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::DecodedImage;
use crate::fit::Fit;

// JPEG marker bytes

const M_SOF0: u8 = 0xC0;
const M_SOF2: u8 = 0xC2;
const M_DHT: u8 = 0xC4;
const M_TEM: u8 = 0x01;
const M_SOI: u8 = 0xD8;
const M_EOI: u8 = 0xD9;
const M_SOS: u8 = 0xDA;
const M_DQT: u8 = 0xDB;
const M_DRI: u8 = 0xDD;
const M_RST0: u8 = 0xD0;
const M_RST7: u8 = 0xD7;

// limits

const MAX_COMP: usize = 4;
const MAX_PIXELS: u32 = 2048 * 2048;

// header bytes to read for marker parsing; large APP/EXIF segments skipped by length
const HEADER_READ: usize = 32768;

// chunk size for streaming reads during MCU decode
const CHUNK_SIZE: usize = 4096;

// DEFLATE sliding-window size for streaming ZIP decompression
const DEFLATE_WINDOW: usize = 32768;

// zig-zag scan order

#[rustfmt::skip]
const ZZ: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

// IDCT constants (IJG ISLOW, CONST_BITS = 13)

const CB: i32 = 13;
const P1: i32 = 2;
const F0298: i32 = 2446;
const F0390: i32 = 3196;
const F0541: i32 = 4433;
const F0765: i32 = 6270;
const F0899: i32 = 7373;
const F1175: i32 = 9633;
const F1501: i32 = 12299;
const F1847: i32 = 15137;
const F1961: i32 = 16069;
const F2053: i32 = 16819;
const F2562: i32 = 20995;
const F3072: i32 = 25172;

// types

#[derive(Clone, Copy, Default)]
struct Component {
    id: u8,
    h_samp: u8,
    v_samp: u8,
    qt_idx: u8,
    dc_tbl: u8,
    ac_tbl: u8,
}

struct HuffTable {
    lut: [(u8, u8); 256],
    mincode: [i32; 17],
    maxcode: [i32; 17],
    valptr: [usize; 17],
    values: [u8; 256],
}

struct JpegState {
    width: u16,
    height: u16,
    num_comp: u8,
    comp: [Component; MAX_COMP],
    max_h: u8,
    max_v: u8,
    qt: [[u16; 64]; 4],
    qt_ok: [bool; 4],
    dc_huff: [HuffTable; 4],
    ac_huff: [HuffTable; 4],
    dc_ok: [bool; 4],
    ac_ok: [bool; 4],
    restart_interval: u16,
    scan_num_comp: u8,
    scan_order: [u8; MAX_COMP],
    progressive: bool,
    // current-scan spectral selection start (0 = DC)
    scan_ss: u8,
    // current-scan spectral selection end (0 = DC only, 63 = all AC)
    scan_se: u8,
    // current-scan successive approximation high bit (0 = first scan of the band)
    scan_ah: u8,
    // current-scan successive approximation low bit (point transform)
    scan_al: u8,
}

impl JpegState {
    fn heap_new() -> Result<Box<Self>, &'static str> {
        let layout = core::alloc::Layout::new::<Self>();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            return Err("jpeg: OOM for decoder state");
        }
        let mut st = unsafe { Box::from_raw(ptr as *mut Self) };
        st.max_h = 1;
        st.max_v = 1;
        for ht in st.dc_huff.iter_mut().chain(st.ac_huff.iter_mut()) {
            ht.maxcode.fill(-1);
        }
        Ok(st)
    }
}

// byte source trait + implementations

// raw byte source for the JPEG bitstream
trait JpegRead {
    fn read_byte(&mut self) -> Result<u8, &'static str>;
    fn is_eof(&self) -> bool;
    // seek back to the first byte of the JPEG (the SOI marker)
    #[cfg_attr(not(feature = "progressive-jpeg"), allow(dead_code))]
    fn rewind(&mut self) -> Result<(), &'static str>;
}

// reads from an in-memory slice
struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl JpegRead for SliceReader<'_> {
    #[inline]
    fn read_byte(&mut self) -> Result<u8, &'static str> {
        if self.pos >= self.data.len() {
            return Err("jpeg: unexpected end of data");
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    #[inline]
    fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn rewind(&mut self) -> Result<(), &'static str> {
        self.pos = 0;
        Ok(())
    }
}

// reads from SD via closure, buffering 4KB chunks
struct ChunkReader<F> {
    read_fn: F,
    #[cfg_attr(not(feature = "progressive-jpeg"), allow(dead_code))]
    start: u32, // absolute offset of the first JPEG byte (for rewind)
    offset: u32, // absolute offset of next byte to fetch
    end: u32,    // end-of-data offset (exclusive)
    buf: [u8; CHUNK_SIZE],
    pos: usize,
    len: usize,
}

impl<F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>> ChunkReader<F> {
    fn new(read_fn: F, start: u32, end: u32) -> Self {
        Self {
            read_fn,
            start,
            offset: start,
            end,
            buf: [0u8; CHUNK_SIZE],
            pos: 0,
            len: 0,
        }
    }

    fn refill(&mut self) -> Result<(), &'static str> {
        if self.offset >= self.end {
            self.len = 0;
            return Ok(());
        }
        let want = CHUNK_SIZE.min((self.end - self.offset) as usize);
        let n = (self.read_fn)(self.offset, &mut self.buf[..want])?;
        if n == 0 {
            self.len = 0;
            return Ok(());
        }
        self.offset += n as u32;
        self.pos = 0;
        self.len = n;
        Ok(())
    }
}

impl<F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>> JpegRead for ChunkReader<F> {
    fn read_byte(&mut self) -> Result<u8, &'static str> {
        if self.pos >= self.len {
            self.refill()?;
            if self.len == 0 {
                return Err("jpeg: unexpected end of data");
            }
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.len && self.offset >= self.end
    }

    fn rewind(&mut self) -> Result<(), &'static str> {
        self.offset = self.start;
        self.pos = 0;
        self.len = 0;
        Ok(())
    }
}

// streaming DEFLATE-from-SD reader; 4KB chunks in, one decompressed byte at a time out.
// peak heap: ~47KB (11KB decompressor + 32KB window + 4KB read buf).
struct DeflateReader<F, S: crate::scratch::ScratchStorage> {
    read_fn: F,
    #[cfg_attr(not(feature = "progressive-jpeg"), allow(dead_code))]
    data_offset: u32, // absolute offset of the entry (for rewind)
    #[cfg_attr(not(feature = "progressive-jpeg"), allow(dead_code))]
    comp_size: usize, // compressed size of the entry (for rewind)
    file_pos: u32,                           // absolute offset of next compressed byte
    comp_left: usize,                        // compressed bytes remaining in ZIP entry
    rbuf: Vec<u8>,                           // compressed-data read buffer
    in_avail: usize,                         // valid bytes in rbuf
    decomp: crate::scratch::Decompressor<S>, // ~11KB
    window: crate::scratch::ScratchBytes<S>, // 32KB circular dictionary
    dict_pos: usize, // write position in window (cumulative, mod DEFLATE_WINDOW)
    read_pos: usize, // next byte to yield from window
    avail: usize,    // decompressed bytes available (dict_pos - read_pos)
    done: bool,      // true once miniz reports Done
}

impl<F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>, S: crate::scratch::ScratchStorage>
    DeflateReader<F, S>
{
    fn new<A>(
        read_fn: F,
        data_offset: u32,
        comp_size: u32,
        scratch: &mut A,
    ) -> Result<Self, &'static str>
    where
        A: FnMut(core::alloc::Layout) -> Result<S, &'static str>,
    {
        let decomp = crate::scratch::Decompressor::new(scratch)?;
        let window = crate::scratch::ScratchBytes::new(
            scratch,
            core::alloc::Layout::array::<u8>(DEFLATE_WINDOW).unwrap(),
        )?;
        let mut rbuf = Vec::new();
        rbuf.try_reserve_exact(CHUNK_SIZE)
            .map_err(|_| "jpeg: OOM for DEFLATE read buffer")?;
        rbuf.resize(CHUNK_SIZE, 0);

        Ok(Self {
            read_fn,
            data_offset,
            comp_size: comp_size as usize,
            file_pos: data_offset,
            comp_left: comp_size as usize,
            rbuf,
            in_avail: 0,
            decomp,
            window,
            dict_pos: 0,
            read_pos: 0,
            avail: 0,
            done: false,
        })
    }

    // decompress more data into the circular window
    fn pump(&mut self) -> Result<(), &'static str> {
        use miniz_oxide::inflate::TINFLStatus;
        use miniz_oxide::inflate::core::{decompress, inflate_flags};

        if self.done {
            return Ok(());
        }

        // top up read buffer from SD
        if self.in_avail < CHUNK_SIZE && self.comp_left > 0 {
            let space = CHUNK_SIZE - self.in_avail;
            let want = space.min(self.comp_left);
            match (self.read_fn)(
                self.file_pos,
                &mut self.rbuf[self.in_avail..self.in_avail + want],
            ) {
                Ok(n) if n > 0 => {
                    self.file_pos += n as u32;
                    self.comp_left -= n;
                    self.in_avail += n;
                }
                Ok(_) => {
                    self.comp_left = 0;
                }
                Err(e) => return Err(e),
            }
        }

        let flags = if self.comp_left > 0 {
            inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
        } else {
            0
        };

        let write_pos = self.dict_pos & (DEFLATE_WINDOW - 1);
        let (status, consumed, produced) = decompress(
            &mut *self.decomp,
            &self.rbuf[..self.in_avail],
            &mut self.window,
            write_pos,
            flags,
        );

        if consumed > 0 && consumed < self.in_avail {
            self.rbuf.copy_within(consumed..self.in_avail, 0);
        }
        self.in_avail -= consumed;

        self.dict_pos += produced;
        self.avail += produced;

        match status {
            TINFLStatus::Done => {
                self.done = true;
            }
            TINFLStatus::HasMoreOutput | TINFLStatus::NeedsMoreInput => {}
            _ => return Err("jpeg: DEFLATE decompression error"),
        }

        Ok(())
    }
}

impl<F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>, S: crate::scratch::ScratchStorage>
    JpegRead for DeflateReader<F, S>
{
    fn read_byte(&mut self) -> Result<u8, &'static str> {
        if self.avail == 0 {
            if self.done {
                return Err("jpeg: unexpected end of DEFLATE stream");
            }
            self.pump()?;
            if self.avail == 0 {
                return Err("jpeg: unexpected end of DEFLATE stream");
            }
        }
        let rp = self.read_pos & (DEFLATE_WINDOW - 1);
        let b = self.window[rp];
        self.read_pos += 1;
        self.avail -= 1;
        Ok(b)
    }

    fn is_eof(&self) -> bool {
        self.avail == 0 && self.done
    }

    // DEFLATE cannot seek: restart the inflater at the first compressed byte.
    // The window holds stale bytes, which miniz overwrites before they are read.
    fn rewind(&mut self) -> Result<(), &'static str> {
        self.decomp.init();
        self.file_pos = self.data_offset;
        self.comp_left = self.comp_size;
        self.in_avail = 0;
        self.dict_pos = 0;
        self.read_pos = 0;
        self.avail = 0;
        self.done = false;
        Ok(())
    }
}

// BitReader: generic over byte source

struct BitReader<R> {
    source: R,
    buf: u32,
    avail: u8,
    marker: u8, // stashed marker byte (non-zero = encountered during next_byte)
}

impl<R: JpegRead> BitReader<R> {
    fn new(source: R) -> Self {
        Self {
            source,
            buf: 0,
            avail: 0,
            marker: 0,
        }
    }

    // fetch next entropy-coded byte, handling JPEG byte stuffing
    fn next_byte(&mut self) -> Result<u8, &'static str> {
        if self.marker != 0 {
            return Ok(0);
        }
        let b = self.source.read_byte()?;
        if b != 0xFF {
            return Ok(b);
        }
        loop {
            if self.source.is_eof() {
                return Ok(0);
            }
            let next = self.source.read_byte()?;
            match next {
                0x00 => return Ok(0xFF),
                0xFF => continue,
                _ => {
                    self.marker = next;
                    return Ok(0);
                }
            }
        }
    }

    fn ensure(&mut self, n: u8) -> Result<(), &'static str> {
        while self.avail < n {
            let b = self.next_byte()?;
            self.buf |= (b as u32) << (24 - self.avail);
            self.avail += 8;
        }
        Ok(())
    }

    #[inline]
    fn peek(&mut self, n: u8) -> Result<u32, &'static str> {
        self.ensure(n)?;
        Ok(self.buf >> (32 - n as u32))
    }

    #[inline]
    fn drop_bits(&mut self, n: u8) {
        self.buf <<= n as u32;
        self.avail -= n;
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> Result<u32, &'static str> {
        if n == 0 {
            return Ok(0);
        }
        self.ensure(n)?;
        let val = self.buf >> (32 - n as u32);
        self.buf <<= n as u32;
        self.avail -= n;
        Ok(val)
    }

    // drop buffered bits and any stashed marker (after the source was rewound)
    #[cfg_attr(not(feature = "progressive-jpeg"), allow(dead_code))]
    fn reset(&mut self) {
        self.buf = 0;
        self.avail = 0;
        self.marker = 0;
    }

    // Advance to the next real marker (neither RSTn nor FF00 stuffing) and
    // return its code. Whatever entropy-coded bytes remain are skipped, so
    // this ends a scan decoded to its last block, skims a scan that is not
    // needed, and finds the next segment between scans. End of data counts
    // as EOI, since the decoder never needs the bytes after the last scan.
    fn next_marker(&mut self) -> Result<u8, &'static str> {
        self.buf = 0;
        self.avail = 0;
        let stashed = core::mem::take(&mut self.marker);
        if stashed != 0 && !(M_RST0..=M_RST7).contains(&stashed) {
            return Ok(stashed);
        }
        loop {
            if self.source.is_eof() {
                return Ok(M_EOI);
            }
            if self.source.read_byte()? != 0xFF {
                continue;
            }
            loop {
                if self.source.is_eof() {
                    return Ok(M_EOI);
                }
                match self.source.read_byte()? {
                    0xFF => continue,
                    0x00 | M_RST0..=M_RST7 => break,
                    m => return Ok(m),
                }
            }
        }
    }

    // Read one marker segment. The buffer keeps the 2-byte length at the
    // front, so the slice parsers below run on it unchanged.
    fn read_segment(&mut self) -> Result<Vec<u8>, &'static str> {
        let hi = self.source.read_byte()?;
        let lo = self.source.read_byte()?;
        let seg = u16::from_be_bytes([hi, lo]) as usize;
        if seg < 2 {
            return Err("jpeg: bad marker length");
        }
        let mut data = Vec::new();
        data.try_reserve_exact(seg)
            .map_err(|_| "jpeg: OOM for marker segment")?;
        data.extend_from_slice(&[hi, lo]);
        for _ in 2..seg {
            data.push(self.source.read_byte()?);
        }
        Ok(data)
    }

    fn skip_segment(&mut self) -> Result<(), &'static str> {
        let hi = self.source.read_byte()?;
        let lo = self.source.read_byte()?;
        let seg = u16::from_be_bytes([hi, lo]) as usize;
        if seg < 2 {
            return Err("jpeg: bad marker length");
        }
        for _ in 2..seg {
            self.source.read_byte()?;
        }
        Ok(())
    }

    // discard remaining bits, advance past the next restart marker
    fn consume_restart(&mut self) -> Result<(), &'static str> {
        self.buf = 0;
        self.avail = 0;

        // if next_byte already stashed a marker, check it now
        if self.marker != 0 {
            let m = self.marker;
            self.marker = 0;
            if m >= M_RST0 && m <= M_RST7 {
                return Ok(());
            }
            // non-RST marker; keep going
            return Ok(());
        }

        // scan forward for the restart marker
        loop {
            if self.source.is_eof() {
                return Ok(());
            }
            let b = self.source.read_byte()?;
            if b != 0xFF {
                continue;
            }
            loop {
                if self.source.is_eof() {
                    return Ok(());
                }
                let m = self.source.read_byte()?;
                match m {
                    0xFF => continue,
                    0x00 => break,
                    M_RST0..=M_RST7 => return Ok(()),
                    _ => return Ok(()),
                }
            }
        }
    }
}

// public API

// parse the header, then decode with the strategy the frame type needs
fn decode_jpeg_from<R, B, A>(
    reader: R,
    max_w: u16,
    max_h: u16,
    allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    R: JpegRead,
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    let mut bit = BitReader::new(reader);
    let st = parse_header(&mut bit)?;
    if st.progressive {
        decode_progressive(st, bit, max_w, max_h, allocate)
    } else {
        validate_tables(&st)?;
        decode_baseline(&st, bit, max_w, max_h, allocate)
    }
}

/// Decode a JPEG (baseline or progressive) from an in-memory buffer to a
/// 1-bit dithered bitmap.
///
/// The image is scaled down, keeping its aspect ratio, so the result fits
/// within `max_w` × `max_h` pixels and fills them along one axis; it is never
/// scaled up.
pub fn decode_jpeg_fit(data: &[u8], max_w: u16, max_h: u16) -> Result<DecodedImage, &'static str> {
    decode_jpeg_fit_with_buffer(data, max_w, max_h, |len| {
        crate::image_buffer(len).map_err(|_| "jpeg: OOM for output")
    })
}

/// Decode into caller-owned packed bitmap storage.
///
/// `allocate` receives the exact output byte length after scaling. The returned
/// buffer must have that length; its previous contents are cleared.
pub fn decode_jpeg_fit_with_buffer<B, A>(
    data: &[u8],
    max_w: u16,
    max_h: u16,
    allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    decode_jpeg_from(SliceReader::new(data), max_w, max_h, allocate)
}

/// Decode a JPEG from a **stored** (uncompressed) ZIP entry by streaming
/// 4 KB chunks through `read_fn`.
///
/// `read_fn(offset, buf)` reads bytes at the given absolute offset and
/// returns the number of bytes actually read. A progressive JPEG is read
/// once per coefficient band, see the module docs.
pub fn decode_jpeg_streaming<F>(
    read_fn: F,
    data_offset: u32,
    data_size: u32,
    max_w: u16,
    max_h: u16,
) -> Result<DecodedImage, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
{
    decode_jpeg_streaming_with_buffer(read_fn, data_offset, data_size, max_w, max_h, |len| {
        crate::image_buffer(len).map_err(|_| "jpeg: OOM for output")
    })
}

/// Stream a stored JPEG into caller-owned storage; see [`decode_jpeg_fit_with_buffer`].
pub fn decode_jpeg_streaming_with_buffer<F, B, A>(
    read_fn: F,
    data_offset: u32,
    data_size: u32,
    max_w: u16,
    max_h: u16,
    allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    let end = data_offset + data_size;
    let reader = ChunkReader::new(read_fn, data_offset, end);
    decode_jpeg_from(reader, max_w, max_h, allocate)
}

/// Decode a JPEG from a **DEFLATE-compressed** ZIP entry by streaming
/// reads through `read_fn`.
///
/// Both ZIP decompression and MCU decode are streamed concurrently,
/// so the full entry is never held in memory. Peak heap ≈ 79 KB for
/// baseline; a progressive JPEG adds its coefficient band and inflates the
/// entry again for every band.
pub fn decode_jpeg_deflate_streaming<F>(
    read_fn: F,
    data_offset: u32,
    comp_size: u32,
    max_w: u16,
    max_h: u16,
) -> Result<DecodedImage, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
{
    decode_jpeg_deflate_streaming_with_buffer(
        read_fn,
        data_offset,
        comp_size,
        max_w,
        max_h,
        |len| crate::image_buffer(len).map_err(|_| "jpeg: OOM for output"),
    )
}

/// Stream a DEFLATE JPEG into caller-owned storage; see [`decode_jpeg_fit_with_buffer`].
pub fn decode_jpeg_deflate_streaming_with_buffer<F, B, A>(
    read_fn: F,
    data_offset: u32,
    comp_size: u32,
    max_w: u16,
    max_h: u16,
    allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    decode_jpeg_deflate_streaming_with_scratch(
        read_fn,
        data_offset,
        comp_size,
        max_w,
        max_h,
        allocate,
        crate::scratch::HeapScratch::zeroed,
    )
}

/// Stream a DEFLATE JPEG with caller-selected layout-aware inflate scratch.
pub fn decode_jpeg_deflate_streaming_with_scratch<F, B, A, S, C>(
    read_fn: F,
    data_offset: u32,
    comp_size: u32,
    max_w: u16,
    max_h: u16,
    allocate: A,
    mut scratch: C,
) -> Result<DecodedImage<B>, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
    S: crate::scratch::ScratchStorage,
    C: FnMut(core::alloc::Layout) -> Result<S, &'static str>,
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    let deflate = DeflateReader::new(read_fn, data_offset, comp_size, &mut scratch)?;
    decode_jpeg_from(deflate, max_w, max_h, allocate)
}

// ── dimension peek (no decode) ─────────────────────────────────────

/// Read the dimensions of a JPEG from an in-memory buffer without decoding.
///
/// Scans markers until SOF0/SOF2 and returns `(width, height)` in source
/// pixels.  No heap allocation — only a linear scan of the header bytes.
pub fn peek_jpeg_dimensions(data: &[u8]) -> Result<(u16, u16), &'static str> {
    scan_sof_dimensions(data)
}

/// Read the dimensions of a JPEG from a **stored** (uncompressed) ZIP entry
/// by streaming reads via `read_fn`.
///
/// Reads up to 32 KB of header data (enough to skip past EXIF/APP segments
/// and reach the SOF marker), then scans for SOF0/SOF2 to extract
/// `(width, height)`.  The header buffer is heap-allocated but no decoder
/// state (~4.5 KB) is created.
pub fn peek_jpeg_dimensions_streaming<F>(
    mut read_fn: F,
    data_offset: u32,
    data_size: u32,
) -> Result<(u16, u16), &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, &'static str>,
{
    let hdr_size = HEADER_READ.min(data_size as usize);
    let mut hdr = Vec::new();
    hdr.try_reserve_exact(hdr_size)
        .map_err(|_| "jpeg: OOM for header peek")?;
    hdr.resize(hdr_size, 0);
    let n = read_fn(data_offset, &mut hdr)?;
    hdr.truncate(n);
    scan_sof_dimensions(&hdr)
}

/// Lightweight SOF scanner: finds the first SOF0 (0xC0) or SOF2 (0xC2)
/// marker and extracts (width, height) without building full decoder state.
fn scan_sof_dimensions(data: &[u8]) -> Result<(u16, u16), &'static str> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != M_SOI {
        return Err("jpeg: invalid signature");
    }
    let mut pos = 2usize;
    let len = data.len();

    loop {
        // skip to next 0xFF
        while pos < len && data[pos] != 0xFF {
            pos += 1;
        }
        // skip padding 0xFF bytes
        while pos < len && data[pos] == 0xFF {
            pos += 1;
        }
        if pos >= len {
            return Err("jpeg: truncated (no SOF found)");
        }
        let marker = data[pos];
        pos += 1;

        match marker {
            0x00 | M_RST0..=M_RST7 => continue,
            M_SOF0 | M_SOF2 => {
                // SOF segment: 2-byte length, 1-byte precision, 2-byte height, 2-byte width
                if pos + 2 + 5 > len {
                    return Err("jpeg: SOF truncated");
                }
                // skip segment length
                let p = pos + 2;
                // data[p] = precision (must be 8)
                let height = be_u16(data, p + 1);
                let width = be_u16(data, p + 3);
                if width == 0 || height == 0 {
                    return Err("jpeg: zero dimensions");
                }
                return Ok((width, height));
            }
            // unsupported SOF variants
            0xC1 | 0xC3 | 0xC5..=0xCB | 0xCD..=0xCF => {
                return Err("jpeg: unsupported SOF variant");
            }
            M_SOS => return Err("jpeg: SOS before SOF"),
            M_EOI => return Err("jpeg: EOI before SOF"),
            _ => {
                // skip unknown marker by its 2-byte length field
                if pos + 2 > len {
                    return Err("jpeg: truncated marker");
                }
                let seg = be_u16(data, pos) as usize;
                if seg < 2 || pos + seg > len {
                    return Err("jpeg: bad marker length");
                }
                pos += seg;
            }
        }
    }
}

// ── decode core (generic over byte source) ─────────────────────────

// tables the current scan reads: DC for a DC scan, AC for an AC scan
fn validate_tables(st: &JpegState) -> Result<(), &'static str> {
    for sci in 0..st.scan_num_comp as usize {
        let ci = st.scan_order[sci] as usize;
        let c = &st.comp[ci];
        if c.qt_idx > 3 || !st.qt_ok[c.qt_idx as usize] {
            return Err("jpeg: missing quant table");
        }
        if st.scan_ss == 0 && !st.dc_ok[c.dc_tbl as usize] {
            return Err("jpeg: missing DC Huffman table");
        }
        if st.scan_se > 0 && !st.ac_ok[c.ac_tbl as usize] {
            return Err("jpeg: missing AC Huffman table");
        }
    }
    Ok(())
}

// output geometry shared by the baseline and progressive decoders
struct Plan {
    w: usize,
    h: usize,
    fit: Fit,
    out_w: usize,
    out_h: usize,
    out_stride: usize,
    mcu_w: usize,
    mcu_h: usize,
    mcus_x: usize,
    mcus_y: usize,
    row_w: usize,
}

impl Plan {
    fn new(st: &JpegState, max_w: u16, max_h: u16) -> Result<Self, &'static str> {
        if max_w == 0 || max_h == 0 {
            return Err("jpeg: zero output bounds");
        }
        let w = st.width as usize;
        let h = st.height as usize;
        if w == 0 || h == 0 {
            return Err("jpeg: zero dimensions");
        }
        if (w as u32).saturating_mul(h as u32) > MAX_PIXELS {
            return Err("jpeg: exceeds pixel limit");
        }

        let fit = Fit::new(w, h, max_w as usize, max_h as usize);
        let (out_w, out_h) = (fit.out_w, fit.out_h);

        let mcu_w = st.max_h as usize * 8;
        let mcu_h = st.max_v as usize * 8;
        let mcus_x = w.div_ceil(mcu_w);
        let mcus_y = h.div_ceil(mcu_h);
        Ok(Self {
            w,
            h,
            fit,
            out_w,
            out_h,
            out_stride: out_w.div_ceil(8),
            mcu_w,
            mcu_h,
            mcus_x,
            mcus_y,
            row_w: mcus_x * mcu_w,
        })
    }
}

// One MCU row of luma awaiting dither, and the 1-bit output it feeds.
// Rows must be flushed top to bottom: the error diffusion carries over.
struct Canvas<'a> {
    y_row: Vec<u8>,
    err_cur: Vec<i16>,
    err_nxt: Vec<i16>,
    pixels: &'a mut [u8],
    out_y: usize,
}

impl<'a> Canvas<'a> {
    fn new(plan: &Plan, pixels: &'a mut [u8]) -> Result<Self, &'static str> {
        if pixels.len() != plan.out_stride * plan.out_h {
            return Err("jpeg: output buffer size mismatch");
        }
        pixels.fill(0);

        let mut y_row = Vec::new();
        y_row
            .try_reserve_exact(plan.row_w * plan.mcu_h)
            .map_err(|_| "jpeg: OOM for y_row")?;
        y_row.resize(plan.row_w * plan.mcu_h, 128u8);
        let mut err_cur = Vec::new();
        err_cur
            .try_reserve_exact(plan.out_w + 2)
            .map_err(|_| "jpeg: OOM for dither")?;
        err_cur.resize(plan.out_w + 2, 0i16);
        let mut err_nxt = Vec::new();
        err_nxt
            .try_reserve_exact(plan.out_w + 2)
            .map_err(|_| "jpeg: OOM for dither")?;
        err_nxt.resize(plan.out_w + 2, 0i16);

        Ok(Self {
            y_row,
            err_cur,
            err_nxt,
            pixels,
            out_y: 0,
        })
    }

    // dither the output rows that MCU row `mcu_row` of `y_row` contains
    fn flush(&mut self, plan: &Plan, mcu_row: usize) {
        for py in 0..plan.mcu_h {
            let src_y = mcu_row * plan.mcu_h + py;
            if src_y >= plan.h || self.out_y >= plan.out_h {
                break;
            }
            if src_y != plan.fit.src_y(self.out_y) {
                continue;
            }
            let row_off = py * plan.row_w;
            let out_row =
                &mut self.pixels[self.out_y * plan.out_stride..(self.out_y + 1) * plan.out_stride];
            dither_row_grey(
                &self.y_row[row_off..],
                &plan.fit,
                plan.out_w,
                &mut self.err_cur,
                &mut self.err_nxt,
                out_row,
            );
            self.out_y += 1;
            core::mem::swap(&mut self.err_cur, &mut self.err_nxt);
            self.err_nxt.fill(0);
        }
    }
}

fn decode_baseline<R: JpegRead, B, A>(
    st: &JpegState,
    mut reader: BitReader<R>,
    max_w: u16,
    max_h: u16,
    allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    let plan = Plan::new(st, max_w, max_h)?;
    log::info!(
        "jpeg: baseline {}x{} → {}x{}",
        plan.w,
        plan.h,
        plan.out_w,
        plan.out_h
    );

    let mut output = allocate(plan.out_stride * plan.out_h)?;
    let mut canvas = Canvas::new(&plan, output.as_mut())?;

    let mut dc_pred = [0i32; MAX_COMP];
    let mut block = [0i32; 64];
    let mut pix = [0u8; 64];
    let mut mcu_cnt: u32 = 0;
    let total_mcus = (plan.mcus_x * plan.mcus_y) as u32;

    // MCU decode loop

    for mcu_row in 0..plan.mcus_y {
        canvas.y_row.fill(128);

        for mcu_col in 0..plan.mcus_x {
            for sci in 0..st.scan_num_comp as usize {
                let ci = st.scan_order[sci] as usize;
                let c = &st.comp[ci];
                let is_y = ci == 0;

                for bv in 0..c.v_samp as usize {
                    for bh in 0..c.h_samp as usize {
                        if is_y {
                            decode_block(
                                &mut reader,
                                &st.dc_huff[c.dc_tbl as usize],
                                &st.ac_huff[c.ac_tbl as usize],
                                &mut dc_pred[ci],
                                &st.qt[c.qt_idx as usize],
                                &mut block,
                                st.scan_se as usize,
                                st.scan_al,
                            )?;
                            idct(&block, &mut pix);
                            put_block(&mut canvas.y_row, &plan, mcu_col, bh, bv, &pix);
                        } else {
                            skip_block(
                                &mut reader,
                                &st.dc_huff[c.dc_tbl as usize],
                                &st.ac_huff[c.ac_tbl as usize],
                                &mut dc_pred[ci],
                                st.scan_se as usize,
                            )?;
                        }
                    }
                }
            }

            mcu_cnt += 1;

            if st.restart_interval > 0
                && mcu_cnt % st.restart_interval as u32 == 0
                && mcu_cnt < total_mcus
            {
                reader.consume_restart()?;
                dc_pred.fill(0);
            }
        }

        canvas.flush(&plan, mcu_row);
    }

    let out_y = canvas.out_y;
    drop(canvas);
    Ok(DecodedImage {
        width: plan.out_w as u16,
        height: out_y as u16,
        data: output,
        stride: plan.out_stride,
    })
}

// copy one IDCT'd luma block into the MCU row buffer
#[inline]
fn put_block(y_row: &mut [u8], plan: &Plan, mcu_col: usize, bh: usize, bv: usize, pix: &[u8; 64]) {
    let bx = mcu_col * plan.mcu_w + bh * 8;
    for r in 0..8 {
        let dst = (bv * 8 + r) * plan.row_w + bx;
        y_row[dst..dst + 8].copy_from_slice(&pix[r * 8..r * 8 + 8]);
    }
}

#[cfg(feature = "progressive-jpeg")]
mod progressive {
    use super::*;

    // ── progressive decode ─────────────────────────────────────────────

    // Budget for the band of Y coefficients (i16, quantized). It is a target:
    // decode_progressive halves the band height until the reservation fits. A
    // bigger band means fewer passes over the entropy stream.
    const MAX_BAND_BYTES: usize = 48 * 1024;

    // offset of a Y block inside the band, in i16 units
    #[inline]
    fn band_block_offset(
        my: usize,
        mx: usize,
        bidx: usize,
        mcus_x: usize,
        y_blocks_per_mcu: usize,
    ) -> usize {
        (((my * mcus_x) + mx) * y_blocks_per_mcu + bidx) * 64
    }

    // DC first scan: DC differential, scaled by Al, stored at coefficient 0
    fn pdc_first<R: JpegRead>(
        r: &mut BitReader<R>,
        dc_ht: &HuffTable,
        dc_pred: &mut i32,
        al: u8,
        blk: &mut [i16],
    ) -> Result<(), &'static str> {
        let dc_size = huff_decode(r, dc_ht)?;
        if dc_size > 0 {
            if dc_size > 11 {
                return Err("jpeg: DC size > 11");
            }
            let bits = r.read_bits(dc_size)?;
            *dc_pred += extend(bits, dc_size);
        }
        blk[0] = (*dc_pred << al) as i16;
        Ok(())
    }

    // DC refinement: one bit per block, OR'd in at bit Al whatever the block holds
    fn pdc_refine<R: JpegRead>(
        r: &mut BitReader<R>,
        al: u8,
        blk: &mut [i16],
    ) -> Result<(), &'static str> {
        if r.read_bits(1)? != 0 {
            blk[0] |= 1i16 << al;
        }
        Ok(())
    }

    // read the EOBn run length that follows an EOB symbol with `run` extra bits
    fn read_eobrun<R: JpegRead>(r: &mut BitReader<R>, run: u8) -> Result<u16, &'static str> {
        Ok(((1u32 << run) + r.read_bits(run)?) as u16)
    }

    // AC first scan: run/size symbols over [ss..=se], scaled by Al; an EOBn
    // symbol ends this block and the next n-1 blocks of the scan
    fn pac_first<R: JpegRead>(
        r: &mut BitReader<R>,
        ac_ht: &HuffTable,
        ss: usize,
        se: usize,
        al: u8,
        eobrun: &mut u16,
        blk: &mut [i16],
    ) -> Result<(), &'static str> {
        if *eobrun > 0 {
            *eobrun -= 1;
            return Ok(());
        }
        let mut k = ss;
        while k <= se {
            let sym = huff_decode(r, ac_ht)?;
            let run = sym >> 4;
            let size = sym & 0x0F;
            if size != 0 {
                k += run as usize;
                if k > se {
                    return Err("jpeg: AC index overflow");
                }
                let bits = r.read_bits(size)?;
                blk[ZZ[k]] = (extend(bits, size) << al) as i16;
                k += 1;
            } else if run == 15 {
                k += 16;
            } else {
                *eobrun = read_eobrun(r, run)? - 1;
                break;
            }
        }
        Ok(())
    }

    // append the correction bit to an already-nonzero coefficient
    #[inline]
    fn refine_bit<R: JpegRead>(
        r: &mut BitReader<R>,
        coef: &mut i16,
        p1: i16,
        m1: i16,
    ) -> Result<(), &'static str> {
        if r.read_bits(1)? != 0 && *coef & p1 == 0 {
            *coef += if *coef >= 0 { p1 } else { m1 };
        }
        Ok(())
    }

    // AC refinement scan. Every already-nonzero coefficient in [ss..=se] costs
    // one correction bit, so parsing needs the exact coefficients of earlier
    // scans for this block (IJG jdphuff.c decode_mcu_AC_refine).
    fn pac_refine<R: JpegRead>(
        r: &mut BitReader<R>,
        ac_ht: &HuffTable,
        ss: usize,
        se: usize,
        al: u8,
        eobrun: &mut u16,
        blk: &mut [i16],
    ) -> Result<(), &'static str> {
        let p1: i16 = 1 << al;
        let m1: i16 = -1 << al;
        let mut k = ss;

        if *eobrun == 0 {
            while k <= se {
                let sym = huff_decode(r, ac_ht)?;
                let mut run = sym >> 4;
                let size = sym & 0x0F;
                let mut new_val = 0i16;
                if size != 0 {
                    if size != 1 {
                        return Err("jpeg: bad refinement size");
                    }
                    new_val = if r.read_bits(1)? != 0 { p1 } else { m1 };
                } else if run != 15 {
                    *eobrun = read_eobrun(r, run)?;
                    break;
                }
                // pass `run` zero coefficients (refining the nonzero ones on
                // the way); the next zero takes `new_val`. ZRL places nothing.
                while k <= se {
                    let coef = &mut blk[ZZ[k]];
                    if *coef != 0 {
                        refine_bit(r, coef, p1, m1)?;
                    } else if run == 0 {
                        break;
                    } else {
                        run -= 1;
                    }
                    k += 1;
                }
                if new_val != 0 {
                    if k > se {
                        return Err("jpeg: AC refinement overflow");
                    }
                    blk[ZZ[k]] = new_val;
                }
                k += 1;
            }
        }

        if *eobrun > 0 {
            // the rest of this block only gets correction bits
            while k <= se {
                let coef = &mut blk[ZZ[k]];
                if *coef != 0 {
                    refine_bit(r, coef, p1, m1)?;
                }
                k += 1;
            }
            *eobrun -= 1;
        }
        Ok(())
    }

    // decode one block of the current scan into `blk`
    fn scan_block<R: JpegRead>(
        r: &mut BitReader<R>,
        st: &JpegState,
        ci: usize,
        dc_pred: &mut i32,
        eobrun: &mut u16,
        blk: &mut [i16],
    ) -> Result<(), &'static str> {
        let c = &st.comp[ci];
        let (ss, se) = (st.scan_ss as usize, st.scan_se as usize);
        let (ah, al) = (st.scan_ah, st.scan_al);
        if ss == 0 {
            if ah == 0 {
                pdc_first(r, &st.dc_huff[c.dc_tbl as usize], dc_pred, al, blk)
            } else {
                pdc_refine(r, al, blk)
            }
        } else {
            let ac_ht = &st.ac_huff[c.ac_tbl as usize];
            if ah == 0 {
                pac_first(r, ac_ht, ss, se, al, eobrun, blk)
            } else {
                pac_refine(r, ac_ht, ss, se, al, eobrun, blk)
            }
        }
    }

    // Decode the scan whose header was just parsed, keeping the Y blocks of MCU
    // rows top..bottom in `band`. Blocks outside the band, and chroma blocks of
    // an interleaved scan, are parsed into a scratch block to stay in sync.
    // Stops after the band's last row; the caller skims the remainder.
    fn walk_scan<R: JpegRead>(
        st: &JpegState,
        bit: &mut BitReader<R>,
        plan: &Plan,
        band: &mut [i16],
        top: usize,
        bottom: usize,
        whole: bool,
    ) -> Result<(), &'static str> {
        let n = st.scan_num_comp as usize;
        let (ss, se) = (st.scan_ss as usize, st.scan_se as usize);
        if ss > se || (ss == 0 && se != 0) || (ss > 0 && n != 1) || st.scan_al > 13 {
            return Err("jpeg: bad progressive scan");
        }
        if !st.scan_order[..n].contains(&0) {
            return Ok(()); // chroma only
        }
        // The nonzero map of every block is needed to parse a refinement scan
        // and only the band's blocks are kept, so these are skipped.
        if st.scan_ah > 0 && ss > 0 && !whole {
            return Ok(());
        }
        validate_tables(st)?;

        let y = st.comp[0];
        let (yh, yv) = (y.h_samp as usize, y.v_samp as usize);
        let y_bpm = yh * yv;
        let restart = st.restart_interval as u32;
        let mut dc_pred = [0i32; MAX_COMP];
        let mut eobrun: u16 = 0;
        let mut scratch = [0i16; 64];
        let mut unit: u32 = 0;

        if n > 1 {
            // interleaved: MCUs in raster order, every component's blocks in each
            let total = (plan.mcus_x * plan.mcus_y) as u32;
            for my in 0..bottom {
                for mx in 0..plan.mcus_x {
                    for sci in 0..n {
                        let ci = st.scan_order[sci] as usize;
                        let c = &st.comp[ci];
                        for bv in 0..c.v_samp as usize {
                            for bh in 0..c.h_samp as usize {
                                let blk = if ci == 0 && my >= top {
                                    let off = band_block_offset(
                                        my - top,
                                        mx,
                                        bv * yh + bh,
                                        plan.mcus_x,
                                        y_bpm,
                                    );
                                    &mut band[off..off + 64]
                                } else {
                                    &mut scratch[..]
                                };
                                scan_block(bit, st, ci, &mut dc_pred[ci], &mut eobrun, blk)?;
                            }
                        }
                    }
                    unit += 1;
                    if restart > 0 && unit % restart == 0 && unit < total {
                        bit.consume_restart()?;
                        dc_pred.fill(0);
                        eobrun = 0;
                    }
                }
            }
            return Ok(());
        }

        // Non-interleaved: blocks of the one component in raster order over its
        // own ceil(samples / 8) grid, not the MCU-padded one (JPEG A.2.2).
        let comp_w = (plan.w * yh).div_ceil(st.max_h as usize);
        let comp_h = (plan.h * yv).div_ceil(st.max_v as usize);
        let (blocks_x, blocks_y) = (comp_w.div_ceil(8), comp_h.div_ceil(8));
        let total = (blocks_x * blocks_y) as u32;
        for cy in 0..blocks_y {
            let my = cy / yv;
            if my >= bottom {
                break;
            }
            for cx in 0..blocks_x {
                let blk = if my >= top {
                    let off = band_block_offset(
                        my - top,
                        cx / yh,
                        (cy % yv) * yh + cx % yh,
                        plan.mcus_x,
                        y_bpm,
                    );
                    &mut band[off..off + 64]
                } else {
                    &mut scratch[..]
                };
                scan_block(bit, st, 0, &mut dc_pred[0], &mut eobrun, blk)?;
                unit += 1;
                if restart > 0 && unit % restart == 0 && unit < total {
                    bit.consume_restart()?;
                    dc_pred[0] = 0;
                    eobrun = 0;
                }
            }
        }
        Ok(())
    }

    // One walk through the file, collecting the Y coefficients of MCU rows
    // top..bottom. Tables are re-read from the start, so each pass sees the
    // Huffman and quantization tables exactly as the encoder redefined them.
    fn run_pass<R: JpegRead>(
        st: &mut JpegState,
        bit: &mut BitReader<R>,
        plan: &Plan,
        band: &mut [i16],
        top: usize,
        bottom: usize,
        whole: bool,
    ) -> Result<(), &'static str> {
        bit.source.rewind()?;
        bit.reset();
        read_soi(bit)?;
        loop {
            match bit.next_marker()? {
                M_SOS => {
                    parse_sos(&bit.read_segment()?, &mut 0, st)?;
                    walk_scan(st, bit, plan, band, top, bottom, whole)?;
                }
                M_EOI => return Ok(()),
                m => parse_segment(bit, st, m)?,
            }
        }
    }

    // An 8-bit block's coefficients stay within ±1024 (11 bits) before
    // quantization; the IDCT's sums only fit i32 for inputs near that range.
    const COEF_LIMIT: i32 = 4096;

    // dequantize one stored block and run the IDCT
    fn dequant_idct(blk: &[i16], qt: &[u16; 64], out: &mut [u8; 64]) {
        let mut deq = [0i32; 64];
        for k in 0..64 {
            // i16 * u16 always fits i32; damaged data is clamped, not wrapped
            deq[ZZ[k]] = (blk[ZZ[k]] as i32 * qt[k] as i32).clamp(-COEF_LIMIT, COEF_LIMIT);
        }
        idct(&deq, out);
    }

    pub(super) fn decode_progressive<R: JpegRead, B, A>(
        mut st: Box<JpegState>,
        mut bit: BitReader<R>,
        max_w: u16,
        max_h: u16,
        allocate: A,
    ) -> Result<DecodedImage<B>, &'static str>
    where
        B: AsMut<[u8]>,
        A: FnOnce(usize) -> Result<B, &'static str>,
    {
        let plan = Plan::new(&st, max_w, max_h)?;
        let y = st.comp[0];
        let (yh, yv) = (y.h_samp as usize, y.v_samp as usize);
        let y_bpm = yh * yv;
        let row_bytes = plan.mcus_x * y_bpm * 64 * 2;

        let mut output = allocate(plan.out_stride * plan.out_h)?;
        let mut canvas = Canvas::new(&plan, output.as_mut())?;

        // the band takes whatever heap is left, so it is reserved last
        let mut rows = (MAX_BAND_BYTES / row_bytes).clamp(1, plan.mcus_y);
        let mut band: Vec<i16> = Vec::new();
        loop {
            match band.try_reserve_exact(rows * row_bytes / 2) {
                Ok(()) => break,
                Err(_) if rows > 1 => rows /= 2,
                Err(_) => return Err("jpeg: OOM for coefficient band"),
            }
        }
        band.resize(rows * row_bytes / 2, 0);
        let whole = rows >= plan.mcus_y;

        log::info!(
            "jpeg: progressive {}x{} → {}x{} ({} pass(es) of {} MCU rows)",
            plan.w,
            plan.h,
            plan.out_w,
            plan.out_h,
            plan.mcus_y.div_ceil(rows),
            rows
        );

        let mut pix = [0u8; 64];
        for top in (0..plan.mcus_y).step_by(rows) {
            let bottom = (top + rows).min(plan.mcus_y);
            band.fill(0);
            run_pass(&mut st, &mut bit, &plan, &mut band, top, bottom, whole)?;

            let qt = &st.qt[y.qt_idx as usize & 3];
            for my in top..bottom {
                canvas.y_row.fill(128);
                for mx in 0..plan.mcus_x {
                    for bv in 0..yv {
                        for bh in 0..yh {
                            let off =
                                band_block_offset(my - top, mx, bv * yh + bh, plan.mcus_x, y_bpm);
                            dequant_idct(&band[off..off + 64], qt, &mut pix);
                            put_block(&mut canvas.y_row, &plan, mx, bh, bv, &pix);
                        }
                    }
                }
                canvas.flush(&plan, my);
            }
        }

        let out_y = canvas.out_y;
        drop(canvas);
        Ok(DecodedImage {
            width: plan.out_w as u16,
            height: out_y as u16,
            data: output,
            stride: plan.out_stride,
        })
    }
}

#[cfg(feature = "progressive-jpeg")]
use progressive::decode_progressive;

// without the feature a progressive frame is refused instead of decoded wrongly
#[cfg(not(feature = "progressive-jpeg"))]
fn decode_progressive<R: JpegRead, B, A>(
    _st: Box<JpegState>,
    _bit: BitReader<R>,
    _max_w: u16,
    _max_h: u16,
    _allocate: A,
) -> Result<DecodedImage<B>, &'static str>
where
    B: AsMut<[u8]>,
    A: FnOnce(usize) -> Result<B, &'static str>,
{
    Err("jpeg: progressive support not built (feature progressive-jpeg)")
}

// marker parsing. Segments are read off the stream into a small buffer and
// handed to the slice parsers, so the same code serves the first look at the
// header and every later pass over a progressive file.

fn read_soi<R: JpegRead>(r: &mut BitReader<R>) -> Result<(), &'static str> {
    if r.source.read_byte().ok() == Some(0xFF) && r.source.read_byte().ok() == Some(M_SOI) {
        Ok(())
    } else {
        Err("jpeg: invalid signature")
    }
}

// parse the segment that follows `marker`; unknown ones are skipped
fn parse_segment<R: JpegRead>(
    r: &mut BitReader<R>,
    st: &mut JpegState,
    marker: u8,
) -> Result<(), &'static str> {
    match marker {
        M_SOF0 => parse_sof(&r.read_segment()?, &mut 0, st, false),
        M_SOF2 => parse_sof(&r.read_segment()?, &mut 0, st, true),
        0xC1 | 0xC3 | 0xC5..=0xCB | 0xCD..=0xCF => Err("jpeg: unsupported SOF variant"),
        M_DHT => parse_dht(&r.read_segment()?, &mut 0, st),
        M_DQT => parse_dqt(&r.read_segment()?, &mut 0, st),
        M_DRI => parse_dri(&r.read_segment()?, &mut 0, st),
        // markers without a length field
        M_SOI | M_TEM => Ok(()),
        _ => r.skip_segment(),
    }
}

// Parse up to and including the first SOS header; the reader is left at the
// start of that scan's entropy-coded data.
fn parse_header<R: JpegRead>(r: &mut BitReader<R>) -> Result<Box<JpegState>, &'static str> {
    read_soi(r)?;
    let mut st = JpegState::heap_new()?;
    loop {
        match r.next_marker()? {
            M_SOS => {
                parse_sos(&r.read_segment()?, &mut 0, &mut st)?;
                return Ok(st);
            }
            M_EOI => return Err("jpeg: EOI before SOS"),
            m => parse_segment(r, &mut st, m)?,
        }
    }
}

fn parse_sof(
    data: &[u8],
    pos: &mut usize,
    st: &mut JpegState,
    progressive: bool,
) -> Result<(), &'static str> {
    if *pos + 2 > data.len() {
        return Err("jpeg: SOF truncated");
    }
    let seg = be_u16(data, *pos) as usize;
    *pos += 2;
    if seg < 8 || *pos + seg - 2 > data.len() {
        return Err("jpeg: SOF truncated");
    }
    let p = *pos;
    if data[p] != 8 {
        return Err("jpeg: only 8-bit precision");
    }
    st.height = be_u16(data, p + 1);
    st.width = be_u16(data, p + 3);
    st.num_comp = data[p + 5];
    st.progressive = progressive;
    if st.num_comp == 0 || st.num_comp as usize > MAX_COMP {
        return Err("jpeg: bad component count");
    }
    if p + 6 + st.num_comp as usize * 3 > data.len() {
        return Err("jpeg: SOF truncated");
    }
    let mut off = p + 6;
    st.max_h = 1;
    st.max_v = 1;
    for i in 0..st.num_comp as usize {
        st.comp[i].id = data[off];
        let samp = data[off + 1];
        st.comp[i].h_samp = samp >> 4;
        st.comp[i].v_samp = samp & 0x0F;
        st.comp[i].qt_idx = data[off + 2];
        if st.comp[i].h_samp == 0 || st.comp[i].v_samp == 0 {
            return Err("jpeg: zero sampling factor");
        }
        st.max_h = st.max_h.max(st.comp[i].h_samp);
        st.max_v = st.max_v.max(st.comp[i].v_samp);
        off += 3;
    }
    *pos += seg - 2;
    Ok(())
}

fn parse_dqt(data: &[u8], pos: &mut usize, st: &mut JpegState) -> Result<(), &'static str> {
    if *pos + 2 > data.len() {
        return Err("jpeg: DQT truncated");
    }
    let seg = be_u16(data, *pos) as usize;
    let end = *pos + seg;
    *pos += 2;
    if end > data.len() {
        return Err("jpeg: DQT truncated");
    }
    while *pos < end {
        let info = data[*pos];
        *pos += 1;
        let prec = info >> 4;
        let id = (info & 0x0F) as usize;
        if id >= 4 {
            return Err("jpeg: DQT id out of range");
        }
        if prec == 0 {
            if *pos + 64 > end {
                return Err("jpeg: DQT truncated");
            }
            for i in 0..64 {
                st.qt[id][i] = data[*pos] as u16;
                *pos += 1;
            }
        } else {
            if *pos + 128 > end {
                return Err("jpeg: DQT truncated");
            }
            for i in 0..64 {
                st.qt[id][i] = be_u16(data, *pos);
                *pos += 2;
            }
        }
        st.qt_ok[id] = true;
    }
    Ok(())
}

fn parse_dht(data: &[u8], pos: &mut usize, st: &mut JpegState) -> Result<(), &'static str> {
    if *pos + 2 > data.len() {
        return Err("jpeg: DHT truncated");
    }
    let seg = be_u16(data, *pos) as usize;
    let end = *pos + seg;
    *pos += 2;
    if end > data.len() {
        return Err("jpeg: DHT truncated");
    }
    while *pos < end {
        if *pos + 17 > end {
            return Err("jpeg: DHT truncated");
        }
        let info = data[*pos];
        *pos += 1;
        let class = info >> 4;
        let id = (info & 0x0F) as usize;
        if id >= 4 {
            return Err("jpeg: DHT id out of range");
        }
        let mut bits = [0u8; 16];
        bits.copy_from_slice(&data[*pos..*pos + 16]);
        *pos += 16;
        let total: usize = bits.iter().map(|&b| b as usize).sum();
        if total > 256 || *pos + total > end {
            return Err("jpeg: DHT value overflow");
        }
        let vals = &data[*pos..*pos + total];
        *pos += total;
        if class == 0 {
            build_huff_table(&mut st.dc_huff[id], &bits, vals);
            st.dc_ok[id] = true;
        } else {
            build_huff_table(&mut st.ac_huff[id], &bits, vals);
            st.ac_ok[id] = true;
        }
    }
    Ok(())
}

fn parse_dri(data: &[u8], pos: &mut usize, st: &mut JpegState) -> Result<(), &'static str> {
    if *pos + 4 > data.len() {
        return Err("jpeg: DRI truncated");
    }
    *pos += 2;
    st.restart_interval = be_u16(data, *pos);
    *pos += 2;
    Ok(())
}

fn parse_sos(data: &[u8], pos: &mut usize, st: &mut JpegState) -> Result<(), &'static str> {
    if *pos + 3 > data.len() {
        return Err("jpeg: SOS truncated");
    }
    let seg = be_u16(data, *pos) as usize;
    if *pos + seg > data.len() {
        return Err("jpeg: SOS truncated");
    }
    *pos += 2;
    st.scan_num_comp = data[*pos];
    *pos += 1;
    if st.scan_num_comp == 0 || st.scan_num_comp > st.num_comp {
        return Err("jpeg: bad SOS component count");
    }
    if *pos + 2 * st.scan_num_comp as usize + 3 > data.len() {
        return Err("jpeg: SOS truncated");
    }
    for sci in 0..st.scan_num_comp as usize {
        let cs = data[*pos];
        let td_ta = data[*pos + 1];
        *pos += 2;
        if td_ta >> 4 >= 4 || td_ta & 0x0F >= 4 {
            return Err("jpeg: SOS table id out of range");
        }
        let mut found = false;
        for j in 0..st.num_comp as usize {
            if st.comp[j].id == cs {
                st.comp[j].dc_tbl = td_ta >> 4;
                st.comp[j].ac_tbl = td_ta & 0x0F;
                st.scan_order[sci] = j as u8;
                found = true;
                break;
            }
        }
        if !found {
            return Err("jpeg: SOS references unknown component");
        }
    }
    st.scan_ss = data[*pos].min(63);
    st.scan_se = data[*pos + 1].min(63);
    let ah_al = data[*pos + 2];
    st.scan_ah = ah_al >> 4;
    st.scan_al = ah_al & 0x0F;
    *pos += 3;
    Ok(())
}

// Huffman table construction

fn build_huff_table(table: &mut HuffTable, bits: &[u8; 16], vals: &[u8]) {
    let total: usize = bits.iter().map(|&b| b as usize).sum();
    table.values[..total].copy_from_slice(&vals[..total]);
    table.lut.fill((0, 0));
    table.maxcode.fill(-1);

    let mut code: u32 = 0;
    let mut si: usize = 0;

    for bl in 1..=16usize {
        let cnt = bits[bl - 1] as usize;
        if cnt > 0 {
            table.valptr[bl] = si;
            table.mincode[bl] = code as i32;
            for _ in 0..cnt {
                if bl <= 8 {
                    let prefix = (code << (8 - bl)) as usize;
                    let fill = 1usize << (8 - bl);
                    for k in 0..fill {
                        if prefix + k < 256 {
                            table.lut[prefix + k] = (vals[si], bl as u8);
                        }
                    }
                }
                si += 1;
                code += 1;
            }
            table.maxcode[bl] = (code - 1) as i32;
        }
        code <<= 1;
    }
}

// Huffman decode

fn huff_decode<R: JpegRead>(r: &mut BitReader<R>, t: &HuffTable) -> Result<u8, &'static str> {
    let peek8 = r.peek(8)? as usize;
    let (sym, nb) = t.lut[peek8];
    if nb > 0 {
        r.drop_bits(nb);
        return Ok(sym);
    }
    let peek16 = r.peek(16)? as i32;
    for bl in 9..=16u8 {
        let code = peek16 >> (16 - bl);
        if t.maxcode[bl as usize] >= 0 && code <= t.maxcode[bl as usize] {
            r.drop_bits(bl);
            let idx = t.valptr[bl as usize] as i32 + code - t.mincode[bl as usize];
            return Ok(t.values[idx as usize]);
        }
    }
    Err("jpeg: invalid Huffman code")
}

#[inline]
fn extend(bits: u32, size: u8) -> i32 {
    let half = 1u32 << (size as u32 - 1);
    if bits < half {
        bits as i32 - ((1u32 << size as u32) as i32 - 1)
    } else {
        bits as i32
    }
}

// block decode (Y) / skip (non-Y)

fn decode_block<R: JpegRead>(
    r: &mut BitReader<R>,
    dc_ht: &HuffTable,
    ac_ht: &HuffTable,
    dc_pred: &mut i32,
    qt: &[u16; 64],
    blk: &mut [i32; 64],
    se: usize,
    al: u8,
) -> Result<(), &'static str> {
    blk.fill(0);

    let dc_size = huff_decode(r, dc_ht)?;
    if dc_size > 0 {
        if dc_size > 11 {
            return Err("jpeg: DC size > 11");
        }
        let bits = r.read_bits(dc_size)?;
        *dc_pred += extend(bits, dc_size);
    }
    blk[0] = ((*dc_pred) << al).wrapping_mul(qt[0] as i32);

    if se > 0 {
        let mut k: usize = 1;
        while k <= se {
            let sym = huff_decode(r, ac_ht)?;
            let run = (sym >> 4) as usize;
            let size = sym & 0x0F;
            if size == 0 {
                if run == 15 {
                    k += 16;
                } else {
                    break;
                }
            } else {
                k += run;
                if k > se {
                    return Err("jpeg: AC index overflow");
                }
                let bits = r.read_bits(size)?;
                let val = extend(bits, size);
                blk[ZZ[k]] = (val << al).wrapping_mul(qt[k] as i32);
                k += 1;
            }
        }
    }
    Ok(())
}

fn skip_block<R: JpegRead>(
    r: &mut BitReader<R>,
    dc_ht: &HuffTable,
    ac_ht: &HuffTable,
    dc_pred: &mut i32,
    se: usize,
) -> Result<(), &'static str> {
    let dc_size = huff_decode(r, dc_ht)?;
    if dc_size > 0 {
        if dc_size > 11 {
            return Err("jpeg: DC size > 11");
        }
        let bits = r.read_bits(dc_size)?;
        *dc_pred += extend(bits, dc_size);
    }
    if se > 0 {
        let mut k: usize = 1;
        while k <= se {
            let sym = huff_decode(r, ac_ht)?;
            let run = (sym >> 4) as usize;
            let size = sym & 0x0F;
            if size == 0 {
                if run == 15 {
                    k += 16;
                } else {
                    break;
                }
            } else {
                k += run + 1;
                let _ = r.read_bits(size)?;
            }
        }
    }
    Ok(())
}

// integer IDCT (IJG ISLOW, two-pass row + col)

fn idct(block: &[i32; 64], out: &mut [u8; 64]) {
    let mut ws = [0i32; 64];

    for row in 0..8 {
        let b = row * 8;
        let (d0, d1, d2, d3) = (block[b], block[b + 1], block[b + 2], block[b + 3]);
        let (d4, d5, d6, d7) = (block[b + 4], block[b + 5], block[b + 6], block[b + 7]);

        if d1 == 0 && d2 == 0 && d3 == 0 && d4 == 0 && d5 == 0 && d6 == 0 && d7 == 0 {
            let dc = d0 << P1;
            ws[b..b + 8].fill(dc);
            continue;
        }

        let z1 = (d2 + d6).wrapping_mul(F0541);
        let tmp2 = z1 + d6.wrapping_mul(-F1847);
        let tmp3 = z1 + d2.wrapping_mul(F0765);
        let tmp0 = (d0 + d4) << CB;
        let tmp1 = (d0 - d4) << CB;
        let (t10, t13) = (tmp0 + tmp3, tmp0 - tmp3);
        let (t11, t12) = (tmp1 + tmp2, tmp1 - tmp2);

        let (zz1, zz2, zz3, zz4) = (d7 + d1, d5 + d3, d7 + d3, d5 + d1);
        let z5 = (zz3 + zz4).wrapping_mul(F1175);
        let mut o0 = d7.wrapping_mul(F0298);
        let mut o1 = d5.wrapping_mul(F2053);
        let mut o2 = d3.wrapping_mul(F3072);
        let mut o3 = d1.wrapping_mul(F1501);
        let (s1, s2) = (zz1.wrapping_mul(-F0899), zz2.wrapping_mul(-F2562));
        let s3 = zz3.wrapping_mul(-F1961) + z5;
        let s4 = zz4.wrapping_mul(-F0390) + z5;
        o0 += s1 + s3;
        o1 += s2 + s4;
        o2 += s2 + s3;
        o3 += s1 + s4;

        let sh = CB - P1;
        ws[b] = descale(t10 + o3, sh);
        ws[b + 7] = descale(t10 - o3, sh);
        ws[b + 1] = descale(t11 + o2, sh);
        ws[b + 6] = descale(t11 - o2, sh);
        ws[b + 2] = descale(t12 + o1, sh);
        ws[b + 5] = descale(t12 - o1, sh);
        ws[b + 3] = descale(t13 + o0, sh);
        ws[b + 4] = descale(t13 - o0, sh);
    }

    for col in 0..8 {
        let (d0, d1, d2, d3) = (ws[col], ws[col + 8], ws[col + 16], ws[col + 24]);
        let (d4, d5, d6, d7) = (ws[col + 32], ws[col + 40], ws[col + 48], ws[col + 56]);

        if d1 == 0 && d2 == 0 && d3 == 0 && d4 == 0 && d5 == 0 && d6 == 0 && d7 == 0 {
            let v = clamp(descale(d0, P1 + 3) + 128);
            out[col] = v;
            out[col + 8] = v;
            out[col + 16] = v;
            out[col + 24] = v;
            out[col + 32] = v;
            out[col + 40] = v;
            out[col + 48] = v;
            out[col + 56] = v;
            continue;
        }

        let z1 = (d2 + d6).wrapping_mul(F0541);
        let tmp2 = z1 + d6.wrapping_mul(-F1847);
        let tmp3 = z1 + d2.wrapping_mul(F0765);
        let tmp0 = (d0 + d4) << CB;
        let tmp1 = (d0 - d4) << CB;
        let (t10, t13) = (tmp0 + tmp3, tmp0 - tmp3);
        let (t11, t12) = (tmp1 + tmp2, tmp1 - tmp2);

        let (zz1, zz2, zz3, zz4) = (d7 + d1, d5 + d3, d7 + d3, d5 + d1);
        let z5 = (zz3 + zz4).wrapping_mul(F1175);
        let mut o0 = d7.wrapping_mul(F0298);
        let mut o1 = d5.wrapping_mul(F2053);
        let mut o2 = d3.wrapping_mul(F3072);
        let mut o3 = d1.wrapping_mul(F1501);
        let (s1, s2) = (zz1.wrapping_mul(-F0899), zz2.wrapping_mul(-F2562));
        let s3 = zz3.wrapping_mul(-F1961) + z5;
        let s4 = zz4.wrapping_mul(-F0390) + z5;
        o0 += s1 + s3;
        o1 += s2 + s4;
        o2 += s2 + s3;
        o3 += s1 + s4;

        let sh = CB + P1 + 3;
        out[col] = clamp(descale(t10 + o3, sh) + 128);
        out[col + 56] = clamp(descale(t10 - o3, sh) + 128);
        out[col + 8] = clamp(descale(t11 + o2, sh) + 128);
        out[col + 48] = clamp(descale(t11 - o2, sh) + 128);
        out[col + 16] = clamp(descale(t12 + o1, sh) + 128);
        out[col + 40] = clamp(descale(t12 - o1, sh) + 128);
        out[col + 24] = clamp(descale(t13 + o0, sh) + 128);
        out[col + 32] = clamp(descale(t13 - o0, sh) + 128);
    }
}

// Floyd-Steinberg dithering

// dither one row of Y pixels from the MCU row buffer inline
#[inline]
fn dither_row_grey(
    row: &[u8],
    fit: &Fit,
    out_w: usize,
    err_cur: &mut [i16],
    err_nxt: &mut [i16],
    out_row: &mut [u8],
) {
    for ox in 0..out_w {
        let g = row[fit.src_x(ox)] as i16;
        let val = (g + err_cur[ox + 1]).clamp(0, 255);
        let black = val < 128;
        let q = if black { 0i16 } else { 255 };
        let e = val - q;
        if black {
            out_row[ox / 8] |= 1 << (7 - (ox & 7));
        }
        err_cur[ox + 2] += e * 7 / 16;
        err_nxt[ox] += e * 3 / 16;
        err_nxt[ox + 1] += e * 5 / 16;
        err_nxt[ox + 2] += e / 16;
    }
}

// helpers

#[inline]
fn descale(x: i32, n: i32) -> i32 {
    (x + (1 << (n - 1))) >> n
}

#[inline]
fn clamp(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

#[inline]
fn be_u16(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}

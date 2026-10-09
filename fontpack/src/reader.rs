// Random-access pack reader: the firmware cannot hold a pack in RAM (the index
// of a CJK pack alone is hundreds of KB), so it reads the SD file on demand.
// Header and record checks are the ones `Pack::parse` uses (see `format`); only
// the records a lookup actually touches are validated.

use core::cmp::Ordering;
use core::fmt;

use crate::format::Record;
use crate::{FontInfo, HEADER_LEN, Header, INDEX_RECORD_LEN, Metrics, PackError, bitmap_size};

// records the final, uncached stretch of a search may span and still be read in
// one go; a larger stretch is halved by single-record probes first. Kept small
// because it lives on the stack.
pub const SPAN_RECORDS: usize = 32;
const SPAN_BYTES: usize = SPAN_RECORDS * INDEX_RECORD_LEN;
// codepoint stored for a cache node not read yet; no valid record has it
// (a validated record is a unicode scalar, at most 0x10ffff)
const UNFILLED: u32 = u32::MAX;

// what a cache's contents are valid for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    font_id: u64,
    pixel_size: u16,
    glyph_count: u32,
    bitmap_len: u32,
}

fn cache_key(header: &Header) -> CacheKey {
    CacheKey {
        font_id: header.info.font_id,
        pixel_size: header.info.pixel_size,
        glyph_count: header.glyph_count,
        bitmap_len: header.bitmap_len,
    }
}

// bytes per sequential read while an index is loaded into memory
const LOAD_CHUNK: usize = 4096;

// The top levels of the binary search over one pack's index, kept between
// `find_cached` calls. Node `n` (heap order: children `2n + 1` and `2n + 2`)
// holds the codepoint of the record the search probes there, read and checked
// the first time a search passes it. The shape of the search depends only on
// `glyph_count`, so nothing but the codepoint is stored. `storage` holds
// `2^levels - 1` words for `levels` levels; any length works, a partial level
// caches its leftmost nodes.
//
// A cache binds to the pack it is first used with and empties itself when a
// different pack (`font_id`, size, record count or bitmap size) shows up.
pub struct IndexCache<S> {
    nodes: S,
    key: Option<CacheKey>,
}

impl<S: AsRef<[u32]> + AsMut<[u32]>> IndexCache<S> {
    // words of storage for `levels` levels of the search
    pub const fn nodes_for_levels(levels: u32) -> usize {
        (1usize << levels) - 1
    }

    pub fn new(storage: S) -> Self {
        Self {
            nodes: storage,
            key: None,
        }
    }

    // forgets everything read so far
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    // nodes holding a read record
    pub fn filled(&self) -> usize {
        match self.key {
            Some(_) => self
                .nodes
                .as_ref()
                .iter()
                .filter(|&&n| n != UNFILLED)
                .count(),
            None => 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.nodes.as_ref().len()
    }

    fn bind(&mut self, header: &Header) {
        let key = cache_key(header);
        if self.key != Some(key) {
            self.nodes.as_mut().fill(UNFILLED);
            self.key = Some(key);
        }
    }

    fn get(&self, node: usize) -> Option<u32> {
        self.nodes
            .as_ref()
            .get(node)
            .copied()
            .filter(|&n| n != UNFILLED)
    }
}

// A whole pack index (every 22 byte record) held in memory, for boards with
// RAM to spare (PSRAM): one sequential load replaces the SD read every lookup
// of `find_cached` still needs, so a lookup is a binary search in memory.
//
// `load_index` validates every record, in order, once. A cache binds to the
// pack it was loaded from; `find_in_ram` answers `None` for any other pack (or
// before a successful load), and the caller falls back to `find_cached` /
// `find`. A pack that changes under an open reader is not noticed.
pub struct RamIndex<S> {
    bytes: S,
    key: Option<CacheKey>,
}

impl<S: AsRef<[u8]> + AsMut<[u8]>> RamIndex<S> {
    pub fn new(storage: S) -> Self {
        Self {
            bytes: storage,
            key: None,
        }
    }

    // bytes of storage a pack of `glyph_count` records needs; `None` if that
    // does not fit `usize`
    pub fn bytes_for(glyph_count: u32) -> Option<usize> {
        usize::try_from(glyph_count)
            .ok()?
            .checked_mul(INDEX_RECORD_LEN)
    }

    // forgets the loaded index
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    // whether a load completed (for some pack)
    pub fn is_loaded(&self) -> bool {
        self.key.is_some()
    }

    pub fn capacity(&self) -> usize {
        self.bytes.as_ref().len()
    }
}

// positioned reads from the pack file. A read that cannot be satisfied in full
// is an error; there are no short reads.
pub trait ReadAt {
    type Error;

    // fills `buf` with the bytes at [offset, offset + buf.len())
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfRange;

impl ReadAt for &[u8] {
    type Error = OutOfRange;

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), OutOfRange> {
        let start = usize::try_from(offset).map_err(|_| OutOfRange)?;
        let end = start.checked_add(buf.len()).ok_or(OutOfRange)?;
        buf.copy_from_slice(self.get(start..end).ok_or(OutOfRange)?);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontError<E> {
    // a pack of another format version
    Unsupported { found: u16 },
    // structural damage, with its cause
    Corrupt(PackError),
    // the read itself failed
    Io(E),
    // the caller's buffer is smaller than the glyph bitmap
    BufferTooSmall { needed: usize },
}

impl<E: fmt::Display> fmt::Display for FontError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { found } => {
                write!(f, "font failure: unsupported pack version {found}")
            }
            Self::Corrupt(e) => write!(f, "font failure: {e}"),
            Self::Io(e) => write!(f, "font read failed: {e}"),
            Self::BufferTooSmall { needed } => {
                write!(f, "glyph buffer too small, {needed} bytes needed")
            }
        }
    }
}

// a glyph found by `PackReader::find`; the bitmap position is relative to the
// bitmap region and only meaningful to the reader that returned it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphRef {
    pub metrics: Metrics,
    pub bitmap_len: u32,
    bitmap_offset: u32,
}

impl Record {
    fn glyph_ref(&self) -> GlyphRef {
        GlyphRef {
            metrics: self.metrics,
            bitmap_len: self.bitmap_len,
            bitmap_offset: self.bitmap_offset,
        }
    }
}

impl GlyphRef {
    // the bitmap position within the bitmap region, for callers that keep a lookup
    pub fn bitmap_offset(&self) -> u32 {
        self.bitmap_offset
    }

    // rebuilds a ref from a kept lookup of the same pack: a validated record's
    // bitmap length is always `bitmap_size` of its metrics, and `read_bitmap`
    // still range-checks the position against the reader it is used with
    pub fn from_located(metrics: Metrics, bitmap_offset: u32) -> Self {
        Self {
            metrics,
            bitmap_len: bitmap_size(metrics.width, metrics.height),
            bitmap_offset,
        }
    }
}

pub struct PackReader<R: ReadAt> {
    src: R,
    header: Header,
    // file offset of the bitmap region
    bitmap_start: u64,
}

impl<R: ReadAt> PackReader<R> {
    // one 44 byte read; `file_len` is the size of the whole file
    pub fn open(mut src: R, file_len: u64) -> Result<Self, FontError<R::Error>> {
        if file_len < HEADER_LEN as u64 {
            return Err(FontError::Corrupt(PackError::TooShort));
        }
        let mut raw = [0u8; HEADER_LEN];
        src.read_at(0, &mut raw).map_err(FontError::Io)?;
        let header = Header::decode(&raw, file_len).map_err(|e| match e {
            PackError::UnsupportedVersion { found } => FontError::Unsupported { found },
            e => FontError::Corrupt(e),
        })?;
        let layout = header
            .layout()
            .ok_or(FontError::Corrupt(PackError::BadLayout))?;
        Ok(Self {
            src,
            header,
            bitmap_start: u64::from(layout.bitmap_offset),
        })
    }

    pub fn header(&self) -> Header {
        self.header
    }

    pub fn info(&self) -> FontInfo {
        self.header.info
    }

    // binary search, one 22 byte read per probe; each probed record is checked
    // on its own (its order against the neighbours cannot be)
    pub fn find(&mut self, c: char) -> Result<Option<GlyphRef>, FontError<R::Error>> {
        let want = u32::from(c);
        let (mut lo, mut hi) = (0u32, self.header.glyph_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let rec = self.read_record(mid)?;
            match rec.codepoint.cmp(&want) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Ok(Some(rec.glyph_ref())),
            }
        }
        Ok(None)
    }

    // one validated record: a 22 byte read at its position
    fn read_record(&mut self, index: u32) -> Result<Record, FontError<R::Error>> {
        let mut raw = [0u8; INDEX_RECORD_LEN];
        let at = HEADER_LEN as u64 + INDEX_RECORD_LEN as u64 * u64::from(index);
        self.src.read_at(at, &mut raw).map_err(FontError::Io)?;
        let rec = Record::decode(&raw).ok_or(FontError::Corrupt(PackError::BadLayout))?;
        rec.validate(index, None, self.header.bitmap_len)
            .map_err(FontError::Corrupt)?;
        Ok(rec)
    }

    // `find` with the top of the search answered from `cache` and the rest read
    // in one go. Finds the same glyph, or fails the same way, as `find`: every
    // record the search relies on is checked on its own, once when it enters the
    // cache and each time it is read from the pack. Differences: a record
    // already in the cache is not read again (a pack that changes under an open
    // reader is not noticed there), and the last stretch of the search is read
    // as a whole, so a damaged tail of the file fails the search even where the
    // plain search would not have gone.
    pub fn find_cached<S: AsRef<[u32]> + AsMut<[u32]>>(
        &mut self,
        c: char,
        cache: &mut IndexCache<S>,
    ) -> Result<Option<GlyphRef>, FontError<R::Error>> {
        cache.bind(&self.header);
        let want = u32::from(c);
        let (mut lo, mut hi) = (0u32, self.header.glyph_count);
        let mut node = 0usize;
        // the cached levels
        while lo < hi && node < cache.capacity() {
            let mid = lo + (hi - lo) / 2;
            let at_mid = match cache.get(node) {
                Some(codepoint) => codepoint,
                None => {
                    let rec = self.read_record(mid)?;
                    if let Some(slot) = cache.nodes.as_mut().get_mut(node) {
                        *slot = rec.codepoint;
                    }
                    if rec.codepoint == want {
                        return Ok(Some(rec.glyph_ref()));
                    }
                    rec.codepoint
                }
            };
            match at_mid.cmp(&want) {
                Ordering::Less => {
                    lo = mid + 1;
                    node = 2 * node + 2;
                }
                Ordering::Greater => {
                    hi = mid;
                    node = 2 * node + 1;
                }
                // a cache holds the codepoint only
                Ordering::Equal => return Ok(Some(self.read_record(mid)?.glyph_ref())),
            }
        }
        // below the cache: halve by probes until the rest fits the buffer
        while hi - lo > SPAN_RECORDS as u32 {
            let mid = lo + (hi - lo) / 2;
            let rec = self.read_record(mid)?;
            match rec.codepoint.cmp(&want) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Ok(Some(rec.glyph_ref())),
            }
        }
        if lo >= hi {
            return Ok(None);
        }
        // one read of what is left, searched in memory like `find` searches the file
        let mut buf = [0u8; SPAN_BYTES];
        let len = (hi - lo) as usize * INDEX_RECORD_LEN;
        let span = buf
            .get_mut(..len)
            .ok_or(FontError::Corrupt(PackError::BadLayout))?;
        let at = HEADER_LEN as u64 + INDEX_RECORD_LEN as u64 * u64::from(lo);
        self.src.read_at(at, span).map_err(FontError::Io)?;
        let first = lo;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let start = (mid - first) as usize * INDEX_RECORD_LEN;
            let raw = span
                .get(start..)
                .ok_or(FontError::Corrupt(PackError::BadLayout))?;
            let rec = Record::decode(raw).ok_or(FontError::Corrupt(PackError::BadLayout))?;
            rec.validate(mid, None, self.header.bitmap_len)
                .map_err(FontError::Corrupt)?;
            match rec.codepoint.cmp(&want) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Ok(Some(rec.glyph_ref())),
            }
        }
        Ok(None)
    }

    // Reads the whole index into `idx` in `LOAD_CHUNK` sized sequential reads
    // and checks every record (validity and ascending codepoints). On any
    // error `idx` is left unloaded. `BufferTooSmall` when the storage is
    // smaller than `RamIndex::bytes_for(glyph_count)`.
    pub fn load_index<S: AsRef<[u8]> + AsMut<[u8]>>(
        &mut self,
        idx: &mut RamIndex<S>,
    ) -> Result<(), FontError<R::Error>> {
        idx.key = None;
        let needed = RamIndex::<S>::bytes_for(self.header.glyph_count)
            .ok_or(FontError::Corrupt(PackError::BadLayout))?;
        let dest = idx
            .bytes
            .as_mut()
            .get_mut(..needed)
            .ok_or(FontError::BufferTooSmall { needed })?;
        let mut done = 0usize;
        while done < needed {
            let n = (needed - done).min(LOAD_CHUNK);
            let end = done
                .checked_add(n)
                .ok_or(FontError::Corrupt(PackError::BadLayout))?;
            let part = dest
                .get_mut(done..end)
                .ok_or(FontError::Corrupt(PackError::BadLayout))?;
            let at = HEADER_LEN as u64 + done as u64;
            self.src.read_at(at, part).map_err(FontError::Io)?;
            done = end;
        }
        let mut prev = None;
        for (i, raw) in dest.chunks_exact(INDEX_RECORD_LEN).enumerate() {
            let rec = Record::decode(raw).ok_or(FontError::Corrupt(PackError::BadLayout))?;
            let index = u32::try_from(i).map_err(|_| FontError::Corrupt(PackError::BadLayout))?;
            prev = Some(
                rec.validate(index, prev, self.header.bitmap_len)
                    .map_err(FontError::Corrupt)?,
            );
        }
        idx.key = Some(cache_key(&self.header));
        Ok(())
    }

    // `find` answered from a loaded index without any read. `None`: `idx` is
    // not loaded for this pack, ask `find_cached` / `find` instead.
    // `Some(None)` is a known absence.
    pub fn find_in_ram<S: AsRef<[u8]>>(
        &self,
        c: char,
        idx: &RamIndex<S>,
    ) -> Option<Option<GlyphRef>> {
        if idx.key != Some(cache_key(&self.header)) {
            return None;
        }
        let bytes = idx.bytes.as_ref();
        let want = u32::from(c);
        let (mut lo, mut hi) = (0usize, usize::try_from(self.header.glyph_count).ok()?);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let raw = bytes.get(mid.checked_mul(INDEX_RECORD_LEN)?..)?;
            let rec = Record::decode(raw)?;
            match rec.codepoint.cmp(&want) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Some(Some(rec.glyph_ref())),
            }
        }
        Some(None)
    }

    // one read of exactly the bitmap; a blank glyph reads nothing
    pub fn read_bitmap<'b>(
        &mut self,
        g: &GlyphRef,
        buf: &'b mut [u8],
    ) -> Result<&'b [u8], FontError<R::Error>> {
        // a ref from another pack must not send the read outside this file
        let end = g.bitmap_offset.checked_add(g.bitmap_len);
        if end.is_none_or(|end| end > self.header.bitmap_len) {
            return Err(FontError::Corrupt(PackError::BadLayout));
        }
        // usize is at least 32 bits on every target
        let needed = g.bitmap_len as usize;
        let out = buf
            .get_mut(..needed)
            .ok_or(FontError::BufferTooSmall { needed })?;
        if needed > 0 {
            let at = self.bitmap_start + u64::from(g.bitmap_offset);
            self.src.read_at(at, out).map_err(FontError::Io)?;
        }
        Ok(out)
    }
}

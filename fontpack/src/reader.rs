// Random-access pack reader: the firmware cannot hold a pack in RAM (the index
// of a CJK pack alone is hundreds of KB), so it reads the SD file on demand.
// Header and record checks are the ones `Pack::parse` uses (see `format`); only
// the records a lookup actually touches are validated.

use core::cmp::Ordering;
use core::fmt;

use crate::format::Record;
use crate::{FontInfo, HEADER_LEN, Header, INDEX_RECORD_LEN, Metrics, PackError};

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
            let mut raw = [0u8; INDEX_RECORD_LEN];
            let at = HEADER_LEN as u64 + INDEX_RECORD_LEN as u64 * u64::from(mid);
            self.src.read_at(at, &mut raw).map_err(FontError::Io)?;
            let rec = Record::decode(&raw).ok_or(FontError::Corrupt(PackError::BadLayout))?;
            rec.validate(mid, None, self.header.bitmap_len)
                .map_err(FontError::Corrupt)?;
            match rec.codepoint.cmp(&want) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => {
                    return Ok(Some(GlyphRef {
                        metrics: rec.metrics,
                        bitmap_len: rec.bitmap_len,
                        bitmap_offset: rec.bitmap_offset,
                    }));
                }
            }
        }
        Ok(None)
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

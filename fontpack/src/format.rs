// The byte format, written once: header and index-record codecs, the derived
// layout, and the per-record rules. Reader and writer both go through here.

use crate::{
    FORMAT_VERSION, FontInfo, HEADER_LEN, Header, INDEX_RECORD_LEN, MAGIC, Metrics, PackError,
};

// little-endian reader over a slice; never panics, short input gives None
struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }

    fn i16(&mut self) -> Option<i16> {
        self.take().map(i16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
}

// `bytes[start..start + len]` without overflow or out-of-range panics
pub(crate) fn span(bytes: &[u8], start: u32, len: u32) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    bytes.get(start..end)
}

// bytes of a 1 bpp bitmap, rows padded to whole bytes
pub fn bitmap_size(width: u16, height: u16) -> u32 {
    u32::from(width).div_ceil(8) * u32::from(height)
}

// the header fields that follow from `glyph_count` and `bitmap_len`; the file
// stores them redundantly and the reader requires them to agree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub index_len: u32,
    pub bitmap_offset: u32,
    pub total_len: u32,
}

impl Header {
    // None if any of the derived u32 fields would overflow
    pub fn layout(&self) -> Option<Layout> {
        let index_len = u64::from(self.glyph_count) * INDEX_RECORD_LEN as u64;
        let index_len = u32::try_from(index_len).ok()?;
        let bitmap_offset = (HEADER_LEN as u32).checked_add(index_len)?;
        let total_len = bitmap_offset.checked_add(self.bitmap_len)?;
        Some(Layout {
            index_len,
            bitmap_offset,
            total_len,
        })
    }

    // `bytes` starts with the header (more may follow); `file_len` is the size
    // of the whole file, which the header must agree with
    pub fn decode(bytes: &[u8], file_len: u64) -> Result<Header, PackError> {
        let raw = read_header(&mut Cursor(bytes)).ok_or(PackError::TooShort)?;
        if raw.magic != MAGIC {
            return Err(PackError::BadMagic);
        }
        if raw.version != FORMAT_VERSION {
            return Err(PackError::UnsupportedVersion { found: raw.version });
        }
        if u64::from(raw.total_len) != file_len {
            return Err(PackError::LengthMismatch);
        }
        let layout = raw.header.layout().ok_or(PackError::BadLayout)?;
        let consistent = raw.index_offset == HEADER_LEN as u32
            && raw.index_len == layout.index_len
            && raw.bitmap_offset == layout.bitmap_offset
            && raw.total_len == layout.total_len;
        if !consistent {
            return Err(PackError::BadLayout);
        }
        Ok(raw.header)
    }

    #[cfg(feature = "builder")]
    pub(crate) fn encode(&self, layout: Layout, out: &mut alloc::vec::Vec<u8>) {
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.info.pixel_size.to_le_bytes());
        out.extend_from_slice(&self.info.font_id.to_le_bytes());
        out.extend_from_slice(&self.info.line_height.to_le_bytes());
        out.extend_from_slice(&self.info.ascent.to_le_bytes());
        out.extend_from_slice(&self.glyph_count.to_le_bytes());
        out.extend_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        out.extend_from_slice(&layout.index_len.to_le_bytes());
        out.extend_from_slice(&layout.bitmap_offset.to_le_bytes());
        out.extend_from_slice(&self.bitmap_len.to_le_bytes());
        out.extend_from_slice(&layout.total_len.to_le_bytes());
    }
}

// the header exactly as stored, before any check
struct RawHeader {
    magic: [u8; 4],
    version: u16,
    header: Header,
    index_offset: u32,
    index_len: u32,
    bitmap_offset: u32,
    total_len: u32,
}

fn read_header(c: &mut Cursor<'_>) -> Option<RawHeader> {
    let magic = c.take()?;
    let version = c.u16()?;
    let pixel_size = c.u16()?;
    let font_id = c.u64()?;
    let line_height = c.u16()?;
    let ascent = c.u16()?;
    let glyph_count = c.u32()?;
    let index_offset = c.u32()?;
    let index_len = c.u32()?;
    let bitmap_offset = c.u32()?;
    let bitmap_len = c.u32()?;
    let total_len = c.u32()?;
    Some(RawHeader {
        magic,
        version,
        header: Header {
            info: FontInfo {
                pixel_size,
                font_id,
                line_height,
                ascent,
            },
            glyph_count,
            bitmap_len,
        },
        index_offset,
        index_len,
        bitmap_offset,
        total_len,
    })
}

// one index record as stored; `bitmap_offset` is relative to the bitmap region
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub codepoint: u32,
    pub bitmap_offset: u32,
    pub bitmap_len: u32,
    pub metrics: Metrics,
}

impl Record {
    // reads the first INDEX_RECORD_LEN bytes; None if there are fewer
    pub fn decode(bytes: &[u8]) -> Option<Record> {
        let mut c = Cursor(bytes);
        Some(Record {
            codepoint: c.u32()?,
            bitmap_offset: c.u32()?,
            bitmap_len: c.u32()?,
            metrics: Metrics {
                advance: c.u16()?,
                offset_x: c.i16()?,
                offset_y: c.i16()?,
                width: c.u16()?,
                height: c.u16()?,
            },
        })
    }

    #[cfg(feature = "builder")]
    pub(crate) fn encode(&self, out: &mut alloc::vec::Vec<u8>) {
        out.extend_from_slice(&self.codepoint.to_le_bytes());
        out.extend_from_slice(&self.bitmap_offset.to_le_bytes());
        out.extend_from_slice(&self.bitmap_len.to_le_bytes());
        out.extend_from_slice(&self.metrics.advance.to_le_bytes());
        out.extend_from_slice(&self.metrics.offset_x.to_le_bytes());
        out.extend_from_slice(&self.metrics.offset_y.to_le_bytes());
        out.extend_from_slice(&self.metrics.width.to_le_bytes());
        out.extend_from_slice(&self.metrics.height.to_le_bytes());
    }

    // `index` is the record number, `prev` the previous record's codepoint,
    // `region_len` the bitmap region size; returns this record's codepoint
    pub fn validate(
        &self,
        index: u32,
        prev: Option<char>,
        region_len: u32,
    ) -> Result<char, PackError> {
        let c = char::from_u32(self.codepoint).ok_or(PackError::InvalidCodepoint { index })?;
        if prev.is_some_and(|p| c <= p) {
            return Err(PackError::CodepointOrder { index });
        }
        let end = self.bitmap_offset.checked_add(self.bitmap_len);
        if end.is_none_or(|end| end > region_len) {
            return Err(PackError::GlyphBitmapRange { index });
        }
        if self.bitmap_len != bitmap_size(self.metrics.width, self.metrics.height) {
            return Err(PackError::GlyphSizeMismatch { index });
        }
        Ok(c)
    }
}

// pulp-fontpack -- SD font pack format: one definition of the bytes, shared by
// the host converter (`builder` feature) and the firmware (reader only).
//
// Rule for this crate: no dependencies, no unsafe, no alloc in the reader, and
// no panicking index or arithmetic: every input byte is untrusted.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "builder")]
extern crate alloc;

#[cfg(feature = "builder")]
mod builder;
mod format;
mod missing;
mod names;
mod page_cache;
mod reader;

#[cfg(feature = "builder")]
pub use builder::{BuildError, GlyphEntry, build_pack};
pub use format::{Layout, Record, bitmap_size};
pub use missing::{missing_glyph_metrics, render_missing_glyph};
pub use names::{PACK_DIR, PackFileName, pack_file_name};
pub use page_cache::{CacheStorageError, PageCache, PageGlyphSlot, PreparationError};
pub use reader::{FontError, GlyphRef, IndexCache, OutOfRange, PackReader, ReadAt, SPAN_RECORDS};

use core::fmt;

pub const MAGIC: [u8; 4] = *b"PFNT";
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 44;
pub const INDEX_RECORD_LEN: usize = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontInfo {
    pub pixel_size: u16,
    pub font_id: u64,
    pub line_height: u16,
    pub ascent: u16,
}

// the redundant layout fields of the file header are derived: see `Layout`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub info: FontInfo,
    pub glyph_count: u32,
    pub bitmap_len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    pub advance: u16,
    pub offset_x: i16,
    pub offset_y: i16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph<'a> {
    pub metrics: Metrics,
    pub bitmap: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackError {
    TooShort,
    BadMagic,
    UnsupportedVersion { found: u16 },
    LengthMismatch,
    BadLayout,
    InvalidCodepoint { index: u32 },
    CodepointOrder { index: u32 },
    GlyphBitmapRange { index: u32 },
    GlyphSizeMismatch { index: u32 },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooShort => f.write_str("font pack shorter than its header"),
            Self::BadMagic => f.write_str("not a font pack (bad magic)"),
            Self::UnsupportedVersion { found } => {
                write!(f, "unsupported font pack version {found}")
            }
            Self::LengthMismatch => f.write_str("font pack length differs from its header"),
            Self::BadLayout => f.write_str("font pack layout fields are inconsistent"),
            Self::InvalidCodepoint { index } => {
                write!(f, "glyph record {index}: codepoint is not a unicode scalar")
            }
            Self::CodepointOrder { index } => {
                write!(
                    f,
                    "glyph record {index}: codepoints not strictly increasing"
                )
            }
            Self::GlyphBitmapRange { index } => {
                write!(
                    f,
                    "glyph record {index}: bitmap range outside the bitmap region"
                )
            }
            Self::GlyphSizeMismatch { index } => {
                write!(
                    f,
                    "glyph record {index}: bitmap length does not match its size"
                )
            }
        }
    }
}

// a fully validated pack borrowing its input; lookups cannot go out of bounds
pub struct Pack<'a> {
    header: Header,
    index: &'a [u8],
    bitmap: &'a [u8],
}

impl<'a> Pack<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Pack<'a>, PackError> {
        let header = Header::decode(bytes, bytes.len() as u64)?;
        let layout = header.layout().ok_or(PackError::BadLayout)?;
        let index =
            format::span(bytes, HEADER_LEN as u32, layout.index_len).ok_or(PackError::BadLayout)?;
        let bitmap = format::span(bytes, layout.bitmap_offset, header.bitmap_len)
            .ok_or(PackError::BadLayout)?;

        let mut prev = None;
        for (i, raw) in index.chunks_exact(INDEX_RECORD_LEN).enumerate() {
            let rec = Record::decode(raw).ok_or(PackError::BadLayout)?;
            prev = Some(rec.validate(i as u32, prev, header.bitmap_len)?);
        }
        Ok(Pack {
            header,
            index,
            bitmap,
        })
    }

    pub fn header(&self) -> Header {
        self.header
    }

    pub fn find(&self, c: char) -> Option<Glyph<'a>> {
        let want = u32::from(c);
        let (mut lo, mut hi) = (0u32, self.header.glyph_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let rec = self.record(mid)?;
            match rec.codepoint.cmp(&want) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return self.glyph(&rec),
            }
        }
        None
    }

    pub fn glyph_at(&self, index: u32) -> Option<(char, Glyph<'a>)> {
        let rec = self.record(index)?;
        Some((char::from_u32(rec.codepoint)?, self.glyph(&rec)?))
    }

    fn record(&self, index: u32) -> Option<Record> {
        let start = usize::try_from(index).ok()?.checked_mul(INDEX_RECORD_LEN)?;
        Record::decode(self.index.get(start..)?)
    }

    fn glyph(&self, rec: &Record) -> Option<Glyph<'a>> {
        let bitmap = format::span(self.bitmap, rec.bitmap_offset, rec.bitmap_len)?;
        Some(Glyph {
            metrics: rec.metrics,
            bitmap,
        })
    }
}

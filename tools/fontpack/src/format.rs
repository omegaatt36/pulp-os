//! Pack writer (§2–§5).

use std::ops::RangeInclusive;

pub const MAGIC: [u8; 8] = *b"PULPFONT";
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_LEN: u16 = 64;
pub const INDEX_OFF: u32 = 512;
pub const RECORD_LEN: u32 = 16;
pub const PIXEL_SIZES: RangeInclusive<u16> = 8..=96;
pub const MAX_GLYPHS: usize = 65_536;

/// One glyph as stored in an index record plus its packed bitmap (§4, §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub code_point: u32,
    pub width: u8,
    pub height: u8,
    pub advance: u8,
    pub offset_x: i8,
    pub offset_y: i8,
    /// `ceil(width / 8) × height` bytes, rows top to bottom, MSB-first (§5).
    pub bitmap: Vec<u8>,
}

/// Everything the writer needs. Glyphs must already be sorted strictly
/// ascending by code point; the writer rejects rather than repairs.
#[derive(Debug, Clone, Copy)]
pub struct PackInput<'a> {
    pub pixel_size: u16,
    pub line_height: u16,
    pub ascent: u16,
    pub fallback_cp: u32,
    pub glyphs: &'a [Glyph],
    pub license: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    PixelSize(u16),
    LineHeight,
    AscentAboveLineHeight {
        ascent: u16,
        line_height: u16,
    },
    GlyphCount(usize),
    EmptyLicense,
    InvalidCodePoint(u32),
    Unsorted {
        previous: u32,
        code_point: u32,
    },
    BitmapLength {
        code_point: u32,
        expected: usize,
        actual: usize,
    },
    FallbackMissing(u32),
    /// The fallback glyph has no set pixel (zero size or blank bitmap), so
    /// every missing glyph would draw as nothing (R4).
    FallbackInvisible(u32),
    /// A section offset or length does not fit in u32.
    TooLarge,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PixelSize(px) => write!(f, "pixel size {px} is outside 8..=96"),
            Self::LineHeight => write!(f, "line height is 0"),
            Self::AscentAboveLineHeight {
                ascent,
                line_height,
            } => write!(f, "ascent {ascent} exceeds line height {line_height}"),
            Self::GlyphCount(n) => write!(f, "glyph count {n} is outside 1..=65536"),
            Self::EmptyLicense => write!(f, "license text is empty"),
            Self::InvalidCodePoint(cp) => write!(f, "U+{cp:04X} is not a Unicode scalar value"),
            Self::Unsorted {
                previous,
                code_point,
            } => write!(
                f,
                "U+{code_point:04X} does not sort strictly after U+{previous:04X}"
            ),
            Self::BitmapLength {
                code_point,
                expected,
                actual,
            } => write!(
                f,
                "U+{code_point:04X} bitmap is {actual} bytes, metrics require {expected}"
            ),
            Self::FallbackMissing(cp) => write!(f, "fallback U+{cp:04X} is not in the font"),
            Self::FallbackInvisible(cp) => write!(
                f,
                "fallback U+{cp:04X} has no ink; choose a visible glyph with --fallback"
            ),
            Self::TooLarge => write!(f, "pack exceeds 4 GiB"),
        }
    }
}

impl std::error::Error for WriteError {}

/// `ceil(width / 8)` (§5).
pub fn row_bytes(width: u8) -> usize {
    (width as usize).div_ceil(8)
}

pub fn is_scalar(cp: u32) -> bool {
    char::from_u32(cp).is_some()
}

pub fn write_pack(input: &PackInput<'_>) -> Result<Vec<u8>, WriteError> {
    validate(input)?;
    let n = input.glyphs.len() as u32;
    let too_large = |v: Option<u32>| v.ok_or(WriteError::TooLarge);

    let index_len = too_large(RECORD_LEN.checked_mul(n))?;
    let bitmap_off = too_large(INDEX_OFF.checked_add(index_len))?;
    let bitmap_len: usize = input.glyphs.iter().map(|g| g.bitmap.len()).sum();
    let bitmap_len = too_large(u32::try_from(bitmap_len).ok())?;
    let license_off = too_large(bitmap_off.checked_add(bitmap_len))?;
    let license_len = too_large(u32::try_from(input.license.len()).ok())?;
    let file_len = too_large(license_off.checked_add(license_len))?;

    let mut out = Vec::with_capacity(file_len as usize);
    out.resize(INDEX_OFF as usize, 0);

    let mut offset = 0u32;
    for g in input.glyphs {
        out.extend_from_slice(&g.code_point.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&[
            g.width,
            g.height,
            g.advance,
            g.offset_x as u8,
            g.offset_y as u8,
            0,
            0,
            0,
        ]);
        offset += g.bitmap.len() as u32;
    }
    for g in input.glyphs {
        out.extend_from_slice(&g.bitmap);
    }
    out.extend_from_slice(input.license);
    debug_assert_eq!(out.len(), file_len as usize);

    let index_crc = crc32fast::hash(&out[INDEX_OFF as usize..bitmap_off as usize]);

    let mut h = Vec::with_capacity(HEADER_LEN as usize);
    h.extend_from_slice(&MAGIC);
    for v in [
        FORMAT_VERSION,
        HEADER_LEN,
        input.pixel_size,
        input.line_height,
        input.ascent,
        0,
    ] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    for v in [
        n,
        input.fallback_cp,
        INDEX_OFF,
        index_len,
        bitmap_off,
        bitmap_len,
        license_off,
        license_len,
        file_len,
        index_crc,
    ] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    let header_crc = crc32fast::hash(&h);
    h.extend_from_slice(&header_crc.to_le_bytes());
    out[..HEADER_LEN as usize].copy_from_slice(&h);
    Ok(out)
}

fn validate(input: &PackInput<'_>) -> Result<(), WriteError> {
    if !PIXEL_SIZES.contains(&input.pixel_size) {
        return Err(WriteError::PixelSize(input.pixel_size));
    }
    if input.line_height == 0 {
        return Err(WriteError::LineHeight);
    }
    if input.ascent > input.line_height {
        return Err(WriteError::AscentAboveLineHeight {
            ascent: input.ascent,
            line_height: input.line_height,
        });
    }
    if !(1..=MAX_GLYPHS).contains(&input.glyphs.len()) {
        return Err(WriteError::GlyphCount(input.glyphs.len()));
    }
    if input.license.is_empty() {
        return Err(WriteError::EmptyLicense);
    }
    let mut previous: Option<u32> = None;
    for g in input.glyphs {
        if !is_scalar(g.code_point) {
            return Err(WriteError::InvalidCodePoint(g.code_point));
        }
        if let Some(p) = previous
            && g.code_point <= p
        {
            return Err(WriteError::Unsorted {
                previous: p,
                code_point: g.code_point,
            });
        }
        previous = Some(g.code_point);
        let expected = row_bytes(g.width) * g.height as usize;
        if g.bitmap.len() != expected {
            return Err(WriteError::BitmapLength {
                code_point: g.code_point,
                expected,
                actual: g.bitmap.len(),
            });
        }
    }
    let fallback = input
        .glyphs
        .binary_search_by_key(&input.fallback_cp, |g| g.code_point)
        .map_err(|_| WriteError::FallbackMissing(input.fallback_cp))?;
    let glyph = &input.glyphs[fallback];
    let stride = row_bytes(glyph.width);
    let whole_bytes = glyph.width as usize / 8;
    let final_bits = glyph.width % 8;
    let visible = stride != 0
        && glyph.bitmap.chunks(stride).any(|row| {
            row[..whole_bytes].iter().any(|&b| b != 0)
                || (final_bits != 0 && row[whole_bytes] & (0xFF << (8 - final_bits)) != 0)
        });
    if !visible {
        return Err(WriteError::FallbackInvisible(input.fallback_cp));
    }
    Ok(())
}

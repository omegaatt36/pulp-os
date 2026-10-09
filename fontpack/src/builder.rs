// Host-side pack writer. Emits exactly the layout the reader checks, through
// the same header/record codec.

use alloc::vec::Vec;

use crate::format::Record;
use crate::{FontInfo, Header, Metrics, format};

pub struct GlyphEntry {
    pub codepoint: char,
    pub metrics: Metrics,
    pub bitmap: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    NotStrictlyIncreasing { index: u32 },
    BitmapSizeMismatch { index: u32 },
    TooLarge,
}

// entries must already be sorted strictly ascending by codepoint
pub fn build_pack(info: &FontInfo, glyphs: &[GlyphEntry]) -> Result<Vec<u8>, BuildError> {
    let glyph_count = u32::try_from(glyphs.len()).map_err(|_| BuildError::TooLarge)?;
    let mut bitmap_len = 0u32;
    let mut prev: Option<char> = None;
    for (i, g) in glyphs.iter().enumerate() {
        let index = i as u32;
        if prev.is_some_and(|p| g.codepoint <= p) {
            return Err(BuildError::NotStrictlyIncreasing { index });
        }
        prev = Some(g.codepoint);
        let want = format::bitmap_size(g.metrics.width, g.metrics.height);
        if u32::try_from(g.bitmap.len()) != Ok(want) {
            return Err(BuildError::BitmapSizeMismatch { index });
        }
        bitmap_len = bitmap_len.checked_add(want).ok_or(BuildError::TooLarge)?;
    }

    let header = Header {
        info: *info,
        glyph_count,
        bitmap_len,
    };
    let layout = header.layout().ok_or(BuildError::TooLarge)?;
    let total = usize::try_from(layout.total_len).map_err(|_| BuildError::TooLarge)?;

    let mut out = Vec::with_capacity(total);
    header.encode(layout, &mut out);
    let mut offset = 0u32;
    for g in glyphs {
        let len = g.bitmap.len() as u32;
        Record {
            codepoint: u32::from(g.codepoint),
            bitmap_offset: offset,
            bitmap_len: len,
            metrics: g.metrics,
        }
        .encode(&mut out);
        offset += len;
    }
    for g in glyphs {
        out.extend_from_slice(&g.bitmap);
    }
    Ok(out)
}

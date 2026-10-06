use core::mem::{size_of, size_of_val};

use crate::{
    FontError, FontInfo, Glyph, Metrics, PackReader, ReadAt, bitmap_size, missing_glyph_metrics,
    render_missing_glyph,
};

/// Caller-reserved metadata for one distinct page codepoint.
#[derive(Clone, Copy, Default)]
pub struct PageGlyphSlot {
    codepoint: char,
    metrics: Option<Metrics>,
    bitmap_start: usize,
    bitmap_end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStorageError {
    SizeOverflow,
    BudgetExceeded { needed: usize, budget: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationError<E> {
    MetadataFull { needed: usize, capacity: usize },
    BitmapFull { needed: usize, capacity: usize },
    Font(FontError<E>),
}

/// A prepared page whose immutable glyph views need no font source.
///
/// Storage handles must expose their entire reserved capacity through `AsRef`
/// and retain the same length and backing memory for this cache's lifetime.
/// Slices and boxed slices satisfy this contract; a shortened `Vec` does not.
/// The budget includes both complete buffers and this object's bookkeeping.
pub struct PageCache<S, B> {
    slots: S,
    bitmap: B,
    storage_bytes: usize,
    len: usize,
    info: Option<FontInfo>,
}

impl<S, B> PageCache<S, B>
where
    S: AsRef<[PageGlyphSlot]> + AsMut<[PageGlyphSlot]>,
    B: AsRef<[u8]> + AsMut<[u8]>,
{
    pub fn new(slots: S, bitmap: B, budget: usize) -> Result<Self, CacheStorageError> {
        let needed = size_of::<Self>()
            .checked_add(size_of_val(slots.as_ref()))
            .and_then(|size| size.checked_add(bitmap.as_ref().len()))
            .ok_or(CacheStorageError::SizeOverflow)?;
        if needed > budget {
            return Err(CacheStorageError::BudgetExceeded { needed, budget });
        }
        Ok(Self {
            slots,
            bitmap,
            storage_bytes: needed,
            len: 0,
            info: None,
        })
    }

    pub fn storage_bytes(&self) -> usize {
        self.storage_bytes
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn info(&self) -> Option<FontInfo> {
        self.info
    }

    /// Replace the page, publishing it only when every glyph is ready.
    /// Any failure leaves both old and partial page data invisible.
    pub fn prepare<R: ReadAt>(
        &mut self,
        reader: &mut PackReader<R>,
        codepoints: &[char],
    ) -> Result<(), PreparationError<R::Error>> {
        self.len = 0;
        self.info = None;
        let info = reader.info();
        let mut count = 0usize;
        let mut bitmap_end = 0usize;
        let slot_capacity = self.slots.as_ref().len();
        let bitmap_capacity = self.bitmap.as_ref().len();

        for &codepoint in codepoints {
            if self
                .slots
                .as_ref()
                .iter()
                .take(count)
                .any(|slot| slot.codepoint == codepoint)
            {
                continue;
            }
            let metadata_full = PreparationError::MetadataFull {
                needed: count.saturating_add(1),
                capacity: slot_capacity,
            };
            if count >= slot_capacity {
                return Err(metadata_full);
            }

            let found = reader.find(codepoint).map_err(PreparationError::Font)?;
            let metrics = found
                .as_ref()
                .map_or_else(|| missing_glyph_metrics(&info), |glyph| glyph.metrics);
            let bitmap_len = found.as_ref().map_or_else(
                || bitmap_size(metrics.width, metrics.height),
                |glyph| glyph.bitmap_len,
            );
            let end = usize::try_from(bitmap_len)
                .ok()
                .and_then(|size| bitmap_end.checked_add(size))
                .ok_or(PreparationError::BitmapFull {
                    needed: usize::MAX,
                    capacity: bitmap_capacity,
                })?;
            let bitmap_full = || PreparationError::BitmapFull {
                needed: end,
                capacity: bitmap_capacity,
            };
            let out = self
                .bitmap
                .as_mut()
                .get_mut(bitmap_end..end)
                .ok_or_else(bitmap_full)?;
            match found {
                Some(glyph) => {
                    reader
                        .read_bitmap(&glyph, out)
                        .map_err(PreparationError::Font)?;
                }
                None => {
                    render_missing_glyph(&info, out).ok_or_else(bitmap_full)?;
                }
            }
            let slot = self.slots.as_mut().get_mut(count).ok_or(metadata_full)?;
            *slot = PageGlyphSlot {
                codepoint,
                metrics: Some(metrics),
                bitmap_start: bitmap_end,
                bitmap_end: end,
            };
            count += 1;
            bitmap_end = end;
        }

        self.len = count;
        self.info = Some(info);
        Ok(())
    }

    pub fn get(&self, codepoint: char) -> Option<Glyph<'_>> {
        let slot = self
            .slots
            .as_ref()
            .get(..self.len)?
            .iter()
            .find(|slot| slot.codepoint == codepoint)?;
        Some(Glyph {
            metrics: slot.metrics?,
            bitmap: self
                .bitmap
                .as_ref()
                .get(slot.bitmap_start..slot.bitmap_end)?,
        })
    }
}

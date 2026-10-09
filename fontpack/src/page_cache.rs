use core::mem::{size_of, size_of_val};

use crate::{
    FontError, FontInfo, Glyph, GlyphRef, Metrics, PackReader, ReadAt, bitmap_size,
    missing_glyph_metrics, render_missing_glyph,
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
    fill: Fill,
}

// progress of a preparation that may be paused between glyphs; `len` and
// `info` stay unset (the page invisible) until the last glyph is in
#[derive(Clone, Copy, Default)]
struct Fill {
    next: usize,
    count: usize,
    bitmap_end: usize,
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
            fill: Fill::default(),
        })
    }

    /// Consume the prepared page so its caller can reuse or resize backing storage.
    /// Published glyph views borrow the cache, so none can survive this transfer.
    pub fn into_storage(self) -> (S, B) {
        (self.slots, self.bitmap)
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
        self.prepare_located(reader, codepoints, |_| None)
    }

    /// `prepare` for callers that already looked codepoints up in this pack:
    /// `located` returns `Some(found)` for a known lookup (`Some(None)` is a
    /// known absence) and `None` to search the index here.
    pub fn prepare_located<R: ReadAt>(
        &mut self,
        reader: &mut PackReader<R>,
        codepoints: &[char],
        located: impl FnMut(char) -> Option<Option<GlyphRef>>,
    ) -> Result<(), PreparationError<R::Error>> {
        self.begin();
        self.resume_located(reader, codepoints, located, || true)
            .map(|_| ())
    }

    /// Start a preparation that `resume_located` carries out in pieces. The
    /// old page is invisible from here on.
    pub fn begin(&mut self) {
        self.len = 0;
        self.info = None;
        self.fill = Fill::default();
    }

    /// Whether this cache holds an unfinished preparation of exactly the first
    /// glyphs of `codepoints` (distinct, in order), so `resume_located` with the
    /// same list continues it.
    pub fn resumes(&self, codepoints: &[char]) -> bool {
        let Fill { next, count, .. } = self.fill;
        self.info.is_none()
            && next > 0
            && next == count
            && next < codepoints.len()
            && self.slots.as_ref().get(..count).is_some_and(|slots| {
                slots
                    .iter()
                    .zip(codepoints)
                    .all(|(slot, &c)| slot.codepoint == c)
            })
    }

    /// Glyphs of the list taken so far by the current preparation.
    pub fn filled(&self) -> usize {
        self.fill.next
    }

    /// Continue the preparation `begin` started. `more` is asked after every
    /// glyph that is not the last; `false` pauses it (`Ok(false)`, the page
    /// still invisible, at least one glyph taken per call). `Ok(true)` means the
    /// page is published. After an `Err` the preparation must be started anew.
    pub fn resume_located<R: ReadAt>(
        &mut self,
        reader: &mut PackReader<R>,
        codepoints: &[char],
        mut located: impl FnMut(char) -> Option<Option<GlyphRef>>,
        mut more: impl FnMut() -> bool,
    ) -> Result<bool, PreparationError<R::Error>> {
        let info = reader.info();
        let slot_capacity = self.slots.as_ref().len();
        let bitmap_capacity = self.bitmap.as_ref().len();
        self.len = 0;
        self.info = None;

        while let Some(&codepoint) = codepoints.get(self.fill.next) {
            let Fill {
                count, bitmap_end, ..
            } = self.fill;
            let duplicate = self
                .slots
                .as_ref()
                .iter()
                .take(count)
                .any(|slot| slot.codepoint == codepoint);
            if !duplicate {
                let metadata_full = PreparationError::MetadataFull {
                    needed: count.saturating_add(1),
                    capacity: slot_capacity,
                };
                if count >= slot_capacity {
                    return Err(metadata_full);
                }

                let found = match located(codepoint) {
                    Some(found) => found,
                    None => reader.find(codepoint).map_err(PreparationError::Font)?,
                };
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
                self.fill.count = count + 1;
                self.fill.bitmap_end = end;
            }
            self.fill.next += 1;
            if self.fill.next < codepoints.len() && !more() {
                return Ok(false);
            }
        }

        self.len = self.fill.count;
        self.info = Some(info);
        Ok(true)
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

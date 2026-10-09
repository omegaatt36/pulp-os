// Small least-recently-used cache of decoded images.
//
// HAL-free and allocation-free like `chapter_ring`: the cache owns opaque
// payloads (a decoded bitmap in the firmware) and accounts their bytes. The
// firmware's payloads are `ImageData` class buffers, so the cache is class
// accounted by construction; `LruConfig::budget` keeps it a small share of that
// class (the decoder scratch of an image in flight comes out of the same class).
//
// Identity. An image is found by the persistent source of the book, a 64-bit
// hash of the resolved image path and the decode budget it was produced for
// (width and height the decoder was asked to fit). The same picture requested at
// the inline budget and at the full-screen budget are two entries; a picture of
// another book, or of a replaced book, never matches.

use crate::memory::{KIB, PSRAM_LARGE_MIN_BYTES, PsramStatus};
use crate::source_id::SourceId;

/// Most images held at once.
pub const MAX_ENTRIES: usize = 8;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LruConfig {
    /// Sum of the cached image bytes; 0 disables the cache.
    pub budget: usize,
    /// Largest single image.
    pub item_max: usize,
}

impl LruConfig {
    pub const OFF: LruConfig = LruConfig {
        budget: 0,
        item_max: 0,
    };
    /// HR8: 384 KiB (eight full-screen 800 x 480 bitmaps are 48,000 B each).
    pub const HR8: LruConfig = LruConfig {
        budget: 384 * KIB,
        item_max: 64 * KIB,
    };

    /// X4, HR2 and degraded builds keep no decoded images: their image class
    /// has no room to spare.
    pub const fn for_status(status: PsramStatus) -> LruConfig {
        match status {
            PsramStatus::Ready { bytes } if bytes >= PSRAM_LARGE_MIN_BYTES => Self::HR8,
            _ => Self::OFF,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ImageKey {
    pub source: SourceId,
    /// FNV-1a 64 of the resolved path inside the archive.
    pub path: u64,
    /// Width and height the decode was asked to fit.
    pub max_w: u16,
    pub max_h: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LruRefusal {
    Disabled,
    Empty,
    TooLarge,
    /// An unrelated source: the cache is aimed at another book.
    WrongSource,
}

struct Entry<T> {
    key: ImageKey,
    bytes: usize,
    stamp: u32,
    value: T,
}

pub struct ImageLru<T> {
    cfg: LruConfig,
    source: SourceId,
    clock: u32,
    used: usize,
    slots: [Option<Entry<T>>; MAX_ENTRIES],
}

impl<T> ImageLru<T> {
    pub const fn new(cfg: LruConfig) -> Self {
        Self {
            cfg,
            source: SourceId::NONE,
            clock: 0,
            used: 0,
            slots: [const { None }; MAX_ENTRIES],
        }
    }

    pub const fn config(&self) -> LruConfig {
        self.cfg
    }

    pub const fn used(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Aim the cache at `source`; another source drops every image.
    pub fn retarget(&mut self, source: SourceId) {
        if source != self.source {
            self.clear();
            self.source = source;
        }
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.used = 0;
        self.source = SourceId::NONE;
    }

    fn tick(&mut self) -> u32 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    /// The image of `key`, marked most recently used.
    pub fn get(&mut self, key: &ImageKey) -> Option<&T> {
        if key.source != self.source {
            return None;
        }
        let stamp = self.tick();
        let entry = self.slots.iter_mut().flatten().find(|e| e.key == *key)?;
        entry.stamp = stamp;
        Some(&entry.value)
    }

    pub fn contains(&self, key: &ImageKey) -> bool {
        key.source == self.source && self.slots.iter().flatten().any(|e| e.key == *key)
    }

    fn evict_oldest(&mut self) -> bool {
        // stamps wrap; compare by age against the clock
        let clock = self.clock;
        let oldest = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|e| (i, clock.wrapping_sub(e.stamp))))
            .max_by_key(|&(_, age)| age)
            .map(|(i, _)| i);
        match oldest {
            Some(i) => {
                let e = self.slots[i].take().expect("positioned");
                self.used -= e.bytes;
                true
            }
            None => false,
        }
    }

    /// Cache `value` (`bytes` of image data) for `key`, evicting the least
    /// recently used images until it fits. Returns how many were evicted. An
    /// image that can never fit is refused and handed back.
    pub fn insert(
        &mut self,
        key: ImageKey,
        bytes: usize,
        value: T,
    ) -> Result<usize, (LruRefusal, T)> {
        if self.cfg.budget == 0 {
            return Err((LruRefusal::Disabled, value));
        }
        if key.source != self.source || self.source.is_none() {
            return Err((LruRefusal::WrongSource, value));
        }
        if bytes == 0 {
            return Err((LruRefusal::Empty, value));
        }
        if bytes > self.cfg.item_max || bytes > self.cfg.budget {
            return Err((LruRefusal::TooLarge, value));
        }
        // an entry of the same key is replaced, not duplicated
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|e| e.key == key))
        {
            let e = self.slots[i].take().expect("positioned");
            self.used -= e.bytes;
        }
        let mut evicted = 0;
        while self.used + bytes > self.cfg.budget || self.len() == MAX_ENTRIES {
            if !self.evict_oldest() {
                break;
            }
            evicted += 1;
        }
        let stamp = self.tick();
        let free = self
            .slots
            .iter()
            .position(Option::is_none)
            .expect("an entry was evicted for a full table");
        self.used += bytes;
        self.slots[free] = Some(Entry {
            key,
            bytes,
            stamp,
            value,
        });
        Ok(evicted)
    }
}

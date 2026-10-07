// large, long-lived byte buffers (chapter text, decoded images, zip central
// directory): the one place where the placement differs between the boards
//
// X4: plain heap `Vec<u8>` (no PSRAM; behaviour and code are as they were).
//
// OnePage C61: budget-checked block from `board_c61::memory::alloc`, which
// puts PSRAM-capable classes in PSRAM while it is `Ready` and falls back to
// internal RAM with the (smaller, X4-sized) degraded limits otherwise. A request
// the budget refuses is an `Err`, which the callers turn into their existing
// "too large / skipped" paths; it never panics. DMA, ISR and runtime data are
// not expressible here (`BufClass` has no such variant).
//
// Contents are zero-initialised on both boards (X4 `resize(n, 0)` did the same).

use core::ops::{Deref, DerefMut};

/// What the buffer holds. Maps to the PSRAM budget classes on the C61.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufClass {
    ChapterText,
    ImageData,
    ZipToc,
    FontGlyphs,
}

/// C61 PSRAM budget of the `FontGlyphs` class; callers size their worst case
/// against it on every board.
pub const FONT_GLYPHS_PSRAM_BYTES: usize = pulp_board_logic::memory::PSRAM_FONT_GLYPHS_BYTES;

/// The allocation failed (heap exhausted, or over the memory budget).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufError;

/// Layout-aware, exclusively owned decoder scratch, with fallible allocation.
/// C61 charges it to the requested class and places it using the board policy.
pub struct DecoderScratch {
    #[cfg(feature = "board-x4")]
    ptr: core::ptr::NonNull<u8>,
    #[cfg(feature = "board-x4")]
    layout: core::alloc::Layout,
    #[cfg(feature = "board-onepage-c61")]
    block: crate::board_c61::memory::MemBuf,
}
impl DecoderScratch {
    pub fn zeroed(class: BufClass, layout: core::alloc::Layout) -> Result<Self, BufError> {
        if layout.size() == 0 {
            return Err(BufError);
        }
        #[cfg(feature = "board-x4")]
        {
            let _ = class;
            // SAFETY: valid nonempty layout; failure is returned, never aborts.
            let ptr = core::ptr::NonNull::new(unsafe { alloc::alloc::alloc_zeroed(layout) })
                .ok_or(BufError)?;
            Ok(Self { ptr, layout })
        }
        #[cfg(feature = "board-onepage-c61")]
        {
            use crate::board_c61::memory::{self, MemClass};
            let class = match class {
                BufClass::ChapterText => MemClass::ChapterText,
                BufClass::ImageData => MemClass::ImageData,
                BufClass::ZipToc => MemClass::ZipToc,
                BufClass::FontGlyphs => MemClass::FontGlyphs,
            };
            let block = memory::alloc(class, layout.size(), layout.align().max(16))
                .map_err(|_| BufError)?;
            Ok(Self { block })
        }
    }
    /// Stable pointer owned by this allocation, valid until it is dropped.
    pub fn ptr(&self) -> *mut u8 {
        #[cfg(feature = "board-x4")]
        {
            self.ptr.as_ptr()
        }
        #[cfg(feature = "board-onepage-c61")]
        {
            self.block.addr() as *mut u8
        }
    }
    pub fn len(&self) -> usize {
        #[cfg(feature = "board-x4")]
        {
            self.layout.size()
        }
        #[cfg(feature = "board-onepage-c61")]
        {
            self.block.len()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
// SAFETY: uniquely owned memory; both allocator implementations are synchronized.
unsafe impl Send for DecoderScratch {}
#[cfg(feature = "board-x4")]
impl Drop for DecoderScratch {
    fn drop(&mut self) {
        // SAFETY: this block came from the global allocator with this layout.
        unsafe { alloc::alloc::dealloc(self.ptr.as_ptr(), self.layout) }
    }
}

#[cfg(feature = "board-x4")]
mod imp {
    use alloc::vec::Vec;

    use super::{BufClass, BufError};

    pub struct BigBuf(Vec<u8>);

    impl BigBuf {
        pub const fn empty() -> Self {
            Self(Vec::new())
        }

        pub fn zeroed(class: BufClass, len: usize) -> Result<Self, BufError> {
            let mut v = Vec::new();
            v.try_reserve_exact(len).map_err(|_| BufError)?;
            // Font cache budgets must see the full reserved backing, including
            // any extra capacity granted by the allocator.
            let exposed_len = if class == BufClass::FontGlyphs {
                v.capacity()
            } else {
                len
            };
            v.resize(exposed_len, 0);
            Ok(Self(v))
        }

        // takes over a heap block as it is (no copy)
        pub fn from_vec(v: Vec<u8>, _class: BufClass) -> Result<Self, BufError> {
            Ok(Self(v))
        }

        pub(super) fn as_slice(&self) -> &[u8] {
            &self.0
        }

        pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
            &mut self.0
        }
    }
}

#[cfg(feature = "board-onepage-c61")]
mod imp {
    use alloc::vec::Vec;

    use super::{BufClass, BufError};
    use crate::board_c61::memory::{self, MemBuf, MemClass};

    const ALIGN: usize = 16;

    pub struct BigBuf(Option<MemBuf>);

    impl BigBuf {
        pub const fn empty() -> Self {
            Self(None)
        }

        pub fn zeroed(class: BufClass, len: usize) -> Result<Self, BufError> {
            if len == 0 {
                return Ok(Self(None));
            }
            let class = match class {
                BufClass::ChapterText => MemClass::ChapterText,
                BufClass::ImageData => MemClass::ImageData,
                BufClass::ZipToc => MemClass::ZipToc,
                BufClass::FontGlyphs => MemClass::FontGlyphs,
            };
            match memory::alloc(class, len, ALIGN) {
                Ok(b) => Ok(Self(Some(b))),
                Err(e) => {
                    log::warn!("bigbuf: {:?} {} B refused: {:?}", class, len, e);
                    memory::log_classes();
                    Err(BufError)
                }
            }
        }

        // copies the heap block into a budgeted one and frees the original
        pub fn from_vec(v: Vec<u8>, class: BufClass) -> Result<Self, BufError> {
            let mut b = Self::zeroed(class, v.len())?;
            b.as_mut_slice().copy_from_slice(&v);
            Ok(b)
        }

        pub(super) fn as_slice(&self) -> &[u8] {
            match &self.0 {
                Some(b) => b.as_slice(),
                None => &[],
            }
        }

        pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
            match &mut self.0 {
                Some(b) => b.as_mut_slice(),
                None => &mut [],
            }
        }
    }
}

pub use imp::BigBuf;

impl BigBuf {
    /// Make sure the buffer is at least `len` bytes (a smaller one is replaced
    /// by a fresh zeroed one; existing contents are not kept).
    pub fn ensure_len(&mut self, class: BufClass, len: usize) -> Result<(), BufError> {
        if self.len() < len {
            *self = Self::zeroed(class, len)?;
        }
        Ok(())
    }
}

impl Default for BigBuf {
    fn default() -> Self {
        Self::empty()
    }
}

impl Deref for BigBuf {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for BigBuf {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for BigBuf {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}
impl AsMut<[u8]> for BigBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

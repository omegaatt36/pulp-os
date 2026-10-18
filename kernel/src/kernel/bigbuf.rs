// large, long-lived byte buffers (chapter text, decoded images, zip central
// directory): the one place where the placement differs between the boards
//
// Budget-checked block from `board_c61::memory::alloc`, which
// puts PSRAM-capable classes in PSRAM while it is `Ready` and falls back to
// internal RAM with the (smaller) degraded limits otherwise. A request
// the budget refuses is an `Err`, which the callers turn into their existing
// "too large / skipped" paths; it never panics. DMA, ISR and runtime data are
// not expressible here (`BufClass` has no such variant).
//
// Contents are zero-initialised. On HR8, purpose accounting is separate from
// lifetime admission: resident state, isolated scratch, or shared cache.

use core::ops::{Deref, DerefMut};

/// What the buffer holds. Maps to the PSRAM budget classes on the C61.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufClass {
    ChapterText,
    ImageData,
    PageTable,
    DisplayFrame,
    StorageCache,
    ZipToc,
    FontGlyphs,
    FontIndex,
    /// Wi-Fi upload session scratch (directory listing, HTTP and TCP buffers).
    NetScratch,
}

impl BufClass {
    fn memory_class(self) -> pulp_board_logic::memory::MemClass {
        use pulp_board_logic::memory::MemClass;
        match self {
            Self::ChapterText => MemClass::ChapterText,
            Self::ImageData => MemClass::ImageData,
            Self::PageTable => MemClass::PageTable,
            Self::DisplayFrame => MemClass::DisplayFrame,
            Self::StorageCache => MemClass::StorageCache,
            Self::ZipToc => MemClass::ZipToc,
            Self::FontGlyphs | Self::FontIndex => MemClass::FontGlyphs,
            Self::NetScratch => MemClass::NetScratch,
        }
    }
    fn kind(self) -> pulp_board_logic::memory::AllocationKind {
        use pulp_board_logic::memory::AllocationKind;
        if matches!(self, Self::FontIndex) {
            AllocationKind::Cache
        } else {
            AllocationKind::for_class(self.memory_class())
        }
    }
}

/// C61 PSRAM budget of the `FontGlyphs` class; callers size their worst case
/// against it on every board.
pub const FONT_GLYPHS_PSRAM_BYTES: usize = pulp_board_logic::memory::PSRAM_FONT_GLYPHS_BYTES;

/// The allocation failed (heap exhausted, or over the memory budget).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufError(pub pulp_board_logic::memory::MemError);

impl BufError {
    pub const fn as_str(self) -> &'static str {
        use pulp_board_logic::memory::MemError;
        match self.0 {
            MemError::ClassLimit { .. } => "memory class limit",
            MemError::PoolExhausted { .. } => "memory pool exhausted",
            MemError::OutOfMemory => "memory allocator exhausted",
            MemError::HeapExhausted {
                heap: pulp_board_logic::memory::HeapKind::Internal,
            } => "internal allocator exhausted",
            MemError::HeapExhausted {
                heap: pulp_board_logic::memory::HeapKind::Scratch,
            } => "scratch allocator exhausted",
            MemError::HeapExhausted { .. } => "memory allocator exhausted",
            MemError::KindLimit {
                kind: pulp_board_logic::memory::AllocationKind::Cache,
                ..
            } => "memory cache limit",
            MemError::KindLimit { .. } => "memory working set limit",
            _ => "invalid memory request",
        }
    }
}

/// Layout-aware, exclusively owned decoder scratch, with fallible allocation.
/// Charged to the requested class and placed using the board policy.
pub struct DecoderScratch {
    block: crate::board_c61::memory::MemBuf,
}
impl DecoderScratch {
    pub fn zeroed(class: BufClass, layout: core::alloc::Layout) -> Result<Self, BufError> {
        Self::zeroed_for(
            class,
            layout,
            pulp_board_logic::memory::AllocationKind::Scratch,
        )
    }
    fn zeroed_for(
        class: BufClass,
        layout: core::alloc::Layout,
        kind: pulp_board_logic::memory::AllocationKind,
    ) -> Result<Self, BufError> {
        if layout.size() == 0 {
            return Err(BufError(pulp_board_logic::memory::MemError::ZeroSize));
        }
        let block = crate::board_c61::memory::alloc_with_kind(
            class.memory_class(),
            kind,
            layout.size(),
            layout.align().max(16),
        )
        .map_err(BufError)?;
        Ok(Self { block })
    }
    /// Stable pointer owned by this allocation, valid until it is dropped.
    pub fn ptr(&self) -> *mut u8 {
        self.block.addr() as *mut u8
    }
    pub fn len(&self) -> usize {
        self.block.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
// SAFETY: uniquely owned memory; the allocator is synchronized.
unsafe impl Send for DecoderScratch {}

/// Fallible board-placed storage initialized from a valid `Copy` value.
/// Typed references are exposed only after every element has been written.
/// Empty and zero-sized element storage needs no allocation.
pub struct TypedBuf<T: Copy> {
    block: Option<DecoderScratch>,
    len: usize,
    marker: core::marker::PhantomData<T>,
}

impl<T: Copy> TypedBuf<T> {
    pub const fn empty() -> Self {
        Self {
            block: None,
            len: 0,
            marker: core::marker::PhantomData,
        }
    }

    pub fn filled(class: BufClass, len: usize, value: T) -> Result<Self, BufError> {
        // Also bound zero-sized element counts; all slice indices fit isize.
        if len > isize::MAX as usize {
            return Err(BufError(pulp_board_logic::memory::MemError::Overflow));
        }
        let layout = core::alloc::Layout::array::<T>(len)
            .map_err(|_| BufError(pulp_board_logic::memory::MemError::Overflow))?;
        let block = if layout.size() == 0 {
            None
        } else {
            Some(DecoderScratch::zeroed_for(class, layout, class.kind())?)
        };
        let mut storage = Self {
            block,
            len,
            marker: core::marker::PhantomData,
        };
        if core::mem::size_of::<T>() != 0 {
            for i in 0..len {
                // SAFETY: layout reserves aligned space for len T values. Write
                // valid values without reading the zeroed, possibly invalid T.
                unsafe { storage.ptr().add(i).write(value) };
            }
        }
        Ok(storage)
    }

    fn ptr(&mut self) -> *mut T {
        self.block
            .as_ref()
            .map_or(core::ptr::NonNull::<T>::dangling().as_ptr(), |b| {
                b.ptr().cast()
            })
    }
}
impl<T: Copy> Default for TypedBuf<T> {
    fn default() -> Self {
        Self::empty()
    }
}
impl<T: Copy> Deref for TypedBuf<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        let ptr = self
            .block
            .as_ref()
            .map_or(core::ptr::NonNull::<T>::dangling().as_ptr(), |b| {
                b.ptr().cast()
            });
        // SAFETY: filled initialized all len elements; empty/ZST slices use
        // an aligned non-null dangling pointer. Allocation lives with self.
        unsafe { core::slice::from_raw_parts(ptr, self.len) }
    }
}
impl<T: Copy> DerefMut for TypedBuf<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: initialized as above, exclusively borrowed through self.
        unsafe { core::slice::from_raw_parts_mut(self.ptr(), self.len) }
    }
}
impl<T: Copy> AsRef<[T]> for TypedBuf<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}
impl<T: Copy> AsMut<[T]> for TypedBuf<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

mod imp {
    use alloc::vec::Vec;

    use super::{BufClass, BufError};
    use crate::board_c61::memory::{self, MemBuf};

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
            match memory::alloc_with_kind(class.memory_class(), class.kind(), len, ALIGN) {
                Ok(b) => Ok(Self(Some(b))),
                Err(e) => {
                    log::warn!("bigbuf: {:?} {} B refused: {:?}", class, len, e);
                    memory::log_classes();
                    Err(BufError(e))
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

/// A fallible vector whose backing follows an explicit board memory class.
/// Unused capacity is initialized too; slices expose only the live prefix.
pub struct ClassVec<T: Copy> {
    storage: TypedBuf<T>,
    len: usize,
    class: BufClass,
}

impl<T: Copy> ClassVec<T> {
    pub const fn new(class: BufClass) -> Self {
        Self {
            storage: TypedBuf::empty(),
            len: 0,
            class,
        }
    }

    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    pub fn try_reserve_exact(&mut self, additional: usize, fill: T) -> Result<(), BufError> {
        let needed = self
            .len
            .checked_add(additional)
            .ok_or(BufError(pulp_board_logic::memory::MemError::Overflow))?;
        if needed > self.capacity() {
            let mut next = TypedBuf::filled(self.class, needed, fill)?;
            next[..self.len].copy_from_slice(self);
            self.storage = next;
        }
        Ok(())
    }

    pub fn insert(&mut self, index: usize, value: T) {
        assert!(index <= self.len && self.len < self.capacity());
        self.storage.copy_within(index..self.len, index + 1);
        self.storage[index] = value;
        self.len += 1;
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut used = 0;
        for i in 0..self.len {
            if keep(&self.storage[i]) {
                self.storage[used] = self.storage[i];
                used += 1;
            }
        }
        self.len = used;
    }

    pub fn clear(&mut self) {
        self.storage = TypedBuf::empty();
        self.len = 0;
    }
}

impl<T: Copy> Deref for ClassVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.storage[..self.len]
    }
}
impl<T: Copy> DerefMut for ClassVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.storage[..self.len]
    }
}

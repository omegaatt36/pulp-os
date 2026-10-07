//! Owned, layout-aware decoder scratch. Allocation policy belongs to the caller.
use alloc::alloc::{alloc_zeroed, dealloc};
use core::{
    alloc::Layout,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};
use miniz_oxide::inflate::core::DecompressorOxide;

/// An exclusively owned allocation, freed by its implementation's `Drop`.
///
/// # Safety
/// `ptr()` must be stable, non-null and valid for `len()` initialized bytes
/// until drop, including when this owner moves. No other owner may access it.
pub unsafe trait ScratchStorage {
    /// Start of the exclusively owned allocation.
    fn ptr(&self) -> *mut u8;
    /// Number of initialized bytes available at the pointer.
    fn len(&self) -> usize;
}

/// Default scratch owner backed by the global heap.
pub struct HeapScratch {
    ptr: NonNull<u8>,
    layout: Layout,
}
impl HeapScratch {
    /// Allocate zeroed memory with the requested layout, or return an error.
    pub fn zeroed(layout: Layout) -> Result<Self, &'static str> {
        if layout.size() == 0 {
            return Err("scratch: empty layout");
        }
        // SAFETY: a nonempty valid Layout; null is returned as a recoverable error.
        let ptr = NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or("scratch: out of memory")?;
        Ok(Self { ptr, layout })
    }
}
// SAFETY: the block is exclusively owned and stable until deallocated on drop.
unsafe impl ScratchStorage for HeapScratch {
    fn ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
    fn len(&self) -> usize {
        self.layout.size()
    }
}
// SAFETY: uniquely owned allocation; no thread-local allocator state.
unsafe impl Send for HeapScratch {}
impl Drop for HeapScratch {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr.as_ptr(), self.layout) }
    }
}

/// Checked scratch bytes exposing the requested logical size.
pub struct ScratchBytes<S: ScratchStorage> {
    storage: S,
    len: usize,
}
impl<S: ScratchStorage> ScratchBytes<S> {
    /// Validate storage bounds/alignment and initialize requested bytes to zero.
    pub fn new<A>(allocate: &mut A, layout: Layout) -> Result<Self, &'static str>
    where
        A: FnMut(Layout) -> Result<S, &'static str>,
    {
        let storage = allocate(layout)?;
        if storage.len() < layout.size()
            || storage.ptr().is_null()
            || storage.ptr() as usize % layout.align() != 0
        {
            return Err("scratch: invalid allocation layout");
        }
        let mut bytes = Self {
            storage,
            len: layout.size(),
        };
        bytes.fill(0);
        Ok(bytes)
    }
}
impl<S: ScratchStorage> Deref for ScratchBytes<S> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.storage.ptr(), self.len) }
    }
}
impl<S: ScratchStorage> DerefMut for ScratchBytes<S> {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.storage.ptr(), self.len) }
    }
}

pub(crate) struct Decompressor<S: ScratchStorage>(ScratchBytes<S>);
impl<S: ScratchStorage> Decompressor<S> {
    pub fn new<A>(allocate: &mut A) -> Result<Self, &'static str>
    where
        A: FnMut(Layout) -> Result<S, &'static str>,
    {
        Ok(Self(ScratchBytes::new(
            allocate,
            Layout::new::<DecompressorOxide>(),
        )?))
    }
}
impl<S: ScratchStorage> Deref for Decompressor<S> {
    type Target = DecompressorOxide;
    fn deref(&self) -> &Self::Target {
        // SAFETY: layout checked, stable owned pointer, initialized to zero.
        // miniz_oxide 0.8's fields are integers/arrays and State::Start = 0;
        // its default constructor also initializes these fields to zero.
        unsafe { &*self.0.storage.ptr().cast() }
    }
}
impl<S: ScratchStorage> DerefMut for Decompressor<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0.storage.ptr().cast() }
    }
}

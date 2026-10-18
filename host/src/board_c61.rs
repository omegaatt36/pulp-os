// host stand-in for kernel/src/board_c61/memory.rs: the placement edge only.
// The kernel files (bigbuf, metadata, reader) call `memory::{alloc, status}`;
// here every block is a zeroed, aligned host-heap block and the PSRAM state is
// the power-on one (no PSRAM). Budgets and class limits are pulp-board-logic's
// and are tested there; the host fixtures are sized for an unbounded heap.
pub mod memory {
    use std::alloc::{Layout, alloc_zeroed, dealloc};
    use std::ptr::NonNull;

    pub use pulp_board_logic::memory::{MemClass, MemError, PsramStatus};

    pub fn status() -> PsramStatus {
        PsramStatus::NotInitialised
    }

    pub struct MemBuf {
        ptr: NonNull<u8>,
        layout: Layout,
    }

    // SAFETY: the block is exclusively owned.
    unsafe impl Send for MemBuf {}

    // SAFETY: shared references expose initialized bytes only; writes require &mut self.
    unsafe impl Sync for MemBuf {}

    impl MemBuf {
        pub fn addr(&self) -> usize {
            self.ptr.as_ptr() as usize
        }

        pub fn len(&self) -> usize {
            self.layout.size()
        }

        pub fn is_empty(&self) -> bool {
            self.layout.size() == 0
        }

        pub fn as_slice(&self) -> &[u8] {
            // SAFETY: `len` zero-initialised bytes, owned.
            unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.layout.size()) }
        }

        pub fn as_mut_slice(&mut self) -> &mut [u8] {
            // SAFETY: as above, and `&mut self` is unique.
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.layout.size()) }
        }
    }

    impl Drop for MemBuf {
        fn drop(&mut self) {
            // SAFETY: allocated in `alloc` with this layout.
            unsafe { dealloc(self.ptr.as_ptr(), self.layout) }
        }
    }

    pub fn alloc(_class: MemClass, size: usize, align: usize) -> Result<MemBuf, MemError> {
        if size == 0 {
            return Err(MemError::Overflow);
        }
        let layout = Layout::from_size_align(size, align).map_err(|_| MemError::Overflow)?;
        // SAFETY: non-zero size.
        let ptr = NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or(MemError::OutOfMemory)?;
        Ok(MemBuf { ptr, layout })
    }

    pub fn alloc_with_kind(
        class: MemClass,
        _kind: pulp_board_logic::memory::AllocationKind,
        size: usize,
        align: usize,
    ) -> Result<MemBuf, MemError> {
        alloc(class, size, align)
    }

    pub fn log_classes() {}
}

// host stand-in for kernel/src/board_c61/memory.rs: the placement edge only.
// The kernel files (bigbuf, metadata, reader) call `memory::{alloc, status}`;
// blocks are zeroed, aligned host-heap allocations. Most fixtures use an
// unbounded heap with the power-on PSRAM state; with_budget opts into the
// production admission rules and a specific PSRAM state for memory tests.
pub mod memory {
    use std::alloc::{Layout, alloc_zeroed, dealloc};
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex};

    use pulp_board_logic::memory::{AllocationKind, MemoryBudget, Reservation};
    pub use pulp_board_logic::memory::{MemClass, MemError, PsramStatus};

    type Budget = Arc<Mutex<MemoryBudget>>;
    thread_local! {
        static BUDGET: RefCell<Option<Budget>> = const { RefCell::new(None) };
    }

    /// Enforce production admission rules for allocations made in this scope.
    /// Existing reader fixtures retain the default unbounded host allocator.
    pub fn with_budget<T>(status: PsramStatus, wifi: bool, f: impl FnOnce(Budget) -> T) -> T {
        struct Restore(Option<Budget>);
        impl Drop for Restore {
            fn drop(&mut self) {
                BUDGET.with(|slot| *slot.borrow_mut() = self.0.take());
            }
        }
        let mut budget = MemoryBudget::for_build(wifi);
        budget.set_status(status).unwrap();
        let budget = Arc::new(Mutex::new(budget));
        let _restore = Restore(BUDGET.with(|slot| slot.replace(Some(budget.clone()))));
        f(budget)
    }

    pub fn status() -> PsramStatus {
        BUDGET.with(|slot| {
            slot.borrow()
                .as_ref()
                .map_or(PsramStatus::NotInitialised, |budget| {
                    budget.lock().unwrap().status()
                })
        })
    }

    pub struct MemBuf {
        ptr: NonNull<u8>,
        layout: Layout,
        reservation: Option<Box<(Budget, Reservation)>>,
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
            if let Some(lease) = self.reservation.take() {
                let (budget, reservation) = *lease;
                budget.lock().unwrap().release(reservation).unwrap();
            }
        }
    }

    pub fn alloc(class: MemClass, size: usize, align: usize) -> Result<MemBuf, MemError> {
        alloc_with_kind(class, AllocationKind::for_class(class), size, align)
    }

    pub fn alloc_with_kind(
        class: MemClass,
        kind: AllocationKind,
        size: usize,
        align: usize,
    ) -> Result<MemBuf, MemError> {
        if size == 0 {
            return Err(MemError::Overflow);
        }
        let layout = Layout::from_size_align(size, align).map_err(|_| MemError::Overflow)?;
        let budget = BUDGET.with(|slot| slot.borrow().clone());
        let reservation = if let Some(budget) = budget {
            let mut state = budget.lock().unwrap();
            let region = state.region_for(class);
            let reservation = state.reserve_in_with_kind(region, class, kind, size, align)?;
            drop(state);
            Some((budget, reservation))
        } else {
            None
        };
        // SAFETY: non-zero size.
        let Some(ptr) = NonNull::new(unsafe { alloc_zeroed(layout) }) else {
            if let Some((budget, reservation)) = reservation {
                budget.lock().unwrap().release(reservation).unwrap();
            }
            return Err(MemError::OutOfMemory);
        };
        Ok(MemBuf {
            ptr,
            layout,
            reservation: reservation.map(Box::new),
        })
    }

    pub fn log_classes() {}
}

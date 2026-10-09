use core::cell::{Cell, RefCell, RefMut};
use core::ops::{Deref, DerefMut};

/// A manager whose close releases its handle even when flushing fails.
pub trait CloseHandle {
    type Handle: Copy;
    type Error;

    fn close_handle(&mut self, handle: Self::Handle) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy)]
enum Slot<H> {
    Free,
    Reserved,
    Open(H),
    Pending(H),
}

/// Interior mutability with bounded, reserved ownership for reentrant closes.
/// Reserve before opening a handle: a busy close then always has room to keep
/// it until the outer borrow ends. The cells and tokens are deliberately !Sync.
pub struct CloseCell<T: CloseHandle, const N: usize> {
    inner: RefCell<T>,
    slots: [Cell<Slot<T::Handle>>; N],
}

impl<T: CloseHandle, const N: usize> CloseCell<T, N> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: RefCell::new(inner),
            slots: core::array::from_fn(|_| Cell::new(Slot::Free)),
        }
    }

    /// Failure leaves the manager untouched; no untracked handle is opened.
    pub fn reserve(&self) -> Option<CloseToken<'_, T, N>> {
        let slot = self
            .slots
            .iter()
            .position(|s| matches!(s.get(), Slot::Free))?;
        self.slots[slot].set(Slot::Reserved);
        Some(CloseToken {
            owner: self,
            slot: Some(slot),
        })
    }

    pub fn borrow_mut(&self) -> CloseBorrow<'_, T, N> {
        self.guard(self.inner.borrow_mut())
    }

    pub fn try_borrow_mut(&self) -> Option<CloseBorrow<'_, T, N>> {
        self.inner
            .try_borrow_mut()
            .ok()
            .map(|inner| self.guard(inner))
    }

    fn guard<'a>(&'a self, mut inner: RefMut<'a, T>) -> CloseBorrow<'a, T, N> {
        self.drain(&mut inner);
        CloseBorrow { owner: self, inner }
    }

    fn drain(&self, inner: &mut T) {
        for slot in &self.slots {
            if let Slot::Pending(handle) = slot.get() {
                slot.set(Slot::Free);
                // The manager handles error cleanup/reporting. Retrying would
                // close a released handle after a failed metadata flush.
                let _ = inner.close_handle(handle);
            }
        }
    }
}

pub struct CloseBorrow<'a, T: CloseHandle, const N: usize> {
    owner: &'a CloseCell<T, N>,
    inner: RefMut<'a, T>,
}

impl<T: CloseHandle, const N: usize> Deref for CloseBorrow<'_, T, N> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: CloseHandle, const N: usize> DerefMut for CloseBorrow<'_, T, N> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: CloseHandle, const N: usize> Drop for CloseBorrow<'_, T, N> {
    fn drop(&mut self) {
        // The caller's &mut T is no longer accessible. Use the existing
        // exclusive borrow, so draining neither reborrows nor aliases it.
        self.owner.drain(&mut self.inner);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CloseError<E> {
    Deferred,
    Failed(E),
}

/// One non-cloneable handle owner, tied to the manager that opened it.
pub struct CloseToken<'a, T: CloseHandle, const N: usize> {
    owner: &'a CloseCell<T, N>,
    slot: Option<usize>,
}

impl<T: CloseHandle, const N: usize> CloseToken<'_, T, N> {
    /// Attach once. On misuse, return ownership of the unaccepted raw handle.
    pub fn attach(&mut self, handle: T::Handle) -> Result<(), T::Handle> {
        if let Some(slot) = self.slot {
            if matches!(self.owner.slots[slot].get(), Slot::Reserved) {
                self.owner.slots[slot].set(Slot::Open(handle));
                return Ok(());
            }
        }
        Err(handle)
    }

    pub fn handle(&self) -> Option<T::Handle> {
        match self.owner.slots[self.slot?].get() {
            Slot::Open(handle) => Some(handle),
            _ => None,
        }
    }

    pub fn close(&mut self) -> Result<(), CloseError<T::Error>> {
        let Some(slot) = self.slot.take() else {
            return Ok(());
        };
        let Slot::Open(handle) = self.owner.slots[slot].get() else {
            self.owner.slots[slot].set(Slot::Free);
            return Ok(());
        };
        let Some(mut inner) = self.owner.try_borrow_mut() else {
            self.owner.slots[slot].set(Slot::Pending(handle));
            return Err(CloseError::Deferred);
        };
        self.owner.slots[slot].set(Slot::Free);
        inner.close_handle(handle).map_err(CloseError::Failed)
    }
}

impl<T: CloseHandle, const N: usize> Drop for CloseToken<'_, T, N> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

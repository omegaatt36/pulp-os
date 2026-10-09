// a value shared by several owners, reached only through short critical-section borrows

use core::cell::RefCell;
use critical_section::Mutex;

pub struct Shared<T>(Mutex<RefCell<T>>);

impl<T> Shared<T> {
    pub const fn new(value: T) -> Self {
        Self(Mutex::new(RefCell::new(value)))
    }

    // runs `f` on the value with interrupts held off. `f` must be short, must
    // not allocate, read storage or drop large payloads, and must not borrow
    // the same `Shared` again (that panics)
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        critical_section::with(|cs| f(&mut self.0.borrow_ref_mut(cs)))
    }
}

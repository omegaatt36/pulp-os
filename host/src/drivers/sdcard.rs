// host stand-in for kernel/src/drivers/sdcard.rs: the object every storage call
// borrows, here the virtual card of crate::storage.
use crate::storage::VirtualStorage;

pub struct SdStorage {
    pub card: VirtualStorage,
    mounted: bool,
}

impl SdStorage {
    pub fn new(card: VirtualStorage) -> Self {
        Self { card, mounted: true }
    }

    pub fn set_mounted(&mut self, mounted: bool) {
        self.mounted = mounted;
    }

    pub fn borrow_card(&self) -> Option<&VirtualStorage> {
        self.mounted.then_some(&self.card)
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }
}

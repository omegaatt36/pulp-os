// host stand-in for kernel/src/drivers/sdcard.rs: the object every storage call
// borrows, here the virtual card of crate::storage.
use crate::storage::VirtualStorage;
use core::cell::Cell;
use pulp_board_logic::calendar::{DateTime, Source};

pub struct SdStorage {
    pub card: VirtualStorage,
    mounted: bool,
    local_time: Cell<Option<DateTime>>,
}

impl SdStorage {
    pub fn new(card: VirtualStorage) -> Self {
        Self {
            card,
            mounted: true,
            local_time: Cell::new(None),
        }
    }

    pub fn set_local_time(&self, date: DateTime) {
        self.local_time.set(Some(date));
    }

    pub fn local_time(&self) -> Option<DateTime> {
        self.local_time.get()
    }

    pub fn current_datetime(&self) -> Option<(DateTime, Source)> {
        self.local_time().map(|date| (date, Source::Browser))
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

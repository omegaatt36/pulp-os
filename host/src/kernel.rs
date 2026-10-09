// host stand-in for kernel/src/kernel/mod.rs: the files the apps talk to are the
// real ones (app, bigbuf, bookmarks, config, dir_cache, handle, timing, wake,
// work_queue); `Kernel` keeps the fields handle.rs uses, over the virtual card.
// The scheduler, tasks, console and the display fields stay firmware-only.
#[path = "../../kernel/src/kernel/app.rs"]
#[allow(dead_code)]
pub mod app;
#[path = "../../kernel/src/kernel/bigbuf.rs"]
#[allow(dead_code)]
pub mod bigbuf;
#[path = "../../kernel/src/kernel/bookmarks.rs"]
#[allow(dead_code)]
pub mod bookmarks;
#[path = "../../kernel/src/kernel/config.rs"]
#[allow(dead_code)]
pub mod config;
#[path = "../../kernel/src/kernel/dir_cache.rs"]
pub mod dir_cache;
#[path = "../../kernel/src/kernel/handle.rs"]
pub mod handle;
#[path = "../../kernel/src/kernel/idle.rs"]
#[allow(dead_code)]
pub mod idle;
#[path = "../../kernel/src/kernel/timing.rs"]
#[allow(dead_code)]
pub mod timing;
#[path = "../../kernel/src/kernel/wake.rs"]
pub mod wake;
#[path = "../../kernel/src/kernel/work_queue.rs"]
#[allow(dead_code)]
pub mod work_queue;

#[path = "../../kernel/src/kernel/rtc_session.rs"]
pub mod rtc_session;
#[path = "../../kernel/src/kernel/storage_change.rs"]
pub mod storage_change;

pub use crate::drivers::storage::StorageError;
pub use crate::error::{Error, ErrorKind, Result, ResultExt};

pub use app::{
    App, AppContext, AppIdType, AppLayer, Launcher, NavEvent, PendingSetting, QuickAction,
    QuickActionKind, RECENT_FILE, Redraw, SessionData, Transition,
};
pub use bigbuf::{BigBuf, BufClass, BufError, FONT_GLYPHS_PSRAM_BYTES};
pub use bookmarks::BookmarkCache;
pub use handle::KernelHandle;
pub use wake::{uptime_secs, uptime_us};

use crate::drivers::sdcard::SdStorage;
use dir_cache::DirCache;

pub const DEFAULT_GHOST_CLEAR_EVERY: u32 = config::DEFAULT_GHOST_CLEAR as u32;

pub struct Kernel {
    pub(crate) sd: SdStorage,
    pub(crate) dir_cache: &'static mut DirCache,
    pub(crate) bm_cache: &'static mut BookmarkCache,
    pub(crate) sd_ok: bool,
    pub(crate) cached_battery_mv: u16,
}

impl Kernel {
    pub fn new(sd: SdStorage) -> Self {
        let sd_ok = sd.is_mounted();
        Self {
            sd,
            dir_cache: alloc::boxed::Box::leak(alloc::boxed::Box::new(DirCache::new())),
            bm_cache: alloc::boxed::Box::leak(alloc::boxed::Box::new(BookmarkCache::new())),
            sd_ok,
            cached_battery_mv: 4000,
        }
    }

    #[inline]
    pub fn handle(&mut self) -> KernelHandle<'_> {
        KernelHandle::new(self)
    }

    pub fn sd(&self) -> &SdStorage {
        &self.sd
    }

    // the card slot: false is a removed card (storage calls fail with NoCard)
    pub fn set_card_present(&mut self, present: bool) {
        self.sd.set_mounted(present);
    }

    // what the scheduler does at boot: load the bookmark cache from the card
    pub fn bookmarks_load(&mut self) {
        self.bm_cache.ensure_loaded(&self.sd);
    }

    // scheduler housekeeping (kernel/src/kernel/scheduler.rs:282-283): when
    // the flush timer is due, flush the bookmark cache to the card. The dirty
    // gate is BookmarkCache::flush's own first line, as in production.
    pub fn bookmarks_flush(&mut self) {
        self.bm_cache.flush(&self.sd);
    }
}

use super::app::AppLayer;
use super::{BookmarkCache, Kernel};
use crate::drivers::sdcard::SdStorage;

impl Kernel {
    pub fn replace_storage<A: AppLayer>(&mut self, sd: SdStorage, sd_ok: bool, apps: &mut A) {
        *self.bm_cache = BookmarkCache::new();
        self.dir_cache.invalidate();
        self.sd = sd;
        self.sd_ok = sd_ok;
        if sd_ok {
            self.bm_cache.ensure_loaded(&self.sd);
        }
        apps.storage_changed(&mut self.handle());
    }
}

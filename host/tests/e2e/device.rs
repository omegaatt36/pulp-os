// A whole device on the host: the production AppManager (Home, Files, Reader,
// Settings, quick menu, button mapper) over a Kernel and a virtual card, driven
// by hardware key events the way the scheduler drives it. Nothing here decides
// behaviour: `tap` / `hold` feed `AppManager::dispatch_event` and apply the
// transition it returns, `settle` runs `run_background` until the apps have
// nothing left to do, `reboot` and `sleep_and_wake` are the scheduler's boot
// and sleep sequences over the same card.
//
// The reader keeps process-wide work channels, so every test takes `serial()`.

use std::future::Future;
use std::pin::pin;
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use pulp_host::apps::files::FilesApp;
use pulp_host::apps::home::HomeApp;
use pulp_host::apps::manager::AppManager;
use pulp_host::apps::reader::ReaderApp;
use pulp_host::apps::settings::SettingsApp;
use pulp_host::apps::widgets::{ButtonFeedback, QuickMenu};
use pulp_host::apps::{AppId, Launcher, Redraw, Transition};
pub use pulp_host::board::Button as Key;
use pulp_host::board::Button;
use pulp_host::board::action::ButtonMapper;
use pulp_host::drivers::input::Event;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::kernel::{AppLayer, Kernel, SessionData};
use pulp_host::reader::run_image_worker;
use pulp_host::render::{Framebuffer, render_stitched};
use pulp_host::storage::VirtualStorage;

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

// background ticks one `settle` may spend before the device counts as hung
const SETTLE_TICKS: usize = 4000;

fn block_on(future: impl Future<Output = ()>) {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..1000 {
        if future.as_mut().poll(&mut cx) == Poll::Ready(()) {
            return;
        }
    }
    panic!("background tick did not finish");
}

pub struct Device {
    k: Kernel,
    pub apps: AppManager,
    renders: Vec<Redraw>,
}

impl Device {
    // power on with `card` in the slot: the scheduler's normal boot
    pub fn boot(card: VirtualStorage) -> Self {
        card.ensure_pulp_dir()
            .expect("card has _PULP/ the way the firmware boots it");
        Self::boot_from(SdStorage::new(card), None)
    }

    fn boot_from(sd: SdStorage, session: Option<&SessionData>) -> Self {
        let mut k = Kernel::new(sd);
        k.bookmarks_load();
        let mut apps = AppManager::new(
            Box::leak(Box::new(Launcher::new())),
            Box::leak(Box::new(HomeApp::new())),
            Box::leak(Box::new(FilesApp::new())),
            Box::leak(Box::new(ReaderApp::new())),
            Box::leak(Box::new(SettingsApp::new())),
            Box::leak(Box::new(QuickMenu::new())),
            Box::leak(Box::new(ButtonFeedback::new())),
            ButtonMapper::new(),
        );
        apps.load_eager_settings(&mut k.handle());
        apps.load_home_recent(&mut k.handle());
        let restored = session.is_some_and(|s| apps.apply_session(s, &mut k.handle()));
        if !restored {
            apps.enter_initial(&mut k.handle());
        }
        let mut device = Self {
            k,
            apps,
            renders: Vec::new(),
        };
        device.settle();
        device
    }

    // a key pressed and released
    pub fn tap(&mut self, key: Button) {
        self.event(Event::Press(key));
        self.event(Event::Release(key));
        self.settle();
    }

    pub fn taps(&mut self, keys: &[Button]) {
        for &key in keys {
            self.tap(key);
        }
    }

    // a key held past the long-press threshold, then released
    pub fn hold(&mut self, key: Button) {
        self.event(Event::Press(key));
        self.event(Event::LongPress(key));
        self.event(Event::Release(key));
        self.settle();
    }

    fn event(&mut self, event: Event) {
        let transition = {
            let mut h = self.k.handle();
            self.apps.dispatch_event(event, h.bookmark_cache_mut())
        };
        if transition != Transition::None {
            self.apps.apply_transition(transition, &mut self.k.handle());
        }
    }

    // The scheduler's loop until the apps are idle: background work, then the
    // refresh of a pending redraw, then the bookmark flush its timer does when
    // due. A refresh here is a full one (no waveform window for `prefetch`);
    // `refresh` is the partial one.
    pub fn settle(&mut self) {
        for _ in 0..SETTLE_TICKS {
            let pending = self.apps.background_pending();
            if pending {
                self.background();
            }
            if !self.render(false) && !pending {
                self.k.bookmarks_flush();
                return;
            }
        }
        panic!(
            "device did not settle in {SETTLE_TICKS} ticks (active {:?})",
            self.apps.active()
        );
    }

    // one background step, as the scheduler runs it before a refresh
    pub fn background(&mut self) {
        block_on(self.apps.run_background(&mut self.k.handle()));
        if self.apps.reader.is_epub() {
            run_image_worker();
        }
    }

    // a key pressed and released, with no scheduler step after it
    pub fn press(&mut self, key: Button) {
        self.event(Event::Press(key));
        self.event(Event::Release(key));
    }

    // a partial refresh of the pending redraw: the app's `prefetch` runs in the
    // panel's waveform window. False when nothing was pending.
    pub fn refresh(&mut self) -> bool {
        self.render(true)
    }

    fn render(&mut self, waveform: bool) -> bool {
        if !self.apps.has_redraw() {
            return false;
        }
        let redraw = self.apps.take_redraw();
        self.renders.push(redraw);
        if waveform {
            AppLayer::prefetch(&mut self.apps, &mut self.k.handle());
        }
        true
    }

    // the refreshes since the last call, oldest first
    pub fn take_renders(&mut self) -> Vec<Redraw> {
        std::mem::take(&mut self.renders)
    }

    // power off and on again with the same card
    pub fn reboot(mut self) -> Self {
        self.settle();
        Self::boot_from(self.k.into_sd(), None)
    }

    // deep sleep and wake: the session record the scheduler writes before
    // sleeping is applied at the next boot
    pub fn sleep_and_wake(mut self) -> Self {
        self.settle();
        let mut session = SessionData::home();
        self.apps.collect_session(&mut session);
        Self::boot_from(self.k.into_sd(), Some(&session))
    }

    pub fn active(&self) -> AppId {
        self.apps.active()
    }

    pub fn card(&self) -> &VirtualStorage {
        &self.k.sd().card
    }

    // _PULP/<name> on the card, None when absent
    pub fn pulp_file(&self, name: &str) -> Option<Vec<u8>> {
        let size = self.card().file_size_in_pulp(name).ok()? as usize;
        let mut bytes = vec![0; size];
        let n = self.card().read_chunk_in_pulp(name, 0, &mut bytes).ok()?;
        bytes.truncate(n);
        Some(bytes)
    }

    // the screen as the panel shows it: the firmware's own strips stitched
    // into a page (full-band against strip agreement is render.rs's subject)
    pub fn screen(&mut self) -> Framebuffer {
        self.apps.prepare_render(&mut self.k.handle());
        let apps = &self.apps;
        render_stitched(&|s| apps.draw(s)).frame
    }

    pub fn reader_book(&self) -> String {
        String::from_utf8_lossy(self.apps.reader.filename_bytes()).into_owned()
    }
}

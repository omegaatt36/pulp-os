// Test rig: drives the real ReaderApp the way the firmware's app manager and
// scheduler do (on_enter -> run `background` until the page is ready ->
// on_event), over the virtual card. No layout, wrapping or paging logic lives
// here: every page shown is laid out by src/apps/reader/*.
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

use embassy_executor::raw::Executor;

use crate::apps::probe;
use crate::apps::reader::ReaderApp;
use crate::apps::settings::SettingsApp;
use crate::apps::{App, AppContext};
use crate::board::action::ActionEvent;
use crate::drivers::sdcard::SdStorage;
use crate::drivers::strip::StripBuffer;
use crate::error::ErrorKind;
use crate::kernel::Kernel;
use crate::kernel::bookmarks::{BmListEntry, BookmarkSlot};
use crate::kernel::config::{self, SystemSettings};
use crate::storage::VirtualStorage;

pub use crate::apps::probe::{
    CHARS_PER_LINE, LineInfo, PAGE_BUF, Phase, QA_FONT_SIZE, QA_NEXT_CHAPTER, QA_PREV_CHAPTER,
    QA_TOC,
};
pub use crate::board::action::Action;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..1_000_000 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
    panic!("future did not complete");
}

// The firmware runs the image decoder in kernel::work_queue's `worker_task`, an
// embassy task on the main executor. Here the same task runs on a raw embassy
// executor that the rig polls after every `background` tick: the cooperative
// scheduling the firmware has when the main task awaits. The decoder is the one
// src/bin/main.rs registers at boot. Both live behind process-global channels
// (work_queue's statics), so the worker exists once per process, is created the
// first time an EPUB reader ticks (TXT / storage tests never start it), and
// tests that run an EPUB reader must not overlap.
//
// The raw executor needs its platform to provide the wake-up hook; waking is a
// no-op because the rig polls explicitly.
#[unsafe(export_name = "__pender")]
fn pender(_context: *mut ()) {}

struct ImageWorker(&'static Executor);

// the executor is only ever polled under the mutex below
unsafe impl Send for ImageWorker {}

static IMAGE_WORKER: OnceLock<Mutex<ImageWorker>> = OnceLock::new();

fn run_image_worker() {
    let worker = IMAGE_WORKER.get_or_init(|| {
        // src/bin/main.rs, after kernel.boot()
        crate::kernel::work_queue::register_image_decoder(crate::apps::reader::decode_work_image);
        let executor: &'static Executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
        executor
            .spawner()
            .spawn(crate::kernel::work_queue::worker_task().expect("spawn worker_task"));
        Mutex::new(ImageWorker(executor))
    });
    let worker = worker.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: serialised by the mutex; the executor is never dropped
    unsafe { worker.0.poll() };
}

pub struct Rig {
    k: Kernel,
    ctx: AppContext,
    app: Box<ReaderApp>,
    text_width_override: Option<u32>,
}

impl Rig {
    pub fn new(storage: VirtualStorage) -> Self {
        let mut k = Kernel::new(SdStorage::new(storage));
        // boot: the kernel loads the bookmark cache before any app runs
        k.bookmarks_load();
        Self {
            k,
            ctx: AppContext::new(),
            app: Box::new(ReaderApp::new()),
            text_width_override: None,
        }
    }

    pub fn storage(&self) -> &VirtualStorage {
        &self.k.sd().card
    }

    pub fn set_sd_ok(&mut self, mounted: bool) {
        self.k.sd_ok = mounted;
        self.k.sd.set_mounted(mounted);
    }

    pub fn bookmarks_dirty(&self) -> bool {
        self.k.bm_cache.is_dirty()
    }

    // AppManager::propagate_fonts: settings -> reader before the first on_enter
    pub fn configure(&mut self, book_font: u8, theme: u8) {
        self.app.set_book_font_size(book_font);
        self.app.set_reading_theme(theme);
        self.apply_text_width();
    }

    // Geometry only: wrapping and page offsets remain production ReaderApp logic.
    pub fn set_text_width(&mut self, width: u32) {
        self.text_width_override = Some(width);
        self.apply_text_width();
    }

    fn apply_text_width(&mut self) {
        if let Some(width) = self.text_width_override {
            probe::set_text_width(&mut self.app, width);
        }
    }

    pub fn open(&mut self, name: &str) {
        self.ctx.set_message(name.as_bytes());
        let mut h = self.k.handle();
        self.app.on_enter(&mut self.ctx, &mut h);
        drop(h);
        self.apply_text_width();
        self.settle();
    }

    // `open` for a reader whose font data is absent (`fonts == None`): the
    // monospace layout path (see probe::drop_fonts)
    pub fn open_monospace(&mut self, name: &str) {
        self.ctx.set_message(name.as_bytes());
        let mut h = self.k.handle();
        self.app.on_enter(&mut self.ctx, &mut h);
        drop(h);
        probe::drop_fonts(&mut self.app);
        self.apply_text_width();
        self.settle();
    }

    pub fn press(&mut self, a: Action) {
        self.app.on_event(ActionEvent::Press(a), &mut self.ctx);
        self.apply_text_width();
        self.settle();
    }

    // one scheduler tick: the reader's `background`, then the worker task gets
    // its turn (the firmware's main task awaits there)
    fn tick(&mut self) {
        self.apply_text_width();
        let mut h = self.k.handle();
        block_on(self.app.background(&mut self.ctx, &mut h));
        drop(h);
        if probe::is_epub(&self.app) {
            run_image_worker();
        }
    }

    // run the background state machine until a page (or the TOC / an error) is up
    fn settle(&mut self) {
        for _ in 0..4000 {
            if probe::phase(&self.app) != Phase::Loading {
                return;
            }
            self.tick();
        }
        panic!(
            "reader did not settle, state {}",
            probe::state_name(&self.app)
        );
    }

    pub fn prepare_render(&mut self) {
        self.app.prepare_render(&mut self.ctx, &mut self.k.handle());
    }

    // AppContext forwarders: the scheduler's view of the loading overlay and of
    // the pending redraw request (no logic)
    pub fn loading_active(&self) -> bool {
        self.ctx.loading_active()
    }

    pub fn loading_message(&self) -> String {
        self.ctx.loading_msg().to_string()
    }

    pub fn loading_pct(&self) -> u8 {
        self.ctx.loading_pct()
    }

    // CjkState::set_pace of the reader's body/heading preparation
    pub fn set_cjk_pace(&mut self, now: fn() -> u64, slice_us: u32) {
        probe::set_cjk_pace(&mut self.app, now, slice_us);
    }

    pub fn has_redraw(&self) -> bool {
        self.ctx.has_redraw()
    }

    pub fn take_redraw(&mut self) {
        let _ = self.ctx.take_redraw();
    }

    // App::on_enter alone, without `settle`: the firmware renders (and so
    // prepares) between background ticks while the book is still loading
    pub fn enter(&mut self, name: &str) {
        self.ctx.set_message(name.as_bytes());
        let mut h = self.k.handle();
        self.app.on_enter(&mut self.ctx, &mut h);
        drop(h);
        self.apply_text_width();
    }

    pub fn phase(&self) -> Phase {
        probe::phase(&self.app)
    }

    pub fn error_kind(&self) -> Option<ErrorKind> {
        probe::error_kind(&self.app)
    }

    pub fn page(&self) -> usize {
        probe::page(&self.app)
    }

    pub fn total_pages(&self) -> usize {
        probe::total_pages(&self.app)
    }

    pub fn fully_indexed(&self) -> bool {
        probe::fully_indexed(&self.app)
    }

    pub fn page_offsets(&self) -> Vec<u32> {
        probe::offsets(&self.app)
    }

    pub fn lines(&self) -> Vec<Vec<u8>> {
        probe::lines(&self.app)
    }

    pub fn max_lines(&self) -> usize {
        probe::max_lines(&self.app)
    }

    pub fn text_w(&self) -> u32 {
        probe::text_w(&self.app)
    }

    pub fn text_margin(&self) -> u16 {
        probe::text_margin(&self.app)
    }

    pub fn font_line_h(&self) -> u16 {
        probe::font_line_h(&self.app)
    }

    pub fn text_y(&self) -> u16 {
        probe::text_y(&self.app)
    }

    // the real ReaderApp::draw: the current page / TOC / error screen
    pub fn draw(&self, strip: &mut StripBuffer) {
        self.app.draw(strip);
    }

    pub fn text_area_h(&self) -> u16 {
        probe::text_area_h(&self.app)
    }

    pub fn chapter(&self) -> u16 {
        probe::chapter(&self.app)
    }

    pub fn spine_len(&self) -> usize {
        probe::spine_len(&self.app)
    }

    pub fn epub_title(&self) -> String {
        probe::epub_title(&self.app)
    }

    pub fn epub_author(&self) -> String {
        probe::epub_author(&self.app)
    }

    pub fn toc_entries(&self) -> Vec<(String, u16)> {
        probe::toc_entries(&self.app)
    }

    pub fn toc_selected(&self) -> usize {
        probe::toc_selected(&self.app)
    }

    pub fn line_infos(&self) -> Vec<LineInfo> {
        probe::line_infos(&self.app)
    }

    pub fn page_image(&self) -> Option<(u16, u16)> {
        probe::page_image(&self.app)
    }

    pub fn quick_action_ids(&self) -> Vec<u8> {
        probe::quick_action_ids(&self.app)
    }

    pub fn quick_trigger(&mut self, id: u8) {
        probe::quick_trigger(&mut self.app, id, &mut self.ctx);
        self.apply_text_width();
        self.settle();
    }

    pub fn suspend(&mut self) {
        self.app.on_suspend();
    }

    // The app manager resumes the retained reader, then the scheduler runs it.
    pub fn resume(&mut self) {
        let mut h = self.k.handle();
        self.app.on_resume(&mut self.ctx, &mut h);
        drop(h);
        self.apply_text_width();
        self.settle();
    }

    // The quick-action overlay forwards the selected cycle value to the app.
    pub fn quick_cycle(&mut self, id: u8, value: u8) {
        self.app.on_quick_cycle_update(id, value, &mut self.ctx);
        self.apply_text_width();
        self.settle();
    }

    // App::on_exit; the next `open` is the firmware re-entering the book
    pub fn exit(&mut self) {
        probe::exit(&mut self.app);
    }

    // scheduler idle time: `ticks` background ticks, no settle, no state check
    pub fn idle(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    pub fn has_bg_work(&self) -> bool {
        probe::has_bg_work(&self.app)
    }

    // the save the firmware's App::save_state makes (src/apps/reader/mod.rs
    // save_state -> ReaderApp::save_position), over the kernel's bookmark
    // cache (the one Rig::new loaded at boot)
    pub fn save_position(&mut self) {
        let mut h = self.k.handle();
        self.app.save_position(h.bookmark_cache_mut());
        drop(h);
    }

    // BookmarkCache::save on the kernel's cache (the direct cache drive)
    pub fn bookmark_save(&mut self, filename: &[u8], byte_offset: u32, chapter: u16) {
        let mut h = self.k.handle();
        h.bookmark_cache_mut().save(filename, byte_offset, chapter);
    }

    // BookmarkCache::remove on the kernel's cache
    pub fn bookmark_remove(&mut self, filename: &[u8]) {
        let mut h = self.k.handle();
        h.bookmark_cache_mut().remove(filename);
    }

    // BookmarkCache::find on the kernel's cache
    pub fn bookmark_find(&self, filename: &[u8]) -> Option<BookmarkSlot> {
        self.k.bm_cache.find(filename)
    }

    // BookmarkCache::load_all: at most out.len() entries in the cache's list
    // order, returns how many were written (short-buffer contract included)
    pub fn bookmark_list_into(&self, out: &mut [BmListEntry]) -> usize {
        self.k.bm_cache.load_all(out)
    }

    // the scheduler's housekeeping save (writes _PULP/BKMK.BIN when dirty)
    pub fn bookmarks_flush(&mut self) {
        self.k.bookmarks_flush();
    }

    // power off: the card goes back out so a fresh rig can re-mount it
    // (a reboot boots through the production path again)
    pub fn into_storage(self) -> VirtualStorage {
        self.k.sd.card
    }

    // the reading-theme index stored in the real ReaderApp (read-only probe,
    // the same view the archived harness exposed as probe::theme_idx)
    pub fn theme_idx(&self) -> u8 {
        probe::theme_idx(&self.app)
    }
}

// The real SettingsApp over the virtual card: the load half of production
// AppManager::load_eager_settings (src/apps/manager.rs:199-204), the real
// editing loop (on_enter / on_event / background) and the card seam. No
// settings logic lives here: every call forwards to the production app.
pub struct SettingsRig {
    k: Kernel,
    ctx: AppContext,
    app: Box<SettingsApp>,
}

impl SettingsRig {
    // the real SettingsApp::new() over a fresh kernel and AppContext; no
    // bookmark load (the scheduler loads bookmarks separately)
    pub fn new(storage: VirtualStorage) -> Self {
        Self {
            k: Kernel::new(SdStorage::new(storage)),
            ctx: AppContext::new(),
            app: Box::new(SettingsApp::new()),
        }
    }

    // SettingsApp::load_eager: the load half of AppManager::load_eager_settings
    // (reads _PULP/SETTINGS.TXT through the real handle, at most 512 bytes, or
    // falls back to defaults, then applies the UI font size)
    pub fn boot(&mut self) {
        let mut h = self.k.handle();
        self.app.load_eager(&mut h);
    }

    pub fn storage(&self) -> &VirtualStorage {
        &self.k.sd.card
    }

    pub fn is_dirty(&self) -> bool {
        crate::apps::settings::is_dirty(&self.app)
    }

    pub fn is_loaded(&self) -> bool {
        self.app.is_loaded()
    }

    pub fn settings(&self) -> SystemSettings {
        *self.app.system_settings()
    }

    pub fn wifi_ssid(&self) -> String {
        self.app.wifi_config().ssid().to_string()
    }

    pub fn enter(&mut self) {
        let mut h = self.k.handle();
        self.app.on_enter(&mut self.ctx, &mut h);
    }

    // the Transition is ignored: the settings app never navigates away under
    // the keys the tests drive, and the tests observe the settings only
    pub fn press(&mut self, a: Action) {
        self.app.on_event(ActionEvent::Press(a), &mut self.ctx);
    }

    // one scheduler background tick: a dirty app saves here
    // (write_app_data -> _PULP/SETTINGS.TXT)
    pub fn tick(&mut self) {
        let mut h = self.k.handle();
        block_on(self.app.background(&mut self.ctx, &mut h));
    }

    // the _PULP/SETTINGS.TXT bytes on the rig's card, None when absent
    pub fn file(&self) -> Option<Vec<u8>> {
        let size = self
            .k
            .sd
            .card
            .file_size_in_pulp(config::SETTINGS_FILE)
            .ok()?;
        let mut buf = vec![0u8; size as usize];
        let n = self
            .k
            .sd
            .card
            .read_chunk_in_pulp(config::SETTINGS_FILE, 0, &mut buf)
            .ok()?;
        buf.truncate(n);
        Some(buf)
    }

    // power off: the card goes back out so a fresh rig can re-mount it
    pub fn into_storage(self) -> VirtualStorage {
        self.k.sd.card
    }
}

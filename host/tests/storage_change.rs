use std::future::Future;
use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use pulp_host::apps::files::FilesApp;
use pulp_host::apps::home::HomeApp;
use pulp_host::apps::manager::AppManager;
use pulp_host::apps::reader::ReaderApp;
use pulp_host::apps::settings::SettingsApp;
use pulp_host::apps::widgets::{ButtonFeedback, QuickMenu};
use pulp_host::apps::{App, AppId, Launcher, RECENT_FILE, Transition};
use pulp_host::board::action::{Action, ActionEvent, ButtonMapper};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::kernel::bookmarks::{BOOKMARK_FILE, BookmarkCache};
use pulp_host::kernel::{Kernel, SessionData, work_queue};
use pulp_host::storage::VirtualStorage;

// ReaderApp uses process-wide work channels even for opening a TXT book.
static READER_LOCK: Mutex<()> = Mutex::new(());

fn block_on(future: impl Future<Output = ()>) {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100 {
        if future.as_mut().poll(&mut cx) == Poll::Ready(()) {
            return;
        }
    }
    panic!("background tick did not finish");
}

fn card(name: &str, offset: u32) -> SdStorage {
    let text = "A long plain text paragraph for reading and paging. ".repeat(600);
    let card = VirtualStorage::memory_with(&[("BOOK.TXT", text.as_bytes())]);
    card.ensure_pulp_dir().unwrap();
    card.write_in_pulp(RECENT_FILE, name.as_bytes()).unwrap();
    let sd = SdStorage::new(card);
    let mut cache = BookmarkCache::new();
    cache.ensure_loaded(&sd);
    cache.save(name.as_bytes(), offset, 0);
    cache.flush(&sd);
    sd
}

fn read_pulp(sd: &SdStorage, name: &str) -> Vec<u8> {
    let mut bytes = vec![0; sd.card.file_size_in_pulp(name).unwrap() as usize];
    let n = sd.card.read_chunk_in_pulp(name, 0, &mut bytes).unwrap();
    assert_eq!(n, bytes.len());
    bytes
}

fn apps(k: &mut Kernel) -> AppManager {
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
    apps.enter_initial(&mut k.handle());
    apps
}

#[test]
fn swap_discards_dirty_or_clean_old_bookmarks_and_reader_state() {
    let _guard = READER_LOCK.lock().unwrap();
    for dirty in [false, true] {
        let mut k = Kernel::new(card("A.TXT", 111));
        k.bookmarks_load();
        let mut apps = apps(&mut k);
        apps.ctx_mut().set_message(b"BOOK.TXT");
        apps.apply_transition(Transition::Push(AppId::Reader), &mut k.handle());
        for _ in 0..10 {
            block_on(apps.run_background(&mut k.handle()));
        }
        apps.reader.save_position(k.handle().bookmark_cache_mut());
        assert!(k.handle().bookmark_cache().find(b"BOOK.TXT").is_some());
        // Settings over Reader also exercises clearing suspended app state.
        apps.apply_transition(Transition::Push(AppId::Settings), &mut k.handle());
        if !dirty {
            k.bookmarks_flush();
        }
        assert_eq!(k.handle().bookmark_cache().is_dirty(), dirty);
        let b = card("B.TXT", 222);
        let original = read_pulp(&b, BOOKMARK_FILE);
        k.replace_storage(b, true, &mut apps);
        k.bookmarks_flush();
        assert_eq!(read_pulp(k.sd(), BOOKMARK_FILE), original);

        assert_eq!(apps.active(), AppId::Home);
        assert_eq!(apps.launcher.depth(), 1);
        assert_eq!(apps.reader.filename_len(), 0);
        assert!(!apps.reader.has_bg_work());
        assert!(apps.has_redraw());
        assert!(k.handle().bookmark_cache().find(b"A.TXT").is_none());
        assert_eq!(
            k.handle()
                .bookmark_cache()
                .find(b"B.TXT")
                .unwrap()
                .byte_offset,
            222
        );
        // Old Reader must not re-dirty the newly loaded cache.
        apps.reader.save_position(k.handle().bookmark_cache_mut());
        block_on(apps.run_background(&mut k.handle()));
        k.bookmarks_flush();
        assert_eq!(read_pulp(k.sd(), BOOKMARK_FILE), original);
        assert!(!k.handle().bookmark_cache().is_dirty());

        let mut session = SessionData::zeroed();
        apps.collect_session(&mut session);
        assert_eq!(session.nav_depth, 1);
        assert_eq!(session.reader_filename_len, 0);
    }
}

#[test]
fn removal_blocks_old_saves_until_a_new_card_is_loaded() {
    let _guard = READER_LOCK.lock().unwrap();
    let mut k = Kernel::new(card("A.TXT", 111));
    k.bookmarks_load();
    let mut apps = apps(&mut k);
    k.handle().bookmark_cache_mut().save(b"A.TXT", 333, 0);
    let mut absent = SdStorage::new(VirtualStorage::memory());
    absent.set_mounted(false);
    k.replace_storage(absent, false, &mut apps);
    assert!(!k.handle().bookmark_cache().is_loaded());
    assert!(!k.handle().bookmark_cache().is_dirty());
    k.handle().bookmark_cache_mut().save(b"A.TXT", 999, 0);
    k.bookmarks_flush();
    let b = card("B.TXT", 222);
    let original_b = read_pulp(&b, BOOKMARK_FILE);
    k.replace_storage(b, true, &mut apps);
    k.bookmarks_flush();
    assert_eq!(read_pulp(k.sd(), BOOKMARK_FILE), original_b);
}

#[test]
fn swap_cancels_pending_delete_and_settings_write_and_reloads_files() {
    let _guard = READER_LOCK.lock().unwrap();
    let mut k = Kernel::new(card("A.TXT", 111));
    k.bookmarks_load();
    let mut apps = apps(&mut k);
    apps.apply_transition(Transition::Push(AppId::Files), &mut k.handle());
    block_on(apps.run_background(&mut k.handle()));
    assert_eq!(apps.files.total(), 1);
    let delete = apps.files.quick_actions()[0].id;
    apps.files.on_quick_trigger(delete, &mut apps.launcher.ctx);
    apps.quick_menu.show(apps.files.quick_actions());
    assert!(apps.quick_menu.open);
    apps.settings.system_settings_mut().sleep_timeout = 99;
    apps.settings.mark_save_needed();
    let b = card("B.TXT", 222);
    let settings = b"sleep_timeout=5\n";
    b.card.write_in_pulp("SETTINGS.TXT", settings).unwrap();
    b.card.write_file("NEW.TXT", b"new card only").unwrap();
    k.replace_storage(b, true, &mut apps);
    assert_eq!(apps.system_settings().sleep_timeout, 5);
    assert_eq!(apps.files.total(), 0);
    assert!(apps.files.quick_actions().is_empty());
    assert!(!apps.quick_menu.open);

    // Drive inactive apps directly to ensure cancelled operations cannot run.
    block_on(
        apps.files
            .background(&mut apps.launcher.ctx, &mut k.handle()),
    );
    block_on(
        apps.settings
            .background(&mut apps.launcher.ctx, &mut k.handle()),
    );
    assert!(k.sd().card.file_size("BOOK.TXT").is_ok());
    assert_eq!(read_pulp(k.sd(), "SETTINGS.TXT"), settings);
    apps.apply_transition(Transition::Push(AppId::Files), &mut k.handle());
    block_on(apps.run_background(&mut k.handle()));
    assert_eq!(apps.files.total(), 2);
    apps.files
        .on_event(ActionEvent::Press(Action::Next), &mut apps.launcher.ctx);
    // Files opens a book when ENTER comes up, not when it goes down
    let pressed = apps
        .files
        .on_event(ActionEvent::Press(Action::Select), &mut apps.launcher.ctx);
    assert_eq!(pressed, Transition::None);
    let transition = apps
        .files
        .on_event(ActionEvent::Release(Action::Select), &mut apps.launcher.ctx);
    assert_eq!(transition, Transition::Push(AppId::Reader));
    assert_eq!(apps.ctx().message(), b"NEW.TXT");
}

#[test]
fn swap_cancels_queued_epub_work_and_loads_the_new_recent_book() {
    let _guard = READER_LOCK.lock().unwrap();
    let mut k = Kernel::new(card("A.TXT", 111));
    k.bookmarks_load();
    let mut apps = apps(&mut k);
    apps.ctx_mut().set_message(b"OLD.EPUB");
    apps.apply_transition(Transition::Push(AppId::Reader), &mut k.handle());
    assert!(apps.reader.is_epub());
    let generation = work_queue::active_generation();
    for _ in 0..2 {
        assert!(work_queue::submit(
            generation,
            work_queue::WorkTask::DecodeImage {
                path_hash: 1,
                data: vec![],
                is_jpeg: false,
                max_w: 100,
                max_h: 100,
            }
        ));
    }
    assert!(!work_queue::can_submit());

    k.replace_storage(card("B.TXT", 222), true, &mut apps);
    assert_ne!(work_queue::active_generation(), generation);
    assert!(work_queue::can_submit());
    assert!(!apps.reader.is_epub());
    let transition = apps
        .home
        .on_event(ActionEvent::Press(Action::Select), &mut apps.launcher.ctx);
    assert_eq!(transition, Transition::Push(AppId::Reader));
    assert_eq!(apps.ctx().message(), b"B.TXT");
}

#[test]
fn files_enter_long_press_opens_nothing_and_a_stray_release_neither() {
    let _guard = READER_LOCK.lock().unwrap();
    let mut k = Kernel::new(card("A.TXT", 111));
    k.bookmarks_load();
    let mut apps = apps(&mut k);
    apps.launcher.ctx.take_redraw();
    let files = &mut apps.files;
    let ctx = &mut apps.launcher.ctx;
    // a release that did not start in the list (ENTER went down in Home)
    assert_eq!(
        files.on_event(ActionEvent::Release(Action::Select), ctx),
        Transition::None
    );
    // press, hold into the long press (the quick menu takes over), release
    assert_eq!(
        files.on_event(ActionEvent::Press(Action::Select), ctx),
        Transition::None
    );
    assert_eq!(
        files.on_event(ActionEvent::LongPress(Action::Select), ctx),
        Transition::None
    );
    assert_eq!(
        files.on_event(ActionEvent::Release(Action::Select), ctx),
        Transition::None
    );
}

#[test]
fn files_cancelled_select_does_not_open_a_book_on_its_release() {
    // the quick menu opened on the long press and the manager swallowed it; the
    // ENTER that activates a menu item comes up after the menu closed
    let _guard = READER_LOCK.lock().unwrap();
    let mut k = Kernel::new(card("A.TXT", 111));
    k.bookmarks_load();
    let mut apps = apps(&mut k);
    apps.launcher.ctx.take_redraw();
    let ctx = &mut apps.launcher.ctx;
    assert_eq!(
        apps.files.on_event(ActionEvent::Press(Action::Select), ctx),
        Transition::None
    );
    apps.files.cancel_select();
    assert_eq!(
        apps.files
            .on_event(ActionEvent::Release(Action::Select), ctx),
        Transition::None
    );
}

use pulp_host::ErrorKind;
use pulp_host::fixtures::{Newline, TxtSpec, build_txt};
use pulp_host::reader::{Action, Phase, Rig, SettingsRig};
use pulp_host::storage::{ReadOutcome, StorageOp, VirtualStorage};

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn card() -> VirtualStorage {
    let bytes = build_txt(&TxtSpec {
        lines: (0..1200).map(|i| format!("Line {i}: a distinct sentence for reliable page navigation.")).collect(),
        newline: Newline::Lf,
        trailing_newline: true,
    });
    let card = VirtualStorage::memory_with(&[("FAIL.TXT", &bytes)]);
    card.ensure_pulp_dir().unwrap();
    card
}

#[test]
fn missing_card_is_a_storage_boundary_failure_and_mount_recovers() {
    let _serial = serial();
    let mut r = Rig::new(card());
    r.set_sd_ok(false);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::NoCard));
    r.set_sd_ok(true);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
}

#[test]
fn failed_open_and_page_read_can_be_retried() {
    let _serial = serial();
    let mut r = Rig::new(card());
    r.storage().inject_error(StorageOp::Read, "FAIL.TXT", 1, ErrorKind::ReadFailed);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::ReadFailed));
    assert_eq!(r.storage().pending_injections(), 0);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
    assert!(r.total_pages() > 1);
    r.press(Action::Next);
    assert!(r.page() > 0);
    r.storage().reset_reads();
    r.storage().inject_error(StorageOp::Read, "FAIL.TXT", 1, ErrorKind::ReadFailed);
    r.press(Action::Prev);
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.error_kind(), Some(ErrorKind::ReadFailed));
    assert!(r.storage().read_log().iter().any(|x| x.outcome == ReadOutcome::ErrorInjected(ErrorKind::ReadFailed)));
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
}

#[test]
fn failed_best_effort_prefetch_keeps_the_displayed_page_ready_and_recovers() {
    let _serial = serial();
    let mut r = Rig::new(card());
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
    let first_page = r.page();
    r.storage().reset_reads();
    r.storage().inject_error(StorageOp::Read, "FAIL.TXT", 1, ErrorKind::ReadFailed);
    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), first_page + 1);
    assert_eq!(r.storage().pending_injections(), 0);
    assert!(r.storage().read_log().iter().any(|x| x.outcome == ReadOutcome::ErrorInjected(ErrorKind::ReadFailed)));
    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), first_page + 2);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
}

#[test]
fn short_read_is_recorded_and_reader_settles() {
    let _serial = serial();
    let mut r = Rig::new(card());
    r.storage().inject_short_read("FAIL.TXT", 1, 17);
    r.open("FAIL.TXT");
    let reads = r.storage().read_log();
    let short = reads.iter().find(|x| x.outcome == ReadOutcome::ShortInjected).expect("short read reached the reader");
    assert_eq!(short.returned, 17);
    assert!(short.requested > short.returned);
    assert!(matches!(r.phase(), Phase::Ready | Phase::Error));
    if r.phase() == Phase::Error { assert!(r.error_kind().is_some()); }
    assert_eq!(r.storage().pending_injections(), 0);
    r.open("FAIL.TXT");
    assert_eq!(r.phase(), Phase::Ready);
}

#[test]
fn settings_write_failure_preserves_bytes_and_next_tick_retries() {
    let _serial = serial();
    let mut r = SettingsRig::new(card());
    r.boot();
    r.enter();
    r.press(Action::NextJump);
    r.tick();
    let old = r.file().expect("baseline settings saved");
    r.press(Action::NextJump);
    assert_eq!(r.settings().sleep_timeout, 20);
    r.storage().inject_error(StorageOp::Write, "_PULP/SETTINGS.TXT", 1, ErrorKind::WriteFailed);
    r.tick();
    assert_eq!(r.storage().pending_injections(), 0);
    assert_eq!(r.file().unwrap(), old);
    assert!(r.is_dirty());
    r.tick();
    assert!(!r.is_dirty());
    let saved = r.file().unwrap();
    assert_ne!(saved, old);
    assert!(String::from_utf8(saved).unwrap().lines().any(|x| x == "sleep_timeout=20"));
}

#[test]
fn bookmark_flush_failure_preserves_bytes_and_dirty_state() {
    let _serial = serial();
    let mut r = Rig::new(card());
    r.bookmark_save(b"FAIL.TXT", 123, 0);
    r.bookmarks_flush();
    let read = |r: &Rig| {
        let n = r.storage().file_size_in_pulp("BKMK.BIN").unwrap() as usize;
        let mut bytes = vec![0; n];
        assert_eq!(r.storage().read_chunk_in_pulp("BKMK.BIN", 0, &mut bytes).unwrap(), n);
        bytes
    };
    let old = read(&r);
    r.bookmark_save(b"FAIL.TXT", 456, 0);
    assert!(r.bookmarks_dirty());
    r.storage().inject_error(StorageOp::Write, "_PULP/BKMK.BIN", 1, ErrorKind::WriteFailed);
    r.bookmarks_flush();
    assert_eq!(r.storage().pending_injections(), 0);
    assert_eq!(read(&r), old);
    assert!(r.bookmarks_dirty());
    r.bookmarks_flush();
    assert!(!r.bookmarks_dirty());
    assert_ne!(read(&r), old);
    let storage = r.into_storage();
    let reboot = Rig::new(storage);
    assert_eq!(reboot.bookmark_find(b"FAIL.TXT").unwrap().byte_offset, 456);
}

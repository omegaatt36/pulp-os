use pulp_host::ErrorKind;
use pulp_host::dir_entry::DirEntry;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::kernel::dir_cache::DirCache;
use pulp_host::storage::{StorageOp, VirtualStorage};

#[test]
fn invalidation_and_failed_reload_hide_entries_and_titles_then_recover() {
    let sd = SdStorage::new(VirtualStorage::memory_with(&[
        ("Z.EPUB", b"z"),
        ("A.EPUB", b"a"),
    ]));
    let mut cache = DirCache::new();
    let mut page = [DirEntry::EMPTY; 4];
    assert_eq!(cache.page(0, &mut page).count, 0);
    cache.ensure_loaded(&sd).unwrap();
    assert_eq!(cache.page(0, &mut page).count, 2);
    assert_eq!(page[0].name_str(), "A.EPUB");
    cache.set_entry_title(0, b"Old title");
    assert!(cache.find_title(b"a.epub").is_some());

    cache.invalidate();
    assert_eq!(cache.page(0, &mut page).total, 0);
    assert!(cache.find_title(b"A.EPUB").is_none());
    assert!(cache.next_untitled_epub(0).is_none());
    cache.set_entry_title(0, b"Cannot restore invalid entry");

    sd.card
        .inject_error(StorageOp::List, "", 1, ErrorKind::ReadFailed);
    assert!(cache.ensure_loaded(&sd).is_err());
    assert_eq!(cache.page(0, &mut page).count, 0);
    assert!(cache.find_title(b"A.EPUB").is_none());
    sd.card.delete_file("A.EPUB").unwrap();
    cache.ensure_loaded(&sd).unwrap();
    assert_eq!(cache.page(0, &mut page).count, 1);
    assert_eq!(page[0].name_str(), "Z.EPUB");
    assert_eq!(cache.page(usize::MAX, &mut page).count, 0);
    assert!(cache.find_title(b"A.EPUB").is_none());
}

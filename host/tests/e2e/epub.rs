// An EPUB through the quick menu: chapter jumps and the table of contents.
use pulp_host::apps::AppId;
use pulp_host::fixtures::standard;
use pulp_host::storage::VirtualStorage;

use crate::device::{Device, Key, serial};

fn card() -> VirtualStorage {
    let book = standard()
        .into_iter()
        .find(|f| f.name == "E3DEFL.EPU")
        .expect("standard EPUB 3 fixture");
    VirtualStorage::memory_with(&[("E3DEFL.EPU", &book.bytes)])
}

#[test]
fn chapters_are_reached_from_the_quick_menu_and_the_contents() {
    let _serial = serial();
    let mut d = Device::boot(card());
    d.taps(&[Key::Enter, Key::Enter]);
    assert_eq!(d.active(), AppId::Reader);
    assert!(d.apps.reader.is_epub());
    assert_eq!(d.apps.reader.chapter(), 0);
    let first = d.screen().to_pbm();

    // quick menu rows: Book Font, Prev Ch, Next Ch, Contents, Refresh, Go Home
    d.hold(Key::Enter);
    d.tap(Key::Next);
    d.tap(Key::Next);
    d.tap(Key::Enter);
    assert_eq!(d.apps.reader.chapter(), 1, "Next Ch");
    assert_ne!(d.screen().to_pbm(), first);

    // Contents opens on the entry of the current chapter; Next moves down it
    d.hold(Key::Enter);
    d.taps(&[Key::Next, Key::Next, Key::Next, Key::Enter]);
    assert!(d.apps.reader.showing_toc(), "the table of contents is up");
    d.tap(Key::Next);
    d.tap(Key::Enter);
    assert!(!d.apps.reader.showing_toc());
    assert_eq!(
        d.apps.reader.chapter(),
        2,
        "the entry below Salt and Pepper"
    );

    // the chapter is remembered across a power cycle
    d.taps(&[Key::Back, Key::Back]);
    let mut d = d.reboot();
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Reader);
    assert_eq!(d.apps.reader.chapter(), 2);
}

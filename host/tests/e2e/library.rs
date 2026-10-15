// Managing the card from the device: deleting a book takes its reading
// position with it, and Home must not keep offering it.
use pulp_host::apps::AppId;
use pulp_host::storage::VirtualStorage;

use crate::device::{Device, Key, serial};

fn card() -> VirtualStorage {
    let text: String = (0..200)
        .map(|i| format!("Paragraph {i}. The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.\n\n"))
        .collect();
    VirtualStorage::memory_with(&[
        ("ALPHA.TXT", text.as_bytes()),
        ("BRAVO.TXT", text.as_bytes()),
    ])
}

#[test]
fn deleting_the_book_that_was_just_read_removes_it_from_home_too() {
    let _serial = serial();
    let mut d = Device::boot(card());
    d.taps(&[Key::Enter, Key::Enter]); // Files, ALPHA.TXT
    assert_eq!(d.reader_book(), "ALPHA.TXT");
    d.taps(&[Key::Next, Key::Next]);
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Files);

    // long ENTER on the book: the quick menu's first row is Delete File
    d.hold(Key::Enter);
    assert!(d.apps.quick_menu.open);
    d.tap(Key::Enter);
    assert!(!d.apps.quick_menu.open);

    assert!(d.card().file_size("ALPHA.TXT").is_err(), "the file is gone");
    assert!(
        d.card().file_size("BRAVO.TXT").is_ok(),
        "the other book stays"
    );
    assert_eq!(d.apps.files.total(), 1);

    // Home must not offer to continue a book that is not there any more
    d.tap(Key::Back);
    let mut d = d.reboot();
    d.tap(Key::Enter);
    assert_eq!(
        d.active(),
        AppId::Files,
        "the first entry on Home is Files again, not Continue"
    );
}

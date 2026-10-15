// Home -> Files -> a book -> pages -> back -> power cycle -> Continue.
use pulp_host::apps::AppId;
use pulp_host::storage::VirtualStorage;

use crate::device::{Device, Key, serial};

// enough paragraphs for dozens of pages, each one different
fn book_text(title: &str) -> String {
    (0..400)
        .map(|i| format!("{title} paragraph {i}. The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.\n\n"))
        .collect()
}

fn card() -> VirtualStorage {
    let alpha = book_text("Alpha");
    let bravo = book_text("Bravo");
    VirtualStorage::memory_with(&[
        ("ALPHA.TXT", alpha.as_bytes()),
        ("BRAVO.TXT", bravo.as_bytes()),
    ])
}

#[test]
fn a_book_opened_from_files_resumes_from_home_after_a_power_cycle() {
    let _serial = serial();
    let mut d = Device::boot(card());
    assert_eq!(d.active(), AppId::Home);
    let home = d.screen().to_pbm();

    // Home: Files is the first entry while nothing has been read
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Files);
    assert_ne!(d.screen().to_pbm(), home, "Files is on screen");

    // the second book
    d.tap(Key::Next);
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Reader);
    assert_eq!(d.reader_book(), "BRAVO.TXT");
    let first_page = d.screen().to_pbm();

    d.taps(&[Key::Next, Key::Next, Key::Next]);
    assert_eq!(d.apps.reader.page(), 3);
    assert_ne!(
        d.screen().to_pbm(),
        first_page,
        "turning pages changes the screen"
    );
    let offset = d.apps.reader.byte_offset();
    assert!(offset > 0);

    // Back out: Files, then Home
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Files);
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);

    // after a power cycle Home offers Continue, and it lands on the same page
    let mut d = d.reboot();
    assert_eq!(d.active(), AppId::Home);
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Reader);
    assert_eq!(d.reader_book(), "BRAVO.TXT");
    assert_eq!(d.apps.reader.byte_offset(), offset);
}

// Deep sleep: the record written before sleeping puts the device back where
// it was, with the navigation stack under it.
use pulp_host::apps::AppId;
use pulp_host::storage::VirtualStorage;

use crate::device::{Device, Key, serial};

fn card() -> VirtualStorage {
    let text: String = (0..400)
        .map(|i| format!("Paragraph {i}. The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.\n\n"))
        .collect();
    VirtualStorage::memory_with(&[("BOOK.TXT", text.as_bytes())])
}

#[test]
fn waking_from_sleep_returns_to_the_page_that_was_open() {
    let _serial = serial();
    let mut d = Device::boot(card());
    d.taps(&[Key::Enter, Key::Enter]);
    assert_eq!(d.active(), AppId::Reader);
    d.taps(&[Key::Next, Key::Next, Key::Next, Key::Next]);
    let (page, offset) = (d.apps.reader.page(), d.apps.reader.byte_offset());
    assert_eq!(page, 4);
    let shown = d.screen().to_pbm();

    let mut d = d.sleep_and_wake();
    assert_eq!(d.active(), AppId::Reader, "the reader is back");
    assert_eq!(d.reader_book(), "BOOK.TXT");
    assert_eq!(d.apps.reader.page(), page);
    assert_eq!(d.apps.reader.byte_offset(), offset);
    assert_eq!(d.screen().to_pbm(), shown, "the same page is on screen");

    // the stack under the reader came back too: Files, then Home
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Files);
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);
}

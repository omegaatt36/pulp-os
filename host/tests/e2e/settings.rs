// What the user changes on the device reaches the apps and survives a power
// cycle: Settings rows, the reader's quick menu, the button swap.
use pulp_host::apps::AppId;
use pulp_host::storage::VirtualStorage;

use crate::device::{Device, Key, serial};

fn card() -> VirtualStorage {
    let text: String = (0..300)
        .map(|i| format!("Paragraph {i}. The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.\n\n"))
        .collect();
    VirtualStorage::memory_with(&[("BOOK.TXT", text.as_bytes())])
}

// Home -> Files -> the only book
fn open_book(d: &mut Device) {
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Files);
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Reader);
}

// Home -> Settings: the last of the three entries, or of four with Continue
fn open_settings(d: &mut Device, continue_entry: bool) {
    let nexts = if continue_entry { 3 } else { 2 };
    for _ in 0..nexts {
        d.tap(Key::Next);
    }
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Settings);
}

#[test]
fn the_reading_theme_set_in_settings_reaches_the_reader_and_survives_a_reboot() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_book(&mut d);
    let default_theme = d.screen().to_pbm();
    let default_settings = d.apps.system_settings().reading_theme;
    d.taps(&[Key::Back, Key::Back]);
    assert_eq!(d.active(), AppId::Home);

    // Settings: Theme is the fifth row; the right front key raises the value
    open_settings(&mut d, true);
    d.taps(&[Key::Next, Key::Next, Key::Next, Key::Next, Key::Right]);
    let theme = d.apps.system_settings().reading_theme;
    assert_ne!(theme, default_settings, "the Theme row changed");
    let saved = d
        .pulp_file("SETTINGS.TXT")
        .expect("settings are on the card");
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);

    // Continue reopens the book, laid out with the new margins
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Reader);
    assert_ne!(
        d.screen().to_pbm(),
        default_theme,
        "the page follows the theme"
    );

    // and the choice is still there after a power cycle
    let mut d = d.reboot();
    assert_eq!(d.apps.system_settings().reading_theme, theme);
    assert_eq!(
        d.pulp_file("SETTINGS.TXT").as_deref(),
        Some(saved.as_slice())
    );
}

#[test]
fn the_book_font_changed_in_the_reader_quick_menu_survives_a_reboot() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_book(&mut d);
    let before = d.apps.reader.font_size_idx();
    let page = d.screen().to_pbm();

    // long ENTER opens the quick menu; its first row is Book Font
    d.hold(Key::Enter);
    assert!(d.apps.quick_menu.open, "the quick menu is open");
    d.tap(Key::Right);
    d.tap(Key::Back);
    assert!(!d.apps.quick_menu.open, "Back closes the quick menu");

    let after = d.apps.reader.font_size_idx();
    assert_ne!(after, before, "the reader took the new font");
    assert_eq!(d.apps.system_settings().book_font_size_idx, after);
    assert_ne!(d.screen().to_pbm(), page, "the page is laid out again");

    // the setting is on the card without the user visiting Settings
    let d = d.reboot();
    assert_eq!(
        d.apps.system_settings().book_font_size_idx,
        after,
        "the font chosen in the reader is still chosen after a power cycle"
    );
}

#[test]
fn swapping_the_buttons_in_settings_remaps_the_front_keys_everywhere() {
    let _serial = serial();
    let mut d = Device::boot(card());

    // unswapped, the right front key is a jump and does nothing on Home
    d.tap(Key::Right);
    assert_eq!(d.active(), AppId::Home);

    open_settings(&mut d, false);
    d.taps(&[
        Key::Next,
        Key::Next,
        Key::Next,
        Key::Next,
        Key::Next,
        Key::Right,
    ]);
    assert!(d.apps.system_settings().swap_buttons);

    // swapped, the left front key is Back
    d.tap(Key::Left);
    assert_eq!(d.active(), AppId::Home);

    // and the right front key selects: Files
    d.tap(Key::Right);
    assert_eq!(d.active(), AppId::Files);

    // the swap is still in force after a power cycle
    let mut d = d.reboot();
    d.tap(Key::Right);
    assert_eq!(d.active(), AppId::Files);
}

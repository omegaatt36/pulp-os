// The book `device-fixtures` writes for the device check opens in the production
// reader and shows what it is meant to: columns without the ruby readings, and
// the picture of chapter 2.
//
// Run: cargo test-host --test device_books
use pulp_host::fixtures::device::vertical_book;
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::storage::VirtualStorage;

fn settle(r: &mut Rig) {
    for _ in 0..4000 {
        if !r.has_bg_work() {
            return;
        }
        r.idle(1);
    }
    panic!("background work did not finish");
}

#[test]
fn the_vertical_book_is_set_in_columns_without_ruby_readings() {
    // the reader keeps process-wide work channels; this binary has one test
    let card = VirtualStorage::memory_with(&[("VERTRUBY.EPU", &vertical_book())]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("VERTRUBY.EPU");
    assert_eq!(r.phase(), Phase::Ready);
    assert!(r.vertical(), "rtl spine and zh-TW language: columns");

    let mut text = String::new();
    let mut picture_chapter = None;
    loop {
        settle(&mut r);
        for line in r.lines() {
            text.push_str(&String::from_utf8_lossy(&line));
        }
        if r.page_image().is_some() {
            picture_chapter = Some(r.chapter());
        }
        let at = (r.chapter(), r.page());
        r.press(Action::Next);
        if (r.chapter(), r.page()) == at {
            break;
        }
    }
    assert!(
        text.contains("漢字直排測試"),
        "the base text of the ruby stays"
    );
    for reading in ['ㄏ', 'ㄢ', 'ㄗ'] {
        assert!(!text.contains(reading), "{reading} is a ruby reading");
    }
    assert_eq!(
        picture_chapter,
        Some(1),
        "the picture of chapter 2 is shown"
    );
}

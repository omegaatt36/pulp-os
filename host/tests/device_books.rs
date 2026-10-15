// The books `device-fixtures` writes for the device check open in the production
// reader and show what they are meant to: columns without the ruby readings, and
// a picture in each chapter whether the JPEG is progressive or baseline.
//
// Run: cargo test-host --test device_books
use std::sync::Mutex;

use pulp_host::fixtures::device::{jpeg_book, plasma, vertical_book};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::storage::VirtualStorage;

// the reader keeps process-wide work channels
static SERIAL: Mutex<()> = Mutex::new(());

fn open(name: &str, bytes: &[u8]) -> Rig {
    let card = VirtualStorage::memory_with(&[(name, bytes)]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open(name);
    assert_eq!(r.phase(), Phase::Ready, "{name} opens to a page");
    settle(&mut r);
    r
}

fn settle(r: &mut Rig) {
    for _ in 0..4000 {
        if !r.has_bg_work() {
            return;
        }
        r.idle(1);
    }
    panic!("background work did not finish");
}

// every page of the book in order: (chapter, page, text, image)
fn pages(r: &mut Rig) -> Vec<(u16, usize, String, Option<(u16, u16)>)> {
    let mut seen = vec![];
    loop {
        settle(r);
        let text = r
            .lines()
            .iter()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect::<String>();
        seen.push((r.chapter(), r.page(), text, r.page_image()));
        let at = (r.chapter(), r.page());
        r.press(Action::Next);
        if (r.chapter(), r.page()) == at {
            return seen;
        }
    }
}

#[test]
fn the_vertical_book_is_set_in_columns_without_ruby_readings() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rgb = plasma();
    let mut r = open("VERTRUBY.EPU", &vertical_book(&rgb));
    assert!(r.vertical(), "rtl spine and zh-TW language: columns");
    let pages = pages(&mut r);
    let text: String = pages.iter().map(|p| p.2.as_str()).collect();
    assert!(
        text.contains("漢字直排測試"),
        "the base text of the ruby stays"
    );
    for reading in ['ㄏ', 'ㄢ', 'ㄗ'] {
        assert!(!text.contains(reading), "{reading} is a ruby reading");
    }
    assert!(
        pages.iter().any(|p| p.0 == 1 && p.3.is_some()),
        "the picture of chapter 2 is on a page"
    );
}

#[test]
fn the_progressive_and_baseline_pictures_both_reach_a_page() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rgb = plasma();
    let book = jpeg_book(&rgb);
    // the entries are stored: the start-of-frame markers are in the book's bytes
    let has = |marker: u8| book.windows(2).any(|w| w == [0xFF, marker]);
    assert!(has(0xC2), "one picture is progressive (SOF2)");
    assert!(has(0xC0), "one picture is baseline (SOF0)");
    let mut r = open("PROGJPG.EPU", &book);
    assert!(!r.vertical());
    let pages = pages(&mut r);
    let size_in = |chapter: u16| {
        pages
            .iter()
            .find(|p| p.0 == chapter && p.3.is_some())
            .and_then(|p| p.3)
    };
    let progressive = size_in(0).expect("the progressive picture is decoded");
    let baseline = size_in(1).expect("the baseline picture is decoded");
    assert_eq!(progressive, baseline, "same picture, same size on the page");
}

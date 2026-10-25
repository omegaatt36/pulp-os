// The books `device-fixtures` writes for the device check open in the production
// reader and show what they are meant to: the Japanese text with its ruby groups, in
// columns or in lines, and the picture of chapter 2.
//
// Run: cargo test-host --test device_books
use pulp_host::fixtures::device::{horizontal_book, vertical_book};
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

// the text of the lines with ruby groups spelled `[base|reading]`
fn spelled(line: &[u8]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if line[i] == 1 && i + 1 < line.len() {
            match line[i + 1] {
                b'R' => out.push(b'['),
                b'T' => out.push(b'|'),
                b'r' => out.push(b']'),
                _ => {}
            }
            i += 2;
        } else {
            out.push(line[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_through(book: &[u8], name: &str) -> (Rig, String, Option<u16>) {
    let card = VirtualStorage::memory_with(&[(name, book)]);
    card.ensure_pulp_dir().unwrap();
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open(name);
    assert_eq!(r.phase(), Phase::Ready);

    let mut text = String::new();
    let mut picture_chapter = None;
    loop {
        settle(&mut r);
        for line in r.lines() {
            text.push_str(&spelled(&line));
            text.push('\n');
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
    (r, text, picture_chapter)
}

const GROUPS: [&str; 7] = [
    "[漢字|かんじ]",
    "[振|ふ]",
    "[仮名|がな]",
    "[私|わたし]",
    "[日本語|にほんご]",
    "[勉強|べんきょう]",
    "[例|たとえ]",
];

#[test]
fn the_ruby_books_are_set_with_their_groups_in_columns_and_in_lines() {
    // the reader keeps process-wide work channels; this binary has one test
    let (mut r, text, picture) = read_through(&vertical_book(), "VERTRUBY.EPU");
    assert!(r.vertical(), "rtl spine: columns");
    for group in GROUPS {
        assert!(text.contains(group), "{group} in the columns");
    }
    assert_eq!(picture, Some(1), "the picture of chapter 2 is shown");
    r.exit();

    let (r, text, picture) = read_through(&horizontal_book(), "HORIRUBY.EPU");
    assert!(!r.vertical(), "no rtl spine: lines");
    for group in GROUPS {
        assert!(text.contains(group), "{group} in the lines");
    }
    assert_eq!(picture, Some(1), "the picture of chapter 2 is shown");
}

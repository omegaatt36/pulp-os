use crate::cjk_support;

use cjk_support::{BOOK, assert_patch, rig, rows, text_lines};
use pulp_host::reader::{Action, Phase, Rig};

#[test]
fn heading_size_survives_page_navigation_and_bookmark_restore_until_closing_marker() {
    // Existing stripped-text markers open/close a heading across page windows.
    let mut input = vec![1, b'H'];
    input.extend_from_slice("臺".repeat(1000).as_bytes());
    let heading_end = input.len() as u32;
    input.extend_from_slice(b"\x01h\nLatin body.");
    let mut r = rig(&input, 0, 2 * 24);
    let first = r.lines();
    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), 1, "fixture reaches its second page");
    let second = r.lines();
    let second_offset = r.page_offsets()[1];
    assert!(second_offset > 2 && second_offset < heading_end);
    let visible = text_lines(&r).concat();
    assert!(!visible.is_empty());
    assert!(visible.chars().all(|ch| ch == '臺'));
    // The heading opened on page one has not closed: 23px, never body 16px.
    assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);

    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), 2, "heading spans at least three pages");
    assert!(r.page_offsets()[2] < heading_end);
    r.press(Action::Prev);
    assert_eq!(r.lines(), second);
    assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);
    r.press(Action::Prev);
    assert_eq!(r.lines(), first);
    r.press(Action::Next);
    assert_eq!(r.lines(), second);
    assert_eq!(r.page_offsets()[r.page()], second_offset);
    assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);

    r.save_position();
    r.bookmarks_flush();
    let mut reboot = Rig::new(r.into_storage());
    reboot.configure(0, 0);
    reboot.set_text_width(2 * 24);
    reboot.open(BOOK);
    assert_eq!(reboot.phase(), Phase::Ready);
    assert_eq!(reboot.page_offsets()[reboot.page()], second_offset);
    assert_eq!(reboot.lines(), second);
    assert_patch(&reboot, reboot.text_margin(), 8, &rows(23, '臺'), 3);
}

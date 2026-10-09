use crate::cjk_support;

use cjk_support::*;
use pulp_host::reader::{Action, Phase, QA_FONT_SIZE, Rig};

const TEXT: &str = "臺灣𠮷繁體中文";
const REPLACED_ROWS: [u8; 3] = [0x42, 0xA5, 0x5A];

// Freeze a different font identity, literal advance and bitmap at unchanged px.
// PFNT v1: font_id at 8, records start at 44 with advance at record byte 12.
fn replacement(px: u16, advance: u16) -> Vec<u8> {
    let mut bytes = pack(px, false);
    bytes[8..16].copy_from_slice(&(0x1234_0000_0000_0000u64 + px as u64).to_le_bytes());
    let count = alphabet().len();
    for record in 0..count {
        let offset = 44 + 22 * record + 12;
        bytes[offset..offset + 2].copy_from_slice(&advance.to_le_bytes());
    }
    for bitmap in bytes[44 + 22 * count..].chunks_exact_mut(3) {
        bitmap.copy_from_slice(&REPLACED_ROWS);
    }
    bytes
}

fn forward_three(r: &mut Rig) -> u32 {
    for _ in 0..3 {
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready);
    }
    assert_eq!(r.page(), 3, "fixture reaches a nonzero raw anchor");
    r.page_offsets()[r.page()]
}

fn assert_anchor(r: &Rig, input: &str, anchor: u32) {
    assert_eq!(r.phase(), Phase::Ready);
    let start = r.page_offsets()[r.page()] as usize;
    let visible = text_lines(r).concat();
    assert!(
        input.is_char_boundary(start),
        "rebuilt page start is a scalar boundary"
    );
    assert!(
        input.is_char_boundary(anchor as usize),
        "captured anchor is a scalar boundary"
    );
    assert!(
        start <= anchor as usize && (anchor as usize) < start + visible.len(),
        "raw anchor {anchor} must be in rebuilt page [{start}, {})",
        start + visible.len()
    );
    assert_eq!(&input[start..start + visible.len()], visible);
}

fn assert_cells(r: &Rig, cells: usize) {
    let lines = text_lines(r);
    assert!(!lines.is_empty());
    for line in &lines {
        assert!(!line.is_empty());
        assert!(
            line.chars().count() <= cells,
            "literal advances permit {cells} cells: {line:?}"
        );
    }
    // All pages asserted here are far from EOF. A whole line must use its width.
    assert_eq!(lines[0].chars().count(), cells);
}

fn assert_round_trip(r: &mut Rig) {
    let (page, offsets, lines) = (r.page(), r.page_offsets(), r.lines());
    assert!(page > 0);
    r.press(Action::Prev);
    assert_eq!(r.phase(), Phase::Ready);
    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), page);
    assert_eq!(r.page_offsets()[page], offsets[page]);
    assert_eq!(r.lines(), lines, "forward/back uses the rebuilt page start");
}

fn walk_all(r: &mut Rig, input: &str) {
    while r.page() > 0 {
        let before = r.page();
        r.press(Action::Prev);
        assert!(r.page() < before, "backward navigation makes progress");
    }
    let mut actual = String::new();
    for _ in 0..512 {
        assert_eq!(r.phase(), Phase::Ready);
        let lines = text_lines(r);
        assert_kinsoku(&lines);
        actual.push_str(&lines.concat());
        let before = r.page();
        r.press(Action::Next);
        if r.page() == before {
            assert_eq!(actual, input, "every raw text scalar appears exactly once");
            for offset in r.page_offsets() {
                assert!(
                    input.is_char_boundary(offset as usize),
                    "offset {offset} splits UTF-8"
                );
            }
            return;
        }
        assert_eq!(r.page(), before + 1);
    }
    panic!("bounded fixture never reached EOF");
}

#[test]
fn same_size_body_replacement_resume_rebuilds_pages_at_the_raw_anchor() {
    let input = TEXT.repeat(180);
    let mut r = rig(input.as_bytes(), 0, 68);
    assert_cells(&r, 4); // Frozen 16px fixture advance17: 68 / 17 = 4.
    let anchor = forward_three(&mut r);
    install(r.storage(), 16, &replacement(16, 34));
    r.resume();
    assert_cells(&r, 2); // Replacement advance34: 68 / 34 = 2.
    assert_anchor(&r, &input, anchor);
    assert_patch(&r, r.text_margin(), 8, &REPLACED_ROWS, 3);
    assert_round_trip(&mut r);
    assert_patch(&r, r.text_margin(), 8, &REPLACED_ROWS, 3);
    walk_all(&mut r, &input);
}

#[test]
fn same_size_heading_replacement_resume_refreshes_the_active_heading_bank() {
    let input = TEXT.repeat(180);
    let mut book = vec![1, b'H'];
    book.extend_from_slice(input.as_bytes());
    book.extend_from_slice(&[1, b'h']);
    let mut r = rig(&book, 0, 96);
    assert_cells(&r, 4); // Heading23px fixture advance24.
    let anchor = forward_three(&mut r);
    install(r.storage(), 23, &replacement(23, 48));
    r.resume();
    assert_cells(&r, 2);
    // The captured nonzero anchor is inside the heading, after its two marker bytes.
    let raw = String::from_utf8(book).unwrap();
    assert_anchor(&r, &raw, anchor);
    assert_patch(&r, r.text_margin(), 8, &REPLACED_ROWS, 3);
    assert_round_trip(&mut r);
    assert_patch(&r, r.text_margin(), 8, &REPLACED_ROWS, 3);
}

#[test]
fn live_size_cycle_preserves_raw_anchor_and_rebuilds_txt_navigation() {
    let input = TEXT.repeat(180);
    let mut r = rig(input.as_bytes(), 0, 68);
    assert_cells(&r, 4);
    let anchor = forward_three(&mut r);
    r.quick_cycle(QA_FONT_SIZE, 1);
    assert_cells(&r, 3); // 19px fixture advance20: 68 / 20 = 3.
    assert_anchor(&r, &input, anchor);
    let first = text_lines(&r)[0].chars().next().unwrap();
    assert_patch(&r, r.text_margin(), 8, &rows(19, first), 3);
    assert_round_trip(&mut r);
    walk_all(&mut r, &input);
}

#[test]
fn bookmark_reboot_with_replaced_pack_locates_the_same_raw_text() {
    let input = TEXT.repeat(180);
    let mut r = rig(input.as_bytes(), 0, 68);
    let anchor = forward_three(&mut r);
    r.save_position();
    assert_eq!(
        r.bookmark_find(BOOK.as_bytes()).unwrap().byte_offset,
        anchor
    );
    r.bookmarks_flush();
    install(r.storage(), 16, &replacement(16, 34));
    let mut reboot = Rig::new(r.into_storage());
    reboot.configure(0, 0);
    reboot.set_text_width(68);
    reboot.open(BOOK);
    assert_cells(&reboot, 2);
    assert_anchor(&reboot, &input, anchor);
    assert_patch(&reboot, reboot.text_margin(), 8, &REPLACED_ROWS, 3);
    assert_round_trip(&mut reboot);
    walk_all(&mut reboot, &input);
}

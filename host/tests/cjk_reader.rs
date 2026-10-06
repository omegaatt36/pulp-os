mod cjk_support;

use cjk_support::*;
use pulp_host::ErrorKind;
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::StorageOp;

#[test]
fn body_sizes_measure_and_draw_the_selected_pack_including_supplementary_text() {
    for (idx, px) in BODY.into_iter().enumerate() {
        let r = rig("臺灣𠮷".as_bytes(), idx as u8, 2 * (px as u32 + 1));
        assert_eq!(text_lines(&r), ["臺灣", "𠮷"], "{px}px advances");
        assert_patch(&r, r.text_margin(), 8, &rows(px, '臺'), 3);
        assert_patch(&r, r.text_margin() + px + 1, 8, &rows(px, '灣'), 3);
        assert_patch_on_line(&r, 1, r.text_margin(), 8, &rows(px, '𠮷'), 3);
    }
    // u16 advance 301 must not become u8 advance 45 before wrapping.
    let c = card("臺灣".as_bytes(), true);
    install(&c, 23, &pack(23, true));
    let mut r = Rig::new(c);
    r.configure(2, 0);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(text_lines(&r), ["臺", "灣"]);
}

#[test]
fn heading_uses_its_size_pack_without_replacing_latin_font_styles() {
    // Existing stripped-text heading markers: 01 H ... 01 h.
    let mut b = vec![1, b'H'];
    b.extend_from_slice("臺灣𠮷".as_bytes());
    b.extend_from_slice(&[1, b'h']);
    for (idx, px) in HEADING.into_iter().enumerate() {
        let r = rig(&b, idx as u8, 2 * (px as u32 + 1));
        assert_eq!(text_lines(&r), ["臺灣", "𠮷"], "heading {px}px advances");
        assert_patch(&r, r.text_margin(), 8, &rows(px, '臺'), 3);
        assert_patch(&r, r.text_margin() + px + 1, 8, &rows(px, '灣'), 3);
        assert_patch_on_line(&r, 1, r.text_margin(), 8, &rows(px, '𠮷'), 3);
    }
    // Pack contains A deliberately; the existing Latin bold/italic/heading
    // renderer must remain the same whether a CJK pack is installed or absent.
    let text = b"A\x01BAbold\x01b\n\x01IAitalic\x01i\n\x01HAheading\x01h";
    let mut plain = Rig::new(card(text, false));
    let mut installed = Rig::new(card(text, true));
    plain.open(BOOK);
    installed.open(BOOK);
    assert_eq!(plain.phase(), Phase::Ready);
    assert_eq!(installed.phase(), Phase::Ready);
    assert_eq!(plain.lines(), installed.lines());
    assert_eq!(
        render_full(&|s| plain.draw(s)).frame.to_pbm(),
        render_full(&|s| installed.draw(s)).frame.to_pbm()
    );
}

#[test]
fn absent_scalar_draws_the_explicit_hollow_box_instead_of_question_mark() {
    let r = rig("𪚥".as_bytes(), 2, 100);
    assert_eq!(text_lines(&r), ["𪚥"]);
    // 23px: side17, advance23, offset_x3, offset_y-17. Literal hollow bitmap.
    let mut box_bits = vec![0xFF, 0xFF, 0x80];
    for _ in 0..15 {
        box_bits.extend_from_slice(&[0x80, 0, 0x80]);
    }
    box_bits.extend_from_slice(&[0xFF, 0xFF, 0x80]);
    assert_patch(&r, r.text_margin() + 3, 17, &box_bits, 17);
}

#[test]
fn narrow_mixed_lines_obey_both_punctuation_sets_and_conserve_scalars() {
    let text =
        format!("{SAMPLE}Latin臺灣，臺。臺、臺？臺！臺）臺」臺』臺】臺（灣臺「灣臺『灣臺【灣");
    // >= two glyph widths: ordinary groups fit; prefix shifts stress scalar boundaries.
    for cells in [2, 3, 5] {
        for prefix in 0..3 {
            let input = format!("{}{text}", "臺".repeat(prefix));
            let r = rig(input.as_bytes(), 0, cells * 17);
            let lines = text_lines(&r);
            assert!(lines.len() > 1, "fixture actually wraps");
            assert_kinsoku(&lines);
            assert_eq!(lines.concat(), input, "cells={cells}, prefix={prefix}");
        }
    }
}

#[test]
fn page_walk_back_and_bookmark_restore_keep_the_same_text_position() {
    let input = SAMPLE.repeat(110);
    let mut r = rig(input.as_bytes(), 0, 5 * 17);
    let first = r.lines();
    r.press(Action::Next);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.page(), 1, "fixture has multiple pages");
    let second = r.lines();
    let second_offset = r.page_offsets()[1];
    assert!(input.is_char_boundary(second_offset as usize));
    r.press(Action::Prev);
    assert_eq!(r.lines(), first);
    r.press(Action::Next);
    assert_eq!(r.lines(), second);
    r.save_position();
    assert_eq!(
        r.bookmark_find(BOOK.as_bytes()).unwrap().byte_offset,
        second_offset
    );
    r.bookmarks_flush();
    let mut reboot = Rig::new(r.into_storage());
    reboot.configure(0, 0);
    reboot.set_text_width(5 * 17);
    reboot.open(BOOK);
    assert_eq!(reboot.phase(), Phase::Ready);
    assert_eq!(reboot.page_offsets()[reboot.page()], second_offset);
    assert_eq!(reboot.lines(), second);
    // Restart at zero with that bookmark removed, then visit every displayed page.
    reboot.bookmark_remove(BOOK.as_bytes());
    reboot.exit();
    reboot.open(BOOK);
    let mut actual = String::new();
    for _ in 0..512 {
        assert_eq!(reboot.phase(), Phase::Ready);
        let lines = text_lines(&reboot);
        assert_kinsoku(&lines);
        actual.push_str(&lines.concat());
        let pos = reboot.page();
        reboot.press(Action::Next);
        if reboot.page() == pos {
            break;
        }
    }
    assert_eq!(
        actual, input,
        "all scalars appear once across page boundaries"
    );
    for off in reboot.page_offsets() {
        assert!(input.is_char_boundary(off as usize));
    }
}

#[test]
fn metrics_precede_wrap_but_only_visible_bitmaps_are_loaded_and_render_reads_nothing() {
    let input = format!("{}𠮷", "臺".repeat(200));
    let r = rig(input.as_bytes(), 0, 2 * 17);
    let chars = alphabet();
    let base = 44 + 22 * chars.len() as u32;
    let tai_offset = base + 3 * chars.iter().position(|&c| c == '臺').unwrap() as u32;
    let reads = r.storage().read_log();
    let bitmap: Vec<_> = reads
        .iter()
        .filter(|q| q.path == path(16) && q.offset >= base)
        .collect();
    assert!(
        !bitmap.is_empty(),
        "visible pack bitmap actually reached the reader"
    );
    assert!(
        bitmap
            .iter()
            .all(|q| q.offset == tai_offset && q.requested == 3),
        "nonvisible supplementary glyph must not load a bitmap"
    );
    assert!(text_lines(&r).iter().all(|line| line == "臺臺"));
    r.storage().reset_reads();
    let full = render_full(&|s| r.draw(s)).frame.to_pbm();
    assert_eq!(full, render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(full, render_full(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(
        r.storage().read_count(),
        0,
        "draw/get never access the SD source"
    );
}

#[test]
fn required_pack_failures_are_recoverable_and_never_publish_invisible_ready_text() {
    for failure in ["corrupt", "read"] {
        let c = card("臺𠮷".as_bytes(), true);
        if failure == "corrupt" {
            install(&c, 23, b"not a pack");
        }
        if failure == "read" {
            c.inject_error(StorageOp::Read, &path(23), 1, ErrorKind::ReadFailed);
        }
        let mut r = Rig::new(c);
        r.configure(2, 0);
        r.open(BOOK);
        assert_eq!(r.phase(), Phase::Error, "required pack {failure}");
        assert!(r.error_kind().is_some(), "recoverable failure has a cause");
        if failure == "read" {
            assert_eq!(r.storage().pending_injections(), 0);
        }
        install(r.storage(), 23, &pack(23, false));
        r.open(BOOK);
        assert_eq!(r.phase(), Phase::Ready, "retry {failure}");
        assert_eq!(text_lines(&r).concat(), "臺𠮷");
        assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);
    }
}

#[test]
fn uninstalled_pack_keeps_ready_text_with_size_specific_boxes_until_install_and_reopen() {
    let mut r = Rig::new(card("臺𠮷".as_bytes(), false));
    r.configure(2, 0);
    r.open(BOOK);
    assert_eq!(
        r.phase(),
        Phase::Ready,
        "an optional pack need not be installed"
    );
    assert_eq!(text_lines(&r).concat(), "臺𠮷");
    // Literal23px missing box:17x17, advance23, offset_x3, offset_y-17.
    let mut box_bits = vec![0xFF, 0xFF, 0x80];
    for _ in 0..15 {
        box_bits.extend_from_slice(&[0x80, 0, 0x80]);
    }
    box_bits.extend_from_slice(&[0xFF, 0xFF, 0x80]);
    r.storage().reset_reads();
    assert_patch(&r, r.text_margin() + 3, 17, &box_bits, 17);
    assert_patch(&r, r.text_margin() + 26, 17, &box_bits, 17);
    assert_eq!(
        r.storage().read_count(),
        0,
        "synthetic box draw reads no SD"
    );
    install(r.storage(), 23, &pack(23, false));
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(text_lines(&r).concat(), "臺𠮷");
    assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);
    assert_patch(&r, r.text_margin() + 24, 8, &rows(23, '𠮷'), 3);
}

#[test]
fn inseparable_punctuation_at_tiny_width_makes_progress_without_losing_text() {
    // Explicit edge policy: an inseparable pair may overhang a one-glyph width.
    let input = "臺（灣），𠮷。";
    let r = rig(input.as_bytes(), 0, 17);
    let lines = text_lines(&r);
    assert_kinsoku(&lines);
    assert_eq!(lines.concat(), input);
    assert!(
        !lines.iter().any(String::is_empty),
        "no empty-line progress loop"
    );
}

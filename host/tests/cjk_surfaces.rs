mod cjk_support;

use core::fmt::Write;

use cjk_support::{BODY, SIZES, alphabet, install, pack, path, rows};
use pulp_host::apps::widgets::{BitmapDynLabel, BitmapLabel};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
};
use pulp_host::fonts::cjk::CjkState;
use pulp_host::fonts::{self, FontSet, Style};
use pulp_host::kernel::Kernel;
use pulp_host::reader::{Action, Phase, QA_TOC, Rig};
use pulp_host::render::{Framebuffer, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;
use pulp_host::ui::{Alignment, Region};

const BOOK: &str = "SURFACES.EPUB";

fn book(title: &str, labels: &[&str]) -> Vec<u8> {
    build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: title.into(),
        author: "Fixture".into(),
        identifier: "surface-fixture".into(),
        chapters: (0..2)
            .map(|_| Chapter {
                title: "Latin heading".into(),
                blocks: vec![Block::Paragraph(vec![Run::Text("Latin body.".into())])],
            })
            .collect(),
        toc: labels
            .iter()
            .map(|s| TocItem {
                title: (*s).into(),
                chapter: 1,
                children: vec![],
            })
            .collect(),
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap()
}

fn card(title: &str, labels: &[&str], packs: bool) -> VirtualStorage {
    let bytes = book(title, labels);
    let card = VirtualStorage::memory_with(&[(BOOK, bytes.as_slice())]);
    card.ensure_pulp_dir().unwrap();
    if packs {
        for px in SIZES {
            install(&card, px, &pack(px, false));
        }
    }
    card
}

fn open(card: VirtualStorage, idx: u8) -> Rig {
    let mut r = Rig::new(card);
    r.configure(idx, 0);
    r.open(BOOK);
    r.prepare_render();
    r
}

fn matches(f: &Framebuffer, x: u16, y: u16, px: u16, ch: char, inverted: bool) -> bool {
    rows(px, ch).iter().enumerate().all(|(dy, row)| {
        (0..8).all(|dx| {
            let ink = row & (0x80 >> dx) != 0;
            f.is_black(x + dx, y + dy as u16) == (ink != inverted)
        })
    })
}

fn glyph_in_band(f: &Framebuffer, x: u16, top: u16, end: u16, px: u16, ch: char, inv: bool) {
    assert!(
        (top..end.saturating_sub(2)).any(|y| matches(f, x, y, px, ch, inv)),
        "literal {px}px {ch} glyph missing at x{x} in y{top}..{end}, inverted={inv}"
    );
}

fn glyph_bitmap_reads(card: &VirtualStorage, px: u16, ch: char) -> usize {
    let chars = alphabet();
    let base = 44 + 22 * chars.len() as u32;
    let offset = base + 3 * chars.iter().position(|&c| c == ch).unwrap() as u32;
    card.read_log()
        .iter()
        .filter(|r| r.path == path(px) && r.offset == offset && r.requested == 3)
        .count()
}

#[test]
fn book_title_uses_16px_chrome_independently_of_body_size_and_draw_reads_nothing() {
    for idx in [0, 4] {
        let r = open(card("臺灣𠮷", &["Latin TOC"], true), idx);
        assert_eq!(r.phase(), Phase::Ready);
        assert_eq!(r.epub_title(), "臺灣𠮷");
        r.storage().reset_reads();
        let f = render_full(&|s| r.draw(s)).frame;
        glyph_in_band(&f, 8, 0, r.text_y(), 16, '臺', false);
        glyph_in_band(&f, 25, 0, r.text_y(), 16, '灣', false);
        glyph_in_band(&f, 42, 0, r.text_y(), 16, '𠮷', false);
        assert_eq!(f.to_pbm(), render_stitched(&|s| r.draw(s)).frame.to_pbm());
        assert_eq!(
            r.storage().read_count(),
            0,
            "all title draw passes use prepared glyphs"
        );
    }
}

#[test]
fn visible_toc_uses_body_size_regular_fallback_and_selected_foreground() {
    for (idx, px) in BODY.into_iter().enumerate() {
        let mut r = open(card("Latin title", &["臺灣", "𠮷"], true), idx as u8);
        assert_eq!(r.phase(), Phase::Ready);
        r.quick_trigger(QA_TOC);
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Toc);
        assert_eq!(r.toc_selected(), 0);
        r.storage().reset_reads();
        let f = render_full(&|s| r.draw(s)).frame;
        let top = r.text_y();
        let line_h = r.font_line_h();
        glyph_in_band(&f, r.text_margin(), top, top + line_h, px, '臺', true);
        glyph_in_band(
            &f,
            r.text_margin() + px + 1,
            top,
            top + line_h,
            px,
            '灣',
            true,
        );
        glyph_in_band(
            &f,
            r.text_margin(),
            top + line_h,
            top + 2 * line_h,
            px,
            '𠮷',
            false,
        );
        assert_eq!(f.to_pbm(), render_stitched(&|s| r.draw(s)).frame.to_pbm());
        assert_eq!(
            r.storage().read_count(),
            0,
            "TOC inverse and normal draw read no SD font bytes"
        );
    }
}

#[test]
fn toc_scroll_prepares_newly_visible_glyph_before_the_next_draw() {
    let mut labels = vec!["臺"; 36];
    labels[35] = "𠮷";
    let mut r = open(card("Latin title", &labels, true), 4);
    assert_eq!(r.phase(), Phase::Ready);
    r.quick_trigger(QA_TOC);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Toc);
    assert!(
        glyph_bitmap_reads(r.storage(), 35, '臺') > 0,
        "visible label bitmap was prepared"
    );
    assert_eq!(
        glyph_bitmap_reads(r.storage(), 35, '𠮷'),
        0,
        "invisible TOC label is not prepared eagerly"
    );
    r.storage().reset_reads();
    for _ in 0..35 {
        r.press(Action::Next);
        r.prepare_render();
    }
    assert_eq!(r.toc_selected(), 35);
    assert!(
        glyph_bitmap_reads(r.storage(), 35, '𠮷') > 0,
        "event made a new glyph visible before render"
    );
    r.storage().reset_reads();
    let f = render_full(&|s| r.draw(s)).frame;
    glyph_in_band(
        &f,
        r.text_margin(),
        r.text_y(),
        r.text_y() + r.text_area_h(),
        35,
        '𠮷',
        true,
    );
    assert_eq!(f.to_pbm(), render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(
        r.storage().read_count(),
        0,
        "scroll redraw uses only prepared glyphs"
    );
}

#[test]
fn uninstalled_title_pack_draws_a_size_specific_box_and_reopen_uses_installed_pack() {
    let mut r = open(card("臺", &["Latin TOC"], false), 2);
    assert_eq!(
        r.phase(),
        Phase::Ready,
        "an optional title pack need not be installed"
    );
    r.storage().reset_reads();
    let fallback = render_full(&|s| r.draw(s)).frame;
    // Literal16px missing box:12x12, advance16, offset_x2, offset_y-12.
    let mut box_bits = vec![0xFF, 0xF0];
    for _ in 0..10 {
        box_bits.extend_from_slice(&[0x80, 0x10]);
    }
    box_bits.extend_from_slice(&[0xFF, 0xF0]);
    assert!(
        (0..r.text_y().saturating_sub(11)).any(|y| {
            (0..12).all(|dy| {
                (0..12).all(|dx| {
                    let ink = box_bits[dy * 2 + dx / 8] & (0x80 >> (dx % 8)) != 0;
                    fallback.is_black(10 + dx as u16, y + dy as u16) == ink
                })
            })
        }),
        "literal16px12x12 title missing box must appear at x10 in the header"
    );
    assert_eq!(
        r.storage().read_count(),
        0,
        "synthetic title draw reads no SD"
    );
    install(r.storage(), 16, &pack(16, false));
    r.open(BOOK);
    r.prepare_render();
    assert_eq!(
        r.phase(),
        Phase::Ready,
        "installing title's pack permits real glyph rendering"
    );
    let f = render_full(&|s| r.draw(s)).frame;
    glyph_in_band(&f, 8, 0, r.text_y(), 16, '臺', false);
}

#[test]
fn dynamic_label_set_text_keeps_the_longest_complete_scalar_prefix() {
    let mut label = BitmapDynLabel::<7>::new(Region::new(0, 0, 100, 40), fonts::body_font(0));
    label.set_text("A臺𠮷Z");
    assert_eq!(
        label.text(),
        "A臺",
        "partial supplementary scalar must not erase preceding text"
    );
    label.set_text("𠮷臺");
    assert_eq!(
        label.text(),
        "𠮷臺",
        "exact capacity includes both complete scalars"
    );
    let mut tiny = BitmapDynLabel::<3>::new(Region::new(0, 0, 100, 40), fonts::body_font(0));
    tiny.set_text("𠮷臺");
    assert_eq!(
        tiny.text(),
        "",
        "a later scalar cannot replace an unfit first scalar"
    );
}

#[test]
fn dynamic_label_write_str_keeps_existing_text_and_a_complete_scalar_prefix() {
    let mut label = BitmapDynLabel::<7>::new(Region::new(0, 0, 100, 40), fonts::body_font(0));
    label.write_str("A").unwrap();
    label.write_str("臺𠮷Z").unwrap();
    assert_eq!(
        label.text(),
        "A臺",
        "append truncation cannot invalidate existing UTF-8"
    );
    label.clear_text();
    label.write_str("𠮷").unwrap();
    label.write_str("臺").unwrap();
    assert_eq!(
        label.text(),
        "𠮷臺",
        "successive writes fill an exact scalar boundary"
    );
}

#[test]
fn prepared_static_and_dynamic_labels_keep_latin_style_alignment_and_inversion() {
    let card = card("Latin title", &["Latin TOC"], true);
    let mut kernel = Kernel::new(SdStorage::new(card));
    let latin = FontSet::for_size(0);
    let mut cjk = CjkState::new();
    cjk.prepare_text(&mut kernel.handle(), "A臺灣".as_bytes(), latin, 16, 23)
        .expect("prepare UI text through the same pack provider as the reader");
    let prepared = cjk.view(latin, 16, 23);
    let region = Region::new(16, 100, 200, 50);
    kernel.sd().card.reset_reads();

    // Existing static Latin font tables remain authoritative for Latin pixels.
    for (style, alignment, inverted) in [
        (Style::Regular, Alignment::CenterLeft, false),
        (Style::Bold, Alignment::Center, true),
        (Style::Italic, Alignment::CenterRight, false),
        (Style::Heading, Alignment::BottomRight, true),
    ] {
        let label = BitmapLabel::new(region, "A italic Bold", latin.font(style))
            .alignment(alignment)
            .inverted(inverted);
        let original = render_full(&|s| label.draw(s).unwrap()).frame.to_pbm();
        let upgraded = render_full(&|s| label.draw_prepared(s, &prepared).unwrap())
            .frame
            .to_pbm();
        assert_eq!(
            upgraded, original,
            "Latin {style:?} pixels and placement remain unchanged"
        );
    }

    for (style, alignment, inverted, divisor) in [
        (Style::Regular, Alignment::CenterLeft, false, 0),
        (Style::Bold, Alignment::Center, true, 2),
        (Style::Italic, Alignment::CenterRight, false, 1),
    ] {
        let font = latin.font(style);
        let label = BitmapLabel::new(region, "A臺灣", font)
            .alignment(alignment)
            .inverted(inverted);
        let mut dynamic = BitmapDynLabel::<16>::new(region, font)
            .alignment(alignment)
            .inverted(inverted);
        dynamic.set_text("A臺灣");
        let f = render_full(&|s| label.draw_prepared(s, &prepared).unwrap()).frame;
        let dynamic_frame = render_full(&|s| dynamic.draw_prepared(s, &prepared).unwrap()).frame;
        assert_eq!(
            f.to_pbm(),
            dynamic_frame.to_pbm(),
            "static and dynamic labels share fallback"
        );
        let latin_advance = font.advance('A') as u16;
        let total_width = latin_advance + 2 * 17;
        let x = region.x
            + if divisor == 0 {
                0
            } else {
                (region.w - total_width) / divisor
            };
        glyph_in_band(
            &f,
            x + latin_advance,
            region.y,
            region.y + region.h,
            16,
            '臺',
            inverted,
        );
        glyph_in_band(
            &f,
            x + latin_advance + 17,
            region.y,
            region.y + region.h,
            16,
            '灣',
            inverted,
        );
        assert_eq!(
            f.to_pbm(),
            render_stitched(&|s| label.draw_prepared(s, &prepared).unwrap())
                .frame
                .to_pbm()
        );
    }
    assert_eq!(
        kernel.sd().card.read_count(),
        0,
        "prepared widget drawing performs no font I/O"
    );
}

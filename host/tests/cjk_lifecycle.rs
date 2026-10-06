mod cjk_support;

use cjk_support::{SIZES, assert_patch_on_line, install, pack, rows, text_lines};
use pulp_host::fixtures::{Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub};
use pulp_host::reader::{Action, Phase, QA_TOC, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

const BOOK: &str = "LIFECYCLE.EPUB";

fn open(packs: bool) -> Rig {
    let bytes = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "Lifecycle".into(),
        author: "Fixture".into(),
        identifier: "urn:pulp:lifecycle".into(),
        chapters: vec![Chapter {
            title: "A".into(),
            blocks: vec![Block::Paragraph(vec![Run::Text("臺灣𠮷".into())])],
        }],
        toc: vec![TocItem { title: "繁體".into(), chapter: 0, children: vec![] }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    }).unwrap();
    let card = VirtualStorage::memory_with(&[(BOOK, bytes.as_slice())]);
    card.ensure_pulp_dir().unwrap();
    if packs {
        for px in SIZES { install(&card, px, &pack(px, false)); }
    }
    let mut r = Rig::new(card);
    r.configure(2, 0);
    // Literal fixture advance is 24, so exactly two body scalars fit.
    r.set_text_width(48);
    r.open(BOOK);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    r
}

fn assert_body(r: &Rig) {
    eprintln!("body bytes {:?}, width {}", r.lines(), r.text_w());
    let lines = text_lines(r);
    assert_eq!(lines.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>(), ["A", "臺灣", "𠮷"], "literal 24px advance preserves wrapping and text");
    let body = lines.iter().position(|s| s == "臺灣").unwrap() as u16;
    assert_patch_on_line(r, body, r.text_margin(), 8, &rows(23, '臺'), 3);
    assert_patch_on_line(r, body, r.text_margin() + 24, 8, &rows(23, '灣'), 3);
    assert_patch_on_line(r, body + 1, r.text_margin(), 8, &rows(23, '𠮷'), 3);
}

fn draw_without_reads(r: &Rig) -> Vec<u8> {
    r.storage().reset_reads();
    let full = render_full(&|s| r.draw(s)).frame.to_pbm();
    assert_eq!(full, render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(r.storage().read_count(), 0, "all draw strips use prepared glyphs");
    full
}

#[test]
fn literal_body_control_distinguishes_prepared_pack_from_no_pack() {
    let missing = open(false);
    let missing_pixels = draw_without_reads(&missing);
    let prepared = open(true);
    assert_body(&prepared);
    assert_ne!(draw_without_reads(&prepared), missing_pixels);
}

#[test]
fn suspended_toc_resume_back_restores_original_body_glyphs_and_pixels() {
    let mut r = open(true);
    assert_body(&r);
    let original_lines = r.lines();
    let original = draw_without_reads(&r);
    r.quick_trigger(QA_TOC);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Toc);
    assert_eq!(r.toc_entries(), [("繁體".into(), 0)]);
    draw_without_reads(&r);
    r.suspend();
    r.resume();
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Toc);
    draw_without_reads(&r);
    r.press(Action::Back);
    r.prepare_render();
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.lines(), original_lines, "return restores the original page text");
    assert_body(&r);
    assert_eq!(draw_without_reads(&r), original, "return restores every full and stitched pixel");
}

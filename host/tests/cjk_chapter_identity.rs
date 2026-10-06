mod cjk_support;

use cjk_support::*;
use pulp_host::fixtures::{Block, Chapter, Compression, EpubSpec, EpubVersion, Run, build_epub};
use pulp_host::reader::{Action, Phase, QA_NEXT_CHAPTER, QA_PREV_CHAPTER, Rig};
use pulp_host::storage::VirtualStorage;

fn replacement() -> Vec<u8> {
    let mut bytes = pack(16, false);
    bytes[8..16].copy_from_slice(&0x1234_0000_0000_0010u64.to_le_bytes());
    for record in 0..alphabet().len() {
        let offset = 44 + 22 * record + 12;
        bytes[offset..offset + 2].copy_from_slice(&34u16.to_le_bytes());
    }
    bytes
}

#[test]
fn replacing_pack_during_chapter_jump_does_not_reuse_the_departed_chapter_anchor() {
    let chapters = ["臺灣𠮷".repeat(240), "繁體中文".repeat(240)];
    let spec = EpubSpec {
        version: EpubVersion::V3,
        title: "臺灣".into(),
        author: "臺灣".into(),
        identifier: "urn:pulp:chapter-identity".into(),
        chapters: chapters
            .iter()
            .enumerate()
            .map(|(i, text)| Chapter {
                title: if i == 0 { "臺" } else { "繁" }.into(),
                blocks: vec![Block::Paragraph(vec![Run::Text(text.clone())])],
            })
            .collect(),
        toc: vec![],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    };
    let epub = build_epub(&spec).unwrap();
    let storage = VirtualStorage::memory_with(&[("CHAPTER.EPUB", &epub)]);
    for px in SIZES {
        install(&storage, px, &pack(px, false));
    }
    let mut r = Rig::new(storage);
    r.configure(0, 0);
    r.set_text_width(68);
    r.open("CHAPTER.EPUB");
    assert_eq!(r.phase(), Phase::Ready);
    for _ in 0..400 {
        if !r.has_bg_work() {
            break;
        }
        r.idle(1);
    }
    assert!(!r.has_bg_work());
    for _ in 0..3 {
        r.press(Action::Next);
    }
    assert_eq!((r.chapter(), r.page()), (0, 3));
    assert!(r.page_offsets()[r.page()] > 0);

    install(r.storage(), 16, &replacement());
    r.quick_trigger(QA_NEXT_CHAPTER);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(
        (r.chapter(), r.page()),
        (1, 0),
        "forward chapter navigation starts the selected chapter, without the old raw anchor"
    );
    assert_eq!(r.page_offsets()[0], 0);

    let mut actual = String::new();
    for _ in 0..512 {
        actual.extend(
            text_lines(&r)
                .concat()
                .chars()
                .filter(|c| !c.is_whitespace()),
        );
        let before = (r.chapter(), r.page());
        r.press(Action::Next);
        if (r.chapter(), r.page()) == before {
            break;
        }
        assert_eq!(r.chapter(), 1);
    }
    assert_eq!(
        actual,
        format!("繁{}", chapters[1]),
        "the selected chapter conserves every scalar under the replacement layout"
    );
    r.quick_trigger(QA_PREV_CHAPTER);
    assert_eq!(
        (r.chapter(), r.page()),
        (0, 0),
        "previous-chapter quick action keeps its first-page destination"
    );
    r.quick_trigger(QA_NEXT_CHAPTER);
    r.press(Action::Prev);
    assert_eq!(r.chapter(), 0);
    assert_eq!(
        r.page() + 1,
        r.total_pages(),
        "ordinary backward paging enters the previous chapter's last page"
    );
}

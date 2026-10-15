// A book for checking vertical writing on the device (`device-fixtures` writes it
// to disk; the host tests open it): zh-TW, right-to-left spine, so columns. Ruby
// (only the base text may show), brackets, dashes, ellipsis, Latin text and
// digits, and an inline picture in chapter 2.

use super::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, Run,
    TocItem, Writing, build_epub_writing,
};

fn text(s: &str) -> Run {
    Run::Text(s.into())
}

fn ruby(base: &str, reading: &str) -> Run {
    Run::Ruby {
        base: base.into(),
        reading: reading.into(),
    }
}

pub fn vertical_book() -> Vec<u8> {
    let long = "這是一段用來撐滿多個直排欄位的繁體中文文字，".repeat(6);
    let paragraphs = |n: usize| -> Vec<Block> {
        (0..n)
            .flat_map(|_| {
                [
                    Block::Paragraph(vec![
                        ruby("漢", "ㄏㄢˋ"),
                        ruby("字", "ㄗˋ"),
                        text(
                            "直排測試：「引號內的文字」，以及（括號）、『二重引號』與【粗括號】。",
                        ),
                    ]),
                    Block::Paragraph(vec![text(
                        "破折號——省略號……驚嘆號！問號？分號；冒號：頓號、句號。",
                    )]),
                    Block::Paragraph(vec![text(
                        "Latin text ABC abc 與數字 2026 10 15 混排，English words wrap sideways.",
                    )]),
                    Block::Paragraph(vec![text(&format!("長句測試：{long}結束。"))]),
                ]
            })
            .collect()
    };
    let mut first = vec![Block::Heading("第一章　直排與注音".into())];
    first.extend(paragraphs(6));
    let mut second = vec![
        Block::Heading("第二章　圖片".into()),
        Block::Paragraph(vec![text("下面是一張內嵌圖片：")]),
        Block::Image(0),
        Block::Paragraph(vec![text("圖片之後的文字，繼續直排。")]),
    ];
    second.extend(paragraphs(3));
    let chapters = vec![
        Chapter {
            title: "第一章 直排與注音".into(),
            blocks: first,
        },
        Chapter {
            title: "第二章 圖片".into(),
            blocks: second,
        },
    ];
    let toc = chapters
        .iter()
        .enumerate()
        .map(|(chapter, c)| TocItem {
            title: c.title.clone(),
            chapter,
            children: vec![],
        })
        .collect();
    let spec = EpubSpec {
        version: EpubVersion::V3,
        title: "直排注音測試".into(),
        author: "pulp-os".into(),
        identifier: "urn:pulp:device-fixture:vertical".into(),
        chapters,
        toc,
        images: vec![ImageSpec {
            path: "images/pic.jpg".into(),
            kind: ImageKind::Jpeg,
            width: 400,
            height: 600,
            pattern: Pattern::HorizontalGradient,
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    };
    let writing = Writing {
        language: "zh-TW",
        page_progression: Some("rtl"),
    };
    build_epub_writing(&spec, &writing).expect("the vertical book is valid")
}

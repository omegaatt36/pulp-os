// Books for checking ruby and vertical writing on the device (`device-fixtures` writes
// them to disk; the host tests open them): Japanese with hiragana readings, once with
// a right-to-left spine, so columns, and once in lines. Ruby over bases of one and of
// several glyphs, readings shorter and longer than their base, brackets, dashes,
// ellipsis, Latin text and digits, and an inline picture in chapter 2.

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

/// Set in columns: the spine reads right to left.
pub fn vertical_book() -> Vec<u8> {
    ruby_book(&Writing {
        language: "ja",
        page_progression: Some("rtl"),
    })
}

/// Set in lines.
pub fn horizontal_book() -> Vec<u8> {
    ruby_book(&Writing {
        language: "ja",
        page_progression: None,
    })
}

fn ruby_book(writing: &Writing<'_>) -> Vec<u8> {
    let long = "これは複数の列を埋めるための長い日本語の文章で、ルビのない行も続きます。".repeat(6);
    let paragraphs = |n: usize| -> Vec<Block> {
        (0..n)
            .flat_map(|_| {
                [
                    Block::Paragraph(vec![
                        ruby("漢字", "かんじ"),
                        text("の"),
                        ruby("振", "ふ"),
                        text("り"),
                        ruby("仮名", "がな"),
                        text("：「引用」、（括弧）、『二重かぎ』と【すみ付き括弧】。"),
                    ]),
                    Block::Paragraph(vec![
                        ruby("私", "わたし"),
                        text("は"),
                        ruby("日本語", "にほんご"),
                        text("を"),
                        ruby("勉強", "べんきょう"),
                        text("しています。読みが基の文字より長い"),
                        ruby("例", "たとえ"),
                        text("と短い"),
                        ruby("例", "れい"),
                        text("です。"),
                    ]),
                    Block::Paragraph(vec![text(
                        "ダッシュ——三点リーダ……感嘆符！疑問符？セミコロン；コロン：読点、句点。",
                    )]),
                    Block::Paragraph(vec![text(
                        "Latin text ABC abc と数字 2026 10 15 の混在、English words wrap sideways.",
                    )]),
                    Block::Paragraph(vec![text(&format!("長文テスト：{long}おわり。"))]),
                ]
            })
            .collect()
    };
    let mut first = vec![Block::Heading("第一章　縦書きとルビ".into())];
    first.extend(paragraphs(6));
    let mut second = vec![
        Block::Heading("第二章　画像".into()),
        Block::Paragraph(vec![text("次は埋め込み画像です：")]),
        Block::Image(0),
        Block::Paragraph(vec![text("画像のあとも、文章は続きます。")]),
    ];
    second.extend(paragraphs(3));
    let chapters = vec![
        Chapter {
            title: "第一章 縦書きとルビ".into(),
            blocks: first,
        },
        Chapter {
            title: "第二章 画像".into(),
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
        title: "ルビ表示テスト".into(),
        author: "pulp-os".into(),
        identifier: "urn:pulp:device-fixture:ruby".into(),
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
    build_epub_writing(&spec, writing).expect("the ruby book is valid")
}

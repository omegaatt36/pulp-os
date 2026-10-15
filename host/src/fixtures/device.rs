// Books for checking vertical writing and progressive JPEG on the device
// (`device-fixtures` writes them to disk; the host tests open them).
//
// - `vertical_book`: zh-TW, right-to-left spine, so columns. Ruby (only the base
//   text may show), brackets, dashes, ellipsis, Latin text and digits, and an
//   inline picture in chapter 2.
// - `jpeg_book`: one 1200x1800 picture twice, progressive in chapter 1 and
//   baseline in chapter 2 (the same quantised coefficients), for timing and
//   quality A/B. The firmware needs `--features progressive-jpeg`.
//
// The pictures come from a real encoder: the synthetic ones in `image` are
// DC-only, so a progressive file made of them would have a single scan.

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use super::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Pattern, Run,
    TocItem, Writing, build_epub_encoded,
};

pub const WIDTH: u16 = 1200;
pub const HEIGHT: u16 = 1800;
const QUALITY: u8 = 85;

// A photograph-like picture: smooth colour fields, soft blobs and a little noise,
// so that every block carries AC coefficients (a progressive file needs them).
pub fn plasma() -> Vec<u8> {
    let (w, h) = (usize::from(WIDTH), usize::from(HEIGHT));
    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut seed = 12345u32;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let mut r = 0.5 + 0.35 * (6.0 * u + 3.0 * v).sin() + 0.15 * (23.0 * v).sin();
            let mut g = 0.5 + 0.35 * (5.0 * v - 2.0 * u + 1.0).sin() + 0.15 * (31.0 * u).sin();
            let mut b = 0.5 + 0.35 * (4.0 * (u + v) + 2.0).sin();
            for (cx, cy, radius) in [(0.3, 0.3, 0.2), (0.7, 0.6, 0.25), (0.45, 0.85, 0.15)] {
                let d = ((u - cx).powi(2) + (v - cy).powi(2)) / (radius * radius);
                let k = (-d).exp();
                r += 0.5 * k;
                g += 0.3 * k;
                b -= 0.2 * k;
            }
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345) & 0x7FFF_FFFF;
            let noise = ((seed >> 16) & 15) as f32 / 15.0 - 0.5;
            for c in [r, g, b] {
                rgb.push(((c + 0.06 * noise) * 200.0 + 25.0).clamp(0.0, 255.0) as u8);
            }
        }
    }
    rgb
}

fn jpeg(rgb: &[u8], progressive: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, QUALITY);
    encoder.set_sampling_factor(SamplingFactor::F_2_2);
    encoder.set_progressive(progressive);
    encoder
        .encode(rgb, WIDTH, HEIGHT, ColorType::Rgb)
        .expect("the picture encodes");
    out
}

fn picture(path: &str) -> ImageSpec {
    ImageSpec {
        path: path.into(),
        kind: ImageKind::Jpeg,
        width: WIDTH,
        height: HEIGHT,
        pattern: Pattern::White,
    }
}

fn text(s: &str) -> Run {
    Run::Text(s.into())
}

fn ruby(base: &str, reading: &str) -> Run {
    Run::Ruby {
        base: base.into(),
        reading: reading.into(),
    }
}

fn toc(title: &str, chapter: usize) -> TocItem {
    TocItem {
        title: title.into(),
        chapter,
        children: vec![],
    }
}

fn spec(title: &str, chapters: Vec<Chapter>, images: Vec<ImageSpec>) -> EpubSpec {
    let toc = chapters
        .iter()
        .enumerate()
        .map(|(i, c)| toc(&c.title, i))
        .collect();
    EpubSpec {
        version: EpubVersion::V3,
        title: title.into(),
        author: "pulp-os".into(),
        identifier: format!("urn:pulp:device-fixture:{title}"),
        chapters,
        toc,
        images,
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    }
}

pub fn vertical_book(rgb: &[u8]) -> Vec<u8> {
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
    let spec = spec(
        "直排注音測試",
        vec![
            Chapter {
                title: "第一章 直排與注音".into(),
                blocks: first,
            },
            Chapter {
                title: "第二章 圖片".into(),
                blocks: second,
            },
        ],
        vec![picture("images/pic.jpg")],
    );
    let writing = Writing {
        language: "zh-TW",
        page_progression: Some("rtl"),
    };
    let baseline = jpeg(rgb, false);
    build_epub_encoded(&spec, &writing, |_| baseline.clone()).expect("the vertical book is valid")
}

pub fn jpeg_book(rgb: &[u8]) -> Vec<u8> {
    let filler = "The quick brown fox jumps over the lazy dog. ".repeat(12);
    let chapter = |title: &str, image: usize| Chapter {
        title: title.into(),
        blocks: vec![
            Block::Heading(title.into()),
            Block::Paragraph(vec![text(&filler)]),
            Block::Image(image),
            Block::Paragraph(vec![text(&filler)]),
        ],
    };
    let spec = spec(
        "Progressive vs baseline JPEG",
        vec![chapter("Progressive", 0), chapter("Baseline", 1)],
        vec![picture("images/prog.jpg"), picture("images/base.jpg")],
    );
    let (progressive, baseline) = (jpeg(rgb, true), jpeg(rgb, false));
    build_epub_encoded(&spec, &Writing::LATIN, |image| {
        if image.path.ends_with("prog.jpg") {
            progressive.clone()
        } else {
            baseline.clone()
        }
    })
    .expect("the JPEG book is valid")
}

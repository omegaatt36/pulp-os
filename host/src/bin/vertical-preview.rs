//! Export vertical pages of the actual ReaderApp as PBM; observations, not a golden oracle.
//!
//! usage: vertical-preview OUT_DIR PACK_DIR [FONT_IDX [PAGES]]
//!
//! PACK_DIR holds the `F000nn.PFN` packs `pulp-fontconv` writes. The book is a
//! synthetic EPUB whose package declares a right-to-left spine and a Japanese
//! (chapter 1) or Chinese (chapter 2) body, so the reader sets it in columns
//! exactly as it would a real vertical book.
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, Writing, build_epub_writing,
};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

const SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];

// Natsume Soseki, "Wagahai wa neko de aru" (1905); public domain
const NEKO: [&str; 3] = [
    "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。",
    "吾輩はここで始めて人間というものを見た。しかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。「この書生というのは時々我々を捕えて煮て食うという話である。」",
    "『ニャー』と鳴いてみたが誰も来ない……ただ薄暗い――そう、ただそれだけの所であった。（二〇二六年）",
];

// Su Shi, "Chi bi fu" (1082); public domain
const CHIBI: [&str; 3] = [
    "壬戌之秋，七月既望，蘇子與客泛舟，遊於赤壁之下。清風徐來，水波不興。舉酒屬客，誦明月之詩，歌窈窕之章。",
    "少焉，月出於東山之上，徘徊於斗牛之間。白露橫江，水光接天。縱一葦之所如，凌萬頃之茫然。浩浩乎如馮虛御風，而不知其所止；飄飄乎如遺世獨立，羽化而登仙。",
    "客曰：「月明星稀，烏鵲南飛，此非曹孟德之詩乎？」版本 v2.0 於 2026-10-12 以 Rust 重製，共 3 章。——完⋯⋯",
];

fn chapter(title: &str, paragraphs: &[&str]) -> Chapter {
    let mut blocks = vec![Block::Heading(title.into())];
    blocks.extend(
        paragraphs
            .iter()
            .map(|p| Block::Paragraph(vec![Run::Text((*p).repeat(2))])),
    );
    Chapter {
        title: title.into(),
        blocks,
    }
}

fn book(language: &str, title: &str, paragraphs: &[&str]) -> Vec<u8> {
    build_epub_writing(
        &EpubSpec {
            version: EpubVersion::V3,
            title: title.into(),
            author: "Observation".into(),
            identifier: format!("vertical-{language}"),
            chapters: vec![chapter(title, paragraphs)],
            toc: vec![TocItem {
                title: title.into(),
                chapter: 0,
                children: vec![],
            }],
            images: vec![],
            cover: None,
            compression: Compression::Stored,
            numeric_entities: false,
        },
        &Writing {
            language,
            page_progression: Some("rtl"),
        },
    )
    .unwrap()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("output directory"));
    let packs = std::path::PathBuf::from(args.next().expect("pack directory"));
    let font: u8 = args.next().map_or(2, |v| v.parse().expect("font index"));
    let pages: usize = args.next().map_or(2, |v| v.parse().expect("pages"));
    std::fs::create_dir_all(&dir).unwrap();
    let books = [
        ("ja", book("ja", "吾輩は猫である", &NEKO)),
        ("zh", book("zh-TW", "赤壁賦", &CHIBI)),
    ];
    for (name, bytes) in &books {
        let file = format!("{}.EPUB", name.to_uppercase());
        let card = VirtualStorage::memory_with(&[(file.as_str(), bytes.as_slice())]);
        card.ensure_pulp_dir().unwrap();
        card.ensure_pulp_subdir("FONTS").unwrap();
        for px in SIZES {
            let pack = format!("F{px:05}.PFN");
            card.write_in_pulp_subdir("FONTS", &pack, &std::fs::read(packs.join(&pack)).unwrap())
                .unwrap();
        }
        let mut r = Rig::new(card);
        r.configure(font, 1);
        r.open(&file);
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Ready, "{name}");
        for page in 0..pages {
            let full = render_full(&|s| r.draw(s)).frame;
            let stitched = render_stitched(&|s| r.draw(s)).frame;
            assert_eq!(full.to_pbm(), stitched.to_pbm(), "{name} page {page}");
            let out = dir.join(format!("{name}-font{font}-page{page}.pbm"));
            full.write_pbm(&out).unwrap();
            println!(
                "{name} page {page}: {} columns, {} black px -> {}",
                r.lines().len(),
                full.black_count(),
                out.display()
            );
            r.press(Action::Next);
            r.prepare_render();
        }
    }
}

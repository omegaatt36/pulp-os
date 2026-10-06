//! Export observations from the actual ReaderApp; this is not a golden oracle.
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
};
use pulp_host::reader::{Phase, QA_TOC, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

const SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];
const SAMPLE: &str = "臺灣「繁體中文」，閱讀測試。山水之間，日月星辰。𪚥";

fn export(r: &mut Rig, dir: &std::path::Path, name: &str) {
    let reads = r.storage().read_log();
    let fonts: Vec<_> = reads.iter().filter(|v| v.path.ends_with(".PFN")).collect();
    println!(
        "{name}: prepare all_ops={} font_ops={} font_requested={} font_returned={}",
        reads.len(),
        fonts.len(),
        fonts.iter().map(|v| v.requested).sum::<usize>(),
        fonts.iter().map(|v| v.returned).sum::<usize>()
    );
    r.storage().reset_reads();
    let full = render_full(&|s| r.draw(s)).frame;
    let full_reads = r.storage().read_count();
    r.storage().reset_reads();
    let stitched = render_stitched(&|s| r.draw(s)).frame;
    let stitched_reads = r.storage().read_count();
    assert_eq!(full.to_pbm(), stitched.to_pbm());
    assert_eq!((full_reads, stitched_reads), (0, 0));
    full.write_pbm(&dir.join(format!("{name}-full.pbm")))
        .unwrap();
    stitched
        .write_pbm(&dir.join(format!("{name}-stitched.pbm")))
        .unwrap();
    println!(
        "{name}: full_draw_reads={full_reads} stitched_draw_reads={stitched_reads} phase={:?}",
        r.phase()
    );
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    let packs = std::path::PathBuf::from(std::env::args().nth(2).expect("pack directory"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("sample.txt"),
        format!("臺灣閱讀測試\n中文章節\n{SAMPLE}"),
    )
    .unwrap();
    let bytes = build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: "臺灣閱讀測試".into(),
        author: "Acceptance".into(),
        identifier: "iansui-observation".into(),
        chapters: vec![Chapter {
            title: "中文章節".into(),
            blocks: vec![
                Block::Heading("中文章節".into()),
                Block::Paragraph(vec![Run::Text(SAMPLE.repeat(7))]),
            ],
        }],
        toc: vec![TocItem {
            title: "中文章節：臺灣山水".into(),
            chapter: 0,
            children: vec![],
        }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap();
    for idx in 0..5 {
        let card = VirtualStorage::memory_with(&[("CJK.EPUB", bytes.as_slice())]);
        card.ensure_pulp_dir().unwrap();
        card.ensure_pulp_subdir("FONTS").unwrap();
        for px in SIZES {
            let name = format!("F{px:05}.PFN");
            card.write_in_pulp_subdir("FONTS", &name, &std::fs::read(packs.join(&name)).unwrap())
                .unwrap();
        }
        let mut r = Rig::new(card);
        r.configure(idx, 0);
        r.storage().reset_reads();
        r.open("CJK.EPUB");
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Ready);
        export(&mut r, &dir, &format!("reader-size{idx}"));
        r.storage().reset_reads();
        r.quick_trigger(QA_TOC);
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Toc);
        export(&mut r, &dir, &format!("toc-size{idx}"));
    }
}

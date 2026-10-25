//! Export pages of the ruby device books, set by the actual ReaderApp, as PBM; observations,
//! not a golden oracle.
//!
//! usage: ruby-preview OUT_DIR PACK_DIR [FONT_IDX [THEME [PAGES [off]]]]
//!
//! PACK_DIR holds the `F000nn.PFN` packs `pulp-fontconv` writes, the ruby sizes among them
//! (`pulp-fontconv bundle`, or `--sizes` with 10,11,12,14,18 added). The books are the ones
//! `device-fixtures` writes: Japanese, in columns (`vertical`) and in lines (`horizontal`).
//! With `off` the Ruby setting is left off, to compare the same pages without readings.
use pulp_host::fixtures::device::{horizontal_book, vertical_book};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("output directory"));
    let packs = std::path::PathBuf::from(args.next().expect("pack directory"));
    let font: u8 = args.next().map_or(2, |v| v.parse().expect("font index"));
    let theme: u8 = args.next().map_or(1, |v| v.parse().expect("theme index"));
    let pages: usize = args.next().map_or(2, |v| v.parse().expect("pages"));
    let ruby = args.next().as_deref() != Some("off");
    std::fs::create_dir_all(&dir).unwrap();
    let books = [
        ("vertical", vertical_book()),
        ("horizontal", horizontal_book()),
    ];
    for (name, bytes) in &books {
        let file = format!("{}.EPUB", name.to_uppercase());
        let card = VirtualStorage::memory_with(&[(file.as_str(), bytes.as_slice())]);
        card.ensure_pulp_dir().unwrap();
        card.ensure_pulp_subdir("FONTS").unwrap();
        for entry in std::fs::read_dir(&packs).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with('F') && name.ends_with(".PFN") {
                card.write_in_pulp_subdir("FONTS", &name, &std::fs::read(&path).unwrap())
                    .unwrap();
            }
        }
        let mut r = Rig::new(card);
        r.configure(font, theme);
        r.set_ruby(ruby);
        r.open(&file);
        r.prepare_render();
        assert_eq!(r.phase(), Phase::Ready, "{name}");
        for page in 0..pages {
            let full = render_full(&|s| r.draw(s)).frame;
            let stitched = render_stitched(&|s| r.draw(s)).frame;
            assert_eq!(full.to_pbm(), stitched.to_pbm(), "{name} page {page}");
            let tag = if ruby { "ruby" } else { "plain" };
            let out = dir.join(format!(
                "{name}-{tag}-font{font}-theme{theme}-page{page}.pbm"
            ));
            full.write_pbm(&out).unwrap();
            println!(
                "{name} page {page}: {} lines, {} black px -> {}",
                r.lines().len(),
                full.black_count(),
                out.display()
            );
            r.press(Action::Next);
            r.prepare_render();
        }
    }
}

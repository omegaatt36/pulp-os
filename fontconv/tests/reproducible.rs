//! Same inputs -> byte-identical outputs; changing any input changes exactly
//! the outputs that depend on it.
#[macro_use]
mod common;

use common::*;
use pulp_fontconv::{Input, Output};

fn run_full(font: &std::path::Path, sizes: Option<&str>, require: Option<&[u8]>) -> Cli {
    let cli = cli_run(font, sizes, require);
    assert!(cli.run.code == 0 || cli.run.code == 3, "{}", cli.run.stderr);
    cli
}

fn lib(font: &[u8], sizes: &[u16], license: &[u8], url: &str, require: Option<&str>) -> Output {
    pulp_fontconv::convert(&Input {
        font,
        sizes,
        license,
        upstream_url: url,
        require_chars: require,
    })
    .unwrap()
}

fn same(a: &Output, b: &Output, name: &str) -> bool {
    a.file(name).expect(name) == b.file(name).expect(name)
}

#[test]
fn two_cli_runs_into_different_directories_are_byte_identical() {
    // identical inputs -> identical file set and bytes, whatever the output path or working directory
    let a = run_full(&bookerly_path(), None, Some("Ab臺".as_bytes()));
    let b = TempDir::new();
    let lic = b.write("license.txt", &license_bytes());
    let req = b.write("require.txt", "Ab臺".as_bytes());
    let out = b.join("elsewhere").join("deeper");
    let r = Args::base(&bookerly_path(), &lic, &out)
        .with("--require-chars", &req)
        .cwd(std::env::temp_dir().as_path())
        .run();
    assert_eq!(r.code, 3, "{}", r.stderr);
    let fa = a.files();
    let fb = read_dir_files(&out);
    assert_eq!(fa.len(), 9 + 3, "default sizes + three text files");
    assert_eq!(fa.keys().collect::<Vec<_>>(), fb.keys().collect::<Vec<_>>());
    for (name, bytes) in &fa {
        assert!(bytes == &fb[name], "{name} differs between runs");
    }
}

#[test]
fn renamed_input_files_give_the_same_output() {
    // font and licence copied to other names/directories: no file name or path leaks into the output
    let a = run_full(&bookerly_path(), Some("16,23"), None);
    let t = TempDir::new();
    std::fs::create_dir(t.join("sub")).unwrap();
    let font = t.join("sub").join("some-other-name.ttf");
    std::fs::copy(bookerly_path(), &font).unwrap();
    let lic = t.write("COPYING", &license_bytes());
    let out = t.join("o");
    let r = Args::base(&font, &lic, &out).with("--sizes", "16,23").run();
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(a.files() == read_dir_files(&out));
}

#[test]
fn size_order_on_the_command_line_does_not_change_the_output() {
    // sizes are a set: 46,16,23 and 16,23,46 produce the same files
    let a = run_full(&bookerly_path(), Some("46,16,23"), None);
    let b = run_full(&bookerly_path(), Some("16,23,46"), None);
    assert!(a.files() == b.files());
}

#[test]
fn library_runs_are_deterministic_and_equal_across_size_permutations() {
    // convert twice -> same files in the same order; permuted sizes -> same
    let font = bookerly_bytes();
    let lic = license_bytes();
    let a = lib(&font, &[16, 23], &lic, UPSTREAM, Some("x臺"));
    let b = lib(&font, &[16, 23], &lic, UPSTREAM, Some("x臺"));
    let c = lib(&font, &[23, 16], &lic, UPSTREAM, Some("x臺"));
    for other in [&b, &c] {
        assert_eq!(a.files.len(), other.files.len());
        for (x, y) in a.files.iter().zip(&other.files) {
            assert_eq!(x.name, y.name);
            assert!(x.bytes == y.bytes, "{} differs", x.name);
        }
        assert_eq!(a.missing, other.missing);
    }
}

#[test]
fn iansui_two_runs_are_byte_identical() {
    // real font, two sizes: both runs produce identical packs and reports
    let path = require_iansui!();
    let a = run_full(&path, Some("16,46"), Some("臺𪚥".as_bytes()));
    let b = run_full(&path, Some("16,46"), Some("臺𪚥".as_bytes()));
    assert!(a.files() == b.files());
}

#[test]
fn different_font_changes_font_id_in_every_pack_and_the_font_hash() {
    // Bookerly Regular vs Italic: different bytes -> different font_id (header bytes 8..16) and font_sha256
    let lic = license_bytes();
    let reg = lib(&bookerly_bytes(), &[16, 23], &lic, UPSTREAM, None);
    let ita = lib(&bookerly_italic_bytes(), &[16, 23], &lic, UPSTREAM, None);
    for px in [16u16, 23] {
        let (a, b) = (pack_bytes(&reg, px), pack_bytes(&ita, px));
        assert_ne!(a[8..16], b[8..16], "font_id must differ at {px}px");
    }
    let (pr, pi) = (
        kv(reg.file("PROV.TXT").unwrap()),
        kv(ita.file("PROV.TXT").unwrap()),
    );
    assert_ne!(get(&pr, "font_sha256"), get(&pi, "font_sha256"));
    assert_ne!(get(&pr, "pack.16.font_id"), get(&pi, "pack.16.font_id"));
}

#[test]
fn one_extra_trailing_font_byte_changes_only_the_font_id_field_of_the_packs() {
    // glyph data is untouched but the font bytes differ, so font_id (header bytes 8..16) must change and nothing else in the pack
    let lic = license_bytes();
    let orig = bookerly_bytes();
    let mut padded = orig.clone();
    padded.push(0);
    let a = lib(&orig, &[23], &lic, UPSTREAM, None);
    let b = lib(&padded, &[23], &lic, UPSTREAM, None);
    let (pa, pb) = (pack_bytes(&a, 23), pack_bytes(&b, 23));
    assert_eq!(pa.len(), pb.len());
    assert_ne!(pa[8..16], pb[8..16], "font_id");
    assert!(
        pa[..8] == pb[..8] && pa[16..] == pb[16..],
        "only font_id may differ"
    );
    assert!(!same(&a, &b, "PROV.TXT"));
}

#[test]
fn each_size_in_one_run_has_its_own_font_id() {
    // font_id depends on pixel size: 9 default sizes give 9 distinct ids (header bytes 8..16)
    let out = convert_with(&bookerly_bytes(), &DEFAULT_SIZES, None);
    let mut ids: Vec<[u8; 8]> = DEFAULT_SIZES
        .iter()
        .map(|&px| pack_bytes(&out, px)[8..16].try_into().unwrap())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), DEFAULT_SIZES.len());
}

#[test]
fn a_pack_is_the_same_whether_built_alone_or_with_other_sizes() {
    // sizes are independent: [16] alone vs [16, 23, 46] -> identical F00016.PFN; the extra sizes only add files
    let font = bookerly_bytes();
    let lic = license_bytes();
    let alone = lib(&font, &[16], &lic, UPSTREAM, None);
    let more = lib(&font, &[16, 23, 46], &lic, UPSTREAM, None);
    assert!(same(&alone, &more, "F00016.PFN"));
    assert!(alone.file("F00023.PFN").is_none());
    assert!(more.file("F00023.PFN").is_some());
    assert!(
        !same(&alone, &more, "PROV.TXT"),
        "provenance lists the packs"
    );
    assert!(
        !same(&alone, &more, "COVERAGE.TXT"),
        "coverage lists the packs"
    );
}

#[test]
fn licence_change_changes_the_copy_and_provenance_but_not_packs_or_coverage() {
    // one different licence byte: OFL.TXT and PROV.TXT change; packs and COVERAGE do not
    let font = bookerly_bytes();
    let l1 = license_bytes();
    let mut l2 = l1.clone();
    l2.push(b'!');
    let a = lib(&font, &[16], &l1, UPSTREAM, None);
    let b = lib(&font, &[16], &l2, UPSTREAM, None);
    assert!(!same(&a, &b, "OFL.TXT"));
    assert!(!same(&a, &b, "PROV.TXT"));
    assert!(same(&a, &b, "F00016.PFN"));
    assert!(same(&a, &b, "COVERAGE.TXT"));
}

#[test]
fn upstream_url_change_changes_only_provenance() {
    // the url is provenance text only
    let font = bookerly_bytes();
    let lic = license_bytes();
    let a = lib(&font, &[16], &lic, "https://example.invalid/a", None);
    let b = lib(&font, &[16], &lic, "https://example.invalid/b", None);
    assert!(!same(&a, &b, "PROV.TXT"));
    for n in ["F00016.PFN", "OFL.TXT", "COVERAGE.TXT"] {
        assert!(same(&a, &b, n), "{n}");
    }
}

#[test]
fn requirement_change_changes_only_the_coverage_report() {
    // different required text: COVERAGE.TXT differs, packs/licence/provenance identical
    let font = bookerly_bytes();
    let lic = license_bytes();
    let a = lib(&font, &[16], &lic, UPSTREAM, Some("A"));
    let b = lib(&font, &[16], &lic, UPSTREAM, Some("A臺"));
    assert!(!same(&a, &b, "COVERAGE.TXT"));
    for n in ["F00016.PFN", "OFL.TXT", "PROV.TXT"] {
        assert!(same(&a, &b, n), "{n}");
    }
    let c = lib(&font, &[16], &lic, UPSTREAM, None);
    assert!(!same(&a, &c, "COVERAGE.TXT"), "with vs without requirement");
}

//! PROV.TXT, the licence copy and the output file set, via the CLI.
#[macro_use]
mod common;

use common::*;
use pulp_fontconv::derive_font_id;
use pulp_fontpack::Pack;

fn bookerly_run(sizes: &str) -> Cli {
    let cli = cli_run(&bookerly_path(), Some(sizes), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    cli
}

#[test]
fn provenance_has_the_exact_key_sequence() {
    // header block, then six lines per pack in ascending size order (given 28,16 on the command line)
    let cli = bookerly_run("28,16");
    let prov = kv(&cli.file("PROV.TXT"));
    let mut want: Vec<String> = [
        "format",
        "format_version",
        "converter",
        "converter_version",
        "convention_version",
        "pack_format_version",
        "font_sha256",
        "font_size",
        "font_upstream_url",
        "license_name",
        "license_file",
        "license_sha256",
        "pack_count",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for px in [16, 28] {
        for f in [
            "file",
            "pixel_size",
            "font_id",
            "glyph_count",
            "size",
            "sha256",
        ] {
            want.push(format!("pack.{px}.{f}"));
        }
    }
    assert_eq!(
        keys(&prov),
        want.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn provenance_header_values_identify_font_licence_converter_and_upstream() {
    // sha-256 and length of Bookerly come from shasum/ls; licence hash from the licence bytes written by the test
    let cli = bookerly_run("16,23");
    let prov = kv(&cli.file("PROV.TXT"));
    assert_eq!(get(&prov, "format"), "pulp-fontconv-provenance");
    assert_eq!(get(&prov, "format_version"), "1");
    assert_eq!(get(&prov, "converter"), "pulp-fontconv");
    assert_eq!(get(&prov, "converter_version"), env!("CARGO_PKG_VERSION"));
    assert_eq!(get(&prov, "convention_version"), "1");
    assert_eq!(get(&prov, "pack_format_version"), "1");
    assert_eq!(get(&prov, "font_sha256"), BOOKERLY_SHA256);
    assert_eq!(get(&prov, "font_size"), BOOKERLY_LEN.to_string());
    assert_eq!(get(&prov, "font_upstream_url"), UPSTREAM);
    assert_eq!(get(&prov, "license_name"), "SIL OFL 1.1");
    assert_eq!(get(&prov, "license_file"), "OFL.TXT");
    assert_eq!(get(&prov, "license_sha256"), sha256_hex(&license_bytes()));
    assert_eq!(get(&prov, "pack_count"), "2");
}

#[test]
fn per_pack_provenance_matches_the_real_pack_files() {
    // file name, size, sha-256 and glyph_count are re-measured from the written pack; font_id from the pack header
    let cli = bookerly_run("16,23,46");
    let prov = kv(&cli.file("PROV.TXT"));
    let font_sha = sha256(&bookerly_bytes());
    for px in [16u16, 23, 46] {
        let name = pack_name(px);
        let bytes = cli.file(&name);
        let pack = Pack::parse(&bytes).unwrap();
        let k = |f: &str| format!("pack.{px}.{f}");
        assert_eq!(get(&prov, &k("file")), name);
        assert_eq!(get(&prov, &k("pixel_size")), px.to_string());
        assert_eq!(get(&prov, &k("size")), bytes.len().to_string());
        assert_eq!(get(&prov, &k("sha256")), sha256_hex(&bytes));
        assert_eq!(
            get(&prov, &k("glyph_count")),
            pack.header().glyph_count.to_string()
        );
        let id = pack.header().info.font_id;
        assert_eq!(get(&prov, &k("font_id")), format!("{id:016x}"));
        assert_eq!(id, derive_font_id(&font_sha, px, 1));
    }
}

#[test]
fn licence_copy_is_byte_identical_to_the_input_including_bom_crlf_and_binary() {
    // OFL.TXT is a verbatim copy: no newline rewriting, BOM stripping or trimming
    let cli = bookerly_run("16");
    assert_eq!(cli.file("OFL.TXT"), license_bytes());
}

#[test]
fn output_directory_holds_exactly_the_packs_and_three_text_files() {
    // nothing else (no temp or backup files), all names 8.3 and equal to pack_file_name for the packs
    let cli = bookerly_run("16,19");
    let names: Vec<String> = cli.files().into_keys().collect();
    assert_eq!(
        names,
        [
            "COVERAGE.TXT",
            "F00016.PFN",
            "F00019.PFN",
            "OFL.TXT",
            "PROV.TXT"
        ]
    );
    for n in &names {
        let (stem, ext) = n.split_once('.').unwrap();
        assert!(stem.len() <= 8 && ext.len() <= 3, "{n}");
        assert!(
            n.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.')
        );
    }
    for px in [16u16, 19] {
        assert_eq!(pack_name(px), pulp_fontpack::pack_file_name(px).as_str());
    }
}

#[test]
fn reports_contain_no_input_or_output_paths_or_host_details() {
    // reproducibility: the temp dir (unique per run), font path and licence path never appear in any output file
    let cli = bookerly_run("16");
    let tmp = cli.tmp.path().to_str().unwrap().to_owned();
    let font = bookerly_path().to_str().unwrap().to_owned();
    let has = |hay: &[u8], needle: &str| hay.windows(needle.len()).any(|w| w == needle.as_bytes());
    for (name, bytes) in cli.files() {
        if name == "OFL.TXT" {
            continue; // verbatim input, not ours to constrain
        }
        for needle in [tmp.as_str(), font.as_str()] {
            assert!(!has(&bytes, needle), "{name} contains {needle}");
        }
        if name.ends_with(".TXT") {
            for needle in ["Bookerly-Regular", "license.txt", "/Users/", "/tmp"] {
                assert!(!has(&bytes, needle), "{name} contains {needle}");
            }
        }
    }
}

#[test]
fn missing_license_option_is_a_usage_error_and_leaves_no_output() {
    // --license absent: exit 2, the out path is never created
    let tmp = TempDir::new();
    let out = tmp.join("out");
    let r = Args::base(&bookerly_path(), &tmp.join("unused"), &out)
        .without("--license")
        .run();
    assert_clean_failure(&r, 2, "no --license");
    assert!(!out.exists(), "no output directory may be created");
}

#[test]
fn nonexistent_license_file_fails_and_leaves_no_pack() {
    // --license points nowhere: runtime failure (1), nothing written
    let tmp = TempDir::new();
    let out = tmp.join("out");
    let r = Args::base(&bookerly_path(), &tmp.join("no-such-licence.txt"), &out).run();
    assert_clean_failure(&r, 1, "missing licence file");
    assert!(!out.exists(), "no output directory may be created");
}

#[test]
fn nonexistent_license_with_a_pre_created_empty_out_dir_leaves_it_empty() {
    // an existing empty out dir stays empty when the licence is unreadable (no partial pack output)
    let tmp = TempDir::new();
    let out = tmp.join("out");
    std::fs::create_dir(&out).unwrap();
    let r = Args::base(&bookerly_path(), &tmp.join("no-such-licence.txt"), &out).run();
    assert_clean_failure(&r, 1, "missing licence file");
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn empty_license_file_is_rejected_rather_than_producing_unlicensed_output() {
    // a zero-byte licence is no licence: exit 1, nothing written
    let tmp = TempDir::new();
    let lic = tmp.write("empty.txt", b"");
    let out = tmp.join("out");
    let r = Args::base(&bookerly_path(), &lic, &out).run();
    assert_clean_failure(&r, 1, "empty licence");
    assert!(!out.exists());
}

#[test]
fn library_rejects_an_empty_licence_and_a_blank_or_multiline_upstream() {
    // the same rules at the library boundary
    let font = bookerly_bytes();
    let base = |license: &'static [u8], url: &'static str| {
        pulp_fontconv::convert(&pulp_fontconv::Input {
            font: &font,
            sizes: &[16],
            license,
            upstream_url: url,
            require_chars: None,
        })
    };
    assert!(base(b"x", "https://example.invalid/u").is_ok());
    assert!(base(b"", "https://example.invalid/u").is_err());
    assert!(base(b"x", "").is_err());
    assert!(base(b"x", "https://a\nsize=1").is_err());
}

#[test]
fn upstream_url_with_a_control_character_or_empty_is_a_usage_error_and_leaves_no_output() {
    // the url is one line of PROV.TXT: newline/empty would corrupt the key=value format
    for bad in ["", "https://a.invalid/\nfont_size=1", "x\ty\u{7}"] {
        let tmp = TempDir::new();
        let lic = tmp.write("license.txt", &license_bytes());
        let out = tmp.join("out");
        let r = Args::base(&bookerly_path(), &lic, &out)
            .replace("--upstream-url", bad)
            .run();
        assert_clean_failure(&r, 2, &format!("url {bad:?}"));
        assert!(!out.exists());
    }
}

// ------------------------------------------------------------- real font

#[test]
fn iansui_provenance_names_the_font_upstream_and_licence_with_the_repo_licence_text() {
    // default sizes through the CLI: 9 packs; font sha/len pinned from shasum; the repo's OFL.txt copied verbatim
    let path = require_iansui!();
    let lic_path = repo_root().join("OFL.txt");
    let tmp = TempDir::new();
    let out = tmp.join("out");
    let r = Args::empty()
        .with("--font", &path)
        .with("--license", &lic_path)
        .with("--upstream-url", IANSUI_UPSTREAM)
        .with("--out", &out)
        .run();
    assert_eq!(r.code, 0, "{}", r.stderr);
    let files = read_dir_files(&out);
    let prov = kv(&files["PROV.TXT"]);
    assert_eq!(get(&prov, "font_sha256"), IANSUI_SHA256);
    assert_eq!(get(&prov, "font_size"), IANSUI_LEN.to_string());
    assert_eq!(
        get(&prov, "font_upstream_url"),
        "https://github.com/ButTaiwan/iansui"
    );
    assert_eq!(get(&prov, "license_name"), "SIL OFL 1.1");
    assert_eq!(get(&prov, "pack_count"), "9");
    assert_eq!(files["OFL.TXT"], std::fs::read(&lic_path).unwrap());
    assert_eq!(get(&prov, "license_sha256"), sha256_hex(&files["OFL.TXT"]));
    for px in DEFAULT_SIZES {
        let bytes = &files[&pack_name(px)];
        assert_eq!(get(&prov, &format!("pack.{px}.sha256")), sha256_hex(bytes));
        assert_eq!(
            get(&prov, &format!("pack.{px}.size")),
            bytes.len().to_string()
        );
        assert_eq!(get(&prov, &format!("pack.{px}.glyph_count")), "12665");
    }
}

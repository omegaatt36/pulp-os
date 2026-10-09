//! COVERAGE.TXT and the missing-character reporting (CLI and library), on the
//! Latin Bookerly font (which lacks all CJK) and, when present, on Iansui.
#[macro_use]
mod common;

use common::*;

const HEADER_KEYS: [&str; 6] = [
    "format",
    "format_version",
    "font_sha256",
    "cmap_total",
    "excluded_total",
    "excluded.control",
];

fn controls_in_cmap(font: &fontdue::Font) -> Vec<char> {
    oracle_cmap(font)
        .into_iter()
        .filter(|c| c.is_control())
        .collect()
}

#[test]
fn coverage_file_has_the_exact_key_sequence_and_font_derived_numbers() {
    // keys in contract order; cmap/excluded/included counts come from fontdue's cmap, 'A' 'b' present, 臺 absent
    let cli = cli_run(&bookerly_path(), Some("23,16"), Some("A臺b".as_bytes()));
    assert_eq!(cli.run.code, 3, "{}", cli.run.stderr);
    let font = load_font(&bookerly_bytes());
    let controls = controls_in_cmap(&font);
    let cov = kv(&cli.file("COVERAGE.TXT"));

    let mut want_keys: Vec<&str> = HEADER_KEYS.to_vec();
    want_keys.push("included_total");
    want_keys.extend(std::iter::repeat_n("excluded", controls.len()));
    want_keys.extend(["pack.16.glyph_count", "pack.23.glyph_count"]);
    want_keys.extend(["require_total", "require_missing", "missing"]);
    assert_eq!(keys(&cov), want_keys);

    assert_eq!(get(&cov, "format"), "pulp-fontconv-coverage");
    assert_eq!(get(&cov, "format_version"), "1");
    assert_eq!(get(&cov, "font_sha256"), BOOKERLY_SHA256);
    let cmap_total = font.chars().len();
    assert_eq!(get(&cov, "cmap_total"), cmap_total.to_string());
    assert_eq!(get(&cov, "excluded_total"), controls.len().to_string());
    assert_eq!(get(&cov, "excluded.control"), controls.len().to_string());
    let included = cmap_total - controls.len();
    assert_eq!(get(&cov, "included_total"), included.to_string());
    assert_eq!(get(&cov, "pack.16.glyph_count"), included.to_string());
    assert_eq!(get(&cov, "pack.23.glyph_count"), included.to_string());
    assert_eq!(get(&cov, "require_total"), "3");
    assert_eq!(get(&cov, "require_missing"), "1");
    assert_eq!(all(&cov, "missing"), ["U+81FA"]);
}

#[test]
fn excluded_lines_list_every_cmap_control_with_its_reason_in_ascending_order() {
    // Bookerly maps U+0000, U+0008..U+000D (some), U+001D, U+0080..U+009F; each appears as U+XXXX:control
    let cli = cli_run(&bookerly_path(), Some("16"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let font = load_font(&bookerly_bytes());
    let want: Vec<String> = controls_in_cmap(&font)
        .into_iter()
        .map(|c| format!("{}:control", u_plus(c)))
        .collect();
    assert!(want.len() > 30, "fixture must have many controls");
    assert!(want.contains(&"U+0000:control".to_owned()));
    assert!(want.contains(&"U+0008:control".to_owned()));
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(all(&cov, "excluded"), want);
}

#[test]
fn without_a_requirement_file_there_are_no_require_or_missing_lines() {
    // no --require-chars: exit 0 and the report stops after the pack lines
    let cli = cli_run(&bookerly_path(), Some("16"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert!(
        keys(&cov)
            .iter()
            .all(|k| !k.starts_with("require") && *k != "missing"),
        "{:?}",
        keys(&cov)
    );
    assert_eq!(*keys(&cov).last().unwrap(), "pack.16.glyph_count");
}

#[test]
fn all_required_chars_present_gives_exit_0_and_an_empty_missing_list() {
    // ASCII letters and digits are all in Bookerly: require_missing=0, no missing= line, exit 0
    let cli = cli_run(
        &bookerly_path(),
        Some("16"),
        Some(b"Hello, World 0123456789"),
    );
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    // distinct non-whitespace, counted by hand: letters H e l o W r d = 7, ',' = 1, digits = 10, total 18
    assert_eq!(get(&cov, "require_total"), "18");
    assert_eq!(get(&cov, "require_missing"), "0");
    assert!(all(&cov, "missing").is_empty());
}

#[test]
fn whitespace_and_repeats_in_the_requirement_file_do_not_count() {
    // required set = distinct non-whitespace scalars: A and B only (space, tab, CRLF, U+3000 skipped)
    let cli = cli_run(
        &bookerly_path(),
        Some("16"),
        Some("A\n \t\u{3000}B\r\nAAB\u{A0}".as_bytes()),
    );
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(get(&cov, "require_total"), "2");
    assert_eq!(get(&cov, "require_missing"), "0");
}

#[test]
fn empty_or_whitespace_only_requirement_file_requires_nothing() {
    // zero required chars: totals 0, exit 0
    for content in [&b""[..], &b"  \n\t\n"[..]] {
        let cli = cli_run(&bookerly_path(), Some("16"), Some(content));
        assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
        let cov = kv(&cli.file("COVERAGE.TXT"));
        assert_eq!(get(&cov, "require_total"), "0");
        assert_eq!(get(&cov, "require_missing"), "0");
    }
}

#[test]
fn missing_chars_are_listed_ascending_once_each_in_u_plus_notation() {
    // 灣 U+7063, 臺 U+81FA, 𪚥 U+2A6A5 are not in Bookerly; given out of order and repeated they come out sorted, once
    let cli = cli_run(
        &bookerly_path(),
        Some("16"),
        Some("\u{2A6A5}臺灣x臺\u{2A6A5}".as_bytes()),
    );
    assert_eq!(cli.run.code, 3, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(all(&cov, "missing"), ["U+7063", "U+81FA", "U+2A6A5"]);
    assert_eq!(get(&cov, "require_total"), "4");
    assert_eq!(get(&cov, "require_missing"), "3");
}

#[test]
fn present_chars_are_never_listed_as_missing() {
    // x and A are in the font: the only missing entry is the CJK one
    let cli = cli_run(&bookerly_path(), Some("16"), Some("xA臺".as_bytes()));
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(all(&cov, "missing"), ["U+81FA"]);
}

#[test]
fn a_control_char_in_the_cmap_is_missing_because_it_is_not_in_the_packs() {
    // U+0008 is mapped by the font but excluded as a control; U+0001 is unmapped; tab/newline are whitespace so not required
    let cli = cli_run(
        &bookerly_path(),
        Some("16"),
        Some("\u{8}\u{1}A\t\n".as_bytes()),
    );
    assert_eq!(cli.run.code, 3, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(all(&cov, "missing"), ["U+0001", "U+0008"]);
    assert_eq!(get(&cov, "require_total"), "3");
}

#[test]
fn missing_requirement_still_writes_every_file_and_says_so_on_stderr() {
    // exit 3 means "converted, but incomplete": all packs + reports exist and parse; stderr mentions "missing"
    let cli = cli_run(&bookerly_path(), Some("16,23"), Some("臺".as_bytes()));
    assert_eq!(cli.run.code, 3);
    assert!(cli.run.stderr.contains("missing"), "{}", cli.run.stderr);
    assert!(!cli.run.stderr.contains("panicked"));
    let names: Vec<String> = cli.files().into_keys().collect();
    assert_eq!(
        names,
        [
            "COVERAGE.TXT",
            "F00016.PFN",
            "F00023.PFN",
            "OFL.TXT",
            "PROV.TXT"
        ]
    );
    for n in ["F00016.PFN", "F00023.PFN"] {
        assert!(pulp_fontpack::Pack::parse(&cli.file(n)).is_ok(), "{n}");
    }
}

#[test]
fn library_missing_list_matches_the_oracle_cmap_and_cli_files_are_identical() {
    // missing = required minus included, computed here from fontdue's cmap; lib and CLI produce the same coverage and packs
    let req = "Ab臺灣\u{8}\u{20BB7}z";
    let font = load_font(&bookerly_bytes());
    let included = oracle_included(&font);
    let mut want: Vec<char> = req
        .chars()
        .filter(|c| !c.is_whitespace() && !included.contains(c))
        .collect();
    want.sort();
    want.dedup();
    assert!(want.len() == 4, "fixture sanity: {want:?}");

    let out = convert_with(&bookerly_bytes(), &[16, 23], Some(req));
    assert_eq!(out.missing, want);

    let cli = cli_run(&bookerly_path(), Some("16,23"), Some(req.as_bytes()));
    assert_eq!(cli.run.code, 3);
    assert_eq!(
        out.file("COVERAGE.TXT").unwrap(),
        &cli.file("COVERAGE.TXT")[..]
    );
    assert_eq!(pack_bytes(&out, 16), &cli.file("F00016.PFN")[..]);
    assert_eq!(pack_bytes(&out, 23), &cli.file("F00023.PFN")[..]);
}

#[test]
fn library_without_requirement_reports_nothing_missing() {
    // require_chars None -> missing stays empty
    let out = convert_with(&bookerly_bytes(), &[16], None);
    assert!(out.missing.is_empty());
}

#[test]
fn library_output_files_are_exactly_the_contract_set_in_name_order() {
    // packs + three text files, ascending by name
    let out = convert_with(&bookerly_bytes(), &[28, 16], None);
    let names: Vec<&str> = out.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "COVERAGE.TXT",
            "F00016.PFN",
            "F00028.PFN",
            "OFL.TXT",
            "PROV.TXT"
        ]
    );
}

// ------------------------------------------------------------- real font

#[test]
fn iansui_requirement_with_the_unmapped_char_reports_exactly_that_char() {
    // 臺 灣 𠮷 are in Iansui, 𪚥 is not: exit 3, missing = U+2A6A5, packs still written
    let path = require_iansui!();
    let cli = cli_run(&path, Some("16"), Some("臺灣𠮷𪚥".as_bytes()));
    assert_eq!(cli.run.code, 3, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(all(&cov, "missing"), ["U+2A6A5"]);
    assert_eq!(get(&cov, "require_total"), "4");
    assert_eq!(get(&cov, "require_missing"), "1");
    assert!(pulp_fontpack::Pack::parse(&cli.file("F00016.PFN")).is_ok());
}

#[test]
fn iansui_requirement_fully_covered_exits_0_with_nothing_missing() {
    // all chars exist in the font
    let path = require_iansui!();
    let cli = cli_run(&path, Some("16"), Some("臺灣「繁體中文」，。𠮷".as_bytes()));
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(get(&cov, "require_missing"), "0");
    assert!(all(&cov, "missing").is_empty());
}

#[test]
fn iansui_coverage_numbers_match_the_font_cmap_and_exclude_only_the_cr_control() {
    // cmap 12,666; the only control in it is U+000D -> excluded_total 1, included 12,665
    let path = require_iansui!();
    let cli = cli_run(&path, Some("16,46"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let font = load_font(&std::fs::read(&path).unwrap());
    let cov = kv(&cli.file("COVERAGE.TXT"));
    assert_eq!(get(&cov, "cmap_total"), font.chars().len().to_string());
    assert_eq!(get(&cov, "excluded_total"), "1");
    assert_eq!(all(&cov, "excluded"), ["U+000D:control"]);
    assert_eq!(
        get(&cov, "included_total"),
        (font.chars().len() - 1).to_string()
    );
}

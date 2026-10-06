//! Size selection: defaults, custom lists, and per-size pack naming.
mod common;

use common::*;

#[test]
fn default_size_constant_is_the_nine_expected_values() {
    // union of body 16/19/23/28/35 and heading 23/27/32/38/46, 23 once -> 9 ascending values
    assert_eq!(
        pulp_fontconv::DEFAULT_SIZES,
        [16, 19, 23, 27, 28, 32, 35, 38, 46]
    );
    assert_eq!(pulp_fontconv::DEFAULT_SIZES, DEFAULT_SIZES);
}

#[test]
fn cli_without_sizes_writes_one_pack_per_default_size() {
    // no --sizes: nine packs F000xx.PFN for the nine defaults, plus the three text files
    let cli = cli_run(&bookerly_path(), None, None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let mut want: Vec<String> = DEFAULT_SIZES.iter().map(|&px| pack_name(px)).collect();
    want.extend(["COVERAGE.TXT", "OFL.TXT", "PROV.TXT"].map(String::from));
    want.sort();
    assert_eq!(cli.files().into_keys().collect::<Vec<_>>(), want);
    for px in DEFAULT_SIZES {
        let bytes = cli.file(&pack_name(px));
        let pack = pulp_fontpack::Pack::parse(&bytes).unwrap();
        assert_eq!(pack.header().info.pixel_size, px);
    }
}

#[test]
fn custom_sizes_produce_only_the_requested_packs() {
    // --sizes 20,31 -> exactly F00020.PFN and F00031.PFN, with those pixel sizes
    let cli = cli_run(&bookerly_path(), Some("31,20"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let names: Vec<String> = cli.files().into_keys().collect();
    assert_eq!(
        names,
        [
            "COVERAGE.TXT",
            "F00020.PFN",
            "F00031.PFN",
            "OFL.TXT",
            "PROV.TXT"
        ]
    );
    for px in [20u16, 31] {
        let bytes = cli.file(&pack_name(px));
        let pack = pulp_fontpack::Pack::parse(&bytes).unwrap();
        assert_eq!(pack.header().info.pixel_size, px);
    }
}

#[test]
fn single_size_run_writes_a_single_pack() {
    // one size -> one pack, pack_count=1
    let cli = cli_run(&bookerly_path(), Some("28"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let prov = kv(&cli.file("PROV.TXT"));
    assert_eq!(get(&prov, "pack_count"), "1");
    assert_eq!(get(&prov, "pack.28.file"), "F00028.PFN");
}

#[test]
fn library_pack_names_are_8_3_and_equal_pack_file_name() {
    // every file name the library emits is upper case 8.3; pack names equal the shared firmware function
    let out = convert_with(&bookerly_bytes(), &DEFAULT_SIZES, None);
    for f in &out.files {
        let (stem, ext) = f.name.split_once('.').expect("extension");
        assert!(stem.len() <= 8 && ext.len() <= 3, "{}", f.name);
        assert!(
            f.name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.'),
            "{}",
            f.name
        );
    }
    for px in DEFAULT_SIZES {
        let name = pulp_fontpack::pack_file_name(px);
        assert!(out.file(name.as_str()).is_some(), "{name}");
        assert_eq!(name.as_str(), pack_name(px));
    }
}

#[test]
fn pack_file_name_of_the_firmware_crate_is_the_literal_rule() {
    // F + 5 digits + .PFN, restated here so a rename in either crate is caught
    assert_eq!(pulp_fontpack::pack_file_name(46).as_str(), "F00046.PFN");
    assert_eq!(pulp_fontpack::PACK_DIR, "FONTS");
}

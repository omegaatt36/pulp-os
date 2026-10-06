//! SD file naming: pack files must be valid FAT 8.3 short names (the firmware's
//! SD driver has no long file name support), unique per pixel size, and
//! computable without alloc. Literals here are the naming rule, not imports.
use pulp_fontpack::{PACK_DIR, PackFileName, pack_file_name};
use std::collections::HashSet;

// the default font sizes of the converter: firmware body 16/19/23/28/35 plus
// heading 23/27/32/38/46, deduplicated
const DEFAULT_SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];

fn is_short_name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

// one FAT 8.3 component: 1..=8 name chars, optional '.' + 1..=3 extension chars
fn assert_8_3(name: &str) {
    let (stem, ext) = match name.split_once('.') {
        Some((s, e)) => (s, Some(e)),
        None => (name, None),
    };
    assert!(
        (1..=8).contains(&stem.len()),
        "{name}: stem must be 1..=8 chars"
    );
    assert!(
        stem.chars().all(is_short_name_char),
        "{name}: bad stem char"
    );
    if let Some(e) = ext {
        assert!(
            (1..=3).contains(&e.len()),
            "{name}: extension must be 1..=3 chars"
        );
        assert!(e.chars().all(is_short_name_char), "{name}: bad ext char");
    }
}

#[test]
fn every_default_size_maps_to_its_fixed_literal_name() {
    // rule: "F" + 5 zero-padded decimal digits + ".PFN"
    let expected = [
        (16, "F00016.PFN"),
        (19, "F00019.PFN"),
        (23, "F00023.PFN"),
        (27, "F00027.PFN"),
        (28, "F00028.PFN"),
        (32, "F00032.PFN"),
        (35, "F00035.PFN"),
        (38, "F00038.PFN"),
        (46, "F00046.PFN"),
    ];
    for (size, name) in expected {
        assert_eq!(pack_file_name(size).as_str(), name, "size {size}");
    }
}

#[test]
fn boundary_sizes_zero_and_u16_max_keep_the_fixed_width_rule() {
    // 5 digits hold every u16, so 0 and 65535 follow the same rule
    assert_eq!(pack_file_name(0).as_str(), "F00000.PFN");
    assert_eq!(pack_file_name(255).as_str(), "F00255.PFN");
    assert_eq!(pack_file_name(65535).as_str(), "F65535.PFN");
}

#[test]
fn default_size_names_are_pairwise_distinct_valid_8_3() {
    // 9 distinct sizes must give 9 distinct names, each a legal short name
    let mut seen = HashSet::new();
    for size in DEFAULT_SIZES {
        let n = pack_file_name(size);
        assert_8_3(n.as_str());
        assert!(seen.insert(n.as_str().to_owned()), "duplicate {n}");
    }
    assert_eq!(seen.len(), DEFAULT_SIZES.len());
}

#[test]
fn every_u16_size_gets_a_valid_8_3_name_and_no_two_sizes_collide() {
    // injective over the whole input domain, so the name determines the size
    let mut seen = HashSet::with_capacity(65536);
    for size in 0..=u16::MAX {
        let n = pack_file_name(size);
        assert_8_3(n.as_str());
        assert!(
            n.as_str().ends_with(".PFN"),
            "size {size}: extension must be PFN"
        );
        assert!(
            seen.insert(n.as_str().to_owned()),
            "size {size} collides with another size"
        );
    }
    assert_eq!(seen.len(), 65536);
}

#[test]
fn name_is_a_pure_function_of_the_size() {
    // same input twice -> equal values (PackFileName is Copy + Eq)
    let a: PackFileName = pack_file_name(23);
    let b: PackFileName = pack_file_name(23);
    assert_eq!(a, b);
    assert_ne!(pack_file_name(23), pack_file_name(24));
}

#[test]
fn display_writes_exactly_the_name_text() {
    // Display and as_str are the same text, no padding or NUL bytes
    for size in [0u16, 7, 16, 46, 255, 65535] {
        let n = pack_file_name(size);
        assert_eq!(n.to_string(), n.as_str());
        assert!(!n.as_str().contains('\0'));
    }
}

#[test]
fn install_directory_names_are_8_3_and_the_pack_dir_is_fonts() {
    // path on the card is _PULP/FONTS/<pack>; each component must be 8.3
    assert_eq!(PACK_DIR, "FONTS");
    assert_8_3(PACK_DIR);
    assert_8_3("_PULP");
    for size in DEFAULT_SIZES {
        assert_8_3(pack_file_name(size).as_str());
    }
}

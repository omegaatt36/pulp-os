// pack file names (§7) and the discovery scan
//
// the converter emits `<FAMILY><SS>.PFP`; the device has to recognise exactly
// that spelling in a scanned SD root without opening the file. The oracle here
// is tools/fontpack/src/names.rs (pack_file_name), not the parser

mod common;

use pulp_render::pack_file::{PackName, find, parse_pack_name};

fn parsed(name: &str) -> Option<PackName> {
    parse_pack_name(name.as_bytes())
}

#[test]
fn parses_the_spelling_the_converter_writes() {
    let p = parsed("IANSUI24.PFP").expect("IANSUI24.PFP is a pack");
    assert_eq!(p.family(), b"IANSUI");
    assert_eq!(p.pixel_size, 24);
}

#[test]
fn parses_the_shortest_and_longest_legal_names() {
    let p = parsed("A08.PFP").expect("1-char family is legal");
    assert_eq!(p.family(), b"A");
    assert_eq!(p.pixel_size, 8);

    let p = parsed("ABCDEF96.PFP").expect("6-char family is legal");
    assert_eq!(p.family(), b"ABCDEF");
    assert_eq!(p.pixel_size, 96);
}

#[test]
fn round_trips_through_the_canonical_spelling() {
    // the device compares a scanned name against a wanted one, so
    // parse -> render -> parse must be the identity
    for name in [
        "A08.PFP",
        "IANSUI24.PFP",
        "ABCDEF96.PFP",
        "F18.PFP",
        "Z960.PFP",
    ] {
        let Some(p) = parsed(name) else {
            continue;
        };
        let (buf, len) = p.file_name();
        assert_eq!(&buf[..len], name.as_bytes(), "round trip changed {name}");
    }
}

#[test]
fn digits_are_legal_family_bytes() {
    // the size is always the last two stem bytes, so the family is whatever
    // is left: here "TT2" and 24 px, not "TT" and 2024
    let p = parsed("TT224.PFP").expect("digits are legal in a family");
    assert_eq!(p.family(), b"TT2");
    assert_eq!(p.pixel_size, 24);
}
#[test]
fn rejects_everything_outside_the_converter_spelling() {
    // lower case: FAT long-name entries can put either on the card, and
    // normalising would make the scan depend on a guess
    assert!(parsed("iansui24.PFP").is_none());
    assert!(parsed("Iansui24.PFP").is_none());
    // wrong extension, including the manifest and licence sidecars
    assert!(parsed("IANSUI24.TXT").is_none());
    assert!(parsed("IANSUOFL.TXT").is_none());
    assert!(parsed("IANSUI24").is_none());
    // family too long to be a short name the converter would ever write
    assert!(parsed("ABCDEFG24.PFP").is_none());
    // family too short to carry a size
    assert!(parsed(".PFP").is_none());
    // unpadded size, and sizes outside 8..=96
    assert!(parsed("IANSUI8.PFP").is_none());
    assert!(parsed("IANSUI07.PFP").is_none());
    assert!(parsed("IANSUI00.PFP").is_none());
    assert!(parsed("IANSUI97.PFP").is_none());
    assert!(parsed("IANSUI99.PFP").is_none());
    // non-alphanumeric family
    assert!(parsed("IANSUI_.PFP").is_none());
}

#[test]
fn find_locates_the_requested_family_and_size() {
    let names: [&[u8]; 4] = [b"BOOK.TXT", b"IANSUI24.PFP", b"IANSUI32.PFP", b"NESPS.PFP"];
    let (buf, len) = find(&names, b"IANSUI", 32).expect("IANSUI at 32 px is on the card");
    assert_eq!(&buf[..len], b"IANSUI32.PFP");
}

#[test]
fn find_ignores_other_families_sizes_and_sidecars() {
    let names: [&[u8]; 4] = [
        b"IANSUI24.PFP",
        b"IANSUI24.TXT",
        b"IANSUOFL.TXT",
        b"OTHER24.PFP",
    ];
    // the size and family must both match; a same-size pack of another
    // family is not a substitute
    assert!(find(&names, b"IANSUI", 24).is_some());
    assert!(
        find(&names, b"IANSUI", 40).is_none(),
        "absent size must not match"
    );
    assert!(find(&names, b"OTHER", 24).is_some());
    assert!(find(&names, b"NOPE", 24).is_none());
}

#[test]
fn find_on_an_empty_listing_is_not_an_error() {
    let names: [&[u8]; 0] = [];
    assert!(find(&names, b"IANSUI", 24).is_none());
}

// silence the unused-import warning the shared module would otherwise raise
#[test]
fn common_module_is_linked() {
    assert_eq!(common::crc32(b"123456789"), 0xCBF4_3926);
}

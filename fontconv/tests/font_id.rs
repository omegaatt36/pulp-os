//! font_id derivation: SHA-256(font_sha256 || pixel_size u16 LE || convention u32 LE),
//! first 8 bytes read as a little-endian u64.
mod common;

use common::*;
use pulp_fontconv::{CONVENTION_VERSION, derive_font_id};
use pulp_fontpack::Pack;
use sha2::{Digest, Sha256};

// recompute from the written formula with sha2, independent of the converter
fn by_formula(font_sha: &[u8; 32], px: u16, conv: u32) -> u64 {
    let mut h = Sha256::new();
    h.update(font_sha);
    h.update(px.to_le_bytes());
    h.update(conv.to_le_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

#[test]
fn font_id_matches_the_formula_for_arbitrary_inputs() {
    // derive_font_id == first 8 LE bytes of SHA-256 over the 38-byte preimage
    let sha = sha256(b"some font");
    for (px, conv) in [(0u16, 0u32), (16, 1), (255, 1), (65535, 7), (23, u32::MAX)] {
        assert_eq!(
            derive_font_id(&sha, px, conv),
            by_formula(&sha, px, conv),
            "{px} {conv}"
        );
    }
}

#[test]
fn font_id_known_answers_for_the_font_abc() {
    // font bytes "abc": sha256 = ba7816bf...15ad (the standard test vector). The three ids below were computed
    // outside Rust with python hashlib and cross-checked with `shasum -a 256` on the raw 38-byte preimage
    let sha = sha256(b"abc");
    assert_eq!(
        hex(&sha),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(derive_font_id(&sha, 16, 1), 0x45f1_f9b2_a119_7eef);
    assert_eq!(derive_font_id(&sha, 23, 1), 0x9958_91a0_73f4_548f);
    assert_eq!(derive_font_id(&sha, 16, 2), 0xff2e_79c1_6689_9647);
}

#[test]
fn font_id_changes_with_each_ingredient() {
    // font hash, pixel size and convention version each change the id
    let a = sha256(b"abc");
    let b = sha256(b"abd");
    let base = derive_font_id(&a, 16, 1);
    assert_ne!(base, derive_font_id(&b, 16, 1));
    assert_ne!(base, derive_font_id(&a, 17, 1));
    assert_ne!(base, derive_font_id(&a, 16, 2));
}

#[test]
fn convention_version_constant_is_1() {
    // the rasterisation convention shipped by this contract; bump it (and the known answers above stay valid)
    assert_eq!(CONVENTION_VERSION, 1);
}

#[test]
fn pack_headers_carry_the_derived_id_of_their_own_font_and_size() {
    // header font_id == formula(sha256(font bytes), pixel_size, convention 1) for every default size
    let bytes = bookerly_bytes();
    let sha = sha256(&bytes);
    let out = convert_with(&bytes, &DEFAULT_SIZES, None);
    for px in DEFAULT_SIZES {
        let pack = Pack::parse(pack_bytes(&out, px)).unwrap();
        assert_eq!(
            pack.header().info.font_id,
            by_formula(&sha, px, 1),
            "{px}px"
        );
    }
}

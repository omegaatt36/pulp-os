//! Test-only helpers that are deliberately independent of the crate under test.
#![allow(dead_code)]

use std::path::PathBuf;

/// Bitwise CRC-32/ISO-HDLC written from the parameters in docs/font-pack.txt §1
/// (reflected poly 0xEDB88320, init 0xFFFFFFFF, xorout 0xFFFFFFFF).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

pub fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

pub fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

pub fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// Recompute and store both CRCs so a test can isolate a structural error (§6).
pub fn refresh_crcs(b: &mut [u8]) {
    let index_off = u32_at(b, 28) as usize;
    let index_len = u32_at(b, 32) as usize;
    let icrc = crc32(&b[index_off..index_off + index_len]);
    put_u32(b, 56, icrc);
    let hcrc = crc32(&b[..60]);
    put_u32(b, 60, hcrc);
}

/// Fresh empty directory under the system temp dir, unique per test name.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pulp-fontpack-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Tracked OFL-licensed Iansui subset (tests/fixtures/README.txt), so the
/// suite never needs the full Iansui TTF.
pub fn subset_ttf_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/IansuiSubset.ttf")
}

pub fn subset_ttf() -> Vec<u8> {
    std::fs::read(subset_ttf_path()).unwrap()
}

/// The subset's license, byte-identical to Iansui's OFL.txt.
pub fn subset_license_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/OFL.txt")
}

pub fn subset_license() -> Vec<u8> {
    std::fs::read(subset_license_path()).unwrap()
}

/// The pack the render acceptance tests load: the subset at 24 px.
pub fn tracked_pack_path() -> PathBuf {
    repo_root().join("render/tests/fixtures/IANSUI24.PFP")
}

/// §3 license section of a pack, located by its header fields.
pub fn license_section(pack: &[u8]) -> &[u8] {
    let off = u32_at(pack, 44) as usize;
    let len = u32_at(pack, 48) as usize;
    &pack[off..off + len]
}

#[test]
fn crc32_matches_spec_check_value() {
    // docs/font-pack.txt §1: crc32("123456789") = 0xCBF43926
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

//! Deterministic fuzzing (xorshift64, fixed seeds): no input may panic or
//! yield a slice outside the input. Whatever `parse` accepts must satisfy the
//! soundness invariants in `common::assert_accepted_pack_is_sound`.
mod common;

use common::*;
use pulp_fontpack::{Pack, build_pack};

fn check(bytes: &[u8], rng: &mut XorShift64) -> bool {
    match Pack::parse(bytes) {
        Ok(pack) => {
            assert_accepted_pack_is_sound(bytes, &pack, rng);
            true
        }
        Err(_) => false,
    }
}

const INTERESTING_U32: [u32; 12] = [
    0,
    1,
    2,
    0xFF,
    0x100,
    0xFFFF,
    0x1_0000,
    0x10_FFFF,
    0x11_0000,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
];

#[test]
fn every_single_byte_xor_of_the_golden_pack_is_handled() {
    // 139 positions x 255 xor values, exhaustive.
    let mut rng = XorShift64::new(0x1234_5678_9ABC_DEF1);
    let mut accepted = 0u32;
    for pos in 0..GOLDEN.len() {
        for x in 1..=255u8 {
            let mut bytes = GOLDEN.to_vec();
            bytes[pos] ^= x;
            if check(&bytes, &mut rng) {
                accepted += 1;
            }
        }
    }
    // bitmap payload bytes and metrics are free-form, so some flips must be accepted
    assert!(accepted > 0);
}

#[test]
fn every_single_byte_xor_of_the_golden_empty_pack_is_handled() {
    let mut rng = XorShift64::new(0xDEAD_BEEF_0000_0001);
    for pos in 0..GOLDEN_EMPTY.len() {
        for x in 1..=255u8 {
            let mut bytes = GOLDEN_EMPTY.to_vec();
            bytes[pos] ^= x;
            check(&bytes, &mut rng);
        }
    }
}

#[test]
fn random_header_and_index_field_overwrites_are_handled() {
    let mut rng = XorShift64::new(0x0F0F_1234_5555_AAAA);
    let big = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    let packs: [Vec<u8>; 3] = [GOLDEN.to_vec(), GOLDEN_EMPTY.to_vec(), big];
    for iter in 0..20_000u32 {
        let mut bytes = packs[(iter % 3) as usize].clone();
        let structural = (44 + 22 * 8).min(bytes.len());
        let edits = 1 + rng.below(3);
        for _ in 0..edits {
            let width = [1usize, 2, 4][rng.below(3) as usize];
            if bytes.len() < width {
                continue;
            }
            let at = if rng.below(10) < 8 {
                rng.below((structural - width + 1) as u64) as usize
            } else {
                rng.below((bytes.len() - width + 1) as u64) as usize
            };
            let v = if rng.below(2) == 0 {
                INTERESTING_U32[rng.below(INTERESTING_U32.len() as u64) as usize]
            } else {
                rng.next_u64() as u32
            };
            bytes[at..at + width].copy_from_slice(&v.to_le_bytes()[..width]);
        }
        check(&bytes, &mut rng);
    }
}

#[test]
fn random_bytes_behind_a_valid_magic_and_version_are_handled() {
    // Forcing "PFNT" + version 1 pushes the input past the cheap checks.
    let mut rng = XorShift64::new(0xABCD_EF01_2345_6789);
    for _ in 0..20_000u32 {
        let len = rng.below(400) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        if bytes.len() >= 6 {
            bytes[0..4].copy_from_slice(b"PFNT");
            bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        }
        if bytes.len() >= HEADER_LEN && rng.below(2) == 0 {
            // make total_len honest so the layout and record checks are reached
            let n = bytes.len() as u32;
            bytes[H_TOTAL_LEN..H_TOTAL_LEN + 4].copy_from_slice(&n.to_le_bytes());
        }
        check(&bytes, &mut rng);
    }
}

#[test]
fn fully_random_inputs_are_handled() {
    let mut rng = XorShift64::new(0x9E37_79B9_7F4A_7C15);
    for _ in 0..5_000u32 {
        let len = rng.below(300) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        check(&bytes, &mut rng);
    }
}

#[test]
fn random_prefixes_and_suffix_extensions_of_valid_packs_are_handled() {
    let mut rng = XorShift64::new(0x5151_5151_A0A0_A0A0);
    let big = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    for iter in 0..3_000u32 {
        let src: &[u8] = if iter % 2 == 0 { &GOLDEN } else { &big };
        let cut = rng.below(src.len() as u64 + 1) as usize;
        let mut bytes = src[..cut].to_vec();
        if iter % 3 == 0 {
            let extra = rng.below(64) as usize;
            bytes.extend((0..extra).map(|_| rng.next_u64() as u8));
        }
        let accepted = check(&bytes, &mut rng);
        // only the untouched pack may be accepted
        assert_eq!(accepted, bytes == src, "iteration {iter}");
    }
}

#[test]
fn lookups_on_every_golden_codepoint_stay_inside_the_input_after_random_flips() {
    let mut rng = XorShift64::new(0x2468_ACE0_1357_9BDF);
    let big = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    for _ in 0..2_000u32 {
        let mut bytes = big.clone();
        for _ in 0..(1 + rng.below(4)) {
            let at = rng.below(bytes.len() as u64) as usize;
            bytes[at] ^= 1 << rng.below(8);
        }
        // Flips inside bitmap payload stay valid; the rest are structural.
        check(&bytes, &mut rng);
    }
}

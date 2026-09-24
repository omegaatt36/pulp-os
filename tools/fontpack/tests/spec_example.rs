//! docs/font-pack.txt §10: the writer must reproduce the worked example byte for byte.
mod common;

use pulp_fontpack::format::{Glyph, PackInput, write_pack};

/// The hex dump from §10, transcribed; rows 0x0040..0x01FF are 448 zero bytes.
fn doc_bytes() -> Vec<u8> {
    let rows: [&str; 4] = [
        "50 55 4C 50 46 4F 4E 54 01 00 40 00 08 00 0A 00",
        "08 00 00 00 01 00 00 00 A1 25 00 00 00 02 00 00",
        "10 00 00 00 10 02 00 00 03 00 00 00 13 02 00 00",
        "04 00 00 00 17 02 00 00 4A 5D A9 E2 90 F6 31 B6",
    ];
    let hex = |s: &str| -> Vec<u8> {
        s.split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect()
    };
    let mut out: Vec<u8> = rows.iter().flat_map(|r| hex(r)).collect();
    out.extend(std::iter::repeat_n(0u8, 448));
    out.extend(hex("A1 25 00 00 00 00 00 00 03 03 04 00 FD 00 00 00"));
    out.extend(hex("E0 A0 E0 4F 46 4C 0A"));
    out
}

#[test]
fn worked_example_is_reproduced_byte_for_byte() {
    let expected = doc_bytes();
    assert_eq!(expected.len(), 535, "§10 says 535 bytes");

    let glyph = Glyph {
        code_point: 0x25A1,
        width: 3,
        height: 3,
        advance: 4,
        offset_x: 0,
        offset_y: -3,
        bitmap: vec![0xE0, 0xA0, 0xE0],
    };
    let input = PackInput {
        pixel_size: 8,
        line_height: 10,
        ascent: 8,
        fallback_cp: 0x25A1,
        glyphs: &[glyph],
        license: b"OFL\n",
    };
    let got = write_pack(&input).expect("worked example is valid");
    assert_eq!(got, expected);
}

#[test]
fn worked_example_crcs_agree_with_independent_crc() {
    // guards the transcription: §10's stated CRCs must match the bytes they cover
    let b = doc_bytes();
    assert_eq!(common::crc32(&b[0x200..0x210]), 0xE2A9_5D4A);
    assert_eq!(common::crc32(&b[..60]), 0xB631_F690);
}

//! §9 self-verify: every written pack is loaded with the device loader
//! (pulp_render::font_pack) and every packed glyph must resolve to itself
//! with the metrics and bitmap bytes it was written with. Each damage case
//! below is one the §6 checks alone let through (or one they catch), so a
//! converter bug fails generation on the host instead of on the device.
mod common;

use common::{put_u32, refresh_crcs};
use pulp_fontpack::convert::{VerifyError, verify};
use pulp_fontpack::format::{Glyph, PackInput, write_pack};
use pulp_render::font_pack::LoadError;

fn glyphs() -> Vec<Glyph> {
    vec![
        Glyph {
            code_point: 0x41,
            width: 9,
            height: 2,
            advance: 10,
            offset_x: 1,
            offset_y: -2,
            bitmap: vec![0xFF, 0x80, 0x80, 0x80],
        },
        Glyph {
            code_point: 0x25A1,
            width: 3,
            height: 3,
            advance: 4,
            offset_x: 0,
            offset_y: -3,
            bitmap: vec![0xE0, 0xA0, 0xE0],
        },
    ]
}

fn input(gs: &[Glyph]) -> PackInput<'_> {
    PackInput {
        pixel_size: 16,
        line_height: 18,
        ascent: 15,
        fallback_cp: 0x25A1,
        glyphs: gs,
        license: b"OFL\n",
    }
}

// layout (§2, hand-derived): index 512..544 (2 records), bitmaps 544..551
// (A: 2 row bytes x 2 rows at 544..548, U+25A1: 3 bytes at 548..551),
// license 551..555
const REC0: usize = 512;
const BOX_ROW1: usize = 548 + 1;
const LICENSE_LAST: usize = 554;

fn verify_err(bytes: &[u8], gs: &[Glyph]) -> VerifyError {
    match verify(bytes, &input(gs)) {
        Ok(_) => panic!("damaged pack passed self-verification"),
        Err(e) => e,
    }
}

#[test]
fn writer_output_loads_and_every_glyph_round_trips() {
    let gs = glyphs();
    let bytes = write_pack(&input(&gs)).unwrap();
    let pack = verify(&bytes, &input(&gs)).expect("valid pack verifies");
    assert_eq!(pack.glyph_count(), 2);
    assert_eq!(pack.pixel_size(), 16);
    assert_eq!(pack.line_height(), 18);
    assert_eq!(pack.ascent(), 15);
    assert_eq!(pack.fallback_cp(), 0x25A1);
    assert_eq!(pack.max_glyph_len(), 4, "A is 2 row bytes x 2 rows");
}

#[test]
fn a_pack_the_loader_rejects_fails_verification() {
    let gs = glyphs();
    let v = write_pack(&input(&gs)).unwrap();

    let mut b = v.clone();
    b[0] = b'X';
    assert_eq!(verify_err(&b, &gs), VerifyError::Load(LoadError::BadMagic));

    assert_eq!(
        verify_err(&v[..v.len() - 1], &gs),
        VerifyError::Load(LoadError::Truncated)
    );

    let mut b = v.clone();
    put_u32(&mut b, REC0, 0xD800);
    refresh_crcs(&mut b);
    assert_eq!(
        verify_err(&b, &gs),
        VerifyError::Load(LoadError::InvalidCodePoint)
    );

    // the size the converter was asked for is the size the loader checks (#8)
    assert_eq!(
        verify(
            &v,
            &PackInput {
                pixel_size: 24,
                ..input(&gs)
            }
        )
        .err(),
        Some(VerifyError::Load(LoadError::WrongPixelSize))
    );
}

// bitmaps are covered by no CRC and no §6 check: only the round trip sees
// a flipped pixel. U+25A1 row 1 is 101 (0xA0); 0xE0 would fill it
#[test]
fn bitmap_damage_fails_the_round_trip() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    assert_eq!(b[BOX_ROW1], 0xA0, "fixture precondition");
    b[BOX_ROW1] = 0xE0;
    assert_eq!(verify_err(&b, &gs), VerifyError::Bitmap(0x25A1));
}

// a writer bug with consistent CRCs is structurally valid (§6 passes) but
// the record no longer matches the rasterised glyph
#[test]
fn metric_damage_with_valid_crcs_fails_the_round_trip() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    assert_eq!(b[REC0 + 10], 10, "fixture precondition: A advance");
    b[REC0 + 10] = 11;
    refresh_crcs(&mut b);
    assert_eq!(verify_err(&b, &gs), VerifyError::Metrics(0x41));
}

// record 0 renamed U+0041 -> U+0042: still sorted and in bounds, so it
// loads, but 'A' now resolves to the fallback
#[test]
fn a_glyph_missing_from_the_index_fails_the_round_trip() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    put_u32(&mut b, REC0, 0x42);
    refresh_crcs(&mut b);
    assert_eq!(verify_err(&b, &gs), VerifyError::Absent(0x41));
}

#[test]
fn header_values_must_match_the_conversion() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    b[14] = 19; // line_height 18 -> 19, still >= ascent
    refresh_crcs(&mut b);
    assert_eq!(
        verify_err(&b, &gs),
        VerifyError::Header {
            field: "line_height",
            expected: 18,
            actual: 19
        }
    );

    // one glyph more than was written
    let mut more = gs.clone();
    more.push(Glyph {
        code_point: 0x4E2D,
        width: 1,
        height: 1,
        advance: 1,
        offset_x: 0,
        offset_y: -1,
        bitmap: vec![0x80],
    });
    let b = write_pack(&input(&gs)).unwrap();
    assert_eq!(
        verify_err(&b, &more),
        VerifyError::Header {
            field: "glyph_count",
            expected: 3,
            actual: 2
        }
    );
}

#[test]
fn license_bytes_must_match() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    assert_eq!(b[LICENSE_LAST], b'\n', "fixture precondition");
    b[LICENSE_LAST] = b'!';
    assert_eq!(verify_err(&b, &gs), VerifyError::License);
}

#[test]
fn license_section_must_have_exact_length() {
    let gs = glyphs();
    let mut b = write_pack(&input(&gs)).unwrap();
    let license_off = u32::from_le_bytes(b[44..48].try_into().unwrap()) as usize;
    b.splice(license_off..license_off, b"EXTRA".iter().copied());
    put_u32(&mut b, 48, (input(&gs).license.len() + 5) as u32);
    let file_len = b.len() as u32;
    put_u32(&mut b, 52, file_len);
    refresh_crcs(&mut b);
    assert_eq!(verify_err(&b, &gs), VerifyError::License);
}

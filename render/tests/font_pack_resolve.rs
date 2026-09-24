// R4: one resolver for measurement and drawing. a code point present in
// the pack resolves to its own record (ASCII included); only an absent
// one resolves to the fallback record, whose metrics and bitmap are then
// used for both layout and drawing (docs/font-pack.txt §3 fallback_cp,
// §6 "Runtime lookup")
//
// every expected metric, offset and bitmap byte below is a value the
// fixture planted or hand arithmetic on the §2/§5 layout; nothing is
// taken from running the resolver

mod common;

use common::*;
use pulp_render::font_pack::{FontPack, PackGlyph, Resolved};

// §10's worked-example glyph, planted as the fallback: 3x3 box, rows
// 111 / 101 / 111 = E0 A0 E0, advance 4, offset_x 0, offset_y -3
fn box_glyph() -> Glyph {
    Glyph {
        cp: 0x25A1,
        width: 3,
        height: 3,
        advance: 4,
        offset_x: 0,
        offset_y: -3,
        bitmap: vec![0xE0, 0xA0, 0xE0],
    }
}

// space: present with zero size (§5 len 0) but a real advance of 5
fn space() -> Glyph {
    Glyph {
        advance: 5,
        ..sized(0x20, 0, 0, 0)
    }
}

// 'A' 7x9 -> advance 8 (common::sized: width + 1)
fn a() -> Glyph {
    sized(0x41, 1, 7, 9)
}

// '中' 15x16 -> advance 16
fn zhong() -> Glyph {
    sized(0x4E2D, 2, 15, 16)
}

// §2 bitmaps back to back in index order: ' ' 0 B, 'A' ceil(7/8)*9 = 9 B,
// U+25A1 ceil(3/8)*3 = 3 B, '中' ceil(15/8)*16 = 32 B
// -> section offsets 0, 0, 9, 12; bitmap_off = 512 + 4 * 16 = 576
const OFFSETS: [u32; 4] = [0, 0, 9, 12];

fn fixture() -> (TestReader, FontPack, Built) {
    let built = Pack::new(16, 0x25A1, vec![space(), a(), box_glyph(), zhong()]).build();
    assert_eq!(built.offsets, OFFSETS, "fixture precondition");
    assert_eq!(built.bitmap_off, 576, "fixture precondition");
    let mut r = TestReader::new(built.bytes.clone());
    let pack = FontPack::load(&mut r, 16).unwrap();
    (r, pack, built)
}

fn record(g: &Glyph, offset: u32) -> PackGlyph {
    PackGlyph {
        code_point: g.cp,
        bitmap_offset: offset,
        width: g.width,
        height: g.height,
        advance: g.advance,
        offset_x: g.offset_x,
        offset_y: g.offset_y,
    }
}

fn fallback_record() -> PackGlyph {
    record(&box_glyph(), OFFSETS[2])
}

// none of these is planted: Hangul, a supplementary-plane ideograph,
// U+FFFD (Iansui lacks it, hence the U+25A1 default), a line feed below
// the first record, and a code point above the last record
const ABSENT: [char; 5] = ['\u{D55C}', '\u{20000}', '\u{FFFD}', '\n', '\u{10FFFF}'];

#[test]
fn absent_code_points_resolve_to_the_planted_fallback() {
    let (mut r, pack, _) = fixture();
    let mut buf = [0u8; 64];
    for ch in ABSENT {
        let want = Resolved {
            glyph: fallback_record(),
            fallback: true,
        };
        assert_eq!(pack.resolve(&mut r, ch).unwrap(), want, "{ch:?}");
        assert_eq!(pack.advance(&mut r, ch).unwrap(), 4, "{ch:?}");

        let (got, bits) = pack.lookup(&mut r, ch, &mut buf).unwrap();
        assert_eq!(got, want, "{ch:?}");
        assert_eq!(bits, &[0xE0, 0xA0, 0xE0], "{ch:?} draws the box");
    }
}

#[test]
fn present_code_points_resolve_from_the_pack() {
    let (mut r, pack, _) = fixture();
    let mut buf = [0u8; 64];
    let present = [
        (space(), OFFSETS[0]),
        (a(), OFFSETS[1]),
        (box_glyph(), OFFSETS[2]),
        (zhong(), OFFSETS[3]),
    ];
    for (g, offset) in present {
        let ch = char::from_u32(g.cp).unwrap();
        let want = Resolved {
            glyph: record(&g, offset),
            fallback: false,
        };
        assert_eq!(pack.resolve(&mut r, ch).unwrap(), want, "{ch:?}");
        assert_eq!(pack.advance(&mut r, ch).unwrap(), g.advance, "{ch:?}");

        let (got, bits) = pack.lookup(&mut r, ch, &mut buf).unwrap();
        assert_eq!(got, want, "{ch:?}");
        assert_eq!(bits, &g.bitmap[..], "{ch:?}");
    }
}

#[test]
fn zero_size_space_is_present_not_fallback() {
    let (mut r, pack, _) = fixture();
    let res = pack.resolve(&mut r, ' ').unwrap();
    assert!(!res.fallback);
    assert_eq!(res.glyph.code_point, 0x20);
    assert_eq!((res.glyph.width, res.glyph.height), (0, 0));
    assert_eq!(pack.advance(&mut r, ' ').unwrap(), 5);
    let mut buf = [0u8; 0];
    let (_, bits) = pack.lookup(&mut r, ' ', &mut buf).unwrap();
    assert!(bits.is_empty());
}

// hand sum of planted advances: A 8 + ' ' 5 + 中 16 + 한 (fallback) 4
// + A 8 + U+FFFD (fallback) 4 + U+20000 (fallback) 4 = 49
#[test]
fn measuring_mixed_text_sums_planted_advances() {
    let (mut r, pack, _) = fixture();
    let text = "A 中한A\u{FFFD}\u{20000}";
    let mut width = 0u32;
    for ch in text.chars() {
        width += pack.advance(&mut r, ch).unwrap() as u32;
    }
    assert_eq!(width, 49);
}

// layout and drawing cannot disagree: the glyph the draw path returns is
// the one measurement used, for every present and absent code point
#[test]
fn measurement_and_drawing_agree() {
    let (mut r, pack, _) = fixture();
    let mut buf = [0u8; 64];
    for ch in "A 中□한\u{FFFD}\u{20000}\n".chars() {
        let res = pack.resolve(&mut r, ch).unwrap();
        let (drawn, _) = pack.lookup(&mut r, ch, &mut buf).unwrap();
        assert_eq!(drawn, res, "{ch:?}");
        assert_eq!(pack.advance(&mut r, ch).unwrap(), drawn.glyph.advance);
    }
}

// the fallback's metrics are cached at load (§6), so an absent code point
// costs at most the one index-sector read that proves it absent, and no
// bitmap read. index = [512, 576), bitmaps start at 576
#[test]
fn resolving_an_absent_code_point_reads_only_the_index() {
    let (mut r, pack, built) = fixture();
    for ch in ABSENT {
        r.reset_counts();
        let res = pack.resolve(&mut r, ch).unwrap();
        assert!(res.fallback);
        assert!(r.reads <= 1, "{ch:?}: {} reads", r.reads);
        for &(off, n) in &r.log {
            assert!(
                off >= 512 && off as usize + n <= built.bitmap_off as usize,
                "{ch:?}: read {n} B at {off} is outside the index"
            );
        }
    }
    // '\n' sorts below the first record, so it needs no read at all
    r.reset_counts();
    pack.resolve(&mut r, '\n').unwrap();
    assert_eq!(r.reads, 0);
}

// same bound in a multi-sector index: 100 records two code points apart
// fill sectors 0..=31, 32..=63, 64..=95, 96..=99 (§2); the probes fall
// between records, at sector edges and past the end
#[test]
fn absent_lookup_reads_one_sector_in_a_multi_sector_index() {
    let glyphs: Vec<Glyph> = (0..100).map(|i| planted(0x100 + 2 * i as u32, i)).collect();
    let fallback = 0x100 + 2 * 50;
    let built = Pack::new(12, fallback, glyphs.clone()).build();
    let index_end = 512 + 100 * 16;
    let mut r = TestReader::new(built.bytes);
    let pack = FontPack::load(&mut r, 12).unwrap();
    for cp in [
        0x101,
        0x100 + 2 * 31 + 1,
        0x100 + 2 * 63 + 1,
        0x100 + 2 * 99 + 1,
    ] {
        let ch = char::from_u32(cp).unwrap();
        r.reset_counts();
        let res = pack.resolve(&mut r, ch).unwrap();
        assert_eq!(
            res,
            Resolved {
                glyph: record(&glyphs[50], built.offsets[50]),
                fallback: true,
            },
            "{cp:#X}"
        );
        assert!(r.reads <= 1, "{cp:#X}: {} reads", r.reads);
        assert!(r.bytes_read <= 512, "{cp:#X}: {} bytes", r.bytes_read);
        for &(off, n) in &r.log {
            assert!(off >= 512 && off as usize + n <= index_end, "{cp:#X}");
        }
    }
}

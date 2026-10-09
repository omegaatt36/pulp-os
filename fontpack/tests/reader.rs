//! `PackReader` behaviour on valid packs: lookup, bitmap reads, indices
//! above the 16-bit range, absent glyph -> `None`, one pack, one size and the
//! `&[u8]` source.
//!
//! Expectations come from the hand-annotated golden bytes, from the builder
//! input (never from reader output), from hand-derived offsets, and from the
//! `Pack::parse(..).glyph_at` oracle.
mod common;
mod reader_support;

use common::*;
use pulp_fontpack::{
    FontError, FontInfo, GlyphEntry, Header, Metrics, OutOfRange, Pack, PackReader, ReadAt,
    build_pack, missing_glyph_metrics, render_missing_glyph,
};
use reader_support::*;

type SliceReader<'a> = PackReader<&'a [u8]>;

fn open_slice(bytes: &[u8]) -> SliceReader<'_> {
    PackReader::open(bytes, bytes.len() as u64).expect("pack must open")
}

/// metrics + bitmap bytes of `c`, or None when absent. Reads through an exact-size buffer.
fn fetch<R: ReadAt>(r: &mut PackReader<R>, c: char) -> Option<(Metrics, Vec<u8>)>
where
    R::Error: std::fmt::Debug,
{
    let g = r.find(c).expect("find must not fail")?;
    let mut buf = vec![0u8; g.bitmap_len as usize];
    let got = r
        .read_bitmap(&g, &mut buf)
        .expect("read_bitmap must not fail");
    assert_eq!(got.len(), g.bitmap_len as usize);
    let v = got.to_vec();
    Some((g.metrics, v))
}

// ------------------------------------------------------------- &[u8] source

#[test]
fn slice_source_reads_exactly_the_requested_range() {
    let data: Vec<u8> = (0..=255u8).collect();
    let mut s: &[u8] = &data;
    let mut b = [0u8; 4];
    s.read_at(0, &mut b).unwrap();
    assert_eq!(b, [0, 1, 2, 3]);
    s.read_at(10, &mut b).unwrap();
    assert_eq!(b, [10, 11, 12, 13]);
    s.read_at(252, &mut b).unwrap(); // ends exactly at the end
    assert_eq!(b, [252, 253, 254, 255]);
    let mut one = [0u8; 1];
    s.read_at(255, &mut one).unwrap();
    assert_eq!(one, [255]);
    // the source slice itself is not consumed by reading
    s.read_at(0, &mut b).unwrap();
    assert_eq!(b, [0, 1, 2, 3]);
}

#[test]
fn slice_source_rejects_anything_past_the_end_without_panicking() {
    let data = [7u8; 10];
    let mut s: &[u8] = &data;
    let mut b = [0u8; 4];
    assert_eq!(s.read_at(7, &mut b), Err(OutOfRange)); // 7+4 = 11 > 10
    assert_eq!(s.read_at(10, &mut b), Err(OutOfRange));
    assert_eq!(s.read_at(11, &mut b), Err(OutOfRange));
    assert_eq!(s.read_at(u64::MAX, &mut b), Err(OutOfRange));
    assert_eq!(s.read_at(u64::MAX - 1, &mut b), Err(OutOfRange)); // offset+len overflows u64
    assert_eq!(s.read_at(u64::MAX - 3, &mut b), Err(OutOfRange)); // exactly 2^64 - 3 + 4 wraps
    assert_eq!(s.read_at(1 << 32, &mut b), Err(OutOfRange));
    assert_eq!(s.read_at(1 << 63, &mut b), Err(OutOfRange));
    let mut one = [0u8; 1];
    assert_eq!(s.read_at(10, &mut one), Err(OutOfRange)); // one past the end
    assert_eq!(s.read_at(u64::MAX, &mut one), Err(OutOfRange));
    // the last in-range ones
    assert_eq!(s.read_at(6, &mut b), Ok(()));
    assert_eq!(s.read_at(9, &mut one), Ok(()));
}

#[test]
fn slice_source_zero_length_reads_follow_the_offset_plus_len_rule() {
    // "Err iff offset+len exceeds the slice": offset == len is in range, offset > len is not
    let data = [1u8; 5];
    let mut s: &[u8] = &data;
    let mut none: [u8; 0] = [];
    assert_eq!(s.read_at(0, &mut none), Ok(()));
    assert_eq!(s.read_at(5, &mut none), Ok(()));
    assert_eq!(s.read_at(6, &mut none), Err(OutOfRange));
    assert_eq!(s.read_at(u64::MAX, &mut none), Err(OutOfRange));
    let mut e: &[u8] = &[];
    assert_eq!(e.read_at(0, &mut none), Ok(()));
    assert_eq!(e.read_at(1, &mut none), Err(OutOfRange));
    let mut b = [0u8; 1];
    assert_eq!(e.read_at(0, &mut b), Err(OutOfRange));
}

// -------------------------------------------------------------- golden pack

#[test]
fn golden_header_and_info_are_reported() {
    let r = open_slice(&GOLDEN);
    assert_eq!(
        r.header(),
        Header {
            info: GOLDEN_INFO,
            glyph_count: 4,
            bitmap_len: 7
        }
    );
    assert_eq!(r.info(), GOLDEN_INFO);
}

#[test]
fn golden_every_glyph_has_the_annotated_metrics_and_bitmap_bytes() {
    let mut r = open_slice(&GOLDEN);
    for (c, m, bitmap) in golden_glyphs() {
        let g = r
            .find(c)
            .unwrap()
            .unwrap_or_else(|| panic!("U+{:04X}", c as u32));
        assert_eq!(g.metrics, m, "U+{:04X} metrics", c as u32);
        assert_eq!(
            g.bitmap_len as usize,
            bitmap.len(),
            "U+{:04X} len",
            c as u32
        );
        let mut buf = vec![0xEEu8; bitmap.len()];
        assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), &bitmap[..]);
        assert_eq!(buf, bitmap, "U+{:04X} bitmap bytes", c as u32);
    }
}

#[test]
fn golden_hand_written_bytes_for_the_two_astral_glyphs() {
    // U+20BB7 (> 0xFFFF): bitmap FF 80 00 80, metrics adv 16 ox 0 oy -2 9x2 (annotated golden bytes)
    let mut r = open_slice(&GOLDEN);
    let (met, bytes) = fetch(&mut r, '\u{20BB7}').unwrap();
    assert_eq!(
        met,
        Metrics {
            advance: 16,
            offset_x: 0,
            offset_y: -2,
            width: 9,
            height: 2
        }
    );
    assert_eq!(bytes, [0xFF, 0x80, 0x00, 0x80]);
    // U+10FFFF, the largest scalar: blank, advance 5
    let (met, bytes) = fetch(&mut r, '\u{10FFFF}').unwrap();
    assert_eq!(met, m(5, 0, 0, 0, 0));
    assert!(bytes.is_empty());
    // U+FFFF (the top of the 16-bit range) and 'A'
    assert_eq!(fetch(&mut r, '\u{FFFF}').unwrap().1, [0x80]);
    assert_eq!(fetch(&mut r, 'A').unwrap().1, [0x3C, 0x42]);
}

#[test]
fn golden_absent_chars_are_ok_none_including_neighbours_of_present_ones() {
    let mut r = open_slice(&GOLDEN);
    for c in [
        '\0',
        '@',
        'B',
        '\u{7F}',
        '\u{FFFE}',
        '\u{10000}',
        '\u{20BB6}',
        '\u{20BB8}',
        '\u{10FFFE}',
        '\u{D7FF}',
        '\u{E000}',
        '\u{1F600}',
        '\u{2A6A5}',
    ] {
        assert!(
            r.find(c).unwrap().is_none(),
            "U+{:04X} must be absent",
            c as u32
        );
    }
}

#[test]
fn golden_lookup_works_in_every_order_and_repeatedly() {
    // no hidden "last found" state: forwards, backwards, repeated
    let mut r = open_slice(&GOLDEN);
    let all = golden_glyphs();
    for _ in 0..3 {
        for (c, m, b) in all.iter().chain(all.iter().rev()) {
            assert_eq!(fetch(&mut r, *c), Some((*m, b.clone())));
        }
    }
}

#[test]
fn glyph_refs_stay_valid_after_later_finds() {
    // the ref carries its own location: find A, find B, then read both
    let mut r = open_slice(&GOLDEN);
    let ga = r.find('A').unwrap().unwrap();
    let gb = r.find('\u{20BB7}').unwrap().unwrap();
    assert!(r.find('Z').unwrap().is_none());
    let mut bb = [0u8; 4];
    assert_eq!(
        r.read_bitmap(&gb, &mut bb).unwrap(),
        [0xFF, 0x80, 0x00, 0x80]
    );
    let mut ba = [0u8; 2];
    assert_eq!(r.read_bitmap(&ga, &mut ba).unwrap(), [0x3C, 0x42]);
    // reading twice gives the same bytes; GlyphRef is Copy + Eq
    let ga2 = ga;
    assert_eq!(ga, ga2);
    assert_eq!(r.read_bitmap(&ga2, &mut ba).unwrap(), [0x3C, 0x42]);
    assert_eq!(r.find('A').unwrap().unwrap(), ga);
    assert_ne!(ga, gb);
}

#[test]
fn bytes_of_buf_beyond_the_bitmap_are_untouched() {
    let mut r = open_slice(&GOLDEN);
    let g = r.find('A').unwrap().unwrap();
    let mut buf = [0xA5u8; 9];
    let out = r.read_bitmap(&g, &mut buf).unwrap();
    assert_eq!(out, [0x3C, 0x42]);
    assert_eq!(out.len(), 2);
    assert_eq!(buf, [0x3C, 0x42, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5]);
    // a glyph in the middle of the region, buffer 3 too large
    let g = r.find('\u{FFFF}').unwrap().unwrap();
    let mut buf = [0x11u8; 4];
    assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), [0x80]);
    assert_eq!(buf, [0x80, 0x11, 0x11, 0x11]);
}

#[test]
fn blank_glyph_reads_back_as_empty_whatever_the_buffer() {
    let mut r = open_slice(&GOLDEN);
    let g = r.find('\u{10FFFF}').unwrap().unwrap();
    assert_eq!(g.bitmap_len, 0);
    let mut none: [u8; 0] = [];
    assert_eq!(r.read_bitmap(&g, &mut none).unwrap(), &[] as &[u8]);
    let mut buf = [0x77u8; 5];
    assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), &[] as &[u8]);
    assert_eq!(buf, [0x77; 5], "a blank glyph writes nothing");
}

#[test]
fn too_small_buffer_is_reported_with_the_needed_size() {
    let mut r = open_slice(&GOLDEN);
    for (c, _, bitmap) in golden_glyphs() {
        let g = r.find(c).unwrap().unwrap();
        for have in 0..bitmap.len() {
            let mut buf = vec![0x33u8; have];
            assert_eq!(
                r.read_bitmap(&g, &mut buf),
                Err(FontError::BufferTooSmall {
                    needed: bitmap.len()
                }),
                "U+{:04X} with {have} bytes",
                c as u32
            );
            assert!(buf.iter().all(|&b| b == 0x33), "nothing written on error");
        }
    }
}

// ----------------------------------------------------------- 0 / 1 glyph packs

#[test]
fn zero_glyph_pack_opens_and_finds_nothing() {
    let mut r = open_slice(&GOLDEN_EMPTY);
    assert_eq!(
        r.header(),
        Header {
            info: GOLDEN_INFO,
            glyph_count: 0,
            bitmap_len: 0
        }
    );
    assert_eq!(r.info(), GOLDEN_INFO);
    for c in ['\0', 'A', '\u{FFFF}', '\u{20BB7}', char::MAX] {
        assert!(r.find(c).unwrap().is_none());
    }
}

#[test]
fn zero_glyph_pack_with_an_unreferenced_bitmap_region_is_fine() {
    // A zero glyph count permits a non-empty bitmap region.
    let info = GOLDEN_INFO;
    let bytes = raw_pack(&info, &[], &[1, 2, 3]);
    let mut r = open_slice(&bytes);
    assert_eq!(r.header().bitmap_len, 3);
    assert!(r.find('A').unwrap().is_none());
}

#[test]
fn single_glyph_packs_find_exactly_their_one_char_across_the_scalar_range() {
    for c in [
        '\0',
        'A',
        '\u{D7FF}',
        '\u{E000}',
        '\u{FFFF}',
        '\u{10000}',
        '\u{20BB7}',
        '\u{10FFFF}',
    ] {
        let entry = synth_glyph(9, c, 12, 5); // 2 * 5 = 10 bytes
        let want = (entry.metrics, entry.bitmap.clone());
        let bytes = build_pack(&SYNTH_INFO, &[entry]).unwrap();
        let mut r = open_slice(&bytes);
        assert_eq!(r.header().glyph_count, 1);
        assert_eq!(fetch(&mut r, c), Some(want), "U+{:04X}", c as u32);
        for other in [
            prev_char(c),
            next_char(c),
            Some('\0'),
            Some(char::MAX),
            Some('M'),
        ]
        .into_iter()
        .flatten()
        {
            if other != c {
                assert!(
                    r.find(other).unwrap().is_none(),
                    "{:04X} found in the single-glyph pack of {:04X}",
                    other as u32,
                    c as u32
                );
            }
        }
    }
}

// ------------------------------------------------- builder packs vs the oracle

/// Every entry found with its builder metrics and bitmap, and the reader agrees
/// with `Pack::parse(..).glyph_at` on every index.
fn assert_reader_matches(bytes: &[u8], entries: &[GlyphEntry]) {
    let oracle = Pack::parse(bytes).expect("builder output parses");
    assert_eq!(oracle.header().glyph_count as usize, entries.len());
    let mut r = open_slice(bytes);
    assert_eq!(r.header(), oracle.header());
    for (i, e) in entries.iter().enumerate() {
        let (oc, og) = oracle.glyph_at(i as u32).unwrap();
        assert_eq!(oc, e.codepoint);
        let got = fetch(&mut r, e.codepoint)
            .unwrap_or_else(|| panic!("U+{:04X} (index {i}) not found", e.codepoint as u32));
        assert_eq!(got.0, e.metrics, "U+{:04X} metrics", e.codepoint as u32);
        assert_eq!(
            got.1, e.bitmap,
            "U+{:04X} bitmap vs builder",
            e.codepoint as u32
        );
        assert_eq!(got.0, og.metrics);
        assert_eq!(
            got.1, og.bitmap,
            "U+{:04X} bitmap vs Pack::glyph_at",
            e.codepoint as u32
        );
    }
}

#[test]
fn builder_golden_entries_match() {
    let bytes = build_pack(&GOLDEN_INFO, &golden_entries()).unwrap();
    assert_eq!(bytes, GOLDEN, "builder reproduces the pinned golden bytes");
    assert_reader_matches(&bytes, &golden_entries());
}

#[test]
fn r2_bitmap_offsets_above_65535_resolve_to_the_right_bytes() {
    // large_offset_entries: glyphs start at 0, 65,535, 65,536 (u16 truncation would give 0), ...
    let entries = large_offset_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let h = Pack::parse(&bytes).unwrap().header();
    assert_eq!(h.bitmap_len, LARGE_OFFSET_BITMAP_LEN);
    assert!(h.bitmap_len > 65_535);
    // the hand-derived offsets are what the builder wrote (from the doc table in common)
    let mut at = 0u32;
    for (i, e) in entries.iter().enumerate() {
        assert_eq!(at, LARGE_OFFSET_EXPECTED[i], "table row {i}");
        at += e.bitmap.len() as u32;
    }
    assert!(
        LARGE_OFFSET_EXPECTED
            .iter()
            .filter(|&&o| o > 65_535)
            .count()
            >= 5
    );
    assert_reader_matches(&bytes, &entries);
}

#[test]
fn r2_codepoints_above_ffff_are_indexed_correctly() {
    // 7-glyph pack: U+FFFF < U+10000 < U+20BB7 < U+10FFFF all distinct and ordered
    let entries = large_offset_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let mut r = open_slice(&bytes);
    for c in ['\u{FFFF}', '\u{10000}', '\u{20BB7}', '\u{10FFFF}'] {
        assert!(r.find(c).unwrap().is_some(), "U+{:04X}", c as u32);
    }
    // a 16-bit truncation would alias these onto present or other chars
    for c in [
        '\u{1}',
        '\u{10001}',
        '\u{20000}',
        '\u{20BB6}',
        '\u{20BB8}',
        '\u{10FFFE}',
    ] {
        assert!(
            r.find(c).unwrap().is_none(),
            "U+{:04X} must be absent",
            c as u32
        );
    }
    // U+0BB7 (low 16 bits of U+20BB7) and U+0000 (low 16 bits of U+10000) are not in the pack
    assert!(r.find('\u{0BB7}').unwrap().is_none());
    assert!(r.find('\0').unwrap().is_none());
}

#[test]
fn many_glyph_pack_with_an_index_over_64_kib_matches_everything() {
    // 2,999 glyphs (3000 minus one surrogate hit), index 65,978 bytes > 65,535
    let entries = many_glyph_entries();
    assert!(entries.len() * 22 > 65_535);
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    assert_reader_matches(&bytes, &entries);
}

#[test]
fn many_glyph_pack_presence_is_exact_around_every_present_codepoint() {
    // present codepoints are the multiples of 351 below 3000*351 minus surrogates:
    // c-1, c, c+1 of every one of them, plus a dense sweep of the low range
    let entries = many_glyph_entries();
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
    let present: std::collections::BTreeSet<u32> =
        entries.iter().map(|e| e.codepoint as u32).collect();
    let mut r = open_slice(&bytes);
    let mut probes: Vec<u32> = (0..0x4000).collect();
    for &p in &present {
        probes.extend([p.wrapping_sub(1), p, p + 1]);
    }
    probes.extend([0x10_FFFF, 0x10_FFFE, 0x20_0000 - 1]);
    for v in probes {
        let Some(c) = char::from_u32(v) else { continue };
        let found = r.find(c).unwrap().is_some();
        assert_eq!(found, present.contains(&v), "U+{v:04X}");
    }
}

#[test]
fn r2_bitmap_region_over_16_mib_with_offsets_above_2_pow_24() {
    // 33 filler glyphs of 64 x 65535 (524,280 bytes each) = 17,301,240 bytes, then five
    // small tail glyphs whose bitmap offsets are all above 16,777,216.
    let filler_len = 8 * 65535usize;
    let mut entries: Vec<GlyphEntry> = (0..33u32)
        .map(|i| GlyphEntry {
            codepoint: char::from_u32(0x1000 + i).unwrap(),
            metrics: m(10 + i as u16, 0, -3, 64, 65535),
            bitmap: vec![0u8; filler_len],
        })
        .collect();
    let tail_specs: [(char, u16, u16); 5] = [
        ('\u{2000}', 8, 3),
        ('\u{FFFF}', 16, 7),
        ('\u{10000}', 24, 5),
        ('\u{20BB7}', 9, 9),
        ('\u{10FFFF}', 33, 2),
    ];
    for (k, &(c, w, h)) in tail_specs.iter().enumerate() {
        entries.push(synth_glyph(500 + k as u32, c, w, h));
    }
    let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();

    // facts the test relies on, derived from the input sizes
    let first_tail_offset = 33 * filler_len;
    assert_eq!(first_tail_offset, 17_301_240);
    assert!(first_tail_offset > 1 << 24);
    let h = Pack::parse(&bytes).unwrap().header();
    assert!(h.bitmap_len as usize > 1 << 24);

    let mut r = open_slice(&bytes);
    assert_eq!(r.header(), h);
    for e in &entries[33..] {
        let got = fetch(&mut r, e.codepoint).unwrap();
        assert_eq!(got.0, e.metrics, "U+{:04X}", e.codepoint as u32);
        assert_eq!(got.1, e.bitmap, "U+{:04X}", e.codepoint as u32);
    }
    // a first, a middle and the last filler too
    for i in [0usize, 16, 32] {
        let e = &entries[i];
        let got = fetch(&mut r, e.codepoint).unwrap();
        assert_eq!(got.0, e.metrics);
        assert_eq!(got.1.len(), filler_len);
        assert!(got.1.iter().all(|&b| b == 0));
    }
    // neighbours of the tail are absent
    for c in [
        '\u{2001}',
        '\u{FFFE}',
        '\u{10001}',
        '\u{20BB6}',
        '\u{10FFFE}',
    ] {
        assert!(r.find(c).unwrap().is_none());
    }
}

// ------------------------------------------------ sparse pack near the u32 top

/// Deterministic pseudo-content of absolute file byte `p`: a function of the
/// position only, so an off-by-2^16 or 2^24 read returns recognisably wrong bytes.
fn sparse_byte(p: u64) -> u8 {
    let x = (p as u32 ^ (p >> 32) as u32).wrapping_mul(0x9E37_79B1);
    (x >> 13) as u8 ^ 0x5A
}

#[test]
fn r2_sparse_pack_with_offsets_up_to_the_u32_limit_reads_at_the_right_absolute_address() {
    // 7 records, header+index = 44 + 154 = 198 bytes; bitmap region fills the rest of a
    // u32::MAX-byte file: bitmap_len = u32::MAX - 198 = 4,294,967,097. Each glyph is
    // 16x3 = 6 bytes at a chosen relative offset; absolute address = 198 + offset.
    let region_len: u32 = u32::MAX - 198;
    assert_eq!(region_len, 4_294_967_097);
    let offsets: [u32; 7] = [
        0,
        65_535,
        65_536,
        16_777_215,
        16_777_216,
        0x8000_0000,
        region_len - 6, // ends exactly at the end of the region (and of the file)
    ];
    let cps: [u32; 7] = [0x61, 0x100, 0xFFFF, 0x1_0000, 0x1F600, 0x20BB7, 0x10_FFFF];
    let recs: Vec<RawRec> = (0..7).map(|i| rawrec(cps[i], offsets[i], 16, 3)).collect();
    let head = raw_pack_with_region(&GOLDEN_INFO, &recs, &[], region_len);
    assert_eq!(head.len(), 198);
    let file_len = u32::MAX as u64;

    let head2 = head.clone();
    let spy = Spy::sparse(file_len, move |off, buf| {
        for (j, b) in buf.iter_mut().enumerate() {
            let p = off + j as u64;
            *b = if p < 198 {
                head2[p as usize]
            } else {
                sparse_byte(p)
            };
        }
    });
    let log = spy.log();
    let mut r = PackReader::open(spy, file_len).expect("sparse pack opens");
    assert_eq!(r.header().glyph_count, 7);
    assert_eq!(r.header().bitmap_len, region_len);

    for i in 0..7 {
        let c = char::from_u32(cps[i]).unwrap();
        let g = r
            .find(c)
            .unwrap()
            .unwrap_or_else(|| panic!("U+{:04X}", cps[i]));
        assert_eq!(g.bitmap_len, 6);
        assert_eq!(g.metrics.width, 16);
        let before = read_count(&log);
        let mut buf = [0u8; 6];
        let got = r.read_bitmap(&g, &mut buf).unwrap().to_vec();
        let abs = 198 + offsets[i] as u64;
        let want: Vec<u8> = (0..6).map(|j| sparse_byte(abs + j)).collect();
        assert_eq!(got, want, "glyph {i} at relative offset {}", offsets[i]);
        // exactly one read, at the hand-derived absolute address
        assert_eq!(reads(&log)[before..], [(abs, 6usize)], "glyph {i}");
    }
    // the last glyph ends exactly at the end of the file
    let last = *reads(&log).last().unwrap();
    assert_eq!(last.0 + last.1 as u64, file_len);
    // an absent char between records
    assert!(r.find('\u{0101}').unwrap().is_none());
}

// ------------------------------------------------------- fallback and pack size

#[test]
fn absent_char_leads_to_the_missing_glyph_for_the_packs_own_size() {
    // find -> Ok(None); the fallback uses the pack's info (16 px: 12x12, adv 16, ox 2, oy -12).
    let mut r = open_slice(&GOLDEN);
    assert!(r.find('Z').unwrap().is_none());
    let mut out = [0u8; 24];
    let got = render_missing_glyph(&r.info(), &mut out).expect("24 bytes suffice for 12x12");
    assert_eq!(got, m(16, 2, -12, 12, 12));
    assert_eq!(got, missing_glyph_metrics(&r.info()));
    assert_eq!(&out[..2], [0xFF, 0xF0], "top border row");
    assert_eq!(&out[2..4], [0x80, 0x10], "first interior row");
}

#[test]
fn each_pack_serves_only_its_own_size_even_when_used_side_by_side() {
    // Two packs, same chars, different pixel sizes / metrics / bitmaps, interleaved use.
    let info16 = FontInfo {
        pixel_size: 16,
        font_id: 0xAAAA_0000_0000_0016,
        line_height: 20,
        ascent: 15,
    };
    let info23 = FontInfo {
        pixel_size: 23,
        font_id: 0xAAAA_0000_0000_0023,
        line_height: 28,
        ascent: 22,
    };
    let mk = |scale: u16, k0: u32| -> Vec<GlyphEntry> {
        ['a', 'b', '\u{4E00}', '\u{20BB7}']
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let mut e = synth_glyph(k0 + i as u32, c, 8 * scale, 4 * scale);
                e.metrics.advance = 100 * scale + i as u16;
                e
            })
            .collect()
    };
    let e16 = mk(1, 0);
    let e23 = mk(2, 50);
    let b16 = build_pack(&info16, &e16).unwrap();
    let b23 = build_pack(&info23, &e23).unwrap();
    let mut r16 = open_slice(&b16);
    let mut r23 = open_slice(&b23);
    assert_eq!(r16.info().pixel_size, 16);
    assert_eq!(r23.info().pixel_size, 23);
    assert_ne!(r16.info().font_id, r23.info().font_id);
    assert_eq!(r16.info(), info16);
    assert_eq!(r23.info(), info23);
    for i in 0..4 {
        let (a, b) = (&e16[i], &e23[i]);
        let ga = fetch(&mut r16, a.codepoint).unwrap();
        let gb = fetch(&mut r23, b.codepoint).unwrap();
        assert_eq!(ga, (a.metrics, a.bitmap.clone()));
        assert_eq!(gb, (b.metrics, b.bitmap.clone()));
        assert_ne!(ga.0, gb.0, "sizes differ in metrics");
    }
    // a fallback glyph is per size, too
    assert_ne!(
        missing_glyph_metrics(&r16.info()),
        missing_glyph_metrics(&r23.info())
    );
}

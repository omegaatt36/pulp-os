//! `PackReader` under damage, I/O failure and instrumentation, plus the
//! read-budget rules.
//!
//! Oracles: literals and the annotated golden bytes; the `Spy` request log (what
//! the reader actually asked of the "SD card"); `Header::decode`,
//! `Record::decode/validate` (directly tested in header_record.rs) and
//! `Pack::parse` for the sweeps. No expectation is taken from reader output.
mod common;
mod reader_support;

use common::*;
use pulp_fontpack::{
    FontError, FontInfo, GlyphEntry, Header, Metrics, Pack, PackError, PackReader, Record,
    build_pack,
};
use reader_support::*;

type Res<T> = Result<T, FontError<SpyErr>>;

fn open_with(bytes: Vec<u8>, file_len: u64) -> (Res<PackReader<Spy>>, Log) {
    let spy = Spy::bytes(bytes);
    let log = spy.log();
    (PackReader::open(spy, file_len), log)
}

fn open_err(bytes: &[u8], file_len: u64) -> (FontError<SpyErr>, Log) {
    let (r, log) = open_with(bytes.to_vec(), file_len);
    match r {
        Ok(_) => panic!("damaged pack was opened (file_len {file_len})"),
        Err(e) => (e, log),
    }
}

fn assert_reqs_within(reqs: &[(u64, usize)], lo: u64, hi: u64, what: &str) {
    for &(off, len) in reqs {
        let end = off.checked_add(len as u64);
        assert!(
            off >= lo && end.is_some_and(|e| e <= hi),
            "{what}: request ({off}, {len}) outside [{lo}, {hi})"
        );
    }
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes(b.try_into().unwrap())
}

// ------------------------------------------------------------------ open

#[test]
fn open_issues_exactly_one_44_byte_read_at_offset_0() {
    for bytes in [GOLDEN.to_vec(), GOLDEN_EMPTY.to_vec()] {
        let (r, log) = open_with(bytes.clone(), bytes.len() as u64);
        let r = r.ok().expect("opens");
        assert_eq!(reads(&log), [(0u64, 44usize)]);
        // accessors do not read
        let _ = r.header();
        let _ = r.info();
        assert_eq!(read_count(&log), 1);
    }
    let big = build_pack(&SYNTH_INFO, &many_glyph_entries()).unwrap();
    let (r, log) = open_with(big.clone(), big.len() as u64);
    assert!(r.is_ok());
    assert_eq!(
        reads(&log),
        [(0u64, 44usize)],
        "no whole-file or whole-index scan"
    );
}

#[test]
fn open_does_not_look_at_the_index_or_the_bitmap() {
    // all index and bitmap bytes destroyed: the header is fine, open is fine (damage there
    // is found only when a record is probed)
    let mut v = GOLDEN.to_vec();
    for b in &mut v[44..] {
        *b = 0xFF;
    }
    let (r, log) = open_with(v, 139);
    assert!(r.is_ok());
    assert_eq!(reads(&log), [(0u64, 44usize)]);
}

#[test]
fn open_of_every_truncation_length_fails_cleanly() {
    // golden cut to n bytes, file_len = n: below 44 -> TooShort with zero reads,
    // from 44 on the header's total_len 139 disagrees with the file -> LengthMismatch
    for n in 0..139usize {
        let (e, log) = open_err(&GOLDEN[..n], n as u64);
        if n < 44 {
            // contract: file_len < 44 -> Corrupt(TooShort) with zero reads, the only answer
            assert_eq!(e, FontError::Corrupt(PackError::TooShort), "n={n}");
            assert!(reads(&log).is_empty(), "n={n}: {:?}", reads(&log));
        } else {
            assert_eq!(e, FontError::Corrupt(PackError::LengthMismatch), "n={n}");
            assert_eq!(reads(&log), [(0u64, 44usize)], "n={n}");
        }
    }
}

#[test]
fn open_with_a_file_len_that_disagrees_with_the_header_fails_cleanly() {
    // the bytes are the intact golden pack, only the caller's file_len is wrong
    for file_len in [
        140u64,
        141,
        1000,
        1 << 32,
        (1 << 32) + 139,
        u32::MAX as u64,
        u64::MAX,
    ] {
        let (e, log) = open_err(&GOLDEN, file_len);
        assert_eq!(
            e,
            FontError::Corrupt(PackError::LengthMismatch),
            "{file_len}"
        );
        assert_eq!(reads(&log), [(0u64, 44usize)]);
    }
    for file_len in [44u64, 45, 100, 131, 132, 138] {
        let (e, _) = open_err(&GOLDEN, file_len);
        assert_eq!(
            e,
            FontError::Corrupt(PackError::LengthMismatch),
            "{file_len}"
        );
    }
    // file_len below the header size, source long enough to be read: TooShort, zero reads
    for file_len in [0u64, 1, 43] {
        let (e, log) = open_err(&GOLDEN, file_len);
        assert_eq!(e, FontError::Corrupt(PackError::TooShort), "{file_len}");
        assert_eq!(
            read_count(&log),
            0,
            "{file_len}: no read below the header size"
        );
    }
}

#[test]
fn open_of_a_header_longer_than_the_file_the_caller_declares_is_not_read_past_file_len() {
    // when file_len >= 44 the only request is the 44-byte header and it lies inside the file
    for file_len in [44u64, 139, 1_000_000] {
        let (_r, log) = open_with(GOLDEN.to_vec(), file_len);
        assert_reqs_within(&reads(&log), 0, file_len, "open");
    }
}

#[test]
fn open_with_one_tampered_header_byte_reports_the_documented_error() {
    // Every header byte, four different damages: outcomes depend on the field.
    for p in 0..44usize {
        for x in [0x01u8, 0x80, 0xFF, 0x55] {
            let mut t = GOLDEN.to_vec();
            t[p] ^= x;
            let (r, log) = open_with(t.clone(), 139);
            assert_eq!(reads(&log), [(0u64, 44usize)], "byte {p}");
            let ctx = format!("byte {p} ^ {x:#x}");
            match p {
                0..=3 => assert_eq!(
                    r.err().expect(&ctx),
                    FontError::Corrupt(PackError::BadMagic),
                    "{ctx}"
                ),
                4 | 5 => assert_eq!(
                    r.err().expect(&ctx),
                    FontError::Unsupported {
                        found: le16(&t[4..6])
                    },
                    "{ctx}"
                ),
                6..=19 => {
                    // pixel_size, font_id, line_height, ascent: unchecked, taken as found
                    let rd = r.ok().unwrap_or_else(|| panic!("{ctx} must open"));
                    let want = FontInfo {
                        pixel_size: le16(&t[6..8]),
                        font_id: u64::from_le_bytes(t[8..16].try_into().unwrap()),
                        line_height: le16(&t[16..18]),
                        ascent: le16(&t[18..20]),
                    };
                    assert_ne!(want, GOLDEN_INFO, "{ctx}: the damage changed a field");
                    assert_eq!(rd.info(), want, "{ctx}");
                    assert_eq!(rd.header().glyph_count, 4);
                    assert_eq!(rd.header().bitmap_len, 7);
                }
                20..=39 => assert_eq!(
                    r.err().expect(&ctx),
                    FontError::Corrupt(PackError::BadLayout),
                    "{ctx}"
                ),
                _ => assert_eq!(
                    r.err().expect(&ctx),
                    FontError::Corrupt(PackError::LengthMismatch),
                    "{ctx}"
                ),
            }
        }
    }
}

#[test]
fn open_reports_unsupported_for_other_versions_and_before_any_structural_damage() {
    for v in [0u16, 2, 9, 0x0100, 0xFFFF] {
        let mut t = GOLDEN.to_vec();
        put_u16(&mut t, H_VERSION, v);
        let (e, _) = open_err(&t, 139);
        assert_eq!(e, FontError::Unsupported { found: v });
        // plus a length and layout fault: the version still wins
        put_u32(&mut t, H_TOTAL_LEN, 77);
        put_u32(&mut t, H_INDEX_LEN, 5);
        let (e, _) = open_err(&t, 139);
        assert_eq!(e, FontError::Unsupported { found: v });
    }
    // wrong magic outranks a wrong version
    let mut t = GOLDEN.to_vec();
    t[0] = b'Q';
    put_u16(&mut t, H_VERSION, 2);
    let (e, _) = open_err(&t, 139);
    assert_eq!(e, FontError::Corrupt(PackError::BadMagic));
}

#[test]
fn open_rejects_layout_overflow_cases_instead_of_wrapping() {
    // four headers that WRAPPING u32 arithmetic would call consistent, through open
    // (file_len = total_len, kept >= 44 so the "file below the header size" rule is not what fires).
    let cases: [(u32, u32, u32, u32, u32); 4] = [
        // count, index_len, bitmap_offset, bitmap_len, total_len
        // 195_225_787 * 22 = 2^32 + 18: index_len 18, bitmap_offset 62
        (195_225_787, 18, 62, 0, 62),
        // 0x8000_0000 * 22 = 11 * 2^32: wraps to 0
        (0x8000_0000, 0, 44, 0, 44),
        // 44 + 4_294_967_270 = 2^32 + 18 -> bitmap_offset 18; 18 + 26 = 44
        (195_225_785, 4_294_967_270, 18, 26, 44),
        // 1 glyph: bitmap_offset 66; 66 + u32::MAX = 2^32 + 65 -> total_len 65
        (1, 22, 66, u32::MAX, 65),
    ];
    for (count, ilen, boff, blen, total) in cases {
        let mut h = GOLDEN_EMPTY.to_vec();
        put_u32(&mut h, H_COUNT, count);
        put_u32(&mut h, H_INDEX_LEN, ilen);
        put_u32(&mut h, H_BITMAP_OFFSET, boff);
        put_u32(&mut h, H_BITMAP_LEN, blen);
        put_u32(&mut h, H_TOTAL_LEN, total);
        let (e, _) = open_err(&h, total as u64);
        assert_eq!(e, FontError::Corrupt(PackError::BadLayout), "count {count}");
    }
}

// ------------------------------------------------ damaged probed records

/// 7 records, codepoints 0x10, 0x20, ... 0x70, each 8x2 (2 bytes) at offset 2*i, region 14 bytes.
fn base_recs() -> Vec<RawRec> {
    (0..7u32)
        .map(|i| rawrec(0x10 * (i + 1), 2 * i, 8, 2))
        .collect()
}

const BASE_REGION: [u8; 14] = [
    0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD,
];

type Damage = (&'static str, fn(&mut RawRec), fn(u32) -> PackError);

fn inv(i: u32) -> PackError {
    PackError::InvalidCodepoint { index: i }
}
fn rge(i: u32) -> PackError {
    PackError::GlyphBitmapRange { index: i }
}
fn siz(i: u32) -> PackError {
    PackError::GlyphSizeMismatch { index: i }
}
fn d(name: &'static str, f: fn(&mut RawRec), e: fn(u32) -> PackError) -> Damage {
    (name, f, e)
}

fn damages() -> Vec<Damage> {
    vec![
        d("cp D800", |r| r.cp = 0xD800, inv),
        d("cp DBFF", |r| r.cp = 0xDBFF, inv),
        d("cp DFFF", |r| r.cp = 0xDFFF, inv),
        d("cp 110000", |r| r.cp = 0x11_0000, inv),
        d("cp u32::MAX", |r| r.cp = u32::MAX, inv),
        d("range end+1", |r| r.off = 13, rge),
        d("range far", |r| r.off = 1000, rge),
        d("range overflow", |r| r.off = u32::MAX, rge),
        d("range overflow by one", |r| r.off = u32::MAX - 1, rge),
        d(
            "size len 3",
            |r| {
                r.off = 0;
                r.len = 3
            },
            siz,
        ),
        d(
            "size len 1",
            |r| {
                r.off = 0;
                r.len = 1
            },
            siz,
        ),
        d(
            "size len 0",
            |r| {
                r.off = 0;
                r.len = 0
            },
            siz,
        ),
        d(
            "size width 9",
            |r| {
                r.off = 0;
                r.m.width = 9
            },
            siz,
        ),
        d(
            "size height 3",
            |r| {
                r.off = 0;
                r.m.height = 3
            },
            siz,
        ),
    ]
}

/// Chars worth probing in the 7-record pack: each codepoint, its neighbours, and the extremes.
fn probe_chars() -> Vec<char> {
    let mut v = vec!['\0', '\u{1}', char::MAX, '\u{D7FF}', '\u{E000}', '\u{FFFF}'];
    for i in 1..=7u32 {
        let c = char::from_u32(0x10 * i).unwrap();
        v.extend([c, prev_char(c).unwrap(), next_char(c).unwrap()]);
    }
    v
}

#[test]
fn a_probed_damaged_record_is_reported_with_its_record_number_and_an_unprobed_one_is_not() {
    // the Spy log tells which records the search really read:
    //   record j probed  <=>  find returns Err(Corrupt(<damage>{index: j}))
    // so damage in unprobed records goes unnoticed (by design) and in probed ones never does.
    let mut probed_cases = 0;
    let mut unprobed_cases = 0;
    for (name, damage, want) in damages() {
        for j in 0..7usize {
            let mut recs = base_recs();
            damage(&mut recs[j]);
            let bytes = raw_pack(&GOLDEN_INFO, &recs, &BASE_REGION);
            let rec_off = (44 + 22 * j) as u64;
            for c in probe_chars() {
                let (mut r, log) = open_spy(&bytes);
                let res = r.find(c);
                let l = reads(&log);
                let probed = l[1..].iter().any(|&(o, _)| o == rec_off);
                assert_reqs_within(&l[1..], 44, 44 + 22 * 7, "find");
                if probed {
                    probed_cases += 1;
                    assert!(
                        matches!(res, Err(FontError::Corrupt(e)) if e == want(j as u32)),
                        "{name}, record {j}, probing {:04X}: {res:?}",
                        c as u32
                    );
                    assert_eq!(
                        *l.last().unwrap(),
                        (rec_off, 22),
                        "the damaged read ends the search"
                    );
                } else {
                    unprobed_cases += 1;
                    let ideal = (1..=7u32).find(|&i| i as usize != j + 1 && 0x10 * i == c as u32);
                    match (res, ideal) {
                        (Ok(Some(g)), Some(i)) => {
                            assert_eq!(g.metrics, recs[i as usize - 1].m, "{name} rec {j}");
                        }
                        (Ok(None), None) => {}
                        // the damaged record's own char (cp changed or never reached)
                        (Ok(None), Some(_)) | (Ok(Some(_)), None) => {
                            panic!("{name}, record {j}, probing {:04X}: wrong answer", c as u32)
                        }
                        (other, _) => panic!("{name} rec {j}: {other:?}"),
                    }
                }
            }
        }
    }
    assert!(probed_cases > 300, "probed cases: {probed_cases}");
    assert!(unprobed_cases > 300, "unprobed cases: {unprobed_cases}");
}

#[test]
fn a_one_record_pack_always_probes_that_record() {
    for (name, damage, want) in damages() {
        let mut recs = vec![rawrec(0x41, 0, 8, 2)];
        damage(&mut recs[0]);
        // region of 14 bytes, so that "size len 3" (offset 0 + 3) is inside it and the
        // size rule, not the range rule, is the one that fires
        let bytes = raw_pack(&GOLDEN_INFO, &recs, &BASE_REGION);
        for c in ['\0', 'A', 'B', char::MAX, '\u{D7FF}'] {
            let (mut r, log) = open_spy(&bytes);
            let res = r.find(c);
            assert_eq!(
                res.err()
                    .unwrap_or_else(|| panic!("{name}: damaged record not reported")),
                FontError::Corrupt(want(0)),
                "{name} probing {:04X}",
                c as u32
            );
            assert_eq!(reads(&log), [(0u64, 44usize), (44, 22)], "{name}");
        }
    }
}

#[test]
fn a_probed_record_failing_several_rules_reports_the_first_in_contract_order() {
    // a: invalid codepoint beats c (range) and d (size)
    let mut r = rawrec(0xD800, 100, 8, 2);
    r.len = 5;
    let b = raw_pack(&GOLDEN_INFO, &[r], &[0, 0]);
    let (mut rd, _) = open_spy(&b);
    assert_eq!(
        rd.find('A').err().unwrap(),
        FontError::Corrupt(PackError::InvalidCodepoint { index: 0 })
    );
    // c: range beats d: offset 100 + len 5 > region 2 and 5 != 2
    let mut r = rawrec(0x41, 100, 8, 2);
    r.len = 5;
    let b = raw_pack(&GOLDEN_INFO, &[r], &[0, 0]);
    let (mut rd, _) = open_spy(&b);
    assert_eq!(
        rd.find('A').err().unwrap(),
        FontError::Corrupt(PackError::GlyphBitmapRange { index: 0 })
    );
}

#[test]
fn a_damaged_probed_record_is_an_error_even_when_it_is_not_the_wanted_char() {
    // single record U+0041 with a bad size; asking for 'Z' still probes and rejects it
    let mut recs = vec![rawrec(0x41, 0, 8, 2)];
    recs[0].len = 1;
    let b = raw_pack(&GOLDEN_INFO, &recs, &[0x3C, 0x42]);
    let (mut r, _) = open_spy(&b);
    assert!(matches!(
        r.find('Z'),
        Err(FontError::Corrupt(PackError::GlyphSizeMismatch {
            index: 0
        }))
    ));
}

// ------------------------------------------------ disorder is never an error

fn recs_for(cps: &[u32]) -> Vec<RawRec> {
    cps.iter()
        .enumerate()
        .map(|(i, &cp)| {
            let mut r = rawrec(cp, 2 * i as u32, 8, 2);
            r.m.advance = 1000 + i as u16; // unique, so a hit identifies its record
            r
        })
        .collect()
}

fn region_for(n: usize) -> Vec<u8> {
    (0..2 * n).map(|i| 0x40 + i as u8).collect()
}

/// Any answer from a disordered index must be Ok; a hit must be the record that has that
/// codepoint; all reads stay in the index; no more than the probe bound.
fn check_disordered(cps: &[u32]) {
    let recs = recs_for(cps);
    let bytes = raw_pack(&GOLDEN_INFO, &recs, &region_for(cps.len()));
    assert!(Pack::parse(&bytes).is_err() || cps.windows(2).all(|w| w[0] < w[1]));
    let n = cps.len() as u32;
    let mut chars: Vec<char> = cps.iter().filter_map(|&c| char::from_u32(c)).collect();
    chars.extend(['\0', '\u{7F}', char::MAX, '\u{55}']);
    for c in chars {
        let (mut r, log) = open_spy(&bytes);
        let res = r
            .find(c)
            .unwrap_or_else(|e| panic!("{cps:X?} find {c:?}: {e:?}"));
        let l = reads(&log);
        assert!(
            l.len() - 1 <= max_probes(n),
            "{cps:X?}: {} probes",
            l.len() - 1
        );
        assert_reqs_within(&l[1..], 44, 44 + 22 * n as u64, "find");
        if let Some(g) = res {
            let k = cps
                .iter()
                .position(|&x| x == c as u32)
                .expect("hit => cp is in the index");
            let matching: Vec<&RawRec> = recs.iter().filter(|r| r.cp == c as u32).collect();
            assert!(
                matching.iter().any(|r| r.m == g.metrics),
                "{cps:X?} {c:?} (k={k})"
            );
            let mut buf = [0u8; 2];
            let bm = r.read_bitmap(&g, &mut buf).unwrap().to_vec();
            let off = matching.iter().find(|r| r.m == g.metrics).unwrap().off as usize;
            assert_eq!(bm, region_for(cps.len())[off..off + 2]);
        }
    }
}

#[test]
fn a_descending_index_never_errors_nor_reads_out_of_bounds() {
    check_disordered(&[0x70, 0x60, 0x50, 0x40, 0x30, 0x20, 0x10]);
    check_disordered(&[0x20BB7, 0x10000, 0xFFFF, 0x41, 0]);
}

#[test]
fn duplicates_and_shuffles_never_error_nor_read_out_of_bounds() {
    check_disordered(&[0x41, 0x41, 0x41]);
    check_disordered(&[0x30, 0x10, 0x20]);
    check_disordered(&[0x10, 0x30, 0x20, 0x40]);
    check_disordered(&[0x10, 0x10, 0x20, 0x20, 0x30]);
    check_disordered(&[0x41]);
    let mut rng = Lcg::new(0x5EED_0001);
    for _ in 0..150 {
        let n = 1 + rng.below(9) as usize;
        let cps: Vec<u32> = (0..n).map(|_| 0x10 * (1 + rng.below(6))).collect();
        check_disordered(&cps);
    }
}

#[test]
fn a_swapped_pair_in_an_otherwise_sorted_index_only_costs_lookups_never_safety() {
    // chars of an intact sorted pack stay findable, except possibly the two swapped
    let cps: Vec<u32> = (1..=15).map(|i| i * 0x10).collect();
    for a in 0..14 {
        let mut s = cps.clone();
        s.swap(a, a + 1);
        check_disordered(&s);
        let recs = recs_for(&s);
        let bytes = raw_pack(&GOLDEN_INFO, &recs, &region_for(s.len()));
        let mut r = PackReader::open(&bytes[..], bytes.len() as u64)
            .ok()
            .unwrap();
        // a hit is never a wrong glyph, and chars absent from the index stay absent
        for c in [0u32, 0x15, 0x105, 0xF1] {
            assert!(r.find(char::from_u32(c).unwrap()).unwrap().is_none());
        }
    }
}

// -------------------------------------------------------------- I/O injection

/// Failure on every read number k, for one scenario: open, find(c), read_bitmap(found glyph).
fn inject(bytes: &[u8], c: char, expect_bitmap_read: bool) {
    let file_len = bytes.len() as u64;
    // clean run: measure the reads each phase takes
    let (mut r, log) = open_spy(bytes);
    let after_open = read_count(&log);
    let found = r.find(c).unwrap();
    let after_find = read_count(&log);
    if let Some(g) = found {
        let mut buf = vec![0u8; g.bitmap_len as usize];
        r.read_bitmap(&g, &mut buf).unwrap();
    }
    let total = read_count(&log);
    assert_eq!(after_open, 1);
    assert_eq!(
        total - after_find,
        usize::from(expect_bitmap_read),
        "bitmap reads"
    );

    for k in 1..=total + 2 {
        let spy = Spy::bytes(bytes.to_vec()).failing_on(k);
        let log = spy.log();
        let ctx = format!("fail on read {k} of {total} (char {:04X})", c as u32);
        let mut reader = match PackReader::open(spy, file_len) {
            Ok(r) => r,
            Err(e) => {
                assert_eq!(k, 1, "{ctx}");
                assert_eq!(e, FontError::Io(SpyErr::Injected(1)), "{ctx}");
                assert_eq!(read_count(&log), 1, "{ctx}: no retry");
                continue;
            }
        };
        assert_ne!(k, 1, "{ctx}: open must fail when its read fails");
        let found = match reader.find(c) {
            Ok(f) => f,
            Err(e) => {
                assert!((2..=after_find).contains(&k), "{ctx}");
                assert_eq!(e, FontError::Io(SpyErr::Injected(k)), "{ctx}");
                assert_eq!(read_count(&log), k, "{ctx}: no retry");
                continue;
            }
        };
        assert!(k > after_find, "{ctx}: find survived its failing read");
        if let Some(g) = found {
            let mut b = vec![0u8; g.bitmap_len as usize];
            match reader.read_bitmap(&g, &mut b) {
                Ok(_) => assert!(k > total, "{ctx}: read_bitmap survived its failing read"),
                Err(e) => {
                    assert_eq!(k, total, "{ctx}");
                    assert_eq!(e, FontError::Io(SpyErr::Injected(k)), "{ctx}");
                    assert_eq!(read_count(&log), k, "{ctx}: no retry");
                }
            }
        } else {
            assert!(k > total, "{ctx}");
        }
        assert_reqs_within(&reads(&log), 0, file_len, &ctx);
    }
}

#[test]
fn a_read_failure_at_any_point_is_io_with_the_source_error_and_is_not_retried() {
    inject(&GOLDEN, 'A', true);
    inject(&GOLDEN, '\u{20BB7}', true);
    inject(&GOLDEN, '\u{10FFFF}', false); // blank: nothing to read
    inject(&GOLDEN, 'Z', false); // absent
    inject(&GOLDEN_EMPTY, 'A', false);
    let large = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    inject(&large, '\u{10000}', true);
    inject(&large, '\u{FFFF}', true);
    inject(&large, '\u{100}', true);
    inject(&large, '\u{101}', false);
    let many = build_pack(&SYNTH_INFO, &many_glyph_entries()).unwrap();
    inject(&many, char::from_u32(351 * 1234).unwrap(), true);
    inject(&many, char::from_u32(351 * 1234 + 1).unwrap(), false);
}

#[test]
fn io_error_values_pass_through_unchanged() {
    // the reader returns the source's own error value, not a stand-in
    let spy = Spy::bytes(GOLDEN.to_vec()).failing_on(2); // the first probe
    let mut r = PackReader::open(spy, 139).ok().unwrap();
    let e = r.find('A').err();
    assert_eq!(e, Some(FontError::Io(SpyErr::Injected(2))));
}

#[test]
fn a_source_shorter_than_the_declared_file_gives_io_not_a_panic() {
    // the card returns "unavailable" for everything past n bytes while file_len says 139
    for n in 44..=139usize {
        let spy = Spy::bytes(GOLDEN[..n].to_vec());
        let log = spy.log();
        let mut r = PackReader::open(spy, 139).ok().expect("header is within n");
        for c in ['A', '\u{FFFF}', '\u{20BB7}', '\u{10FFFF}', 'Z'] {
            match r.find(c) {
                Ok(Some(g)) => {
                    let mut buf = vec![0u8; g.bitmap_len as usize];
                    match r.read_bitmap(&g, &mut buf) {
                        Ok(s) => assert_eq!(s.len(), g.bitmap_len as usize),
                        Err(FontError::Io(SpyErr::Unavailable)) => {}
                        Err(e) => panic!("n={n}: {e:?}"),
                    }
                }
                Ok(None) | Err(FontError::Io(SpyErr::Unavailable)) => {}
                Err(e) => panic!("n={n} {c:?}: {e:?}"),
            }
        }
        assert_reqs_within(&reads(&log), 0, 139, "short source");
    }
}

// ---------------------------------------------------------------- read budget

/// n glyphs with codepoints 7 + 3*i; every 5th is blank; the rest 8x(1 + i%3).
fn budget_entries(n: usize) -> Vec<GlyphEntry> {
    (0..n)
        .map(|i| {
            let c = char::from_u32(7 + 3 * i as u32).unwrap();
            if i % 5 == 4 {
                GlyphEntry {
                    codepoint: c,
                    metrics: Metrics {
                        advance: 6,
                        offset_x: 0,
                        offset_y: 0,
                        width: 0,
                        height: 0,
                    },
                    bitmap: vec![],
                }
            } else {
                synth_glyph(i as u32, c, 8, 1 + (i % 3) as u16)
            }
        })
        .collect()
}

#[test]
fn find_and_read_bitmap_stay_within_the_read_budget_for_every_pack_size() {
    for n in [
        0usize, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 100, 255, 256, 1000, 3000,
    ] {
        let entries = budget_entries(n);
        let bytes = build_pack(&SYNTH_INFO, &entries).unwrap();
        let (mut r, log) = open_spy(&bytes);
        assert_eq!(reads(&log), [(0u64, 44usize)], "n={n}");
        let bitmap_base = 44 + 22 * n as u64;
        // hand-derived: relative offset of glyph i = sum of the bitmap lengths before it
        let mut off = 0u64;
        for (i, e) in entries.iter().enumerate() {
            let before = read_count(&log);
            let g = r
                .find(e.codepoint)
                .unwrap()
                .unwrap_or_else(|| panic!("n={n} i={i}"));
            let l = reads(&log);
            let probes = &l[before..];
            assert!(
                probes.len() >= 1 && probes.len() <= max_probes(n as u32),
                "n={n} i={i}: {}",
                probes.len()
            );
            for &(o, len) in probes {
                assert_eq!(len, 22, "n={n}: probe size");
                assert!(
                    o >= 44 && (o - 44) % 22 == 0 && (o - 44) / 22 < n as u64,
                    "n={n}: probe at {o}"
                );
            }
            assert_eq!(g.metrics, e.metrics);
            assert_eq!(g.bitmap_len as usize, e.bitmap.len());

            let before = read_count(&log);
            let mut buf = vec![0xEEu8; e.bitmap.len() + 2];
            let got = r.read_bitmap(&g, &mut buf).unwrap().to_vec();
            assert_eq!(got, e.bitmap);
            let l = reads(&log);
            if e.bitmap.is_empty() {
                assert_eq!(l.len(), before, "n={n} i={i}: a blank glyph is not read");
            } else {
                assert_eq!(
                    l[before..],
                    [(bitmap_base + off, e.bitmap.len())],
                    "n={n} i={i}"
                );
            }
            off += e.bitmap.len() as u64;
        }
        // absent chars: probes only, within the bound
        for c in [
            '\0',
            '\u{6}',
            '\u{8}',
            '\u{9}',
            char::MAX,
            char::from_u32(7 + 3 * n as u32 + 1).unwrap(),
        ] {
            let before = read_count(&log);
            assert!(r.find(c).unwrap().is_none(), "n={n} {c:?}");
            let l = reads(&log);
            assert!(l.len() - before <= max_probes(n as u32), "n={n} {c:?}");
            assert_reqs_within(&l[before..], 44, 44 + 22 * n as u64, "absent probe");
            if n == 0 {
                assert_eq!(l.len(), before, "a 0-glyph pack costs 0 reads per find");
            }
        }
    }
}

#[test]
fn golden_read_addresses_are_the_hand_derived_ones() {
    // bitmap region starts at 44 + 22*4 = 132; glyph offsets 0, 2, 3, (blank 7)
    let (mut r, log) = open_spy(&GOLDEN);
    for (c, abs, len) in [
        ('A', 132u64, 2usize),
        ('\u{FFFF}', 134, 1),
        ('\u{20BB7}', 135, 4),
    ] {
        let g = r.find(c).unwrap().unwrap();
        let before = read_count(&log);
        let mut b = [0u8; 4];
        r.read_bitmap(&g, &mut b).unwrap();
        assert_eq!(reads(&log)[before..], [(abs, len)], "U+{:04X}", c as u32);
    }
    let g = r.find('\u{10FFFF}').unwrap().unwrap();
    let before = read_count(&log);
    let mut b = [0u8; 4];
    assert!(r.read_bitmap(&g, &mut b).unwrap().is_empty());
    assert_eq!(read_count(&log), before);
}

#[test]
fn r2_large_offset_glyphs_are_read_from_their_exact_addresses() {
    // 7 glyphs, bitmap base 44 + 7*22 = 198; relative offsets from the table in common:
    // 0, 65535, 65536, 65544, 65744, 65894, 66134 (a u16 truncation moves 65536.. to 0..)
    let bytes = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    let (mut r, log) = open_spy(&bytes);
    let entries = large_offset_entries();
    for (i, e) in entries.iter().enumerate() {
        let g = r.find(e.codepoint).unwrap().unwrap();
        let before = read_count(&log);
        let mut buf = vec![0u8; e.bitmap.len()];
        assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), &e.bitmap[..]);
        assert_eq!(
            reads(&log)[before..],
            [(198 + LARGE_OFFSET_EXPECTED[i] as u64, e.bitmap.len())],
            "glyph {i}"
        );
    }
}

// ------------------------------------------------------------------ full Unicode, virtual

/// Scalar number `k` (0-based, skipping the surrogate block) and its inverse.
fn scalar(k: u32) -> char {
    char::from_u32(if k < 0xD800 { k } else { k + 0x800 }).unwrap()
}
fn scalar_index(c: char) -> u32 {
    let v = c as u32;
    if v < 0xD800 { v } else { v - 0x800 }
}

const VM: u32 = 556_032; // records: every even scalar number 0, 2, 4, ... (1,112,064 / 2)
const VREC_OFF: u32 = 32; // bitmap offset of record j = 32 * j (region 17,793,024 > 16 MiB)

fn v_metrics(j: u32) -> Metrics {
    m(
        (j % 1000) as u16 + 1,
        (j % 7) as i16 - 3,
        -((j % 11) as i16),
        16,
        1,
    )
}
fn v_byte(j: u32, t: u32) -> u8 {
    (j.wrapping_mul(0x9E37_79B1) >> (3 + 8 * t)) as u8
}

fn v_header() -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"PFNT");
    put_u16(&mut h, H_VERSION, 1);
    put_u16(&mut h, H_PIXEL_SIZE, 20);
    put_u16(&mut h, H_LINE_HEIGHT, 25);
    put_u16(&mut h, H_ASCENT, 19);
    put_u32(&mut h, H_COUNT, VM);
    put_u32(&mut h, H_INDEX_OFFSET, 44);
    put_u32(&mut h, H_INDEX_LEN, 22 * VM);
    put_u32(&mut h, H_BITMAP_OFFSET, 44 + 22 * VM);
    put_u32(&mut h, H_BITMAP_LEN, VREC_OFF * VM);
    put_u32(&mut h, H_TOTAL_LEN, 44 + 22 * VM + VREC_OFF * VM);
    h
}

fn v_record(j: u32) -> [u8; 22] {
    let mut r = [0u8; 22];
    let m = v_metrics(j);
    put_u32(&mut r, R_CODEPOINT, scalar(2 * j) as u32);
    put_u32(&mut r, R_BITMAP_OFFSET, VREC_OFF * j);
    put_u32(&mut r, R_BITMAP_LEN, 2);
    put_u16(&mut r, R_ADVANCE, m.advance);
    put_u16(&mut r, R_OFFSET_X, m.offset_x as u16);
    put_u16(&mut r, R_OFFSET_Y, m.offset_y as u16);
    put_u16(&mut r, R_WIDTH, 16);
    put_u16(&mut r, R_HEIGHT, 1);
    r
}

fn v_file_byte(p: u64) -> u8 {
    let index_end = 44 + 22 * VM as u64;
    if p < 44 {
        v_header()[p as usize]
    } else if p < index_end {
        v_record(((p - 44) / 22) as u32)[((p - 44) % 22) as usize]
    } else {
        let rel = p - index_end;
        let (j, t) = (
            (rel / VREC_OFF as u64) as u32,
            (rel % VREC_OFF as u64) as u32,
        );
        if t < 2 { v_byte(j, t) } else { 0xEE }
    }
}

#[test]
fn r2_virtual_pack_of_half_of_unicode_with_a_16_mib_bitmap_region() {
    // 556,032 records (index 12.2 MB), every even scalar number is present; bitmap offsets up
    // to 17.8 MB. The file only exists as a function (no memory): reads are synthesised.
    let file_len = 44 + 22 * VM as u64 + (VREC_OFF * VM) as u64;
    assert_eq!(file_len, 44 + 54 * 556_032);
    let spy = Spy::sparse(file_len, |off, buf| {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = v_file_byte(off + i as u64);
        }
    });
    let log = spy.log();
    let mut r = PackReader::open(spy, file_len).expect("virtual pack opens");
    assert_eq!(r.header().glyph_count, VM);
    assert_eq!(r.info().pixel_size, 20);

    // scalar numbers to probe: the start, around the surrogate gap, around U+FFFF/U+10000,
    // U+20BB7, the end, and a stride sweep
    let mut s: Vec<u32> = (0..70).collect();
    s.extend(0xD7F0..0xD810);
    s.extend(0xFFF0..0x1_0010);
    s.extend(0x20BB7 - 0x800 - 6..0x20BB7 - 0x800 + 6);
    s.extend(0x10F7FF - 20..=0x10F7FF);
    s.extend((0..0x10F800).step_by(1009));
    let index_end = 44 + 22 * VM as u64;
    let mut hits = 0;
    for k in s {
        let c = scalar(k);
        assert_eq!(scalar_index(c), k);
        let before = read_count(&log);
        let found = r
            .find(c)
            .unwrap_or_else(|e| panic!("U+{:04X}: {e:?}", c as u32));
        let l = reads(&log);
        assert!(
            l.len() - before <= max_probes(VM),
            "{} probes",
            l.len() - before
        );
        assert_eq!(max_probes(VM), 20);
        assert_reqs_within(&l[before..], 44, index_end, "probe");
        if k % 2 == 1 {
            assert!(
                found.is_none(),
                "U+{:04X} (odd scalar number) must be absent",
                c as u32
            );
            continue;
        }
        let j = k / 2;
        let g = found.unwrap_or_else(|| panic!("U+{:04X} (record {j}) not found", c as u32));
        hits += 1;
        assert_eq!(g.metrics, v_metrics(j), "U+{:04X}", c as u32);
        assert_eq!(g.bitmap_len, 2);
        let before = read_count(&log);
        let mut buf = [0u8; 2];
        let got = r.read_bitmap(&g, &mut buf).unwrap().to_vec();
        assert_eq!(got, [v_byte(j, 0), v_byte(j, 1)], "U+{:04X}", c as u32);
        assert_eq!(
            reads(&log)[before..],
            [(index_end + (VREC_OFF as u64) * j as u64, 2usize)],
            "U+{:04X}",
            c as u32
        );
    }
    assert!(hits > 500);
    // the extremes explicitly
    assert!(r.find('\0').unwrap().is_some());
    assert!(r.find('\u{10FFFE}').unwrap().is_some());
    assert!(r.find('\u{10FFFF}').unwrap().is_none());
}

#[test]
fn largest_legal_index_probes_stay_inside_the_declared_file() {
    // header-only claim of 195,225,784 glyphs in a u32::MAX-byte file whose records are noise:
    // every probe must be a 22-byte read inside the index, at most 28 of them, whatever the noise says
    let mut h = GOLDEN_EMPTY.to_vec();
    put_u32(&mut h, H_COUNT, 195_225_784);
    put_u32(&mut h, H_INDEX_LEN, 4_294_967_248);
    put_u32(&mut h, H_BITMAP_OFFSET, 4_294_967_292);
    put_u32(&mut h, H_BITMAP_LEN, 3);
    put_u32(&mut h, H_TOTAL_LEN, u32::MAX);
    let file_len = u32::MAX as u64;
    for seed in 0..6u32 {
        let h2 = h.clone();
        let spy = Spy::sparse(file_len, move |off, buf| {
            for (i, b) in buf.iter_mut().enumerate() {
                let p = off + i as u64;
                *b = if p < 44 {
                    h2[p as usize]
                } else {
                    ((p as u32 ^ seed).wrapping_mul(0x9E37_79B1) >> 11) as u8
                };
            }
        });
        let log = spy.log();
        let mut r = PackReader::open(spy, file_len).expect("header is consistent");
        assert_eq!(max_probes(195_225_784), 28);
        for c in ['\0', 'A', '\u{FFFF}', '\u{20BB7}', char::MAX] {
            let before = read_count(&log);
            let _ = r.find(c); // any outcome but a panic is acceptable on noise
            let l = reads(&log);
            assert!(l.len() - before <= 28);
            assert_reqs_within(&l[before..], 44, 4_294_967_292, "noise probe");
            for &(o, len) in &l[before..] {
                assert_eq!(len, 22);
                assert_eq!((o - 44) % 22, 0);
            }
        }
    }
}

// ------------------------------------------------------------ sweeps (no panic)

#[derive(Default, Debug)]
struct Stats {
    opened: u32,
    header_err: u32,
    unsupported: u32,
    io: u32,
    found: u32,
    absent: u32,
    record_err: u32,
    bitmap_ok: u32,
    too_small: u32,
}

fn header_level(e: PackError) -> bool {
    matches!(
        e,
        PackError::TooShort
            | PackError::BadMagic
            | PackError::UnsupportedVersion { .. }
            | PackError::LengthMismatch
            | PackError::BadLayout
    )
}

/// Run open + probes against `bytes` with the caller-declared `file_len` and check every
/// invariant of the contract that does not need to know the "right" answer.
fn exercise(bytes: &[u8], file_len: u64, probes: &[char], rng: &mut Lcg, st: &mut Stats) {
    let (res, log) = open_with(bytes.to_vec(), file_len);
    let open_reads = reads(&log);
    if file_len >= 44 {
        assert_eq!(
            open_reads,
            [(0u64, 44usize)],
            "open reads exactly the header"
        );
    } else {
        assert!(open_reads.is_empty(), "file_len {file_len} < 44: no read");
    }

    // oracle for the header-level verdict
    let header_verdict: Option<Result<Header, PackError>> = if file_len >= 44 && bytes.len() >= 44 {
        Some(Header::decode(&bytes[..44], file_len))
    } else {
        None
    };

    let mut r = match res {
        Ok(r) => r,
        Err(e) => {
            match e {
                FontError::Unsupported { found } => {
                    st.unsupported += 1;
                    assert_eq!(
                        header_verdict,
                        Some(Err(PackError::UnsupportedVersion { found }))
                    );
                }
                FontError::Corrupt(pe) => {
                    st.header_err += 1;
                    assert!(
                        header_level(pe),
                        "open reported a record-level error: {pe:?}"
                    );
                    assert!(!matches!(pe, PackError::UnsupportedVersion { .. }));
                    match header_verdict {
                        Some(v) => assert_eq!(v, Err(pe)),
                        None => {
                            assert_eq!(pe, PackError::TooShort);
                            assert!(file_len < 44, "TooShort without a declared file below 44");
                        }
                    }
                }
                FontError::Io(SpyErr::Unavailable) => {
                    st.io += 1;
                    assert!(bytes.len() < 44, "the source had the header bytes");
                    assert!(file_len >= 44, "file_len < 44 must be TooShort, not Io");
                }
                other => panic!("unexpected open error {other:?}"),
            }
            return;
        }
    };
    st.opened += 1;
    let want_header = header_verdict
        .expect("an opened pack had a readable header")
        .expect("and a valid one");
    assert_eq!(r.header(), want_header);
    assert_eq!(r.info(), want_header.info);
    let n = want_header.glyph_count;
    let base = 44 + 22 * n as u64;
    let region = want_header.bitmap_len;
    let whole = (bytes.len() as u64 == file_len)
        .then(|| Pack::parse(bytes).ok())
        .flatten();
    let have_all = bytes.len() as u64 >= file_len;

    for &c in probes {
        let before = read_count(&log);
        let res = r.find(c);
        let l = reads(&log);
        let probe_reads = &l[before..];
        assert!(
            probe_reads.len() <= max_probes(n),
            "{} probes for n={n}",
            probe_reads.len()
        );
        for &(o, len) in probe_reads {
            assert_eq!(len, 22);
            assert!(
                o >= 44 && (o - 44) % 22 == 0 && (o - 44) / 22 < n as u64,
                "probe at {o}"
            );
            assert!(o + 22 <= file_len);
        }
        let raw = |index: u32| -> Option<Record> {
            let s = 44 + 22 * index as usize;
            bytes.get(s..).and_then(Record::decode)
        };
        match res {
            Ok(Some(g)) => {
                st.found += 1;
                // it must be a probed, valid record with that codepoint
                let hit = probe_reads
                    .iter()
                    .filter_map(|&(o, _)| raw(((o - 44) / 22) as u32))
                    .find(|rec| {
                        rec.codepoint == c as u32
                            && rec.metrics == g.metrics
                            && rec.bitmap_len == g.bitmap_len
                    })
                    .expect("a hit is a probed record");
                assert!(hit.bitmap_offset as u64 + hit.bitmap_len as u64 <= region as u64);
                assert_eq!(
                    hit.bitmap_len as usize,
                    (g.metrics.width as usize).div_ceil(8) * g.metrics.height as usize
                );
                if let Some(p) = &whole {
                    let og = p.find(c).expect("oracle agrees it is present");
                    assert_eq!(og.metrics, g.metrics);
                }
                let len = g.bitmap_len as usize;
                for have in [len, len + 3, len.saturating_sub(1), 0] {
                    let mut buf = vec![0xD7u8; have];
                    let before = read_count(&log);
                    let out = r.read_bitmap(&g, &mut buf);
                    let l = reads(&log);
                    let new = &l[before..];
                    if have < len {
                        assert_eq!(out, Err(FontError::BufferTooSmall { needed: len }));
                        assert!(new.is_empty());
                        assert!(buf.iter().all(|&b| b == 0xD7));
                        st.too_small += 1;
                        continue;
                    }
                    match out {
                        Ok(s) => {
                            st.bitmap_ok += 1;
                            assert_eq!(s.len(), len);
                            if len == 0 {
                                assert!(new.is_empty());
                            } else {
                                let abs = base + hit.bitmap_offset as u64;
                                assert_eq!(new, [(abs, len)]);
                                assert!(abs + len as u64 <= file_len);
                                assert_eq!(s, &bytes[abs as usize..abs as usize + len]);
                            }
                            assert!(
                                buf[len..].iter().all(|&b| b == 0xD7),
                                "beyond bitmap_len untouched"
                            );
                        }
                        Err(FontError::Io(SpyErr::Unavailable)) => {
                            assert!(!have_all, "source had the bytes");
                            st.io += 1;
                        }
                        Err(e) => panic!("read_bitmap: {e:?}"),
                    }
                }
            }
            Ok(None) => {
                st.absent += 1;
                if let Some(p) = &whole {
                    assert!(
                        p.find(c).is_none(),
                        "{c:?} is present in a pack Pack::parse accepts"
                    );
                }
            }
            Err(FontError::Corrupt(pe)) => {
                st.record_err += 1;
                let idx = match pe {
                    PackError::InvalidCodepoint { index }
                    | PackError::GlyphBitmapRange { index }
                    | PackError::GlyphSizeMismatch { index } => index,
                    other => panic!("find reported a non-record error {other:?}"),
                };
                assert!(idx < n);
                assert!(
                    whole.is_none(),
                    "a pack Pack::parse accepts cannot have a damaged record"
                );
                let rec = raw(idx).expect("probed record is inside the data");
                assert_eq!(rec.validate(idx, None, region), Err(pe));
                let last = *probe_reads.last().unwrap();
                assert_eq!(
                    last,
                    (44 + 22 * idx as u64, 22),
                    "the failing probe ends the search"
                );
            }
            Err(FontError::Io(SpyErr::Unavailable)) => {
                st.io += 1;
                assert!(!have_all);
            }
            Err(other) => panic!("find: unexpected {other:?}"),
        }
    }
    let _ = rng;
}

fn pick_pos(rng: &mut Lcg, len: usize) -> usize {
    if len == 0 {
        0
    } else if rng.chance(2) {
        rng.below(len.min(44) as u32) as usize
    } else if rng.chance(2) {
        rng.below(len.min(44 + 22 * 8) as u32) as usize
    } else {
        rng.below(len as u32) as usize
    }
}

fn mutate(rng: &mut Lcg, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(3) {
        match rng.below(8) {
            0 if !v.is_empty() => {
                let p = pick_pos(rng, v.len());
                v[p] = rng.next_u32() as u8;
            }
            1 if !v.is_empty() => {
                let p = pick_pos(rng, v.len());
                v[p] ^= 1 << rng.below(8);
            }
            2 if v.len() >= 4 => {
                let p = pick_pos(rng, v.len() - 3);
                let e = rng.edgy_u32();
                put_u32(&mut v, p, e);
            }
            3 if v.len() >= 2 => {
                let p = pick_pos(rng, v.len() - 1);
                let e = rng.edgy_u32() as u16;
                put_u16(&mut v, p, e);
            }
            4 if v.len() >= 44 => {
                let f = [
                    H_COUNT,
                    H_INDEX_OFFSET,
                    H_INDEX_LEN,
                    H_BITMAP_OFFSET,
                    H_BITMAP_LEN,
                    H_TOTAL_LEN,
                ];
                let p = f[rng.below(6) as usize];
                let e = rng.edgy_u32();
                put_u32(&mut v, p, e);
            }
            5 => {
                let n = rng.below(v.len() as u32 + 1) as usize;
                v.truncate(n);
            }
            6 => {
                for _ in 0..1 + rng.below(30) {
                    v.push(rng.next_u32() as u8);
                }
            }
            _ => {}
        }
    }
    v
}

fn declared_len(rng: &mut Lcg, v: &[u8]) -> u64 {
    match rng.below(7) {
        0..=2 => v.len() as u64,
        3 => (v.len() as u64).saturating_sub(1),
        4 => v.len() as u64 + 1,
        5 if v.len() >= 44 => get_u32(v, H_TOTAL_LEN) as u64,
        _ => [0, 43, 44, 139, u32::MAX as u64, 1 << 32, u64::MAX][rng.below(7) as usize],
    }
}

fn probes_for(rng: &mut Lcg, v: &[u8]) -> Vec<char> {
    let mut p = vec!['\0', char::MAX, 'A', '\u{FFFF}', '\u{10000}', '\u{20BB7}'];
    for i in 0..12usize {
        let s = 44 + 22 * i;
        if v.len() >= s + 4 {
            if let Some(c) = char::from_u32(get_u32(v, s)) {
                p.push(c);
                p.extend(prev_char(c));
                p.extend(next_char(c));
            }
        }
    }
    for _ in 0..6 {
        p.push(rng.any_char());
    }
    p
}

#[test]
fn mutation_sweep_over_valid_packs_never_panics_and_never_reads_out_of_bounds() {
    let small = build_pack(&SYNTH_INFO, &budget_entries(20)).unwrap();
    let large = build_pack(&SYNTH_INFO, &large_offset_entries()).unwrap();
    let bases: Vec<Vec<u8>> = vec![
        GOLDEN.to_vec(),
        GOLDEN_EMPTY.to_vec(),
        small,
        large,
        build_pack(&SYNTH_INFO, &budget_entries(1)).unwrap(),
    ];
    let mut rng = Lcg::new(0xC0FFEE_0001);
    let mut st = Stats::default();
    for it in 0..6000 {
        let base = &bases[(it % bases.len() as u32) as usize];
        let v = mutate(&mut rng, base);
        let fl = declared_len(&mut rng, &v);
        let probes = probes_for(&mut rng, &v);
        exercise(&v, fl, &probes, &mut rng, &mut st);
    }
    // the sweep must actually reach every outcome, otherwise it proves nothing
    assert!(st.opened > 300, "{st:?}");
    assert!(st.header_err > 300, "{st:?}");
    assert!(st.unsupported > 5, "{st:?}");
    assert!(st.found > 1000, "{st:?}");
    assert!(st.absent > 1000, "{st:?}");
    assert!(st.record_err > 100, "{st:?}");
    assert!(st.bitmap_ok > 1000, "{st:?}");
    assert!(st.too_small > 100, "{st:?}");
}

/// A random pack with a consistent header and random (mostly sound, sometimes broken) records.
fn random_pack(rng: &mut Lcg) -> Vec<u8> {
    let n = rng.below(9) as usize;
    let mut cps: Vec<u32> = (0..n).map(|_| rng.any_char() as u32).collect();
    cps.sort();
    cps.dedup();
    let mut bitmap: Vec<u8> = Vec::new();
    let mut recs = Vec::new();
    for &cp in &cps {
        let (w, h) = (rng.below(20) as u16, rng.below(5) as u16);
        let mut r = rawrec(cp, bitmap.len() as u32, w, h);
        for _ in 0..r.len {
            bitmap.push(rng.next_u32() as u8);
        }
        if rng.chance(6) {
            match rng.below(5) {
                0 => r.cp = rng.edgy_u32(),
                1 => r.off = rng.edgy_u32(),
                2 => r.len = rng.edgy_u32() % 64,
                3 => r.m.width = rng.edgy_u32() as u16,
                _ => r.m.height = rng.below(7) as u16,
            }
        }
        recs.push(r);
    }
    let pad = rng.below(4);
    for _ in 0..pad {
        bitmap.push(0);
    }
    raw_pack(&GOLDEN_INFO, &recs, &bitmap)
}

#[test]
fn random_packs_with_consistent_headers_agree_with_the_whole_file_parser() {
    let mut rng = Lcg::new(0xBADC0DE_5);
    let mut st = Stats::default();
    for _ in 0..5000 {
        let v = random_pack(&mut rng);
        let mut probes = probes_for(&mut rng, &v);
        probes.extend((0..10).map(|_| rng.any_char()));
        let fl = if rng.chance(8) {
            declared_len(&mut rng, &v)
        } else {
            v.len() as u64
        };
        exercise(&v, fl, &probes, &mut rng, &mut st);
        // whole-file oracle: header-valid packs open, whatever their records say
        if fl == v.len() as u64 {
            let (r, _) = open_with(v.clone(), fl);
            assert!(r.is_ok(), "consistent header must open");
        }
    }
    assert!(st.opened > 4000, "{st:?}");
    assert!(st.found > 1000, "{st:?}");
    assert!(st.record_err > 100, "{st:?}");
    assert!(st.absent > 1000, "{st:?}");
}

#[test]
fn random_headers_with_arbitrary_fields_never_panic() {
    let mut rng = Lcg::new(0x1234_5678_9ABC);
    let mut st = Stats::default();
    for _ in 0..4000 {
        let mut v = vec![0u8; 44 + rng.below(100) as usize];
        for b in &mut v {
            *b = rng.next_u32() as u8;
        }
        v[0..4].copy_from_slice(b"PFNT");
        put_u16(&mut v, H_VERSION, 1);
        if rng.chance(2) {
            // make the layout consistent for a random count so open succeeds more often
            let count = rng.below(5);
            put_u32(&mut v, H_COUNT, count);
            put_u32(&mut v, H_INDEX_OFFSET, 44);
            put_u32(&mut v, H_INDEX_LEN, 22 * count);
            put_u32(&mut v, H_BITMAP_OFFSET, 44 + 22 * count);
            let rest = (v.len() as u32).saturating_sub(44 + 22 * count);
            put_u32(&mut v, H_BITMAP_LEN, rest);
            put_u32(&mut v, H_TOTAL_LEN, 44 + 22 * count + rest);
        }
        let fl = if rng.chance(2) {
            get_u32(&v, H_TOTAL_LEN) as u64
        } else {
            v.len() as u64
        };
        let probes = probes_for(&mut rng, &v);
        exercise(&v, fl, &probes, &mut rng, &mut st);
    }
    assert!(st.header_err > 100, "{st:?}");
}

// ------------------------------------------------------------------- FontError

#[test]
fn font_error_display_is_non_empty_and_distinct_per_variant() {
    let all: Vec<(&str, FontError<SpyErr>)> = vec![
        ("Unsupported", FontError::Unsupported { found: 2 }),
        ("Corrupt", FontError::Corrupt(PackError::BadLayout)),
        ("Io", FontError::Io(SpyErr::Unavailable)),
        ("BufferTooSmall", FontError::BufferTooSmall { needed: 7 }),
    ];
    let texts: Vec<String> = all.iter().map(|(_, e)| format!("{e}")).collect();
    for ((name, _), t) in all.iter().zip(&texts) {
        assert!(!t.trim().is_empty(), "{name} has an empty Display text");
    }
    for i in 0..texts.len() {
        for j in i + 1..texts.len() {
            assert_ne!(
                texts[i], texts[j],
                "{} and {} share a Display text",
                all[i].0, all[j].0
            );
        }
    }
}

#[test]
fn font_error_is_copy_eq_and_debug() {
    let a: FontError<SpyErr> = FontError::BufferTooSmall { needed: 3 };
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(a, FontError::BufferTooSmall { needed: 4 });
    assert_ne!(
        FontError::<SpyErr>::Unsupported { found: 2 },
        FontError::Unsupported { found: 3 }
    );
    assert_ne!(
        FontError::<SpyErr>::Corrupt(PackError::BadMagic),
        FontError::Corrupt(PackError::BadLayout)
    );
    assert_ne!(
        FontError::<SpyErr>::Io(SpyErr::Injected(1)),
        FontError::Io(SpyErr::Injected(2))
    );
    assert!(!format!("{a:?}").is_empty());
}

// ------------------------------------------- GlyphRef from another (larger) pack

/// A large pack whose records are crafted against a 10-byte target region: only the index is
/// ever read from it, the bitmap region is virtual (declared u32::MAX bytes long file).
struct Donor {
    reader: PackReader<Spy>,
    /// (codepoint, bitmap_offset, bitmap_len)
    recs: Vec<(u32, u32, u32)>,
}

const TARGET_REGION: [u8; 10] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9];

fn donor() -> Donor {
    let region_len: u32 = u32::MAX - (44 + 22 * 14);
    let spec: [(u32, u32, u32); 14] = [
        (0x10, 0, 10),              // exactly the target region
        (0x20, 1, 10),              // ends 1 byte past it
        (0x30, 4, 10),              // starts inside, ends outside
        (0x40, 100, 10),            // completely outside
        (0x50, 9, 1),               // last byte of the region
        (0x60, 10, 1),              // starts at the end, 1 byte
        (0x70, 10, 0),              // blank at offset == region_len
        (0x80, 11, 0),              // blank 1 past
        (0x90, 1000, 0),            // blank far outside
        (0xA0, 0, 5),               // inside
        (0xB0, 100, 5),             // outside
        (0xC0, 6, 5),               // ends 1 byte past
        (0xD0, region_len - 1, 1),  // last byte of a ~4 GiB region
        (0xE0, 0x0100_0000 + 7, 8), // above 2^24
    ];
    let recs: Vec<RawRec> = spec
        .iter()
        .map(|&(cp, off, len)| {
            // 8 wide, so len = height; blank glyphs are 0 x 0
            if len == 0 {
                RawRec {
                    cp,
                    off,
                    len: 0,
                    m: m(3, 0, 0, 0, 0),
                }
            } else {
                rawrec(cp, off, 8, len as u16)
            }
        })
        .collect();
    let head = raw_pack_with_region(&GOLDEN_INFO, &recs, &[], region_len);
    let file_len = head.len() as u64 + region_len as u64;
    assert_eq!(file_len, u32::MAX as u64);
    let spy = Spy::sparse(file_len, move |off, buf| {
        for (j, b) in buf.iter_mut().enumerate() {
            let p = off + j as u64;
            *b = if (p as usize) < head.len() {
                head[p as usize]
            } else {
                0
            };
        }
    });
    Donor {
        reader: PackReader::open(spy, file_len).expect("donor opens"),
        recs: spec.to_vec(),
    }
}

/// 10-byte region, one record; header + 1 record = 66 bytes, so the region is [66, 76).
fn target() -> (Vec<u8>, PackReader<Spy>, Log) {
    let bytes = raw_pack(&GOLDEN_INFO, &[rawrec(0x41, 0, 8, 2)], &TARGET_REGION);
    assert_eq!(bytes.len(), 76);
    let spy = Spy::bytes(bytes.clone());
    let log = spy.log();
    (bytes, PackReader::open(spy, 76).expect("target opens"), log)
}

fn foreign(d: &mut Donor, cp: u32) -> pulp_fontpack::GlyphRef {
    d.reader
        .find(char::from_u32(cp).unwrap())
        .expect("donor find")
        .unwrap_or_else(|| panic!("donor lacks U+{cp:X}"))
}

#[test]
fn a_glyph_ref_from_a_larger_pack_is_read_only_when_its_range_lies_inside_this_region() {
    // target region_len = 10; rule: offset + len <= 10, else Corrupt(BadLayout) with zero reads
    let mut d = donor();
    let (_bytes, mut r, log) = target();
    let bad = Err(FontError::Corrupt(PackError::BadLayout));
    // (cp, offset, len, ok?)
    let mut seen_ok = 0;
    let mut seen_bad = 0;
    for (cp, off, len) in d.recs.clone() {
        let g = foreign(&mut d, cp);
        assert_eq!(g.bitmap_len, len);
        let inside = off as u64 + len as u64 <= 10;
        for buf_len in [0usize, len as usize, len as usize + 3, 64] {
            if buf_len < len as usize && inside {
                continue; // that is the too-small case, covered below
            }
            let before = read_count(&log);
            let mut buf = vec![0x5Cu8; buf_len];
            let res = r.read_bitmap(&g, &mut buf).map(|s| s.to_vec());
            let after = reads(&log)[before..].to_vec();
            if inside {
                seen_ok += 1;
                let want = &TARGET_REGION[off as usize..off as usize + len as usize];
                assert_eq!(res, Ok(want.to_vec()), "U+{cp:X} off {off} len {len}");
                if len == 0 {
                    assert!(after.is_empty(), "U+{cp:X}: a blank glyph costs no read");
                } else {
                    // header + 1 record = 66; region base 66
                    assert_eq!(after, [(66 + off as u64, len as usize)], "U+{cp:X}");
                }
                assert!(buf[len as usize..].iter().all(|&b| b == 0x5C));
            } else {
                seen_bad += 1;
                assert_eq!(res, bad, "U+{cp:X} off {off} len {len} buf {buf_len}");
                assert!(after.is_empty(), "U+{cp:X}: BadLayout must cost zero reads");
                assert!(buf.iter().all(|&b| b == 0x5C), "nothing written");
            }
        }
    }
    assert!(
        seen_ok > 10 && seen_bad > 20,
        "{seen_ok} ok, {seen_bad} bad"
    );
    // whatever happened, no request left the 76-byte file
    assert_reqs_within(&reads(&log), 0, 76, "target");
    // and the reader still works for its own glyph
    let g = r.find('A').unwrap().unwrap();
    let mut b = [0u8; 2];
    assert_eq!(r.read_bitmap(&g, &mut b).unwrap(), [0xA0, 0xA1]);
}

#[test]
fn a_foreign_range_outside_the_region_is_bad_layout_before_a_too_small_buffer_is_noticed() {
    // precedence: range outside the region -> BadLayout; range valid but buf too small -> BufferTooSmall
    let mut d = donor();
    let (_b, mut r, log) = target();
    // outside + buffer too small
    for cp in [0x20u32, 0x30, 0x40, 0xB0, 0xC0, 0xD0, 0xE0] {
        let g = foreign(&mut d, cp);
        assert!(g.bitmap_len >= 1);
        let mut buf = vec![0u8; g.bitmap_len as usize - 1];
        assert_eq!(
            r.read_bitmap(&g, &mut buf),
            Err(FontError::Corrupt(PackError::BadLayout)),
            "U+{cp:X}"
        );
        let mut empty: [u8; 0] = [];
        assert_eq!(
            r.read_bitmap(&g, &mut empty),
            Err(FontError::Corrupt(PackError::BadLayout)),
            "U+{cp:X} empty buffer"
        );
    }
    // inside + buffer too small: BufferTooSmall (range was fine)
    for (cp, len) in [(0x10u32, 10usize), (0xA0, 5), (0x50, 1)] {
        let g = foreign(&mut d, cp);
        let mut buf = vec![0u8; len - 1];
        assert_eq!(
            r.read_bitmap(&g, &mut buf),
            Err(FontError::BufferTooSmall { needed: len }),
            "U+{cp:X}"
        );
    }
    assert_eq!(
        read_count(&log),
        1,
        "only the header read of open: no bitmap read at all"
    );
}

#[test]
fn a_foreign_blank_glyph_is_valid_at_offset_equal_to_the_region_length_and_bad_layout_beyond() {
    let mut d = donor();
    let (_b, mut r, log) = target();
    let before = read_count(&log);
    // 0x70: blank at offset 10 == region_len 10 -> Ok(&[]), zero reads
    let g = foreign(&mut d, 0x70);
    assert_eq!(g.bitmap_len, 0);
    let mut buf = [0x77u8; 4];
    assert_eq!(r.read_bitmap(&g, &mut buf), Ok(&[][..]));
    assert_eq!(buf, [0x77; 4]);
    assert_eq!(read_count(&log), before);
    let mut none: [u8; 0] = [];
    assert_eq!(r.read_bitmap(&g, &mut none), Ok(&[][..]));
    // 0x80: offset 11 > 10; 0x90: offset 1000: BadLayout, zero reads
    for cp in [0x80u32, 0x90] {
        let g = foreign(&mut d, cp);
        assert_eq!(g.bitmap_len, 0);
        for len in [0usize, 4] {
            let mut b = vec![0u8; len];
            assert_eq!(
                r.read_bitmap(&g, &mut b),
                Err(FontError::Corrupt(PackError::BadLayout)),
                "U+{cp:X} buf {len}"
            );
        }
    }
    assert_eq!(read_count(&log), before, "no read for any of them");
}

#[test]
fn a_foreign_range_is_never_requested_from_the_card_even_at_the_u32_limits() {
    // 0xD0 sits in the last byte of a ~4 GiB region, 0xE0 above 2^24: against the 76-byte file the
    // reader must refuse before the card is touched (a request at offset ~4 GiB is outside [0, 76))
    let mut d = donor();
    let (_b, mut r, log) = target();
    let before = read_count(&log);
    for cp in [0xD0u32, 0xE0] {
        let g = foreign(&mut d, cp);
        let mut buf = vec![0u8; 64];
        assert_eq!(
            r.read_bitmap(&g, &mut buf),
            Err(FontError::Corrupt(PackError::BadLayout)),
            "U+{cp:X}"
        );
    }
    assert_eq!(read_count(&log), before);
    assert_reqs_within(&reads(&log), 0, 76, "target");
}

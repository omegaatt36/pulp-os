//! `PackReader::find_cached` (the index cache) against `PackReader::find`.
//!
//! The cache is an optimisation only: for every pack, good or damaged, and every
//! character, the answer (hit, absence or error) must be the one `find` gives.
//! Expectations come from `find` on a plain reader and from the builder input;
//! the read counts are the Spy log's.
mod common;
mod reader_support;

use common::*;
use pulp_fontpack::{
    FontError, FontInfo, GlyphRef, INDEX_RECORD_LEN, IndexCache, PackError, PackReader,
    SPAN_RECORDS,
};
use reader_support::*;

type Cache = IndexCache<Vec<u32>>;

fn cache(levels: u32) -> Cache {
    IndexCache::new(vec![0; Cache::nodes_for_levels(levels)])
}

fn info(font_id: u64, px: u16) -> FontInfo {
    FontInfo {
        font_id,
        pixel_size: px,
        ..GOLDEN_INFO
    }
}

/// `n` records, codepoints `base + step * k`, one-byte bitmaps; the advance is
/// unique per record so a hit names its record.
fn recs(n: u32, base: u32, step: u32) -> Vec<RawRec> {
    (0..n)
        .map(|k| {
            let mut r = rawrec(base + step * k, k, 8, 1);
            r.m.advance = k as u16 + 1;
            r
        })
        .collect()
}

fn pack(rs: &[RawRec]) -> Vec<u8> {
    pack_with(&GOLDEN_INFO, rs)
}

fn pack_with(info: &FontInfo, rs: &[RawRec]) -> Vec<u8> {
    raw_pack(info, rs, &vec![0xA5; rs.len()])
}

type Found = Result<Option<GlyphRef>, FontError<SpyErr>>;

fn plain(bytes: &[u8], c: char) -> Found {
    let (mut r, _) = open_spy(bytes);
    r.find(c)
}

/// Characters worth asking about in a pack: every record, its neighbours, and the extremes.
fn probes(rs: &[RawRec]) -> Vec<char> {
    let mut v = vec!['\0', '\u{1}', '\u{D7FF}', '\u{E000}', char::MAX];
    for r in rs {
        for cp in [r.cp.wrapping_sub(1), r.cp, r.cp.wrapping_add(1)] {
            v.extend(char::from_u32(cp));
        }
    }
    v.sort();
    v.dedup();
    v
}

const LEVELS: [u32; 7] = [0, 1, 2, 3, 6, 9, 10];

// -------------------------------------------------------------- equivalence

#[test]
fn large_pack_every_char_gives_the_answer_of_find_for_every_cache_size() {
    // 12665 records, the size of the real packs; gaps between codepoints so
    // absent characters sit between, before and after the records
    let rs = recs(12665, 0x100, 3);
    let bytes = pack(&rs);
    let (mut oracle, _) = open_spy(&bytes);
    let chars = probes(&rs);
    assert!(chars.len() > 3 * 12665);
    let expected: Vec<Found> = chars.iter().map(|&c| oracle.find(c)).collect();
    for levels in LEVELS {
        let mut c = cache(levels);
        let (mut r, _) = open_spy(&bytes);
        // twice: the first pass fills the cache as it goes, the second runs on it
        for pass in 0..2 {
            for (&ch, want) in chars.iter().zip(&expected) {
                assert_eq!(
                    &r.find_cached(ch, &mut c),
                    want,
                    "levels {levels}, pass {pass}, {:04X}",
                    ch as u32
                );
            }
        }
    }
}

#[test]
fn first_and_last_record_and_the_scalar_extremes() {
    let mut rs = recs(5000, 0x20, 7);
    rs[0].cp = 0;
    let last = rs.len() - 1;
    rs[last].cp = 0x10_FFFF;
    let bytes = pack(&rs);
    for levels in LEVELS {
        let mut c = cache(levels);
        let (mut r, _) = open_spy(&bytes);
        for ch in [
            '\0',
            '\u{1}',
            char::MAX,
            '\u{10FFFE}',
            '\u{20}',
            '\u{27}',
            '\u{D7FF}',
        ] {
            assert_eq!(
                r.find_cached(ch, &mut c),
                plain(&bytes, ch),
                "levels {levels} {ch:?}"
            );
        }
        let g = r.find_cached('\0', &mut c).unwrap().unwrap();
        assert_eq!(g.metrics.advance, 1);
        let g = r.find_cached(char::MAX, &mut c).unwrap().unwrap();
        assert_eq!(g.metrics.advance, 5000);
    }
}

#[test]
fn every_small_pack_size_around_the_span_and_cache_boundaries() {
    let s = SPAN_RECORDS as u32;
    let mut sizes: Vec<u32> = (0..=70).collect();
    sizes.extend([
        2 * s - 1,
        2 * s,
        2 * s + 1,
        4 * s,
        4 * s + 3,
        127,
        128,
        129,
        255,
        256,
        257,
    ]);
    for n in sizes {
        let rs = recs(n, 0x30, 2);
        let bytes = pack(&rs);
        let chars = probes(&rs);
        for levels in LEVELS {
            let mut c = cache(levels);
            let (mut r, _) = open_spy(&bytes);
            for &ch in &chars {
                assert_eq!(
                    r.find_cached(ch, &mut c),
                    plain(&bytes, ch),
                    "n {n}, levels {levels}, {:04X}",
                    ch as u32
                );
            }
        }
    }
}

#[test]
fn a_partial_level_caches_only_what_fits() {
    let rs = recs(3000, 0x100, 2);
    let bytes = pack(&rs);
    for words in [1usize, 2, 4, 5, 10, 100, 1000] {
        let mut c: Cache = IndexCache::new(vec![0; words]);
        let (mut r, _) = open_spy(&bytes);
        for &ch in &probes(&rs[..600]) {
            assert_eq!(
                r.find_cached(ch, &mut c),
                plain(&bytes, ch),
                "words {words}"
            );
        }
        assert!(c.filled() <= words);
    }
}

#[test]
fn a_disordered_index_gives_find_s_answers_through_the_cache() {
    let mut rng = Lcg::new(0xCAFE_0001);
    for _ in 0..40 {
        let n = 1 + rng.below(300);
        let mut rs: Vec<RawRec> = (0..n)
            .map(|k| {
                let mut r = rawrec(0x40 + rng.below(200), k, 8, 1);
                r.m.advance = k as u16 + 1;
                r
            })
            .collect();
        if rng.chance(2) {
            rs.sort_by_key(|r| std::cmp::Reverse(r.cp));
        }
        let bytes = pack(&rs);
        for levels in [0, 3, 6] {
            let mut c = cache(levels);
            let (mut r, _) = open_spy(&bytes);
            for ch in (0x30..0x120u32).filter_map(char::from_u32) {
                assert_eq!(r.find_cached(ch, &mut c), plain(&bytes, ch), "n {n} {ch:?}");
            }
        }
    }
}

// ------------------------------------------------------------- read counting

fn requests(log: &Log) -> Vec<(u64, usize)> {
    reads(log)
}

/// 512 byte blocks a card would fetch for these requests with a one block
/// cache (the embedded-sdmmc behaviour); file offsets stand in for card blocks.
fn block_fetches(reqs: &[(u64, usize)]) -> usize {
    let mut last = None;
    let mut n = 0;
    for &(off, len) in reqs {
        if len == 0 {
            continue;
        }
        for b in off / 512..=(off + len as u64 - 1) / 512 {
            if last != Some(b) {
                n += 1;
                last = Some(b);
            }
        }
    }
    n
}

#[test]
fn warm_lookups_take_at_most_two_reads_each_and_the_old_path_takes_fourteen() {
    let rs = recs(12665, 0x100, 3);
    let bytes = pack(&rs);
    let mut worst_old = 0;
    let mut old_total = 0;
    let mut old_blocks = 0;
    let n = rs.len();
    let sample: Vec<char> = (0..n)
        .step_by(7)
        .map(|k| char::from_u32(rs[k].cp).unwrap())
        .collect();
    for &ch in &sample {
        let (mut r, log) = open_spy(&bytes);
        let before = read_count(&log);
        r.find(ch).unwrap();
        let l = requests(&log);
        worst_old = worst_old.max(l.len() - before);
        old_total += l.len() - before;
        old_blocks += block_fetches(&l[before..]);
    }
    println!(
        "old path: {} lookups, avg {:.2} reads, worst {worst_old}, avg {:.2} block fetches",
        sample.len(),
        old_total as f64 / sample.len() as f64,
        old_blocks as f64 / sample.len() as f64
    );
    for levels in [8u32, 9, 10] {
        let mut c = cache(levels);
        let (mut r, log) = open_spy(&bytes);
        // warm up: look every record up once, which reads every cache node
        for rec in &rs {
            r.find_cached(char::from_u32(rec.cp).unwrap(), &mut c)
                .unwrap();
        }
        assert_eq!(c.filled(), c.capacity().min(n));
        let (mut worst, mut total, mut blocks, mut spans) = (0, 0, 0, 0);
        let mut longest = 0;
        for &ch in &sample {
            let before = read_count(&log);
            r.find_cached(ch, &mut c).unwrap();
            let l = requests(&log);
            let mine = &l[before..];
            worst = worst.max(mine.len());
            total += mine.len();
            blocks += block_fetches(mine);
            spans += usize::from(mine.iter().any(|&(_, len)| len > INDEX_RECORD_LEN));
            longest = longest.max(mine.iter().map(|&(_, len)| len).max().unwrap_or(0));
        }
        println!(
            "levels {levels}: {} warm lookups, avg {:.2} reads, worst {worst}, avg {:.2} block fetches, longest read {longest} B, {spans} span reads",
            sample.len(),
            total as f64 / sample.len() as f64,
            blocks as f64 / sample.len() as f64
        );
        // 12665 records: a stretch of 50 below 8 levels does not fit the span
        // buffer and takes a probe first; from 9 levels on the stretch (25)
        // is read whole
        assert!(
            worst <= if levels >= 9 { 1 } else { 2 },
            "levels {levels}: worst {worst} reads"
        );
        assert!(
            worst_old >= 13,
            "the old path should need about 14 reads, not {worst_old}"
        );
        // a miss (absent char) reads the same span
        let before = read_count(&log);
        assert_eq!(r.find_cached('\u{101}', &mut c), Ok(None));
        assert!(read_count(&log) - before <= if levels >= 9 { 1 } else { 2 });
    }
}

#[test]
fn a_cold_cache_never_costs_more_reads_than_find_and_fills_as_it_goes() {
    let rs = recs(12665, 0x100, 3);
    let bytes = pack(&rs);
    let mut rng = Lcg::new(77);
    let mut c = cache(9);
    let (mut r, log) = open_spy(&bytes);
    let mut chosen: Vec<u32> = (0..250).map(|_| rs[rng.below(12665) as usize].cp).collect();
    chosen.sort();
    chosen.dedup();
    let (mut first_ten, mut rest, mut rest_n) = (0, 0, 0);
    for (i, &cp) in chosen.iter().enumerate() {
        let before = read_count(&log);
        r.find_cached(char::from_u32(cp).unwrap(), &mut c).unwrap();
        let used = read_count(&log) - before;
        assert!(
            used <= max_probes(12665) + 1,
            "lookup {i} used {used} reads"
        );
        if i < 10 {
            first_ten += used;
        } else {
            rest += used;
            rest_n += 1;
        }
    }
    println!(
        "cold, {} ascending scalars: first ten {first_ten} reads, then avg {:.2} reads/lookup",
        chosen.len(),
        rest as f64 / rest_n as f64
    );
    assert!(rest as f64 / (rest_n as f64) < 6.0);
}

#[test]
fn a_cached_hit_in_the_top_levels_reads_that_record_once() {
    let rs = recs(1000, 0x100, 2);
    let bytes = pack(&rs);
    let mut c = cache(4);
    let (mut r, log) = open_spy(&bytes);
    let root = char::from_u32(rs[500].cp).unwrap();
    r.find_cached(root, &mut c).unwrap(); // fills the root
    let before = read_count(&log);
    let g = r.find_cached(root, &mut c).unwrap().unwrap();
    assert_eq!(g.metrics.advance, 501);
    assert_eq!(requests(&log)[before..], [(44 + 22 * 500, 22)]);
}

#[test]
fn an_absent_char_between_cached_probes_needs_no_extra_read_when_warm() {
    let rs = recs(64, 0x100, 2); // 64 records: with 6 levels (63 nodes) one more level of span
    let bytes = pack(&rs);
    let mut c = cache(6);
    let (mut r, log) = open_spy(&bytes);
    for rec in &rs {
        r.find_cached(char::from_u32(rec.cp).unwrap(), &mut c)
            .unwrap();
    }
    let before = read_count(&log);
    assert_eq!(r.find_cached('\u{101}', &mut c), Ok(None));
    assert!(read_count(&log) - before <= 1);
}

// ------------------------------------------------------------------ damage

type Damage = (&'static str, fn(&mut RawRec), fn(u32) -> PackError);

fn damages() -> Vec<Damage> {
    fn inv(index: u32) -> PackError {
        PackError::InvalidCodepoint { index }
    }
    fn rge(index: u32) -> PackError {
        PackError::GlyphBitmapRange { index }
    }
    fn siz(index: u32) -> PackError {
        PackError::GlyphSizeMismatch { index }
    }
    vec![
        ("cp surrogate", |r| r.cp = 0xD800, inv),
        ("cp above unicode", |r| r.cp = 0x11_0000, inv),
        ("range far", |r| r.off = 100_000, rge),
        ("range overflow", |r| r.off = u32::MAX, rge),
        (
            "size len 3",
            |r| {
                r.off = 0;
                r.len = 3
            },
            siz,
        ),
        (
            "size width 9",
            |r| {
                r.off = 0;
                r.m.width = 9
            },
            siz,
        ),
    ]
}

#[test]
fn a_damaged_record_anywhere_fails_like_find_in_the_levels_and_in_the_span() {
    // 100 records: a root and a few levels in the cache, the rest in the span;
    // 200 with two levels: the stretch below the cache is longer than the span
    // buffer and is halved by probes first
    for (n, levels) in [(100u32, 3u32), (100, 0), (200, 2), (37, 6)] {
        let clean = recs(n, 0x100, 2);
        for (name, damage, want) in damages() {
            for j in 0..n as usize {
                let mut rs = clean.clone();
                damage(&mut rs[j]);
                let bytes = pack(&rs);
                // one cache for every probe: a damaged record is never cached
                let mut c = cache(levels);
                let (mut r, _) = open_spy(&bytes);
                for ch in probes(&clean) {
                    let got = r.find_cached(ch, &mut c);
                    let old = plain(&bytes, ch);
                    assert_eq!(
                        got, old,
                        "{name}, record {j}, n {n}, levels {levels}, {:04X}",
                        ch as u32
                    );
                    if let Err(e) = got {
                        assert_eq!(e, FontError::Corrupt(want(j as u32)));
                    }
                }
            }
        }
    }
}

#[test]
fn a_damaged_record_in_the_cached_levels_is_rejected_again_on_every_lookup() {
    let n = 100u32;
    let clean = recs(n, 0x100, 2);
    let mut rs = clean.clone();
    rs[50].cp = 0xD800; // the root
    let bytes = pack(&rs);
    let mut c = cache(3);
    let (mut r, _) = open_spy(&bytes);
    for _ in 0..3 {
        for rec in [&clean[0], &clean[49], &clean[51], &clean[99]] {
            let ch = char::from_u32(rec.cp).unwrap();
            assert_eq!(
                r.find_cached(ch, &mut c),
                Err(FontError::Corrupt(PackError::InvalidCodepoint {
                    index: 50
                }))
            );
        }
    }
    assert_eq!(c.filled(), 0, "a rejected record must not enter the cache");
}

#[test]
fn a_record_that_is_never_probed_is_not_judged_by_the_cache_either() {
    let clean = recs(100, 0x100, 2);
    let mut rs = clean.clone();
    rs[0].cp = 0xD800; // only a search that goes to the very first record sees it
    let bytes = pack(&rs);
    let mut c = cache(3);
    let (mut r, _) = open_spy(&bytes);
    let g = r.find_cached(char::from_u32(clean[99].cp).unwrap(), &mut c);
    assert_eq!(g, plain(&bytes, char::from_u32(clean[99].cp).unwrap()));
    assert!(matches!(g, Ok(Some(_))));
}

// -------------------------------------------------------------- invalidation

#[test]
fn a_cache_never_answers_for_another_pack() {
    // same size, same record count, same bitmap length; only the font id,
    // the pixel size or the codepoints differ
    let a = pack_with(&info(1, 16), &recs(500, 0x100, 2));
    let same_id_other_codes = pack_with(&info(1, 16), &recs(500, 0x180, 3));
    let other_id = pack_with(&info(2, 16), &recs(500, 0x180, 3));
    let other_px = pack_with(&info(1, 19), &recs(500, 0x180, 3));
    let other_count = pack_with(&info(1, 16), &recs(501, 0x180, 3));
    for (name, b) in [
        ("font id", &other_id),
        ("pixel size", &other_px),
        ("record count", &other_count),
    ] {
        let mut c = cache(6);
        let (mut ra, _) = open_spy(&a);
        let (mut rb, _) = open_spy(b);
        for k in 0..500u32 {
            ra.find_cached(char::from_u32(0x100 + 2 * k).unwrap(), &mut c)
                .unwrap();
        }
        assert!(c.filled() > 50);
        for k in 0..500u32 {
            for cp in [0x180 + 3 * k, 0x180 + 3 * k + 1, 0x100 + 2 * k] {
                let ch = char::from_u32(cp).unwrap();
                assert_eq!(rb.find_cached(ch, &mut c), plain(b, ch), "{name} {cp:X}");
            }
        }
        // and back
        for cp in [0x100u32, 0x101, 0x102, 0x3FE] {
            let ch = char::from_u32(cp).unwrap();
            assert_eq!(
                ra.find_cached(ch, &mut c),
                plain(&a, ch),
                "{name} back {cp:X}"
            );
        }
    }
    // not covered by the key by design: the same ids and sizes with other
    // codepoints are the same pack as far as a cache can tell; the firmware
    // clears its caches when the book or the pack identity changes
    let _ = same_id_other_codes;
}

#[test]
fn invalidate_empties_the_cache() {
    let bytes = pack(&recs(500, 0x100, 2));
    let mut c = cache(6);
    let (mut r, log) = open_spy(&bytes);
    for k in 0..200u32 {
        r.find_cached(char::from_u32(0x100 + 2 * k).unwrap(), &mut c)
            .unwrap();
    }
    assert!(c.filled() > 5);
    let warm = {
        let before = read_count(&log);
        r.find_cached('\u{100}', &mut c).unwrap();
        read_count(&log) - before
    };
    c.invalidate();
    assert_eq!(c.filled(), 0);
    let cold = {
        let before = read_count(&log);
        r.find_cached('\u{100}', &mut c).unwrap();
        read_count(&log) - before
    };
    assert!(cold > warm, "cold {cold}, warm {warm}");
}

// ------------------------------------------------------------ I/O injection

#[test]
fn a_failed_read_is_io_and_leaves_the_cache_usable() {
    let rs = recs(300, 0x100, 2);
    let bytes = pack(&rs);
    let ch = char::from_u32(rs[123].cp).unwrap();
    let total = {
        let (mut r, log) = open_spy(&bytes);
        let before = read_count(&log);
        r.find_cached(ch, &mut cache(5)).unwrap();
        read_count(&log) - before
    };
    assert!(total >= 2);
    for k in 1..=total {
        // reads of this lookup are numbered after open's one read
        let spy = Spy::bytes(bytes.clone()).failing_on(1 + k);
        let mut r = PackReader::open(spy, bytes.len() as u64).unwrap();
        let mut c = cache(5);
        assert_eq!(
            r.find_cached(ch, &mut c),
            Err(FontError::Io(SpyErr::Injected(1 + k))),
            "read {k}"
        );
        // the source works again (only the k-th read fails): same answer as ever
        assert_eq!(
            r.find_cached(ch, &mut c),
            plain(&bytes, ch),
            "retry after {k}"
        );
    }
}

#[test]
fn glyph_bitmaps_read_through_a_cached_lookup_are_the_ones_of_the_plain_lookup() {
    use pulp_fontpack::{GlyphEntry, Metrics, build_pack};
    let entries: Vec<GlyphEntry> = (0..2000u32)
        .map(|k| {
            let (w, h) = (1 + (k % 20) as u16, 1 + (k % 7) as u16);
            GlyphEntry {
                codepoint: char::from_u32(0x4E00 + k).unwrap(),
                metrics: Metrics {
                    advance: w + 1,
                    offset_x: (k % 3) as i16 - 1,
                    offset_y: -((k % 5) as i16),
                    width: w,
                    height: h,
                },
                bitmap: (0..pulp_fontpack::bitmap_size(w, h))
                    .map(|i| (k + i) as u8)
                    .collect(),
            }
        })
        .collect();
    let bytes = build_pack(&GOLDEN_INFO, &entries).unwrap();
    let mut c = cache(8);
    let (mut r, _) = open_spy(&bytes);
    let (mut p, _) = open_spy(&bytes);
    for e in &entries {
        let g = r.find_cached(e.codepoint, &mut c).unwrap().unwrap();
        let mut buf = vec![0; g.bitmap_len as usize];
        assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), &e.bitmap[..]);
        assert_eq!(Some(g), p.find(e.codepoint).unwrap());
    }
}

#[test]
fn a_cold_page_of_scalars_fetches_a_fifth_of_the_blocks_of_the_plain_search() {
    // 250 and 700 distinct ascending scalars from the common part of a 12665
    // record pack, the cache cold at the start; blocks as a one block cache
    // card fetches them (model, see `block_fetches`)
    let rs = recs(12665, 0x100, 3);
    let bytes = pack(&rs);
    for (n, levels, factor) in [(250usize, 10u32, 5usize), (700, 10, 7), (700, 9, 4)] {
        let mut rng = Lcg::new(5);
        let mut chosen: Vec<u32> = (0..2 * n)
            .map(|_| rs[rng.below(5000) as usize].cp)
            .collect();
        chosen.sort();
        chosen.dedup();
        chosen.truncate(n);
        let mut c = cache(levels);
        let (mut r, log) = open_spy(&bytes);
        let (mut p, plain_log) = open_spy(&bytes);
        for &cp in &chosen {
            let ch = char::from_u32(cp).unwrap();
            assert_eq!(r.find_cached(ch, &mut c), p.find(ch));
        }
        let (cached, plain) = (
            block_fetches(&requests(&log)[1..]),
            block_fetches(&requests(&plain_log)[1..]),
        );
        println!(
            "{} scalars, {levels} levels: {} reads {cached} blocks ({:.2}/lookup); plain {} reads {plain} blocks ({:.2}/lookup)",
            chosen.len(),
            read_count(&log) - 1,
            cached as f64 / chosen.len() as f64,
            read_count(&plain_log) - 1,
            plain as f64 / chosen.len() as f64
        );
        assert!(
            cached * factor <= plain,
            "{n} scalars, {levels} levels: {cached} vs {plain}"
        );
    }
}

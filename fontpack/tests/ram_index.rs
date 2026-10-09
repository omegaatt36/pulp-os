//! `PackReader::load_index` / `find_in_ram` (the whole index in memory) against
//! `PackReader::find`.
//!
//! The RAM index is an optimisation only: for a good pack and every character
//! the answer must be the one `find` gives, with no read after the load. A pack
//! it cannot vouch for (damaged, other pack, storage too small, failed read)
//! must never be answered from memory. Expectations come from `find` on a plain
//! reader and from the builder input; read counts are the Spy log's.
mod common;
mod reader_support;

use common::*;
use pulp_fontpack::{
    FontError, FontInfo, GlyphRef, HEADER_LEN, INDEX_RECORD_LEN, PackError, PackReader, RamIndex,
};
use reader_support::*;

type Ram = RamIndex<Vec<u8>>;

fn ram_for(n: u32) -> Ram {
    RamIndex::new(vec![0; Ram::bytes_for(n).unwrap()])
}

fn recs(n: u32, base: u32, step: u32) -> Vec<RawRec> {
    (0..n)
        .map(|k| {
            let mut r = rawrec(base + step * k, k, 8, 1);
            r.m.advance = k as u16 + 1;
            r
        })
        .collect()
}

fn pack_with(info: &FontInfo, rs: &[RawRec]) -> Vec<u8> {
    raw_pack(info, rs, &vec![0xA5; rs.len()])
}

fn pack(rs: &[RawRec]) -> Vec<u8> {
    pack_with(&GOLDEN_INFO, rs)
}

fn plain(bytes: &[u8], c: char) -> Option<GlyphRef> {
    let (mut r, _) = open_spy(bytes);
    r.find(c).expect("good pack")
}

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

#[test]
fn large_pack_every_char_gives_the_answer_of_find() {
    // 12665 records, the size of the real packs
    let rs = recs(12665, 0x100, 3);
    let bytes = pack(&rs);
    let (mut r, _) = open_spy(&bytes);
    let mut ram = ram_for(12665);
    r.load_index(&mut ram).expect("good pack loads");
    assert!(ram.is_loaded());
    for c in probes(&rs) {
        assert_eq!(r.find_in_ram(c, &ram), Some(plain(&bytes, c)), "{c:?}");
    }
}

#[test]
fn small_packs_of_every_size_up_to_a_few_levels_agree_with_find() {
    for n in 1..=70u32 {
        let rs = recs(n, 0x4E00, 2);
        let bytes = pack(&rs);
        let (mut r, _) = open_spy(&bytes);
        let mut ram = ram_for(n);
        r.load_index(&mut ram).unwrap();
        for c in probes(&rs) {
            assert_eq!(
                r.find_in_ram(c, &ram),
                Some(plain(&bytes, c)),
                "n={n} {c:?}"
            );
        }
    }
}

#[test]
fn load_is_sequential_chunked_reads_and_lookups_read_nothing() {
    let n = 12665u32;
    let rs = recs(n, 0x100, 3);
    let bytes = pack(&rs);
    let (mut r, log) = open_spy(&bytes);
    let opened = log.borrow().len();
    let mut ram = ram_for(n);
    r.load_index(&mut ram).unwrap();
    let total = n as usize * INDEX_RECORD_LEN;
    let loads: Vec<(u64, usize)> = log.borrow()[opened..].to_vec();
    assert_eq!(loads.iter().map(|&(_, l)| l).sum::<usize>(), total);
    assert!(loads.iter().all(|&(_, l)| l <= 4096));
    assert_eq!(loads.len(), total.div_ceil(4096));
    // contiguous and ascending from the first record
    let mut at = HEADER_LEN as u64;
    for &(off, len) in &loads {
        assert_eq!(off, at);
        at += len as u64;
    }
    let after_load = log.borrow().len();
    for c in probes(&rs) {
        let _ = r.find_in_ram(c, &ram);
    }
    assert_eq!(log.borrow().len(), after_load, "a lookup read the pack");
}

#[test]
fn a_hit_carries_the_glyph_ref_of_its_record() {
    let rs = recs(100, 0x4E00, 1);
    let bytes = pack(&rs);
    let (mut r, _) = open_spy(&bytes);
    let mut ram = ram_for(100);
    r.load_index(&mut ram).unwrap();
    let hit = r
        .find_in_ram(char::from_u32(0x4E00 + 41).unwrap(), &ram)
        .unwrap()
        .expect("present");
    assert_eq!(hit.metrics.advance, 42);
    assert_eq!(hit.bitmap_offset(), 41);
}

#[test]
fn unloaded_index_never_answers() {
    let rs = recs(10, 0x100, 1);
    let bytes = pack(&rs);
    let (r, _) = open_spy(&bytes);
    let ram = ram_for(10);
    assert!(!ram.is_loaded());
    assert_eq!(r.find_in_ram('\u{100}', &ram), None);
}

#[test]
fn storage_too_small_is_refused_and_leaves_the_index_unloaded() {
    let rs = recs(50, 0x100, 1);
    let bytes = pack(&rs);
    let (mut r, _) = open_spy(&bytes);
    let mut ram = RamIndex::new(vec![0u8; 50 * INDEX_RECORD_LEN - 1]);
    assert_eq!(
        r.load_index(&mut ram),
        Err(FontError::BufferTooSmall {
            needed: 50 * INDEX_RECORD_LEN
        })
    );
    assert!(!ram.is_loaded());
    assert_eq!(r.find_in_ram('\u{100}', &ram), None);
}

#[test]
fn a_failed_read_leaves_the_index_unloaded() {
    let rs = recs(2000, 0x100, 1);
    let bytes = pack(&rs);
    let spy = Spy::bytes(bytes.clone()).failing_on(2);
    let mut r = PackReader::open(spy, bytes.len() as u64).unwrap();
    let mut ram = ram_for(2000);
    // read 1 is the header (already served by open), so the 2nd read counted
    // from now is the 2nd chunk
    let err = r.load_index(&mut ram).unwrap_err();
    assert!(matches!(err, FontError::Io(_)), "{err:?}");
    assert!(!ram.is_loaded());
    assert_eq!(r.find_in_ram('\u{100}', &ram), None);
}

#[test]
fn a_damaged_record_anywhere_refuses_the_whole_index() {
    let rs = recs(300, 0x100, 1);
    let mut bytes = pack(&rs);
    // record 217: bitmap offset far outside the bitmap region
    let at = HEADER_LEN + 217 * INDEX_RECORD_LEN + 4;
    bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let (mut r, _) = open_spy(&bytes);
    let mut ram = ram_for(300);
    assert_eq!(
        r.load_index(&mut ram),
        Err(FontError::Corrupt(PackError::GlyphBitmapRange {
            index: 217
        }))
    );
    assert!(!ram.is_loaded());
    assert_eq!(r.find_in_ram('\u{100}', &ram), None);
}

#[test]
fn out_of_order_codepoints_refuse_the_index() {
    let rs = recs(100, 0x100, 1);
    let mut bytes = pack(&rs);
    // record 60 repeats record 59's codepoint
    let (a, b) = (
        HEADER_LEN + 59 * INDEX_RECORD_LEN,
        HEADER_LEN + 60 * INDEX_RECORD_LEN,
    );
    let cp: [u8; 4] = bytes[a..a + 4].try_into().unwrap();
    bytes[b..b + 4].copy_from_slice(&cp);
    let (mut r, _) = open_spy(&bytes);
    let mut ram = ram_for(100);
    assert_eq!(
        r.load_index(&mut ram),
        Err(FontError::Corrupt(PackError::CodepointOrder { index: 60 }))
    );
    assert!(!ram.is_loaded());
}

#[test]
fn an_index_loaded_for_one_pack_does_not_answer_for_another() {
    let rs = recs(64, 0x100, 1);
    let a = pack_with(
        &FontInfo {
            font_id: 1,
            ..GOLDEN_INFO
        },
        &rs,
    );
    let b = pack_with(
        &FontInfo {
            font_id: 2,
            ..GOLDEN_INFO
        },
        &rs,
    );
    let (mut ra, _) = open_spy(&a);
    let (rb, _) = open_spy(&b);
    let mut ram = ram_for(64);
    ra.load_index(&mut ram).unwrap();
    assert!(ra.find_in_ram('\u{100}', &ram).is_some());
    assert_eq!(rb.find_in_ram('\u{100}', &ram), None);
    // other pixel size, same id
    let c = pack_with(
        &FontInfo {
            font_id: 1,
            pixel_size: GOLDEN_INFO.pixel_size + 1,
            ..GOLDEN_INFO
        },
        &rs,
    );
    let (rc, _) = open_spy(&c);
    assert_eq!(rc.find_in_ram('\u{100}', &ram), None);
}

#[test]
fn a_reload_after_a_failure_recovers() {
    let rs = recs(300, 0x100, 1);
    let good = pack(&rs);
    let mut bad = good.clone();
    let at = HEADER_LEN + 5 * INDEX_RECORD_LEN + 4;
    bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut ram = ram_for(300);
    let (mut rbad, _) = open_spy(&bad);
    assert!(rbad.load_index(&mut ram).is_err());
    let (mut rgood, _) = open_spy(&good);
    rgood.load_index(&mut ram).unwrap();
    assert!(rgood.find_in_ram('\u{100}', &ram).is_some());
    ram.invalidate();
    assert_eq!(rgood.find_in_ram('\u{100}', &ram), None);
}

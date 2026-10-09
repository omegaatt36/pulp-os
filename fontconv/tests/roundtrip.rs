//! Converter -> `PackReader` round trip: code points and bitmap offsets above
//! 16 bits, damage and I/O failure on real converter output, fallback for a
//! char the font lacks, and each pixel size is served from its own pack.
//!
//! Expectations come only from the independent fontdue oracle in
//! `common/mod.rs`, from the written font_id formula (font_id.rs), from literals
//! derived by hand from the format rules, from `Pack::parse(..).glyph_at` and,
//! for the damage tests, from `Record::decode/validate` applied to
//! the bytes the reader was actually handed (verified in fontpack's
//! header_record.rs). Nothing is copied from converter or reader output.
//!
//! Real-Iansui tests skip loudly (`SKIPPED(no-iansui-font): <test>`) without the
//! font file; `IANSUI_REQUIRED=1` turns that into a failure.
#[macro_use]
mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

use common::*;
use pulp_fontconv::Output;
use pulp_fontpack::{
    FontError, Metrics, Pack, PackError, PackReader, ReadAt, Record, missing_glyph_metrics,
    render_missing_glyph,
};
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------- spy source

/// Every request seen by a `Spy`: `(offset, len)`, in order.
type Log = Rc<RefCell<Vec<(u64, usize)>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpyErr {
    /// The k-th read (1-based, counted from the creation of the spy) failed on purpose.
    Injected(usize),
    /// The request does not lie inside the bytes the spy has.
    OutOfRange,
}

impl fmt::Display for SpyErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpyErr::Injected(k) => write!(f, "injected failure on read {k}"),
            SpyErr::OutOfRange => write!(f, "request outside the card"),
        }
    }
}

/// A `ReadAt` over a byte slice that logs every request (before anything else),
/// can fail the k-th one, and can flip bits of one absolute byte on its way out
/// (the card returning damaged data).
struct Spy<'a> {
    data: &'a [u8],
    log: Log,
    calls: usize,
    fail_on: Option<usize>,
    flip: Option<(u64, u8)>,
}

impl<'a> Spy<'a> {
    fn new(data: &'a [u8]) -> Spy<'a> {
        Spy {
            data,
            log: Rc::new(RefCell::new(Vec::new())),
            calls: 0,
            fail_on: None,
            flip: None,
        }
    }
    fn failing_on(mut self, k: usize) -> Spy<'a> {
        self.fail_on = Some(k);
        self
    }
    fn flipping(mut self, at: u64, xor: u8) -> Spy<'a> {
        self.flip = Some((at, xor));
        self
    }
    fn log(&self) -> Log {
        self.log.clone()
    }
}

impl ReadAt for Spy<'_> {
    type Error = SpyErr;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), SpyErr> {
        self.log.borrow_mut().push((offset, buf.len()));
        self.calls += 1;
        if self.fail_on == Some(self.calls) {
            return Err(SpyErr::Injected(self.calls));
        }
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(SpyErr::OutOfRange)?;
        if end > self.data.len() as u64 {
            return Err(SpyErr::OutOfRange);
        }
        buf.copy_from_slice(&self.data[offset as usize..end as usize]);
        if let Some((p, x)) = self.flip {
            if p >= offset && p < end {
                buf[(p - offset) as usize] ^= x;
            }
        }
        Ok(())
    }
}

fn reqs(log: &Log) -> Vec<(u64, usize)> {
    log.borrow().clone()
}

fn clear(log: &Log) {
    log.borrow_mut().clear();
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

/// Worst-case probe count of a binary search over `n` records: ceil(log2(n+1)),
/// i.e. the bit length of `n`.
fn max_probes(n: u32) -> usize {
    let k = (32 - n.leading_zeros()) as usize;
    // self-check of the formula: 2^(k-1) <= n < 2^k  <=>  k = ceil(log2(n+1))
    assert!(n == 0 && k == 0 || (1u64 << (k - 1)) <= n as u64 && (n as u64) < (1u64 << k));
    k
}

/// The largest scalar below / the smallest scalar above `c` (surrogate block skipped).
fn prev_char(c: char) -> Option<char> {
    let mut v = c as u32;
    loop {
        v = v.checked_sub(1)?;
        if let Some(p) = char::from_u32(v) {
            return Some(p);
        }
    }
}

fn next_char(c: char) -> Option<char> {
    let mut v = c as u32;
    loop {
        v += 1;
        if v > 0x10FFFF {
            return None;
        }
        if let Some(n) = char::from_u32(v) {
            return Some(n);
        }
    }
}

/// font_id as written in font_id.rs: the first 8 bytes, little
/// endian, of SHA-256(font_sha256 || pixel_size u16 LE || convention u32 LE = 1).
fn font_id_by_formula(font_sha: &[u8; 32], px: u16) -> u64 {
    let mut h = Sha256::new();
    h.update(font_sha);
    h.update(px.to_le_bytes());
    h.update(1u32.to_le_bytes());
    u64::from_le_bytes(h.finalize()[..8].try_into().unwrap())
}

// --------------------------------------------------------------- shared fixtures

struct Real {
    font: fontdue::Font,
    sha: [u8; 32],
    out: Output,
    /// cmap minus control characters, ascending: the converter's included set.
    included: Vec<char>,
    included_set: BTreeSet<char>,
}

fn build_real(bytes: &[u8]) -> Real {
    let font = load_font(bytes);
    let included = oracle_included(&font);
    let included_set = included.iter().copied().collect();
    Real {
        sha: sha256(bytes),
        out: convert_with(bytes, &DEFAULT_SIZES, None),
        font,
        included,
        included_set,
    }
}

fn bookerly() -> &'static Real {
    static R: OnceLock<Real> = OnceLock::new();
    R.get_or_init(|| build_real(&bookerly_bytes()))
}

fn iansui(path: &Path) -> &'static Real {
    static R: OnceLock<Real> = OnceLock::new();
    R.get_or_init(|| build_real(&std::fs::read(path).unwrap()))
}

fn open_spy(bytes: &[u8]) -> (PackReader<Spy<'_>>, Log) {
    let spy = Spy::new(bytes);
    let log = spy.log();
    let r = PackReader::open(spy, bytes.len() as u64).expect("converter output must open");
    (r, log)
}

// ---------------------------------------------------- full cmap, every glyph, one size

/// Facts about the pack used by the large-region assertions.
struct Facts {
    region: u64,
    /// glyphs (non-blank) whose bitmap starts above 65,535 in the region
    over_u16: usize,
    /// glyphs whose bitmap starts above 2^20
    over_1mib: usize,
    /// chars above U+FFFF that were found
    astral: usize,
}

/// The whole pack for `px`, checked glyph by glyph against the oracle:
/// header/info, find + read_bitmap bytes, the hand-derived bitmap address (the
/// writer emits bitmaps back to back in index order: the format rules), the read
/// budget, agreement with `Pack::glyph_at`, and exact absence of every neighbour.
fn check_pack(real: &Real, px: u16) -> Facts {
    let bytes = pack_bytes(&real.out, px);
    let file_len = bytes.len() as u64;
    let whole = Pack::parse(bytes).expect("converter output parses");
    let (mut r, log) = open_spy(bytes);
    assert_eq!(
        reqs(&log),
        [(0u64, 44usize)],
        "open = one 44-byte read at 0"
    );

    // The pack reports the requested size with the oracle's line metrics.
    let info = r.info();
    let (line_height, ascent) = oracle_line_metrics(&real.font, px);
    assert_eq!(info.pixel_size, px);
    assert_eq!(
        (info.line_height, info.ascent),
        (line_height, ascent),
        "{px}px"
    );
    assert_eq!(
        info.font_id,
        font_id_by_formula(&real.sha, px),
        "{px}px font_id"
    );
    assert_eq!(r.header(), whole.header(), "header vs Pack::parse");
    assert_eq!(r.header().info, info);
    assert_eq!(r.header().glyph_count as usize, real.included.len());

    let n = r.header().glyph_count;
    let bound = max_probes(n);
    let base = 44 + 22 * n as u64;
    let mut rel = 0u64;
    let mut facts = Facts {
        region: 0,
        over_u16: 0,
        over_1mib: 0,
        astral: 0,
    };
    for (i, &c) in real.included.iter().enumerate() {
        let want = oracle_glyph(&real.font, c, px);
        let ctx = format!("U+{:04X} at {px}px", c as u32);

        clear(&log);
        let g = r
            .find(c)
            .unwrap_or_else(|e| panic!("{ctx}: {e:?}"))
            .unwrap_or_else(|| panic!("{ctx}: not found"));
        let probes = reqs(&log);
        assert!(
            !probes.is_empty() && probes.len() <= bound,
            "{ctx}: {} probes (bound {bound})",
            probes.len()
        );
        for &(o, len) in &probes {
            assert_eq!(len, 22, "{ctx}: probe size");
            assert!(
                o >= 44 && (o - 44) % 22 == 0 && (o - 44) / 22 < n as u64,
                "{ctx}: probe at {o} is not an index record"
            );
        }
        assert_eq!(
            g.metrics,
            Metrics {
                advance: want.advance,
                offset_x: want.offset_x,
                offset_y: want.offset_y,
                width: want.width,
                height: want.height
            },
            "{ctx}: metrics"
        );
        let len = want.bitmap.len();
        assert_eq!(g.bitmap_len as usize, len, "{ctx}: bitmap_len");

        clear(&log);
        let mut buf = vec![0xA5u8; len + 3];
        let got = r
            .read_bitmap(&g, &mut buf)
            .unwrap_or_else(|e| panic!("{ctx}: {e:?}"))
            .to_vec();
        assert_eq!(got, want.bitmap, "{ctx}: bitmap bytes");
        assert!(
            buf[len..].iter().all(|&b| b == 0xA5),
            "{ctx}: wrote past the bitmap"
        );
        if len == 0 {
            assert!(reqs(&log).is_empty(), "{ctx}: a blank glyph costs no read");
        } else {
            assert_eq!(reqs(&log), [(base + rel, len)], "{ctx}: bitmap address");
            assert!(base + rel + len as u64 <= file_len);
            if rel > 65_535 {
                facts.over_u16 += 1;
            }
            if rel > 1 << 20 {
                facts.over_1mib += 1;
            }
        }

        // the cross-check oracle, same index position
        let (c2, g2) = whole.glyph_at(i as u32).expect("glyph_at");
        assert_eq!(c2, c, "{ctx}: index order");
        assert_eq!(g2.metrics, g.metrics, "{ctx}: vs Pack::glyph_at metrics");
        assert_eq!(g2.bitmap, &got[..], "{ctx}: vs Pack::glyph_at bitmap");

        if c as u32 > 0xFFFF {
            facts.astral += 1;
        }
        rel += len as u64;

        // neighbours that the font does not map are exactly absent
        for nb in [prev_char(c), next_char(c)].into_iter().flatten() {
            if !real.included_set.contains(&nb) {
                clear(&log);
                assert!(
                    r.find(nb).unwrap().is_none(),
                    "{ctx}: neighbour U+{:04X} must be absent",
                    nb as u32
                );
                let probes = reqs(&log);
                assert!(probes.len() <= bound);
                assert_reqs_within(&probes, 44, base, "absent probe");
            }
        }
    }
    assert_eq!(
        rel,
        r.header().bitmap_len as u64,
        "bitmap_len = sum of oracle bitmaps"
    );
    facts.region = rel;

    // controls in the cmap are excluded by the converter, unmapped scalars never present
    let mut absent: Vec<char> = oracle_cmap(&real.font)
        .into_iter()
        .filter(|c| c.is_control())
        .collect();
    absent.extend([
        '\0',
        '\u{FFFF}',
        '\u{10FFFF}',
        '\u{2A6A5}',
        '\u{E000}',
        '\u{1F600}',
        '\u{D7FF}',
    ]);
    for c in absent {
        if real.included_set.contains(&c) {
            continue;
        }
        assert!(r.find(c).unwrap().is_none(), "U+{:04X} at {px}px", c as u32);
    }
    facts
}

macro_rules! full_cmap_tests {
    ($($name:ident: $px:literal),* $(,)?) => {
        mod bookerly_every_glyph {
            use super::*;
            $(
                #[test]
                fn $name() {
                    let f = check_pack(bookerly(), $px);
                    assert!(f.region > 0);
                }
            )*
        }
        mod iansui_every_glyph {
            use super::*;
            $(
                #[test]
                fn $name() {
                    // 12,665 glyphs: the bitmap region is far above 64 KiB at every size, with most
                    // glyphs starting beyond byte 65,535, and U+20BB7 and friends are above U+FFFF
                    let path = require_iansui!();
                    let f = check_pack(iansui(&path), $px);
                    assert!(f.region > 65_535, "region {}", f.region);
                    assert!(f.over_u16 > 1000, "{} glyphs start above 65,535", f.over_u16);
                    assert!(f.astral > 0, "no char above U+FFFF was exercised");
                }
            )*
        }
    };
}

full_cmap_tests!(
    px16: 16,
    px19: 19,
    px23: 23,
    px27: 27,
    px28: 28,
    px32: 32,
    px35: 35,
    px38: 38,
    px46: 46,
);

#[test]
fn default_sizes_are_the_nine_this_file_covers() {
    // the macro above lists the sizes literally; keep it in step with the converter default
    assert_eq!(DEFAULT_SIZES, [16, 19, 23, 27, 28, 32, 35, 38, 46]);
}

#[test]
fn bookerly_46px_has_a_region_over_64_kib_and_offsets_beyond_it() {
    let f = check_pack(bookerly(), 46);
    assert!(f.region > 65_535, "region {}", f.region);
    assert!(f.over_u16 > 0, "no glyph starts above 65,535");
}

// ------------------------------------------ large indices with real Iansui

#[test]
fn iansui_23px_region_is_772597_bytes_with_most_glyphs_above_the_16_bit_range() {
    // the contract states 772,597 bytes for the 23px region; here it is re-derived as the sum
    // of the oracle's bitmap sizes, and the reader must agree with both
    let path = require_iansui!();
    let r = iansui(&path);
    let sum: usize = r
        .included
        .iter()
        .map(|&c| oracle_glyph(&r.font, c, 23).bitmap.len())
        .sum();
    assert_eq!(sum, 772_597);
    let bytes = pack_bytes(&r.out, 23);
    let rd = PackReader::open(bytes, bytes.len() as u64).unwrap();
    assert_eq!(rd.header().bitmap_len as usize, 772_597);
    assert_eq!(rd.header().glyph_count, 12_665);
    assert!(rd.header().bitmap_len > 65_535);
    assert!(772_597 > 1 << 16);
}

#[test]
fn iansui_u20bb7_is_found_above_both_16_bit_limits_with_the_oracles_data() {
    // U+20BB7 (4-byte UTF-8): code point > 0xFFFF; its bitmap starts beyond byte 65,535 of the
    // region (relative offset = sum of the oracle sizes of the glyphs before it)
    let path = require_iansui!();
    let r = iansui(&path);
    let target = '\u{20BB7}';
    for px in DEFAULT_SIZES {
        let bytes = pack_bytes(&r.out, px);
        let (mut rd, log) = open_spy(bytes);
        let n = rd.header().glyph_count;
        let base = 44 + 22 * n as u64;
        let mut rel = 0u64;
        for &c in &r.included {
            if c == target {
                break;
            }
            rel += oracle_glyph(&r.font, c, px).bitmap.len() as u64;
        }
        assert!(rel > 65_535, "{px}px: U+20BB7 starts at {rel}");
        let want = oracle_glyph(&r.font, target, px);
        assert!(!want.bitmap.is_empty());

        let g = rd.find(target).unwrap().expect("U+20BB7 is in Iansui");
        assert_eq!(
            (
                g.metrics.advance,
                g.metrics.offset_x,
                g.metrics.offset_y,
                g.metrics.width,
                g.metrics.height
            ),
            (
                want.advance,
                want.offset_x,
                want.offset_y,
                want.width,
                want.height
            ),
            "{px}px"
        );
        clear(&log);
        let mut buf = vec![0u8; g.bitmap_len as usize];
        assert_eq!(
            rd.read_bitmap(&g, &mut buf).unwrap(),
            &want.bitmap[..],
            "{px}px"
        );
        assert_eq!(
            reqs(&log),
            [(base + rel, want.bitmap.len())],
            "{px}px address"
        );

        // a 16-bit truncation of the code point would alias U+20BB7 onto U+0BB7; of U+2A6A5 onto U+A6A5
        assert!(
            rd.find('\u{2A6A5}').unwrap().is_none(),
            "{px}px: U+2A6A5 is not in Iansui"
        );
        assert!(!r.included_set.contains(&'\u{2A6A5}'));
    }
}

// ------------------------------------------- chars the font lacks

#[test]
fn chars_the_font_lacks_are_absent_and_never_alias_into_present_ones() {
    let b = bookerly();
    // not in Bookerly: Traditional Chinese, supplementary ideographs, the top of Unicode
    let lacking = [
        '臺',
        '灣',
        '\u{20BB7}',
        '\u{2A6A5}',
        '\u{FFFF}',
        '\u{10FFFF}',
        '\u{1F600}',
        '\u{E000}',
    ];
    for c in lacking {
        assert!(
            !b.included_set.contains(&c),
            "fixture: Bookerly must lack U+{:04X}",
            c as u32
        );
        // 16-bit aliases of the same char must not be what is returned
        for px in DEFAULT_SIZES {
            let (mut r, _) = open_spy(pack_bytes(&b.out, px));
            assert!(r.find(c).unwrap().is_none(), "U+{:04X} at {px}px", c as u32);
        }
    }
}

#[test]
fn iansui_lacks_u2a6a5_and_the_unassigned_scalars_at_every_size() {
    let path = require_iansui!();
    let r = iansui(&path);
    for c in [
        '\u{2A6A5}',
        '\u{FFFF}',
        '\u{10FFFF}',
        '\u{E000}',
        '\u{1F600}',
        '\u{D7FF}',
    ] {
        assert!(
            !r.included_set.contains(&c),
            "fixture: Iansui must lack U+{:04X}",
            c as u32
        );
        for px in DEFAULT_SIZES {
            let (mut rd, log) = open_spy(pack_bytes(&r.out, px));
            clear(&log);
            assert!(
                rd.find(c).unwrap().is_none(),
                "U+{:04X} at {px}px",
                c as u32
            );
            assert!(
                reqs(&log).len() <= 14,
                "13-14 probes at most for 12,665 glyphs"
            );
        }
    }
}

#[test]
fn an_absent_char_gets_the_missing_glyph_box_of_the_size_of_the_pack_it_was_looked_up_in() {
    // End to end on converter output: find -> Ok(None) -> the box from the pack's own
    // FontInfo; the box is derived below from its formula, per size.
    let b = bookerly();
    for px in DEFAULT_SIZES {
        let (mut r, _) = open_spy(pack_bytes(&b.out, px));
        assert!(r.find('臺').unwrap().is_none());
        let info = r.info();
        assert_eq!(info.pixel_size, px);

        // s = clamp(px * 3 / 4, 3, 255); advance = max(px, s + 2); offset_x = (advance - s) / 2
        let s = (px as u32 * 3 / 4).clamp(3, 255);
        let advance = (px as u32).max(s + 2);
        let want = Metrics {
            advance: advance as u16,
            offset_x: ((advance - s) / 2) as i16,
            offset_y: -(s as i16),
            width: s as u16,
            height: s as u16,
        };
        assert_eq!(missing_glyph_metrics(&info), want, "{px}px");

        let stride = (s as usize).div_ceil(8);
        let mut out = vec![0x5Au8; stride * s as usize];
        assert_eq!(render_missing_glyph(&info, &mut out), Some(want), "{px}px");
        for y in 0..s as usize {
            for x in 0..stride * 8 {
                let ink = x < s as usize
                    && (x == 0 || y == 0 || x == s as usize - 1 || y == s as usize - 1);
                let set = out[y * stride + x / 8] & (0x80 >> (x % 8)) != 0;
                assert_eq!(set, ink, "{px}px box pixel ({x},{y})");
            }
        }
    }
    // hand-derived anchors from the contract's examples
    let (r16, _) = open_spy(pack_bytes(&b.out, 16));
    assert_eq!(
        missing_glyph_metrics(&r16.info()),
        Metrics {
            advance: 16,
            offset_x: 2,
            offset_y: -12,
            width: 12,
            height: 12
        }
    );
    let (r23, _) = open_spy(pack_bytes(&b.out, 23));
    assert_eq!(
        missing_glyph_metrics(&r23.info()),
        Metrics {
            advance: 23,
            offset_x: 3,
            offset_y: -17,
            width: 17,
            height: 17
        }
    );
}

// ----------------------------------------------------------------- sizes

/// Sample chars present in Bookerly.
const BOOKERLY_SAMPLE: [char; 9] = ['H', 'p', 'g', 'W', 'A', 'a', '0', '|', 'x'];
/// Present in Iansui (real_font.rs SAMPLE).
const IANSUI_SAMPLE: [char; 11] = [
    '臺', '灣', '「', '。', '𠮷', '龜', '鬱', 'A', 'z', '３', '　',
];

fn size_consistency(real: &Real, sample: &[char]) {
    let mut ids = BTreeSet::new();
    let mut prev_line = 0u16;
    // per size: (info, per-char (metrics, bitmap) as served)
    let mut served: Vec<(u16, Vec<(Metrics, Vec<u8>)>)> = Vec::new();
    for px in DEFAULT_SIZES {
        let bytes = pack_bytes(&real.out, px);
        let (mut r, _) = open_spy(bytes);
        let info = r.info();
        assert_eq!(
            info.pixel_size, px,
            "the pack opened for {px} reports {}",
            info.pixel_size
        );
        assert_eq!(info.font_id, font_id_by_formula(&real.sha, px));
        assert!(
            ids.insert(info.font_id),
            "font_id of {px}px repeats another size"
        );
        let (lh, asc) = oracle_line_metrics(&real.font, px);
        assert_eq!((info.line_height, info.ascent), (lh, asc), "{px}px");
        assert!(
            info.line_height > prev_line,
            "line_height must grow with the size: {px}px"
        );
        prev_line = info.line_height;

        let mut row = Vec::new();
        for &c in sample {
            let want = oracle_glyph(&real.font, c, px);
            let g = r
                .find(c)
                .unwrap()
                .unwrap_or_else(|| panic!("U+{:04X} at {px}px", c as u32));
            let mut buf = vec![0u8; g.bitmap_len as usize];
            let bm = r.read_bitmap(&g, &mut buf).unwrap().to_vec();
            let m = g.metrics;
            assert_eq!(
                (m.advance, m.offset_x, m.offset_y, m.width, m.height),
                (
                    want.advance,
                    want.offset_x,
                    want.offset_y,
                    want.width,
                    want.height
                ),
                "U+{:04X} metrics at {px}px",
                c as u32
            );
            assert_eq!(bm, want.bitmap, "U+{:04X} bitmap at {px}px", c as u32);
            row.push((m, bm));
        }
        served.push((px, row));
    }
    assert_eq!(ids.len(), DEFAULT_SIZES.len());
    // a size-N glyph is never what a size-M pack serves: wherever the oracle says two sizes differ,
    // the two packs differ too, and every pair of sizes differs for at least one sample char
    for (i, (pa, ra)) in served.iter().enumerate() {
        for (pb, rb) in &served[i + 1..] {
            let mut differing = 0;
            for (k, &c) in sample.iter().enumerate() {
                let oa = oracle_glyph(&real.font, c, *pa);
                let ob = oracle_glyph(&real.font, c, *pb);
                if oa != ob {
                    differing += 1;
                    assert_ne!(
                        ra[k], rb[k],
                        "U+{:04X}: {pa}px and {pb}px packs serve the same data",
                        c as u32
                    );
                }
            }
            assert!(
                differing > 0,
                "{pa}px and {pb}px have identical sample glyphs: fixture too weak"
            );
        }
    }
}

#[test]
fn bookerly_each_size_pack_reports_its_size_and_serves_that_sizes_glyphs() {
    size_consistency(bookerly(), &BOOKERLY_SAMPLE);
}

#[test]
fn iansui_each_size_pack_reports_its_size_and_serves_that_sizes_glyphs() {
    let path = require_iansui!();
    size_consistency(iansui(&path), &IANSUI_SAMPLE);
}

#[test]
fn a_pack_never_passes_for_another_size_through_the_slice_source() {
    // the `&[u8]` source reads packs of two sizes side by side without mixing them up
    let b = bookerly();
    let (b16, b46) = (pack_bytes(&b.out, 16), pack_bytes(&b.out, 46));
    let mut r16 = PackReader::open(b16, b16.len() as u64).unwrap();
    let mut r46 = PackReader::open(b46, b46.len() as u64).unwrap();
    assert_eq!((r16.info().pixel_size, r46.info().pixel_size), (16, 46));
    for c in BOOKERLY_SAMPLE {
        let a = r16.find(c).unwrap().unwrap();
        let z = r46.find(c).unwrap().unwrap();
        let (oa, oz) = (oracle_glyph(&b.font, c, 16), oracle_glyph(&b.font, c, 46));
        assert_eq!((a.metrics.width, a.metrics.height), (oa.width, oa.height));
        assert_eq!((z.metrics.width, z.metrics.height), (oz.width, oz.height));
        let mut ba = vec![0u8; a.bitmap_len as usize];
        let mut bz = vec![0u8; z.bitmap_len as usize];
        assert_eq!(r16.read_bitmap(&a, &mut ba).unwrap(), &oa.bitmap[..]);
        assert_eq!(r46.read_bitmap(&z, &mut bz).unwrap(), &oz.bitmap[..]);
    }
}

// ------------------------------------------------------- caller buffer too small

fn short_buffer_check(real: &Real, sample: &[char]) {
    let bytes = pack_bytes(&real.out, 23);
    let (mut r, log) = open_spy(bytes);
    for &c in sample {
        let need = oracle_glyph(&real.font, c, 23).bitmap.len();
        assert!(need > 0, "fixture: U+{:04X} must have ink", c as u32);
        let g = r.find(c).unwrap().unwrap();
        for have in [0, 1, need / 2, need - 1] {
            clear(&log);
            let mut buf = vec![0x33u8; have];
            assert_eq!(
                r.read_bitmap(&g, &mut buf),
                Err(FontError::BufferTooSmall { needed: need }),
                "U+{:04X} with {have} of {need} bytes",
                c as u32
            );
            assert!(reqs(&log).is_empty(), "a too-small buffer costs no read");
            assert!(buf.iter().all(|&b| b == 0x33), "nothing written");
        }
        // exactly enough is enough
        let mut buf = vec![0x33u8; need];
        assert!(r.read_bitmap(&g, &mut buf).is_ok());
    }
}

#[test]
fn bookerly_a_buffer_shorter_than_the_bitmap_is_buffer_too_small_without_any_read() {
    short_buffer_check(bookerly(), &['H', 'g', 'W', '|']);
}

#[test]
fn iansui_a_buffer_shorter_than_the_bitmap_is_buffer_too_small_without_any_read() {
    let path = require_iansui!();
    short_buffer_check(iansui(&path), &['臺', '𠮷', '龜', '鬱']);
}

// ------------------------------------------------------- the `&[u8]` source

#[test]
fn slice_source_serves_bookerly_packs_like_the_oracle() {
    let b = bookerly();
    for px in [16u16, 23, 46] {
        let bytes = pack_bytes(&b.out, px);
        let mut r = PackReader::open(bytes, bytes.len() as u64).unwrap();
        for &c in &b.included {
            let want = oracle_glyph(&b.font, c, px);
            let g = r.find(c).unwrap().unwrap();
            assert_eq!(g.bitmap_len as usize, want.bitmap.len());
            let mut buf = vec![0u8; want.bitmap.len()];
            assert_eq!(
                r.read_bitmap(&g, &mut buf).unwrap(),
                &want.bitmap[..],
                "U+{:04X} {px}px",
                c as u32
            );
        }
    }
}

#[test]
fn slice_source_with_a_wrong_file_len_is_a_length_mismatch_never_a_panic() {
    let bytes = pack_bytes(&bookerly().out, 23);
    let len = bytes.len() as u64;
    for wrong in [len + 1, len - 1, len + 4096, 0, 43, 44, 1 << 32, u64::MAX] {
        match PackReader::open(bytes, wrong) {
            Err(FontError::Corrupt(PackError::LengthMismatch)) => assert!(wrong >= 44, "{wrong}"),
            Err(FontError::Corrupt(PackError::TooShort)) => assert!(wrong < 44, "{wrong}"),
            Err(e) => panic!("file_len {wrong}: {e:?}"),
            Ok(_) => panic!("file_len {wrong} was accepted"),
        }
    }
    assert!(PackReader::open(bytes, len).is_ok());
}

#[test]
fn a_card_shorter_than_its_file_len_gives_io_for_the_missing_tail_and_not_a_panic() {
    // the bytes lose their last byte while file_len still says "whole file": the header and the
    // first glyph still work, the last non-blank glyph (it ends exactly at the end) does not
    let b = bookerly();
    let bytes = pack_bytes(&b.out, 23);
    let cut = &bytes[..bytes.len() - 1];
    let mut r = PackReader::open(cut, bytes.len() as u64).expect("the header is intact");
    let first = b.included[0];
    let want = oracle_glyph(&b.font, first, 23);
    let g = r.find(first).unwrap().unwrap();
    let mut buf = vec![0u8; g.bitmap_len as usize];
    assert_eq!(r.read_bitmap(&g, &mut buf).unwrap(), &want.bitmap[..]);

    let last = *b
        .included
        .iter()
        .rev()
        .find(|&&c| !oracle_glyph(&b.font, c, 23).bitmap.is_empty())
        .unwrap();
    let g = r.find(last).unwrap().unwrap();
    let mut buf = vec![0u8; g.bitmap_len as usize];
    assert_eq!(
        r.read_bitmap(&g, &mut buf),
        Err(FontError::Io(pulp_fontpack::OutOfRange))
    );
}

// ------------------------------------------------------------- read budget

fn budget_check(real: &Real, px: u16, expect_probe_bound: Option<usize>) {
    let bytes = pack_bytes(&real.out, px);
    let (mut r, log) = open_spy(bytes);
    assert_eq!(
        reqs(&log),
        [(0u64, 44usize)],
        "open: exactly one read of 44 bytes at 0"
    );
    let n = r.header().glyph_count;
    if let Some(b) = expect_probe_bound {
        assert_eq!(max_probes(n), b, "ceil(log2({n}+1))");
    }
    let bound = max_probes(n);
    let index_end = 44 + 22 * n as u64;
    let mut worst = 0usize;
    let mut base_rel = 0u64;
    for &c in &real.included {
        clear(&log);
        let g = r.find(c).unwrap().unwrap();
        let l = reqs(&log);
        worst = worst.max(l.len());
        assert!(
            !l.is_empty() && l.len() <= bound,
            "U+{:04X}: {} reads",
            c as u32,
            l.len()
        );
        for &(o, len) in &l {
            assert_eq!(len, 22);
            assert!(
                o + 22 <= index_end,
                "U+{:04X}: a find read past the index",
                c as u32
            );
        }
        clear(&log);
        let mut buf = vec![0u8; g.bitmap_len as usize];
        r.read_bitmap(&g, &mut buf).unwrap();
        let l = reqs(&log);
        let len = g.bitmap_len as usize;
        if len == 0 {
            assert!(l.is_empty());
        } else {
            assert_eq!(l, [(index_end + base_rel, len)], "read_bitmap = one read");
        }
        base_rel += len as u64;
    }
    // the search is a real binary search: some lookup uses (nearly) the whole budget
    assert!(
        worst + 2 >= bound,
        "worst case {worst} is far below the bound {bound}"
    );
}

#[test]
fn bookerly_read_budget_per_lookup_is_the_binary_search_bound() {
    budget_check(bookerly(), 23, None);
}

#[test]
fn iansui_23px_read_budget_is_at_most_14_reads_of_22_bytes_per_find_and_one_per_bitmap() {
    // 12,665 glyphs: 2^13 = 8192 < 12,666 <= 2^14 = 16384, so ceil(log2(12,666)) = 14
    let path = require_iansui!();
    budget_check(iansui(&path), 23, Some(14));
}

// ----------------------------------------------------- I/O failure injection

/// open, find(c), read_bitmap: fail on every read number k (and one past the end).
fn inject(real: &Real, px: u16, c: char) {
    let bytes = pack_bytes(&real.out, px);
    let file_len = bytes.len() as u64;
    let ctx0 = format!("U+{:04X} at {px}px", c as u32);

    // clean run: how many reads each phase takes
    let (mut r, log) = open_spy(bytes);
    let after_open = reqs(&log).len();
    let found = r.find(c).unwrap();
    let after_find = reqs(&log).len();
    if let Some(g) = &found {
        let mut buf = vec![0u8; g.bitmap_len as usize];
        r.read_bitmap(g, &mut buf).unwrap();
    }
    let total = reqs(&log).len();
    assert_eq!(after_open, 1);
    assert!(after_find >= 1);

    for k in 1..=total + 1 {
        let ctx = format!("{ctx0}, read {k} of {total} fails");
        let spy = Spy::new(bytes).failing_on(k);
        let log = spy.log();
        let mut reader = match PackReader::open(spy, file_len) {
            Ok(r) => r,
            Err(e) => {
                assert_eq!(k, 1, "{ctx}");
                assert_eq!(e, FontError::Io(SpyErr::Injected(1)), "{ctx}");
                assert_eq!(reqs(&log).len(), 1, "{ctx}: no retry");
                continue;
            }
        };
        assert_ne!(k, 1, "{ctx}: open must fail when its only read fails");
        let found = match reader.find(c) {
            Ok(f) => f,
            Err(e) => {
                assert!((2..=after_find).contains(&k), "{ctx}");
                assert_eq!(e, FontError::Io(SpyErr::Injected(k)), "{ctx}");
                assert_eq!(reqs(&log).len(), k, "{ctx}: no retry, no further read");
                assert_reqs_within(&reqs(&log), 0, file_len, &ctx);
                continue;
            }
        };
        assert!(k > after_find, "{ctx}: find survived its failing read");
        match found {
            Some(g) => {
                let mut b = vec![0u8; g.bitmap_len as usize];
                match reader.read_bitmap(&g, &mut b) {
                    Ok(_) => assert!(k > total, "{ctx}: read_bitmap survived its failing read"),
                    Err(e) => {
                        assert_eq!(k, total, "{ctx}");
                        assert_eq!(e, FontError::Io(SpyErr::Injected(k)), "{ctx}");
                        assert_eq!(reqs(&log).len(), k, "{ctx}: no retry");
                    }
                }
            }
            None => assert!(k > total, "{ctx}"),
        }
        assert_reqs_within(&reqs(&log), 0, file_len, &ctx);
    }
}

fn probe_chars(real: &Real) -> Vec<char> {
    let inc = &real.included;
    let mut v = vec![
        inc[0],
        inc[inc.len() / 2],
        inc[inc.len() - 1],
        '\u{2A6A5}',
        '\u{FFFF}',
    ];
    // a blank glyph (no bitmap read): the first whitespace-like char with an empty oracle bitmap
    if let Some(&b) = inc
        .iter()
        .find(|&&c| oracle_glyph(&real.font, c, 23).bitmap.is_empty())
    {
        v.push(b);
    }
    v
}

#[test]
fn bookerly_a_failing_read_anywhere_is_io_with_the_source_error_and_is_not_retried() {
    let b = bookerly();
    for c in probe_chars(b) {
        inject(b, 23, c);
    }
    inject(b, 46, 'H');
    inject(b, 16, '\u{20BB7}'); // absent
}

#[test]
fn iansui_a_failing_read_anywhere_is_io_with_the_source_error_and_is_not_retried() {
    let path = require_iansui!();
    let r = iansui(&path);
    for c in probe_chars(r) {
        inject(r, 23, c);
    }
    inject(r, 46, '\u{20BB7}');
    inject(r, 16, '臺');
    inject(r, 28, '\u{2A6A5}');
}

// ------------------------------------------------------ damaged bytes on the card

/// Header byte `p` flipped by the card: the outcome per field (the format rules).
fn header_tamper(bytes: &[u8]) {
    let file_len = bytes.len() as u64;
    for p in 0..44u64 {
        for x in [0x01u8, 0x80, 0xFF] {
            let spy = Spy::new(bytes).flipping(p, x);
            let log = spy.log();
            let res = PackReader::open(spy, file_len);
            assert_eq!(reqs(&log), [(0u64, 44usize)], "byte {p}");
            let ctx = format!("header byte {p} ^ {x:#x}");
            let mut flipped = bytes[..44].to_vec();
            flipped[p as usize] ^= x;
            match p {
                0..=3 => assert_eq!(
                    res.err().expect(&ctx),
                    FontError::Corrupt(PackError::BadMagic),
                    "{ctx}"
                ),
                4 | 5 => assert_eq!(
                    res.err().expect(&ctx),
                    FontError::Unsupported {
                        found: u16::from_le_bytes([flipped[4], flipped[5]])
                    },
                    "{ctx}"
                ),
                6..=19 => {
                    // pixel_size, font_id, line_height, ascent: not range-checked, taken as found
                    let r = res.unwrap_or_else(|e| panic!("{ctx}: {e:?}"));
                    let i = r.info();
                    assert_eq!(
                        i.pixel_size,
                        u16::from_le_bytes([flipped[6], flipped[7]]),
                        "{ctx}"
                    );
                    assert_eq!(
                        i.font_id,
                        u64::from_le_bytes(flipped[8..16].try_into().unwrap()),
                        "{ctx}"
                    );
                    assert_eq!(
                        i.line_height,
                        u16::from_le_bytes([flipped[16], flipped[17]]),
                        "{ctx}"
                    );
                    assert_eq!(
                        i.ascent,
                        u16::from_le_bytes([flipped[18], flipped[19]]),
                        "{ctx}"
                    );
                }
                20..=39 => assert_eq!(
                    res.err().expect(&ctx),
                    FontError::Corrupt(PackError::BadLayout),
                    "{ctx}"
                ),
                _ => assert_eq!(
                    res.err().expect(&ctx),
                    FontError::Corrupt(PackError::LengthMismatch),
                    "{ctx}"
                ),
            }
        }
    }
}

/// One byte of a probed record flipped by the card. The expected outcome is derived from what
/// the reader was handed: the first probed record that fails `Record::validate` (t1 step 6 a, c,
/// d) must end the search with exactly that error and index; when every probed record is valid
/// the lookup must be `Ok`. Whatever the damage, requests stay inside the index.
fn record_tamper(bytes: &[u8], c: char) {
    let file_len = bytes.len() as u64;
    let (mut clean, clean_log) = open_spy(bytes);
    let n = clean.header().glyph_count;
    let region = clean.header().bitmap_len;
    let index_end = 44 + 22 * n as u64;
    clear(&clean_log);
    let _ = clean.find(c).unwrap();
    let path: Vec<u64> = reqs(&clean_log).iter().map(|&(o, _)| o).collect();
    assert!(!path.is_empty());
    let bound = max_probes(n);

    for &s in &path {
        for pos in 0..22usize {
            for x in [0x01u8, 0x80, 0xFF] {
                let ctx = format!("{c:?}: record at {s}, byte {pos} ^ {x:#x}");
                let spy = Spy::new(bytes).flipping(s + pos as u64, x);
                let log = spy.log();
                let mut r = PackReader::open(spy, file_len).expect("the header is untouched");
                clear(&log);
                let res = r.find(c);
                let l = reqs(&log);
                assert!(l.len() <= bound, "{ctx}");
                assert_reqs_within(&l, 44, index_end, &ctx);

                // what the reader saw, record by record, in probe order
                let mut expect_err: Option<PackError> = None;
                let mut seen_hits: Vec<Metrics> = Vec::new();
                for &(o, len) in &l {
                    assert_eq!(len, 22, "{ctx}");
                    let mut rec = bytes[o as usize..o as usize + 22].to_vec();
                    if o == s {
                        rec[pos] ^= x;
                    }
                    let rec = Record::decode(&rec).unwrap();
                    let idx = ((o - 44) / 22) as u32;
                    match rec.validate(idx, None, region) {
                        Err(e) => {
                            expect_err = Some(e);
                            assert_eq!(
                                *l.last().unwrap(),
                                (o, 22),
                                "{ctx}: the failing probe ends the search"
                            );
                            break;
                        }
                        Ok(cp) => {
                            if cp == c {
                                seen_hits.push(rec.metrics);
                            }
                        }
                    }
                }
                match (res, expect_err) {
                    (Err(FontError::Corrupt(got)), Some(want)) => assert_eq!(got, want, "{ctx}"),
                    (Ok(Some(g)), None) => {
                        assert!(
                            seen_hits.contains(&g.metrics),
                            "{ctx}: hit is not a probed record of {c:?}"
                        );
                    }
                    (Ok(None), None) => {
                        assert!(seen_hits.is_empty(), "{ctx}: a probed match was missed")
                    }
                    (other, want) => panic!("{ctx}: got {other:?}, expected error {want:?}"),
                }
            }
        }
    }
}

#[test]
fn bookerly_a_card_that_flips_bits_in_the_header_is_reported_per_field() {
    header_tamper(pack_bytes(&bookerly().out, 23));
}

#[test]
fn iansui_a_card_that_flips_bits_in_the_header_is_reported_per_field() {
    let path = require_iansui!();
    header_tamper(pack_bytes(&iansui(&path).out, 23));
}

#[test]
fn bookerly_a_card_that_flips_bits_in_a_probed_record_is_never_unsafe() {
    let b = bookerly();
    let bytes = pack_bytes(&b.out, 23);
    for c in probe_chars(b) {
        record_tamper(bytes, c);
    }
}

#[test]
fn iansui_a_card_that_flips_bits_in_a_probed_record_is_never_unsafe() {
    let path = require_iansui!();
    let r = iansui(&path);
    let bytes = pack_bytes(&r.out, 23);
    for c in probe_chars(r) {
        record_tamper(bytes, c);
    }
    record_tamper(bytes, '\u{20BB7}');
}

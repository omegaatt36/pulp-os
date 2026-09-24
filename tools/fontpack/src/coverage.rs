//! Unicode coverage by block (R12), so missing CJK coverage is explicit.
//!
//! Bounds are copied from the Unicode Character Database `Blocks.txt`
//! (Blocks-18.0.0.txt, <https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt>).
//! Sizes are block sizes, which include unassigned code points (for example
//! Hangul Syllables AC00..D7AF has 11,184 slots, 11,172 assigned AC00..D7A3).

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Han,
    Kana,
    Hangul,
    Bopomofo,
    Symbols,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Han,
        Group::Kana,
        Group::Hangul,
        Group::Bopomofo,
        Group::Symbols,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Han => "Han ideographs",
            Group::Kana => "Kana",
            Group::Hangul => "Hangul",
            Group::Bopomofo => "Bopomofo",
            Group::Symbols => "CJK symbols, radicals, forms",
        }
    }
}

#[derive(Debug)]
pub struct Block {
    pub group: Group,
    pub name: &'static str,
    pub first: u32,
    pub last: u32,
}

impl Block {
    pub fn size(&self) -> u32 {
        self.last - self.first + 1
    }
}

const fn b(group: Group, first: u32, last: u32, name: &'static str) -> Block {
    Block {
        group,
        name,
        first,
        last,
    }
}

use Group::{Bopomofo, Han, Hangul, Kana, Symbols};

#[rustfmt::skip]
pub const BLOCKS: &[Block] = &[
    b(Han, 0x4E00, 0x9FFF, "CJK Unified Ideographs"),
    b(Han, 0x3400, 0x4DBF, "CJK Unified Ideographs Extension A"),
    b(Han, 0x20000, 0x2A6DF, "CJK Unified Ideographs Extension B"),
    b(Han, 0x2A700, 0x2B73F, "CJK Unified Ideographs Extension C"),
    b(Han, 0x2B740, 0x2B81F, "CJK Unified Ideographs Extension D"),
    b(Han, 0x2B820, 0x2CEAF, "CJK Unified Ideographs Extension E"),
    b(Han, 0x2CEB0, 0x2EBEF, "CJK Unified Ideographs Extension F"),
    b(Han, 0x30000, 0x3134F, "CJK Unified Ideographs Extension G"),
    b(Han, 0x31350, 0x323AF, "CJK Unified Ideographs Extension H"),
    b(Han, 0x2EBF0, 0x2EE5F, "CJK Unified Ideographs Extension I"),
    b(Han, 0x323B0, 0x3347F, "CJK Unified Ideographs Extension J"),
    b(Han, 0xF900, 0xFAFF, "CJK Compatibility Ideographs"),
    b(Han, 0x2F800, 0x2FA1F, "CJK Compatibility Ideographs Supplement"),
    b(Kana, 0x3040, 0x309F, "Hiragana"),
    b(Kana, 0x30A0, 0x30FF, "Katakana"),
    b(Kana, 0x31F0, 0x31FF, "Katakana Phonetic Extensions"),
    b(Kana, 0x1AFF0, 0x1AFFF, "Kana Extended-B"),
    b(Kana, 0x1B000, 0x1B0FF, "Kana Supplement"),
    b(Kana, 0x1B100, 0x1B12F, "Kana Extended-A"),
    b(Kana, 0x1B130, 0x1B16F, "Small Kana Extension"),
    b(Hangul, 0xAC00, 0xD7AF, "Hangul Syllables"),
    b(Hangul, 0x1100, 0x11FF, "Hangul Jamo"),
    b(Hangul, 0x3130, 0x318F, "Hangul Compatibility Jamo"),
    b(Hangul, 0xA960, 0xA97F, "Hangul Jamo Extended-A"),
    b(Hangul, 0xD7B0, 0xD7FF, "Hangul Jamo Extended-B"),
    b(Bopomofo, 0x3100, 0x312F, "Bopomofo"),
    b(Bopomofo, 0x31A0, 0x31BF, "Bopomofo Extended"),
    b(Symbols, 0x3000, 0x303F, "CJK Symbols and Punctuation"),
    b(Symbols, 0xFF00, 0xFFEF, "Halfwidth and Fullwidth Forms"),
    b(Symbols, 0xFE30, 0xFE4F, "CJK Compatibility Forms"),
    b(Symbols, 0x3200, 0x32FF, "Enclosed CJK Letters and Months"),
    b(Symbols, 0x3300, 0x33FF, "CJK Compatibility"),
    b(Symbols, 0x2E80, 0x2EFF, "CJK Radicals Supplement"),
    b(Symbols, 0x2F00, 0x2FDF, "Kangxi Radicals"),
    b(Symbols, 0x31C0, 0x31EF, "CJK Strokes"),
];

#[derive(Debug)]
pub struct BlockCoverage {
    pub block: &'static Block,
    pub mapped: u32,
}

/// Mapped count per block, one entry per `BLOCKS` entry in table order.
/// `sorted_cps` must be sorted ascending.
pub fn by_block(sorted_cps: &[u32]) -> Vec<BlockCoverage> {
    BLOCKS
        .iter()
        .map(|block| {
            let lo = sorted_cps.partition_point(|&cp| cp < block.first);
            let hi = sorted_cps.partition_point(|&cp| cp <= block.last);
            BlockCoverage {
                block,
                mapped: (hi - lo) as u32,
            }
        })
        .collect()
}

/// `(group, mapped, total block size)` per group, in `Group::ALL` order.
pub fn by_group(coverage: &[BlockCoverage]) -> Vec<(Group, u32, u32)> {
    Group::ALL
        .iter()
        .map(|&g| {
            let (mapped, size) = coverage
                .iter()
                .filter(|c| c.block.group == g)
                .fold((0, 0), |(m, s), c| (m + c.mapped, s + c.block.size()));
            (g, mapped, size)
        })
        .collect()
}

/// Human-readable table: per block `mapped / size`, then group totals.
pub fn render(sorted_cps: &[u32]) -> String {
    let cov = by_block(sorted_cps);
    let mut s = String::new();
    let _ = writeln!(s, "Unicode coverage (bounds: Unicode Blocks-18.0.0.txt)");
    let _ = writeln!(
        s,
        "  {:<21} {:<40} {:>15} {:>13}",
        "range", "block", "mapped / size", "pct"
    );
    for c in &cov {
        let range = format!("U+{:04X}..U+{:04X}", c.block.first, c.block.last);
        let _ = writeln!(
            s,
            "  {:<21} {:<40} {:>15} {:>12.1}%",
            range,
            c.block.name,
            format!("{} / {}", c.mapped, c.block.size()),
            pct(c.mapped, c.block.size()),
        );
    }
    let _ = writeln!(s, "  totals by group:");
    for (g, mapped, size) in by_group(&cov) {
        let _ = writeln!(
            s,
            "  {:<62} {:>15} {:>12.1}%",
            g.name(),
            format!("{mapped} / {size}"),
            pct(mapped, size)
        );
    }
    let in_blocks: u32 = cov.iter().map(|c| c.mapped).sum();
    let above_bmp = sorted_cps.iter().filter(|&&cp| cp > 0xFFFF).count();
    let _ = writeln!(
        s,
        "  outside the blocks above: {}; above U+FFFF: {above_bmp}",
        sorted_cps.len() - in_blocks as usize
    );
    s
}

fn pct(mapped: u32, size: u32) -> f64 {
    100.0 * mapped as f64 / size as f64
}

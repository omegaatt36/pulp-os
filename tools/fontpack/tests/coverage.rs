//! Coverage counts for a controlled code point set (R12).
use pulp_fontpack::coverage::{BLOCKS, Group, by_block, by_group};

fn block(name: &str) -> &'static pulp_fontpack::coverage::Block {
    BLOCKS
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("{name}"))
}

#[test]
fn block_bounds_match_unicode_blocks_txt() {
    // Blocks.txt (Unicode 18.0.0), https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt
    let want: &[(&str, u32, u32)] = &[
        ("CJK Unified Ideographs", 0x4E00, 0x9FFF),
        ("CJK Unified Ideographs Extension A", 0x3400, 0x4DBF),
        ("CJK Unified Ideographs Extension B", 0x20000, 0x2A6DF),
        ("CJK Unified Ideographs Extension H", 0x31350, 0x323AF),
        ("CJK Compatibility Ideographs", 0xF900, 0xFAFF),
        ("Hiragana", 0x3040, 0x309F),
        ("Katakana", 0x30A0, 0x30FF),
        ("Hangul Syllables", 0xAC00, 0xD7AF),
        ("Hangul Jamo", 0x1100, 0x11FF),
        ("CJK Symbols and Punctuation", 0x3000, 0x303F),
        ("Halfwidth and Fullwidth Forms", 0xFF00, 0xFFEF),
        ("Bopomofo", 0x3100, 0x312F),
    ];
    for &(name, first, last) in want {
        let b = block(name);
        assert_eq!((b.first, b.last), (first, last), "{name}");
    }
    // blocks never overlap, so a code point is counted at most once
    let mut sorted: Vec<_> = BLOCKS.iter().collect();
    sorted.sort_by_key(|b| b.first);
    for w in sorted.windows(2) {
        assert!(
            w[0].last < w[1].first,
            "{} overlaps {}",
            w[0].name,
            w[1].name
        );
    }
}

#[test]
fn counts_follow_the_chosen_set() {
    // chosen set, sorted: 2 Latin (outside every block), 3 Hiragana, 1 Katakana,
    // 4 CJK Unified (incl. both block ends), 1 Ext A, 2 Ext B, 1 Hangul syllable
    // (last assigned AC00..D7A3 syllable), 1 fullwidth form
    let cps: Vec<u32> = vec![
        0x41, 0x61, 0x3041, 0x3042, 0x309F, 0x30A2, 0x3400, 0x4E00, 0x4E01, 0x6C38, 0x9FFF, 0xD7A3,
        0xFF01, 0x20000, 0x2A6DF,
    ];
    let cov = by_block(&cps);
    let count = |name: &str| {
        cov.iter()
            .find(|c| c.block.name == name)
            .map(|c| c.mapped)
            .unwrap()
    };
    assert_eq!(count("Hiragana"), 3);
    assert_eq!(count("Katakana"), 1);
    assert_eq!(count("CJK Unified Ideographs"), 4);
    assert_eq!(count("CJK Unified Ideographs Extension A"), 1);
    assert_eq!(count("CJK Unified Ideographs Extension B"), 2);
    assert_eq!(count("Hangul Syllables"), 1);
    assert_eq!(count("Halfwidth and Fullwidth Forms"), 1);
    assert_eq!(count("Hangul Jamo"), 0);
    assert_eq!(count("CJK Symbols and Punctuation"), 0);
    assert_eq!(
        cov.len(),
        BLOCKS.len(),
        "every block is reported, including empty ones"
    );

    // block sizes are last - first + 1
    assert_eq!(block("CJK Unified Ideographs").size(), 0x9FFF - 0x4E00 + 1);
    assert_eq!(block("CJK Unified Ideographs").size(), 20_992);
    assert_eq!(block("Hiragana").size(), 96);

    // groups: Han = 4 + 1 + 2 = 7; Kana = 3 + 1 = 4; Hangul = 1
    let groups = by_group(&cov);
    let g = |grp: Group| groups.iter().find(|x| x.0 == grp).map(|x| x.1).unwrap();
    assert_eq!(g(Group::Han), 7);
    assert_eq!(g(Group::Kana), 4);
    assert_eq!(g(Group::Hangul), 1);
    assert_eq!(g(Group::Bopomofo), 0);
    assert_eq!(g(Group::Symbols), 1);
}

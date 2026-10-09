//! Staging only the start of a page window (`ReaderApp::stage_prefix`) against
//! staging all of it (`usize::MAX`, what the reader did before).
//!
//! The prefix is an optimisation only: for every text, font size and width the
//! pages (their start offsets and every laid-out line) must be exactly the ones
//! a fully staged window gives, including prefixes far smaller than a page, which
//! make the layout measure scalars that are not staged yet and so exercise the
//! "stage more and lay out again" path. Expectations come from the fully staged
//! run of the same production ReaderApp; nothing here lays text out itself.
use crate::cjk_support;

use cjk_support::{BOOK, card};
use pulp_host::reader::{Action, Phase, Rig};

/// Small deterministic generator (no external files, no network).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

const CJK: [&str; 8] = ["臺", "灣", "繁", "體", "中", "文", "𠮷", "A"];
const PUNCT: [&str; 10] = ["，", "。", "、", "？", "！", "）", "」", "（", "「", "』"];
const LATIN_WORDS: [&str; 6] = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"];

fn cjk_paragraph(g: &mut Lcg, scalars: usize) -> String {
    let mut s = String::new();
    for _ in 0..scalars {
        if g.next() % 7 == 0 {
            s.push_str(g.pick(&PUNCT));
        } else {
            s.push_str(g.pick(&CJK));
        }
    }
    s
}

fn latin_paragraph(g: &mut Lcg, words: usize) -> String {
    let mut s = String::new();
    for i in 0..words {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(g.pick(&LATIN_WORDS));
    }
    s
}

/// Several fixtures that differ in what a window holds around the page it starts.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let mut g = Lcg(0x5EED);
    let mut out = Vec::new();

    // CJK all the way (the case the prefix is for)
    let mut t = String::new();
    for _ in 0..40 {
        t.push_str(&cjk_paragraph(&mut g, 120));
        t.push('\n');
    }
    out.push(("cjk only", t.into_bytes()));

    // pages of Latin only with CJK later in the window, and the other way round
    let mut t = String::new();
    for _ in 0..10 {
        t.push_str(&latin_paragraph(&mut g, 260));
        t.push('\n');
        t.push_str(&cjk_paragraph(&mut g, 90));
        t.push('\n');
    }
    out.push(("latin runs then cjk", t.into_bytes()));

    // one scalar needing a fallback glyph, thousands of bytes into Latin text
    let mut t = latin_paragraph(&mut g, 900);
    t.push_str(" 臺 ");
    t.push_str(&latin_paragraph(&mut g, 300));
    out.push(("late cjk scalar", t.into_bytes()));

    // no fallback glyph at all
    let mut t = String::new();
    for _ in 0..30 {
        t.push_str(&latin_paragraph(&mut g, 150));
        t.push('\n');
    }
    out.push(("latin only", t.into_bytes()));

    // headings and bold/italic markers (0x01 'H'/'h' heading, 'B'/'b', 'I'/'i')
    let mut t = Vec::new();
    for i in 0..30 {
        if i % 4 == 0 {
            t.extend_from_slice(&[1, b'H']);
            t.extend_from_slice(cjk_paragraph(&mut g, 12).as_bytes());
            t.extend_from_slice(&[1, b'h']);
            t.push(b'\n');
        }
        t.extend_from_slice(cjk_paragraph(&mut g, 60).as_bytes());
        t.extend_from_slice(&[1, b'B']);
        t.extend_from_slice(latin_paragraph(&mut g, 20).as_bytes());
        t.extend_from_slice(&[1, b'b']);
        t.extend_from_slice(cjk_paragraph(&mut g, 60).as_bytes());
        t.push(b'\n');
    }
    out.push(("headings and styles", t));

    // a heading that stays open across many pages
    let mut t = vec![1, b'H'];
    for _ in 0..12 {
        t.extend_from_slice(cjk_paragraph(&mut g, 150).as_bytes());
        t.push(b'\n');
    }
    t.extend_from_slice(&[1, b'h']);
    t.extend_from_slice(b"\nend");
    out.push(("long heading", t));

    out
}

struct Walk {
    offsets: Vec<u32>,
    pages: Vec<Vec<Vec<u8>>>,
    sd_reads: usize,
}

fn walk(text: &[u8], idx: u8, width: u32, prefix: usize) -> Walk {
    let mut r = Rig::new(card(text, true));
    r.configure(idx, 0);
    r.set_text_width(width);
    r.set_stage_prefix(prefix);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    let mut pages = Vec::new();
    loop {
        let page = r.page();
        pages.push(r.lines());
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready);
        if r.page() == page {
            break;
        }
    }
    Walk {
        offsets: r.page_offsets(),
        pages,
        sd_reads: r.storage().read_count(),
    }
}

#[test]
fn every_prefix_lays_out_exactly_like_a_fully_staged_window() {
    for (name, text) in fixtures() {
        for idx in [0u8, 2, 4] {
            for width in [180u32, 400] {
                let full = walk(&text, idx, width, usize::MAX);
                assert!(
                    full.pages.len() > 3,
                    "{name} idx={idx} w={width}: fixture must span several pages"
                );
                for prefix in [1024usize, 400, 200, 64, 16, 1] {
                    let got = walk(&text, idx, width, prefix);
                    assert_eq!(
                        got.offsets, full.offsets,
                        "{name} idx={idx} w={width} prefix={prefix}: page offsets"
                    );
                    assert_eq!(
                        got.pages.len(),
                        full.pages.len(),
                        "{name} idx={idx} w={width} prefix={prefix}: page count"
                    );
                    for (p, (a, b)) in got.pages.iter().zip(&full.pages).enumerate() {
                        assert_eq!(
                            a, b,
                            "{name} idx={idx} w={width} prefix={prefix}: page {p} lines"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn staging_a_prefix_reads_no_more_than_the_lookups_and_one_pack_header_per_page() {
    // the same scalars get looked up (each once), only later, a page at a time:
    // each such staging opens the pack again, which reads its 44 byte header
    // (once per pixel size a page uses: body and heading)
    for (name, text) in fixtures() {
        let full = walk(&text, 2, 400, usize::MAX);
        let prefix = walk(&text, 2, 400, 1024);
        let slack = 2 * full.pages.len();
        assert!(
            prefix.sd_reads <= full.sd_reads + slack,
            "{name}: {} reads with a prefix, {} without (+ at most {slack} headers)",
            prefix.sd_reads,
            full.sd_reads
        );
    }
}

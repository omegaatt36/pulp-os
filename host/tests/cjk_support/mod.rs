//! Literal pack fixtures and observations; all wrapping/drawing stays in ReaderApp.
#![allow(dead_code)]

use pulp_host::reader::{Phase, Rig};
use pulp_host::render::{Framebuffer, render_full};
use pulp_host::storage::VirtualStorage;

pub const BOOK: &str = "CJK.TXT";
pub const SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];
pub const BODY: [u16; 5] = [16, 19, 23, 28, 35];
pub const HEADING: [u16; 5] = [23, 27, 32, 38, 46];
pub const SAMPLE: &str = "臺灣「繁體中文」，𠮷。";
pub const NO_HEAD: &str = "，。、？！）」』】";
pub const NO_TAIL: &str = "（「『【";

pub fn alphabet() -> Vec<char> {
    let mut c: Vec<char> = "A臺灣繁體中文𠮷，。、？！）」』】（「『【"
        .chars()
        .collect();
    c.sort_unstable();
    c.dedup();
    c
}

pub fn rows(px: u16, c: char) -> [u8; 3] {
    [0x81, px as u8, c as u32 as u8]
}

fn u16le(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}

/// v1 bytes by hand: 44-byte header, 22-byte sorted records, three bitmap bytes each.
/// Glyph metrics: advance px+1, bearing (0,-3), width 8, height 3.
/// The optional wide glyph proves the integration cannot narrow advance to u8.
pub fn pack(px: u16, wide: bool) -> Vec<u8> {
    let chars = alphabet();
    let n = chars.len() as u32;
    let base = 44 + 22 * n;
    let region = 3 * n;
    let mut v = Vec::new();
    v.extend_from_slice(b"PFNT");
    u16le(&mut v, 1);
    u16le(&mut v, px);
    v.extend_from_slice(&(0xABCD_0000_0000_0000u64 + px as u64).to_le_bytes());
    u16le(&mut v, px + 4);
    u16le(&mut v, px);
    for x in [n, 44, 22 * n, base, region, base + region] {
        u32le(&mut v, x);
    }
    assert_eq!(v.len(), 44);
    for (i, &c) in chars.iter().enumerate() {
        u32le(&mut v, c as u32);
        u32le(&mut v, 3 * i as u32);
        u32le(&mut v, 3);
        u16le(&mut v, if wide && c != 'A' { 301 } else { px + 1 });
        u16le(&mut v, 0);
        u16le(&mut v, (-3i16) as u16);
        u16le(&mut v, 8);
        u16le(&mut v, 3);
    }
    for c in chars {
        v.extend_from_slice(&rows(px, c));
    }
    assert_eq!(v.len(), (base + region) as usize);
    v
}

pub fn path(px: u16) -> String {
    format!("_PULP/FONTS/F{px:05}.PFN")
}

pub fn install(card: &VirtualStorage, px: u16, bytes: &[u8]) {
    card.ensure_pulp_dir().unwrap();
    card.ensure_pulp_subdir("FONTS").unwrap();
    card.write_in_pulp_subdir("FONTS", &format!("F{px:05}.PFN"), bytes)
        .unwrap();
}

pub fn card(text: &[u8], packs: bool) -> VirtualStorage {
    let c = VirtualStorage::memory_with(&[(BOOK, text)]);
    c.ensure_pulp_dir().unwrap();
    if packs {
        for px in SIZES {
            install(&c, px, &pack(px, false));
        }
    }
    c
}

pub fn rig(text: &[u8], idx: u8, width: u32) -> Rig {
    let mut r = Rig::new(card(text, true));
    r.configure(idx, 0);
    r.set_text_width(width);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    assert_eq!(r.text_w(), width, "geometry override survives reader entry");
    r
}

pub fn text_lines(r: &Rig) -> Vec<String> {
    r.lines()
        .into_iter()
        .map(|b| {
            let mut visible = Vec::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == 1 && i + 1 < b.len() {
                    i += 2;
                } else {
                    visible.push(b[i]);
                    i += 1;
                }
            }
            String::from_utf8(visible).expect("every line ends on a scalar boundary")
        })
        .collect()
}

/// Locate the literal glyph at its literal horizontal bearing. Vertical placement
/// follows the reader's baseline policy and is not used as a metrics oracle.
pub fn assert_patch(r: &Rig, x: u16, width: usize, bitmap: &[u8], height: usize) {
    assert_patch_on_line(r, 0, x, width, bitmap, height);
}

pub fn assert_patch_on_line(
    r: &Rig,
    line: u16,
    x: u16,
    width: usize,
    bitmap: &[u8],
    height: usize,
) {
    let f = render_full(&|s| r.draw(s)).frame;
    let top = r.text_y() + line * r.font_line_h();
    let end = (top + r.font_line_h()).min(799 - height as u16);
    assert!(
        (top..=end).any(|y| patch(&f, x, y, width, bitmap, height)),
        "literal {width}x{height} glyph missing at x={x} in line {line}"
    );
}

fn patch(f: &Framebuffer, x: u16, y: u16, w: usize, bm: &[u8], h: usize) -> bool {
    let stride = w.div_ceil(8);
    (0..h).all(|dy| {
        (0..w).all(|dx| {
            let ink = bm[dy * stride + dx / 8] & (0x80 >> (dx % 8)) != 0;
            f.is_black(x + dx as u16, y + dy as u16) == ink
        })
    })
}

pub fn assert_kinsoku(lines: &[String]) {
    for line in lines.iter().filter(|s| !s.is_empty()) {
        assert!(
            !NO_HEAD.contains(line.chars().next().unwrap()),
            "forbidden head: {line:?}"
        );
        assert!(
            !NO_TAIL.contains(line.chars().last().unwrap()),
            "forbidden tail: {line:?}"
        );
    }
}

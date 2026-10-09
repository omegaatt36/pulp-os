use crate::cjk_support;

use cjk_support::{SIZES, alphabet, install, pack, rows};
use pulp_host::apps::widgets::BitmapLabel;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
};
use pulp_host::fonts::cjk::{SurfaceFonts, VisibleText};
use pulp_host::fonts::{self};
use pulp_host::kernel::Kernel;
use pulp_host::reader::{Phase, Rig};
use pulp_host::render::{Framebuffer, render_full};
use pulp_host::storage::VirtualStorage;
use pulp_host::ui::{Alignment, Region};

const BOOK: &str = "FIXES.EPUB";

fn epub(title: &str) -> Vec<u8> {
    build_epub(&EpubSpec {
        version: EpubVersion::V3,
        title: title.into(),
        author: "Fixture".into(),
        identifier: "surface-fixes".into(),
        chapters: (0..2)
            .map(|_| Chapter {
                title: "Latin heading".into(),
                blocks: vec![Block::Paragraph(vec![Run::Text("Latin body.".into())])],
            })
            .collect(),
        toc: vec![TocItem {
            title: "Latin TOC".into(),
            chapter: 1,
            children: vec![],
        }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    })
    .unwrap()
}

// ---------------------------------------------------------------------------
// T-A: a font failure raised by the pre-render preparation hook must leave the
// reader the way every other reader failure does: error screen up, loading
// overlay cleared, page region marked for redraw.
// ---------------------------------------------------------------------------

#[test]
fn corrupt_installed_title_pack_during_load_ends_in_error_with_loading_cleared_and_redraw_requested()
 {
    let bytes = epub("臺灣𠮷");
    let card = VirtualStorage::memory_with(&[(BOOK, bytes.as_slice())]);
    card.ensure_pulp_dir().unwrap();
    // installed 16px chrome pack (the title surface size) that is pure garbage
    install(&card, 16, &[0xA5; 200]);

    let mut r = Rig::new(card);
    r.configure(0, 0);
    r.enter(BOOK);
    assert!(
        r.loading_active(),
        "entering a book shows the loading overlay"
    );

    // The scheduler renders between background ticks: after every tick the
    // pending redraw is consumed by the frame, then prepare_render runs.
    for _ in 0..400 {
        r.take_redraw();
        r.prepare_render();
        if r.phase() != Phase::Loading {
            break;
        }
        r.idle(1);
    }

    assert_eq!(
        r.phase(),
        Phase::Error,
        "an installed but corrupt title pack is a font preparation failure"
    );
    assert!(
        r.error_kind().is_some(),
        "the failure is reported through the reader's error"
    );
    let (loading, redraw) = (r.loading_active(), r.has_redraw());
    assert!(
        !loading && redraw,
        "after the font failure: loading_active={loading} (must be false, the overlay must not \
         stay painted over the error), redraw_requested={redraw} (must be true, the error \
         screen has to be painted)"
    );
}

// ---------------------------------------------------------------------------
// T-B: preparing the same visible text at the same size again must not read
// the SD font pack again; a real change of text or size must.
// ---------------------------------------------------------------------------

fn full_card() -> VirtualStorage {
    let card = VirtualStorage::memory_with(&[]);
    card.ensure_pulp_dir().unwrap();
    for px in SIZES {
        install(&card, px, &pack(px, false));
    }
    card
}

fn bytes_read(card: &VirtualStorage) -> usize {
    card.read_log().iter().map(|r| r.returned).sum()
}

fn small_glyph_at(f: &Framebuffer, x: u16, top: u16, end: u16, px: u16, ch: char) -> bool {
    (top..end.saturating_sub(2)).any(|y| {
        rows(px, ch).iter().enumerate().all(|(dy, row)| {
            (0..8).all(|dx| f.is_black(x + dx, y + dy as u16) == (row & (0x80 >> dx) != 0))
        })
    })
}

#[test]
fn unchanged_text_and_size_do_not_reread_the_pack_but_changes_do() {
    assert!(alphabet().contains(&'臺') && alphabet().contains(&'𠮷'));
    let mut kernel = Kernel::new(SdStorage::new(full_card()));
    let mut surface = SurfaceFonts::new();
    let region = Region::new(16, 100, 300, 50);

    let text = |idx: u8, s: &str| {
        let mut v = VisibleText::new();
        v.add(s, fonts::body_font(idx), false);
        v
    };
    let draw = |surface: &SurfaceFonts, idx: u8, s: &str| {
        let view = surface.view();
        let label =
            BitmapLabel::new(region, s, fonts::body_font(idx)).alignment(Alignment::CenterLeft);
        render_full(&|st| label.draw_prepared(st, &view).unwrap()).frame
    };

    // 1. first preparation reads the pack and draws the literal 16px glyphs
    surface.set_size(0);
    kernel.sd().card.reset_reads();
    surface.prepare(&mut kernel.handle(), &text(0, "臺灣"));
    assert!(surface.error.is_none(), "first prepare succeeds");
    assert!(
        kernel.sd().card.read_count() > 0 && bytes_read(&kernel.sd().card) > 0,
        "first prepare must read the installed pack"
    );
    let first = draw(&surface, 0, "臺灣");
    let (top, end) = (region.y, region.y + region.h);
    assert!(small_glyph_at(&first, 16, top, end, 16, '臺'));
    assert!(small_glyph_at(&first, 33, top, end, 16, '灣'));

    // 2. identical text, identical size: zero SD reads, identical pixels
    kernel.sd().card.reset_reads();
    surface.prepare(&mut kernel.handle(), &text(0, "臺灣"));
    assert!(surface.error.is_none());
    assert_eq!(
        (kernel.sd().card.read_count(), bytes_read(&kernel.sd().card)),
        (0, 0),
        "re-preparing unchanged text at an unchanged size must not touch the SD card"
    );
    let second = draw(&surface, 0, "臺灣");
    assert_eq!(
        first.to_pbm(),
        second.to_pbm(),
        "unchanged prepare keeps the pixels"
    );
    assert!(small_glyph_at(&second, 16, top, end, 16, '臺'));
    assert!(small_glyph_at(&second, 33, top, end, 16, '灣'));

    // 3. one new scalar appears: the pack is read again and the glyph draws
    kernel.sd().card.reset_reads();
    surface.prepare(&mut kernel.handle(), &text(0, "臺灣𠮷"));
    assert!(surface.error.is_none());
    assert!(
        kernel.sd().card.read_count() > 0,
        "changed visible text must be prepared from the pack"
    );
    let third = draw(&surface, 0, "臺灣𠮷");
    assert!(small_glyph_at(&third, 16, top, end, 16, '臺'));
    assert!(small_glyph_at(&third, 33, top, end, 16, '灣'));
    assert!(small_glyph_at(&third, 50, top, end, 16, '𠮷'));

    // 4. same text, different size (19px body): prepared again at that size
    surface.set_size(1);
    kernel.sd().card.reset_reads();
    surface.prepare(&mut kernel.handle(), &text(1, "臺灣𠮷"));
    assert!(surface.error.is_none());
    assert!(
        kernel.sd().card.read_count() > 0,
        "a different size must be prepared from that size's pack"
    );
    let fourth = draw(&surface, 1, "臺灣𠮷");
    assert!(small_glyph_at(&fourth, 16, top, end, 19, '臺'));
    assert!(small_glyph_at(&fourth, 36, top, end, 19, '灣'));
    assert!(small_glyph_at(&fourth, 56, top, end, 19, '𠮷'));
}

// ---------------------------------------------------------------------------
// T-C: a visible surface must hold a realistic page of distinct CJK glyphs:
// 150 distinct scalars at the 35px body size.
// ---------------------------------------------------------------------------

const PX: u16 = 35;
const N: u32 = 150;
const FIRST: u32 = 0x4E00;
const PER_ROW: usize = 10;
const ROW_H: u16 = 50;
// 35 columns need 5 bytes per row, 35 rows: 175 bytes per glyph bitmap
const STRIDE: usize = 5;
const GLYPH_BYTES: usize = STRIDE * PX as usize;

fn big_rows(cp: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(GLYPH_BYTES);
    for r in 0..PX as u8 {
        // distinct per scalar and per row; last byte uses only the top 3 bits
        v.extend_from_slice(&[cp as u8, (cp >> 8) as u8 ^ 0x5A, r | 0x80, 0xA5 ^ r, 0xE0]);
    }
    assert_eq!(v.len(), GLYPH_BYTES);
    v
}

/// Same v1 layout as cjk_support::pack (44-byte header, 22-byte sorted records),
/// with N consecutive scalars whose bitmaps are real 35x35 size.
fn big_pack() -> Vec<u8> {
    let base = 44 + 22 * N;
    let region = GLYPH_BYTES as u32 * N;
    let mut v = Vec::new();
    v.extend_from_slice(b"PFNT");
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&PX.to_le_bytes());
    v.extend_from_slice(&(0xABCD_0000_0000_0000u64 + PX as u64).to_le_bytes());
    v.extend_from_slice(&(PX + 4).to_le_bytes());
    v.extend_from_slice(&PX.to_le_bytes());
    for x in [N, 44, 22 * N, base, region, base + region] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    assert_eq!(v.len(), 44);
    for i in 0..N {
        v.extend_from_slice(&(FIRST + i).to_le_bytes());
        v.extend_from_slice(&(GLYPH_BYTES as u32 * i).to_le_bytes());
        v.extend_from_slice(&(GLYPH_BYTES as u32).to_le_bytes());
        v.extend_from_slice(&(PX + 1).to_le_bytes()); // advance 36
        v.extend_from_slice(&0u16.to_le_bytes()); // bearing x
        v.extend_from_slice(&((-30i16) as u16).to_le_bytes()); // bearing y
        v.extend_from_slice(&PX.to_le_bytes()); // width 35
        v.extend_from_slice(&PX.to_le_bytes()); // height 35
    }
    for i in 0..N {
        v.extend_from_slice(&big_rows(FIRST + i));
    }
    assert_eq!(v.len(), (base + region) as usize);
    v
}

fn big_glyph_at(f: &Framebuffer, x: u16, top: u16, end: u16, cp: u32) -> bool {
    let bm = big_rows(cp);
    (top..=end - PX).any(|y| {
        (0..PX as usize).all(|dy| {
            (0..PX as usize).all(|dx| {
                let ink = bm[dy * STRIDE + dx / 8] & (0x80 >> (dx % 8)) != 0;
                f.is_black(x + dx as u16, y + dy as u16) == ink
            })
        })
    })
}

#[test]
fn a_page_of_150_distinct_35px_glyphs_prepares_and_draws_from_the_pack() {
    let card = VirtualStorage::memory_with(&[]);
    card.ensure_pulp_dir().unwrap();
    install(&card, PX, &big_pack());
    let mut kernel = Kernel::new(SdStorage::new(card));

    let font = fonts::body_font(4);
    let texts: Vec<String> = (0..N as usize / PER_ROW)
        .map(|row| {
            (0..PER_ROW)
                .map(|i| char::from_u32(FIRST + (row * PER_ROW + i) as u32).unwrap())
                .collect()
        })
        .collect();
    let mut visible = VisibleText::new();
    for t in &texts {
        visible.add(t, font, false);
    }

    let mut surface = SurfaceFonts::new();
    surface.set_size(4);
    surface.prepare(&mut kernel.handle(), &visible);
    assert!(
        surface.error.is_none(),
        "150 distinct 35px glyphs ({} bitmap bytes) are one realistic page; got {:?}",
        GLYPH_BYTES * N as usize,
        surface.error.as_ref().map(|e| e.kind())
    );

    let view = surface.view();
    let labels: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(row, t)| {
            BitmapLabel::new(Region::new(0, row as u16 * ROW_H, 480, ROW_H), t, font)
                .alignment(Alignment::CenterLeft)
        })
        .collect();
    kernel.sd().card.reset_reads();
    let f = render_full(&|s| {
        for l in &labels {
            l.draw_prepared(s, &view).unwrap();
        }
    })
    .frame;
    assert_eq!(kernel.sd().card.read_count(), 0, "draw reads nothing");

    // every glyph of the page is the literal pack bitmap at advance 36
    // (never the synthetic missing box, whose rows cannot equal the fingerprint)
    for row in 0..N as usize / PER_ROW {
        let top = row as u16 * ROW_H;
        for i in 0..PER_ROW {
            let cp = FIRST + (row * PER_ROW + i) as u32;
            assert!(
                big_glyph_at(&f, i as u16 * (PX + 1), top, top + ROW_H, cp),
                "U+{cp:04X} (row {row}, column {i}) is not drawn from the pack"
            );
        }
    }
}

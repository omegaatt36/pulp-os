// reviewed golden images of prepared Iansui pages (R2) and full-frame vs
// partial-window equality of the same prepared page (R9)
//
// goldens: pages 1 and 2 of fixtures/iansui-sample.txt, set with
// fixtures/IANSUI24.PFP and drawn through render_full, compared byte for
// byte with render/tests/golden/iansui-page{1,2}.pbm. each golden is the
// whole physical panel frame as binary PBM (P4, 1 = black, 48 KB): the
// full frame, not a crop of the text block, so ink anywhere on the panel
// (stray or missing) is part of the comparison
//
// review record, 2026-09-24: the PNG copies of both tracked PBMs were
// inspected in this branch review against fixtures/iansui-sample.txt.
// page 1 shows the CJK punctuation pairs intact; page 2 shows the visible
// square for absent 한 and the kana line. SHA-256 of the PBM files:
//   iansui-page1.pbm 64549847ea7b8c5d99af1de717de1737fb6af7291b044861d3af7e9cea95190a
//   iansui-page2.pbm 055ca0ba8ca9bf200aa50a6a28f59664130ae176c3bb44cf27800b0d3c2f0148
// these hashes identify the reviewed bytes; they do not replace visual
// inspection when a golden is regenerated
//
// oracle: a golden is generated output; what makes it an oracle is human
// review of its PNG copy, written on every run to
// $CARGO_TARGET_TMPDIR/golden-review/iansui-page{1,2}.png (under
// `cargo host-test`: target/<host triple>/tmp/) in logical portrait
// orientation (text upright). goldens are rewritten only on request:
//
//     PULP_UPDATE_GOLDEN=1 cargo host-test
//
// the full-vs-partial tests below do not use the goldens: they compare
// two renders of one prepared page against each other, inside windows
// whose physical rectangles are derived by hand from the Deg270 rule
// (see strip_render.rs):
//   logical (x, y, w, h) -> physical (y, 480 - x - w, h, w)
// byte-alignment padding outside a requested window is forced white by
// the driver (the harness mirrors it) and is not compared

mod common;

use std::path::PathBuf;

use common::TestReader;
use common::strip::{Frame, assert_area_eq, render_full, render_partial};
use pulp_render::font_pack::FontPack;
use pulp_render::layout::{LineSpan, Markup, WrapParams, line_glyphs, wrap};
use pulp_render::page::{PackMeasure, PageGeometry, PageGlyphs};
use pulp_render::panel::Rotation;
use pulp_render::strip::StripBuffer;

const PX: u16 = 24;
// portrait page: 20 px side margins on the 480 px logical width, text
// from logical y 40. six lines per page, fewer than the panel holds, so
// the one-paragraph sample spans two pages
const LEFT: i32 = 20;
const TOP: i32 = 40;
const MAX_WIDTH: u32 = 440;
const MAX_LINES: usize = 6;
const ROT: Rotation = Rotation::Deg270;

const UPDATE_VAR: &str = "PULP_UPDATE_GOLDEN";

// the sample contains no markup; any byte values would do
const MARKUP: Markup = Markup {
    marker: 0x01,
    img_ref: b'P',
    bold_on: b'B',
    bold_off: b'b',
    italic_on: b'I',
    italic_off: b'i',
    heading_on: b'H',
    heading_off: b'h',
    quote_on: b'Q',
    quote_off: b'q',
};

type Cache = PageGlyphs<96, 8192>;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// one laid-out page of the sample, prepared for drawing
struct Page {
    text: Vec<u8>,
    lines: Vec<LineSpan>,
    cache: Box<Cache>,
    geom: PageGeometry,
}

impl Page {
    fn draw(&self, s: &mut StripBuffer) {
        self.cache
            .draw(s, &self.text, &self.lines, MARKUP, &self.geom)
            .unwrap();
    }

    fn chars(&self) -> String {
        self.lines
            .iter()
            .flat_map(|span| line_glyphs(&self.text, span, MARKUP).map(|(ch, _)| ch))
            .collect()
    }
}

// lookup -> wrap -> prepare for every page of the sample
fn pages() -> Vec<Page> {
    let mut r = TestReader::new(fixture("IANSUI24.PFP"));
    let pack = FontPack::load(&mut r, PX).expect("the tracked pack loads");
    let text = fixture("iansui-sample.txt");
    let params = WrapParams {
        markup: MARKUP,
        max_lines: MAX_LINES,
        max_width_px: MAX_WIDTH,
        indent_px: 0,
        img_heights: &[],
        default_img_h: 0,
    };
    let geom = PageGeometry {
        left: LEFT,
        top: TOP,
        line_height: pack.line_height() as i32,
        ascent: pack.ascent() as i32,
        indent_px: 0,
    };

    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut lines = vec![LineSpan::EMPTY; MAX_LINES];
        let wrapped = wrap(
            &text[start..],
            true,
            &mut PackMeasure::new(&pack, &mut r),
            &params,
            &mut lines,
        )
        .unwrap();
        assert!(wrapped.consumed > 0, "page at byte {start} makes progress");
        lines.truncate(wrapped.line_count);
        let page = text[start..start + wrapped.consumed].to_vec();
        let mut cache = Box::new(Cache::new());
        cache.prepare(&pack, &mut r, &page, &lines, MARKUP).unwrap();
        out.push(Page {
            text: page,
            lines,
            cache,
            geom,
        });
        start += wrapped.consumed;
    }
    out
}

fn scratch(dir: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

// grayscale 1-bit PNG of the frame as the reader sees it: Deg270 maps
// logical (lx, ly) to physical (ly, 479 - lx), logical 480 x 800
fn write_png(frame: &Frame, path: &PathBuf) {
    const LW: u32 = 480;
    const LH: u32 = 800;
    let row_bytes = LW as usize / 8;
    let mut raw = Vec::with_capacity((row_bytes + 1) * LH as usize);
    for ly in 0..LH as u16 {
        raw.push(0); // filter: none
        for bx in 0..row_bytes as u16 {
            let mut byte = 0u8;
            for bit in 0..8 {
                let lx = bx * 8 + bit;
                if !frame.is_black(ly, 479 - lx) {
                    byte |= 0x80 >> bit; // PNG gray 1 = white
                }
            }
            raw.push(byte);
        }
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = common::crc32(&out[start..]); // PNG uses CRC-32/ISO-HDLC
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&LW.to_be_bytes());
    ihdr.extend_from_slice(&LH.to_be_bytes());
    ihdr.extend_from_slice(&[1, 0, 0, 0, 0]); // depth 1, gray, deflate, no filter/interlace
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(
        &mut png,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 9),
    );
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).expect("write PNG");
}

// compare `actual` with the tracked golden `name`, or rewrite the golden
// when PULP_UPDATE_GOLDEN=1. always leaves a PNG of the golden for review
fn check_golden(name: &str, actual: &Frame) {
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.pbm"));
    let review = scratch("golden-review");
    let update = "PULP_UPDATE_GOLDEN=1 cargo host-test";

    if std::env::var(UPDATE_VAR).as_deref() == Ok("1") {
        std::fs::create_dir_all(golden.parent().unwrap()).expect("create golden dir");
        std::fs::write(&golden, actual.to_pbm()).expect("write golden");
        write_png(actual, &review.join(format!("{name}.png")));
        eprintln!("{name}: golden rewritten: {}", golden.display());
        return;
    }

    let actual_png = review.join(format!("{name}-actual.png"));
    let Ok(bytes) = std::fs::read(&golden) else {
        write_png(actual, &actual_png);
        panic!(
            "{name}: no golden at {}\n  review the render: {}\n  if it is right, record it with: {update}",
            golden.display(),
            actual_png.display()
        );
    };
    let expected = Frame::from_pbm(&bytes)
        .unwrap_or_else(|| panic!("{}: not an 800x480 P4 PBM", golden.display()));
    write_png(&expected, &review.join(format!("{name}.png")));
    let diff_png = review.join(format!("{name}-diff.png"));
    if expected == *actual {
        // drop review copies left by an earlier failing run
        let _ = std::fs::remove_file(&actual_png);
        let _ = std::fs::remove_file(&diff_png);
        return;
    }

    let diff = expected.diff(actual);
    let failed = scratch("golden-failed");
    let actual_pbm = failed.join(format!("{name}-actual.pbm"));
    let diff_pbm = failed.join(format!("{name}-diff.pbm"));
    std::fs::write(&actual_pbm, actual.to_pbm()).expect("write actual");
    std::fs::write(&diff_pbm, diff.to_pbm()).expect("write diff");
    write_png(actual, &actual_png);
    write_png(&diff, &diff_png);
    panic!(
        "{name}: {} pixel(s) differ from the golden {}\n  \
         golden: {}\n  actual: {}\n  diff (black = differs): {}\n  \
         physical frames: {} {}\n  \
         if the new render is right after review, record it with: {update}",
        diff.black_pixels().len(),
        golden.display(),
        review.join(format!("{name}.png")).display(),
        actual_png.display(),
        diff_png.display(),
        actual_pbm.display(),
        diff_pbm.display(),
    );
}

#[test]
fn page1_with_unsplit_pairs_matches_reviewed_golden() {
    let pages = pages();
    // the golden shows the unsplittable …… and —— pairs set mid-line
    let chars = pages[0].chars();
    for want in ["……", "——"] {
        assert!(chars.contains(want), "page 1 lacks {want}: {chars}");
    }
    let frame = render_full(ROT, &|s: &mut StripBuffer| pages[0].draw(s));
    check_golden("iansui-page1", &frame);
}

#[test]
fn page2_with_fallback_matches_reviewed_golden() {
    let pages = pages();
    assert_eq!(pages.len(), 2, "six-line pages split the sample in two");
    // the golden shows the fallback □ drawn for 한, and kana
    let chars = pages[1].chars();
    for want in ["한", "ひらがな", "カタカナ"] {
        assert!(chars.contains(want), "page 2 lacks {want}: {chars}");
    }
    let frame = render_full(ROT, &|s: &mut StripBuffer| pages[1].draw(s));
    check_golden("iansui-page2", &frame);
}

// a partial window over page 1 (line tops at logical y 40 + 27 k, k < 6;
// ink from logical x 20 to ~460). phys is the requested window derived
// by hand from logical with (y, 480 - x - w, h, w); aligned is the byte-
// aligned RAM window the driver writes: px = x & !7, pw rounded up to
// whole bytes, masks covering the padding bits (MSB = leftmost pixel)
struct Window {
    name: &'static str,
    logical: (u16, u16, u16, u16),
    phys: (u16, u16, u16, u16),
    // (px, pw, left_mask, right_mask)
    aligned: (u16, u16, u8, u8),
}

const WINDOWS: [Window; 5] = [
    // byte-aligned: physical x 48 and width 64 are whole bytes, no masks
    Window {
        name: "aligned",
        logical: (40, 48, 200, 64),
        phys: (48, 240, 64, 200),
        aligned: (48, 64, 0x00, 0x00),
    },
    // unaligned, all four edges through glyphs of lines 0..=3 (logical x
    // 37 and 200 are inside the 1st and 8th em cells, y 53 and 129 inside
    // lines 0 and 3): physical x 53..130 pads 5 bits left (48..53) and 6
    // right (130..136)
    Window {
        name: "unaligned",
        logical: (37, 53, 164, 77),
        phys: (53, 279, 77, 164),
        aligned: (48, 88, 0xF8, 0x3F),
    },
    // physical rows 10..100 straddle the full-frame strip boundaries
    // 39|40 and 79|80 (logical x 440|439 and 400|399); x 60..160 pads 4
    // bits left
    Window {
        name: "strip-straddle",
        logical: (380, 60, 90, 100),
        phys: (60, 10, 100, 90),
        aligned: (56, 104, 0xF0, 0x00),
    },
    // one whole text line (line 2, logical y 94..121) across the full
    // logical width: physical x 94..121 pads 6 bits left, 7 right
    Window {
        name: "full-line",
        logical: (0, 94, 480, 27),
        phys: (94, 0, 27, 480),
        aligned: (88, 40, 0xFC, 0x7F),
    },
    // the whole text block (lines 0..=5, logical y 40..202): 21-byte rows
    // allow 4000 / 21 = 190 rows per chunk, so the driver writes physical
    // rows 0..190, 190..380, 380..480 in three chunks; pads 6 bits right
    Window {
        name: "text-block",
        logical: (0, 40, 480, 162),
        phys: (40, 0, 162, 480),
        aligned: (40, 168, 0x00, 0x3F),
    },
];

// R9: for each base the panel may hold, a partial pass over each window
// matches the full-frame pass of the same prepared page inside the
// requested window
#[test]
fn partial_windows_match_the_full_render() {
    let pages = pages();
    let draw = |s: &mut StripBuffer| pages[0].draw(s);
    let full = render_full(ROT, &draw);
    let other = render_full(ROT, &|s: &mut StripBuffer| pages[1].draw(s));
    let bases = [
        ("blank", Frame::blank()),
        ("same-page", full.clone()),
        ("page-turn", other),
    ];

    for w in &WINDOWS {
        let (x, y, ww, wh) = w.logical;
        let (px, py, pw, ph) = w.phys;
        assert!(
            !full.black_pixels_in(px, py, pw, ph).is_empty(),
            "{}: window holds ink",
            w.name
        );
        for (base_name, base) in &bases {
            let (partial, rs) = render_partial(base, ROT, x, y, ww, wh, &draw)
                .unwrap_or_else(|| panic!("{}: window is not empty", w.name));
            assert_eq!(
                (rs.px, rs.py, rs.pw, rs.ph, rs.left_mask, rs.right_mask),
                (w.aligned.0, py, w.aligned.1, ph, w.aligned.2, w.aligned.3),
                "{}: aligned RAM window",
                w.name
            );
            let name = format!("r9-{}-{base_name}", w.name);
            assert_area_eq(&name, &full, &partial, px, py, pw, ph);
        }
    }
}

// the windows above test what they claim: the unaligned one's four edges
// cut through ink, and the straddling one has ink on both sides of each
// strip boundary it crosses
#[test]
fn windows_cut_through_glyphs_and_strips() {
    let pages = pages();
    let full = render_full(ROT, &|s: &mut StripBuffer| pages[0].draw(s));

    let (px, py, pw, ph) = WINDOWS[1].phys;
    let edges = [
        ("left", px, py, 1, ph),
        ("right", px + pw - 1, py, 1, ph),
        ("top", px, py, pw, 1),
        ("bottom", px, py + ph - 1, pw, 1),
    ];
    for (edge, x, y, w, h) in edges {
        assert!(
            !full.black_pixels_in(x, y, w, h).is_empty(),
            "unaligned window: no ink on its {edge} edge"
        );
    }

    let (px, _, pw, _) = WINDOWS[2].phys;
    for row in [39, 40, 79, 80] {
        assert!(
            !full.black_pixels_in(px, row, pw, 1).is_empty(),
            "strip-straddle window: no ink on physical row {row}"
        );
    }
}

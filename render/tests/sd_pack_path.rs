// the device's read path, exercised on the host
//
// everything else in these tests hands the loader a `SliceReader`: the whole
// pack is already in memory. That is not what the firmware does. On the
// device the pack is a file on the SD card and every read is a seek plus a
// FAT-sector copy (kernel/src/drivers/sd_pack.rs -> storage::read_file_chunk).
//
// this test runs the same page through a `SectorReader`, which serves reads
// from a real file in 512-byte clusters and caps every call at one sector,
// and requires the result to be byte-identical to the golden the in-memory
// path produced. It pins the three assumptions SdPackReader relies on:
//   - a pack is found by name alone, with no directory traversal
//   - a read may be short only at end of file
//   - glyph prewarm does one lookup per DISTINCT scalar, not per occurrence
//     (if this regresses, a page turn costs a 30 ms SD read per character)

mod common;

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use common::strip::Frame;
use pulp_render::font_pack::{FontPack, IoError, PackReader};
use pulp_render::layout::{LineSpan, Markup, WrapParams, line_glyphs, wrap};
use pulp_render::pack_file::parse_pack_name;
use pulp_render::page::{PageGeometry, PageGlyphs};
use pulp_render::panel::Rotation;
use pulp_render::strip::StripBuffer;

const PX: u16 = 24;
const PACK: &str = "IANSUI24.PFP";
const LEFT: i32 = 20;
const TOP: i32 = 40;
const MAX_WIDTH: u32 = 440;
const MAX_LINES: usize = 6;

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
const SECTOR: usize = 512;

/// A PackReader over a real file, served one FAT sector at a time.
///
/// Records every call so the tests can assert on the access pattern, not just
/// the output.
struct SectorReader {
    file: RefCell<File>,
    size: u32,
    /// (offset, len) of every read_at, in order
    reads: RefCell<Vec<(u32, usize)>>,
    /// cap every call to this many bytes; usize::MAX = unlimited
    max_read: usize,
}

impl SectorReader {
    fn open(path: &std::path::Path, max_read: usize) -> Self {
        let mut file = File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let size = file.seek(SeekFrom::End(0)).expect("seek end") as u32;
        file.seek(SeekFrom::Start(0)).expect("rewind");
        Self {
            file: RefCell::new(file),
            size,
            reads: RefCell::new(Vec::new()),
            max_read,
        }
    }

    fn read_count(&self) -> usize {
        self.reads.borrow().len()
    }
}

impl PackReader for SectorReader {
    fn size(&mut self) -> Result<u32, IoError> {
        Ok(self.size)
    }

    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, IoError> {
        let want = buf.len().min(self.max_read);
        self.reads.borrow_mut().push((offset, want));
        if want == 0 {
            return Ok(0);
        }
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| IoError::Io)?;
        // a read is short only at end of file; anything else is a real fault
        let n = file.read(&mut buf[..want]).map_err(|_| IoError::Io)?;
        Ok(n)
    }
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// The name the SD root listing yields, parsed rather than hardcoded, so this
// test covers the discovery step too: a pack on the card is found by name.
fn discovered_name() -> String {
    let parsed = parse_pack_name(PACK.as_bytes()).expect("the tracked pack name parses");
    let (buf, len) = parsed.file_name();
    String::from_utf8(buf[..len].to_vec()).expect("pack name is ASCII")
}

#[test]
fn the_pack_is_found_on_the_card_by_name_alone() {
    assert_eq!(discovered_name(), PACK);
}

#[test]
fn loads_and_renders_identically_through_sector_reads() {
    // one sector per call: the worst case a FAT short-name file can present
    let mut r = SectorReader::open(&fixture_path(PACK), SECTOR);
    let pack = FontPack::load(&mut r, PX).expect("the pack loads off the card");
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

    let mut lines = vec![LineSpan::EMPTY; MAX_LINES];
    let wrapped = wrap(
        &text,
        true,
        &mut pulp_render::page::PackMeasure::new(&pack, &mut r),
        &params,
        &mut lines,
    )
    .expect("wrap");
    lines.truncate(wrapped.line_count);
    let page = &text[..wrapped.consumed];

    let mut cache = Box::new(Cache::new());
    cache
        .prepare(&pack, &mut r, page, &lines, MARKUP)
        .expect("prepare");

    // the golden was produced by the in-memory SliceReader path in
    // iansui_golden.rs; a mismatch means the device path differs, and the
    // device is the one that matters
    let golden_bytes = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join("iansui-page1.pbm"),
    )
    .expect("the reviewed golden is tracked");
    let expected = Frame::from_pbm(&golden_bytes).expect("golden is a canonical 800x480 P4 PBM");

    let frame = common::strip::render_full(Rotation::Deg270, &|s: &mut StripBuffer| {
        cache.draw(s, page, &lines, MARKUP, &geom).expect("draw");
    });
    // Frame has no Debug; compare the serialisations so a failure prints bytes
    let actual_pbm = frame.to_pbm();
    let expected_pbm = expected.to_pbm();
    assert_eq!(
        actual_pbm,
        expected_pbm,
        "sector-read render differs from the reviewed golden ({} pixels differ)",
        frame.diff(&expected).black_pixels().len()
    );
}

#[test]
fn prewarm_reads_each_distinct_scalar_once() {
    let mut r = SectorReader::open(&fixture_path(PACK), usize::MAX);
    let pack = FontPack::load(&mut r, PX).expect("the pack loads off the card");
    let text = fixture("iansui-sample.txt");

    let params = WrapParams {
        markup: MARKUP,
        max_lines: MAX_LINES,
        max_width_px: MAX_WIDTH,
        indent_px: 0,
        img_heights: &[],
        default_img_h: 0,
    };
    let mut lines = vec![LineSpan::EMPTY; MAX_LINES];
    let wrapped = wrap(
        &text,
        true,
        &mut pulp_render::page::PackMeasure::new(&pack, &mut r),
        &params,
        &mut lines,
    )
    .expect("wrap");
    lines.truncate(wrapped.line_count);
    let page = &text[..wrapped.consumed];

    let distinct: std::collections::BTreeSet<char> = lines
        .iter()
        .flat_map(|span| line_glyphs(page, span, MARKUP).map(|(ch, _)| ch))
        .collect();
    let occurrences: usize = lines
        .iter()
        .flat_map(|span| line_glyphs(page, span, MARKUP))
        .count();

    // the sample repeats characters heavily; if the cache ever stopped
    // deduplicating, this is what would regress
    assert!(
        distinct.len() < occurrences,
        "sample has no repeated scalars, so this test proves nothing"
    );

    let before = r.read_count();
    let mut cache = Box::new(Cache::new());
    cache
        .prepare(&pack, &mut r, page, &lines, MARKUP)
        .expect("prepare");
    let during = r.read_count() - before;

    // a prewarm is one metrics lookup plus one bitmap read per distinct
    // scalar; allow a little slack for the shared-fallback path, but it must
    // scale with `distinct`, not with `occurrences`
    let budget = distinct.len() * 4;
    assert!(
        during <= budget,
        "prewarm issued {during} reads for {} distinct / {occurrences} occurrences \
         (budget {budget}); deduplication regressed",
        distinct.len()
    );
    assert_eq!(cache.glyph_count(), distinct.len());
}

#[test]
fn a_read_is_short_only_at_end_of_file() {
    let mut r = SectorReader::open(&fixture_path(PACK), usize::MAX);
    let mut buf = [0u8; 64];
    assert_eq!(r.size().unwrap(), 17_506, "fixture size");
    assert_eq!(r.read_at(0, &mut buf).unwrap(), 64);
    assert_eq!(
        r.read_at(17_506 - 32, &mut buf).unwrap(),
        32,
        "short at EOF"
    );
    assert_eq!(r.read_at(17_506, &mut buf).unwrap(), 0, "empty at EOF");
    assert_eq!(r.read_at(17_506 + 1, &mut buf).unwrap(), 0);
    // beyond the declared size is still not a panic
    assert_eq!(r.read_at(u32::MAX - 1, &mut buf).unwrap(), 0);
}

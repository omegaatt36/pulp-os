// onepage-host-validation / fixture validation -- distributable English TXT / EPUB2 / EPUB3
// fixtures (English reader regression: the fixtures the reader regression navigation / TOC / image / settings /
// bookmark regressions will run on).
//
// Run: scripts/host-test.sh --test fixtures
//
// fixture validation proves the fixtures are VALID, DETERMINISTIC and DISTRIBUTABLE books.
// The page-turn / TOC-navigation / settings / bookmark regressions themselves
// are reader regression and are NOT written here (the one reader-level check below, "a TXT
// fixture is more than one page", only establishes that a fixture is big
// enough to be worth paging).
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// host/Cargo.toml: `miniz_oxide` (already in Cargo.lock through smol-epub; no
// new crate) as a [dependencies] entry of pulp-host, with
// `default-features = false, features = ["with-alloc"]`: the builder needs
// `deflate::compress_to_vec` / `compress_to_vec_zlib`, the tests need
// `inflate::decompress_to_vec` / `decompress_to_vec_zlib`.
//
// host/src/lib.rs exposes `pub mod fixtures;` with exactly the items below
// (all spec types derive Debug, Clone, PartialEq, Eq; the enums also Copy
// where marked). No clock, no randomness, no I/O: every function is a pure
// function of its arguments.
//
//   // ---- TXT ------------------------------------------------------------
//   #[derive(Clone, Copy)] pub enum Newline { Lf, CrLf }
//   pub struct TxtSpec {
//       pub lines: Vec<String>,        // logical lines, no '\r' / '\n' inside; "" = blank line
//       pub newline: Newline,
//       pub trailing_newline: bool,
//   }
//   pub fn build_txt(spec: &TxtSpec) -> Vec<u8>
//       bytes = lines.join(eol) + (eol if trailing_newline), UTF-8, eol = "\n" | "\r\n".
//       `lines` empty -> empty Vec (whatever trailing_newline says).
//       Infallible.
//
//   // ---- EPUB spec ------------------------------------------------------
//   #[derive(Clone, Copy)] pub enum EpubVersion { V2, V3 }
//   #[derive(Clone, Copy)] pub enum Compression { Stored, Deflate, Mixed }
//   pub enum Run { Text(String), Bold(String), Italic(String), Break }
//   pub enum Block { Paragraph(Vec<Run>), Heading(String), Image(usize) }
//                                      // Image(i): index into EpubSpec::images
//   pub struct Chapter { pub title: String, pub blocks: Vec<Block> }
//   pub struct TocItem { pub title: String, pub chapter: usize, pub children: Vec<TocItem> }
//                                      // chapter: index into EpubSpec::chapters
//   #[derive(Clone, Copy)] pub enum Pattern {
//       Black, White,
//       Checker { cell: u16 },         // pixel (x,y) black iff (x/cell + y/cell) is even
//       HorizontalGradient,            // PNG: luminance x*255/(w-1) (0 when w == 1);
//                                      // JPEG: per 8x8 block, luminance bx*255/(bw-1), bx = x/8, bw = w/8
//   }
//   #[derive(Clone, Copy)] pub enum ImageKind { PngGray1, PngGray8, PngPalette8, Jpeg }
//   pub struct ImageSpec { pub path: String, pub kind: ImageKind, pub width: u16, pub height: u16, pub pattern: Pattern }
//                                      // path is relative to "OEBPS/", e.g. "images/cover.png"
//   pub struct EpubSpec {
//       pub version: EpubVersion,
//       pub title: String,             // <= 64 bytes (smol-epub TITLE_CAP)
//       pub author: String,            // <= 64 bytes (AUTHOR_CAP)
//       pub identifier: String,        // dc:identifier text
//       pub chapters: Vec<Chapter>,    // spine order == chapter order
//       pub toc: Vec<TocItem>,         // nested; production flattens it pre-order; may be empty
//       pub images: Vec<ImageSpec>,
//       pub cover: Option<usize>,      // index into images
//       pub compression: Compression,
//       pub numeric_entities: bool,
//   }
//   pub enum SpecError { NoChapters, EmptyChapter(usize), BadTocTarget(usize),
//                        BadImage(usize), BadImageRef(usize), TitleTooLong }
//   pub fn build_epub(spec: &EpubSpec) -> Result<Vec<u8>, SpecError>
//
// Spec hygiene (text the builder is allowed to be handed; the standard
// fixtures obey it, and the tests assert that they do):
//   * Run / Heading / chapter-title / TOC-title strings are one line (no
//     '\r' '\n' '\t'), and each paragraph's plain text (Break = '\n') has no
//     leading / trailing space, no double space, no space next to a Break, no
//     empty paragraph, no leading / trailing / doubled Break. (Reason: the
//     production html stripper collapses whitespace; a spec that already
//     obeys the normal form is its own oracle.)
//   * book title, author and TOC titles of the STANDARD fixtures contain none
//     of & < > " (the production OPF / NCX / nav readers do not decode
//     entities: see known_limitation_titles_are_not_entity_decoded_by_production).
//     The builder still ESCAPES them if a spec has them (negative tests).
//   * characters are printable ASCII plus ALLOWED_NON_ASCII below.
//
// Errors (spec must be checked up front; exactly one defect per test, so the
// precedence between defects is deliberately unspecified):
//   chapters empty                                   -> NoChapters
//   chapter i has no blocks                          -> EmptyChapter(i)
//   a TocItem (any depth) with chapter >= chapters.len()  -> BadTocTarget(that chapter value)
//   Block::Image(k) or cover == Some(k) with k >= images.len() -> BadImageRef(k)
//   image i: width or height 0; Checker{cell:0};
//            HorizontalGradient on PngGray1 / PngPalette8;
//            Jpeg with width or height not a multiple of 8, or Checker cell % 8 != 0
//                                                    -> BadImage(i)
//   title or author > 64 bytes, or any TOC title > 48 bytes (TOC_TITLE_CAP)
//   -- BYTES, not chars --                           -> TitleTooLong
//   exactly 64 / 64 / 48 bytes is accepted.
//   Special characters are NEVER an error: & < > " in any title / author /
//   identifier / text are escaped (below), the result is well-formed XML.
//
// ZIP layout, all EPUBs (the tests walk the bytes themselves):
//   * entries in this exact order:
//       0 mimetype
//       1 META-INF/container.xml
//       2 OEBPS/content.opf
//       3 OEBPS/toc.ncx (V2)  |  OEBPS/nav.xhtml (V3)
//       4.. OEBPS/text/chapterNN.xhtml, NN = 1-based, two digits (chapter0 -> "chapter01")
//       then one OEBPS/<ImageSpec::path> per image, in spec order
//   * local headers: general-purpose flags 0 (no data descriptor), no extra
//     field, DOS time 0x0000, DOS date 0x0021 (1980-01-01), sizes + CRC-32 in
//     the local header; central directory entry: same values, no extra, no
//     comment; entries tightly packed (entry i starts where entry i-1 ended,
//     the central directory starts where the last entry ended); EOCD last,
//     comment length 0, nothing after it.
//   * mimetype is ALWAYS method 0 (STORED), content exactly
//     "application/epub+zip" (no newline).
//   * Compression::Stored  : every entry method 0.
//     Compression::Deflate : every entry except mimetype method 8 (also the
//                            PNG / JPEG entries).
//     Compression::Mixed   : entry i >= 1 is method 8 iff i is odd, else 0.
//     The method is as stated even when deflating makes the entry bigger.
//     Method-8 data is a raw DEFLATE stream.
//
// Documents:
//   * container.xml: one <rootfile full-path="OEBPS/content.opf"
//     media-type="application/oebps-package+xml"/>.
//   * content.opf: V2 version="2.0", V3 version="3.0"; <dc:title>, <dc:creator>,
//     <dc:language>en</dc:language>, <dc:identifier>, and
//     <dc:rights>Synthetic test fixture generated by pulp-os; contains no third-party text.</dc:rights>;
//     V3 also carries a dcterms:modified of exactly 2000-01-01T00:00:00Z.
//     manifest: one <item id= href= media-type=> per non-mimetype / non-container
//     / non-opf entry (href relative to OEBPS/): NCX
//     "application/x-dtbncx+xml" (V2) or the nav document with
//     properties="nav" (V3); chapters "application/xhtml+xml"; images
//     "image/png" | "image/jpeg". spine: one <itemref idref=> per chapter in
//     order; V2's <spine toc="ID"> names the NCX item.
//     Cover: V2 -> <meta name="cover" content="ITEM-ID"/> where ITEM-ID is the
//     manifest id of the cover image; V3 -> that image's manifest item has
//     properties="cover-image". No cover -> neither.
//   * toc.ncx (V2): <navMap> of nested <navPoint><navLabel><text>TITLE</text>
//     </navLabel><content src="text/chapterNN.xhtml"/> ... </navPoint>.
//     nav.xhtml (V3): <nav epub:type="toc"> with nested <ol><li><a href=
//     "text/chapterNN.xhtml">TITLE</a> <ol>...</ol></li></ol>.
//   * chapterNN.xhtml: <head><title> = escaped chapter title; <body> =
//       <h1>CHAPTER TITLE</h1>, then one element per block:
//         Paragraph -> <p>runs</p> (Text = escaped text, Bold = <b>..</b>,
//                       Italic = <i>..</i>, Break = <br/>), no whitespace
//                       between the tags and the text they wrap
//         Heading   -> <h2>text</h2>
//         Image(i)  -> <p><img src="../PATH" alt=""/></p>
//   * Escaping (every XML document, every text node and attribute value):
//       & -> &amp;   < -> &lt;   > -> &gt;   " -> &quot;
//     numeric_entities == true additionally writes every non-ASCII character
//     of the chapter XHTML as a decimal reference &#N; (so those entries are
//     pure ASCII); false writes them as UTF-8.
//   * Every XML part is well-formed.
//
// Images (path inside the EPUB is "OEBPS/" + ImageSpec::path):
//   * PNG: signature, IHDR, [PLTE], one IDAT (zlib), IEND; no interlace; every
//     scanline filter byte 0 (None); every chunk CRC valid.
//       PngGray1    colour type 0, depth 1: bit 1 = white, 0 = black
//       PngGray8    colour type 0, depth 8: sample = luminance
//       PngPalette8 colour type 3, depth 8, PLTE = exactly (255,255,255),(0,0,0);
//                   index 0 = white, 1 = black
//     (black = luminance < 128; Black/White/Checker only ever produce 0 / 255)
//   * JPEG: baseline, SOF0, 8-bit, ONE component, SOI .. SOS .. EOI, at least
//     one DQT and one DC + one AC DHT, no restart markers, byte-stuffed
//     entropy data; decodable by smol_epub::jpeg.
//
// Standard set:
//   pub enum Spec { Txt(TxtSpec), Epub(EpubSpec) }
//   pub struct Fixture { pub name: &'static str, pub spec: Spec, pub bytes: Vec<u8> }
//   pub fn standard() -> Vec<Fixture>
//       exactly the 11 fixtures named in STANDARD_NAMES, in that order,
//       `bytes` == build_txt / build_epub(spec) for its own spec.
//   pub fn standard_card() -> pulp_host::storage::VirtualStorage
//       an in-memory card holding every standard fixture at the root under its
//       name, with `_PULP/` already created (the way the firmware boots it).
//
// What the tests treat as ground truth (never the builder's own output):
//   * the spec, through formulas written in this file;
//   * a small ZIP / XML / PNG / JPEG reader written in this file;
//   * the production smol-epub parser and decoders (smol_epub::zip / epub /
//     xml / cache / png / jpeg), which is what ReaderApp links.
// ============================================================================

use std::collections::BTreeSet;

use miniz_oxide::inflate::{decompress_to_vec, decompress_to_vec_zlib};
use pulp_host::fixtures::{
    self, Block, Chapter, Compression, EpubSpec, EpubVersion, Fixture, ImageKind, ImageSpec, Newline, Pattern, Run, Spec, SpecError,
    TocItem, TxtSpec, build_epub, build_txt,
};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::storage::DirEntry;
use smol_epub::DecodedImage;
use smol_epub::cache::{stream_strip_entry, strip_html_buf};
use smol_epub::epub::{self, EpubMeta, EpubSpine, EpubToc, TocSource};
use smol_epub::zip::{METHOD_DEFLATE, METHOD_STORED, ZipIndex, extract_entry};
use smol_epub::{jpeg, png, xml};

// ---------------------------------------------------------------------------
// constants (the contract's numbers)
// ---------------------------------------------------------------------------

const STANDARD_NAMES: [&str; 11] = [
    "PLAINLF.TXT",
    "PLAINCR.TXT",
    "NOEOL.TXT",
    "UTF8.TXT",
    "TINY.TXT",
    "E2STORED.EPU",
    "E2DEFL.EPU",
    "E2MIXED.EPU",
    "E3STORED.EPU",
    "E3DEFL.EPU",
    "E3MIXED.EPU",
];
const FIRST_EPUB: usize = 5;

// repo-bloat guards: the whole standard set stays below 1 MiB, one file below 256 KiB
const MAX_TOTAL_BYTES: usize = 1 << 20;
const MAX_FILE_BYTES: usize = 256 << 10;

// the only non-ASCII characters a fixture may contain (2-byte Latin-1 letters,
// 3-byte typographic punctuation)
const ALLOWED_NON_ASCII: &str = "\u{e9}\u{e8}\u{ea}\u{eb}\u{ef}\u{f1}\u{f6}\u{fc}\u{e7}\u{df}\u{e0}\u{2013}\u{2014}\u{2018}\u{2019}\u{201c}\u{201d}\u{2026}";

const RIGHTS: &str = "Synthetic test fixture generated by pulp-os; contains no third-party text.";
const MODIFIED: &str = "2000-01-01T00:00:00Z";
const TXT_PROVENANCE_PREFIX: &str = "Synthetic test fixture:";
const ID_PREFIX: &str = "urn:pulp-os:fixture:";

const DOS_TIME: u16 = 0x0000;
const DOS_DATE: u16 = 0x0021;

// smol-epub streams entries through 4 KB chunks (doc comments of
// cache::stream_strip_entry / png / jpeg); an entry of 3+ chunks crosses
// chunk boundaries inside both the stored and the deflate paths
const STREAM_CHUNK: usize = 4096;

// markers of smol_epub::html_strip: [MARKER, tag]
const M: u8 = 0x01;

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

// CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320), bit by bit: an independent
// implementation, check value crc32("123456789") == 0xCBF43926
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn all_fixtures() -> Vec<Fixture> {
    fixtures::standard()
}

fn epub_fixtures() -> Vec<(String, EpubSpec, Vec<u8>)> {
    all_fixtures()
        .into_iter()
        .filter_map(|f| match f.spec {
            Spec::Epub(s) => Some((f.name.to_string(), s, f.bytes)),
            Spec::Txt(_) => None,
        })
        .collect()
}

fn txt_fixtures() -> Vec<(String, TxtSpec, Vec<u8>)> {
    all_fixtures()
        .into_iter()
        .filter_map(|f| match f.spec {
            Spec::Txt(s) => Some((f.name.to_string(), s, f.bytes)),
            Spec::Epub(_) => None,
        })
        .collect()
}

fn s(x: &str) -> String {
    x.to_string()
}
fn text(x: &str) -> Run {
    Run::Text(s(x))
}
fn para(runs: Vec<Run>) -> Block {
    Block::Paragraph(runs)
}
fn chapter(title: &str, blocks: Vec<Block>) -> Chapter {
    Chapter { title: s(title), blocks }
}
fn toc(title: &str, chapter: usize, children: Vec<TocItem>) -> TocItem {
    TocItem { title: s(title), chapter, children }
}
fn image(path: &str, kind: ImageKind, width: u16, height: u16, pattern: Pattern) -> ImageSpec {
    ImageSpec { path: s(path), kind, width, height, pattern }
}

// a small hand-written book: 3 chapters, nested TOC, no images
fn mini_spec(version: EpubVersion, compression: Compression) -> EpubSpec {
    EpubSpec {
        version,
        title: s("Mini Book"),
        author: s("A. Writer"),
        identifier: s("urn:pulp-os:fixture:mini"),
        chapters: vec![
            chapter("One", vec![para(vec![text("First paragraph.")])]),
            chapter("Two", vec![para(vec![text("Second paragraph.")]), para(vec![text("Third.")])]),
            chapter("Three", vec![para(vec![text("Last paragraph.")])]),
        ],
        toc: vec![toc("One", 0, vec![]), toc("Two", 1, vec![toc("Three", 2, vec![])])],
        images: vec![],
        cover: None,
        compression,
        numeric_entities: false,
    }
}

fn is_ascii_text(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\t') || (' '..='~').contains(&c)
}

// ---------------------------------------------------------------------------
// independent ZIP reader / validator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum ZipFault {
    TooShort,
    BadEocd,
    CentralDirectoryBounds,
    BadCentralHeader(usize),
    BadLocalHeader(usize),
    LocalCentralMismatch(usize),
    FlagsSet(usize),
    NotFixedTimestamp(usize),
    ExtraField(usize),
    Gap(usize),
    UnsupportedMethod(usize, u16),
    BadDeflate(usize),
    SizeMismatch(usize),
    CrcMismatch(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZEntry {
    name: String,
    method: u16,
    crc: u32,
    comp_size: u32,
    size: u32,
    local_offset: u32,
    // content after decompression
    data: Vec<u8>,
}

fn check_zip(b: &[u8]) -> Result<Vec<ZEntry>, ZipFault> {
    use ZipFault::*;
    if b.len() < 22 {
        return Err(TooShort);
    }
    let e = b.len() - 22;
    if le32(b, e) != 0x0605_4b50 || le16(b, e + 20) != 0 {
        return Err(BadEocd);
    }
    if le16(b, e + 4) != 0 || le16(b, e + 6) != 0 || le16(b, e + 8) != le16(b, e + 10) {
        return Err(BadEocd);
    }
    let n = le16(b, e + 10) as usize;
    let cd_size = le32(b, e + 12) as usize;
    let cd_off = le32(b, e + 16) as usize;
    if cd_off + cd_size != e {
        return Err(CentralDirectoryBounds);
    }
    let mut out = Vec::new();
    let mut c = cd_off;
    let mut expect_local = 0usize;
    for i in 0..n {
        if c + 46 > e || le32(b, c) != 0x0201_4b50 {
            return Err(BadCentralHeader(i));
        }
        let flags = le16(b, c + 8);
        let method = le16(b, c + 10);
        let (time, date) = (le16(b, c + 12), le16(b, c + 14));
        let crc = le32(b, c + 16);
        let comp = le32(b, c + 20);
        let size = le32(b, c + 24);
        let nlen = le16(b, c + 28) as usize;
        let xlen = le16(b, c + 30) as usize;
        let clen = le16(b, c + 32) as usize;
        let loff = le32(b, c + 42) as usize;
        if c + 46 + nlen > e {
            return Err(BadCentralHeader(i));
        }
        if xlen != 0 || clen != 0 {
            return Err(ExtraField(i));
        }
        let name = String::from_utf8(b[c + 46..c + 46 + nlen].to_vec()).map_err(|_| BadCentralHeader(i))?;
        c += 46 + nlen;

        // local header
        if loff != expect_local {
            return Err(Gap(i));
        }
        if loff + 30 > cd_off || le32(b, loff) != 0x0403_4b50 {
            return Err(BadLocalHeader(i));
        }
        let lflags = le16(b, loff + 6);
        let lmethod = le16(b, loff + 8);
        let (ltime, ldate) = (le16(b, loff + 10), le16(b, loff + 12));
        let (lcrc, lcomp, lsize) = (le32(b, loff + 14), le32(b, loff + 18), le32(b, loff + 22));
        let lnlen = le16(b, loff + 26) as usize;
        let lxlen = le16(b, loff + 28) as usize;
        if lxlen != 0 {
            return Err(ExtraField(i));
        }
        if flags != 0 || lflags != 0 {
            return Err(FlagsSet(i));
        }
        if loff + 30 + lnlen > cd_off || b[loff + 30..loff + 30 + lnlen] != *name.as_bytes() {
            return Err(LocalCentralMismatch(i));
        }
        if lmethod != method || lcrc != crc || lcomp != comp || lsize != size {
            return Err(LocalCentralMismatch(i));
        }
        if (time, date) != (DOS_TIME, DOS_DATE) || (ltime, ldate) != (DOS_TIME, DOS_DATE) {
            return Err(NotFixedTimestamp(i));
        }
        let ds = loff + 30 + lnlen;
        let de = ds + comp as usize;
        if de > cd_off {
            return Err(BadLocalHeader(i));
        }
        expect_local = de;
        let raw = &b[ds..de];
        let data = match method {
            0 => {
                if comp != size {
                    return Err(SizeMismatch(i));
                }
                raw.to_vec()
            }
            8 => decompress_to_vec(raw).map_err(|_| BadDeflate(i))?,
            m => return Err(UnsupportedMethod(i, m)),
        };
        if data.len() != size as usize {
            return Err(SizeMismatch(i));
        }
        if crc32(&data) != crc {
            return Err(CrcMismatch(i));
        }
        out.push(ZEntry { name, method, crc, comp_size: comp, size, local_offset: loff as u32, data });
    }
    if c != e {
        return Err(CentralDirectoryBounds);
    }
    if expect_local != cd_off {
        return Err(Gap(n));
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EpubFault {
    MimetypeNotFirst,
    MimetypeCompressed,
    MimetypeContent,
    DuplicateName,
    NoContainer,
}

fn check_epub_rules(ents: &[ZEntry]) -> Result<(), EpubFault> {
    if ents.first().map(|e| e.name.as_str()) != Some("mimetype") {
        return Err(EpubFault::MimetypeNotFirst);
    }
    if ents[0].method != 0 {
        return Err(EpubFault::MimetypeCompressed);
    }
    if ents[0].data != b"application/epub+zip" {
        return Err(EpubFault::MimetypeContent);
    }
    let names: BTreeSet<&str> = ents.iter().map(|e| e.name.as_str()).collect();
    if names.len() != ents.len() {
        return Err(EpubFault::DuplicateName);
    }
    if !names.contains("META-INF/container.xml") {
        return Err(EpubFault::NoContainer);
    }
    Ok(())
}

// test-side ZIP writer for hand-made (good and bad) archives; layout identical
// to what the contract demands, so a good archive must pass check_zip
fn write_zip(entries: &[(&str, &[u8], u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for (name, data, method) in entries {
        let body = if *method == 8 { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
        let off = out.len() as u32;
        let mut l = vec![0u8; 30];
        put32(&mut l, 0, 0x0403_4b50);
        put16(&mut l, 4, 20);
        put16(&mut l, 8, *method);
        put16(&mut l, 12, DOS_DATE);
        put32(&mut l, 14, crc32(data));
        put32(&mut l, 18, body.len() as u32);
        put32(&mut l, 22, data.len() as u32);
        put16(&mut l, 26, name.len() as u16);
        out.extend_from_slice(&l);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
        let mut c = vec![0u8; 46];
        put32(&mut c, 0, 0x0201_4b50);
        put16(&mut c, 4, 20);
        put16(&mut c, 6, 20);
        put16(&mut c, 10, *method);
        put16(&mut c, 14, DOS_DATE);
        put32(&mut c, 16, crc32(data));
        put32(&mut c, 20, body.len() as u32);
        put32(&mut c, 24, data.len() as u32);
        put16(&mut c, 28, name.len() as u16);
        put32(&mut c, 42, off);
        cd.extend_from_slice(&c);
        cd.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    let mut e = vec![0u8; 22];
    put32(&mut e, 0, 0x0605_4b50);
    put16(&mut e, 8, entries.len() as u16);
    put16(&mut e, 10, entries.len() as u16);
    put32(&mut e, 12, cd.len() as u32);
    put32(&mut e, 16, cd_off);
    out.extend_from_slice(&e);
    out
}

// byte offsets of entry i's headers inside `b` (b must already pass check_zip)
fn header_offsets(b: &[u8], i: usize) -> (usize, usize) {
    let e = b.len() - 22;
    let mut c = le32(b, e + 16) as usize;
    for _ in 0..i {
        c += 46 + le16(b, c + 28) as usize;
    }
    (le32(b, c + 42) as usize, c)
}

// ---------------------------------------------------------------------------
// independent XML well-formedness checker (what the builder's escaping must
// achieve): UTF-8, one root, balanced and matching tags, quoted attributes
// without raw '<', every '&' a known / numeric entity reference
// ---------------------------------------------------------------------------

fn entity_len(s: &str, i: usize) -> Result<usize, String> {
    let rest = &s[i..];
    let semi = rest.find(';').filter(|&p| p <= 12).ok_or_else(|| format!("bare '&' at byte {i}"))?;
    let body = &rest[1..semi];
    let ok = matches!(body, "amp" | "lt" | "gt" | "quot" | "apos")
        || body.strip_prefix("#x").is_some_and(|h| !h.is_empty() && h.bytes().all(|c| c.is_ascii_hexdigit()))
        || body.strip_prefix('#').is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()));
    if ok { Ok(semi + 1) } else { Err(format!("unknown entity '&{body};' at byte {i}")) }
}

fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-')
}

fn check_xml(src: &[u8]) -> Result<(), String> {
    let s = std::str::from_utf8(src).map_err(|e| format!("not UTF-8: {e}"))?;
    let b = s.as_bytes();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    let mut roots = 0;
    while i < b.len() {
        match b[i] {
            b'<' => {
                let rest = &s[i..];
                if rest.starts_with("<?") {
                    i += rest.find("?>").ok_or("unterminated <? ?>")? + 2;
                } else if rest.starts_with("<!--") {
                    i += rest.find("-->").ok_or("unterminated comment")? + 3;
                } else if rest.starts_with("<!") {
                    i += rest.find('>').ok_or("unterminated <!")? + 1;
                } else if rest.starts_with("</") {
                    let end = rest.find('>').ok_or("unterminated end tag")?;
                    let name = rest[2..end].trim();
                    match stack.pop() {
                        Some(open) if open == name => {}
                        other => return Err(format!("end tag </{name}> closes {other:?}")),
                    }
                    i += end + 1;
                } else {
                    let mut j = i + 1;
                    let name_start = j;
                    while j < b.len() && is_name_byte(b[j]) {
                        j += 1;
                    }
                    if j == name_start {
                        return Err(format!("empty tag name at byte {i}"));
                    }
                    let name = s[name_start..j].to_string();
                    let mut attrs: BTreeSet<String> = BTreeSet::new();
                    let self_closing;
                    loop {
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j >= b.len() {
                            return Err(format!("unterminated tag <{name}"));
                        }
                        if b[j] == b'>' {
                            self_closing = false;
                            j += 1;
                            break;
                        }
                        if b[j] == b'/' && b.get(j + 1) == Some(&b'>') {
                            self_closing = true;
                            j += 2;
                            break;
                        }
                        let a0 = j;
                        while j < b.len() && is_name_byte(b[j]) {
                            j += 1;
                        }
                        if j == a0 {
                            return Err(format!("bad attribute in <{name} at byte {j}"));
                        }
                        let aname = s[a0..j].to_string();
                        if !attrs.insert(aname.clone()) {
                            return Err(format!("duplicate attribute {aname} in <{name}>"));
                        }
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if b.get(j) != Some(&b'=') {
                            return Err(format!("attribute {aname} of <{name}> has no '='"));
                        }
                        j += 1;
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        let q = *b.get(j).ok_or("eof in attribute")?;
                        if q != b'"' && q != b'\'' {
                            return Err(format!("attribute {aname} of <{name}> is not quoted"));
                        }
                        j += 1;
                        loop {
                            let c = *b.get(j).ok_or("eof in attribute value")?;
                            if c == q {
                                j += 1;
                                break;
                            }
                            if c == b'<' {
                                return Err(format!("raw '<' in attribute {aname}"));
                            }
                            if c == b'&' {
                                j += entity_len(s, j)?;
                            } else {
                                j += 1;
                            }
                        }
                    }
                    if stack.is_empty() {
                        roots += 1;
                        if roots > 1 {
                            return Err("second root element".into());
                        }
                    }
                    if !self_closing {
                        stack.push(name);
                    }
                    i = j;
                }
            }
            b'&' => {
                if stack.is_empty() {
                    return Err("entity outside root".into());
                }
                i += entity_len(s, i)?;
            }
            c => {
                if stack.is_empty() && !c.is_ascii_whitespace() && !(i == 0 && s.starts_with('\u{feff}')) {
                    return Err(format!("text outside the root element at byte {i}"));
                }
                i += 1;
            }
        }
    }
    if !stack.is_empty() {
        return Err(format!("unclosed {stack:?}"));
    }
    if roots != 1 {
        return Err(format!("{roots} root elements"));
    }
    Ok(())
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if s.as_bytes()[i] == b'&' {
            let n = entity_len(s, i).expect("valid entity");
            let body = &s[i + 1..i + n - 1];
            out.push(match body {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                b if b.starts_with("#x") => char::from_u32(u32::from_str_radix(&b[2..], 16).unwrap()).unwrap(),
                b => char::from_u32(b[1..].parse().unwrap()).unwrap(),
            });
            i += n;
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

// ---------------------------------------------------------------------------
// independent PNG / JPEG structure checkers
// ---------------------------------------------------------------------------

// the contract's luminance of pixel (x, y) (PNG)
fn lum(p: Pattern, x: u32, y: u32, w: u32) -> u8 {
    match p {
        Pattern::Black => 0,
        Pattern::White => 255,
        Pattern::Checker { cell } => {
            let c = cell as u32;
            if (x / c + y / c) % 2 == 0 { 0 } else { 255 }
        }
        Pattern::HorizontalGradient => {
            if w > 1 {
                (x * 255 / (w - 1)) as u8
            } else {
                0
            }
        }
    }
}

fn check_png(b: &[u8], im: &ImageSpec) -> Result<(), String> {
    if b.len() < 8 || b[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("bad PNG signature".into());
    }
    let (w, h) = (im.width as usize, im.height as usize);
    let (depth, ctype) = match im.kind {
        ImageKind::PngGray1 => (1u8, 0u8),
        ImageKind::PngGray8 => (8, 0),
        ImageKind::PngPalette8 => (8, 3),
        ImageKind::Jpeg => return Err("not a PNG spec".into()),
    };
    let mut i = 8;
    let mut kinds: Vec<[u8; 4]> = Vec::new();
    let mut idat = Vec::new();
    let mut plte = None;
    while i < b.len() {
        if i + 12 > b.len() {
            return Err("truncated chunk".into());
        }
        let len = be32(b, i) as usize;
        if i + 12 + len > b.len() {
            return Err("chunk runs past the end".into());
        }
        let kind: [u8; 4] = b[i + 4..i + 8].try_into().unwrap();
        let body = &b[i + 8..i + 8 + len];
        if crc32(&b[i + 4..i + 8 + len]) != be32(b, i + 8 + len) {
            return Err(format!("bad CRC in chunk {}", String::from_utf8_lossy(&kind)));
        }
        match &kind {
            b"IHDR" => {
                if len != 13 || !kinds.is_empty() {
                    return Err("IHDR not first / wrong length".into());
                }
                if be32(body, 0) as usize != w || be32(body, 4) as usize != h {
                    return Err(format!("IHDR size {}x{}, spec {w}x{h}", be32(body, 0), be32(body, 4)));
                }
                if body[8] != depth || body[9] != ctype || body[10] != 0 || body[11] != 0 || body[12] != 0 {
                    return Err(format!("IHDR depth/type/comp/filter/interlace = {:?}", &body[8..13]));
                }
            }
            b"PLTE" => plte = Some(body.to_vec()),
            b"IDAT" => idat.extend_from_slice(body),
            _ => {}
        }
        kinds.push(kind);
        i += 12 + len;
    }
    if kinds.first() != Some(b"IHDR") || kinds.last() != Some(b"IEND") {
        return Err(format!("chunk order {kinds:?}"));
    }
    if kinds.iter().filter(|k| *k == b"IDAT").count() != 1 {
        return Err("expected exactly one IDAT".into());
    }
    match (ctype, plte) {
        (3, Some(p)) if p == [255, 255, 255, 0, 0, 0] => {}
        (3, other) => return Err(format!("palette image PLTE = {other:?}")),
        (_, Some(_)) => return Err("PLTE in a greyscale image".into()),
        (_, None) => {}
    }
    let raw = decompress_to_vec_zlib(&idat).map_err(|e| format!("IDAT zlib: {e:?}"))?;
    let row_bytes = match im.kind {
        ImageKind::PngGray1 => w.div_ceil(8),
        _ => w,
    };
    if raw.len() != h * (1 + row_bytes) {
        return Err(format!("IDAT inflates to {} bytes, expected {}", raw.len(), h * (1 + row_bytes)));
    }
    for y in 0..h {
        let row = &raw[y * (1 + row_bytes)..(y + 1) * (1 + row_bytes)];
        if row[0] != 0 {
            return Err(format!("row {y} filter {}", row[0]));
        }
        for x in 0..w {
            let l = lum(im.pattern, x as u32, y as u32, w as u32);
            let got = match im.kind {
                ImageKind::PngGray1 => (row[1 + x / 8] >> (7 - x % 8)) & 1,
                _ => row[1 + x],
            };
            let want = match im.kind {
                ImageKind::PngGray1 => (l >= 128) as u8,
                ImageKind::PngGray8 => l,
                _ => (l < 128) as u8,
            };
            if got != want {
                return Err(format!("pixel ({x},{y}) sample {got}, spec {want}"));
            }
        }
    }
    Ok(())
}

fn check_jpeg(b: &[u8], im: &ImageSpec) -> Result<(), String> {
    if b.len() < 4 || b[..2] != [0xFF, 0xD8] || b[b.len() - 2..] != [0xFF, 0xD9] {
        return Err("missing SOI / EOI".into());
    }
    let mut i = 2;
    let (mut dqt, mut dc, mut ac, mut sof) = (0, 0, 0, false);
    loop {
        if i + 4 > b.len() || b[i] != 0xFF {
            return Err(format!("expected a marker at byte {i}"));
        }
        let m = b[i + 1];
        let len = be16(b, i + 2) as usize;
        if len < 2 || i + 2 + len > b.len() {
            return Err(format!("bad segment length at byte {i}"));
        }
        let seg = &b[i + 4..i + 2 + len];
        match m {
            0xDB => dqt += 1,
            0xC4 => match seg[0] >> 4 {
                0 => dc += 1,
                1 => ac += 1,
                c => return Err(format!("DHT class {c}")),
            },
            0xC0 => {
                sof = true;
                if seg[0] != 8 || seg[5] != 1 {
                    return Err(format!("SOF0 precision {} components {}", seg[0], seg[5]));
                }
                let (h, w) = (be16(seg, 1), be16(seg, 3));
                if (w, h) != (im.width, im.height) {
                    return Err(format!("SOF0 {w}x{h}, spec {}x{}", im.width, im.height));
                }
            }
            0xDA => {
                if !sof || dqt == 0 || dc == 0 || ac == 0 {
                    return Err(format!("SOS before SOF/DQT/DHT (sof {sof}, dqt {dqt}, dc {dc}, ac {ac})"));
                }
                let scan = &b[i + 2 + len..b.len() - 2];
                if scan.is_empty() {
                    return Err("empty scan".into());
                }
                for (k, &c) in scan.iter().enumerate() {
                    if c == 0xFF && scan.get(k + 1) != Some(&0x00) {
                        return Err(format!("unstuffed 0xFF in the scan at {k}"));
                    }
                }
                return Ok(());
            }
            0xC1..=0xC3 | 0xC5..=0xCF | 0xDD | 0xD0..=0xD7 => return Err(format!("marker FF{m:02X} not allowed")),
            _ => {}
        }
        i += 2 + len;
    }
}

// ---------------------------------------------------------------------------
// production-parser access (what ReaderApp links)
// ---------------------------------------------------------------------------

fn read_at(bytes: &[u8]) -> impl FnMut(u32, &mut [u8]) -> Result<usize, &'static str> + '_ {
    move |off, buf| {
        let off = off as usize;
        if off > bytes.len() {
            return Err("read past end");
        }
        let n = buf.len().min(bytes.len() - off);
        buf[..n].copy_from_slice(&bytes[off..off + n]);
        Ok(n)
    }
}

fn open_zip(bytes: &[u8]) -> ZipIndex {
    let tail = bytes.len().saturating_sub(65557);
    let (cd_off, cd_size) = ZipIndex::parse_eocd(&bytes[tail..], bytes.len() as u32).expect("production EOCD parse");
    let mut zip = ZipIndex::new();
    zip.parse_central_directory(&bytes[cd_off as usize..(cd_off + cd_size) as usize]).expect("production central directory parse");
    zip
}

fn entry_bytes(bytes: &[u8], zip: &ZipIndex, name: &str) -> Vec<u8> {
    let i = zip.find(name).unwrap_or_else(|| panic!("{name} not found by production ZipIndex::find"));
    let e = zip.entry(i);
    extract_entry(e, e.local_offset, read_at(bytes)).unwrap_or_else(|err| panic!("extract_entry({name}): {err}"))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

struct Parsed {
    zip: ZipIndex,
    opf_path: String,
    opf: Vec<u8>,
    meta: EpubMeta,
    spine: EpubSpine,
    toc_source: Option<TocSource>,
    toc: EpubToc,
}

fn parse_book(bytes: &[u8]) -> Parsed {
    let zip = open_zip(bytes);
    let container = entry_bytes(bytes, &zip, "META-INF/container.xml");
    let mut path = [0u8; epub::OPF_PATH_CAP];
    let n = epub::parse_container(&container, &mut path).expect("production parse_container");
    let opf_path = String::from_utf8(path[..n].to_vec()).unwrap();
    let opf = entry_bytes(bytes, &zip, &opf_path);
    let mut meta = EpubMeta::new();
    let mut spine = EpubSpine::new();
    epub::parse_opf(&opf, dir_of(&opf_path), &zip, &mut meta, &mut spine).expect("production parse_opf");
    let toc_source = epub::find_toc_source(&opf, dir_of(&opf_path), &zip);
    let mut toc = EpubToc::new();
    if let Some(src) = toc_source {
        let toc_name = zip.entry_name(src.zip_index()).to_string();
        let data = entry_bytes(bytes, &zip, &toc_name);
        epub::parse_toc(src, &data, dir_of(&toc_name), &spine, &zip, &mut toc);
    }
    Parsed { zip, opf_path, opf, meta, spine, toc_source, toc }
}

fn chapter_entry_name(i: usize) -> String {
    format!("OEBPS/text/chapter{:02}.xhtml", i + 1)
}

fn expected_entry_names(spec: &EpubSpec) -> Vec<String> {
    let mut v = vec![s("mimetype"), s("META-INF/container.xml"), s("OEBPS/content.opf")];
    v.push(s(if spec.version == EpubVersion::V2 { "OEBPS/toc.ncx" } else { "OEBPS/nav.xhtml" }));
    for i in 0..spec.chapters.len() {
        v.push(chapter_entry_name(i));
    }
    for im in &spec.images {
        v.push(format!("OEBPS/{}", im.path));
    }
    v
}

fn expected_method(c: Compression, i: usize) -> u16 {
    match c {
        Compression::Stored => 0,
        Compression::Deflate => {
            if i == 0 {
                0
            } else {
                8
            }
        }
        Compression::Mixed => {
            if i != 0 && i % 2 == 1 {
                8
            } else {
                0
            }
        }
    }
}

// pre-order flattening of the nested TOC: what the production parsers return
fn flatten(items: &[TocItem], out: &mut Vec<(String, usize)>) {
    for it in items {
        out.push((it.title.clone(), it.chapter));
        flatten(&it.children, out);
    }
}

fn wrap(on: u8, off: u8, t: &str) -> Vec<u8> {
    let mut v = vec![M, on];
    v.extend_from_slice(t.as_bytes());
    v.extend_from_slice(&[M, off]);
    v
}

// What the production stripper must hand back for one chapter, derived only
// from the spec: blocks separated by a blank line ("\n\n"), one final "\n"; the
// h1 title and every heading are [M,'H'] .. [M,'h']; bold [M,'B'] .. [M,'b'];
// italic [M,'I'] .. [M,'i']; <br/> is "\n"; an image is [M,'P',len,src] with
// src exactly the <img src> ("../PATH").
fn expected_chapter_text(spec: &EpubSpec, ch: &Chapter) -> Vec<u8> {
    let mut blocks: Vec<Vec<u8>> = vec![wrap(b'H', b'h', &ch.title)];
    for b in &ch.blocks {
        blocks.push(match b {
            Block::Heading(t) => wrap(b'H', b'h', t),
            Block::Paragraph(runs) => {
                let mut v = Vec::new();
                for r in runs {
                    match r {
                        Run::Text(t) => v.extend_from_slice(t.as_bytes()),
                        Run::Bold(t) => v.extend(wrap(b'B', b'b', t)),
                        Run::Italic(t) => v.extend(wrap(b'I', b'i', t)),
                        Run::Break => v.push(b'\n'),
                    }
                }
                v
            }
            Block::Image(i) => {
                let src = format!("../{}", spec.images[*i].path);
                let mut v = vec![M, b'P', src.len() as u8];
                v.extend_from_slice(src.as_bytes());
                v
            }
        });
    }
    let mut out = blocks.join(&b"\n\n"[..]);
    out.push(b'\n');
    out
}

fn plain_of(runs: &[Run]) -> String {
    let mut p = String::new();
    for r in runs {
        match r {
            Run::Text(t) | Run::Bold(t) | Run::Italic(t) => p.push_str(t),
            Run::Break => p.push('\n'),
        }
    }
    p
}

// ---------------------------------------------------------------------------
// TXT (English reader regression: LF / CRLF, multi-byte, long lines, blank lines, with / without
// final newline, longer than a page)
// ---------------------------------------------------------------------------

fn txt(lines: &[&str], newline: Newline, trailing: bool) -> Vec<u8> {
    build_txt(&TxtSpec { lines: lines.iter().map(|l| s(l)).collect(), newline, trailing_newline: trailing })
}

#[test]
fn build_txt_matches_hand_written_bytes() {
    // hand-computed expectations
    assert_eq!(txt(&["a", "", "b"], Newline::Lf, true), b"a\n\nb\n");
    assert_eq!(txt(&["a", "", "b"], Newline::Lf, false), b"a\n\nb");
    assert_eq!(txt(&["a", "", "b"], Newline::CrLf, true), b"a\r\n\r\nb\r\n");
    assert_eq!(txt(&["a", "", "b"], Newline::CrLf, false), b"a\r\n\r\nb");
    assert_eq!(txt(&["only"], Newline::Lf, true), b"only\n");
    assert_eq!(txt(&["only"], Newline::CrLf, false), b"only");
    // a single blank line
    assert_eq!(txt(&[""], Newline::Lf, true), b"\n");
    assert_eq!(txt(&[""], Newline::Lf, false), b"");
    // trailing blank line survives
    assert_eq!(txt(&["x", ""], Newline::Lf, false), b"x\n");
    assert_eq!(txt(&["", ""], Newline::CrLf, false), b"\r\n");
    // no lines: no bytes, whatever trailing_newline says
    assert_eq!(txt(&[], Newline::Lf, true), b"");
    assert_eq!(txt(&[], Newline::CrLf, false), b"");
    // multi-byte: e-acute (C3 A9), right single quote (E2 80 99), em dash (E2 80 94), ellipsis (E2 80 A6)
    let mut want: Vec<u8> = b"caf".to_vec();
    want.extend_from_slice(&[0xC3, 0xA9]);
    want.push(b'\n');
    want.extend_from_slice(b"it");
    want.extend_from_slice(&[0xE2, 0x80, 0x99]);
    want.extend_from_slice(b"s ");
    want.extend_from_slice(&[0xE2, 0x80, 0x94]);
    want.extend_from_slice(b" wait");
    want.extend_from_slice(&[0xE2, 0x80, 0xA6]);
    want.push(b'\n');
    assert_eq!(txt(&["caf\u{e9}", "it\u{2019}s \u{2014} wait\u{2026}"], Newline::Lf, true), want);
}

#[test]
fn standard_txt_bytes_equal_the_spec_and_obey_newline_rules() {
    let mut seen = 0;
    for (name, spec, bytes) in txt_fixtures() {
        seen += 1;
        // independent derivation of the bytes from the spec
        let eol: &str = if spec.newline == Newline::CrLf { "\r\n" } else { "\n" };
        let mut want = String::new();
        for (i, l) in spec.lines.iter().enumerate() {
            assert!(!l.contains('\n') && !l.contains('\r'), "{name}: line {i} contains a line break");
            if i > 0 {
                want.push_str(eol);
            }
            want.push_str(l);
        }
        if spec.trailing_newline && !spec.lines.is_empty() {
            want.push_str(eol);
        }
        assert_eq!(bytes, want.as_bytes(), "{name}: bytes differ from the spec");
        // newline hygiene on the produced file
        let lf = bytes.iter().filter(|&&b| b == b'\n').count();
        let cr = bytes.iter().filter(|&&b| b == b'\r').count();
        match spec.newline {
            Newline::Lf => assert_eq!(cr, 0, "{name}: LF file contains CR"),
            Newline::CrLf => {
                assert_eq!(cr, lf, "{name}: every LF must be preceded by CR");
                assert!(bytes.windows(2).filter(|w| w == b"\r\n").count() == lf, "{name}: lone CR or LF");
            }
        }
        std::str::from_utf8(&bytes).unwrap_or_else(|e| panic!("{name}: not UTF-8: {e}"));
    }
    assert_eq!(seen, 5, "five TXT fixtures");
}

#[test]
fn standard_txt_set_covers_every_text_shape_r8_names() {
    let t = txt_fixtures();
    let by = |n: &str| t.iter().find(|(name, _, _)| name == n).unwrap_or_else(|| panic!("{n} missing"));
    let (_, lf, _) = by("PLAINLF.TXT");
    assert_eq!((lf.newline, lf.trailing_newline), (Newline::Lf, true));
    let (_, cr, _) = by("PLAINCR.TXT");
    assert_eq!((cr.newline, cr.trailing_newline), (Newline::CrLf, true));
    let (_, no, nob) = by("NOEOL.TXT");
    assert_eq!((no.newline, no.trailing_newline), (Newline::Lf, false));
    // the FILE really ends without a line break: the last line is not blank
    assert!(no.lines.last().is_some_and(|l| !l.is_empty()), "NOEOL.TXT: last line must be non-empty");
    assert!(!nob.ends_with(b"\n") && !nob.ends_with(b"\r"), "NOEOL.TXT ends without a line break");
    assert!(by("PLAINLF.TXT").2.ends_with(b"\n") && !by("PLAINLF.TXT").2.ends_with(b"\r\n"), "PLAINLF.TXT ends with a bare LF");
    assert!(by("PLAINCR.TXT").2.ends_with(b"\r\n"), "PLAINCR.TXT ends with CRLF");
    let (_, u8s, u8b) = by("UTF8.TXT");
    assert!(u8s.lines.iter().any(|l| l.chars().any(|c| (c as u32) > 0xFF)), "UTF8.TXT needs a 3-byte character");
    assert!(u8s.lines.iter().any(|l| l.chars().any(|c| (0x80..=0x7FF).contains(&(c as u32)))), "UTF8.TXT needs a 2-byte character");
    assert!(u8b.len() > u8s.lines.len(), "multi-byte file is longer than its char count");
    let (_, _, tiny) = by("TINY.TXT");
    assert!(tiny.len() < 100, "TINY.TXT is a one-screen file ({} bytes)", tiny.len());

    for (name, spec, bytes) in &t {
        assert!(
            spec.lines.first().is_some_and(|l| l.starts_with(TXT_PROVENANCE_PREFIX)),
            "{name}: first line must start with {TXT_PROVENANCE_PREFIX:?}"
        );
        if name == "TINY.TXT" {
            continue;
        }
        // text longer than one page, blank lines and a line far longer than the 480 px text area
        assert!(bytes.len() >= 6000, "{name}: only {} bytes", bytes.len());
        let blanks: Vec<usize> = spec.lines.iter().enumerate().filter(|(_, l)| l.is_empty()).map(|(i, _)| i).collect();
        assert!(blanks.len() >= 3 && blanks.iter().any(|&i| i > 5), "{name}: needs blank lines inside the body, not only after the header ({blanks:?})");
        assert!(spec.lines.iter().any(|l| l.len() >= 400), "{name}: needs a line of >= 400 bytes (must wrap)");
        assert!(spec.lines.iter().any(|l| l.len() >= 1 && l.len() <= 60), "{name}: needs short lines too");
    }
}

#[test]
fn standard_txt_is_longer_than_one_page_in_the_production_reader_except_tiny() {
    // the only reader-level assertion in fixture validation: "more than a page" measured by the
    // production paginator. Navigation / bookmark regressions are reader regression.
    for f in all_fixtures() {
        if !matches!(f.spec, Spec::Txt(_)) {
            continue;
        }
        let mut rig = Rig::new(fixtures::standard_card());
        rig.configure(2, 0);
        rig.open(f.name);
        assert_eq!(rig.phase(), Phase::Ready, "{}: opens", f.name);
        assert_eq!(rig.page(), 0, "{}: opens on page 0", f.name);
        rig.press(Action::Next);
        assert_eq!(rig.phase(), Phase::Ready, "{}: still ready after Next", f.name);
        if f.name == "TINY.TXT" {
            assert_eq!(rig.page(), 0, "TINY.TXT is a single page");
        } else {
            assert_eq!(rig.page(), 1, "{}: has a second page", f.name);
            rig.press(Action::Next);
            assert_eq!(rig.page(), 2, "{}: has a third page", f.name);
        }
    }
}

// ---------------------------------------------------------------------------
// the standard set: names, card, size, determinism, distributability
// ---------------------------------------------------------------------------

#[test]
fn standard_lists_the_pinned_names_in_the_pinned_order() {
    let names: Vec<&str> = all_fixtures().iter().map(|f| f.name).collect();
    assert_eq!(names, STANDARD_NAMES);
    // kinds line up with the names
    for (i, f) in all_fixtures().iter().enumerate() {
        assert_eq!(matches!(f.spec, Spec::Epub(_)), i >= FIRST_EPUB, "{}", f.name);
    }
}

#[test]
fn standard_names_are_listable_firmware_filenames() {
    let mut seen = BTreeSet::new();
    for name in STANDARD_NAMES {
        assert!(seen.insert(name), "{name} duplicated");
        // independent 8.3 rule: 1..=8 upper-case alphanumerics, '.', then a 3-letter extension.
        // EPUBs use ".EPU": a real FAT card stores only the 8.3 alias, and the
        // firmware lists SFNs with ext TXT / EPUB / EPU / MD.
        let (base, ext) = name.split_once('.').unwrap();
        assert!((1..=8).contains(&base.len()), "{name}: base");
        assert!(base.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()), "{name}: upper-case alphanumerics only");
        assert!(matches!(ext, "TXT" | "EPU"), "{name}: extension {ext}");
        assert!(name.len() <= 13, "{name}: DirEntry::name is 13 bytes");
        // and the firmware's own predicate (kernel/src/drivers/dir_entry.rs)
        assert!(pulp_host::dir_entry::is_listed_name(name.as_bytes()), "{name} would not be listed");
    }
    for (i, name) in STANDARD_NAMES.iter().enumerate() {
        assert_eq!(name.ends_with(".EPU"), i >= FIRST_EPUB, "{name}");
    }
}

#[test]
fn standard_card_lists_sizes_and_reads_back_every_fixture() {
    let fx = all_fixtures();
    let card = fixtures::standard_card();
    let mut buf = vec![DirEntry::EMPTY; 32];
    let n = card.list_root_files(&mut buf).expect("list");
    let mut listed: Vec<(String, u32)> = buf[..n].iter().map(|e| (e.name_str().to_string(), e.size)).collect();
    listed.sort();
    let mut want: Vec<(String, u32)> = fx.iter().map(|f| (f.name.to_string(), f.bytes.len() as u32)).collect();
    want.sort();
    assert_eq!(listed, want, "list_root_files shows exactly the 11 fixtures with their byte sizes (and not _PULP)");

    for f in &fx {
        assert_eq!(card.file_size(f.name).unwrap() as usize, f.bytes.len(), "{}", f.name);
        // odd chunk size so chunk edges fall inside multi-byte sequences and ZIP headers
        let mut got = Vec::new();
        let mut chunk = [0u8; 509];
        loop {
            let k = card.read_file_chunk(f.name, got.len() as u32, &mut chunk).unwrap();
            if k == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..k]);
        }
        assert_eq!(got, f.bytes, "{}: read back through read_file_chunk", f.name);
    }
    // _PULP/ exists the way the firmware boots it
    card.write_in_pulp("PROBE.BIN", b"x").expect("_PULP/ present");
}

#[test]
fn standard_set_is_small_enough_to_commit() {
    let fx = all_fixtures();
    let total: usize = fx.iter().map(|f| f.bytes.len()).sum();
    assert!(total < MAX_TOTAL_BYTES, "standard set is {total} bytes, limit {MAX_TOTAL_BYTES}");
    for f in &fx {
        assert!(!f.bytes.is_empty(), "{} is empty", f.name);
        assert!(f.bytes.len() < MAX_FILE_BYTES, "{} is {} bytes, limit {MAX_FILE_BYTES}", f.name, f.bytes.len());
    }
}

#[test]
fn builders_are_deterministic_and_standard_bytes_come_from_their_spec() {
    let a = all_fixtures();
    let b = all_fixtures();
    assert_eq!(a, b, "two standard() calls are identical (names, specs, bytes)");
    for f in &a {
        let again = match &f.spec {
            Spec::Txt(t) => build_txt(t),
            Spec::Epub(e) => build_epub(e).expect("standard spec builds"),
        };
        assert_eq!(f.bytes, again, "{}: bytes == build(spec)", f.name);
    }
    // the card is built from the same bytes
    let card = fixtures::standard_card();
    for f in &a {
        let mut got = vec![0u8; f.bytes.len() + 1];
        let n = card.read_file_chunk(f.name, 0, &mut got).unwrap();
        assert_eq!(&got[..n], &f.bytes[..], "{}", f.name);
    }
}

#[test]
fn every_spec_field_changes_the_bytes() {
    // a builder that ignores a field cannot be an oracle for it: change one
    // field at a time on a small book and require different bytes (and the
    // unchanged spec to be reproduced exactly)
    let mut base = mini_spec(EpubVersion::V3, Compression::Deflate);
    base.images = vec![image("images/a.png", ImageKind::PngGray1, 16, 16, Pattern::Checker { cell: 2 })];
    base.cover = Some(0);
    base.chapters[0].blocks.push(Block::Image(0));
    base.chapters[0].blocks.push(para(vec![text("caf\u{e9} done.")]));
    let want = build_epub(&base).unwrap();
    assert_eq!(build_epub(&base.clone()).unwrap(), want);

    let mut variants: Vec<(&str, EpubSpec)> = Vec::new();
    let mut v = base.clone();
    v.version = EpubVersion::V2;
    variants.push(("version", v));
    let mut v = base.clone();
    v.title = s("Mini Boom");
    variants.push(("title", v));
    let mut v = base.clone();
    v.author = s("B. Writer");
    variants.push(("author", v));
    let mut v = base.clone();
    v.identifier = s("urn:pulp-os:fixture:other");
    variants.push(("identifier", v));
    let mut v = base.clone();
    v.chapters[1].blocks[0] = para(vec![text("Second paragraph!")]);
    variants.push(("paragraph text", v));
    let mut v = base.clone();
    v.chapters[2].title = s("Tree");
    variants.push(("chapter title", v));
    let mut v = base.clone();
    v.toc[1].children[0].title = s("Trois");
    variants.push(("toc title", v));
    let mut v = base.clone();
    v.toc.pop();
    variants.push(("toc length", v));
    let mut v = base.clone();
    v.images[0].width = 24;
    variants.push(("image width", v));
    let mut v = base.clone();
    v.images[0].pattern = Pattern::Checker { cell: 4 };
    variants.push(("image pattern", v));
    let mut v = base.clone();
    v.images[0].path = s("images/b.png");
    variants.push(("image path", v));
    let mut v = base.clone();
    v.cover = None;
    variants.push(("cover", v));
    let mut v = base.clone();
    v.compression = Compression::Stored;
    variants.push(("compression", v));
    let mut v = base.clone();
    v.numeric_entities = true;
    variants.push(("numeric_entities", v));
    for (what, v) in variants {
        assert_ne!(build_epub(&v).unwrap(), want, "changing {what} must change the bytes");
    }
    // TXT: a changed line, newline kind, trailing flag all change the bytes (hand-written test above pins them)
}

#[test]
fn standard_fixtures_are_ascii_plus_the_listed_unicode_only() {
    // 1. every byte of every text part is ASCII or one of the allowed characters
    let check_text = |what: &str, bytes: &[u8]| {
        let t = std::str::from_utf8(bytes).unwrap_or_else(|e| panic!("{what}: not UTF-8: {e}"));
        for c in t.chars() {
            assert!(is_ascii_text(c) || ALLOWED_NON_ASCII.contains(c), "{what}: character U+{:04X} {c:?} is outside the allowed set", c as u32);
        }
    };
    // 2. the characters the SPEC TEXT (what a reader shows) actually uses
    let mut used: BTreeSet<char> = BTreeSet::new();
    for f in all_fixtures() {
        match &f.spec {
            Spec::Txt(t) => {
                check_text(f.name, &f.bytes);
                used.extend(t.lines.iter().flat_map(|l| l.chars()));
            }
            Spec::Epub(spec) => {
                let ents = check_zip(&f.bytes).unwrap();
                for e in &ents {
                    if !spec.images.iter().any(|im| e.name == format!("OEBPS/{}", im.path)) {
                        check_text(&format!("{}:{}", f.name, e.name), &e.data);
                    }
                }
                check_text(&format!("{}: author", f.name), spec.author.as_bytes());
                used.extend(spec.title.chars().chain(spec.author.chars()));
                for ch in &spec.chapters {
                    used.extend(ch.title.chars());
                    for b in &ch.blocks {
                        match b {
                            Block::Heading(h) => used.extend(h.chars()),
                            Block::Paragraph(runs) => used.extend(plain_of(runs).chars()),
                            Block::Image(_) => {}
                        }
                    }
                }
                // provenance: self-declared synthetic content
                let opf = ents.iter().find(|e| e.name == "OEBPS/content.opf").unwrap();
                assert_eq!(xml::tag_text(&opf.data, b"rights"), Some(RIGHTS.as_bytes()), "{}: dc:rights", f.name);
                assert!(spec.identifier.starts_with(ID_PREFIX), "{}: identifier {:?}", f.name, spec.identifier);
            }
        }
    }
    // the allowed set is actually exercised: 2-byte (e-acute) and 3-byte (quotes, dash, ellipsis)
    // characters, and the four characters that need escaping in XML
    for c in ['\u{e9}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2014}', '\u{2026}', '&', '<', '>', '"'] {
        assert!(used.contains(&c), "no fixture text contains U+{:04X} {c:?}", c as u32);
    }
}

// ---------------------------------------------------------------------------
// ZIP validity (independent oracle) and the checker's own teeth
// ---------------------------------------------------------------------------

#[test]
fn crc32_helper_has_the_published_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn every_standard_epub_is_a_valid_zip_and_a_valid_epub_container() {
    let mut tested = 0;
    for (name, spec, bytes) in epub_fixtures() {
        tested += 1;
        let ents = check_zip(&bytes).unwrap_or_else(|e| panic!("{name}: invalid ZIP: {e:?}"));
        check_epub_rules(&ents).unwrap_or_else(|e| panic!("{name}: invalid EPUB container: {e:?}"));
        let names: Vec<&str> = ents.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, expected_entry_names(&spec), "{name}: entry names and order");
        for (i, e) in ents.iter().enumerate() {
            assert_eq!(e.method, expected_method(spec.compression, i), "{name}: entry {i} {} method", e.name);
        }
        assert_eq!(ents[0].local_offset, 0, "{name}: mimetype is the first bytes of the file");
        // the mimetype entry is verbatim at byte 30: no extra field, stored
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..58], b"application/epub+zip");
        // the production index agrees on the entry table
        let zip = open_zip(&bytes);
        assert_eq!(zip.count(), ents.len(), "{name}");
        for (i, e) in ents.iter().enumerate() {
            let p = zip.entry(i);
            assert_eq!(zip.entry_name(i), e.name, "{name}[{i}]");
            assert_eq!((p.method, p.comp_size, p.uncomp_size, p.local_offset), (e.method, e.comp_size, e.size, e.local_offset), "{name}[{i}]");
        }
    }
    assert_eq!(tested, 6);
}

#[test]
fn compression_modes_really_show_stored_and_deflate_entries() {
    let mut mixed_seen = 0;
    for (name, spec, bytes) in epub_fixtures() {
        let ents = check_zip(&bytes).unwrap();
        let methods: BTreeSet<u16> = ents.iter().map(|e| e.method).collect();
        match spec.compression {
            Compression::Stored => assert_eq!(methods, BTreeSet::from([METHOD_STORED]), "{name}: all STORED"),
            Compression::Deflate => {
                assert_eq!(methods, BTreeSet::from([METHOD_STORED, METHOD_DEFLATE]), "{name}");
                assert_eq!(ents.iter().filter(|e| e.method == METHOD_STORED).count(), 1, "{name}: only mimetype is stored");
                assert!(ents.iter().skip(1).all(|e| e.method == METHOD_DEFLATE), "{name}");
            }
            Compression::Mixed => {
                mixed_seen += 1;
                assert_eq!(methods, BTreeSet::from([METHOD_STORED, METHOD_DEFLATE]), "{name}");
                // chapters: some of each method, inside the same book
                let ch: Vec<u16> = ents.iter().filter(|e| e.name.contains("/text/")).map(|e| e.method).collect();
                assert!(ch.contains(&METHOD_STORED) && ch.contains(&METHOD_DEFLATE), "{name}: chapter methods {ch:?}");
                // images: some of each method too
                let im: Vec<u16> = spec.images.iter().map(|i| ents.iter().find(|e| e.name == format!("OEBPS/{}", i.path)).unwrap().method).collect();
                assert!(im.contains(&METHOD_STORED) && im.contains(&METHOD_DEFLATE), "{name}: image methods {im:?}");
            }
        }
        // DEFLATE entries are genuinely compressed streams: bytes on disk differ from content
        for e in ents.iter().filter(|e| e.method == METHOD_DEFLATE && e.size > 256 && e.name.ends_with(".xhtml")) {
            assert!(e.comp_size < e.size, "{name}: {} deflated {} -> {}", e.name, e.size, e.comp_size);
        }
    }
    assert_eq!(mixed_seen, 2);
    // matrix: exactly one fixture per (version, compression)
    let mut cells = BTreeSet::new();
    for (_, spec, _) in epub_fixtures() {
        assert!(cells.insert((format!("{:?}", spec.version), format!("{:?}", spec.compression))));
    }
    assert_eq!(cells.len(), 6);
}

#[test]
fn standard_epubs_have_names_versions_and_compression_as_named() {
    let want = [
        ("E2STORED.EPU", EpubVersion::V2, Compression::Stored),
        ("E2DEFL.EPU", EpubVersion::V2, Compression::Deflate),
        ("E2MIXED.EPU", EpubVersion::V2, Compression::Mixed),
        ("E3STORED.EPU", EpubVersion::V3, Compression::Stored),
        ("E3DEFL.EPU", EpubVersion::V3, Compression::Deflate),
        ("E3MIXED.EPU", EpubVersion::V3, Compression::Mixed),
    ];
    let got = epub_fixtures();
    for (i, (n, v, c)) in want.iter().enumerate() {
        assert_eq!(got[i].0, *n);
        assert_eq!((got[i].1.version, got[i].1.compression), (*v, *c), "{n}");
    }
}

#[test]
fn checker_accepts_a_hand_built_good_epub() {
    let z = write_zip(&[
        ("mimetype", b"application/epub+zip", 0),
        ("META-INF/container.xml", b"<container/>", 8),
        ("OEBPS/a.xhtml", &b"hello hello hello hello hello hello hello hello"[..], 8),
        ("OEBPS/b.bin", &[0u8, 1, 2, 255][..], 0),
    ]);
    let ents = check_zip(&z).expect("good archive passes");
    assert_eq!(check_epub_rules(&ents), Ok(()));
    assert_eq!(ents.iter().map(|e| e.method).collect::<Vec<_>>(), [0, 8, 8, 0]);
    assert_eq!(ents[2].data, b"hello hello hello hello hello hello hello hello");
}

#[test]
fn checker_flags_epub_rule_violations() {
    let ok: &[u8] = b"application/epub+zip";
    let c: (&str, &[u8]) = ("META-INF/container.xml", b"<c/>");
    // mimetype compressed
    let z = write_zip(&[("mimetype", ok, 8), (c.0, c.1, 0)]);
    assert_eq!(check_epub_rules(&check_zip(&z).expect("valid ZIP")), Err(EpubFault::MimetypeCompressed));
    // mimetype not first
    let z = write_zip(&[(c.0, c.1, 0), ("mimetype", ok, 0)]);
    assert_eq!(check_epub_rules(&check_zip(&z).unwrap()), Err(EpubFault::MimetypeNotFirst));
    // wrong content (trailing newline)
    let z = write_zip(&[("mimetype", b"application/epub+zip\n", 0), (c.0, c.1, 0)]);
    assert_eq!(check_epub_rules(&check_zip(&z).unwrap()), Err(EpubFault::MimetypeContent));
    // no container
    let z = write_zip(&[("mimetype", ok, 0), ("OEBPS/x", b"x", 0)]);
    assert_eq!(check_epub_rules(&check_zip(&z).unwrap()), Err(EpubFault::NoContainer));
    // duplicate names
    let z = write_zip(&[("mimetype", ok, 0), (c.0, c.1, 0), (c.0, c.1, 0)]);
    assert_eq!(check_epub_rules(&check_zip(&z).unwrap()), Err(EpubFault::DuplicateName));
}

#[test]
fn checker_flags_zip_level_corruption() {
    let good = build_epub(&mini_spec(EpubVersion::V2, Compression::Stored)).unwrap();
    let good_d = build_epub(&mini_spec(EpubVersion::V2, Compression::Deflate)).unwrap();
    check_zip(&good).unwrap();
    check_zip(&good_d).unwrap();
    let ents = check_zip(&good).unwrap();
    let opf = 2usize;
    assert_eq!(ents[opf].name, "OEBPS/content.opf");
    let (lo, co) = header_offsets(&good, opf);

    // flipped data byte in a stored entry -> CRC
    let mut b = good.clone();
    b[lo + 30 + ents[opf].name.len() + 5] ^= 0x20;
    assert_eq!(check_zip(&b), Err(ZipFault::CrcMismatch(opf)));

    // wrong CRC in both headers
    let mut b = good.clone();
    put32(&mut b, lo + 14, ents[opf].crc ^ 1);
    put32(&mut b, co + 16, ents[opf].crc ^ 1);
    assert_eq!(check_zip(&b), Err(ZipFault::CrcMismatch(opf)));

    // wrong uncompressed size in both headers (stored: comp != size)
    let mut b = good.clone();
    put32(&mut b, lo + 22, ents[opf].size + 1);
    put32(&mut b, co + 24, ents[opf].size + 1);
    assert_eq!(check_zip(&b), Err(ZipFault::SizeMismatch(opf)));

    // central directory disagrees with the local header
    let mut b = good.clone();
    put16(&mut b, co + 10, 8);
    assert_eq!(check_zip(&b), Err(ZipFault::LocalCentralMismatch(opf)));

    // a DEFLATE entry relabelled STORED in both headers: sizes no longer match
    let de = check_zip(&good_d).unwrap();
    let k = 4; // chapter01.xhtml
    assert_eq!(de[k].method, 8);
    assert_ne!(de[k].comp_size, de[k].size, "precondition: this entry really shrank");
    let (lo, co) = header_offsets(&good_d, k);
    let mut b = good_d.clone();
    put16(&mut b, lo + 8, 0);
    put16(&mut b, co + 10, 0);
    assert_eq!(check_zip(&b), Err(ZipFault::SizeMismatch(k)));

    // a STORED entry relabelled DEFLATE in both headers: not a deflate stream
    let mut b = good.clone();
    let (lo1, co1) = header_offsets(&good, 1);
    put16(&mut b, lo1 + 8, 8);
    put16(&mut b, co1 + 10, 8);
    let r = check_zip(&b);
    assert!(
        matches!(r, Err(ZipFault::BadDeflate(1)) | Err(ZipFault::SizeMismatch(1)) | Err(ZipFault::CrcMismatch(1))),
        "stored data labelled as deflate must be rejected, got {r:?}"
    );

    // extra field on the mimetype entry
    let mut b = good.clone();
    put16(&mut b, 28, 4);
    assert_eq!(check_zip(&b), Err(ZipFault::ExtraField(0)));

    // data-descriptor flag
    let mut b = good.clone();
    put16(&mut b, 6, 8);
    put16(&mut b, header_offsets(&good, 0).1 + 8, 8);
    assert_eq!(check_zip(&b), Err(ZipFault::FlagsSet(0)));

    // non-fixed timestamp
    let mut b = good.clone();
    put16(&mut b, 10, 0x6000);
    put16(&mut b, header_offsets(&good, 0).1 + 12, 0x6000);
    assert_eq!(check_zip(&b), Err(ZipFault::NotFixedTimestamp(0)));

    // junk after the EOCD, truncated file, archive comment
    let mut b = good.clone();
    b.push(0);
    assert_eq!(check_zip(&b), Err(ZipFault::BadEocd));
    let b = good[..good.len() - 1].to_vec();
    assert_eq!(check_zip(&b), Err(ZipFault::BadEocd));
    let mut b = good.clone();
    let e = b.len() - 22;
    put16(&mut b, e + 20, 1);
    b.push(b'x');
    assert_eq!(check_zip(&b), Err(ZipFault::BadEocd));
    assert_eq!(check_zip(&good[..10]), Err(ZipFault::TooShort));

    // a gap between entries (garbage inserted before the central directory)
    let mut b = good.clone();
    let cd_off = le32(&b, b.len() - 22 + 16) as usize;
    b.insert(cd_off, 0);
    let e = b.len() - 22;
    put32(&mut b, e + 16, cd_off as u32 + 1);
    assert_eq!(check_zip(&b), Err(ZipFault::Gap(ents.len())));
}

// ---------------------------------------------------------------------------
// XML well-formedness and escaping
// ---------------------------------------------------------------------------

#[test]
fn xml_checker_has_teeth() {
    check_xml(b"<?xml version=\"1.0\"?>\n<!DOCTYPE a>\n<a x=\"1\" y='2'>t&amp;&#233;&#xE9;&quot;<b/> <c k=\"&lt;\">z</c></a>\n").unwrap();
    for (bad, why) in [
        (&b"<a>&</a>"[..], "bare ampersand"),
        (b"<a>&bogus;</a>", "unknown entity"),
        (b"<a>x</b>", "mismatched end tag"),
        (b"<a><b></a>", "unclosed child"),
        (b"<a x=1/>", "unquoted attribute"),
        (b"<a x=\"<\"/>", "raw < in attribute"),
        (b"<a x=\"1\" x=\"2\"/>", "duplicate attribute"),
        (b"<a/><b/>", "two roots"),
        (b"<a>", "unclosed root"),
        (b"text<a/>", "text outside root"),
        (b"<a>\xff</a>", "not UTF-8"),
        (b"<a x=\"&;\"/>", "empty entity"),
    ] {
        assert!(check_xml(bad).is_err(), "must reject: {why}");
    }
}

#[test]
fn every_xml_part_of_every_standard_epub_is_well_formed() {
    for (name, _spec, bytes) in epub_fixtures() {
        let mut parts = 0;
        for e in check_zip(&bytes).unwrap() {
            if e.name.ends_with(".xml") || e.name.ends_with(".opf") || e.name.ends_with(".ncx") || e.name.ends_with(".xhtml") {
                parts += 1;
                check_xml(&e.data).unwrap_or_else(|err| panic!("{name}:{}: {err}", e.name));
            }
        }
        assert!(parts >= 7, "{name}: found only {parts} XML parts");
    }
}

fn special_spec(version: EpubVersion) -> EpubSpec {
    EpubSpec {
        version,
        title: s("A & B <C> \"D\" 'E'"),
        author: s("O'Hara & \"Sons\" <x>"),
        identifier: s("urn:pulp-os:fixture:a&b<c>"),
        chapters: vec![
            chapter("Salt & Pepper <1> \"quoted\"", vec![para(vec![text("1 < 2 & 3 > 2, \"yes\" - 'no'.")]), Block::Heading(s("R&D <h2> \"x\""))]),
            chapter("Two", vec![para(vec![Run::Bold(s("a&b")), text(" and "), Run::Italic(s("<i>")), Run::Break, text("end \"&amp;\".")])]),
        ],
        toc: vec![toc("Salt & Pepper <1> \"quoted\"", 0, vec![]), toc("T&C <2>", 1, vec![])],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    }
}

#[test]
fn special_characters_are_escaped_not_emitted_raw() {
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let spec = special_spec(v);
        let bytes = build_epub(&spec).expect("special characters are never an error");
        let ents = check_zip(&bytes).unwrap();
        for e in ents.iter().filter(|e| e.name != "mimetype") {
            check_xml(&e.data).unwrap_or_else(|err| panic!("{v:?} {}: {err}", e.name));
        }
        let opf = std::str::from_utf8(&ents[2].data).unwrap();
        // the escapes are the contract's, written out
        assert!(opf.contains("A &amp; B &lt;C&gt; &quot;D&quot; 'E'") || opf.contains("A &amp; B &lt;C&gt; &quot;D&quot; &apos;E&apos;"), "{opf}");
        assert!(opf.contains("urn:pulp-os:fixture:a&amp;b&lt;c&gt;"), "{opf}");
        assert!(!opf.contains("<C>") && !opf.contains("a&b"), "raw special characters leaked into the OPF");
        // un-escaping the OPF text gives back the spec
        assert_eq!(xml_unescape(std::str::from_utf8(xml::tag_text(&ents[2].data, b"title").unwrap()).unwrap()), spec.title);
        assert_eq!(xml_unescape(std::str::from_utf8(xml::tag_text(&ents[2].data, b"creator").unwrap()).unwrap()), spec.author);
        assert_eq!(xml_unescape(std::str::from_utf8(xml::tag_text(&ents[2].data, b"identifier").unwrap()).unwrap()), spec.identifier);
        // chapter text
        let c1 = std::str::from_utf8(&ents[4].data).unwrap();
        assert!(c1.contains("1 &lt; 2 &amp; 3 &gt; 2, &quot;yes&quot;"), "{c1}");
        let c2 = std::str::from_utf8(&ents[5].data).unwrap();
        assert!(c2.contains("end &quot;&amp;amp;&quot;."), "a literal '&amp;' in the spec is escaped once more: {c2}");
        assert!(c2.contains("<b>a&amp;b</b>") && c2.contains("<i>&lt;i&gt;</i>") && c2.contains("<br/>"), "{c2}");
        // the production stripper turns the escapes back into the spec's text
        for (i, ch) in spec.chapters.iter().enumerate() {
            let stripped = strip_html_buf(&ents[4 + i].data).unwrap();
            assert_eq!(stripped, expected_chapter_text(&spec, ch), "{v:?} chapter {i}");
        }
    }
}

#[test]
fn numeric_entities_flag_controls_the_encoding_of_non_ascii() {
    let mut spec = mini_spec(EpubVersion::V3, Compression::Stored);
    spec.chapters[0].title = s("Caf\u{e9} \u{201c}Un\u{201d}");
    spec.chapters[0].blocks = vec![para(vec![text("r\u{e9}sum\u{e9} \u{2014} it\u{2019}s ok\u{2026}")])];
    for numeric in [false, true] {
        spec.numeric_entities = numeric;
        let bytes = build_epub(&spec).unwrap();
        let ents = check_zip(&bytes).unwrap();
        let ch = &ents[4].data;
        check_xml(ch).unwrap();
        if numeric {
            assert!(ch.is_ascii(), "numeric_entities: chapter XHTML must be pure ASCII");
            let t = std::str::from_utf8(ch).unwrap();
            for r in ["&#233;", "&#8212;", "&#8217;", "&#8230;", "&#8220;", "&#8221;"] {
                assert!(t.contains(r), "{r} missing in {t}");
            }
            assert!(!t.contains("&#x"), "decimal references only");
        } else {
            let t = std::str::from_utf8(ch).unwrap();
            assert!(t.contains("r\u{e9}sum\u{e9} \u{2014} it\u{2019}s ok\u{2026}") && !t.contains("&#"), "{t}");
        }
        // either way the production stripper yields the identical text
        let want = expected_chapter_text(&spec, &spec.chapters[0]);
        assert_eq!(strip_html_buf(ch).unwrap(), want, "numeric_entities={numeric}");
        // the OPF stays UTF-8 / unescaped for non-ASCII in both modes? (titles are ASCII here)
        assert!(ents[2].data.is_ascii());
    }
}

// ---------------------------------------------------------------------------
// formal parse == spec
// ---------------------------------------------------------------------------

#[test]
fn production_container_and_opf_parse_to_the_spec() {
    for (name, spec, bytes) in epub_fixtures() {
        let p = parse_book(&bytes);
        assert_eq!(p.opf_path, "OEBPS/content.opf", "{name}: parse_container");
        let mut found = [0u8; epub::OPF_PATH_CAP];
        let n = epub::find_opf_in_zip(&p.zip, &mut found).expect("fallback finder");
        assert_eq!(&found[..n], b"OEBPS/content.opf", "{name}: find_opf_in_zip");
        assert_eq!(p.meta.title_str(), spec.title, "{name}: title");
        assert_eq!(p.meta.author_str(), spec.author, "{name}: author");
        assert_eq!(p.spine.len(), spec.chapters.len(), "{name}: spine length");
        for i in 0..spec.chapters.len() {
            assert_eq!(p.zip.entry_name(p.spine.items[i] as usize), chapter_entry_name(i), "{name}: spine[{i}]");
        }
        // language / identifier / rights through the production tag scanner
        assert_eq!(xml::tag_text(&p.opf, b"language"), Some(&b"en"[..]), "{name}");
        assert_eq!(xml::tag_text(&p.opf, b"identifier"), Some(spec.identifier.as_bytes()), "{name}");
        assert_eq!(xml::tag_text(&p.opf, b"rights"), Some(RIGHTS.as_bytes()), "{name}");
        // package version
        let mut versions = Vec::new();
        xml::for_each_tag(&p.opf, b"package", |t| versions.push(xml::get_attr(t, b"version").map(|v| v.to_vec())));
        let want = if spec.version == EpubVersion::V2 { b"2.0".to_vec() } else { b"3.0".to_vec() };
        assert_eq!(versions, vec![Some(want)], "{name}: package version");
        if spec.version == EpubVersion::V3 {
            assert!(String::from_utf8_lossy(&p.opf).contains(&format!(">{MODIFIED}<")), "{name}: dcterms:modified");
        } else {
            assert!(!String::from_utf8_lossy(&p.opf).contains("dcterms"), "{name}");
        }
    }
}

#[test]
fn opf_manifest_and_spine_are_consistent_with_the_zip() {
    for (name, spec, bytes) in epub_fixtures() {
        let p = parse_book(&bytes);
        // (id, href, media-type, properties)
        let mut items: Vec<(String, String, String, Option<String>)> = Vec::new();
        xml::for_each_tag(&p.opf, b"item", |t| {
            let g = |a: &[u8]| xml::get_attr(t, a).map(|v| String::from_utf8(v.to_vec()).unwrap());
            items.push((g(b"id").unwrap(), g(b"href").unwrap(), g(b"media-type").unwrap(), g(b"properties")));
        });
        let hrefs: Vec<&str> = items.iter().map(|i| i.1.as_str()).collect();
        let want: Vec<String> = expected_entry_names(&spec).into_iter().skip(3).map(|n| n.strip_prefix("OEBPS/").unwrap().to_string()).collect();
        let mut a: Vec<&str> = hrefs.clone();
        a.sort();
        let mut b: Vec<&str> = want.iter().map(|s| s.as_str()).collect();
        b.sort();
        assert_eq!(a, b, "{name}: manifest hrefs == nav/ncx + chapters + images");
        let ids: BTreeSet<&str> = items.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(ids.len(), items.len(), "{name}: manifest ids unique");
        for it in &items {
            let mt = if it.1.ends_with(".xhtml") {
                "application/xhtml+xml"
            } else if it.1.ends_with(".ncx") {
                "application/x-dtbncx+xml"
            } else if it.1.ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            assert_eq!(it.2, mt, "{name}: media-type of {}", it.1);
            assert!(p.zip.find(&format!("OEBPS/{}", it.1)).is_some(), "{name}: manifest href {} has no ZIP entry", it.1);
        }
        // nav / ncx declaration
        let nav: Vec<&str> = items.iter().filter(|i| i.3.as_deref().is_some_and(|p| p.split(' ').any(|t| t == "nav"))).map(|i| i.1.as_str()).collect();
        let mut spine_toc = Vec::new();
        xml::for_each_tag(&p.opf, b"spine", |t| spine_toc.push(xml::get_attr(t, b"toc").map(|v| String::from_utf8(v.to_vec()).unwrap())));
        if spec.version == EpubVersion::V3 {
            assert_eq!(nav, ["nav.xhtml"], "{name}: nav item");
            assert!(matches!(p.toc_source, Some(TocSource::Nav(i)) if p.zip.entry_name(i) == "OEBPS/nav.xhtml"), "{name}: find_toc_source");
        } else {
            assert!(nav.is_empty(), "{name}");
            let ncx = items.iter().find(|i| i.1 == "toc.ncx").unwrap();
            assert_eq!(spine_toc, vec![Some(ncx.0.clone())], "{name}: <spine toc> names the NCX item");
            assert!(matches!(p.toc_source, Some(TocSource::Ncx(i)) if p.zip.entry_name(i) == "OEBPS/toc.ncx"), "{name}: find_toc_source");
        }
        // itemrefs, in order, resolve to the chapters in order
        let mut refs = Vec::new();
        xml::for_each_tag(&p.opf, b"itemref", |t| refs.push(String::from_utf8(xml::get_attr(t, b"idref").unwrap().to_vec()).unwrap()));
        assert_eq!(refs.len(), spec.chapters.len(), "{name}");
        for (i, r) in refs.iter().enumerate() {
            let it = items.iter().find(|it| &it.0 == r).unwrap_or_else(|| panic!("{name}: idref {r} not in manifest"));
            assert_eq!(it.1, format!("text/chapter{:02}.xhtml", i + 1), "{name}: itemref {i}");
        }
    }
}

#[test]
fn production_toc_parse_equals_the_flattened_spec_toc() {
    for (name, spec, bytes) in epub_fixtures() {
        let p = parse_book(&bytes);
        let mut want = Vec::new();
        flatten(&spec.toc, &mut want);
        assert!(want.len() >= 4, "{name}: standard TOCs have several entries");
        assert_eq!(p.toc.len(), want.len(), "{name}: TOC entry count");
        for (i, (title, ch)) in want.iter().enumerate() {
            let e = &p.toc.entries[i];
            assert_eq!(e.title_str(), title, "{name}: TOC[{i}] title");
            assert_eq!(e.spine_idx as usize, *ch, "{name}: TOC[{i}] -> spine index");
        }
    }
}

#[test]
fn nested_toc_is_flattened_in_document_order_with_titles_intact() {
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let mut spec = mini_spec(v, Compression::Deflate);
        spec.chapters.push(chapter("Four", vec![para(vec![text("Fourth.")])]));
        spec.chapters.push(chapter("Five", vec![para(vec![text("Fifth.")])]));
        // depth 3, siblings after a deep branch, a title that shares a prefix with its parent,
        // the last entry pointing back at chapter 0
        spec.toc = vec![
            toc("Part I", 0, vec![toc("Part I.a", 1, vec![toc("Part I.a.x", 2, vec![])]), toc("Part I.b", 3, vec![])]),
            toc("Part II", 4, vec![]),
            toc("Back to the start", 0, vec![]),
        ];
        let p = parse_book(&build_epub(&spec).unwrap());
        let got: Vec<(String, u16)> = (0..p.toc.len()).map(|i| (p.toc.entries[i].title_str().to_string(), p.toc.entries[i].spine_idx)).collect();
        let want: Vec<(String, u16)> = [("Part I", 0), ("Part I.a", 1), ("Part I.a.x", 2), ("Part I.b", 3), ("Part II", 4), ("Back to the start", 0)]
            .iter()
            .map(|(t, c)| (t.to_string(), *c))
            .collect();
        assert_eq!(got, want, "{v:?}");
    }
}

#[test]
fn an_epub_without_toc_entries_parses_to_an_empty_toc() {
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let mut spec = mini_spec(v, Compression::Stored);
        spec.toc.clear();
        let bytes = build_epub(&spec).expect("empty TOC is allowed");
        let ents = check_zip(&bytes).unwrap();
        check_xml(&ents[3].data).unwrap();
        let p = parse_book(&bytes);
        assert_eq!(p.toc.len(), 0, "{v:?}");
        assert_eq!(p.spine.len(), 3, "{v:?}: spine is independent of the TOC");
    }
}

#[test]
fn toc_title_boundaries_match_the_production_caps() {
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let mut spec = mini_spec(v, Compression::Stored);
        spec.title = "T".repeat(epub::TITLE_CAP);
        spec.author = "W".repeat(epub::AUTHOR_CAP);
        spec.toc = vec![toc(&"x".repeat(epub::TOC_TITLE_CAP), 0, vec![]), toc(&format!("{}\u{e9}", "y".repeat(epub::TOC_TITLE_CAP - 2)), 1, vec![])];
        let p = parse_book(&build_epub(&spec).unwrap());
        assert_eq!(p.meta.title_str(), spec.title, "{v:?}: 64-byte title survives whole");
        assert_eq!(p.meta.author_str(), spec.author, "{v:?}: 64-byte author survives whole");
        assert_eq!(p.toc.len(), 2);
        assert_eq!(p.toc.entries[0].title_str(), spec.toc[0].title, "{v:?}: 48-byte TOC title");
        assert_eq!(p.toc.entries[1].title_str(), spec.toc[1].title, "{v:?}: 48-byte TOC title ending in a 2-byte char");
    }
}

// KNOWN PRODUCTION LIMITATION (characterised, not endorsed): smol_epub returns the
// OPF title / creator and the NCX / nav entry titles as the raw XML text, so a
// title written `T&amp;C` is shown as "T&amp;C" in the TOC and the library
// (chapter body text IS decoded). This test pins today's behaviour so reader regression does
// not mistake it for a fixture bug; if production starts decoding, it fails and
// must be inverted, and the "no specials in titles" rule of the spec hygiene
// test can be dropped.
#[test]
fn known_limitation_titles_are_not_entity_decoded_by_production() {
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let mut spec = mini_spec(v, Compression::Stored);
        spec.title = s("Fish & Chips");
        spec.author = s("R&D \"Ltd\"");
        spec.toc[0].title = s("T&C <1>");
        let bytes = build_epub(&spec).unwrap();
        let ents = check_zip(&bytes).unwrap();
        for e in ents.iter().filter(|e| e.name != "mimetype") {
            check_xml(&e.data).unwrap_or_else(|err| panic!("{v:?} {}: {err}", e.name));
        }
        let p = parse_book(&bytes);
        assert_eq!(p.meta.title_str(), "Fish &amp; Chips", "{v:?}: dc:title is returned undecoded");
        assert_eq!(p.meta.author_str(), "R&amp;D &quot;Ltd&quot;", "{v:?}: dc:creator is returned undecoded");
        assert_eq!(p.toc.entries[0].title_str(), "T&amp;C &lt;1&gt;", "{v:?}: TOC title is returned undecoded");
        // and decoding it ourselves recovers the spec: the bytes are right, only the reader does not decode
        assert_eq!(xml_unescape(p.meta.title_str()), spec.title);
        assert_eq!(xml_unescape(p.toc.entries[0].title_str()), spec.toc[0].title);
    }
}

// ---------------------------------------------------------------------------
// chapter text: spec -> bytes -> production extract + strip -> spec
// ---------------------------------------------------------------------------

#[test]
fn spec_text_obeys_the_whitespace_normal_form() {
    let mut paragraphs = 0;
    for (name, spec, _) in epub_fixtures() {
        for (ci, ch) in spec.chapters.iter().enumerate() {
            let one_line = |t: &str, what: &str| {
                assert!(!t.is_empty() && !t.contains(['\n', '\r', '\t']) && t.trim() == t && !t.contains("  "), "{name} ch{ci} {what}: {t:?}");
            };
            one_line(&ch.title, "title");
            for b in &ch.blocks {
                match b {
                    Block::Heading(t) => one_line(t, "heading"),
                    Block::Image(i) => assert!(*i < spec.images.len(), "{name}"),
                    Block::Paragraph(runs) => {
                        paragraphs += 1;
                        for r in runs {
                            if let Run::Text(t) | Run::Bold(t) | Run::Italic(t) = r {
                                assert!(!t.is_empty() && !t.contains(['\n', '\r', '\t']), "{name} ch{ci}: run {t:?}");
                            }
                        }
                        let p = plain_of(runs);
                        assert!(!p.is_empty() && !p.starts_with([' ', '\n']) && !p.ends_with([' ', '\n']), "{name} ch{ci}: {p:?}");
                        assert!(!p.contains("  ") && !p.contains(" \n") && !p.contains("\n ") && !p.contains("\n\n"), "{name} ch{ci}: {p:?}");
                    }
                }
            }
        }
        // book title, author and TOC titles: one line and none of & < > " -- the production OPF / NCX /
        // nav parsers return those texts verbatim, WITHOUT decoding entities (see
        // known_limitation_titles_are_not_entity_decoded_by_production), so a spec that
        // contains them could never round-trip. Chapter text and chapter titles (h1) are decoded and
        // may contain them.
        let mut t = Vec::new();
        flatten(&spec.toc, &mut t);
        let mut texts: Vec<String> = t.into_iter().map(|(title, _)| title).collect();
        texts.push(spec.title.clone());
        texts.push(spec.author.clone());
        for title in texts {
            assert!(!title.is_empty() && !title.contains(['\n', '\r', '\t']) && title.trim() == title, "{name}: {title:?}");
            assert!(!title.contains(['&', '<', '>', '"']), "{name}: {title:?} has an XML special character");
        }
    }
    assert!(paragraphs >= 30, "{paragraphs} paragraphs in the standard EPUBs");
}

#[test]
fn production_extract_and_strip_give_the_spec_text_for_every_chapter() {
    let mut chapters_checked = 0;
    for (name, spec, bytes) in epub_fixtures() {
        let zip = open_zip(&bytes);
        for (i, ch) in spec.chapters.iter().enumerate() {
            let entry_name = chapter_entry_name(i);
            let idx = zip.find(&entry_name).unwrap();
            let e = zip.entry(idx);
            let want = expected_chapter_text(&spec, ch);

            // route 1: extract the whole entry, then the in-memory stripper
            let xhtml = extract_entry(e, e.local_offset, read_at(&bytes)).unwrap();
            let got = strip_html_buf(&xhtml).unwrap();
            assert_eq!(got, want, "{name} {entry_name} (method {}): extract_entry + strip_html_buf", e.method);

            // route 2: the streaming pipeline the reader uses (4 KB chunks, no whole-entry buffer)
            let mut streamed = Vec::new();
            let n = stream_strip_entry(e, e.local_offset, read_at(&bytes), |chunk| {
                streamed.extend_from_slice(chunk);
                Ok(())
            })
            .unwrap();
            assert_eq!(n as usize, streamed.len(), "{name} {entry_name}: reported length");
            assert_eq!(streamed, want, "{name} {entry_name} (method {}): stream_strip_entry", e.method);
            chapters_checked += 1;
        }
    }
    assert!(chapters_checked >= 20, "{chapters_checked} chapters checked");
}

#[test]
fn standard_epubs_have_entries_longer_than_three_stream_chunks_in_both_methods() {
    let (mut big_stored, mut big_deflate) = (false, false);
    for (_, _, bytes) in epub_fixtures() {
        for e in check_zip(&bytes).unwrap().iter().filter(|e| e.name.contains("/text/")) {
            if e.size as usize >= 3 * STREAM_CHUNK {
                big_stored |= e.method == METHOD_STORED;
                big_deflate |= e.method == METHOD_DEFLATE;
            }
        }
    }
    assert!(big_stored, "no STORED chapter of >= {} bytes", 3 * STREAM_CHUNK);
    assert!(big_deflate, "no DEFLATE chapter of >= {} bytes", 3 * STREAM_CHUNK);
}

#[test]
fn hand_written_chapter_exercises_every_markup_form() {
    let spec = EpubSpec {
        version: EpubVersion::V2,
        title: s("Markup"),
        author: s("M. Up"),
        identifier: s("urn:pulp-os:fixture:markup"),
        chapters: vec![chapter(
            "Every Tag",
            vec![
                Block::Heading(s("A Heading")),
                para(vec![text("Plain, "), Run::Bold(s("bold")), text(", "), Run::Italic(s("italic")), text(", "), Run::Bold(s("b&b")), text(" end.")]),
                para(vec![text("Line one"), Run::Break, text("Line two"), Run::Break, Run::Italic(s("Line three"))]),
                para(vec![text("Quotes: \u{201c}smart\u{201d} and \u{2018}single\u{2019}, caf\u{e9}, dash \u{2014} ellipsis\u{2026} & \"plain\" <ok>.")]),
                para(vec![text("Last.")]),
            ],
        )],
        toc: vec![toc("Every Tag", 0, vec![])],
        images: vec![],
        cover: None,
        compression: Compression::Mixed,
        numeric_entities: false,
    };
    for numeric in [false, true] {
        let mut sp = spec.clone();
        sp.numeric_entities = numeric;
        let bytes = build_epub(&sp).unwrap();
        let ents = check_zip(&bytes).unwrap();
        // hand-computed expectation of the stripped text (not produced by the code under test)
        let mut want: Vec<u8> = Vec::new();
        want.extend_from_slice(&[M, b'H']);
        want.extend_from_slice(b"Every Tag");
        want.extend_from_slice(&[M, b'h']);
        want.extend_from_slice(b"\n\n");
        want.extend_from_slice(&[M, b'H']);
        want.extend_from_slice(b"A Heading");
        want.extend_from_slice(&[M, b'h']);
        want.extend_from_slice(b"\n\nPlain, ");
        want.extend_from_slice(&[M, b'B']);
        want.extend_from_slice(b"bold");
        want.extend_from_slice(&[M, b'b']);
        want.extend_from_slice(b", ");
        want.extend_from_slice(&[M, b'I']);
        want.extend_from_slice(b"italic");
        want.extend_from_slice(&[M, b'i']);
        want.extend_from_slice(b", ");
        want.extend_from_slice(&[M, b'B']);
        want.extend_from_slice(b"b&b");
        want.extend_from_slice(&[M, b'b']);
        want.extend_from_slice(b" end.\n\nLine one\nLine two\n");
        want.extend_from_slice(&[M, b'I']);
        want.extend_from_slice(b"Line three");
        want.extend_from_slice(&[M, b'i']);
        want.extend_from_slice(b"\n\nQuotes: \xE2\x80\x9Csmart\xE2\x80\x9D and \xE2\x80\x98single\xE2\x80\x99, caf\xC3\xA9, dash \xE2\x80\x94 ellipsis\xE2\x80\xA6 & \"plain\" <ok>.\n\nLast.\n");
        assert_eq!(strip_html_buf(&ents[4].data).unwrap(), want, "numeric_entities={numeric}");
        // the contract's tags
        let html = String::from_utf8(ents[4].data.clone()).unwrap();
        for tag in ["<h1>Every Tag</h1>", "<h2>A Heading</h2>", "<p>Plain, <b>bold</b>, <i>italic</i>, <b>b&amp;b</b> end.</p>", "<p>Line one<br/>Line two<br/><i>Line three</i></p>", "<p>Last.</p>"] {
            assert!(html.contains(tag), "missing {tag} in {html}");
        }
        let zip = open_zip(&bytes);
        let e = zip.entry(zip.find("OEBPS/text/chapter01.xhtml").unwrap());
        let mut streamed = Vec::new();
        stream_strip_entry(e, e.local_offset, read_at(&bytes), |c| {
            streamed.extend_from_slice(c);
            Ok(())
        })
        .unwrap();
        assert_eq!(streamed, want, "numeric_entities={numeric}: streamed");
    }
}

#[test]
fn chapter_head_title_is_not_part_of_the_stripped_text() {
    // <head><title> must not leak into the body text: the expectation above has exactly one title (the h1)
    let mut spec = mini_spec(EpubVersion::V2, Compression::Stored);
    spec.chapters[0].title = s("Unique Chapter Title");
    let ents = check_zip(&build_epub(&spec).unwrap()).unwrap();
    let html = String::from_utf8(ents[4].data.clone()).unwrap();
    assert!(html.contains("<title>Unique Chapter Title</title>") && html.contains("<h1>Unique Chapter Title</h1>"), "{html}");
    let stripped = strip_html_buf(&ents[4].data).unwrap();
    assert_eq!(stripped.windows(20).filter(|w| *w == b"Unique Chapter Title").count(), 1);
}

// ---------------------------------------------------------------------------
// images
// ---------------------------------------------------------------------------

fn black(img: &DecodedImage, x: usize, y: usize) -> bool {
    (img.data[y * img.stride + x / 8] >> (7 - x % 8)) & 1 == 1
}

// the decoded 1-bit bitmap against the spec (exact for Black / White / Checker,
// which only ever contain pure black / pure white so no dither can change them;
// a density property for the gradient)
fn assert_decoded_matches(img: &DecodedImage, spec: &ImageSpec, what: &str) {
    let (w, h) = (spec.width as usize, spec.height as usize);
    assert_eq!((img.width as usize, img.height as usize), (w, h), "{what}: decoded size");
    assert_eq!(img.stride, w.div_ceil(8), "{what}: stride");
    assert_eq!(img.data.len(), img.stride * h, "{what}: data length");
    match spec.pattern {
        Pattern::Black | Pattern::White | Pattern::Checker { .. } => {
            for y in 0..h {
                for x in 0..w {
                    let want = match spec.pattern {
                        Pattern::Black => true,
                        Pattern::White => false,
                        Pattern::Checker { cell } => (x / cell as usize + y / cell as usize) % 2 == 0,
                        Pattern::HorizontalGradient => unreachable!(),
                    };
                    assert_eq!(black(img, x, y), want, "{what}: pixel ({x},{y}) black?");
                }
            }
        }
        Pattern::HorizontalGradient => {
            let q = w / 4;
            let count = |x0: usize, x1: usize| (0..h).map(|y| (x0..x1).filter(|&x| black(img, x, y)).count()).sum::<usize>() as f64 / (h * (x1 - x0)) as f64;
            let (left, right) = (count(0, q), count(w - q, w));
            assert!(left > 0.70, "{what}: left quarter of a black->white ramp is only {left:.2} black");
            assert!(right < 0.30, "{what}: right quarter of a black->white ramp is {right:.2} black");
            assert!(left > right + 0.4, "{what}: ramp direction (left {left:.2}, right {right:.2})");
        }
    }
}

fn is_jpeg(k: ImageKind) -> bool {
    k == ImageKind::Jpeg
}

fn decode_in_memory(data: &[u8], k: ImageKind, mw: u16, mh: u16) -> Result<DecodedImage, &'static str> {
    if is_jpeg(k) { jpeg::decode_jpeg_fit(data, mw, mh) } else { png::decode_png_fit(data, mw, mh) }
}

// decode straight out of the EPUB bytes by the entry's own method: the
// STORED / DEFLATE streaming paths the reader uses
fn decode_from_zip(bytes: &[u8], zip: &ZipIndex, name: &str, k: ImageKind, mw: u16, mh: u16) -> Result<DecodedImage, &'static str> {
    let idx = zip.find(name).unwrap_or_else(|| panic!("{name} not in the ZIP"));
    let e = zip.entry(idx);
    let mut header = [0u8; 30];
    read_at(bytes)(e.local_offset, &mut header).unwrap();
    let off = e.local_offset + ZipIndex::local_header_data_skip(&header).unwrap();
    match (is_jpeg(k), e.method) {
        (false, METHOD_STORED) => png::decode_png_streaming(read_at(bytes), off, e.comp_size, mw, mh),
        (false, METHOD_DEFLATE) => png::decode_png_deflate_streaming(read_at(bytes), off, e.comp_size, mw, mh),
        (true, METHOD_STORED) => jpeg::decode_jpeg_streaming(read_at(bytes), off, e.comp_size, mw, mh),
        (true, METHOD_DEFLATE) => jpeg::decode_jpeg_deflate_streaming(read_at(bytes), off, e.comp_size, e.uncomp_size, mw, mh),
        (_, m) => panic!("method {m}"),
    }
}

#[test]
fn every_standard_image_is_a_valid_png_or_jpeg_of_the_specified_pixels() {
    let mut count = 0;
    for (name, spec, bytes) in epub_fixtures() {
        let ents = check_zip(&bytes).unwrap();
        for im in &spec.images {
            count += 1;
            let ent = ents.iter().find(|e| e.name == format!("OEBPS/{}", im.path)).unwrap_or_else(|| panic!("{name}: {} missing", im.path));
            let r = if is_jpeg(im.kind) { check_jpeg(&ent.data, im) } else { check_png(&ent.data, im) };
            r.unwrap_or_else(|e| panic!("{name}: {}: {e}", im.path));
            match im.kind {
                ImageKind::Jpeg => assert!(im.path.ends_with(".jpg") || im.path.ends_with(".jpeg"), "{}", im.path),
                _ => assert!(im.path.ends_with(".png"), "{}", im.path),
            }
        }
    }
    assert!(count >= 12, "{count} images in the standard set");
}

#[test]
fn production_decoders_reproduce_the_spec_pixels_through_memory_and_both_zip_paths() {
    for (name, spec, bytes) in epub_fixtures() {
        let zip = open_zip(&bytes);
        for im in &spec.images {
            let entry = format!("OEBPS/{}", im.path);
            let what = format!("{name}:{} ({:?}, method {})", im.path, im.kind, zip.entry(zip.find(&entry).unwrap()).method);
            // in-memory decode of the independently decompressed bytes
            let raw = entry_bytes(&bytes, &zip, &entry);
            let a = decode_in_memory(&raw, im.kind, 4096, 4096).unwrap_or_else(|e| panic!("{what}: in-memory decode: {e}"));
            assert_decoded_matches(&a, im, &format!("{what} [memory]"));
            // streaming decode out of the archive, STORED or DEFLATE as the entry is
            let b = decode_from_zip(&bytes, &zip, &entry, im.kind, 4096, 4096).unwrap_or_else(|e| panic!("{what}: zip decode: {e}"));
            assert_decoded_matches(&b, im, &format!("{what} [zip stream]"));
            assert_eq!(a.data, b.data, "{what}: both routes decode identically");
            // dimension peeks
            if is_jpeg(im.kind) {
                assert_eq!(jpeg::peek_jpeg_dimensions(&raw), Ok((im.width, im.height)), "{what}: peek_jpeg_dimensions");
            } else {
                assert_eq!(png::peek_png_dimensions(&raw), Ok((im.width as u32, im.height as u32)), "{what}: peek_png_dimensions");
            }
        }
    }
}

#[test]
fn standard_images_cover_every_kind_and_both_zip_methods_with_distinct_sizes() {
    let mut pairs: BTreeSet<(String, u16)> = BTreeSet::new();
    let mut png_sizes: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut kinds_in_book: Vec<(String, BTreeSet<String>)> = Vec::new();
    for (name, spec, bytes) in epub_fixtures() {
        let ents = check_zip(&bytes).unwrap();
        let mut ks = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for im in &spec.images {
            assert!(paths.insert(im.path.clone()), "{name}: duplicate image path {}", im.path);
            let m = ents.iter().find(|e| e.name == format!("OEBPS/{}", im.path)).unwrap().method;
            pairs.insert((format!("{:?}", im.kind), m));
            ks.insert(format!("{:?}", im.kind));
            if !is_jpeg(im.kind) {
                png_sizes.insert((im.width, im.height));
            }
        }
        kinds_in_book.push((name, ks));
    }
    for k in ["PngGray1", "PngGray8", "PngPalette8", "Jpeg"] {
        for m in [METHOD_STORED, METHOD_DEFLATE] {
            assert!(pairs.contains(&(k.to_string(), m)), "no {k} image stored with method {m}: add one so that decoder path has a fixture");
        }
    }
    // the 1-bit, greyscale and palette PNGs differ in size
    assert!(png_sizes.len() >= 3, "PNG sizes {png_sizes:?}");
    for (name, ks) in &kinds_in_book {
        if name.contains("MIXED") {
            assert_eq!(ks.len(), 4, "{name}: a MIXED book carries all four image kinds, has {ks:?}");
        }
    }
    // each of the three PNG kinds has its own dimensions in at least one book
    let mut per_kind: std::collections::BTreeMap<String, BTreeSet<(u16, u16)>> = Default::default();
    for (_, spec, _) in epub_fixtures() {
        for im in &spec.images {
            per_kind.entry(format!("{:?}", im.kind)).or_default().insert((im.width, im.height));
        }
    }
    let g1 = &per_kind["PngGray1"];
    let g8 = &per_kind["PngGray8"];
    let pl = &per_kind["PngPalette8"];
    assert!(g1.is_disjoint(g8) && g1.is_disjoint(pl) && g8.is_disjoint(pl), "1-bit {g1:?}, grey {g8:?}, palette {pl:?} must not share a size");
}

#[test]
fn decoders_agree_with_independently_derivable_images_of_every_kind() {
    // black stays black, white stays white, a checker is exact (dither cannot
    // change pure 0 / 255 samples); odd sizes exercise padding bits and
    // non-multiple-of-8 widths; JPEG sizes are multiples of 8 by contract
    let mut cases: Vec<ImageSpec> = Vec::new();
    for kind in [ImageKind::PngGray1, ImageKind::PngGray8, ImageKind::PngPalette8] {
        for (w, h) in [(1u16, 1u16), (7, 5), (8, 8), (9, 3), (33, 17)] {
            for pattern in [Pattern::Black, Pattern::White, Pattern::Checker { cell: 1 }, Pattern::Checker { cell: 3 }] {
                cases.push(image("images/t.png", kind, w, h, pattern));
            }
        }
    }
    for (w, h) in [(8u16, 8u16), (16, 8), (24, 16), (40, 32)] {
        for pattern in [Pattern::Black, Pattern::White, Pattern::Checker { cell: 8 }, Pattern::Checker { cell: 16 }] {
            cases.push(image("images/t.jpg", ImageKind::Jpeg, w, h, pattern));
        }
    }
    for compression in [Compression::Stored, Compression::Deflate] {
        for im in &cases {
            let mut spec = mini_spec(EpubVersion::V3, compression);
            spec.images = vec![im.clone()];
            spec.chapters[0].blocks.push(Block::Image(0));
            let bytes = build_epub(&spec).unwrap_or_else(|e| panic!("{im:?}: {e:?}"));
            let ents = check_zip(&bytes).unwrap();
            let ent = ents.last().unwrap();
            let what = format!("{im:?} {compression:?}");
            if is_jpeg(im.kind) { check_jpeg(&ent.data, im) } else { check_png(&ent.data, im) }.unwrap_or_else(|e| panic!("{what}: {e}"));
            let zip = open_zip(&bytes);
            let d = decode_from_zip(&bytes, &zip, &format!("OEBPS/{}", im.path), im.kind, 4096, 4096).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_decoded_matches(&d, im, &what);
        }
    }
}

#[test]
fn gradients_decode_as_dark_to_light_in_png_and_jpeg() {
    for (kind, w, h) in [(ImageKind::PngGray8, 64u16, 16u16), (ImageKind::Jpeg, 72, 16)] {
        let im = image(if is_jpeg(kind) { "images/g.jpg" } else { "images/g.png" }, kind, w, h, Pattern::HorizontalGradient);
        let mut spec = mini_spec(EpubVersion::V2, Compression::Deflate);
        spec.images = vec![im.clone()];
        spec.chapters[0].blocks.push(Block::Image(0));
        let bytes = build_epub(&spec).unwrap();
        let ents = check_zip(&bytes).unwrap();
        if is_jpeg(kind) { check_jpeg(&ents.last().unwrap().data, &im) } else { check_png(&ents.last().unwrap().data, &im) }.unwrap();
        let zip = open_zip(&bytes);
        let d = decode_from_zip(&bytes, &zip, &format!("OEBPS/{}", im.path), kind, 4096, 4096).unwrap();
        assert_decoded_matches(&d, &im, &format!("{kind:?} gradient"));
    }
}

#[test]
fn production_integer_downscale_halves_a_checker_and_keeps_pure_images_pure() {
    for (kind, w, h, path) in [(ImageKind::PngGray8, 64u16, 32u16, "images/d.png"), (ImageKind::Jpeg, 64, 32, "images/d.jpg")] {
        // fit into half the size: integer factor 2 (64x32 -> 32x16)
        for pattern in [Pattern::Black, Pattern::White, Pattern::Checker { cell: 16 }] {
            let im = image(path, kind, w, h, pattern);
            let mut spec = mini_spec(EpubVersion::V2, Compression::Stored);
            spec.images = vec![im.clone()];
            spec.chapters[0].blocks.push(Block::Image(0));
            let bytes = build_epub(&spec).unwrap();
            let zip = open_zip(&bytes);
            let raw = entry_bytes(&bytes, &zip, &format!("OEBPS/{path}"));
            let small = decode_in_memory(&raw, kind, w / 2, h / 2).unwrap();
            assert_eq!((small.width, small.height), (w / 2, h / 2), "{kind:?} {pattern:?}: fit into {}x{}", w / 2, h / 2);
            let n = small.width as usize * small.height as usize;
            let blacks = (0..small.height as usize).map(|y| (0..small.width as usize).filter(|&x| black(&small, x, y)).count()).sum::<usize>();
            match pattern {
                Pattern::Black => assert_eq!(blacks, n, "{kind:?}: black stays black"),
                Pattern::White => assert_eq!(blacks, 0, "{kind:?}: white stays white"),
                _ => assert_eq!(blacks, n / 2, "{kind:?}: a 16 px checker downscaled by 2 stays half black"),
            }
            // streaming path honours the same cap
            let via_zip = decode_from_zip(&bytes, &zip, &format!("OEBPS/{path}"), kind, w / 2, h / 2).unwrap();
            assert_eq!((via_zip.width, via_zip.height), (w / 2, h / 2));
            assert_eq!(via_zip.data, small.data, "{kind:?} {pattern:?}: memory and zip decode agree after downscaling");
        }
    }
}

#[test]
fn chapter_image_references_resolve_to_the_images_in_the_zip() {
    let mut refs_total = 0;
    for (name, spec, bytes) in epub_fixtures() {
        let zip = open_zip(&bytes);
        for (ci, ch) in spec.chapters.iter().enumerate() {
            let xhtml = entry_bytes(&bytes, &zip, &chapter_entry_name(ci));
            let stripped = strip_html_buf(&xhtml).unwrap();
            // every [M,'P',len,src] in order
            let mut found: Vec<String> = Vec::new();
            let mut i = 0;
            while i + 2 < stripped.len() {
                if stripped[i] == M && stripped[i + 1] == b'P' {
                    let len = stripped[i + 2] as usize;
                    found.push(String::from_utf8(stripped[i + 3..i + 3 + len].to_vec()).unwrap());
                    i += 3 + len;
                } else {
                    i += 1;
                }
            }
            let want: Vec<&str> = ch.blocks.iter().filter_map(|b| if let Block::Image(k) = b { Some(spec.images[*k].path.as_str()) } else { None }).collect();
            assert_eq!(found.len(), want.len(), "{name} ch{ci}: image references");
            for (src, path) in found.iter().zip(want) {
                assert_eq!(src, &format!("../{path}"), "{name} ch{ci}: raw src");
                assert!(String::from_utf8_lossy(&xhtml).contains(&format!("<p><img src=\"{src}\" alt=\"\"/></p>")), "{name} ch{ci}: <img> markup");
                let mut out = [0u8; 512];
                let n = epub::resolve_path(dir_of(&chapter_entry_name(ci)), src, &mut out);
                let resolved = std::str::from_utf8(&out[..n]).unwrap();
                assert_eq!(resolved, format!("OEBPS/{path}"), "{name} ch{ci}: resolve_path");
                assert!(zip.find(resolved).is_some(), "{name}: {resolved} is a ZIP entry");
                refs_total += 1;
            }
        }
    }
    assert!(refs_total >= 8, "{refs_total} inline image references in the standard set");
}

#[test]
fn every_image_is_referenced_and_every_reference_exists() {
    for (name, spec, _) in epub_fixtures() {
        let referenced: BTreeSet<usize> = spec.chapters.iter().flat_map(|c| c.blocks.iter()).filter_map(|b| if let Block::Image(i) = b { Some(*i) } else { None }).collect();
        for i in 0..spec.images.len() {
            assert!(referenced.contains(&i) || spec.cover == Some(i), "{name}: image {i} is neither shown inline nor the cover");
        }
    }
}

// cover declaration (no production cover API exists: smol-epub neither reads
// nor exposes it; the declaration is checked through the public tag scanner)
fn cover_declared(opf: &[u8], version: EpubVersion) -> Vec<String> {
    let mut items: Vec<(String, String, Option<String>)> = Vec::new();
    xml::for_each_tag(opf, b"item", |t| {
        let g = |a: &[u8]| xml::get_attr(t, a).map(|v| String::from_utf8(v.to_vec()).unwrap());
        items.push((g(b"id").unwrap(), g(b"href").unwrap(), g(b"properties")));
    });
    match version {
        EpubVersion::V3 => items.iter().filter(|i| i.2.as_deref().is_some_and(|p| p.split(' ').any(|t| t == "cover-image"))).map(|i| i.1.clone()).collect(),
        EpubVersion::V2 => {
            let mut ids = Vec::new();
            xml::for_each_tag(opf, b"meta", |t| {
                if xml::get_attr(t, b"name") == Some(&b"cover"[..]) {
                    ids.push(String::from_utf8(xml::get_attr(t, b"content").unwrap().to_vec()).unwrap());
                }
            });
            ids.iter().map(|id| items.iter().find(|i| &i.0 == id).unwrap_or_else(|| panic!("cover meta names unknown item {id}")).1.clone()).collect()
        }
    }
}

#[test]
fn cover_images_are_declared_the_epub2_and_epub3_way() {
    let mut with_cover = BTreeSet::new();
    for (name, spec, bytes) in epub_fixtures() {
        let p = parse_book(&bytes);
        let got = cover_declared(&p.opf, spec.version);
        match spec.cover {
            Some(c) => {
                assert_eq!(got, vec![spec.images[c].path.clone()], "{name}: cover declaration");
                with_cover.insert(format!("{:?}", spec.version));
            }
            None => assert!(got.is_empty(), "{name}: no cover declared"),
        }
        // the other version's mechanism must not be present
        let opf = String::from_utf8_lossy(&p.opf);
        match spec.version {
            EpubVersion::V2 => assert!(!opf.contains("cover-image"), "{name}"),
            EpubVersion::V3 => assert!(!opf.contains("name=\"cover\""), "{name}"),
        }
    }
    assert_eq!(with_cover.len(), 2, "both an EPUB2 and an EPUB3 standard book declare a cover");
    // an explicit non-zero cover index
    for v in [EpubVersion::V2, EpubVersion::V3] {
        let mut spec = mini_spec(v, Compression::Stored);
        spec.images = vec![
            image("images/a.png", ImageKind::PngGray1, 8, 8, Pattern::Black),
            image("images/b.png", ImageKind::PngGray8, 8, 8, Pattern::White),
        ];
        spec.chapters[0].blocks.push(Block::Image(0));
        spec.cover = Some(1);
        let p = parse_book(&build_epub(&spec).unwrap());
        assert_eq!(cover_declared(&p.opf, v), vec![s("images/b.png")], "{v:?}");
    }
}

// ---------------------------------------------------------------------------
// coverage matrix of the standard EPUB specs (what reader regression can rely on)
// ---------------------------------------------------------------------------

#[test]
fn standard_epubs_carry_toc_markup_entities_and_images_for_t6() {
    let books = epub_fixtures();
    let mut runs = BTreeSet::new();
    let mut blocks = BTreeSet::new();
    let (mut numeric, mut raw) = (false, false);
    for (name, spec, _) in &books {
        assert!(spec.chapters.len() >= 4, "{name}: >= 4 chapters (spine navigation needs a few)");
        let mut flat = Vec::new();
        flatten(&spec.toc, &mut flat);
        assert!(flat.len() >= 4, "{name}: TOC entries");
        assert!(spec.toc.iter().any(|t| !t.children.is_empty()), "{name}: nested TOC");
        assert!(!spec.images.is_empty(), "{name}: at least one image");
        let mut targets: Vec<usize> = flat.iter().map(|(_, c)| *c).collect();
        targets.sort();
        targets.dedup();
        assert!(targets.len() >= 4, "{name}: TOC points at >= 4 distinct chapters");
        numeric |= spec.numeric_entities;
        raw |= !spec.numeric_entities;
        for ch in &spec.chapters {
            for b in &ch.blocks {
                match b {
                    Block::Paragraph(rs) => {
                        blocks.insert("paragraph");
                        for r in rs {
                            runs.insert(match r {
                                Run::Text(_) => "text",
                                Run::Bold(_) => "bold",
                                Run::Italic(_) => "italic",
                                Run::Break => "break",
                            });
                        }
                    }
                    Block::Heading(_) => {
                        blocks.insert("heading");
                    }
                    Block::Image(_) => {
                        blocks.insert("image");
                    }
                }
            }
        }
    }
    assert_eq!(runs, BTreeSet::from(["text", "bold", "italic", "break"]));
    assert_eq!(blocks, BTreeSet::from(["paragraph", "heading", "image"]));
    assert!(numeric && raw, "both numeric-entity and raw-UTF-8 chapters exist");
    // each version has a nested TOC and a cover book
    for v in [EpubVersion::V2, EpubVersion::V3] {
        assert_eq!(books.iter().filter(|(_, s, _)| s.version == v).count(), 3);
    }
}

// ---------------------------------------------------------------------------
// negative specs
// ---------------------------------------------------------------------------

#[test]
fn invalid_specs_are_rejected_with_the_contracted_error() {
    let base = || {
        let mut s = mini_spec(EpubVersion::V3, Compression::Stored);
        s.images = vec![image("images/a.png", ImageKind::PngGray1, 8, 8, Pattern::Black)];
        s
    };
    build_epub(&base()).expect("base spec is valid");

    let mut sp = base();
    sp.chapters.clear();
    sp.toc.clear();
    assert_eq!(build_epub(&sp), Err(SpecError::NoChapters));

    let mut sp = base();
    sp.chapters[1].blocks.clear();
    assert_eq!(build_epub(&sp), Err(SpecError::EmptyChapter(1)));

    let mut sp = base();
    sp.toc[0].chapter = 3;
    assert_eq!(build_epub(&sp), Err(SpecError::BadTocTarget(3)));

    let mut sp = base();
    sp.toc[1].children[0].chapter = 9; // nested entry
    assert_eq!(build_epub(&sp), Err(SpecError::BadTocTarget(9)));

    let mut sp = base();
    sp.chapters[0].blocks.push(Block::Image(1));
    assert_eq!(build_epub(&sp), Err(SpecError::BadImageRef(1)));

    let mut sp = base();
    sp.cover = Some(5);
    assert_eq!(build_epub(&sp), Err(SpecError::BadImageRef(5)));

    // image defects (image index 0 is the broken one, index 1 stays fine)
    let bad_images = [
        image("images/a.png", ImageKind::PngGray1, 0, 8, Pattern::Black),
        image("images/a.png", ImageKind::PngGray8, 8, 0, Pattern::Black),
        image("images/a.png", ImageKind::PngGray1, 8, 8, Pattern::Checker { cell: 0 }),
        image("images/a.png", ImageKind::PngGray1, 8, 8, Pattern::HorizontalGradient),
        image("images/a.png", ImageKind::PngPalette8, 8, 8, Pattern::HorizontalGradient),
        image("images/a.jpg", ImageKind::Jpeg, 70, 8, Pattern::Black),
        image("images/a.jpg", ImageKind::Jpeg, 8, 12, Pattern::Black),
        image("images/a.jpg", ImageKind::Jpeg, 16, 16, Pattern::Checker { cell: 4 }),
    ];
    for bad in bad_images {
        let mut sp = base();
        sp.images = vec![bad.clone(), image("images/ok.png", ImageKind::PngGray1, 8, 8, Pattern::White)];
        assert_eq!(build_epub(&sp), Err(SpecError::BadImage(0)), "{bad:?}");
        let mut sp = base();
        sp.images = vec![image("images/ok.png", ImageKind::PngGray1, 8, 8, Pattern::White), bad.clone()];
        assert_eq!(build_epub(&sp), Err(SpecError::BadImage(1)), "{bad:?} as the second image");
    }
    // gradient is fine where it is defined
    for ok in [
        image("images/a.png", ImageKind::PngGray8, 8, 8, Pattern::HorizontalGradient),
        image("images/a.jpg", ImageKind::Jpeg, 16, 8, Pattern::HorizontalGradient),
        image("images/a.png", ImageKind::PngPalette8, 1, 1, Pattern::Checker { cell: 1 }),
    ] {
        let mut sp = base();
        sp.images = vec![ok.clone()];
        build_epub(&sp).unwrap_or_else(|e| panic!("{ok:?}: {e:?}"));
    }
}

#[test]
fn title_author_and_toc_title_limits_count_bytes() {
    let base = mini_spec(EpubVersion::V2, Compression::Stored);
    let t = |n: usize| "t".repeat(n);
    let mut sp = base.clone();
    sp.title = t(epub::TITLE_CAP);
    build_epub(&sp).expect("64-byte title");
    sp.title = t(epub::TITLE_CAP + 1);
    assert_eq!(build_epub(&sp), Err(SpecError::TitleTooLong));
    // 33 two-byte characters = 66 bytes but only 33 chars
    sp.title = "\u{e9}".repeat(33);
    assert_eq!(build_epub(&sp), Err(SpecError::TitleTooLong), "limits are in bytes");
    sp.title = "\u{e9}".repeat(32);
    build_epub(&sp).expect("32 two-byte characters = 64 bytes");

    let mut sp = base.clone();
    sp.author = t(epub::AUTHOR_CAP + 1);
    assert_eq!(build_epub(&sp), Err(SpecError::TitleTooLong));
    sp.author = t(epub::AUTHOR_CAP);
    build_epub(&sp).expect("64-byte author");

    let mut sp = base.clone();
    sp.toc[0].title = t(epub::TOC_TITLE_CAP + 1);
    assert_eq!(build_epub(&sp), Err(SpecError::TitleTooLong));
    sp.toc[0].title = t(epub::TOC_TITLE_CAP);
    build_epub(&sp).expect("48-byte TOC title");
    let mut sp = base;
    sp.toc[1].children[0].title = t(epub::TOC_TITLE_CAP + 1);
    assert_eq!(build_epub(&sp), Err(SpecError::TitleTooLong), "nested entries obey the cap too");
}

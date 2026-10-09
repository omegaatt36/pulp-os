// Shared helpers of the smol_* test files: raw EPUB / ZIP builders (arbitrary bytes in
// chapters, titles and TOC labels), a driver for the production parse chain, and the
// routes that turn a chapter entry into stripped text (streaming, in-memory, async,
// chunked `HtmlStripStream`). Nothing here decodes or strips text: expected text comes
// from std (`str::from_utf8`, `String::from_utf8_lossy`, `char`) or is written by hand.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::future::Future;
use std::task::{Context, Poll, Waker};

use pulp_host::fixtures::{RawEntry, build_zip};
use smol_epub::async_io::{AsyncReadAt, AsyncWriteChunk, stream_strip_entry_async};
use smol_epub::cache::{stream_strip_entry, strip_html_buf};
use smol_epub::epub::{self, EpubMeta, EpubSpine, EpubToc};
use smol_epub::html_strip::{HtmlStripStream, MARKER};
use smol_epub::zip::{ZipIndex, extract_entry};

pub const FFFD: char = '\u{FFFD}';

// 2-byte (C3 A9), 3-byte (E8 87 BA) and 4-byte (F0 A0 AE B7) scalars
pub const E_ACUTE: &str = "\u{e9}";
pub const TAI: &str = "\u{81fa}";
pub const YOSHI: &str = "\u{20bb7}";
// 2-, 3- and 4-byte scalars with CJK punctuation: e-acute, Taiwan, corner quotes,
// "traditional Chinese", full-width comma, YOSHI
pub const MIXED_UNIT: &str =
    "\u{e9}\u{81fa}\u{7063}\u{300c}\u{7e41}\u{9ad4}\u{4e2d}\u{6587}\u{300d}\u{ff0c}\u{20bb7}";

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

// ---------------------------------------------------------------------------
// text helpers
// ---------------------------------------------------------------------------

pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

// exactly `n` ASCII bytes of lower-case words: single spaces only, never a leading or
// trailing space (so HTML white-space collapsing leaves it unchanged); random enough to
// keep DEFLATE from collapsing it
pub fn filler(n: usize, seed: u64) -> String {
    let mut rng = Lcg(seed);
    let mut out = String::with_capacity(n);
    for i in 0..n {
        let space = i > 0 && i + 1 < n && !out.ends_with(' ') && rng.next() % 7 == 0;
        out.push(if space {
            ' '
        } else {
            (b'a' + (rng.next() % 26) as u8) as char
        });
    }
    out
}

// the longest prefix of `s` on a scalar boundary that is at most `cap` bytes
pub fn fitting_prefix(s: &str, cap: usize) -> &str {
    let mut n = cap.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

// `pad` ASCII bytes, then `unit` repeated until at least `min` bytes
pub fn long_title(pad: usize, unit: &str, min: usize) -> String {
    let mut t = "a".repeat(pad);
    while t.len() < min {
        t.push_str(unit);
    }
    t
}

// the stripper's text with the style markers ([MARKER, tag]) removed and the white space
// at both ends trimmed (the stripper's paragraph newlines)
pub fn plain(out: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        if out[i] == MARKER {
            i += 2;
        } else {
            v.push(out[i]);
            i += 1;
        }
    }
    let start = v
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(v.len());
    let end = v
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |p| p + 1);
    v[start..end].to_vec()
}

pub fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// documents
// ---------------------------------------------------------------------------

pub const DOC_HEAD: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>";
pub const DOC_TAIL: &str = "</p></body></html>";

// one paragraph around `inner` (raw bytes: any entity, any byte)
pub fn p_doc(inner: &[u8]) -> Vec<u8> {
    let mut v = DOC_HEAD.as_bytes().to_vec();
    v.extend_from_slice(inner);
    v.extend_from_slice(DOC_TAIL.as_bytes());
    v
}

// a ZIP whose only entry is `xhtml`
pub fn chapter_zip(xhtml: &[u8], deflate: bool) -> Vec<u8> {
    build_zip(&[RawEntry {
        name: "c.xhtml".to_string(),
        data: xhtml.to_vec(),
        deflate,
    }])
}

pub fn read_at(bytes: &[u8]) -> impl FnMut(u32, &mut [u8]) -> Result<usize, &'static str> + '_ {
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

pub fn open_zip(bytes: &[u8]) -> Result<ZipIndex, String> {
    let tail = bytes.len().saturating_sub(65557);
    let (cd_off, cd_size) = ZipIndex::parse_eocd(&bytes[tail..], bytes.len() as u32)
        .map_err(|e| format!("eocd: {e}"))?;
    let mut zip = ZipIndex::new();
    zip.parse_central_directory(&bytes[cd_off as usize..(cd_off + cd_size) as usize])
        .map_err(|e| format!("central directory: {e}"))?;
    Ok(zip)
}

pub fn entry_bytes(bytes: &[u8], zip: &ZipIndex, name: &str) -> Result<Vec<u8>, String> {
    let i = zip
        .find(name)
        .ok_or_else(|| format!("{name}: not in index"))?;
    let e = zip.entry(i);
    extract_entry(e, e.local_offset, read_at(bytes)).map_err(|e| format!("{name}: {e}"))
}

// ---------------------------------------------------------------------------
// chapter routes: each returns the stripped text (markers included) of entry `name`
// ---------------------------------------------------------------------------

pub fn route_stream(bytes: &[u8], zip: &ZipIndex, name: &str) -> Vec<u8> {
    let e = zip.entry(zip.find(name).expect("entry"));
    let mut out = Vec::new();
    let n = stream_strip_entry(e, e.local_offset, read_at(bytes), |c| {
        out.extend_from_slice(c);
        Ok(())
    })
    .expect("stream_strip_entry");
    assert_eq!(
        n as usize,
        out.len(),
        "stream_strip_entry's reported length"
    );
    out
}

// None when `extract_entry` itself fails (pinned apart in smol_stream.rs: the strip routes
// are judged on the entries it does return)
pub fn route_buf(bytes: &[u8], zip: &ZipIndex, name: &str) -> Option<Vec<u8>> {
    let xhtml = entry_bytes(bytes, zip, name).ok()?;
    Some(strip_html_buf(&xhtml).expect("strip_html_buf"))
}

struct Mem<'a>(&'a [u8]);

impl AsyncReadAt for Mem<'_> {
    async fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, &'static str> {
        let off = offset as usize;
        if off > self.0.len() {
            return Err("read past end");
        }
        let n = buf.len().min(self.0.len() - off);
        buf[..n].copy_from_slice(&self.0[off..off + n]);
        Ok(n)
    }
}

struct Sink(Vec<u8>);

impl AsyncWriteChunk for Sink {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), &'static str> {
        self.0.extend_from_slice(data);
        Ok(())
    }
}

pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..10_000_000 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
    panic!("future did not complete");
}

pub fn route_async(bytes: &[u8], zip: &ZipIndex, name: &str) -> Vec<u8> {
    let e = zip.entry(zip.find(name).expect("entry"));
    let mut sink = Sink(Vec::new());
    block_on(stream_strip_entry_async(
        e,
        e.local_offset,
        &mut Mem(bytes),
        &mut sink,
    ))
    .expect("stream_strip_entry_async");
    sink.0
}

// HtmlStripStream fed `chunk` input bytes at a time into an output slice of `out_cap` bytes
pub fn route_feed(input: &[u8], chunk: usize, out_cap: usize) -> Vec<u8> {
    let mut s = HtmlStripStream::new();
    let mut out = Vec::new();
    let mut buf = vec![0u8; out_cap];
    for piece in input.chunks(chunk) {
        let mut rest = piece;
        let mut idle = 0;
        while !rest.is_empty() {
            let (c, w) = s.feed(rest, &mut buf);
            out.extend_from_slice(&buf[..w]);
            rest = &rest[c..];
            idle = if c == 0 && w == 0 { idle + 1 } else { 0 };
            assert!(
                idle < 4,
                "feed made no progress (chunk {chunk}, out_cap {out_cap}, {} input bytes left)",
                rest.len()
            );
        }
    }
    loop {
        let w = s.finish(&mut buf);
        out.extend_from_slice(&buf[..w]);
        if w < buf.len() {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// raw EPUB
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ver {
    V2,
    V3,
}

// every text is raw bytes: nothing is escaped or validated
pub struct Book {
    pub ver: Ver,
    pub title: Vec<u8>,
    pub creator: Vec<u8>,
    // TOC labels, entry i points at chapter (i % chapters.len())
    pub toc_titles: Vec<Vec<u8>>,
    // chapter XHTML documents
    pub chapters: Vec<Vec<u8>>,
    pub deflate: bool,
}

impl Book {
    pub fn new(ver: Ver, chapters: Vec<Vec<u8>>) -> Self {
        Self {
            ver,
            title: b"Title".to_vec(),
            creator: b"Author".to_vec(),
            toc_titles: Vec::new(),
            chapters,
            deflate: false,
        }
    }
}

pub fn chapter_name(i: usize) -> String {
    format!("OEBPS/text/chapter{:02}.xhtml", i + 1)
}

fn chapter_href(i: usize) -> String {
    format!("text/chapter{:02}.xhtml", i + 1)
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub fn container_xml() -> Vec<u8> {
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n<rootfiles>\n<rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\n</rootfiles>\n</container>\n".to_vec()
}

pub fn opf_xml(b: &Book) -> Vec<u8> {
    let v3 = b.ver == Ver::V3;
    let mut x = cat(&[
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"",
        if v3 { b"3.0" } else { b"2.0" },
        b"\" unique-identifier=\"bookid\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:title>",
        &b.title,
        b"</dc:title>\n<dc:creator>",
        &b.creator,
        b"</dc:creator>\n<dc:language>zh</dc:language>\n<dc:identifier id=\"bookid\">urn:test:smol</dc:identifier>\n</metadata>\n<manifest>\n",
    ]);
    if v3 {
        x.extend_from_slice(b"<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    } else {
        x.extend_from_slice(
            b"<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
        );
    }
    for i in 0..b.chapters.len() {
        x.extend_from_slice(
            format!(
                "<item id=\"ch{}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>\n",
                i + 1,
                chapter_href(i)
            )
            .as_bytes(),
        );
    }
    x.extend_from_slice(b"</manifest>\n");
    x.extend_from_slice(if v3 {
        b"<spine>\n".as_slice()
    } else {
        b"<spine toc=\"ncx\">\n".as_slice()
    });
    for i in 0..b.chapters.len() {
        x.extend_from_slice(format!("<itemref idref=\"ch{}\"/>\n", i + 1).as_bytes());
    }
    x.extend_from_slice(b"</spine>\n</package>\n");
    x
}

pub fn ncx_xml(b: &Book) -> Vec<u8> {
    let mut x = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n<head><meta name=\"dtb:uid\" content=\"urn:test:smol\"/></head>\n<docTitle><text>T</text></docTitle>\n<navMap>\n".to_vec();
    for (i, t) in b.toc_titles.iter().enumerate() {
        x.extend_from_slice(
            format!(
                "<navPoint id=\"np{o}\" playOrder=\"{o}\"><navLabel><text>",
                o = i + 1
            )
            .as_bytes(),
        );
        x.extend_from_slice(t);
        x.extend_from_slice(
            format!(
                "</text></navLabel><content src=\"{}\"/></navPoint>\n",
                chapter_href(i % b.chapters.len())
            )
            .as_bytes(),
        );
    }
    x.extend_from_slice(b"</navMap>\n</ncx>\n");
    x
}

pub fn nav_xml(b: &Book) -> Vec<u8> {
    let mut x = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"{XHTML_NS}\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n<head><title>T</title></head>\n<body>\n<nav epub:type=\"toc\">\n<ol>\n"
    )
    .into_bytes();
    for (i, t) in b.toc_titles.iter().enumerate() {
        x.extend_from_slice(
            format!("<li><a href=\"{}\">", chapter_href(i % b.chapters.len())).as_bytes(),
        );
        x.extend_from_slice(t);
        x.extend_from_slice(b"</a></li>\n");
    }
    x.extend_from_slice(b"</ol>\n</nav>\n</body>\n</html>\n");
    x
}

// the entries of a book, in archive order: mimetype, container, OPF, TOC, chapters
pub fn book_entries(b: &Book) -> Vec<RawEntry> {
    let toc = match b.ver {
        Ver::V2 => ("OEBPS/toc.ncx", ncx_xml(b)),
        Ver::V3 => ("OEBPS/nav.xhtml", nav_xml(b)),
    };
    let mut v = vec![
        RawEntry {
            name: "mimetype".into(),
            data: b"application/epub+zip".to_vec(),
            deflate: false,
        },
        RawEntry {
            name: "META-INF/container.xml".into(),
            data: container_xml(),
            deflate: b.deflate,
        },
        RawEntry {
            name: "OEBPS/content.opf".into(),
            data: opf_xml(b),
            deflate: b.deflate,
        },
        RawEntry {
            name: toc.0.into(),
            data: toc.1,
            deflate: b.deflate,
        },
    ];
    for (i, c) in b.chapters.iter().enumerate() {
        v.push(RawEntry {
            name: chapter_name(i),
            data: c.clone(),
            deflate: b.deflate,
        });
    }
    v
}

pub fn build_book(b: &Book) -> Vec<u8> {
    build_zip(&book_entries(b))
}

// ---------------------------------------------------------------------------
// the production parse chain (the one ReaderApp runs), stage errors as text
// ---------------------------------------------------------------------------

pub struct Parsed {
    pub zip: ZipIndex,
    pub meta: EpubMeta,
    pub spine: EpubSpine,
    pub toc: EpubToc,
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

pub fn try_parse(bytes: &[u8]) -> Result<Parsed, String> {
    let zip = open_zip(bytes)?;
    let container = entry_bytes(bytes, &zip, "META-INF/container.xml")?;
    let mut path = [0u8; epub::OPF_PATH_CAP];
    let n = epub::parse_container(&container, &mut path).map_err(|e| format!("container: {e}"))?;
    let opf_path = String::from_utf8(path[..n].to_vec()).map_err(|e| e.to_string())?;
    let opf = entry_bytes(bytes, &zip, &opf_path)?;
    let mut meta = EpubMeta::new();
    let mut spine = EpubSpine::new();
    epub::parse_opf(&opf, dir_of(&opf_path), &zip, &mut meta, &mut spine)
        .map_err(|e| format!("opf: {e}"))?;
    let mut toc = EpubToc::new();
    if let Some(src) = epub::find_toc_source(&opf, dir_of(&opf_path), &zip) {
        let toc_name = zip.entry_name(src.zip_index()).to_string();
        let data = entry_bytes(bytes, &zip, &toc_name)?;
        epub::parse_toc(src, &data, dir_of(&toc_name), &spine, &zip, &mut toc);
    }
    Ok(Parsed {
        zip,
        meta,
        spine,
        toc,
    })
}

pub fn parse(bytes: &[u8]) -> Parsed {
    try_parse(bytes).unwrap_or_else(|e| panic!("the book parses: {e}"))
}

// the stored bytes of TOC entry i / the title / the author, not through `*_str()`
// (which may hide invalid bytes)
pub fn toc_title_bytes(p: &Parsed, i: usize) -> Vec<u8> {
    let e = &p.toc.entries[i];
    e.title[..e.title_len as usize].to_vec()
}

pub fn meta_title_bytes(p: &Parsed) -> Vec<u8> {
    p.meta.title[..p.meta.title_len as usize].to_vec()
}

pub fn meta_author_bytes(p: &Parsed) -> Vec<u8> {
    p.meta.author[..p.meta.author_len as usize].to_vec()
}

// the pads at which a text that starts `prefix` bytes into the document moves through every
// alignment of the internal buffers (4096-byte read / strip buffers, the 3968-byte flush
// point, the 32768-byte window) over `span` bytes of text
pub fn boundary_pads(prefix: usize, span: usize) -> Vec<usize> {
    let mut centres: BTreeSet<usize> = BTreeSet::new();
    for k in 1..=10 {
        centres.insert(3968 * k);
        centres.insert(4096 * k);
    }
    centres.insert(32768);
    centres.insert(32768 + 3968);
    let mut pads: BTreeSet<usize> = (0..=span + 16).collect();
    for c in centres {
        for pos in c.saturating_sub(span + 8)..=c + 8 {
            if let Some(p) = pos.checked_sub(prefix) {
                pads.insert(p);
            }
        }
    }
    pads.into_iter().collect()
}

// ---------------------------------------------------------------------------
// soft assertions: a test checks every case and fails once at the end with the count and
// the first few failures (a sweep over many inputs shows how far a defect reaches)
// ---------------------------------------------------------------------------

thread_local! {
    static FAILS: std::cell::RefCell<(usize, Vec<String>)> = const { std::cell::RefCell::new((0, Vec::new())) };
}

pub fn soft_fail(msg: String) {
    FAILS.with(|f| {
        let mut f = f.borrow_mut();
        f.0 += 1;
        if f.1.len() < 6 {
            f.1.push(msg);
        }
    });
}

// panics with the failures collected so far (none: returns)
pub fn finish() {
    let (n, first) = FAILS.with(|f| std::mem::take(&mut *f.borrow_mut()));
    assert!(
        n == 0,
        "{n} checks failed; first {}:\n  {}",
        first.len(),
        first.join("\n  ")
    );
}

// the stripper's text (markers and end white space aside) is `expected`, valid UTF-8
pub fn check_text(out: &[u8], expected: &str, label: &str) {
    let t = plain(out);
    match std::str::from_utf8(&t) {
        Err(e) => soft_fail(format!("{label}: not valid UTF-8 ({e}): {}", hex(&t))),
        Ok(s) if s != expected => soft_fail(format!("{label}: got {s:?}, want {expected:?}")),
        Ok(_) => {}
    }
}

// stored title bytes are `expected`, valid UTF-8
pub fn check_stored(stored: &[u8], expected: &str, label: &str) {
    match std::str::from_utf8(stored) {
        Err(e) => soft_fail(format!(
            "{label}: stored bytes not valid UTF-8 ({e}): {}",
            hex(stored)
        )),
        Ok(s) if s != expected => soft_fail(format!("{label}: got {s:?}, want {expected:?}")),
        Ok(_) => {}
    }
}

// ---------------------------------------------------------------------------
// more routes (gap tests)
// ---------------------------------------------------------------------------

// `strip_html_buf_async` on a whole document
pub fn route_buf_async(xhtml: &[u8]) -> Vec<u8> {
    block_on(smol_epub::async_io::strip_html_buf_async(xhtml)).expect("strip_html_buf_async")
}

// `route_feed` with its own output size for `finish` (the documented contract: call again
// until fewer bytes than the slice length come back)
pub fn route_feed_caps(input: &[u8], chunk: usize, out_cap: usize, finish_cap: usize) -> Vec<u8> {
    let mut s = HtmlStripStream::new();
    let mut out = Vec::new();
    let mut buf = vec![0u8; out_cap];
    for piece in input.chunks(chunk) {
        let mut rest = piece;
        let mut idle = 0;
        while !rest.is_empty() {
            let (c, w) = s.feed(rest, &mut buf);
            out.extend_from_slice(&buf[..w]);
            rest = &rest[c..];
            idle = if c == 0 && w == 0 { idle + 1 } else { 0 };
            assert!(
                idle < 4,
                "feed made no progress (chunk {chunk}, out_cap {out_cap})"
            );
        }
    }
    let mut fin = vec![0u8; finish_cap];
    for _ in 0..1000 {
        let w = s.finish(&mut fin);
        out.extend_from_slice(&fin[..w]);
        if w < fin.len() {
            return out;
        }
    }
    panic!("finish never came back short (finish_cap {finish_cap})");
}

// every route a whole chapter document goes through: (route name, stripped text)
pub fn all_routes(xhtml: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    for deflate in [false, true] {
        let bytes = chapter_zip(xhtml, deflate);
        let zip = open_zip(&bytes).unwrap();
        v.push((
            format!("stream, deflate {deflate}"),
            route_stream(&bytes, &zip, "c.xhtml"),
        ));
        if let Some(b) = route_buf(&bytes, &zip, "c.xhtml") {
            v.push((format!("buf, deflate {deflate}"), b));
        }
        v.push((
            format!("async stream, deflate {deflate}"),
            route_async(&bytes, &zip, "c.xhtml"),
        ));
    }
    v.push(("buf async".to_string(), route_buf_async(xhtml)));
    v.push((
        "feed whole".to_string(),
        route_feed(xhtml, xhtml.len(), 4096),
    ));
    v.push(("feed 1 byte".to_string(), route_feed(xhtml, 1, 4096)));
    v.push(("feed 3 bytes, out 64".to_string(), route_feed(xhtml, 3, 64)));
    v
}

// run `f` on its own thread; None when it has not finished in `secs` seconds (the thread is
// left behind: a loop that never ends must fail the test, not hang it)
pub fn with_timeout<T: Send + 'static>(
    secs: u64,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

pub fn extract_sync(bytes: &[u8], zip: &ZipIndex, name: &str) -> Result<Vec<u8>, &'static str> {
    let e = zip.entry(zip.find(name).expect("entry"));
    extract_entry(e, e.local_offset, read_at(bytes))
}

pub fn extract_async(bytes: &[u8], zip: &ZipIndex, name: &str) -> Result<Vec<u8>, &'static str> {
    let e = zip.entry(zip.find(name).expect("entry"));
    block_on(smol_epub::async_io::extract_entry_async(
        e,
        e.local_offset,
        &mut Mem(bytes),
    ))
}

// the archive with the uncompressed size its (single) central directory entry declares
// replaced: the entry then claims to be smaller than what it inflates to
pub fn declare_uncompressed_size(zip_bytes: &[u8], size: u32) -> Vec<u8> {
    let mut v = zip_bytes.to_vec();
    let n = v.len();
    // EOCD without a comment: offset of the central directory at 16..20 of its 22 bytes
    let cd = u32::from_le_bytes(v[n - 6..n - 2].try_into().unwrap()) as usize;
    assert_eq!(
        &v[cd..cd + 4],
        &0x0201_4b50u32.to_le_bytes(),
        "central directory entry"
    );
    v[cd + 24..cd + 28].copy_from_slice(&size.to_le_bytes());
    v
}

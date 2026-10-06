// EPUB regression -- EPUB navigation through the production ReaderApp:
// open, previous/next paging across chapters, TOC, chapter jumps, image pages
// on EPUB 2 / EPUB 3 fixtures.
//
// Run: scripts/host-test.sh --test reader_epub
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// Everything below only FORWARDS to the real ReaderApp (its pub fields /
// methods / events through crate::apps::probe, the same way paging regression / render regression did). No
// layout, paging, TOC, chapter or image logic may be written in pulp-host. The
// model is the archived scripts/reader-regression/os/src/rig.rs (`Rig`,
// probe.rs) with the in-memory FakeFs replaced by VirtualStorage. Existing
// `Rig` methods (new / storage / configure / open / press / phase / error_kind /
// page / total_pages / fully_indexed / page_offsets / lines / max_lines / text_w /
// text_margin / font_line_h / text_y / text_area_h / draw) are unchanged.
// `Rig::press` still returns (); Action::{Select, Back, Next, Prev, NextJump,
// PrevJump} are all used (they are the production board::action::Action).
//
// New public items in pulp_host::reader:
//
//   // production reader::QA_FONT_SIZE / QA_PREV_CHAPTER / QA_NEXT_CHAPTER /
//   // QA_TOC (values 1 / 3 / 4 / 5), re-exported (not copied literals)
//   pub const QA_FONT_SIZE: u8;
//   pub const QA_PREV_CHAPTER: u8;
//   pub const QA_NEXT_CHAPTER: u8;
//   pub const QA_TOC: u8;
//
//   // one laid-out line of the current page: the RAW bytes of the page buffer
//   // (style markers [smol_epub::html_strip::MARKER, tag] included) and the
//   // LineSpan flags of that line
//   #[derive(Debug, Clone, PartialEq, Eq)]
//   pub struct LineInfo {
//       pub bytes: Vec<u8>,
//       // LineSpan::is_image(): the line is part of an image's reserved area
//       pub image: bool,
//       // LineSpan::is_image_origin(): the line that carries the image (is_image && len > 0)
//       pub image_origin: bool,
//   }
//
// New methods on `Rig`:
//
//   pub fn chapter(&self) -> u16;
//        ReaderApp::chapter(): 0-based spine index of the chapter on screen. For
//        an EPUB `page()` / `total_pages()` / `page_offsets()` are PER CHAPTER
//        (as in the archived harness: total_pages() right after open is the
//        page count of chapter 0, and a chapter's page table is complete once
//        it is shown).
//   pub fn spine_len(&self) -> usize;                 // app.epub.spine.len()
//   pub fn epub_title(&self) -> String;               // app.epub.meta.title_str()
//   pub fn epub_author(&self) -> String;              // app.epub.meta.author_str()
//   pub fn toc_entries(&self) -> Vec<(String, u16)>;
//        (title_str(), spine_idx) of app.epub.toc.entries[..len()], in the
//        parser's order (flat, pre-order); empty when there is no TOC. Valid
//        any time after `open` (the TOC is loaded during open), not only in
//        Phase::Toc.
//   pub fn toc_selected(&self) -> usize;              // app.epub.toc_selected
//   pub fn line_infos(&self) -> Vec<LineInfo>;
//        the current page, one entry per laid-out line; `bytes` of entry i is
//        identical to `lines()[i]` (the tests assert that).
//   pub fn page_image(&self) -> Option<(u16, u16)>;
//        (width, height) of app.page_img, the decoded 1-bit image the current
//        page draws; None when the page has no decoded image.
//   pub fn quick_action_ids(&self) -> Vec<u8>;
//        ids of App::quick_actions(), in order.
//   pub fn quick_trigger(&mut self, id: u8);
//        App::on_quick_trigger(id, ctx), then settle (exactly like `press`).
//
//   pub fn exit(&mut self);
//        App::on_exit() (as the archived rig's `exit`); no settle. `exit()` then
//        `open(name)` is how the firmware re-enters a book: the second open finds
//        the chapter / image caches the first one wrote under _PULP/.
//   pub fn idle(&mut self, ticks: usize);
//        scheduler idle time, as in the archived rig: `ticks` ticks of the
//        reader's `background` (and of the image worker, see below), no settle,
//        no state check. Bounded by `ticks`.
//   pub fn has_bg_work(&self) -> bool;
//        ReaderApp::has_bg_work(): the reader still has chapters or images of
//        the open book to cache in the background.
//
// Settle semantics (extends the paging regression contract; applies to `open`, `press` and
// `quick_trigger`): on return phase() != Phase::Loading (Ready, Toc or Error)
// and whatever the production state machine still does to finish ENTERING the
// page has run, including a deferred image decode (ReaderApp.defer_image_decode,
// ticks of `background`). Production decodes a page's
// image when the page is entered and the image is already in the card cache, so
// the tests only look at `page_image()` of pages entered after
// `finish_background()` + `exit()` + `open()` (the "warm" book below).
//
// Image worker: production offloads image decoding to kernel::work_queue's
// `worker_task` (an embassy task: `worker_task()` returns a SpawnToken) and
// the firmware registers the decoder with `work_queue::register_image_decoder`
// at boot (src/bin/main.rs). The rig must do the same: register the decoder
// and drive `worker_task` between `background` ticks. Hint, verified on a
// throwaway stub: leak a Box<embassy_executor::raw::Executor> (provide the
// `#[unsafe(export_name = "__pender")]` fn), spawn `worker_task()` on it once
// per Rig, and call `unsafe { executor.poll() }` after every `background` tick.
// Without it the image caching never completes.
//
// What the tests treat as the observable result:
//   * The text of a page is its non-image lines, style markers ([MARKER, tag])
//     removed, UTF-8 decoded, ALL whitespace removed (a wrapper may drop the
//     space at a line end; hyphen/dash break points are not specified). The
//     expected text is derived from the fixture spec (host/src/fixtures.rs:
//     chapter title as <h1>, then headings / paragraph runs in block order,
//     images contribute no text; the XHTML <head><title> is not page text).
//   * Fixture TOC titles / entries: the spec's nested TOC flattened pre-order
//     (smol-epub docs: "Nested navPoint elements are flattened into a linear
//     list"; fixtures.rs: "the production parsers flatten it pre-order").
//   * Images are observed through ReaderApp::draw, rendered by render regression's
//     render_stitched / render_full, by DIFFERENTIAL rendering: the same book
//     is built with the same image in Pattern::Black and Pattern::White (same
//     kind and size => same layout) and the pixels that differ are, by
//     construction, the image area. Geometry bounds come from Rig::text_*,
//     font_line_h and the image-flagged lines.
//
// Expected values never come from running the code under test (see the
// derivations next to each test).
// ============================================================================

use std::sync::OnceLock;

use pulp_host::fixtures::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, Fixture, ImageKind, ImageSpec, Pattern,
    Run, Spec, TocItem, build_epub, standard,
};
use pulp_host::reader::{
    Action, LineInfo, Phase, QA_FONT_SIZE, QA_NEXT_CHAPTER, QA_PREV_CHAPTER, QA_TOC, Rig,
};
use pulp_host::render::{Framebuffer, HEIGHT, WIDTH, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;
use smol_epub::html_strip::{HEADING_ON, MARKER};

// (book font index, reading theme index) used unless a test says otherwise
const FONT: u8 = 2;
const THEME: u8 = 1;
// corners and interior points of the 5 x 4 (font x theme) grid
const CONFIGS: [(u8, u8); 5] = [(0, 0), (2, 1), (4, 3), (1, 2), (3, 0)];
// a book walk never needs more steps than this; more means a runaway
const MAX_STEPS: usize = 3000;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

// The image worker of the production reader (kernel::work_queue) is one task
// behind process-global channels: tests that run a reader take this lock for
// their whole duration, and every test keeps at most one Rig alive at a time.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn fixtures() -> &'static [Fixture] {
    static F: OnceLock<Vec<Fixture>> = OnceLock::new();
    F.get_or_init(standard)
}

struct Book {
    name: &'static str,
    spec: EpubSpec,
    bytes: Vec<u8>,
}

// the six standard EPUBs: EPUB 2 / EPUB 3 x stored / deflate / mixed
fn books() -> Vec<Book> {
    let v: Vec<Book> = fixtures()
        .iter()
        .filter_map(|f| match &f.spec {
            Spec::Epub(spec) => Some(Book {
                name: f.name,
                spec: spec.clone(),
                bytes: f.bytes.clone(),
            }),
            Spec::Txt(_) => None,
        })
        .collect();
    assert_eq!(v.len(), 6, "the standard set has six EPUBs");
    v
}

// the spec without its images (and cover): same chapters' text, headings, TOC,
// version and compression. Used where a test is about navigation and text, not
// about image layout (see the `image_chapters_` tests for that).
fn text_only(spec: &EpubSpec) -> EpubSpec {
    let mut t = spec.clone();
    for ch in &mut t.chapters {
        ch.blocks.retain(|b| !matches!(b, Block::Image(_)));
    }
    t.images.clear();
    t.cover = None;
    t
}

// the six standard EPUBs without their images (same names)
fn text_books() -> Vec<Book> {
    books()
        .into_iter()
        .map(|b| {
            let spec = text_only(&b.spec);
            let bytes = build_epub(&spec).expect("text-only spec is valid");
            Book {
                name: b.name,
                spec,
                bytes,
            }
        })
        .collect()
}

fn book(name: &str) -> Book {
    books()
        .into_iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("standard fixture {name}"))
}

fn text_book(name: &str) -> Book {
    text_books()
        .into_iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("standard fixture {name}"))
}

// the first open of a book on a fresh card: nothing cached yet
fn rig_cold(name: &str, bytes: &[u8], font: u8, theme: u8) -> Rig {
    let card = VirtualStorage::memory_with(&[(name, bytes)]);
    card.ensure_pulp_dir()
        .expect("card has _PULP/ the way the firmware boots it");
    let mut r = Rig::new(card);
    r.configure(font, theme);
    r.open(name);
    assert_eq!(
        r.phase(),
        Phase::Ready,
        "{name} (font {font}, theme {theme}) must open to a page"
    );
    r
}

// idle ticks allowed for caching a whole book; an intact book needs a small
// fraction of this (the tests assert it gets there)
const WARM_TICKS: usize = 400;

// A "warm" book: opened, left to cache the book in idle time, closed and opened
// again, so the chapter and image caches are in place when the reader enters
// its first page (what a reader sees from the second visit on). With
// `must_finish` the idle time has to be enough to cache everything.
fn rig_warm(name: &str, bytes: &[u8], font: u8, theme: u8, must_finish: bool) -> Rig {
    let mut r = rig_cold(name, bytes, font, theme);
    for _ in 0..WARM_TICKS {
        if !r.has_bg_work() {
            break;
        }
        r.idle(1);
    }
    if must_finish {
        assert!(
            !r.has_bg_work(),
            "{name}: the background caching of an intact book finishes"
        );
    }
    r.exit();
    r.open(name);
    assert_eq!(
        r.phase(),
        Phase::Ready,
        "{name} (font {font}, theme {theme}) must reopen to a page"
    );
    assert_eq!(
        (r.chapter(), r.page()),
        (0, 0),
        "{name}: reopens on the first page"
    );
    r
}

fn rig(name: &str, bytes: &[u8], font: u8, theme: u8) -> Rig {
    rig_warm(name, bytes, font, theme, true)
}

fn open(b: &Book) -> Rig {
    rig(b.name, &b.bytes, FONT, THEME)
}

// ---------------------------------------------------------------------------
// what a page / chapter / book looks like, derived from the spec
// ---------------------------------------------------------------------------

fn ws_free(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// the page's text: non-image lines, markers removed, whitespace removed
fn page_text(lines: &[LineInfo]) -> String {
    let mut bytes = Vec::new();
    for l in lines.iter().filter(|l| !l.image) {
        let b = &l.bytes;
        let mut i = 0;
        while i < b.len() {
            if b[i] == MARKER {
                i += 2;
                continue;
            }
            bytes.push(b[i]);
            i += 1;
        }
    }
    ws_free(&String::from_utf8(bytes).expect("page text is well-formed UTF-8"))
}

fn run_text(r: &Run) -> &str {
    match r {
        Run::Text(t) | Run::Bold(t) | Run::Italic(t) => t,
        Run::Break => "\n",
    }
}

// the chapter title (the XHTML <h1>) then every block's text, in order
fn chapter_text(ch: &Chapter) -> String {
    let mut s = ch.title.clone();
    s.push('\n');
    for b in &ch.blocks {
        match b {
            Block::Paragraph(runs) => runs.iter().for_each(|r| s.push_str(run_text(r))),
            Block::Heading(t) => s.push_str(t),
            Block::Image(_) => {}
        }
        s.push('\n');
    }
    ws_free(&s)
}

// the images of the book in reading order
fn images_in_order(spec: &EpubSpec) -> Vec<&ImageSpec> {
    spec.chapters
        .iter()
        .flat_map(|c| &c.blocks)
        .filter_map(|b| match b {
            Block::Image(i) => Some(&spec.images[*i]),
            _ => None,
        })
        .collect()
}

fn image_blocks(ch: &Chapter) -> usize {
    ch.blocks
        .iter()
        .filter(|b| matches!(b, Block::Image(_)))
        .count()
}

fn flatten_toc(items: &[TocItem], out: &mut Vec<(String, u16)>) {
    for it in items {
        out.push((it.title.clone(), it.chapter as u16));
        flatten_toc(&it.children, out);
    }
}

fn first_diff(got: &str, want: &str) -> String {
    let (g, w): (Vec<char>, Vec<char>) = (got.chars().collect(), want.chars().collect());
    let at = g
        .iter()
        .zip(&w)
        .position(|(a, b)| a != b)
        .unwrap_or(g.len().min(w.len()));
    let near = |v: &[char]| {
        v[at.saturating_sub(12)..(at + 12).min(v.len())]
            .iter()
            .collect::<String>()
    };
    format!(
        "lengths {} vs {}, first difference at char {at}: got ...{}... want ...{}...",
        g.len(),
        w.len(),
        near(&g),
        near(&w)
    )
}

// ---------------------------------------------------------------------------
// positions and walks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pos {
    chapter: u16,
    page: usize,
    // pages of this chapter
    total: usize,
    lines: Vec<LineInfo>,
    text: String,
    // image-origin lines on the page
    images: usize,
    image: Option<(u16, u16)>,
}

fn pos(r: &Rig) -> Pos {
    let lines = r.line_infos();
    let raw: Vec<Vec<u8>> = lines.iter().map(|l| l.bytes.clone()).collect();
    assert_eq!(
        raw,
        r.lines(),
        "line_infos() must carry the same bytes as lines()"
    );
    Pos {
        chapter: r.chapter(),
        page: r.page(),
        total: r.total_pages(),
        text: page_text(&lines),
        images: lines.iter().filter(|l| l.image_origin).count(),
        image: r.page_image(),
        lines,
    }
}

fn at(p: &Pos) -> (u16, usize) {
    (p.chapter, p.page)
}

fn assert_same_pos(ctx: &str, got: &Pos, want: &Pos) {
    assert_eq!(at(got), at(want), "{ctx}: position");
    assert_eq!(
        got.total,
        want.total,
        "{ctx}: pages in the chapter at {:?}",
        at(want)
    );
    assert_eq!(got.lines, want.lines, "{ctx}: lines of {:?}", at(want));
    assert_eq!(
        got.image,
        want.image,
        "{ctx}: decoded image of {:?}",
        at(want)
    );
}

fn assert_same_walk(ctx: &str, got: &[Pos], want: &[Pos]) {
    assert_eq!(got.len(), want.len(), "{ctx}: number of pages");
    for (g, w) in got.iter().zip(want) {
        assert_same_pos(ctx, g, w);
    }
}

// Press(Next) from the current page until the position stops changing. Every
// step is exactly one page of the same chapter, or the first page of the next
// chapter taken from a chapter's last page. Returns every page shown, the
// starting page first.
fn walk_forward(r: &mut Rig) -> Vec<Pos> {
    walk_forward_visit(r, &mut |_, _| {})
}

// `walk_forward`, calling `visit` on every page shown (the starting page too)
fn walk_forward_visit(r: &mut Rig, visit: &mut dyn FnMut(&Rig, &Pos)) -> Vec<Pos> {
    let mut out = vec![pos(r)];
    visit(r, &out[0]);
    for _ in 0..MAX_STEPS {
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready, "Next must leave a page up");
        let p = pos(r);
        let last = out.last().unwrap();
        if at(&p) == at(last) {
            return out;
        }
        let within = p.chapter == last.chapter && p.page == last.page + 1 && p.page < p.total;
        let across = p.chapter == last.chapter + 1 && p.page == 0 && last.page + 1 == last.total;
        assert!(
            within || across,
            "Next from {:?} (of {} pages) landed on {:?}: neither the next page nor the next chapter's first page",
            at(last),
            last.total,
            at(&p)
        );
        visit(r, &p);
        out.push(p);
    }
    panic!("walk_forward did not reach the end of the book");
}

// Press(Prev) until the position stops changing: one page, or the previous
// chapter's LAST page taken from a chapter's first page.
fn walk_back(r: &mut Rig) -> Vec<Pos> {
    let mut out = vec![pos(r)];
    for _ in 0..MAX_STEPS {
        r.press(Action::Prev);
        assert_eq!(r.phase(), Phase::Ready, "Prev must leave a page up");
        let p = pos(r);
        let last = out.last().unwrap();
        if at(&p) == at(last) {
            return out;
        }
        let within = p.chapter == last.chapter && p.page + 1 == last.page;
        let across = p.chapter + 1 == last.chapter && last.page == 0 && p.page + 1 == p.total;
        assert!(
            within || across,
            "Prev from {:?} landed on {:?}: neither the previous page nor the previous chapter's last page",
            at(last),
            at(&p)
        );
        out.push(p);
    }
    panic!("walk_back did not reach the start of the book");
}

fn first_page(walk: &[Pos], chapter: usize) -> &Pos {
    walk.iter()
        .find(|p| p.chapter as usize == chapter && p.page == 0)
        .unwrap_or_else(|| panic!("chapter {chapter} was never shown"))
}

fn last_page(walk: &[Pos], chapter: usize) -> &Pos {
    walk.iter()
        .rfind(|p| p.chapter as usize == chapter)
        .unwrap_or_else(|| panic!("chapter {chapter} was never shown"))
}

// The pages of `walk` tile the spec: every chapter shows pages 0..n in order
// with a stable page count, the chapters follow each other in spine order, the
// text of the pages of chapter c concatenated IS the chapter's text (nothing
// lost, nothing repeated, nothing from another chapter), and each chapter
// starts with its <h1> title line.
fn check_book_text(ctx: &str, spec: &EpubSpec, walk: &[Pos]) {
    let chapters = spec.chapters.len();
    assert_eq!(
        walk.iter().map(|p| p.chapter as usize).max(),
        Some(chapters - 1),
        "{ctx}: the walk reaches the last chapter"
    );
    let seen: usize = (0..chapters)
        .map(|c| check_chapter_text(ctx, spec, walk, c))
        .sum();
    assert_eq!(
        seen,
        walk.len(),
        "{ctx}: every page belongs to a spec chapter, in order"
    );
}

// the part of `check_book_text` that concerns chapter `c`; returns its page count
fn check_chapter_text(ctx: &str, spec: &EpubSpec, walk: &[Pos], c: usize) -> usize {
    let pages: Vec<&Pos> = walk.iter().filter(|p| p.chapter as usize == c).collect();
    assert!(!pages.is_empty(), "{ctx}: chapter {c} is never shown");
    for (i, p) in pages.iter().enumerate() {
        assert_eq!(
            p.page, i,
            "{ctx}: chapter {c}: pages are shown in order, once each"
        );
        assert_eq!(
            p.total,
            pages.len(),
            "{ctx}: chapter {c}: page count seen at page {i}"
        );
    }
    let got: String = pages.iter().map(|p| p.text.as_str()).collect();
    let want = chapter_text(&spec.chapters[c]);
    assert!(
        got == want,
        "{ctx}: chapter {c} text: {}",
        first_diff(&got, &want)
    );
    let head = &pages[0].lines[0].bytes;
    assert!(
        head.starts_with(&[MARKER, HEADING_ON]),
        "{ctx}: chapter {c} starts with its <h1> title line, got {head:?}"
    );
    pages.len()
}

// ---------------------------------------------------------------------------
// English reader regression open an EPUB
// ---------------------------------------------------------------------------

// Expected: every standard EPUB opens to Phase::Ready on chapter 0 page 0; the
// book has more than one page (chapter 0 alone: 48 paragraphs); title / author
// / chapter count are the spec's; page 0 shows the BEGINNING of spec chapter 0:
// its text is a prefix of the chapter text (title "The Long Road", then the
// heading "Setting Out") and the first line is the <h1> title.
#[test]
fn every_epub_opens_to_the_first_page_with_the_spec_metadata() {
    let _serial = serial();
    for b in books() {
        let r = open(&b);
        let ctx = b.name;
        assert_eq!(
            (r.chapter(), r.page()),
            (0, 0),
            "{ctx}: opens on chapter 0, page 0"
        );
        assert!(r.total_pages() > 1, "{ctx}: chapter 0 spans several pages");
        assert_eq!(r.spine_len(), b.spec.chapters.len(), "{ctx}: spine length");
        assert_eq!(r.epub_title(), b.spec.title, "{ctx}: title");
        assert_eq!(r.epub_author(), b.spec.author, "{ctx}: author");

        let p = pos(&r);
        let ch0 = chapter_text(&b.spec.chapters[0]);
        assert!(
            ch0.starts_with(&p.text),
            "{ctx}: page 0 is the start of chapter 0: {}",
            first_diff(&p.text, &ch0)
        );
        let head = ws_free(&format!("{}Setting Out", b.spec.chapters[0].title));
        assert!(
            p.text.starts_with(&head),
            "{ctx}: page 0 starts with the chapter title and its first heading"
        );
        assert!(
            p.text.len() > head.len(),
            "{ctx}: page 0 has body text after the headings"
        );
        assert!(
            p.text.len() < ch0.len(),
            "{ctx}: page 0 is not the whole chapter"
        );
        assert!(
            p.lines[0].bytes.starts_with(&[MARKER, HEADING_ON]),
            "{ctx}: first line is the <h1> title"
        );
    }
}

// ---------------------------------------------------------------------------
// English reader regression previous / next paging across chapters
// ---------------------------------------------------------------------------

// Expected: walking Next from the first page to the end shows chapter c's
// pages 0..n_c in order (each Next is one page, or the next chapter's first
// page from a chapter's last page), more than one page in all, and the pages'
// text concatenated per chapter equals the spec chapter text exactly. A missing
// or duplicated page, a lost chapter boundary or text from the wrong chapter
// shows up as a text difference or a broken step in `walk_forward`. Run on the
// six books without images, both on the first open (nothing cached) and once
// everything is cached (images: see the `image_chapters_` tests).
#[test]
fn forward_walk_shows_every_chapter_page_by_page_without_loss_or_repeat() {
    let _serial = serial();
    for b in text_books() {
        let walk = walk_forward(&mut open(&b));
        assert!(
            walk.len() > b.spec.chapters.len(),
            "{}: more pages than chapters",
            b.name
        );
        check_book_text(b.name, &b.spec, &walk);
        let cold = walk_forward(&mut rig_cold(b.name, &b.bytes, FONT, THEME));
        check_book_text(&format!("{} (first open)", b.name), &b.spec, &cold);
    }
}

// ---- chapters with images (English reader regression "previous/next paging" + "images") -------------
//
// The tests named `image_chapters_*` run the same checks on the standard books
// WITH their images (chapter 0 has the cover inline, chapter 1 two figures,
// chapter 2 the photo). The requirement is the same as above: the page turns
// show every line of text once and the book reads the same however it is
// reached or stored. They are kept apart because their outcome depends on how
// the reader lays image pages out; see the EPUB regression report.

// Expected: first visit (nothing cached): the pages show the spec's text, once.
#[test]
fn image_chapters_keep_their_text_on_the_first_visit() {
    let _serial = serial();
    for b in books() {
        let mut r = rig_cold(b.name, &b.bytes, FONT, THEME);
        let walk = walk_forward(&mut r);
        check_book_text(&format!("{} (first open)", b.name), &b.spec, &walk);
    }
}

// Expected: same once the book and its images are cached.
#[test]
fn image_chapters_keep_their_text_once_cached() {
    let _serial = serial();
    for b in books() {
        let walk = walk_forward(&mut open(&b));
        check_book_text(b.name, &b.spec, &walk);
    }
}

// Expected: a chapter has the same pages whether the reader paged into it (Next
// from the previous chapter's last page) or jumped to it (NextJump): same page
// count, same lines.
#[test]
fn image_chapters_have_the_same_pages_however_they_are_reached() {
    let _serial = serial();
    for b in books() {
        let paged = walk_forward(&mut open(&b));
        let mut r = open(&b);
        for c in 1..b.spec.chapters.len() {
            r.press(Action::NextJump);
            assert_same_pos(
                &format!("{}: NextJump into chapter {c} vs paging", b.name),
                &pos(&r),
                first_page(&paged, c),
            );
        }
    }
}

// Expected (English reader regression "Mixed / STORED / DEFLATE consistency"): the six books give the
// same pages, image pages included.
#[test]
fn image_chapters_lay_out_the_same_for_every_version_and_compression() {
    let _serial = serial();
    let base = book("E2STORED.EPU");
    let want = walk_forward(&mut open(&base));
    for b in books() {
        let got = walk_forward(&mut open(&b));
        assert_same_walk(&format!("{} vs {}", b.name, base.name), &got, &want);
    }
}

// Same invariants at five font x theme geometries (different page breaks, one
// per config), on one book of each compression.
#[test]
fn chapter_text_is_conserved_at_every_font_and_theme_geometry() {
    let _serial = serial();
    for name in ["E3MIXED.EPU", "E2DEFL.EPU"] {
        let b = text_book(name);
        for (font, theme) in CONFIGS {
            let mut r = rig(b.name, &b.bytes, font, theme);
            let walk = walk_forward(&mut r);
            check_book_text(&format!("{name} font {font} theme {theme}"), &b.spec, &walk);
        }
    }
}

// Expected (English reader regression previous/next; archived navigation.rs
// `txt_forward_then_back_visits_the_same_pages` and
// `epub_next_crosses_into_the_next_chapter_and_prev_comes_back_to_its_last_page`
// and `epub_book_boundaries_are_no_ops`): Prev from the end visits exactly the
// pages Next visited, in reverse, down to chapter 0 page 0, with the same text
// on every page; Prev at a chapter's first page lands on the previous
// chapter's LAST page; Prev on the first page of the book and Next / NextJump on
// the last page change nothing and leave a page up.
#[test]
fn backward_walk_mirrors_the_forward_walk_and_the_book_ends_are_no_ops() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let mut r = open(&b);
        let first = pos(&r);

        r.press(Action::Prev);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}: Prev on the first page");
        assert_same_pos(&format!("{ctx}: Prev on the first page"), &pos(&r), &first);
        r.press(Action::PrevJump);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}: PrevJump on the first page");
        assert_same_pos(
            &format!("{ctx}: PrevJump on the first page"),
            &pos(&r),
            &first,
        );

        let forward = walk_forward(&mut r);
        let last = pos(&r);
        let want_last = b.spec.chapters.len() as u16 - 1;
        assert_eq!(
            last.chapter, want_last,
            "{ctx}: the walk ends in the last chapter"
        );
        assert_eq!(
            last.page + 1,
            last.total,
            "{ctx}: the walk ends on that chapter's last page"
        );

        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}: Next on the last page");
        assert_same_pos(&format!("{ctx}: Next on the last page"), &pos(&r), &last);
        r.press(Action::NextJump);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}: NextJump on the last page");
        assert_same_pos(
            &format!("{ctx}: NextJump on the last page"),
            &pos(&r),
            &last,
        );

        let mut back = walk_back(&mut r);
        back.reverse();
        assert_same_walk(&format!("{ctx}: backward vs forward"), &back, &forward);
        assert_same_pos(&format!("{ctx}: back at the start"), &pos(&r), &first);
    }
}

// Expected (English reader regression + "Mixed / STORED / DEFLATE consistency"): the six books are one
// text (the chapters are the same for every version and compression; only
// the container differs: EPUB 2 NCX vs EPUB 3 nav, stored vs deflated vs mixed
// entries, literal UTF-8 vs numeric character references), so every page of
// every book -- lines, page counts -- equals the same page of E2STORED. (The
// books without images here; with images: `image_chapters_lay_out_the_same_...`.)
#[test]
fn every_version_and_compression_gives_the_same_pages() {
    let _serial = serial();
    let base = text_book("E2STORED.EPU");
    let want = walk_forward(&mut open(&base));
    for b in text_books() {
        let got = walk_forward(&mut open(&b));
        assert_same_walk(&format!("{} vs {}", b.name, base.name), &got, &want);
    }
}

// ---------------------------------------------------------------------------
// English reader regression TOC
// ---------------------------------------------------------------------------

fn open_toc(r: &mut Rig) {
    r.quick_trigger(QA_TOC);
    assert_eq!(r.phase(), Phase::Toc, "the TOC quick action opens the TOC");
}

// open the TOC, move the selection to entry `k` with Next, pick it with Select
fn select_toc_entry(r: &mut Rig, k: usize) {
    open_toc(r);
    let n = r.toc_entries().len();
    let from = r.toc_selected();
    for _ in 0..(k + n - from) % n {
        r.press(Action::Next);
    }
    assert_eq!(r.toc_selected(), k, "the selection is on entry {k}");
    r.press(Action::Select);
    assert_eq!(r.phase(), Phase::Ready, "Select closes the TOC onto a page");
}

// Expected: the loaded TOC is the spec TOC flattened pre-order (nested as
// Part One > {Salt and Pepper, Pictures and Figures}, Part Two: Cafe Days >
// {Last Words}): five entries, titles exactly the spec's (including the
// 2-byte e-acute), each pointing at its spec chapter. EPUB 2 reads it from the
// NCX, EPUB 3 from the nav document.
#[test]
fn the_toc_is_the_spec_toc_flattened_pre_order() {
    let _serial = serial();
    for b in books() {
        let r = open(&b);
        let mut want = Vec::new();
        flatten_toc(&b.spec.toc, &mut want);
        assert_eq!(want.len(), 5, "fixture has five TOC targets");
        assert_eq!(
            r.toc_entries(),
            want,
            "{}: TOC entries (title, spine index)",
            b.name
        );
    }
}

// Expected (archived navigation.rs `epub_toc_overlay_select_jumps_to_the_chapter`):
// the overlay opens on the entry of the current chapter (entry i is chapter i
// in these books), Next / Prev move the selection and wrap at both ends, Back
// closes it without leaving the page: same chapter, same page, same lines.
#[test]
fn toc_overlay_opens_on_the_current_chapter_wraps_and_back_returns_to_the_page() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let mut r = open(&b);
        r.press(Action::NextJump); // chapter 1
        assert_eq!(r.chapter(), 1, "{ctx}");
        let before = pos(&r);

        open_toc(&mut r);
        assert_eq!(
            r.toc_selected(),
            1,
            "{ctx}: opens on the current chapter's entry"
        );
        for want in [2, 3, 4, 0] {
            r.press(Action::Next);
            assert_eq!(
                r.toc_selected(),
                want,
                "{ctx}: Next moves down and wraps after the last entry"
            );
        }
        for want in [4, 3] {
            r.press(Action::Prev);
            assert_eq!(
                r.toc_selected(),
                want,
                "{ctx}: Prev moves up and wraps before the first entry"
            );
        }
        assert_eq!(
            r.phase(),
            Phase::Toc,
            "{ctx}: moving the selection keeps the TOC open"
        );

        r.press(Action::Back);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}: Back closes the overlay");
        assert_same_pos(&format!("{ctx}: after Back"), &pos(&r), &before);
    }
}

// Expected (English reader regression "TOC"): selecting entry k lands on the FIRST page of chapter k,
// showing exactly what Next-paging shows there (same lines as the plain walk),
// whose text begins with spec chapter k. Starting chapters differ from the
// target each time (sequence 2, 0, 3, 1, 4 from chapter 0), so a no-op or a
// wrong target is visible.
#[test]
fn selecting_a_toc_entry_lands_on_the_first_page_of_its_chapter() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let reference = walk_forward(&mut open(&b));
        let mut r = open(&b);
        for k in [2usize, 0, 3, 1, 4] {
            let cur = r.chapter() as usize;
            assert_ne!(cur, k, "{ctx}: the test moves to another chapter");
            select_toc_entry(&mut r, k);
            let p = pos(&r);
            assert_same_pos(
                &format!("{ctx}: TOC entry {k}"),
                &p,
                first_page(&reference, k),
            );
            assert!(
                chapter_text(&b.spec.chapters[k]).starts_with(&p.text),
                "{ctx}: TOC entry {k}: page text starts spec chapter {k}"
            );
        }
    }
}

// Expected (archived navigation.rs: "NextJump also selects (X4 right-hand key)"):
// in the overlay NextJump picks the highlighted entry like Select does.
#[test]
fn next_jump_in_the_toc_also_selects_the_entry() {
    let _serial = serial();
    let b = text_book("E3STORED.EPU");
    let reference = walk_forward(&mut open(&b));
    let mut r = open(&b);
    open_toc(&mut r);
    r.press(Action::Next);
    r.press(Action::Next);
    r.press(Action::Next);
    assert_eq!(r.toc_selected(), 3);
    r.press(Action::NextJump);
    assert_eq!(r.phase(), Phase::Ready);
    assert_same_pos(
        "NextJump picks entry 3",
        &pos(&r),
        first_page(&reference, 3),
    );
}

// Expected: after a TOC jump the book continues seamlessly. Prev goes to the
// previous chapter's last page and Next comes back to the TOC target's first
// page (both identical to the plain walk); then paging forward to the end
// shows exactly the rest of the book, and paging back from there reaches the
// start of the book again.
#[test]
fn paging_after_a_toc_jump_continues_the_book() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let reference = walk_forward(&mut open(&b));
        let idx = |c: usize| {
            reference
                .iter()
                .position(|p| at(p) == (c as u16, 0))
                .unwrap()
        };

        let mut r = open(&b);
        select_toc_entry(&mut r, 3);
        r.press(Action::Prev);
        assert_same_pos(
            &format!("{ctx}: Prev after the jump"),
            &pos(&r),
            last_page(&reference, 2),
        );
        r.press(Action::Next);
        assert_same_pos(
            &format!("{ctx}: Next back to the target"),
            &pos(&r),
            first_page(&reference, 3),
        );

        let rest = walk_forward(&mut r);
        assert_same_walk(
            &format!("{ctx}: forward from entry 3"),
            &rest,
            &reference[idx(3)..],
        );
        let mut back = walk_back(&mut r);
        back.reverse();
        assert_same_walk(&format!("{ctx}: back to the start"), &back, &reference);
    }
}

// Expected: the TOC screen is drawn (ink on the page, not the page's own
// pixels), and its stitched-strip and full-frame renderings are pixel-equal
// (strip equality applied to the TOC screen).
#[test]
fn the_toc_screen_renders_and_stitched_equals_full() {
    let _serial = serial();
    for b in books() {
        let mut r = open(&b);
        let page = render_stitched(&|s| r.draw(s)).frame;
        open_toc(&mut r);
        let stitched = render_stitched(&|s| r.draw(s)).frame;
        let full = render_full(&|s| r.draw(s)).frame;
        assert!(
            stitched.black_count() > 0,
            "{}: the TOC screen has ink",
            b.name
        );
        assert_eq!(
            stitched.to_pbm(),
            full.to_pbm(),
            "{}: stitched == full",
            b.name
        );
        assert_ne!(
            stitched.to_pbm(),
            page.to_pbm(),
            "{}: the TOC screen is not the page",
            b.name
        );
    }
}

// ---------------------------------------------------------------------------
// English reader regression chapter jumps
// ---------------------------------------------------------------------------

// Expected (archived navigation.rs `epub_jump_goes_to_next_and_previous_chapter_first_page`):
// NextJump from the middle of a chapter lands on the next chapter's first page
// (same lines as paging there, text starts spec chapter c); at the last chapter
// it keeps the reader on that chapter with a page up.
#[test]
fn next_jump_goes_to_the_first_page_of_the_next_chapter_and_stops_at_the_last() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let n = b.spec.chapters.len();
        let reference = walk_forward(&mut open(&b));
        let mut r = open(&b);
        r.press(Action::Next);
        r.press(Action::Next);
        assert_eq!(
            at(&pos(&r)),
            (0, 2),
            "{ctx}: chapter 0 has at least three pages"
        );
        for c in 1..n {
            if c > 1 {
                r.press(Action::Next);
                assert_eq!(
                    at(&pos(&r)),
                    (c as u16 - 1, 1),
                    "{ctx}: chapter {} has at least two pages",
                    c - 1
                );
            }
            r.press(Action::NextJump);
            let p = pos(&r);
            assert_same_pos(
                &format!("{ctx}: NextJump into chapter {c}"),
                &p,
                first_page(&reference, c),
            );
            assert!(
                chapter_text(&b.spec.chapters[c]).starts_with(&p.text),
                "{ctx}: NextJump into chapter {c}: page text starts spec chapter {c}"
            );
        }
        r.press(Action::NextJump);
        assert_eq!(
            r.phase(),
            Phase::Ready,
            "{ctx}: NextJump in the last chapter"
        );
        assert_eq!(
            r.chapter() as usize,
            n - 1,
            "{ctx}: no chapter after the last"
        );
    }
}

// Expected (same archived test): PrevJump from a chapter's first page lands on
// the previous chapter's first page, all the way to chapter 0, where it changes
// nothing.
#[test]
fn prev_jump_goes_to_the_first_page_of_the_previous_chapter_and_stops_at_the_first() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let n = b.spec.chapters.len();
        let reference = walk_forward(&mut open(&b));
        let mut r = open(&b);
        for _ in 1..n {
            r.press(Action::NextJump);
        }
        assert_same_pos(
            &format!("{ctx}: at the last chapter"),
            &pos(&r),
            first_page(&reference, n - 1),
        );
        for c in (0..n - 1).rev() {
            r.press(Action::PrevJump);
            let p = pos(&r);
            assert_same_pos(
                &format!("{ctx}: PrevJump into chapter {c}"),
                &p,
                first_page(&reference, c),
            );
            assert!(
                chapter_text(&b.spec.chapters[c]).starts_with(&p.text),
                "{ctx}: PrevJump into chapter {c}: page text starts spec chapter {c}"
            );
        }
        r.press(Action::PrevJump);
        assert_eq!(
            r.phase(),
            Phase::Ready,
            "{ctx}: PrevJump in the first chapter"
        );
        assert_same_pos(
            &format!("{ctx}: PrevJump at chapter 0"),
            &pos(&r),
            first_page(&reference, 0),
        );
    }
}

// Expected (archived navigation.rs `epub_quick_actions_list_depends_on_the_book`):
// an EPUB with a TOC and several chapters offers font size, previous chapter,
// next chapter and TOC, in that order; a single-chapter EPUB offers font size
// and TOC only.
#[test]
fn quick_actions_list_depends_on_the_chapters() {
    let _serial = serial();
    for b in books() {
        let r = open(&b);
        assert_eq!(
            r.quick_action_ids(),
            vec![QA_FONT_SIZE, QA_PREV_CHAPTER, QA_NEXT_CHAPTER, QA_TOC],
            "{}",
            b.name
        );
    }
    let mut spec = book("E2STORED.EPU").spec.clone();
    spec.chapters.truncate(1);
    spec.toc = vec![TocItem {
        title: "Part One".to_string(),
        chapter: 0,
        children: vec![],
    }];
    let bytes = build_epub(&spec).expect("single-chapter spec is valid");
    let r = rig("ONE.EPU", &bytes, FONT, THEME);
    assert_eq!(r.spine_len(), 1);
    assert_eq!(
        r.quick_action_ids(),
        vec![QA_FONT_SIZE, QA_TOC],
        "single-chapter book"
    );
}

// Expected (archived navigation.rs `epub_quick_prev_next_chapter_actions`): the
// next-chapter action moves to the next chapter's first page and does nothing
// after the last; the previous-chapter action moves to the previous chapter's
// first page and does nothing before the first.
#[test]
fn next_and_previous_chapter_quick_actions_move_by_chapters() {
    let _serial = serial();
    for b in text_books() {
        let ctx = b.name;
        let n = b.spec.chapters.len();
        let reference = walk_forward(&mut open(&b));
        let mut r = open(&b);
        for c in 1..n {
            r.quick_trigger(QA_NEXT_CHAPTER);
            assert_same_pos(
                &format!("{ctx}: next chapter {c}"),
                &pos(&r),
                first_page(&reference, c),
            );
        }
        r.quick_trigger(QA_NEXT_CHAPTER);
        assert_eq!(
            r.phase(),
            Phase::Ready,
            "{ctx}: next chapter after the last"
        );
        assert_eq!(
            r.chapter() as usize,
            n - 1,
            "{ctx}: no chapter after the last"
        );
        for c in (0..n - 1).rev() {
            r.quick_trigger(QA_PREV_CHAPTER);
            assert_same_pos(
                &format!("{ctx}: previous chapter {c}"),
                &pos(&r),
                first_page(&reference, c),
            );
        }
        r.quick_trigger(QA_PREV_CHAPTER);
        assert_eq!(
            r.phase(),
            Phase::Ready,
            "{ctx}: previous chapter before the first"
        );
        assert_eq!(r.chapter(), 0, "{ctx}: no chapter before the first");
    }
}

// ---------------------------------------------------------------------------
// English reader regression image pages
// ---------------------------------------------------------------------------

fn shot(r: &Rig) -> Framebuffer {
    render_stitched(&|s| r.draw(s)).frame
}

#[derive(Debug, Clone, Copy)]
struct Diff {
    x0: u16,
    y0: u16,
    // exclusive
    x1: u16,
    y1: u16,
    count: usize,
}

impl Diff {
    fn w(&self) -> u16 {
        self.x1 - self.x0
    }
    fn h(&self) -> u16 {
        self.y1 - self.y0
    }
}

// bounding box and number of the pixels that differ
fn diff(a: &Framebuffer, b: &Framebuffer) -> Option<Diff> {
    let mut d: Option<Diff> = None;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if a.is_black(x, y) != b.is_black(x, y) {
                d = Some(match d {
                    None => Diff {
                        x0: x,
                        y0: y,
                        x1: x + 1,
                        y1: y + 1,
                        count: 1,
                    },
                    Some(d) => Diff {
                        x0: d.x0.min(x),
                        y0: d.y0.min(y),
                        x1: d.x1.max(x + 1),
                        y1: d.y1.max(y + 1),
                        count: d.count + 1,
                    },
                });
            }
        }
    }
    d
}

// Rig geometry the image checks need
#[derive(Debug, Clone, Copy)]
struct Geo {
    margin: u16,
    text_w: u32,
    text_y: u16,
    line_h: u16,
}

fn geo(r: &Rig) -> Geo {
    Geo {
        margin: r.text_margin(),
        text_w: r.text_w(),
        text_y: r.text_y(),
        line_h: r.font_line_h(),
    }
}

// first and last image-flagged line of the page, as the vertical band
// [top, bottom) in px those lines occupy (line i spans text_y + i * line_h)
fn image_band(g: &Geo, lines: &[LineInfo]) -> (u16, u16) {
    let first = lines.iter().position(|l| l.image).expect("an image line");
    let last = lines.iter().rposition(|l| l.image).unwrap();
    (
        g.text_y + first as u16 * g.line_h,
        g.text_y + (last as u16 + 1) * g.line_h,
    )
}

// aspect ratio of (w, h) is the source's (W, H) up to the rounding of both
// scaled sides to whole pixels: |w * H - h * W| < max(W, H)
fn keeps_aspect(decoded: (u16, u16), source: (u16, u16)) -> bool {
    let (w, h) = (u32::from(decoded.0), u32::from(decoded.1));
    let (sw, sh) = (u32::from(source.0), u32::from(source.1));
    w >= 1 && h >= 1 && (w * sh).abs_diff(h * sw) < sw.max(sh)
}

// Next until a page carries an image; returns its page number
fn goto_image_page(r: &mut Rig) -> usize {
    for _ in 0..MAX_STEPS {
        if pos(r).images > 0 {
            return r.page();
        }
        let before = at(&pos(r));
        r.press(Action::Next);
        assert_ne!(at(&pos(r)), before, "the book ends before an image page");
    }
    panic!("no image page");
}

// Next until the reader is at `want` (chapter, page)
fn goto(r: &mut Rig, want: (u16, usize)) {
    for _ in 0..MAX_STEPS {
        let here = (r.chapter(), r.page());
        if here == want {
            return;
        }
        r.press(Action::Next);
        assert_ne!(
            (r.chapter(), r.page()),
            here,
            "Next cannot reach {want:?}, stuck at {here:?}"
        );
    }
    panic!("never reached {want:?}");
}

fn goto_page(r: &mut Rig, page: usize) {
    while r.page() < page {
        let before = r.page();
        r.press(Action::Next);
        assert_eq!(r.page(), before + 1, "Next advances one page");
    }
    assert_eq!(r.page(), page);
}

// Expected (English reader regression "images"): every <img> of the spec shows up as exactly one image
// line group in reading order -- the image-origin lines of chapter c's pages
// sum to the spec's image blocks of chapter c (cover inline in chapter 0: 1,
// then 2, 1, 0, 0) -- and each of the four pages carries one decoded image.
// A page with an image has a decoded image after settle; a page without one has
// none (no stale image). The decoded sizes keep the source's aspect ratio and
// fit the text area. Together this covers the Gray8, Gray1, palette and JPEG
// decoders and both stored and deflated image entries (the six books).
#[test]
fn every_image_block_becomes_one_decoded_image_page() {
    let _serial = serial();
    for b in books() {
        let ctx = b.name;
        let walk = walk_forward(&mut open(&b));
        for (c, ch) in b.spec.chapters.iter().enumerate() {
            let shown: usize = walk
                .iter()
                .filter(|p| p.chapter as usize == c)
                .map(|p| p.images)
                .sum();
            assert_eq!(shown, image_blocks(ch), "{ctx}: image lines of chapter {c}");
        }
        assert!(
            walk[0].images == 1,
            "{ctx}: the cover is shown inline at the top of chapter 0"
        );
        for p in &walk {
            assert_eq!(
                p.image.is_some(),
                p.images > 0,
                "{ctx}: page {:?} has {} image line(s), decoded image {:?}",
                at(p),
                p.images,
                p.image
            );
            assert!(
                p.images <= 1,
                "{ctx}: page {:?} carries at most one image",
                at(p)
            );
        }
        let r = open(&b);
        let g = geo(&r);
        let pages: Vec<&Pos> = walk.iter().filter(|p| p.images > 0).collect();
        let sources = images_in_order(&b.spec);
        assert_eq!(pages.len(), sources.len(), "{ctx}: one page per image");
        for (p, src) in pages.iter().zip(&sources) {
            let (w, h) = p.image.unwrap();
            assert!(
                keeps_aspect((w, h), (src.width, src.height)),
                "{ctx}: {}: decoded {w}x{h} vs source {}x{}",
                src.path,
                src.width,
                src.height
            );
            assert!(
                u32::from(w) <= g.text_w && h <= r.text_area_h(),
                "{ctx}: {}: decoded {w}x{h} fits the text area",
                src.path
            );
        }
    }
}

// Expected (strip equality on image pages): stitched strips and the full-frame reference
// render each page of the book -- the four image pages and every text page --
// to the same pixels.
#[test]
fn image_and_text_pages_render_identically_stitched_and_full() {
    let _serial = serial();
    for b in books() {
        let mut r = open(&b);
        walk_forward_visit(&mut r, &mut |r, p| {
            let (stitched, full) = (shot(r), render_full(&|s| r.draw(s)).frame);
            assert_eq!(
                stitched.to_pbm(),
                full.to_pbm(),
                "{}: page {:?} ({} image line(s)): stitched != full",
                b.name,
                at(p),
                p.images
            );
            if p.images > 0 {
                assert!(
                    stitched.black_count() > 0,
                    "{}: page {:?} has ink",
                    b.name,
                    at(p)
                );
            }
        });
    }
}

// the same book with every image replaced by an all-white one of the same kind
// and size (same layout, so the differing pixels are the images)
fn white_twin(spec: &EpubSpec) -> EpubSpec {
    let mut t = spec.clone();
    for im in &mut t.images {
        im.pattern = Pattern::White;
    }
    t
}

// Expected: the pixels an image page owns are inside the text area, inside the
// vertical band of the page's image-flagged lines, and not empty: render every
// image page of each standard book and of its all-white twin; the pages have the
// same lines, the two renderings differ, and every differing pixel lies in
// x in [margin, margin + text_w) and in the image band. The cover is a
// gradient, the others checkerboards, so each image has black pixels.
#[test]
fn standard_images_draw_ink_inside_their_own_band() {
    let _serial = serial();
    for b in books() {
        let ctx = b.name;
        let twin_bytes = build_epub(&white_twin(&b.spec)).expect("twin spec is valid");
        // (page, its rendering) of every image page; one rig alive at a time
        let capture = |name: &str, bytes: &[u8]| {
            let mut r = rig(name, bytes, FONT, THEME);
            let g = geo(&r);
            let mut pages: Vec<(Pos, Framebuffer)> = Vec::new();
            walk_forward_visit(&mut r, &mut |r, p| {
                if p.images > 0 {
                    pages.push((p.clone(), shot(r)));
                }
            });
            (g, pages)
        };
        let (g, real) = capture(b.name, &b.bytes);
        let (_, twin) = capture(b.name, &twin_bytes);
        assert_eq!(real.len(), 4, "{ctx}: four image pages");
        assert_eq!(twin.len(), 4, "{ctx}: four image pages in the white twin");
        for ((pa, fa), (pw, fw)) in real.iter().zip(&twin) {
            let want = at(pa);
            assert_eq!(want, at(pw), "{ctx}: same pages carry the images");
            assert_eq!(
                pa.lines, pw.lines,
                "{ctx}: {want:?}: the white twin lays out the same page"
            );
            assert_eq!(pa.image, pw.image, "{ctx}: {want:?}: same decoded size");
            let d =
                diff(fa, fw).unwrap_or_else(|| panic!("{ctx}: {want:?}: the image draws nothing"));
            let (top, bottom) = image_band(&g, &pa.lines);
            assert!(
                d.x0 >= g.margin && u32::from(d.x1) <= u32::from(g.margin) + g.text_w,
                "{ctx}: {want:?}: ink x {}..{} outside the text area",
                d.x0,
                d.x1
            );
            assert!(
                d.y0 >= top && d.y1 <= bottom,
                "{ctx}: {want:?}: ink y {}..{} outside the image band {top}..{bottom}",
                d.y0,
                d.y1
            );
            let (iw, ih) = pa.image.unwrap();
            assert!(
                d.w() <= iw && d.h() <= ih,
                "{ctx}: {want:?}: ink {}x{} exceeds the decoded {iw}x{ih}",
                d.w(),
                d.h()
            );
        }
    }
}

// ---- single-image books: solid, pattern, scaling -----------------------------

fn plate_path(kind: ImageKind) -> &'static str {
    match kind {
        ImageKind::Jpeg => "images/plate.jpg",
        _ => "images/plate.png",
    }
}

const SENTENCES: [&str; 4] = [
    "The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.",
    "Marta kept the lamp room spotless, though nobody had climbed the stairs since spring.",
    "A gull perched on the rail and watched the nets dry with the patience of an auditor.",
    "By noon the market smelled of rye bread, wet rope, and oranges from somewhere far warmer.",
];

fn para(seed: usize) -> Block {
    Block::Paragraph(vec![Run::Text(
        (0..4)
            .map(|i| SENTENCES[(seed + i) % 4])
            .collect::<Vec<_>>()
            .join(" "),
    )])
}

// one chapter: heading, two paragraphs, the image, two paragraphs
fn plate_spec(version: EpubVersion, kind: ImageKind, pattern: Pattern, w: u16, h: u16) -> EpubSpec {
    EpubSpec {
        version,
        title: "Plates".to_string(),
        author: "Test Author".to_string(),
        identifier: "urn:pulp-os:fixture:plates".to_string(),
        chapters: vec![Chapter {
            title: "Plate".to_string(),
            blocks: vec![
                Block::Heading("Study".to_string()),
                para(0),
                para(1),
                Block::Image(0),
                para(2),
                para(3),
            ],
        }],
        toc: vec![TocItem {
            title: "Plate".to_string(),
            chapter: 0,
            children: vec![],
        }],
        images: vec![ImageSpec {
            path: plate_path(kind).to_string(),
            kind,
            width: w,
            height: h,
            pattern,
        }],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    }
}

struct Plate {
    frame: Framebuffer,
    pos: Pos,
    geo: Geo,
    text_area_h: u16,
}

// open the one-image book and render its image page; `page` forces the page
// (None: the first page with an image)
fn plate(kind: ImageKind, pattern: Pattern, w: u16, h: u16, page: Option<usize>) -> Plate {
    plate_at(FONT, THEME, kind, pattern, w, h, page, false)
}

// `plate` at an explicit book font and reading theme
fn plate_at(
    font: u8,
    theme: u8,
    kind: ImageKind,
    pattern: Pattern,
    w: u16,
    h: u16,
    page: Option<usize>,
    early_image: bool,
) -> Plate {
    let mut spec = plate_spec(EpubVersion::V3, kind, pattern, w, h);
    if early_image {
        // the image right after the heading: it starts at the top of the first
        // page, so the page has room for all of it
        spec.chapters[0].blocks = vec![
            Block::Heading("Study".to_string()),
            Block::Image(0),
            para(0),
            para(1),
            para(2),
            para(3),
        ];
    }
    let bytes = build_epub(&spec).expect("plate spec is valid");
    let mut r = rig("PLATE.EPU", &bytes, font, theme);
    match page {
        Some(p) => goto_page(&mut r, p),
        None => {
            goto_image_page(&mut r);
        }
    }
    // A page the reader is on right after (re)opening a book may not have its
    // cached image decoded yet (observed when the image is not at the top of
    // the first page; see the EPUB regression report). Turning away and back enters the
    // page again, which decodes it.
    if r.page_image().is_none() {
        let p = r.page();
        if p + 1 < r.total_pages() {
            r.press(Action::Next);
            r.press(Action::Prev);
        } else {
            r.press(Action::Prev);
            r.press(Action::Next);
        }
        assert_eq!(r.page(), p, "back on the image page");
    }
    Plate {
        frame: shot(&r),
        pos: pos(&r),
        geo: geo(&r),
        text_area_h: r.text_area_h(),
    }
}

// (kind, width, height): every decoder, with row padding (61 wide) and without
const SOLID_CASES: [(ImageKind, u16, u16); 7] = [
    (ImageKind::PngGray1, 64, 48),
    (ImageKind::PngGray1, 61, 37),
    (ImageKind::PngGray8, 64, 48),
    (ImageKind::PngPalette8, 64, 48),
    (ImageKind::PngPalette8, 61, 37),
    (ImageKind::Jpeg, 64, 48),
    (ImageKind::Jpeg, 96, 64),
];

// Expected (English reader regression "images"): an all-black image draws a solid black rectangle and
// an all-white one nothing but background, whatever the decoder. So the pixels
// that differ between the black-image book and the white-image book form ONE
// SOLID RECTANGLE (differing-pixel count == bounding-box area) that
//   * is the decoded image the reader reports: width x height == page_image(),
//   * keeps the source's aspect ratio,
//   * lies inside the text area horizontally and inside the vertical band of
//     the page's image-flagged lines,
// and the three renderings come from identical page layouts.
#[test]
fn solid_images_fill_one_rectangle_inside_the_image_band() {
    let _serial = serial();
    for (kind, w, h) in SOLID_CASES {
        let ctx = format!("{kind:?} {w}x{h}");
        let black = plate(kind, Pattern::Black, w, h, None);
        let white = plate(kind, Pattern::White, w, h, Some(black.pos.page));
        assert_eq!(
            black.pos.lines, white.pos.lines,
            "{ctx}: black and white books lay out the same page"
        );
        assert_eq!(black.pos.images, 1, "{ctx}: one image on the page");

        let d = diff(&black.frame, &white.frame)
            .unwrap_or_else(|| panic!("{ctx}: the image draws nothing"));
        assert_eq!(
            d.count,
            usize::from(d.w()) * usize::from(d.h()),
            "{ctx}: the image is a solid {}x{} rectangle",
            d.w(),
            d.h()
        );
        for y in d.y0..d.y1 {
            for x in d.x0..d.x1 {
                assert!(
                    black.frame.is_black(x, y) && !white.frame.is_black(x, y),
                    "{ctx}: ({x}, {y}) is black only in the black book"
                );
            }
        }
        let (iw, ih) = black
            .pos
            .image
            .unwrap_or_else(|| panic!("{ctx}: no decoded image"));
        assert_eq!(
            (d.w(), d.h()),
            (iw, ih),
            "{ctx}: the rectangle is the decoded image"
        );
        assert!(
            keeps_aspect((iw, ih), (w, h)),
            "{ctx}: decoded {iw}x{ih} keeps the aspect of {w}x{h}"
        );
        let g = black.geo;
        assert!(
            d.x0 >= g.margin && u32::from(d.x1) <= u32::from(g.margin) + g.text_w,
            "{ctx}: x {}..{} outside the text area",
            d.x0,
            d.x1
        );
        let (top, bottom) = image_band(&g, &black.pos.lines);
        assert!(
            d.y0 >= top && d.y1 <= bottom,
            "{ctx}: y {}..{} outside the image band {top}..{bottom}",
            d.y0,
            d.y1
        );
    }
}

// Expected: a pattern image's ink is part of the black image's ink and not all
// of it (monotone in luminance): every pixel the pattern book draws black
// beyond the white book is inside the solid rectangle, none of the white
// book's ink disappears, and 0 < ink < the rectangle's area. Cases: 1-bit and
// palette checkerboards, 8-bit and JPEG checkerboards and horizontal
// gradients (dark on the left, light on the right).
#[test]
fn pattern_images_ink_a_proper_subset_of_the_solid_rectangle() {
    let _serial = serial();
    let cases = [
        (ImageKind::PngGray1, Pattern::Checker { cell: 8 }),
        (ImageKind::PngGray8, Pattern::Checker { cell: 8 }),
        (ImageKind::PngGray8, Pattern::HorizontalGradient),
        (ImageKind::PngPalette8, Pattern::Checker { cell: 4 }),
        (ImageKind::Jpeg, Pattern::Checker { cell: 8 }),
        (ImageKind::Jpeg, Pattern::HorizontalGradient),
    ];
    for (kind, pattern) in cases {
        let ctx = format!("{kind:?} {pattern:?}");
        let (w, h) = (64, 48);
        let black = plate(kind, Pattern::Black, w, h, None);
        let page = black.pos.page;
        let white = plate(kind, Pattern::White, w, h, Some(page));
        let pat = plate(kind, pattern, w, h, Some(page));
        assert_eq!(
            pat.pos.lines, black.pos.lines,
            "{ctx}: same layout as the solid book"
        );
        assert!(pat.pos.image.is_some(), "{ctx}: decoded");

        let solid = diff(&black.frame, &white.frame).expect("solid image draws");
        let mut ink = 0usize;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let (p, wh, bl) = (
                    pat.frame.is_black(x, y),
                    white.frame.is_black(x, y),
                    black.frame.is_black(x, y),
                );
                assert!(!wh || p, "{ctx}: ({x}, {y}) lost ink the white book has");
                if p != wh {
                    ink += 1;
                    assert!(
                        bl,
                        "{ctx}: ({x}, {y}) is inked outside the solid black image"
                    );
                }
            }
        }
        assert!(ink > 0, "{ctx}: the pattern draws ink");
        assert!(
            ink < solid.count,
            "{ctx}: the pattern is not solid ({ink} of {})",
            solid.count
        );
    }
}

// Expected: a source wider and taller than the text area is scaled down, not
// clipped, not skipped: the solid rectangle fits the text width, the band of
// its image lines and the text area height, and keeps the 4:3 aspect of
// the 560x420 source (text width is at most 464 px, so this source cannot be drawn 1:1).
#[test]
fn an_oversized_image_is_scaled_into_the_text_area() {
    let _serial = serial();
    let (kind, w, h) = (ImageKind::PngGray8, 560u16, 420u16);
    let black = plate(kind, Pattern::Black, w, h, None);
    let white = plate(kind, Pattern::White, w, h, Some(black.pos.page));
    assert_eq!(black.pos.lines, white.pos.lines);
    let d = diff(&black.frame, &white.frame).expect("the image draws");
    assert_eq!(
        d.count,
        usize::from(d.w()) * usize::from(d.h()),
        "solid rectangle"
    );
    let (iw, ih) = black.pos.image.expect("decoded");
    assert_eq!(
        (d.w(), d.h()),
        (iw, ih),
        "the rectangle is the decoded image"
    );
    assert!(
        keeps_aspect((iw, ih), (w, h)),
        "decoded {iw}x{ih} keeps 4:3"
    );
    assert!(
        u32::from(iw) <= black.geo.text_w,
        "{iw} px wide fits the {} px text width",
        black.geo.text_w
    );
    assert!(
        ih <= black.text_area_h,
        "{ih} px high fits the {} px text area",
        black.text_area_h
    );
    assert!(u32::from(iw) < u32::from(w), "the source is scaled down");
    let g = black.geo;
    assert!(d.x0 >= g.margin && u32::from(d.x1) <= u32::from(g.margin) + g.text_w);
    let (top, bottom) = image_band(&g, &black.pos.lines);
    assert!(
        d.y0 >= top && d.y1 <= bottom,
        "y {}..{} outside the image band {top}..{bottom}",
        d.y0,
        d.y1
    );
}

// ---- damaged images ----------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> usize {
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
        .unwrap_or_else(|| panic!("{:?} not found", String::from_utf8_lossy(needle)))
}

// Rewrite the bytes of the STORED zip entry `name` with `damage`, and fix the
// entry's CRC-32 in its local and central headers so that the zip layer still
// accepts it and the damage reaches the image decoder. The fixture zip layout
// is pinned (host/src/fixtures/zip.rs): local header 30 bytes + name, no extra
// field, sizes in the header; central record 46 bytes + name.
fn damage_entry(epub: &[u8], name: &str, damage: &dyn Fn(&mut [u8])) -> Vec<u8> {
    let mut out = epub.to_vec();
    let local_name = find(&out, name.as_bytes(), 0);
    let local = local_name - 30;
    assert_eq!(
        &out[local..local + 4],
        b"PK\x03\x04",
        "local header of {name}"
    );
    assert_eq!(
        u16::from_le_bytes([out[local + 8], out[local + 9]]),
        0,
        "{name} is stored"
    );
    let size = u32::from_le_bytes(out[local + 18..local + 22].try_into().unwrap()) as usize;
    let data = local_name + name.len();
    damage(&mut out[data..data + size]);
    let crc = crc32(&out[data..data + size]).to_le_bytes();
    out[local + 14..local + 18].copy_from_slice(&crc);
    let central_name = find(&out, name.as_bytes(), local_name + 1);
    let central = central_name - 46;
    assert_eq!(
        &out[central..central + 4],
        b"PK\x01\x02",
        "central record of {name}"
    );
    out[central + 16..central + 20].copy_from_slice(&crc);
    out
}

// the signature / header is destroyed: the decoder must refuse the image
fn kill_header(d: &mut [u8]) {
    d[..16].fill(0);
}

// the middle third of the file is overwritten with noise
fn scribble_middle(d: &mut [u8]) {
    let n = d.len();
    d[n / 3..2 * n / 3].fill(0xA5);
}

// Expected (English reader regression "images" failure): an image that cannot be loaded never takes
// the reader down. For each of the four images of a STORED book, with the
// header destroyed (decoder refuses it) or the middle scribbled over (decoder
// may refuse or draw garbage, both fine), the book is cached for a bounded
// time (a damaged image may keep the background busy: not asserted here),
// reopened, and:
//   * every page of the walk is a page (Phase::Ready, no Error, no panic, no
//     hang) and the walk reaches the last page of the last chapter,
//   * the chapters without images read exactly as the spec says,
//   * every page with an image line draws without panicking and stitched == full,
//   * with the header destroyed, exactly that image's page has no decoded image,
//     the other three keep theirs.
#[test]
fn a_damaged_image_leaves_the_book_readable() {
    let _serial = serial();
    for name in ["E2STORED.EPU", "E3STORED.EPU"] {
        let b = book(name);
        let intact = walk_forward(&mut open(&b));
        let intact_decoded = intact.iter().filter(|p| p.image.is_some()).count();
        assert_eq!(
            intact_decoded, 4,
            "{name}: all four images decode when intact"
        );
        for (i, im) in b.spec.images.iter().enumerate() {
            let entry = format!("OEBPS/{}", im.path);
            for (mode, damage) in [
                ("header destroyed", &kill_header as &dyn Fn(&mut [u8])),
                ("middle scribbled", &scribble_middle),
            ] {
                if mode == "middle scribbled" && name != "E2STORED.EPU" {
                    continue;
                }
                let ctx = format!("{name}: {} {mode}", im.path);
                let bytes = damage_entry(&b.bytes, &entry, damage);
                assert_ne!(bytes, b.bytes, "{ctx}: the file was damaged");
                let mut r = rig_warm(name, &bytes, FONT, THEME, false);
                // every page of the walk is a page; those with an image draw without panic
                let walk = walk_forward_visit(&mut r, &mut |r, p| {
                    if p.images > 0 {
                        let (s, f) = (shot(r), render_full(&|st| r.draw(st)).frame);
                        assert_eq!(
                            s.to_pbm(),
                            f.to_pbm(),
                            "{ctx}: page {:?} stitched != full",
                            at(p)
                        );
                    }
                });
                // the walk reached the end of the book, chapter by chapter
                let last = walk.last().unwrap();
                assert_eq!(
                    (last.chapter as usize, last.page + 1),
                    (b.spec.chapters.len() - 1, last.total),
                    "{ctx}: the walk ends on the last page of the last chapter"
                );
                // chapters without images read exactly as the spec says (image
                // chapters: see the `image_chapters_` tests)
                for (c, ch) in b.spec.chapters.iter().enumerate() {
                    if image_blocks(ch) == 0 {
                        check_chapter_text(&ctx, &b.spec, &walk, c);
                    }
                }
                if mode == "header destroyed" {
                    let decoded = walk.iter().filter(|p| p.image.is_some()).count();
                    assert_eq!(
                        decoded, 3,
                        "{ctx}: image {i} is not decoded, the other three are"
                    );
                }
            }
        }
    }
}

// ---- background caching: observable and terminating ---------------------------

// Idle ticks after which the reader's background caching of a book MUST be over,
// damaged images included. Derivation: caching a healthy standard book (five
// chapters, four images) takes a handful of ticks -- measured at 11 for every
// one of the six standard books at fonts 0 / 2 / 4, themes 0 / 1 / 3 -- so 600 is
// more than fifty times what the work needs. A damaged image may cost a few
// retries; a reader that is still busy after 600 ticks is retrying forever. The
// assertions below use this constant, never a measured tick count.
const CACHE_TICK_LIMIT: usize = 600;
// after the limit the book must stay quiet for this many more ticks, in steps
const QUIET_TICKS: usize = 600;
const QUIET_STEP: usize = 50;

// Expected (English reader regression "images" failure): a damaged image does not keep the reader's
// background caching busy forever. For every image of the STORED EPUB 2 and
// EPUB 3 books with its header destroyed, and of the EPUB 2 book with its middle
// scribbled over, on the first open (nothing cached): after CACHE_TICK_LIMIT idle
// ticks has_bg_work() is false, and it stays false over QUIET_TICKS more ticks.
#[test]
fn a_damaged_image_does_not_keep_the_background_caching_busy() {
    let _serial = serial();
    let cases: [(&str, &str, &dyn Fn(&mut [u8])); 3] = [
        ("E2STORED.EPU", "header destroyed", &kill_header),
        ("E3STORED.EPU", "header destroyed", &kill_header),
        ("E2STORED.EPU", "middle scribbled", &scribble_middle),
    ];
    for (name, mode, damage) in cases {
        let b = book(name);
        for im in &b.spec.images {
            let ctx = format!("{name}: {} {mode}", im.path);
            let bytes = damage_entry(&b.bytes, &format!("OEBPS/{}", im.path), damage);
            let mut r = rig_cold(name, &bytes, FONT, THEME);
            r.idle(CACHE_TICK_LIMIT);
            assert!(
                !r.has_bg_work(),
                "{ctx}: still caching after {CACHE_TICK_LIMIT} idle ticks"
            );
            for step in 1..=QUIET_TICKS / QUIET_STEP {
                r.idle(QUIET_STEP);
                assert!(
                    !r.has_bg_work(),
                    "{ctx}: busy again {} ticks after it finished",
                    step * QUIET_STEP
                );
            }
        }
    }
}

// Expected (English reader regression "images" failure): next to a damaged image the healthy ones are
// still cached and shown, including those in LATER chapters (a damaged image
// must not block the caching behind it). For each of the four images of the
// STORED EPUB 2 / EPUB 3 books with its header destroyed: after the bounded
// idle time (and `exit` + `open`) every page of the walk is Ready, the walk
// reaches the last page of the last chapter, the four image pages come in spec
// order, the damaged image's page has no decoded image, and each healthy
// image's page has one that keeps the source's aspect ratio.
#[test]
fn healthy_images_are_still_cached_and_shown_next_to_a_damaged_one() {
    let _serial = serial();
    for name in ["E2STORED.EPU", "E3STORED.EPU"] {
        let b = book(name);
        let sources = images_in_order(&b.spec);
        assert_eq!(sources.len(), 4);
        for (bad, im) in b.spec.images.iter().enumerate() {
            let ctx = format!("{name}: {} header destroyed", im.path);
            let bytes = damage_entry(&b.bytes, &format!("OEBPS/{}", im.path), &kill_header);
            let mut r = rig_cold(name, &bytes, FONT, THEME);
            r.idle(CACHE_TICK_LIMIT);
            assert!(!r.has_bg_work(), "{ctx}: caching ends");
            r.exit();
            r.open(name);
            assert_eq!(r.phase(), Phase::Ready, "{ctx}: reopens to a page");
            let walk = walk_forward(&mut r);
            let last = walk.last().unwrap();
            assert_eq!(
                (last.chapter as usize, last.page + 1),
                (b.spec.chapters.len() - 1, last.total),
                "{ctx}: the walk ends on the last page of the last chapter"
            );
            let pages: Vec<&Pos> = walk.iter().filter(|p| p.images > 0).collect();
            assert_eq!(pages.len(), 4, "{ctx}: four image pages");
            for (k, (p, src)) in pages.iter().zip(&sources).enumerate() {
                if k == bad {
                    assert_eq!(
                        p.image,
                        None,
                        "{ctx}: the damaged image at {:?} has no decoded image",
                        at(p)
                    );
                } else {
                    let got = p.image.unwrap_or_else(|| {
                        panic!(
                            "{ctx}: healthy image {} at {:?} is not decoded",
                            src.path,
                            at(p)
                        )
                    });
                    assert!(
                        keeps_aspect(got, (src.width, src.height)),
                        "{ctx}: {}: decoded {got:?} vs source {}x{}",
                        src.path,
                        src.width,
                        src.height
                    );
                }
            }
        }
    }
}

// Expected (English reader regression "images"; guards the rig's view of the reader's background
// state): on the first open of a book with uncached images the reader has
// caching work left (has_bg_work() is true) and the first page, which carries
// the cover, has no decoded image yet. Idle time then finishes the work
// (has_bg_work() is false within the bounded WARM_TICKS), and the caching is
// real: after `exit` + `open` nothing is left to cache and the cover page has
// its decoded image, as does each of the four image pages of the book. Six
// books: EPUB 2 / EPUB 3 x stored / deflate / mixed.
#[test]
fn idle_time_really_caches_the_images_and_the_background_state_says_so() {
    let _serial = serial();
    for b in books() {
        let ctx = b.name;
        let mut r = rig_cold(b.name, &b.bytes, FONT, THEME);
        assert!(
            r.has_bg_work(),
            "{ctx}: a freshly opened book has chapters and images left to cache"
        );
        assert_eq!(
            r.page_image(),
            None,
            "{ctx}: the cover is not cached yet on the first open"
        );
        for _ in 0..WARM_TICKS {
            if !r.has_bg_work() {
                break;
            }
            r.idle(1);
        }
        assert!(!r.has_bg_work(), "{ctx}: idle time finishes the caching");
        r.exit();
        r.open(b.name);
        assert!(
            !r.has_bg_work(),
            "{ctx}: a fully cached book has nothing left to cache"
        );
        assert!(
            r.page_image().is_some(),
            "{ctx}: the cover page shows its image once cached"
        );
        let walk = walk_forward(&mut r);
        let decoded = walk
            .iter()
            .filter(|p| p.images > 0 && p.image.is_some())
            .count();
        assert_eq!(decoded, 4, "{ctx}: all four image pages show their image");
    }
}

// ---- layout must not depend on caching progress -------------------------------

// every page of chapter `c`, first to last, as its lines; the reader is on the
// chapter's first page. Then back from the last page to the first, which must
// show the same pages.
fn read_chapter(ctx: &str, r: &mut Rig, c: u16) -> Vec<Vec<LineInfo>> {
    assert_eq!(
        (r.chapter(), r.page()),
        (c, 0),
        "{ctx}: on the first page of chapter {c}"
    );
    let mut fwd = vec![r.line_infos()];
    for _ in 0..MAX_STEPS {
        if r.page() + 1 >= r.total_pages() {
            break;
        }
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready, "{ctx}");
        assert_eq!(r.chapter(), c, "{ctx}: Next inside chapter {c} stays in it");
        fwd.push(r.line_infos());
    }
    assert_eq!(
        fwd.len(),
        r.total_pages(),
        "{ctx}: chapter {c} has {} pages",
        r.total_pages()
    );
    let mut back = vec![r.line_infos()];
    for _ in 1..fwd.len() {
        r.press(Action::Prev);
        assert_eq!(r.chapter(), c, "{ctx}: Prev inside chapter {c} stays in it");
        back.push(r.line_infos());
    }
    back.reverse();
    assert!(
        back == fwd,
        "{ctx}: chapter {c} read backwards shows other pages than read forwards"
    );
    fwd
}

// Expected (English reader regression previous / next paging with images): how a chapter is laid out
// does not depend on how far the background caching has got when the reader
// enters it. For each of the six books and each chapter with images (0, 1, 2),
// and for every idle-tick count 0..=12 spent before the jump (the six books are
// fully cached within a dozen ticks, so this walks the jump through every
// stage of the caching), the book is opened for the first time, left for
// `ticks` idle ticks, taken by NextJump to the chapter, left until the caching
// is over, and the chapter is read page by page forward and back. Then
//   * the chapter's text, pages concatenated, is the spec's chapter text (derived
//     from the spec: nothing lost, nothing repeated),
//   * its page count and every page's lines equal those of the same chapter in
//     the same book cached completely before it was entered (the "warm" book).
#[test]
fn image_chapter_layout_does_not_depend_on_caching_progress() {
    let _serial = serial();
    for b in books() {
        let want_pages = |c: u16| -> Vec<Vec<LineInfo>> {
            let mut w = open(&b);
            for _ in 0..c {
                w.press(Action::NextJump);
            }
            read_chapter(&format!("{} warm", b.name), &mut w, c)
        };
        for c in 0..3u16 {
            let warm = want_pages(c);
            let spec_text = chapter_text(&b.spec.chapters[c as usize]);
            let warm_text: String = warm.iter().map(|p| page_text(p)).collect();
            assert!(
                warm_text == spec_text,
                "{} warm: chapter {c} text: {}",
                b.name,
                first_diff(&warm_text, &spec_text)
            );
            for ticks in 0..=12 {
                let ctx = format!(
                    "{}: first open, {ticks} idle ticks, jump to chapter {c}",
                    b.name
                );
                let mut r = rig_cold(b.name, &b.bytes, FONT, THEME);
                r.idle(ticks);
                for _ in 0..c {
                    r.press(Action::NextJump);
                }
                for _ in 0..WARM_TICKS {
                    if !r.has_bg_work() {
                        break;
                    }
                    r.idle(1);
                }
                assert!(!r.has_bg_work(), "{ctx}: the caching finishes");
                let got = read_chapter(&ctx, &mut r, c);
                let got_text: String = got.iter().map(|p| page_text(p)).collect();
                assert!(
                    got_text == spec_text,
                    "{ctx}: chapter {c} text: {}",
                    first_diff(&got_text, &spec_text)
                );
                assert_eq!(got.len(), warm.len(), "{ctx}: pages in chapter {c}");
                for (i, (g, w)) in got.iter().zip(&warm).enumerate() {
                    assert!(
                        g == w,
                        "{ctx}: page {i} of chapter {c} differs from the warm book's"
                    );
                }
            }
        }
    }
}

// ---- tall images ---------------------------------------------------------------

// Expected (English reader regression "images"): an image taller than the room the reader keeps for an
// inline image is scaled down to that room and drawn entirely inside the lines
// reserved for it, whatever the line height. For every book font (5) x reading
// theme (4) -- the line height and so the rounding of the reserved room to
// whole lines change with each -- and for a 200x900 and a 300x1200 8-bit PNG:
// the black-image book and the white-image book (same layout) differ in ONE
// SOLID RECTANGLE that
//   * is the decoded image the reader reports (page_image()), scaled down from
//     the source (height < source height) with its aspect ratio,
//   * lies in the text area horizontally and in the vertical band of the page's
//     image-flagged lines, so no pixel below the band changes (the text under
//     the image is not covered), and the band is not shorter than the image.
#[test]
fn tall_images_are_drawn_inside_their_image_band_at_every_font_and_theme() {
    let _serial = serial();
    for font in 0..5u8 {
        for theme in 0..4u8 {
            for (w, h) in [(200u16, 900u16), (300, 1200)] {
                let ctx = format!("font {font} theme {theme} {w}x{h}");
                let kind = ImageKind::PngGray8;
                let black = plate_at(font, theme, kind, Pattern::Black, w, h, None, true);
                let white = plate_at(
                    font,
                    theme,
                    kind,
                    Pattern::White,
                    w,
                    h,
                    Some(black.pos.page),
                    true,
                );
                assert_eq!(black.pos.lines, white.pos.lines, "{ctx}: same page layout");
                let (iw, ih) = black
                    .pos
                    .image
                    .unwrap_or_else(|| panic!("{ctx}: no decoded image"));
                assert!(
                    ih < h && keeps_aspect((iw, ih), (w, h)),
                    "{ctx}: decoded {iw}x{ih} is the source scaled down"
                );
                let d = diff(&black.frame, &white.frame)
                    .unwrap_or_else(|| panic!("{ctx}: the image draws nothing"));
                assert_eq!(
                    d.count,
                    usize::from(d.w()) * usize::from(d.h()),
                    "{ctx}: the image is a solid rectangle"
                );
                let g = black.geo;
                assert_eq!(
                    (d.w(), d.h()),
                    (iw, ih),
                    "{ctx}: the rectangle is the decoded image (rows {}..{}, image band {:?}, text area ends at {}, page {}/{})",
                    d.y0,
                    d.y1,
                    image_band(&g, &black.pos.lines),
                    g.text_y + black.text_area_h,
                    black.pos.page,
                    black.pos.total
                );
                assert!(
                    d.x0 >= g.margin && u32::from(d.x1) <= u32::from(g.margin) + g.text_w,
                    "{ctx}: x {}..{} outside the text area",
                    d.x0,
                    d.x1
                );
                let (top, bottom) = image_band(&g, &black.pos.lines);
                assert!(
                    d.y0 >= top && d.y1 <= bottom,
                    "{ctx}: image rows {}..{} leave the image band {top}..{bottom}",
                    d.y0,
                    d.y1
                );
                assert!(
                    bottom - top >= ih,
                    "{ctx}: the band ({}) is shorter than the image ({ih})",
                    bottom - top
                );
            }
        }
    }
}

// Read-only view of the real ReaderApp's state for crate::reader::Rig. This
// module sits in `apps` (the parent of `reader`) so it can see the `pub(super)`
// fields of the real struct; it adds no behaviour.
use super::App;
use super::reader::{ReaderApp, State};
use crate::error::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Ready,
    Toc,
    Error,
}

pub fn phase(a: &ReaderApp) -> Phase {
    match a.state {
        State::Ready => Phase::Ready,
        State::ShowToc => Phase::Toc,
        State::Error => Phase::Error,
        _ => Phase::Loading,
    }
}

pub fn state_name(a: &ReaderApp) -> String {
    format!("{:?}", a.state)
}

pub fn error_kind(a: &ReaderApp) -> Option<ErrorKind> {
    a.error.as_ref().map(|e| e.kind())
}

pub fn page(a: &ReaderApp) -> usize {
    a.pg.page
}

pub fn total_pages(a: &ReaderApp) -> usize {
    a.pg.total_pages
}

pub fn fully_indexed(a: &ReaderApp) -> bool {
    a.pg.fully_indexed
}

pub fn offsets(a: &ReaderApp) -> Vec<u32> {
    a.pg.offsets[..a.pg.total_pages].to_vec()
}

pub fn max_lines(a: &ReaderApp) -> usize {
    a.max_lines as usize
}

pub fn text_w(a: &ReaderApp) -> u32 {
    a.text_w
}

pub fn text_margin(a: &ReaderApp) -> u16 {
    a.text_margin
}

pub fn font_line_h(a: &ReaderApp) -> u16 {
    a.font_line_h
}

pub fn text_y(a: &ReaderApp) -> u16 {
    a.text_y
}

pub fn text_area_h(a: &ReaderApp) -> u16 {
    a.text_area_h
}

// the current page, one entry per laid-out line, raw bytes of the page buffer
pub fn lines(a: &ReaderApp) -> Vec<Vec<u8>> {
    a.pg.lines[..a.pg.line_count]
        .iter()
        .map(|l| a.pg.buf[l.start as usize..(l.start + l.len) as usize].to_vec())
        .collect()
}

// ---- EPUB navigation views (EPUB regression): the real reader's own fields / methods ----

pub const QA_FONT_SIZE: u8 = super::reader::QA_FONT_SIZE;
pub const QA_PREV_CHAPTER: u8 = super::reader::QA_PREV_CHAPTER;
pub const QA_NEXT_CHAPTER: u8 = super::reader::QA_NEXT_CHAPTER;
pub const QA_TOC: u8 = super::reader::QA_TOC;

// one laid-out line of the current page: the raw page-buffer bytes and the
// real LineSpan's image flags
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInfo {
    pub bytes: Vec<u8>,
    pub image: bool,
    pub image_origin: bool,
}

pub fn is_epub(a: &ReaderApp) -> bool {
    a.is_epub
}

pub fn chapter(a: &ReaderApp) -> u16 {
    a.chapter()
}

pub fn spine_len(a: &ReaderApp) -> usize {
    a.epub.spine.len()
}

pub fn epub_title(a: &ReaderApp) -> String {
    a.epub.meta.title_str().to_string()
}

pub fn epub_author(a: &ReaderApp) -> String {
    a.epub.meta.author_str().to_string()
}

pub fn toc_entries(a: &ReaderApp) -> Vec<(String, u16)> {
    a.epub.toc.as_ref().map_or_else(Vec::new, |t| {
        t.entries[..t.len()]
            .iter()
            .map(|e| (e.title_str().to_string(), e.spine_idx))
            .collect()
    })
}

pub fn toc_selected(a: &ReaderApp) -> usize {
    a.epub.toc_selected
}

pub fn line_infos(a: &ReaderApp) -> Vec<LineInfo> {
    a.pg.lines[..a.pg.line_count]
        .iter()
        .map(|l| LineInfo {
            bytes: a.pg.buf[l.start as usize..(l.start + l.len) as usize].to_vec(),
            image: l.is_image(),
            image_origin: l.is_image_origin(),
        })
        .collect()
}

// the decoded 1-bit image the current page draws
pub fn page_image(a: &ReaderApp) -> Option<(u16, u16)> {
    a.page_img.as_ref().map(|i| (i.width, i.height))
}

pub fn quick_action_ids(a: &ReaderApp) -> Vec<u8> {
    a.quick_actions().iter().map(|q| q.id).collect()
}

pub fn quick_trigger(a: &mut ReaderApp, id: u8, ctx: &mut super::AppContext) {
    a.on_quick_trigger(id, ctx);
}

pub fn exit(a: &mut ReaderApp) {
    a.on_exit();
}

pub fn has_bg_work(a: &ReaderApp) -> bool {
    a.has_bg_work()
}

pub fn theme_idx(a: &ReaderApp) -> u8 {
    a.reading_theme_idx
}

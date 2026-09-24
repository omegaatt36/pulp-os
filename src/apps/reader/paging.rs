// text wrapping, page navigation, and load/prefetch

use pulp_render::layout::{self, Markup, WrapParams, WrapState, Wrapped};
use smol_epub::html_strip::{
    BOLD_OFF, BOLD_ON, HEADING_OFF, HEADING_ON, IMG_REF, ITALIC_OFF, ITALIC_ON, MARKER, QUOTE_OFF,
    QUOTE_ON,
};

use pulp_render::page::PackMeasure;

use crate::error::{Error, ErrorKind};
use crate::kernel::KernelHandle;
use crate::kernel::pack::HandlePackReader;

use super::font::BookFont;
use super::{
    DEFAULT_IMG_H, INDENT_PX, LINES_PER_PAGE, LineSpan, MAX_PAGES, NO_PREFETCH, PAGE_BUF,
    ReaderApp, State,
};

// the html_strip stream format, as the layout reads it
pub(crate) const MARKUP: Markup = Markup {
    marker: MARKER,
    img_ref: IMG_REF,
    bold_on: BOLD_ON,
    bold_off: BOLD_OFF,
    italic_on: ITALIC_ON,
    italic_off: ITALIC_OFF,
    heading_on: HEADING_ON,
    heading_off: HEADING_OFF,
    quote_on: QUOTE_ON,
    quote_off: QUOTE_OFF,
};

impl ReaderApp {
    // lay out pg.buf[..n] as the current page; eof tells whether the
    // chunk ends at the end of the text. The returned state belongs to
    // its consumed byte offset, so a later page can resume markup styling.
    /// Lay the current page out and report what it consumed.
    ///
    /// `k` is the SD card, and the pack font needs it: measuring a CJK glyph
    /// means reading its advance out of the pack, so there is no way to lay
    /// out a pack-backed page without one. Every call site runs inside a
    /// background pass that already holds a handle, so this takes it
    /// directly rather than caching a borrow in the app.
    /// Lay the current page out and report what it consumed.
    ///
    /// `k` is the SD card, and the pack font needs it: measuring a CJK glyph
    /// means reading its advance out of the pack, so a pack-backed page
    /// cannot be laid out without one. Every call site runs inside a
    /// background pass that already holds a handle, so it is passed in
    /// rather than cached in the app (a self-referential borrow of the
    /// kernel would not survive the scheduler's borrow checker).
    /// Lay the current page out and report what it consumed.
    ///
    /// `k` is the SD card, and the pack font needs it: measuring a CJK glyph
    /// means reading its advance out of the pack, so a pack-backed page
    /// cannot be laid out without one. Every call site runs inside a
    /// background pass that already holds a handle, so it is passed in
    /// rather than cached in the app -- a self-referential borrow of the
    /// kernel would not survive the scheduler's borrow of it.
    pub(super) fn wrap_lines_counted(
        &mut self,
        k: &mut KernelHandle<'_>,
        n: usize,
        eof: bool,
        initial: WrapState,
    ) -> Wrapped {
        // Pack first: it needs storage, so it gets the handle. Any failure
        // drops to the built-in face and lays the page out again rather than
        // showing an empty page.
        let packed = match &self.fonts {
            Some(BookFont::Pack { .. }) => Some(self.wrap_with_pack(k, n, eof, initial)),
            _ => None,
        };
        if let Some(Ok(out)) = packed {
            self.pg.line_count = out.line_count;
            return out;
        }
        if matches!(self.fonts, Some(BookFont::Pack { .. })) {
            log::info!("reader: pack unusable, falling back to built-in font");
            self.select_builtin_font();
        }

        // Destructure so the font, the params and the line buffer are
        // disjoint: WrapParams reads the image-height table while
        // wrap_stream fills pg.lines, and through `self` the borrow checker
        // cannot see them as disjoint.
        let built_in = {
            let ReaderApp {
                fonts,
                pg,
                max_lines,
                text_w,
                img_heights,
                img_height_count,
                ..
            } = self;
            match fonts {
                Some(BookFont::BuiltIn(fs)) => {
                    let params = WrapParams {
                        markup: MARKUP,
                        max_lines: *max_lines as usize,
                        max_width_px: *text_w,
                        indent_px: INDENT_PX,
                        img_heights: &img_heights[..*img_height_count as usize],
                        default_img_h: DEFAULT_IMG_H,
                    };
                    match layout::wrap_stream(
                        &pg.buf[..n],
                        eof,
                        fs,
                        &params,
                        &mut pg.lines,
                        initial,
                    ) {
                        Ok(out) => {
                            pg.line_count = out.line_count;
                            Ok(out)
                        }
                        Err(_) => Err(()),
                    }
                }
                _ => Err(()),
            }
        };

        match built_in {
            Ok(out) => out,
            Err(_) => Wrapped {
                consumed: self.wrap_monospace(n),
                line_count: self.pg.line_count,
                next_state: WrapState::default(),
            },
        }
    }

    /// Measure this page against the pack on the card. `Err` means the pack
    /// could not be read and the caller should use the built-in fonts.
    fn wrap_with_pack(
        &mut self,
        k: &mut KernelHandle<'_>,
        n: usize,
        eof: bool,
        initial: WrapState,
    ) -> Result<Wrapped, ()> {
        let Some(BookFont::Pack { name, name_len, .. }) = &self.fonts else {
            return Err(());
        };
        let (name, name_len) = (*name, *name_len);
        let mut reader = HandlePackReader::open(k, &name, name_len).map_err(|_| ())?;

        let ReaderApp {
            fonts,
            pg,
            max_lines,
            text_w,
            img_heights,
            img_height_count,
            ..
        } = self;
        let Some(BookFont::Pack { pack, .. }) = fonts else {
            return Err(());
        };
        let params = WrapParams {
            markup: MARKUP,
            max_lines: *max_lines as usize,
            max_width_px: *text_w,
            indent_px: INDENT_PX,
            img_heights: &img_heights[..*img_height_count as usize],
            default_img_h: DEFAULT_IMG_H,
        };
        let mut measure = PackMeasure::new(pack, &mut reader);
        layout::wrap_stream(
            &pg.buf[..n],
            eof,
            &mut measure,
            &params,
            &mut pg.lines,
            initial,
        )
        .map_err(|_| ())
    }

    fn page_has_content(&self) -> bool {
        let lines = &self.pg.lines[..self.pg.line_count];
        // valid for both fonts: line_glyphs skips image spans, and the
        // built-in path's spans come out of the same layout::wrap
        layout::page_has_content(&self.pg.buf[..self.pg.buf_len], lines, MARKUP)
    }

    pub(super) fn wrap_monospace(&mut self, n: usize) -> usize {
        use super::CHARS_PER_LINE;

        let max = self.max_lines as usize;
        self.pg.line_count = 0;
        let mut col: usize = 0;
        let mut line_start: usize = 0;

        for i in 0..n {
            let b = self.pg.buf[i];
            match b {
                b'\r' => {}
                b'\n' => {
                    let end = trim_trailing_cr(&self.pg.buf, line_start, i);
                    self.push_line(line_start, end);
                    line_start = i + 1;
                    col = 0;
                    if self.pg.line_count >= max {
                        return line_start;
                    }
                }
                _ => {
                    col += 1;
                    if col >= CHARS_PER_LINE {
                        self.push_line(line_start, i + 1);
                        line_start = i + 1;
                        col = 0;
                        if self.pg.line_count >= max {
                            return line_start;
                        }
                    }
                }
            }
        }

        if line_start < n && self.pg.line_count < max {
            let end = trim_trailing_cr(&self.pg.buf, line_start, n);
            self.push_line(line_start, end);
        }

        n
    }

    pub(super) fn push_line(&mut self, start: usize, end: usize) {
        if self.pg.line_count < LINES_PER_PAGE {
            self.pg.lines[self.pg.line_count] = LineSpan {
                start: start as u16,
                len: (end - start) as u16,
                flags: 0,
                indent: 0,
            };
            self.pg.line_count += 1;
        }
    }

    pub(super) fn reset_paging(&mut self) {
        self.pg.page = 0;
        self.pg.offsets[0] = 0;
        self.pg.wrap_states[0] = WrapState::default();
        self.pg.total_pages = 1;
        self.pg.fully_indexed = false;
        self.pg.buf_len = 0;
        self.pg.line_count = 0;
        self.pg.prefetch_page = NO_PREFETCH;
        self.pg.prefetch_len = 0;
        self.page_img = None;
        self.fullscreen_img = false;
        self.glyphs_dirty = true;
    }

    pub(super) fn load_and_prefetch(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        if !self.epub.ch_cache.is_empty() {
            loop {
                let start = (self.pg.offsets[self.pg.page] as usize).min(self.epub.ch_cache.len());
                let end = (start + PAGE_BUF).min(self.epub.ch_cache.len());
                let n = end - start;
                if n > 0 {
                    self.pg.buf[..n].copy_from_slice(&self.epub.ch_cache[start..end]);
                }
                self.pg.buf_len = n;
                self.pg.prefetch_page = NO_PREFETCH;
                self.pg.prefetch_len = 0;
                self.prescan_image_heights(k, n);
                let out = self.wrap_lines_counted(
                    k,
                    n,
                    end == self.epub.ch_cache.len(),
                    self.pg.wrap_states[self.pg.page],
                );
                if !self.page_has_content() {
                    if let Some(next) =
                        layout::next_page_offset(start, out.consumed, self.epub.ch_cache.len())
                            .map_err(|_| {
                                Error::new(ErrorKind::InvalidData, "reader chapter offset")
                            })?
                    {
                        self.pg.offsets[self.pg.page] = u32::try_from(next).map_err(|_| {
                            Error::new(ErrorKind::InvalidData, "reader chapter offset")
                        })?;
                        self.pg.wrap_states[self.pg.page] = out.next_state;
                        continue;
                    }
                    if self.pg.page > 0 && self.pg.page + 1 == self.pg.total_pages {
                        self.pg.total_pages -= 1;
                        self.pg.page -= 1;
                        self.pg.fully_indexed = true;
                        continue;
                    }
                }
                self.decode_page_images(k);
                return Ok(());
            }
        }

        let (nb, nl) = self.name_copy();
        let name = core::str::from_utf8(&nb[..nl]).unwrap_or("");

        loop {
            if self.pg.prefetch_page == self.pg.page {
                let pf_len = self.pg.prefetch_len;
                self.pg.buf[..pf_len].copy_from_slice(&self.pg.prefetch[..pf_len]);
                self.pg.buf_len = pf_len;
                self.pg.prefetch_page = NO_PREFETCH;
                self.pg.prefetch_len = 0;
            } else if self.is_epub && self.epub.chapters_cached {
                let cf_str = self.epub.cache_file_str();
                let ch = self.epub.chapter as usize;
                let ch_base = self.epub.chapter_table[ch].0;
                let n = k.read_cache_chunk(
                    cf_str,
                    ch_base + self.pg.offsets[self.pg.page],
                    &mut self.pg.buf,
                )?;
                self.pg.buf_len = n;
            } else if self.file_size == 0 {
                let (size, n) = k.read_file_start(name, &mut self.pg.buf)?;
                self.file_size = size;
                self.pg.buf_len = n;
                log::info!("reader: opened {} ({} bytes)", name, size);

                if size == 0 {
                    self.pg.fully_indexed = true;
                    self.pg.line_count = 0;
                    return Ok(());
                }
            } else {
                let n = k.read_chunk(name, self.pg.offsets[self.pg.page], &mut self.pg.buf)?;
                self.pg.buf_len = n;
            }

            self.prescan_image_heights(k, self.pg.buf_len);
            let page_offset = self.pg.offsets[self.pg.page];
            let eof = page_offset as u64 + self.pg.buf_len as u64 >= self.file_size as u64;
            let out =
                self.wrap_lines_counted(k, self.pg.buf_len, eof, self.pg.wrap_states[self.pg.page]);
            let next_offset = layout::next_page_offset(
                page_offset as usize,
                out.consumed,
                self.file_size as usize,
            )
            .map_err(|_| Error::new(ErrorKind::InvalidData, "reader page offset"))?;

            if !self.page_has_content() {
                if let Some(next) = next_offset {
                    self.pg.offsets[self.pg.page] = u32::try_from(next)
                        .map_err(|_| Error::new(ErrorKind::InvalidData, "reader page offset"))?;
                    self.pg.wrap_states[self.pg.page] = out.next_state;
                    continue;
                }
                if self.pg.page > 0 && self.pg.page + 1 == self.pg.total_pages {
                    // The previous page covered the last drawable text;
                    // this indexed tail contained only controls/markup.
                    self.pg.total_pages -= 1;
                    self.pg.page -= 1;
                    self.pg.fully_indexed = true;
                    self.pg.prefetch_page = NO_PREFETCH;
                    continue;
                }
            }

            if self.pg.page + 1 >= self.pg.total_pages && !self.pg.fully_indexed {
                if let Some(next_offset) = next_offset {
                    if self.pg.total_pages < MAX_PAGES {
                        self.pg.offsets[self.pg.total_pages] =
                            u32::try_from(next_offset).map_err(|_| {
                                Error::new(ErrorKind::InvalidData, "reader page offset")
                            })?;
                        self.pg.wrap_states[self.pg.total_pages] = out.next_state;
                        self.pg.total_pages += 1;
                    } else {
                        return Err(Error::new(
                            ErrorKind::BufferTooSmall,
                            "reader page index full",
                        ));
                    }
                } else {
                    self.pg.fully_indexed = true;
                }
            }

            if self.pg.page + 1 < self.pg.total_pages {
                if self.pg.prefetch.len() < PAGE_BUF {
                    self.pg.prefetch.resize(PAGE_BUF, 0);
                }
                let pf_offset = self.pg.offsets[self.pg.page + 1];
                let pf_result = if self.is_epub && self.epub.chapters_cached {
                    let cf_str = self.epub.cache_file_str();
                    let ch = self.epub.chapter as usize;
                    let ch_base = self.epub.chapter_table[ch].0;
                    k.read_cache_chunk(cf_str, ch_base + pf_offset, &mut self.pg.prefetch)
                } else {
                    k.read_chunk(name, pf_offset, &mut self.pg.prefetch)
                };
                match pf_result {
                    Ok(n) => {
                        self.pg.prefetch_len = n;
                        self.pg.prefetch_page = self.pg.page + 1;
                    }
                    Err(_) => {
                        self.pg.prefetch_page = NO_PREFETCH;
                        self.pg.prefetch_len = 0;
                    }
                }
            } else {
                self.pg.prefetch_page = NO_PREFETCH;
                self.pg.prefetch_len = 0;
            }

            self.decode_page_images(k);
            return Ok(());
        }
    }

    pub(super) fn preindex_all_pages(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        if self.epub.ch_cache.is_empty() {
            return Ok(());
        }

        let total = self.epub.ch_cache.len();
        self.pg.offsets[0] = 0;
        self.pg.wrap_states[0] = WrapState::default();
        self.pg.total_pages = 1;

        let mut offset = 0usize;
        while offset < total {
            let end = (offset + PAGE_BUF).min(total);
            let n = end - offset;
            self.pg.buf[..n].copy_from_slice(&self.epub.ch_cache[offset..end]);
            self.pg.buf_len = n;
            self.prescan_image_heights(k, n);

            let out = self.wrap_lines_counted(
                k,
                n,
                end == total,
                self.pg.wrap_states[self.pg.total_pages - 1],
            );
            let next_offset = layout::next_page_offset(offset, out.consumed, total)
                .map_err(|_| Error::new(ErrorKind::InvalidData, "reader chapter offset"))?;

            if !self.page_has_content() {
                if let Some(next_offset) = next_offset {
                    self.pg.offsets[self.pg.total_pages - 1] = u32::try_from(next_offset)
                        .map_err(|_| Error::new(ErrorKind::InvalidData, "reader chapter offset"))?;
                    self.pg.wrap_states[self.pg.total_pages - 1] = out.next_state;
                    offset = next_offset;
                    continue;
                }
                if self.pg.total_pages > 1 {
                    self.pg.total_pages -= 1;
                }
                break;
            }

            if let Some(next_offset) = next_offset {
                if self.pg.total_pages == MAX_PAGES {
                    return Err(Error::new(
                        ErrorKind::BufferTooSmall,
                        "reader page index full",
                    ));
                }
                self.pg.offsets[self.pg.total_pages] = u32::try_from(next_offset)
                    .map_err(|_| Error::new(ErrorKind::InvalidData, "reader chapter offset"))?;
                self.pg.wrap_states[self.pg.total_pages] = out.next_state;
                self.pg.total_pages += 1;
                offset = next_offset;
            } else {
                break;
            }
        }

        self.pg.fully_indexed = true;
        log::info!("chapter pre-indexed: {} pages", self.pg.total_pages);
        Ok(())
    }

    pub(super) fn scan_to_last_page(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        while !self.pg.fully_indexed && self.pg.total_pages < MAX_PAGES {
            self.pg.page = self.pg.total_pages - 1;
            self.load_and_prefetch(k)?;
            if self.pg.page + 1 < self.pg.total_pages {
                self.pg.page += 1;
            } else {
                break;
            }
        }
        if self.pg.total_pages > 0 {
            self.pg.page = self.pg.total_pages - 1;
        }
        self.pg.prefetch_page = NO_PREFETCH;
        self.load_and_prefetch(k)
    }

    pub(super) fn page_forward(&mut self) -> bool {
        if self.state != State::Ready {
            return false;
        }

        if self.pg.page + 1 < self.pg.total_pages {
            self.pg.page += 1;
            self.state = State::NeedPage;
            return true;
        }

        if self.is_epub
            && self.pg.fully_indexed
            && (self.epub.chapter as usize + 1) < self.epub.spine.len()
        {
            self.epub.chapter += 1;
            self.goto_last_page = false;
            self.state = State::NeedIndex;
            return true;
        }

        false
    }

    pub(super) fn page_backward(&mut self) -> bool {
        if self.state != State::Ready {
            return false;
        }

        if self.pg.page > 0 {
            self.pg.page -= 1;
            self.state = State::NeedPage;
            return true;
        }

        if self.is_epub && self.epub.chapter > 0 {
            self.epub.chapter -= 1;
            self.goto_last_page = true;
            self.state = State::NeedIndex;
            return true;
        }

        false
    }

    // next chapter (EPUB) or +10 pages (TXT)
    pub(super) fn jump_forward(&mut self) -> bool {
        if self.state != State::Ready {
            return false;
        }
        if self.is_epub {
            if (self.epub.chapter as usize + 1) < self.epub.spine.len() {
                self.epub.chapter += 1;
                self.goto_last_page = false;
                self.state = State::NeedIndex;
                return true;
            }
        } else {
            let last = if self.pg.total_pages > 0 {
                self.pg.total_pages - 1
            } else {
                0
            };
            let target = (self.pg.page + 10).min(last);
            if target != self.pg.page {
                self.pg.page = target;
                self.state = State::NeedPage;
                return true;
            }
        }
        false
    }

    // prev chapter (EPUB) or -10 pages (TXT)
    pub(super) fn jump_backward(&mut self) -> bool {
        if self.state != State::Ready {
            return false;
        }
        if self.is_epub {
            if self.epub.chapter > 0 {
                self.epub.chapter -= 1;
                self.goto_last_page = false;
                self.state = State::NeedIndex;
                return true;
            }
        } else {
            let target = self.pg.page.saturating_sub(10);
            if target != self.pg.page {
                self.pg.page = target;
                self.state = State::NeedPage;
                return true;
            }
        }
        false
    }
}

pub(super) fn trim_trailing_cr(buf: &[u8], start: usize, end: usize) -> usize {
    if end > start && buf[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

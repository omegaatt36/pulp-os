// text wrapping, page navigation, and load/prefetch

use smol_epub::html_strip::{IMG_REF, MARKER, QUOTE_OFF, QUOTE_ON};

use crate::fonts;
use crate::fonts::bitmap::FIRST_CHAR;
use crate::kernel::BufClass;
use crate::kernel::KernelHandle;
use pulp_board_logic::page_index::{BANK_BODY_USED, BANK_HEADING_USED, PageStep, next_page};
use pulp_kernel::util::utf8_incomplete_tail_len;

use super::{
    INDENT_PX, LINES_PER_PAGE, LineSpan, NO_PREFETCH, PAGE_BUF, ReaderApp, State, decode_utf8_char,
    inline_img_max_h,
};

/// Page-start marker flag: an earlier window of this chapter staged fallback
/// glyphs, so style and indent carried over the page boundary apply even to a
/// window that holds only Latin text. Bits 0..=2 are bold, italic and heading.
const CARRIED_FALLBACK: u8 = 1 << 7;

impl ReaderApp {
    fn current_layout_identity(
        &self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<super::LayoutIdentity> {
        let idx = self.book_font_size_idx.min(4) as usize;
        let body_px = fonts::cjk::BODY_PIXELS[idx];
        let heading_px = fonts::cjk::HEADING_PIXELS[idx];
        let previous = self.layout_identity;
        Ok(super::LayoutIdentity {
            body_px,
            heading_px,
            body: if previous.is_some_and(|id| id.body.is_some()) {
                Some(fonts::cjk::bank_identity(k, body_px)?)
            } else {
                None
            },
            heading: if previous.is_some_and(|id| id.heading.is_some()) {
                Some(fonts::cjk::bank_identity(k, heading_px)?)
            } else {
                None
            },
            version: super::LAYOUT_VERSION,
            text_w: self.text_w,
            text_area_h: self.text_area_h,
            line_h: self.font_line_h,
            max_lines: self.max_lines,
        })
    }

    fn record_fallback_identity(&mut self, k: &mut KernelHandle<'_>) -> crate::error::Result<()> {
        // which packs this chapter depends on, for its stored page index
        if let Some(identity) = &self.layout_identity {
            if self.cjk.uses_bank(identity.body_px) {
                self.pg.banks_used |= BANK_BODY_USED;
            }
            if self.cjk.uses_bank(identity.heading_px) {
                self.pg.banks_used |= BANK_HEADING_USED;
            }
        }
        if let Some(identity) = &mut self.layout_identity {
            // Retain every participating bank even when later windows contain
            // Latin only or rendering releases its transient glyph caches.
            if identity.body.is_none() && self.cjk.uses_bank(identity.body_px) {
                identity.body = Some(fonts::cjk::bank_identity(k, identity.body_px)?);
            }
            if identity.heading.is_none() && self.cjk.uses_bank(identity.heading_px) {
                identity.heading = Some(fonts::cjk::bank_identity(k, identity.heading_px)?);
            }
        }
        Ok(())
    }

    pub(super) fn invalidate_layout(&mut self) {
        // Before chapter indexing, the page table can still belong to the
        // departed chapter. Only explicit restore positions apply to the target.
        let anchor = if self.is_epub && self.state == State::NeedIndex {
            self.session_position
                .map(|(_, offset)| offset)
                .or(self.restore_offset)
        } else {
            Some(self.byte_offset())
        };
        self.reset_paging();
        self.restore_offset = anchor;
        // An explicit rebuild already preserves the source anchor. Establish
        // the new identity when wrapping instead of invalidating twice.
        self.layout_identity = None;
        self.cjk.clear();
        self.render_fonts_released = false;
        self.state = if self.is_epub && self.epub.chapters_cached {
            State::NeedIndex
        } else {
            State::NeedPage
        };
    }

    pub(super) fn check_layout_identity(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        if let Some(previous) = self.layout_identity {
            let current = self.current_layout_identity(k)?;
            if current != previous {
                self.invalidate_layout();
                self.layout_identity = Some(current);
            }
        }
        Ok(())
    }

    pub(super) fn wrap_lines_counted(
        &mut self,
        k: &mut KernelHandle<'_>,
        n: usize,
        at_eof: bool,
    ) -> crate::error::Result<usize> {
        #[cfg(feature = "sd-metrics")]
        let tick = self.profile.tick();
        let result = self.wrap_lines_counted_inner(k, n, at_eof);
        #[cfg(feature = "sd-metrics")]
        self.profile.record(super::profile::Phase::Layout, tick);
        result
    }

    fn wrap_lines_counted_inner(
        &mut self,
        k: &mut KernelHandle<'_>,
        n: usize,
        at_eof: bool,
    ) -> crate::error::Result<usize> {
        if self.layout_identity.is_none() {
            self.layout_identity = Some(self.current_layout_identity(k)?);
        }
        if self.fonts.is_none() && fonts::font_data::HAS_REGULAR {
            // An explicitly selected mono path keeps its original cell layout.
            return Ok(self.wrap_monospace(n));
        }
        let initial_flags = self.pg.style_flags[self.pg.page];
        let initial_indent = self.pg.indents[self.pg.page];
        if !fonts::font_data::HAS_REGULAR {
            self.fonts = None;
        }
        let mut fonts_copy = self.fonts;
        if fonts_copy.is_none() {
            let fs = fonts::FontSet::for_size(self.book_font_size_idx);
            let idx = self.book_font_size_idx.min(4) as usize;
            self.cjk.stage_metrics_prefix(
                k,
                &self.pg.buf[..n],
                fs,
                fonts::cjk::BODY_PIXELS[idx],
                fonts::cjk::HEADING_PIXELS[idx],
                initial_flags,
                self.stage_prefix,
            )?;
            if self.cjk.has_fallback() {
                // The SD fallback does not require optional flash font assets.
                fonts_copy = Some(fs);
                self.fonts = fonts_copy;
            } else {
                let consumed = self.wrap_monospace(n);
                self.record_continuation(consumed, initial_flags, initial_indent);
                return Ok(consumed);
            }
        }

        if let Some(fs) = fonts_copy {
            let idx = self.book_font_size_idx.min(4) as usize;
            // Stage the start of the window only: the page takes a small part of
            // it. The layout reports the last byte it measured; when that lies
            // beyond what was staged it measured scalars without metrics, so
            // stage more and lay out again. The accepted layout therefore only
            // ever measured staged scalars: it is the one a fully staged window
            // gives (`has_fallback` is the same either way).
            let mut prefix = self.stage_prefix.min(n);
            loop {
                self.cjk.stage_metrics_prefix(
                    k,
                    &self.pg.buf[..n],
                    fs,
                    fonts::cjk::BODY_PIXELS[idx],
                    fonts::cjk::HEADING_PIXELS[idx],
                    initial_flags,
                    prefix,
                )?;
                self.record_fallback_identity(k)?;
                let view = self.cjk.view(
                    fs,
                    fonts::cjk::BODY_PIXELS[idx],
                    fonts::cjk::HEADING_PIXELS[idx],
                );
                let carried = view.has_fallback() || initial_flags & CARRIED_FALLBACK != 0;
                let mut examined = 0;
                let laid_out = wrap_proportional(
                    &self.pg.buf,
                    n,
                    &view,
                    &mut self.pg.lines,
                    self.max_lines as usize,
                    self.text_w,
                    inline_img_max_h(self.text_area_h),
                    if carried { initial_flags } else { 0 },
                    if carried { initial_indent } else { 0 },
                    at_eof,
                    &mut examined,
                );
                if examined > prefix && prefix < n {
                    prefix = prefix.saturating_mul(2).max(examined).min(n);
                    continue;
                }
                let (c, count) = laid_out?;
                self.record_continuation(c, initial_flags, initial_indent);
                self.pg.line_count = count;
                return Ok(c);
            }
        } else {
            let consumed = self.wrap_monospace(n);
            self.record_continuation(consumed, initial_flags, initial_indent);
            Ok(consumed)
        }
    }

    fn record_continuation(&mut self, consumed: usize, flags: u8, mut indent: u8) {
        let carried = flags & CARRIED_FALLBACK != 0 || self.cjk.has_fallback();
        let mut styles = fonts::StyleState::from_flags(flags);
        if self.pg.page + 1 >= self.pg.capacity() {
            return;
        }
        let buf = &self.pg.buf[..consumed];
        let mut i = 0;
        while i < buf.len() {
            if buf[i] == MARKER && i + 1 < buf.len() {
                if buf[i + 1] == IMG_REF && i + 2 < buf.len() {
                    let end = i + 3 + buf[i + 2] as usize;
                    if end <= buf.len() && buf[i + 2] > 0 {
                        i = end;
                        continue;
                    }
                }
                styles.apply_marker(buf[i + 1]);
                match buf[i + 1] {
                    QUOTE_ON => indent = indent.saturating_add(1),
                    QUOTE_OFF => indent = indent.saturating_sub(1),
                    _ => {}
                }
                i += 2;
            } else {
                i += decode_utf8_char(buf, i).1;
            }
        }
        self.pg.style_flags[self.pg.page + 1] =
            styles.flags() | if carried { CARRIED_FALLBACK } else { 0 };
        self.pg.indents[self.pg.page + 1] = indent;
    }

    pub(super) fn prepare_page_fonts(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        #[cfg(feature = "sd-metrics")]
        let tick = self.profile.tick();
        let result = self.prepare_page_fonts_inner(k);
        #[cfg(feature = "sd-metrics")]
        self.profile.record(super::profile::Phase::Visible, tick);
        result
    }

    fn prepare_page_fonts_inner(&mut self, k: &mut KernelHandle<'_>) -> crate::error::Result<()> {
        if let Some(fs) = self.fonts {
            let idx = self.book_font_size_idx.min(4) as usize;
            let body = fonts::cjk::BODY_PIXELS[idx];
            let heading = fonts::cjk::HEADING_PIXELS[idx];
            self.cjk.begin_visible();
            for span in &self.pg.lines[..self.pg.line_count] {
                if !span.is_image() {
                    let start = span.start as usize;
                    self.cjk.mark_visible_with_flags(
                        &self.pg.buf[start..start + span.len as usize],
                        span.flags,
                        fs,
                        body,
                        heading,
                    )?;
                }
            }
            self.cjk.prepare_visible(k, body, heading)?;
        }
        Ok(())
    }

    pub(super) fn wrap_monospace(&mut self, n: usize) -> usize {
        use super::CHARS_PER_LINE;

        let max = self.max_lines as usize;
        self.pg.line_count = 0;
        let mut col: usize = 0;
        let mut line_start: usize = 0;

        let mut i = 0;
        while i < n {
            let b = self.pg.buf[i];
            match b {
                b'\r' => i += 1,
                b'\n' => {
                    let end = trim_trailing_cr(&self.pg.buf, line_start, i);
                    self.push_line(line_start, end);
                    i += 1;
                    line_start = i;
                    col = 0;
                    if self.pg.line_count >= max {
                        return line_start;
                    }
                }
                _ => {
                    // one column per scalar, a malformed subpart is one cell
                    i += decode_utf8_char(&self.pg.buf[..n], i).1;
                    col += 1;
                    if col >= CHARS_PER_LINE {
                        self.push_line(line_start, i);
                        line_start = i;
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
        self.pg.offsets.fill(0);
        self.pg.style_flags.fill(0);
        self.pg.indents.fill(0);
        self.pg.total_pages = usize::from(!self.pg.offsets.is_empty());
        self.pg.fully_indexed = false;
        self.pg.truncated = false;
        self.pg.banks_used = 0;
        self.pg.buf_len = 0;
        self.pg.line_count = 0;
        self.pg.prefetch_page = NO_PREFETCH;
        self.pg.prefetch_len = 0;
        self.page_img = None;
        self.fullscreen_img = false;
        self.paused.index = false;
        self.paused.restore = false;
        self.paused.window = None;
        self.paused.verified = false;
    }

    /// The current page, or with a restore position pending the walk from the
    /// first page up to it; the position is spent on every outcome but a pause
    /// between CJK slices, which the next call resumes at the page it stopped in.
    pub(super) fn load_page_step(&mut self, k: &mut KernelHandle<'_>) -> crate::error::Result<()> {
        self.pg.ensure_tables()?;
        let result = match self.restore_offset {
            Some(target_off) => self.walk_to(k, target_off),
            None => self.load_and_prefetch(k, true),
        };
        if !matches!(&result, Err(e) if fonts::cjk::is_slice_end(e)) {
            self.restore_offset = None;
            self.paused.restore = false;
        }
        result
    }

    fn walk_to(&mut self, k: &mut KernelHandle<'_>, target_off: u32) -> crate::error::Result<()> {
        if !self.paused.restore {
            self.pg.page = 0;
        }
        self.paused.restore = true;
        loop {
            self.load_and_prefetch(k, false)?;
            if self.pg.page + 1 >= self.pg.total_pages {
                break;
            }
            if self.pg.offsets[self.pg.page + 1] > target_off {
                break;
            }
            self.pg.page += 1;
        }
        self.prepare_page_fonts(k)
    }

    /// Page table of the chapter in RAM and, for a backwards chapter change,
    /// the last page up to which it is walked. Between CJK slices
    /// (`paused.index`) it carries on where it stopped.
    pub(super) fn index_chapter_pages(
        &mut self,
        k: &mut KernelHandle<'_>,
        want_last: bool,
    ) -> crate::error::Result<()> {
        if self.is_epub {
            let resident = self.paused.index || self.epub.try_cache_chapter(k);
            // a stored index of this very layout replaces the layout of the
            // whole chapter (also for a chapter that streams from the card); a
            // resumed (paused) indexing carries on instead
            if !self.paused.index && self.load_page_index(k) {
                // complete table loaded
            } else if resident {
                self.preindex_all_pages(k)?;
                self.store_page_index(k);
            }
        }
        if want_last {
            self.scan_to_last_page(k)?;
        }
        Ok(())
    }

    pub(super) fn load_and_prefetch(
        &mut self,
        k: &mut KernelHandle<'_>,
        visible: bool,
    ) -> crate::error::Result<()> {
        #[cfg(feature = "sd-metrics")]
        let text_tick = self.profile.tick();
        let resident = self.epub.current_text().len();
        if resident != 0 {
            let start = (self.pg.offsets[self.pg.page] as usize).min(resident);
            let end = (start + PAGE_BUF).min(resident);
            let n = end - start;
            if n > 0 {
                self.pg.buf[..n].copy_from_slice(&self.epub.current_text()[start..end]);
            }
            self.pg.buf_len = n;
            self.pg.prefetch_page = NO_PREFETCH;
            self.pg.prefetch_len = 0;
            #[cfg(feature = "sd-metrics")]
            self.profile.record(super::profile::Phase::Text, text_tick);
            self.wrap_lines_counted(
                k,
                text_len(&self.pg.buf, n, end == resident),
                end == resident,
            )?;
            if visible {
                self.prepare_page_fonts(k)?;
            }
            self.decode_page_images(k);
            return Ok(());
        }

        let (nb, nl) = self.name_copy();
        let name = core::str::from_utf8(&nb[..nl]).unwrap_or("");

        // A pause of this very window (CJK glyph slices) left its bytes in
        // `pg.buf`: lay them out again without opening the file on the card.
        let base = self.pg.offsets[self.pg.page];
        let source = if self.is_epub && self.epub.chapters_cached {
            self.epub.chapter + 1
        } else {
            0
        };
        let kept = self.paused.window.is_some_and(|w| {
            w.chapter == source && w.offset == base && w.len as usize == self.pg.buf_len
        });
        let fetched = (|| -> crate::error::Result<bool> {
            if !kept {
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
                    // the cache file is flat: a read past the end of this
                    // chapter would return the beginning of the next one
                    let left = (self.file_size as usize).saturating_sub(base as usize);
                    let want = left.min(PAGE_BUF);
                    let n = k.read_cache_chunk(cf_str, ch_base + base, &mut self.pg.buf[..want])?;
                    self.pg.buf_len = n;
                } else if self.file_size == 0 {
                    let (size, n) = k.read_file_start(name, &mut self.pg.buf)?;
                    self.file_size = size;
                    self.pg.buf_len = n;
                    log::info!("reader: opened {} ({} bytes)", name, size);

                    if size == 0 {
                        self.pg.fully_indexed = true;
                        self.pg.line_count = 0;
                        return Ok(true);
                    }
                } else {
                    let n = k.read_chunk(name, self.pg.offsets[self.pg.page], &mut self.pg.buf)?;
                    self.pg.buf_len = n;
                }

                // A short device read is not EOF when the known file size says more
                // bytes remain. Complete this bounded window before deriving pages.
                while self.pg.buf_len < PAGE_BUF
                    && base as usize + self.pg.buf_len < self.file_size as usize
                {
                    let at = base + self.pg.buf_len as u32;
                    let out = &mut self.pg.buf[self.pg.buf_len..];
                    let out_len = out.len().min(self.file_size as usize - at as usize);
                    let out = &mut out[..out_len];
                    let n = if self.is_epub && self.epub.chapters_cached {
                        let cf_str = self.epub.cache_file_str();
                        let ch_base = self.epub.chapter_table[self.epub.chapter as usize].0;
                        k.read_cache_chunk(cf_str, ch_base + at, out)?
                    } else {
                        k.read_chunk(name, at, out)?
                    };
                    if n == 0 || n > out.len() {
                        return Err(crate::error::Error::READ_FAILED);
                    }
                    self.pg.buf_len += n;
                }
                self.paused.window = Some(super::TextWindow {
                    chapter: source,
                    offset: base,
                    len: self.pg.buf_len as u32,
                });
            }
            Ok(false)
        })();
        #[cfg(feature = "sd-metrics")]
        self.profile.record(super::profile::Phase::Text, text_tick);
        if fetched? {
            return Ok(());
        }
        let at_eof =
            self.pg.offsets[self.pg.page] as usize + self.pg.buf_len >= self.file_size as usize;
        let consumed =
            self.wrap_lines_counted(k, text_len(&self.pg.buf, self.pg.buf_len, at_eof), at_eof)?;
        if visible {
            self.prepare_page_fonts(k)?;
        }
        let next_offset = self.pg.offsets[self.pg.page] + consumed as u32;

        if self.pg.page + 1 >= self.pg.total_pages && !self.pg.fully_indexed && !self.pg.truncated {
            match next_page(
                next_offset as usize,
                self.file_size as usize,
                consumed,
                self.pg.total_pages,
                self.pg.capacity(),
            ) {
                PageStep::Append => {
                    self.pg.offsets[self.pg.total_pages] = next_offset;
                    self.pg.total_pages += 1;
                }
                PageStep::Done => {
                    self.pg.fully_indexed = true;
                    // a chapter indexed by reading it (too large for the ring)
                    // is complete now: keep its index for the next open
                    self.store_page_index(k);
                }
                // the text goes on but the table is full: the index is NOT
                // complete, and nothing below may treat it as the end
                PageStep::Truncated => self.pg.truncated = true,
            }
        }

        self.prefetch_next_page(k);

        // loaded: only a pause keeps the window for the next slice
        self.paused.window = None;
        self.decode_page_images(k);
        Ok(())
    }

    pub(super) fn prefetch_next_page(&mut self, k: &mut KernelHandle<'_>) {
        if self.pg.prefetch_page != NO_PREFETCH {
            return;
        }
        #[cfg(feature = "sd-metrics")]
        let prefetch_tick = self.profile.tick();
        if self.pg.page + 1 < self.pg.total_pages
            && self
                .pg
                .prefetch
                .ensure_len(BufClass::ChapterText, PAGE_BUF)
                .is_ok()
        {
            let pf_offset = self.pg.offsets[self.pg.page + 1];
            let (nb, nl) = self.name_copy();
            let name = core::str::from_utf8(&nb[..nl]).unwrap_or("");
            let pf_result = if self.is_epub && self.epub.chapters_cached {
                let cf_str = self.epub.cache_file_str();
                let ch = self.epub.chapter as usize;
                let ch_base = self.epub.chapter_table[ch].0;
                let left = (self.file_size as usize).saturating_sub(pf_offset as usize);
                let want = left.min(self.pg.prefetch.len());
                k.read_cache_chunk(cf_str, ch_base + pf_offset, &mut self.pg.prefetch[..want])
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
        #[cfg(feature = "sd-metrics")]
        self.profile
            .record(super::profile::Phase::Text, prefetch_tick);
    }

    pub(super) fn preindex_all_pages(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        let total = self.epub.current_text().len();
        if total == 0 {
            return Ok(());
        }

        self.pg.ensure_tables()?;
        if self.paused.index {
            // resumed: the table is complete up to the page the pause came in
            if self.pg.fully_indexed || self.pg.truncated {
                return Ok(());
            }
        } else {
            self.pg.offsets[0] = 0;
            self.pg.style_flags[0] = 0;
            self.pg.indents[0] = 0;
            self.pg.total_pages = 1;
            self.pg.truncated = false;
        }

        let capacity = self.pg.capacity();
        let mut offset = self.pg.offsets[self.pg.total_pages - 1] as usize;
        let mut truncated = false;
        while offset < total {
            self.pg.page = self.pg.total_pages - 1;
            let end = (offset + PAGE_BUF).min(total);
            let n = end - offset;
            #[cfg(feature = "sd-metrics")]
            let text_tick = self.profile.tick();
            self.pg.buf[..n].copy_from_slice(&self.epub.current_text()[offset..end]);
            self.pg.buf_len = n;
            #[cfg(feature = "sd-metrics")]
            self.profile.record(super::profile::Phase::Text, text_tick);

            let consumed =
                self.wrap_lines_counted(k, text_len(&self.pg.buf, n, end == total), end == total)?;
            let next_offset = offset + consumed;

            match next_page(next_offset, total, consumed, self.pg.total_pages, capacity) {
                PageStep::Append => {
                    self.pg.offsets[self.pg.total_pages] = next_offset as u32;
                    self.pg.total_pages += 1;
                    offset = next_offset;
                }
                PageStep::Done => break,
                PageStep::Truncated => {
                    truncated = true;
                    break;
                }
            }
        }

        self.pg.page = 0;
        // complete only when the text ended; a full table is a truncation
        self.pg.fully_indexed = !truncated;
        self.pg.truncated = truncated;
        log::info!(
            "chapter pre-indexed: {} pages{}",
            self.pg.total_pages,
            if truncated { " (table full)" } else { "" }
        );
        Ok(())
    }

    pub(super) fn scan_to_last_page(
        &mut self,
        k: &mut KernelHandle<'_>,
    ) -> crate::error::Result<()> {
        while !self.pg.fully_indexed
            && !self.pg.truncated
            && self.pg.total_pages < self.pg.capacity()
        {
            self.pg.page = self.pg.total_pages - 1;
            self.load_and_prefetch(k, false)?;
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
        self.load_and_prefetch(k, true)
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

        // a full table has nowhere to put the pages beyond it: the reader moves
        // on as it always did, but the chapter is not reported complete
        // (`fully_indexed` stays false: no page total, no stored index)
        if self.is_epub
            && (self.pg.fully_indexed || self.pg.truncated)
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

// UTF-8 decoding is provided by pulp_kernel::util::decode_utf8_char
// (re-exported via super::decode_utf8_char)

// bytes of the n read into buf to lay out: unless the text ends there, a scalar
// cut by the end of the read is left for the page that starts at it
fn text_len(buf: &[u8], n: usize, at_eof: bool) -> usize {
    if at_eof {
        n
    } else {
        n - utf8_incomplete_tail_len(&buf[..n])
    }
}

pub(super) fn trim_trailing_cr(buf: &[u8], start: usize, end: usize) -> usize {
    if end > start && buf[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

// true if ch is a word-separator for line-wrapping (space, NBSP, etc)
#[inline]
fn is_wrap_space(ch: char) -> bool {
    matches!(ch, ' ' | '\u{00A0}')
}

fn no_line_head(ch: char) -> bool {
    "，。、？！）」』】".contains(ch)
}
fn no_line_tail(ch: char) -> bool {
    "（「『【".contains(ch)
}

pub(super) fn wrap_proportional(
    buf: &[u8],
    n: usize,
    fonts: &fonts::cjk::LayoutFonts<'_>,
    lines: &mut [LineSpan],
    max_lines: usize,
    max_width_px: u32,
    img_h: u16,
    initial_flags: u8,
    initial_indent: u8,
    at_eof: bool,
    // end of the last scalar whose advance was asked for (see the caller)
    examined: &mut usize,
) -> crate::error::Result<(usize, usize)> {
    let max_l = max_lines.min(lines.len());
    let base_max_w = max_width_px;
    let mut line_count: usize = 0;
    let mut line_start: usize = 0;
    let mut cursor_x: u32 = 0;
    let mut last_space: usize = 0;
    let mut cursor_at_space: u32 = 0;
    let mut legal_break = 0usize;
    let mut width_at_break = 0u32;
    let mut previous = None;
    let mut line_flags = initial_flags;
    let mut legal_flags = initial_flags;
    let mut space_flags = initial_flags;
    // Lines start in the style that was active at their first byte whenever
    // the window carries style across pages or stages fallback glyphs.
    let line_start_flags = fonts.has_fallback() || initial_flags & CARRIED_FALLBACK != 0;

    let mut styles = fonts::StyleState::from_flags(initial_flags);
    let mut indent = initial_indent;
    let mut max_w = base_max_w.saturating_sub(INDENT_PX * indent as u32);

    macro_rules! emit {
        ($start:expr, $end:expr) => {
            if line_count < max_l {
                let e = trim_trailing_cr(buf, $start, $end);
                lines[line_count] = LineSpan {
                    start: ($start) as u16,
                    len: (e - ($start)) as u16,
                    flags: if line_start_flags {
                        line_flags
                    } else {
                        styles.flags()
                    },
                    indent,
                };
                line_count += 1;
            }
        };
    }

    let mut i = 0;
    while i < n {
        let b = buf[i];

        if b == MARKER && i + 1 < n {
            if buf[i + 1] == IMG_REF && i + 2 < n {
                let path_len = buf[i + 2] as usize;
                let path_start = i + 3;
                if path_start + path_len <= n && path_len > 0 {
                    if line_start < i {
                        emit!(line_start, i);
                        if line_count >= max_l {
                            return Ok((i, line_count));
                        }
                    }

                    let line_h = fonts.line_height(fonts::Style::Regular);
                    // every image reserves the same height, so a chapter's
                    // pages depend on its text and the settings only: not on
                    // the image files, their compression or the image cache
                    // ceiling division: ensure reserved lines fully cover image height
                    let img_lines = img_h.div_ceil(line_h).max(1) as usize;

                    if line_count < max_l {
                        lines[line_count] = LineSpan {
                            start: path_start as u16,
                            len: path_len as u16,
                            flags: LineSpan::FLAG_IMAGE,
                            indent: 0,
                        };
                        line_count += 1;
                    }

                    for _ in 1..img_lines {
                        if line_count >= max_l {
                            break;
                        }
                        lines[line_count] = LineSpan {
                            start: 0,
                            len: 0,
                            flags: LineSpan::FLAG_IMAGE,
                            indent: 0,
                        };
                        line_count += 1;
                    }

                    i = path_start + path_len;
                    line_start = i;
                    legal_break = line_start;
                    previous = None;
                    line_flags = styles.flags();
                    cursor_x = 0;
                    last_space = line_start;
                    space_flags = line_flags;
                    cursor_at_space = 0;
                    if line_count >= max_l {
                        return Ok((line_start, line_count));
                    }
                    continue;
                }
            }

            styles.apply_marker(buf[i + 1]);
            match buf[i + 1] {
                QUOTE_ON => {
                    indent = indent.saturating_add(1);
                    max_w = base_max_w.saturating_sub(INDENT_PX * indent as u32);
                }
                QUOTE_OFF => {
                    indent = indent.saturating_sub(1);
                    max_w = base_max_w.saturating_sub(INDENT_PX * indent as u32);
                }
                _ => {}
            }
            i += 2;
            continue;
        }

        if b == b'\r' {
            i += 1;
            continue;
        }

        if b == b'\n' {
            emit!(line_start, i);
            line_start = i + 1;
            legal_break = line_start;
            previous = None;
            line_flags = styles.flags();
            cursor_x = 0;
            last_space = line_start;
            space_flags = line_flags;
            cursor_at_space = 0;
            if line_count >= max_l {
                return Ok((line_start, line_count));
            }
            i += 1;
            continue;
        }

        // UTF-8 multi-byte: decode the full character and measure it
        // using the font's extended glyph tables; a malformed subpart
        // measures as the replacement character
        if b >= 0x80 || (fonts.has_fallback() && b >= b' ') {
            let (ch, seq_len) = decode_utf8_char(&buf[..n], i);
            *examined = (*examined).max(i + seq_len);

            // soft hyphen (U+00AD): zero-width break opportunity
            if ch == '\u{00AD}' {
                last_space = i + seq_len;
                space_flags = styles.flags();
                cursor_at_space = cursor_x;
                i += seq_len;
                continue;
            }

            // NBSP and regular spaces: word-break opportunity
            if is_wrap_space(ch) && !fonts.has_fallback() {
                let sty = styles.style();
                cursor_x += fonts.advance(' ', sty) as u32;
                last_space = i + seq_len;
                space_flags = styles.flags();
                cursor_at_space = cursor_x;
                if cursor_x > max_w {
                    emit!(line_start, i);
                    line_start = i + seq_len;
                    legal_break = line_start;
                    previous = None;
                    line_flags = styles.flags();
                    cursor_x = 0;
                    last_space = line_start;
                    space_flags = line_flags;
                    cursor_at_space = 0;
                    if line_count >= max_l {
                        return Ok((line_start, line_count));
                    }
                }
                i += seq_len;
                continue;
            }

            let sty = styles.style();
            let adv = fonts.advance(if ch == '\u{a0}' { ' ' } else { ch }, sty) as u32;
            if fonts.has_fallback() {
                // A boundary is legal only after a non-opening character and
                // before a non-closing character. Inseparable groups overhang
                // when there is no legal earlier boundary on this line.
                if i > line_start && !previous.is_some_and(no_line_tail) && !no_line_head(ch) {
                    legal_break = i;
                    width_at_break = cursor_x;
                    legal_flags = styles.flags();
                }
                cursor_x += adv;
                if cursor_x > max_w && legal_break > line_start {
                    emit!(line_start, legal_break);
                    cursor_x -= width_at_break;
                    line_start = legal_break;
                    line_flags = legal_flags;
                    legal_break = line_start;
                    width_at_break = 0;
                    last_space = line_start;
                    space_flags = line_flags;
                    cursor_at_space = 0;
                    if line_count >= max_l {
                        return Ok((line_start, line_count));
                    }
                }
                previous = Some(ch);
            } else {
                cursor_x += adv;
                if cursor_x > max_w {
                    if last_space > line_start {
                        emit!(line_start, last_space);
                        cursor_x -= cursor_at_space;
                        line_start = last_space;
                        line_flags = space_flags;
                    } else if i > line_start {
                        emit!(line_start, i);
                        line_start = i;
                        line_flags = styles.flags();
                        cursor_x = adv;
                    }
                    last_space = line_start;
                    space_flags = line_flags;
                    cursor_at_space = 0;
                    if line_count >= max_l {
                        return Ok((line_start, line_count));
                    }
                }
            }
            i += seq_len;
            continue;
        }

        // --- ASCII fast path: batch space and word runs ---
        let sty = styles.style();
        let font = fonts.font(sty);
        let glyphs = font.glyphs;

        if b == b' ' {
            let adv = glyphs[(b' ' - FIRST_CHAR) as usize].advance as u32;
            cursor_x += adv;
            last_space = i + 1;
            space_flags = styles.flags();
            cursor_at_space = cursor_x;
            if cursor_x > max_w {
                emit!(line_start, i);
                line_start = i + 1;
                line_flags = styles.flags();
                cursor_x = 0;
                last_space = line_start;
                space_flags = line_flags;
                cursor_at_space = 0;
                if line_count >= max_l {
                    return Ok((line_start, line_count));
                }
            }
            i += 1;
            continue;
        }

        // Printable non-space ASCII (0x21..=0x7E): batch-scan the word run.
        // Find end of contiguous printable non-space ASCII bytes, sum advances.
        let word_start = i;
        let remaining = max_w.saturating_sub(cursor_x);
        let mut run_adv: u32 = 0;
        let mut j = i;
        while j < n {
            let c = buf[j];
            // stop at space, control chars, MARKER, high-bit bytes
            if c <= b' ' || c > 0x7E {
                break;
            }
            let a = glyphs[(c - FIRST_CHAR) as usize].advance as u32;
            if run_adv + a > remaining && j > word_start {
                // would overflow; stop batch here so we handle break properly
                break;
            }
            run_adv += a;
            j += 1;
        }

        if j > i {
            // consumed j - i bytes as a batch
            cursor_x += run_adv;
            i = j;
            if cursor_x > max_w {
                // overflow: break at last space or at word start
                if last_space > line_start {
                    emit!(line_start, last_space);
                    cursor_x -= cursor_at_space;
                    line_start = last_space;
                    line_flags = space_flags;
                } else {
                    emit!(line_start, word_start);
                    line_start = word_start;
                    line_flags = styles.flags();
                    // recompute cursor_x from line_start..i
                    cursor_x = 0;
                    for k in line_start..i {
                        let c = buf[k];
                        if c >= FIRST_CHAR && c <= 0x7E {
                            cursor_x += glyphs[(c - FIRST_CHAR) as usize].advance as u32;
                        }
                    }
                }
                last_space = line_start;
                space_flags = line_flags;
                cursor_at_space = 0;
                if line_count >= max_l {
                    return Ok((line_start, line_count));
                }
            }
            continue;
        }

        // single non-printable byte that wasn't caught above; skip
        i += 1;
    }

    if !at_eof && fonts.has_fallback() && cursor_x > max_w && legal_break <= line_start {
        // A trailing inseparable group may still continue beyond this read.
        // Retry it from its own offset when earlier lines made progress;
        // a group filling the entire bounded window has no safe continuation.
        if line_start > 0 {
            return Ok((line_start, line_count));
        }
        return Err(
            crate::error::Error::from_kind(crate::error::ErrorKind::BufferTooSmall)
                .with_source("font layout"),
        );
    }
    if line_start < n && line_count < max_l {
        let e = trim_trailing_cr(buf, line_start, n);
        if e > line_start {
            lines[line_count] = LineSpan {
                start: line_start as u16,
                len: (e - line_start) as u16,
                flags: if line_start_flags {
                    line_flags
                } else {
                    styles.flags()
                },
                indent,
            };
            line_count += 1;
        }
    }

    Ok((n, line_count))
}

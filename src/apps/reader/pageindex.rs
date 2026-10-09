// persistent page index of an EPUB chapter
//
// laying out a whole chapter is what opening it costs. the page offsets, style
// flags and indents of a completely indexed chapter are kept on the card in the
// book's cache directory (_PULP/_XXXXXXX/PGnnn.IDX) and read back instead of
// being recomputed. the record format and every validation rule are in
// pulp_board_logic::page_index (host tested); this file only gathers the key of
// the layout wanted now, moves bytes, and falls back to regeneration for
// anything that is not a valid record of exactly that layout

use pulp_board_logic::page_index::{
    BANK_BODY_INSTALLED, BANK_BODY_USED, BANK_HEADING_INSTALLED, BANK_HEADING_USED, ENTRY_BYTES,
    FOOTER_BYTES, HEADER_BYTES, LayoutKey, Reader, Reject, Writer, encode_header, header_banks,
    parse_header, record_len, record_name,
};

use crate::fonts;
use crate::kernel::KernelHandle;
use smol_epub::cache;

use super::ReaderApp;

// entries moved per storage call
const RUN: usize = 128;

impl ReaderApp {
    // everything the layout of the current chapter depends on, as it is now:
    // `used` says which fallback packs the layout used (`BANK_*_USED`): only
    // those packs are consulted, and their identities are read from the card,
    // not remembered, so a pack that was replaced, added or removed since a
    // record was written shows. a chapter that used none (Latin text) never
    // touches a pack here
    fn page_index_key(
        &self,
        k: &mut KernelHandle<'_>,
        used: u8,
    ) -> crate::error::Result<LayoutKey> {
        let idx = usize::from(self.book_font_size_idx.min(4));
        let body_px = fonts::cjk::BODY_PIXELS[idx];
        let heading_px = fonts::cjk::HEADING_PIXELS[idx];
        let mut banks = used & (BANK_BODY_USED | BANK_HEADING_USED);
        let (mut body_font, mut heading_font) = (0, 0);
        if used & BANK_BODY_USED != 0 {
            let id = fonts::cjk::bank_identity(k, body_px)?;
            body_font = id.font_id;
            if id.installed {
                banks |= BANK_BODY_INSTALLED;
            }
        }
        if used & BANK_HEADING_USED != 0 {
            let id = fonts::cjk::bank_identity(k, heading_px)?;
            heading_font = id.font_id;
            if id.installed {
                banks |= BANK_HEADING_INSTALLED;
            }
        }
        // the built-in metrics; the monospace layout (no font set) has none
        let latin_sig = match self.fonts {
            Some(set) if fonts::font_data::HAS_REGULAR => set.signature(),
            _ => 0,
        };
        Ok(LayoutKey {
            source: self.epub.source,
            chapter: self.epub.chapter,
            text_size: self.file_size,
            layout_version: super::LAYOUT_VERSION,
            latin_sig,
            body_px,
            heading_px,
            banks,
            body_font,
            heading_font,
            text_w: self.text_w,
            text_area_h: self.text_area_h,
            line_h: self.font_line_h,
            max_lines: self.max_lines,
        })
    }

    // the small profiles (X4, HR2, degraded) never touch the stored indexes
    fn page_index_usable(&self) -> bool {
        self.epub.profile.persist_index
            && self.is_epub
            && self.epub.chapters_cached
            && !self.epub.source.is_none()
            && self.file_size > 0
            && self.pg.capacity() > 0
    }

    // fill the page table from the stored record of this chapter. true: the
    // table holds the complete index (`fully_indexed`). false: no record, or
    // not a valid one for this layout; the table is back to its empty state and
    // the chapter is laid out as usual
    pub(super) fn load_page_index(&mut self, k: &mut KernelHandle<'_>) -> bool {
        if !self.page_index_usable() || self.pg.ensure_tables().is_err() {
            return false;
        }
        let dir = self.epub.cache_dir;
        let dir = cache::dir_name_str(&dir);
        let name = record_name(self.epub.chapter);
        let name = core::str::from_utf8(&name).unwrap_or("PG000.IDX");
        let capacity = self.pg.capacity();

        // which packs the stored layout used decides which identities to look
        // up; a missing or unreadable header is a miss
        let mut header = [0u8; HEADER_BYTES];
        let used = match k.read_app_subdir_chunk(dir, name, 0, &mut header) {
            Ok(n) if n >= HEADER_BYTES => match header_banks(&header) {
                Ok(banks) => banks,
                Err(why) => {
                    self.pg.index_counts.rejected = self.pg.index_counts.rejected.saturating_add(1);
                    log::info!(
                        "page index: ch{} header rejected: {:?}",
                        self.epub.chapter,
                        why
                    );
                    return false;
                }
            },
            _ => return false,
        };
        let Ok(want) = self.page_index_key(k, used) else {
            return false;
        };

        let pg = &mut self.pg;
        let loaded = k.with_app_subdir_file(dir, name, |file| {
            let Some(file) = file else {
                return Ok(None);
            };
            let len = u64::from(file.len()?);
            Ok(Some(read_record(
                file,
                &want,
                len,
                capacity,
                &mut |i, offset, style, indent| {
                    pg.offsets[i] = offset;
                    pg.style_flags[i] = style;
                    pg.indents[i] = indent;
                },
            )))
        });

        let count = match loaded {
            Ok(Some(Ok(count))) => count,
            outcome => {
                match outcome {
                    Ok(Some(Err(why))) => {
                        self.pg.index_counts.rejected =
                            self.pg.index_counts.rejected.saturating_add(1);
                        log::info!("page index: ch{} rejected: {:?}", self.epub.chapter, why)
                    }
                    Err(e) => log::info!("page index: ch{} unreadable: {}", self.epub.chapter, e),
                    _ => {}
                }
                // entries may have been filled before the record failed
                self.pg.offsets.fill(0);
                self.pg.style_flags.fill(0);
                self.pg.indents.fill(0);
                self.pg.total_pages = 1;
                self.pg.fully_indexed = false;
                self.pg.truncated = false;
                return false;
            }
        };
        self.pg.total_pages = count;
        self.pg.fully_indexed = true;
        self.pg.truncated = false;
        self.pg.page = 0;
        // The validated record describes the entire chapter, including banks
        // absent from its first visible/prefetched windows. Keep that identity
        // so resume checks replacements before using any stored page offsets.
        self.pg.banks_used = want.banks & (BANK_BODY_USED | BANK_HEADING_USED);
        self.layout_identity = Some(super::LayoutIdentity {
            body_px: want.body_px,
            heading_px: want.heading_px,
            body: (want.banks & BANK_BODY_USED != 0).then_some(fonts::cjk::BankIdentity {
                pixel_size: want.body_px,
                font_id: want.body_font,
                installed: want.banks & BANK_BODY_INSTALLED != 0,
            }),
            heading: (want.banks & BANK_HEADING_USED != 0).then_some(fonts::cjk::BankIdentity {
                pixel_size: want.heading_px,
                font_id: want.heading_font,
                installed: want.banks & BANK_HEADING_INSTALLED != 0,
            }),
            version: want.layout_version,
            text_w: want.text_w,
            text_area_h: want.text_area_h,
            line_h: want.line_h,
            max_lines: want.max_lines,
        });
        self.pg.index_counts.loaded = self.pg.index_counts.loaded.saturating_add(1);
        log::info!(
            "page index: ch{} loaded, {} pages",
            self.epub.chapter,
            count
        );
        true
    }

    // store the index of the current chapter if, and only if, it is complete:
    // the text ended. a table that filled up first is never written (it would
    // describe a prefix as if it were the chapter). the record is committed by
    // its footer, written last; a write that fails part-way leaves a file the
    // validation rejects, which is removed on a best-effort basis
    pub(super) fn store_page_index(&mut self, k: &mut KernelHandle<'_>) {
        if !self.page_index_usable()
            || !self.pg.fully_indexed
            || self.pg.truncated
            || self.pg.total_pages == 0
        {
            return;
        }
        let Ok(key) = self.page_index_key(k, self.pg.banks_used) else {
            return;
        };
        let count = self.pg.total_pages;
        let dir = self.epub.cache_dir;
        let dir = cache::dir_name_str(&dir);
        let name = record_name(self.epub.chapter);
        let name = core::str::from_utf8(&name).unwrap_or("PG000.IDX");

        let result = (|| -> crate::error::Result<()> {
            k.write_app_subdir(dir, name, &encode_header(&key, count))?;
            let mut writer = Writer::new();
            let mut run = [0u8; RUN * ENTRY_BYTES];
            let mut page = 0;
            while page < count {
                let n = (count - page).min(RUN);
                for i in 0..n {
                    writer.push(
                        &mut run[i * ENTRY_BYTES..],
                        self.pg.offsets[page + i],
                        self.pg.style_flags[page + i],
                        self.pg.indents[page + i],
                    );
                }
                k.append_app_subdir(dir, name, &run[..n * ENTRY_BYTES])?;
                page += n;
            }
            let footer = writer.footer(count).ok_or(crate::error::Error::from_kind(
                crate::error::ErrorKind::InvalidData,
            ))?;
            k.append_app_subdir(dir, name, &footer)
        })();
        match result {
            Ok(()) => {
                self.pg.index_counts.stored = self.pg.index_counts.stored.saturating_add(1);
                log::info!(
                    "page index: ch{} stored, {} pages",
                    self.epub.chapter,
                    count
                )
            }
            Err(e) => {
                log::warn!("page index: ch{} not stored: {}", self.epub.chapter, e);
                let _ = k.delete_app_subdir(dir, name);
            }
        }
    }
}

fn read_exact(
    file: &mut crate::drivers::storage::SubdirFile<'_>,
    mut at: u32,
    buf: &mut [u8],
) -> Result<(), Reject> {
    let mut done = 0;
    while done < buf.len() {
        match file.read_at(at, &mut buf[done..]) {
            Ok(n) if n > 0 => {
                done += n;
                at += n as u32;
            }
            // a record shorter than its header says, or a failing card
            _ => return Err(Reject::Truncated),
        }
    }
    Ok(())
}

// validate and hand over a record: header against the wanted key and the file
// length, entries as they stream, footer last. Entries already handed to `sink`
// are void when this returns an error
fn read_record(
    file: &mut crate::drivers::storage::SubdirFile<'_>,
    want: &LayoutKey,
    len: u64,
    capacity: usize,
    sink: &mut impl FnMut(usize, u32, u8, u8),
) -> Result<usize, Reject> {
    let mut header = [0u8; HEADER_BYTES];
    read_exact(file, 0, &mut header)?;
    let count = parse_header(&header, want, len, capacity)?;
    debug_assert_eq!(record_len(count), Some(len as usize));

    let mut reader = Reader::new(count, want.text_size);
    let mut run = [0u8; RUN * ENTRY_BYTES];
    let mut page = 0;
    while page < count {
        let n = (count - page).min(RUN);
        let bytes = &mut run[..n * ENTRY_BYTES];
        read_exact(file, (HEADER_BYTES + page * ENTRY_BYTES) as u32, bytes)?;
        reader.feed(bytes, sink)?;
        page += n;
    }
    let mut footer = [0u8; FOOTER_BYTES];
    read_exact(
        file,
        (HEADER_BYTES + count * ENTRY_BYTES) as u32,
        &mut footer,
    )?;
    reader.finish(&footer)?;
    Ok(count)
}

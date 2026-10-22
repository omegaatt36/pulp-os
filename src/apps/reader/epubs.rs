// epub init, chapter cache pipeline, and background cache state machine
//
// pure epub-state methods live on impl EpubState (init_zip,
// check_cache, finish_cache, cache_chapter_async, try_cache_chapter).
// methods that also touch PageState or ReaderApp fields stay on
// impl ReaderApp (epub_init_opf, epub_index_chapter, bg_cache_step).

use core::cell::RefCell;
use core::future::Future;

use embassy_time::{Duration, with_timeout};

use smol_epub::cache;
use smol_epub::epub;

use crate::error::{Error, ErrorKind};
use crate::kernel::KernelHandle;
use crate::kernel::work_queue;
use crate::kernel::{BigBuf, BufClass};
use pulp_board_logic::source_id::SourceId;
use pulp_kernel::util::utf8_prefix_len;

use super::{BgCacheState, EOCD_TAIL, EpubState, PAGE_BUF, ReaderApp, WARM_CHUNK, Warm};

// one cell shared between reader and writer; safe because
// stream_strip_entry_async never borrows both simultaneously
struct CellReader<'a, 'k>(&'a RefCell<&'a mut KernelHandle<'k>>, &'a str);
// CellWriter appends to a flat cache file in _PULP/ (v3 format)
struct CellWriter<'a, 'k> {
    kernel: &'a RefCell<&'a mut KernelHandle<'k>>,
    name: &'a str,
    size: u32,
}

// Includes the v3 header and chapter table, as well as chapter text.
const MAX_TEXT_CACHE_SIZE: u32 = 64 * 1024 * 1024;
const CHAPTER_STREAM_TIMEOUT: Duration = Duration::from_secs(120);

fn cache_append_end(size: u32, bytes: usize) -> Result<u32, &'static str> {
    let end = size
        .checked_add(u32::try_from(bytes).map_err(|_| "cache: byte count overflow")?)
        .ok_or("cache: byte count overflow")?;
    if end > MAX_TEXT_CACHE_SIZE {
        return Err("cache: book text cache too large");
    }
    Ok(end)
}

fn validate_text_cache(
    file_size: u32,
    data_offset: u32,
    chapters: &[(u32, u32)],
) -> Result<(), &'static str> {
    cache_append_end(file_size, 0)?;
    if file_size < data_offset {
        return Err("cache: chapter table truncated");
    }
    for (i, &(offset, size)) in chapters.iter().enumerate() {
        if size > cache::MAX_STRIPPED_CHAPTER_SIZE || offset < data_offset {
            return Err("cache: invalid chapter range");
        }
        let end = cache_append_end(offset, size as usize)?;
        if end > file_size {
            return Err("cache: chapter range out of bounds");
        }
        // Completed chapters cannot share text ranges. Zero-length chapters
        // occupy no bytes and may have the same offset as their neighbor.
        for &(other_offset, other_size) in &chapters[..i] {
            let other_end = cache_append_end(other_offset, other_size as usize)?;
            if size != 0 && other_size != 0 && offset < other_end && other_offset < end {
                return Err("cache: overlapping chapter ranges");
            }
        }
    }
    Ok(())
}

async fn chapter_with_deadline<F>(limit: Duration, stream: F) -> Result<u32, &'static str>
where
    F: Future<Output = Result<u32, &'static str>>,
{
    with_timeout(limit, stream)
        .await
        .map_err(|_| "cache: chapter streaming timed out")?
}

impl smol_epub::async_io::AsyncReadAt for CellReader<'_, '_> {
    async fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.0
            .borrow_mut()
            .read_chunk(self.1, offset, buf)
            .map_err(|e: Error| -> &'static str { e.into() })
    }
}

impl smol_epub::async_io::AsyncWriteChunk for CellWriter<'_, '_> {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), &'static str> {
        let next = cache_append_end(self.size, data.len())?;
        self.kernel
            .borrow_mut()
            .append_cache(self.name, data)
            .map_err(|e: Error| -> &'static str { e.into() })?;
        self.size = next;
        Ok(())
    }
}

impl EpubState {
    pub(super) fn init_zip(
        &mut self,
        k: &mut KernelHandle<'_>,
        name: &str,
        scratch: &mut [u8],
    ) -> crate::error::Result<()> {
        self.zip.clear();
        let epub_size = k.file_size(name)?;
        if epub_size < 22 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "epub_init_zip: too small",
            ));
        }
        self.archive_size = epub_size;
        self.name_hash = cache::fnv1a(name.as_bytes());
        self.cache_file = cache::cache_filename(self.name_hash);
        // until the central directory is hashed the identity is unknown and
        // nothing may be keyed by it
        self.source = SourceId::NONE;
        self.cache_started = false;
        self.ring.clear();
        self.images.clear();
        self.warm = None;

        let tail_size = (epub_size as usize).min(EOCD_TAIL);
        let tail_offset = epub_size - tail_size as u32;
        let n = k.read_chunk(name, tail_offset, &mut scratch[..tail_size])?;
        let (cd_offset, cd_size) =
            smol_epub::zip::ZipIndex::parse_eocd(&scratch[..n], epub_size)
                .map_err(|_| Error::new(ErrorKind::ParseFailed, "epub_init_zip: EOCD"))?;

        log::info!(
            "epub: CD at offset {} size {} ({} file bytes)",
            cd_offset,
            cd_size,
            epub_size
        );

        let mut cd_buf = BigBuf::zeroed(BufClass::ZipToc, cd_size as usize)
            .map_err(|_| Error::new(ErrorKind::OutOfMemory, "epub_init_zip: CD alloc"))?;
        super::read_full(k, name, cd_offset, &mut cd_buf)?;
        self.zip = super::metadata::new_zip();
        self.zip
            .parse_central_directory(&cd_buf)
            .map_err(|_| Error::new(ErrorKind::ParseFailed, "epub_init_zip: CD parse"))?;
        // the text cache file is named after the file name, so a replacement
        // book of the same name and size would find it. What identifies the
        // content is the central directory (names, CRC-32s, sizes): hash those
        // bytes. the identity validates the cache header, names the directory
        // of the book's images and page indexes, and keys the working set
        self.source = SourceId::from_archive(epub_size, cd_offset, &cd_buf);
        self.cache_dir = cache::dir_name_for_hash(self.source.dir_hash());
        self.images.retarget(self.source);
        drop(cd_buf);

        log::info!("epub: {} entries in ZIP", self.zip.count());

        Ok(())
    }

    pub(super) fn check_cache(
        &mut self,
        k: &mut KernelHandle<'_>,
        scratch: &mut [u8],
    ) -> crate::error::Result<bool> {
        // Every check must establish validity afresh, including when called
        // again after a cache hit on the same state.
        self.chapters_cached = false;
        self.cache_started = false;
        self.ch_cached.fill(false);
        self.chapter_table.fill((0, 0));
        if self.spine.len() > self.chapter_table.len() {
            return Err(Error::from("cache: too many chapters"));
        }
        let cf = self.cache_file;
        let cf_str = cache::cache_filename_str(&cf);

        let file_size = k.cache_file_size(cf_str).ok();
        // try reading v3 header
        let hdr_cap = cache::HEADER_SIZE.min(scratch.len());
        if let Ok(n) = k.read_cache_chunk(cf_str, 0, &mut scratch[..hdr_cap])
            && n >= cache::HEADER_SIZE
        {
            let hdr_buf: &[u8; cache::HEADER_SIZE] = scratch
                .first_chunk()
                .expect("scratch holds at least a header");
            if let Ok(hdr) = cache::parse_v3_header(hdr_buf)
                && cache::validate_v3_header(
                    &hdr,
                    self.archive_size,
                    self.name_hash,
                    self.spine.len(),
                )
                .is_ok()
                && hdr.chapter_count as usize <= self.chapter_table.len()
                && cache::validate_v3_source(&hdr, self.source.raw()).is_ok()
                && hdr.chapters_complete()
            {
                // read chapter table
                let count = hdr.chapter_count as usize;
                let tbl_bytes = count * cache::CHAPTER_ENTRY_SIZE;
                let tbl_offset = hdr.table_offset();
                if tbl_bytes <= scratch.len()
                    && let Ok(tn) =
                        k.read_cache_chunk(cf_str, tbl_offset, &mut scratch[..tbl_bytes])
                    && tn >= tbl_bytes
                    && cache::parse_chapter_table(
                        &scratch[..tbl_bytes],
                        count,
                        &mut self.chapter_table,
                    )
                    .is_ok()
                    && file_size.is_some_and(|size| {
                        validate_text_cache(size, hdr.data_offset(), &self.chapter_table[..count])
                            .is_ok()
                    })
                {
                    self.chapters_cached = true;
                    self.cache_started = true;
                    for i in 0..count {
                        self.ch_cached[i] = true;
                    }
                    // ensure image subdir exists for skip markers
                    let dir_buf = self.cache_dir;
                    let dir = cache::dir_name_str(&dir_buf);
                    let _ = k.ensure_app_subdir(dir);
                    log::info!("epub: v3 cache hit ({} chapters)", count);
                    return Ok(true);
                }
            }
        }

        // A parsed table may have failed validation; never retain its ranges.
        self.chapter_table.fill((0, 0));
        self.chunks.clear();
        log::info!("epub: building v3 cache for {} chapters", self.spine.len());
        // ensure image subdir exists (images stay in _PULP/_XXXXXXX/)
        let dir_buf = self.cache_dir;
        let dir = cache::dir_name_str(&dir_buf);
        k.ensure_app_subdir(dir)?;
        self.cache_chapter = 0;
        self.cache_started = false;
        Ok(false)
    }

    pub(super) fn finish_cache(
        &mut self,
        k: &mut KernelHandle<'_>,
        title: &[u8],
        filename: &[u8],
    ) -> crate::error::Result<bool> {
        let cf = self.cache_file;
        let cf_str = cache::cache_filename_str(&cf);
        let spine_len = self.spine.len();

        if spine_len > self.chapter_table.len() || spine_len > self.ch_cached.len() {
            return Err(Error::from("cache: too many chapters"));
        }
        if !self.cache_started || !self.ch_cached[..spine_len].iter().all(|&cached| cached) {
            return Err(Error::from("cache: chapters incomplete"));
        }
        let data_offset =
            cache::HEADER_SIZE as u32 + (spine_len * cache::CHAPTER_ENTRY_SIZE) as u32;
        validate_text_cache(
            k.cache_file_size(cf_str)?,
            data_offset,
            &self.chapter_table[..spine_len],
        )
        .map_err(Error::from)?;

        // build v3 header with chapters_complete flag
        let mut hdr = cache::CacheHeader::empty();
        hdr.version = cache::CACHE_FORMAT;
        hdr.chapter_count = spine_len as u16;
        hdr.flags = cache::FLAG_CHAPTERS_COMPLETE;
        hdr.epub_size = self.archive_size;
        hdr.name_hash = self.name_hash;
        hdr.source_id = self.source.raw();

        let tlen = utf8_prefix_len(title, cache::TITLE_CAP);
        hdr.title[..tlen].copy_from_slice(&title[..tlen]);
        hdr.title_len = tlen as u8;
        let nlen = filename.len().min(cache::NAME_CAP);
        hdr.name[..nlen].copy_from_slice(&filename[..nlen]);
        hdr.name_len = nlen as u8;

        let mut hdr_buf = [0u8; cache::HEADER_SIZE];
        cache::encode_v3_header(&hdr, &mut hdr_buf);
        // Write the table before publishing the completion flag.
        // write chapter table
        let tbl_size = spine_len * cache::CHAPTER_ENTRY_SIZE;
        // encode in chunks to avoid large stack buffers
        let mut tbl_buf = [0u8; 8]; // one entry at a time
        for i in 0..spine_len {
            cache::encode_chapter_table(&self.chapter_table[i..i + 1], &mut tbl_buf);
            let offset = cache::HEADER_SIZE as u32 + (i * cache::CHAPTER_ENTRY_SIZE) as u32;
            k.write_cache_at(cf_str, offset, &tbl_buf[..cache::CHAPTER_ENTRY_SIZE])?;
        }
        let _ = tbl_size; // used for clarity above
        k.write_cache_at(cf_str, 0, &hdr_buf)?;

        self.chapters_cached = true;
        log::info!("epub: v3 cache complete ({} chapters)", spine_len);
        Ok(false)
    }

    // async streaming chapter cache: decompress, strip HTML, append to v3 flat file.
    // tracks offset/size in chapter_table for random access reads.
    pub(super) async fn cache_chapter_async(
        &mut self,
        k: &mut KernelHandle<'_>,
        ch: usize,
        epub_name: &str,
    ) -> crate::error::Result<()> {
        self.cache_chapter_with_deadline(k, ch, epub_name, CHAPTER_STREAM_TIMEOUT)
            .await
    }

    async fn cache_chapter_with_deadline(
        &mut self,
        k: &mut KernelHandle<'_>,
        ch: usize,
        epub_name: &str,
        limit: Duration,
    ) -> crate::error::Result<()> {
        if self.spine.len() > self.chapter_table.len() || self.spine.len() > self.ch_cached.len() {
            return Err(Error::from("cache: too many chapters"));
        }
        if ch >= self.spine.len() || self.ch_cached[ch] {
            return Ok(());
        }

        let cf = self.cache_file;
        let cf_str = cache::cache_filename_str(&cf);
        let entry_idx = self.spine.items[ch] as usize;
        let entry = *self.zip.entry(entry_idx);

        // the first chapter written in this session (not necessarily chapter 0:
        // a restored position caches its own chapter first) creates the file
        // with a placeholder header + empty chapter table so appends start
        // at the correct data offset, and nothing of an older or foreign file
        // survives
        if !self.cache_started {
            let spine_len = self.spine.len();
            let mut init_buf = [0u8; cache::HEADER_SIZE];
            // write a minimal header (will be overwritten by finish_cache)
            let mut hdr = cache::CacheHeader::empty();
            hdr.version = cache::CACHE_FORMAT;
            hdr.chapter_count = spine_len as u16;
            hdr.epub_size = self.archive_size;
            hdr.name_hash = self.name_hash;
            hdr.source_id = self.source.raw();
            cache::encode_v3_header(&hdr, &mut init_buf);
            let mut cache_size = cache_append_end(0, init_buf.len()).map_err(Error::from)?;
            k.write_cache(cf_str, &init_buf)?;
            // pad with zeroes for the chapter table
            let tbl_size = spine_len * cache::CHAPTER_ENTRY_SIZE;
            let zeros = [0u8; 64];
            let mut remaining = tbl_size;
            while remaining > 0 {
                let chunk = remaining.min(zeros.len());
                let next = cache_append_end(cache_size, chunk).map_err(Error::from)?;
                k.append_cache(cf_str, &zeros[..chunk])?;
                cache_size = next;
                remaining -= chunk;
            }
            self.cache_started = true;
        }

        // record the offset where this chapter's data starts
        let ch_offset = k.cache_file_size(cf_str)?;
        cache_append_end(ch_offset, 0).map_err(Error::from)?;

        let k_cell = RefCell::new(&mut *k);

        let mut reader = CellReader(&k_cell, epub_name);
        let mut writer = CellWriter {
            kernel: &k_cell,
            name: cf_str,
            size: ch_offset,
        };

        let text_size = chapter_with_deadline(
            limit,
            smol_epub::async_io::stream_strip_entry_async_with_scratch(
                &entry,
                entry.local_offset,
                &mut reader,
                &mut writer,
                |layout| super::allocate_decoder_scratch(BufClass::ChapterText, layout),
            ),
        )
        .await
        .map_err(|msg| Error::from(msg).with_source("cache_chapter_async: stream"))?;

        self.chapter_table[ch] = (ch_offset, text_size);
        self.ch_cached[ch] = true;

        log::info!(
            "epub: cached ch{}/{} = {} bytes at offset {}",
            ch,
            self.spine.len(),
            text_size,
            ch_offset,
        );
        Ok(())
    }

    // make the current chapter's text resident. it is found in the ring by its
    // full key (so a warmed neighbor needs no read at all) or read from the
    // cache file into a fresh block; only a block that was read completely is
    // published. false: the chapter streams from the card (too large for a ring
    // slot, no memory, or a short or failed read)
    pub(super) fn try_cache_chapter(&mut self, k: &mut KernelHandle<'_>) -> bool {
        let Some(key) = self.chapter_key(self.chapter as usize) else {
            return false;
        };
        self.ring.retarget(self.source, self.chapter);

        if self.ring.contains(&key) {
            log::info!(
                "chapter cache: reusing ch{} ({} bytes)",
                key.chapter,
                key.size
            );
            return true;
        }
        if let Err(why) = self.ring.admit(&key) {
            log::info!(
                "chapter cache: ch{} ({} bytes) streams from the card: {:?}",
                key.chapter,
                key.size,
                why
            );
            return false;
        }

        let ch_size = key.size as usize;
        let mut buf = match BigBuf::zeroed(BufClass::ChapterText, ch_size) {
            Ok(b) => b,
            Err(_) => {
                log::info!("chapter cache: OOM for {} bytes", ch_size);
                return false;
            }
        };

        let cf = self.cache_file;
        let cf_str = cache::cache_filename_str(&cf);
        let mut pos = 0usize;
        while pos < ch_size {
            let chunk = (ch_size - pos).min(PAGE_BUF);
            match k.read_cache_chunk(cf_str, key.offset + pos as u32, &mut buf[pos..pos + chunk]) {
                Ok(n) if n > 0 => pos += n,
                // the cache file ends before the chapter does: not a chapter
                Ok(_) => {
                    log::info!("chapter cache: short read at {} of {}", pos, ch_size);
                    return false;
                }
                Err(e) => {
                    log::info!("chapter cache: SD read failed at {}: {}", pos, e);
                    return false;
                }
            }
        }

        match self.ring.insert(key, buf) {
            Ok(()) => {
                log::info!(
                    "chapter cache: loaded ch{} ({} bytes) into RAM",
                    key.chapter,
                    ch_size,
                );
                true
            }
            Err((why, _)) => {
                log::info!("chapter cache: ch{} not kept: {:?}", key.chapter, why);
                false
            }
        }
    }

    // one bounded step of reading a neighbor chapter into the ring: pick the
    // next candidate, then read WARM_CHUNK bytes per call. The block stays
    // private to the job until every byte arrived; a job whose window moved
    // (navigation, new book, memory pressure) is dropped, never published
    pub(super) fn warm_step(&mut self, k: &mut KernelHandle<'_>) {
        // `warm_idle` tells the scheduler it may stop calling this; it is
        // only true when a call ended with nothing in flight and nothing to start
        self.warm_idle = false;
        if self.warm_blocked || !self.chapters_cached || self.source.is_none() {
            self.warm_idle = true;
            return;
        }
        let Some(mut warm) = self.warm.take() else {
            let count = self.spine.len().min(cache::MAX_CACHE_CHAPTERS) as u16;
            let Some(job) = self
                .ring
                .next_warm(count, |ch| self.chapter_key(usize::from(ch)))
            else {
                self.warm_idle = true;
                return;
            };
            match BigBuf::zeroed(BufClass::ChapterText, job.key().size as usize) {
                Ok(buf) => self.warm = Some(Warm { job, buf }),
                Err(_) => {
                    log::info!("warm: no memory for ch{}", job.key().chapter);
                    self.warm_blocked = true;
                    self.warm_idle = true;
                }
            }
            return;
        };

        if warm.job.generation() != self.ring.generation() {
            log::info!("warm: ch{} dropped, window moved", warm.job.key().chapter);
            return;
        }
        let Some((at, len)) = warm.job.next_read(WARM_CHUNK) else {
            return;
        };
        let cf = self.cache_file;
        let cf_str = cache::cache_filename_str(&cf);
        let base = warm.job.key().offset + at;
        let at = at as usize;
        let read = k.read_cache_chunk(cf_str, base, &mut warm.buf[at..at + len]);
        let done = match read {
            Ok(n) => warm.job.record(n, len),
            Err(_) => Err(pulp_board_logic::chapter_ring::WarmError::ShortRead),
        };
        match done {
            Ok(false) => self.warm = Some(warm),
            Ok(true) => {
                let chapter = warm.job.key().chapter;
                match self.ring.publish(&warm.job, warm.buf) {
                    Ok(()) => log::info!("warm: ch{} resident", chapter),
                    Err((why, _)) => log::info!("warm: ch{} not kept: {:?}", chapter, why),
                }
            }
            Err(e) => {
                log::info!("warm: ch{} read failed: {:?}", warm.job.key().chapter, e);
                self.warm_blocked = true;
                self.warm_idle = true;
            }
        }
    }

    #[inline]
    pub(super) fn current_chapter_size(&self) -> u32 {
        self.chapter_size(self.chapter as usize)
    }
}

impl ReaderApp {
    pub(super) fn epub_init_opf(&mut self, k: &mut KernelHandle<'_>) -> crate::error::Result<()> {
        let (nb, nl) = self.name_copy();
        let name = core::str::from_utf8(&nb[..nl]).unwrap_or("");

        let mut opf_path_buf = [0u8; epub::OPF_PATH_CAP];
        let opf_path_len = if let Some(container_idx) = self.epub.zip.find("META-INF/container.xml")
        {
            let container_data = super::extract_zip_entry(k, name, &self.epub.zip, container_idx)
                .map_err(|_| {
                Error::new(ErrorKind::ReadFailed, "epub_init_opf: container read")
            })?;
            let len = epub::parse_container(&container_data, &mut opf_path_buf).map_err(|_| {
                Error::new(ErrorKind::ParseFailed, "epub_init_opf: container parse")
            })?;
            drop(container_data);
            len
        } else {
            log::warn!("epub: no container.xml, scanning for .opf");
            epub::find_opf_in_zip(&self.epub.zip, &mut opf_path_buf)
                .map_err(|_| Error::new(ErrorKind::NotFound, "epub_init_opf: no .opf in zip"))?
        };

        let opf_path = core::str::from_utf8(&opf_path_buf[..opf_path_len])
            .map_err(|_| Error::new(ErrorKind::BadEncoding, "epub_init_opf: OPF path"))?;

        log::info!("epub: OPF at {}", opf_path);

        let opf_idx = self
            .epub
            .zip
            .find(opf_path)
            .or_else(|| self.epub.zip.find_icase(opf_path))
            .ok_or(Error::new(ErrorKind::NotFound, "epub_init_opf: OPF entry"))?;
        let opf_data = super::extract_zip_entry(k, name, &self.epub.zip, opf_idx)
            .map_err(|_| Error::new(ErrorKind::ReadFailed, "epub_init_opf: OPF read"))?;

        let opf_dir = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        epub::parse_opf(
            &opf_data,
            opf_dir,
            &self.epub.zip,
            &mut self.epub.meta,
            &mut self.epub.spine,
        )
        .map_err(|_| Error::new(ErrorKind::ParseFailed, "epub_init_opf: OPF parse"))?;
        self.set_vertical(self.epub.meta.vertical);

        // defer TOC to NeedToc to avoid stack overflow while OPF is live
        self.epub.toc_source = epub::find_toc_source(&opf_data, opf_dir, &self.epub.zip);
        drop(opf_data);

        log::info!(
            "epub: \"{}\" by {} -- {} chapters",
            self.epub.meta.title_str(),
            self.epub.meta.author_str(),
            self.epub.spine.len()
        );

        let tlen = self.epub.meta.title_len as usize;
        if tlen > 0 {
            let n = tlen.min(self.title.len());
            self.title[..n].copy_from_slice(&self.epub.meta.title[..n]);
            self.title_len = n as u8;

            if let Err(e) = k.save_title(name, self.epub.meta.title_str()) {
                log::warn!("epub: failed to save title mapping: {}", e);
            }
        }

        self.epub.toc = None;

        Ok(())
    }

    pub(super) fn epub_index_chapter(&mut self) {
        self.reset_paging();
        // aim the working set at this chapter: the neighbors that are still
        // adjacent stay resident, the rest leave, and a neighbor read that was
        // pending for the old position can no longer publish
        self.epub.ring.retarget(self.epub.source, self.epub.chapter);
        self.epub.warm = None;
        self.epub.warm_blocked = false;
        self.file_size = self.epub.current_chapter_size();
        log::info!(
            "epub: index chapter {}/{} ({} bytes cached text)",
            self.epub.chapter + 1,
            self.epub.spine.len(),
            self.file_size,
        );
    }

    // run one step of background caching; async because CacheChapter
    // awaits cache_chapter_async which yields during deflate
    pub(super) async fn bg_cache_step(&mut self, k: &mut KernelHandle<'_>) {
        match self.epub.bg_cache {
            BgCacheState::CacheChapter => {
                let spine_len = self.epub.spine.len();

                // skip chapters already cached
                while (self.epub.cache_chapter as usize) < spine_len
                    && self.epub.ch_cached[self.epub.cache_chapter as usize]
                {
                    self.epub.cache_chapter += 1;
                }

                // priority: cache chapters adjacent to reading position
                // before continuing the sequential scan; forward/backward
                // nav stays instant
                let reading_ch = self.epub.chapter as usize;
                let (nb, nl) = self.name_copy();
                let name = core::str::from_utf8(&nb[..nl]).unwrap_or("");
                for &adj in &[reading_ch + 1, reading_ch.saturating_sub(1)] {
                    if adj < spine_len && adj != reading_ch && !self.epub.ch_cached[adj] {
                        log::info!(
                            "epub: priority cache ch{} (adjacent to ch{})",
                            adj,
                            reading_ch,
                        );
                        if let Err(e) = self.epub.cache_chapter_async(k, adj, name).await {
                            log::warn!("epub: priority ch{} failed: {}", adj, e);
                        }
                    }
                }

                let ch = self.epub.cache_chapter as usize;
                if ch >= spine_len {
                    let _ = self.epub.finish_cache(
                        k,
                        &self.title[..self.title_len as usize],
                        &self.filename[..self.filename_len],
                    );
                    self.epub.img_cache_ch = self.epub.chapter;
                    self.epub.img_cache_offset = 0;
                    self.epub.img_scan_wrapped = false;
                    self.epub.skip_large_img = false;
                    self.epub.img_found_count = 0;
                    self.epub.img_cached_count = 0;
                    self.epub.bg_cache = BgCacheState::CacheImage;
                    return;
                }

                match self.epub.cache_chapter_async(k, ch, name).await {
                    Ok(()) => {
                        self.epub.cache_chapter += 1;
                        // try nearby image dispatch before next chapter
                        if self.try_dispatch_nearby_image(k) {
                            self.epub.bg_cache = BgCacheState::WaitNearbyImage;
                        }
                        // else stay in CacheChapter
                    }
                    Err(e) => {
                        log::warn!("bg: ch{} failed: {}, skipping", ch, e);
                        self.epub.cache_chapter += 1;
                    }
                }
            }

            BgCacheState::WaitNearbyImage => {
                match self.epub_recv_image_result(k) {
                    Ok(Some(_)) => {
                        if self.try_dispatch_nearby_image(k) {
                            // stay in WaitNearbyImage
                        } else {
                            self.epub.bg_cache = BgCacheState::CacheChapter;
                        }
                    }
                    Ok(None) if work_queue::is_idle() => {
                        log::warn!("bg: worker idle with no result, recovering");
                        self.epub.bg_cache = BgCacheState::CacheChapter;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::warn!("bg: nearby image error: {}, continuing", e);
                        self.epub.bg_cache = BgCacheState::CacheChapter;
                    }
                }
            }
            BgCacheState::CacheImage => {
                match self.epub_find_and_dispatch_image(k) {
                    Ok(true) => {
                        // worker busy: dispatched a small image, wait
                        // worker idle: decoded inline, scan next tick
                        if !work_queue::is_idle() {
                            self.epub.bg_cache = BgCacheState::WaitImage;
                        }
                    }
                    Ok(false) => self.epub.bg_cache = BgCacheState::Idle,
                    Err(e) => {
                        log::warn!("bg: image error: {}, continuing", e);
                        // stay in CacheImage; next tick scans for the next one
                    }
                }
            }
            BgCacheState::WaitImage => match self.epub_recv_image_result(k) {
                Ok(Some(_)) => self.epub.bg_cache = BgCacheState::CacheImage,
                Ok(None) if work_queue::is_idle() => {
                    log::warn!("bg: worker idle with no result, recovering");
                    self.epub.bg_cache = BgCacheState::CacheImage;
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("bg: image recv error: {}", e);
                    self.epub.bg_cache = BgCacheState::CacheImage;
                }
            },
            BgCacheState::Idle => {}
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod limit_tests {
    use super::*;
    use crate::drivers::sdcard::SdStorage;
    use crate::kernel::Kernel;
    use crate::storage::{StorageOp, VirtualStorage};
    use smol_epub::async_io::AsyncWriteChunk;
    use smol_epub::zip::{METHOD_DEFLATE, METHOD_STORED};
    use std::task::{Context, Poll, Waker};

    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = f.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    fn state_and_kernel(method: u16, declared: u32) -> (EpubState, Kernel) {
        let input = vec![b'x'; 512 * 1024];
        let compressed = if method == METHOD_STORED {
            input
        } else {
            miniz_oxide::deflate::compress_to_vec(&input, 6)
        };
        let mut bytes = vec![0u8; 30];
        bytes[..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&compressed);
        let mut cd = [0u8; 47];
        cd[..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd[10..12].copy_from_slice(&method.to_le_bytes());
        cd[20..24].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
        cd[24..28].copy_from_slice(&declared.to_le_bytes());
        cd[28..30].copy_from_slice(&1u16.to_le_bytes());
        cd[46] = b'c';
        let mut state = EpubState::new();
        state.zip.parse_central_directory(&cd).unwrap();
        state.spine.count = 1;
        state.spine.items[0] = 0;
        state.archive_size = bytes.len() as u32;
        state.name_hash = cache::fnv1a(b"TEST.EPUB");
        state.cache_file = cache::cache_filename(state.name_hash);
        state.source = SourceId::from_archive(state.archive_size, 0, &cd);
        state.cache_dir = cache::dir_name_for_hash(state.source.dir_hash());
        let card = VirtualStorage::memory_with(&[("TEST.EPUB", &bytes)]);
        card.ensure_pulp_dir().unwrap();
        (state, Kernel::new(SdStorage::new(card)))
    }

    fn header(kernel: &mut Kernel, file: &str) -> cache::CacheHeader {
        let mut bytes = [0; cache::HEADER_SIZE];
        assert_eq!(
            kernel
                .handle()
                .read_cache_chunk(file, 0, &mut bytes)
                .unwrap(),
            bytes.len()
        );
        cache::parse_v3_header(&bytes).unwrap()
    }

    #[test]
    fn valid_empty_cached_chapter_reads_as_eof_in_both_profiles() {
        let mut kernel = Kernel::new(SdStorage::new(VirtualStorage::memory()));
        for large in [false, true] {
            let mut state = EpubState::new();
            state.spine.count = 1;
            state.chapters_cached = true;
            state.ch_cached[0] = true;
            if large {
                state.profile = super::super::metadata::MemProfile::for_status(
                    pulp_board_logic::memory::PsramStatus::Ready {
                        bytes: 8 * 1024 * 1024,
                    },
                );
            }
            let mut out = [0xa5; 8];
            assert_eq!(
                state.read_text(&mut kernel.handle(), 0, &mut out).unwrap(),
                0
            );
            assert_eq!(out, [0xa5; 8]);
            state.ch_cached[0] = false;
            assert!(state.read_text(&mut kernel.handle(), 0, &mut out).is_err());
        }
    }

    #[test]
    fn cell_writer_checks_cumulative_budget_before_storage_append() {
        let card = VirtualStorage::memory();
        card.ensure_pulp_dir().unwrap();
        card.write_in_pulp("LIMIT.BIN", b"").unwrap();
        let mut kernel = Kernel::new(SdStorage::new(card));
        let mut handle = kernel.handle();
        let cell = RefCell::new(&mut handle);
        let mut writer = CellWriter {
            kernel: &cell,
            name: "LIMIT.BIN",
            size: MAX_TEXT_CACHE_SIZE - 1,
        };
        block_on(writer.write_chunk(b"a")).unwrap();
        assert_eq!(writer.size, MAX_TEXT_CACHE_SIZE);
        // Leave a pending fault to prove a refused append never reached storage.
        kernel_fault(&cell, "LIMIT.BIN");
        assert_eq!(
            block_on(writer.write_chunk(b"b")),
            Err("cache: book text cache too large")
        );
        assert_eq!(cell.borrow_mut().cache_file_size("LIMIT.BIN").unwrap(), 1);
        writer.size = MAX_TEXT_CACHE_SIZE - 1;
        assert!(
            block_on(writer.write_chunk(b"b")).is_err(),
            "the unused storage fault must still fire"
        );
        assert_eq!(
            writer.size,
            MAX_TEXT_CACHE_SIZE - 1,
            "failed writes cannot advance accounting"
        );
        assert_eq!(cell.borrow_mut().cache_file_size("LIMIT.BIN").unwrap(), 1);
    }

    fn kernel_fault(cell: &RefCell<&mut KernelHandle<'_>>, file: &str) {
        cell.borrow().kernel.sd.card.inject_error(
            StorageOp::Append,
            &format!("_PULP/{file}"),
            1,
            ErrorKind::WriteFailed,
        );
    }

    #[test]
    fn book_budget_includes_header_table_and_all_previous_chapters() {
        let mut size =
            cache_append_end(0, cache::HEADER_SIZE + 8 * cache::CHAPTER_ENTRY_SIZE).unwrap();
        for _ in 0..7 {
            size = cache_append_end(size, cache::MAX_STRIPPED_CHAPTER_SIZE as usize).unwrap();
        }
        let remaining = MAX_TEXT_CACHE_SIZE - size;
        assert!(remaining < cache::MAX_STRIPPED_CHAPTER_SIZE);
        assert_eq!(
            cache_append_end(size, remaining as usize),
            Ok(MAX_TEXT_CACHE_SIZE)
        );
        assert_eq!(
            cache_append_end(size, remaining as usize + 1),
            Err("cache: book text cache too large")
        );
        assert!(cache_append_end(u32::MAX, 1).is_err());
    }

    #[test]
    fn failed_or_timed_out_chapters_cannot_be_published_complete() {
        for (method, declared, limit) in [
            (METHOD_DEFLATE, 1, CHAPTER_STREAM_TIMEOUT),
            (METHOD_STORED, 512 * 1024, Duration::from_ticks(0)),
        ] {
            let (mut state, mut kernel) = state_and_kernel(method, declared);
            let error = block_on(state.cache_chapter_with_deadline(
                &mut kernel.handle(),
                0,
                "TEST.EPUB",
                limit,
            ))
            .unwrap_err();
            if method == METHOD_STORED {
                assert_eq!(error.source_tag(), "cache_chapter_async: stream");
                assert!(
                    kernel
                        .handle()
                        .cache_file_size(state.cache_file_str())
                        .unwrap()
                        > 136,
                    "timeout interrupts after a partial append"
                );
            }
            assert!(!state.ch_cached[0]);
            assert_eq!(state.chapter_table[0], (0, 0));
            assert!(
                state
                    .finish_cache(&mut kernel.handle(), b"Test", b"TEST.EPUB")
                    .is_err()
            );
            assert!(!state.chapters_cached);
            assert!(!header(&mut kernel, state.cache_file_str()).chapters_complete());
        }
    }

    #[test]
    fn valid_large_chapter_completes_and_preexisting_invalid_ranges_are_rejected() {
        let (mut state, mut kernel) = state_and_kernel(METHOD_STORED, 512 * 1024);
        block_on(state.cache_chapter_async(&mut kernel.handle(), 0, "TEST.EPUB")).unwrap();
        assert!(state.ch_cached[0]);
        state
            .finish_cache(&mut kernel.handle(), b"Test", b"TEST.EPUB")
            .unwrap();
        assert!(header(&mut kernel, state.cache_file_str()).chapters_complete());
        let mut scratch = [0; 2048];
        assert!(
            state
                .check_cache(&mut kernel.handle(), &mut scratch)
                .unwrap()
        );
        let cf = state.cache_file;
        let file = cache::cache_filename_str(&cf);
        // Keep a valid completed header, but forge a range outside the file.
        let mut table = [0; cache::CHAPTER_ENTRY_SIZE];
        cache::encode_chapter_table(&[(u32::MAX, 1)], &mut table);
        kernel
            .handle()
            .write_cache_at(file, cache::HEADER_SIZE as u32, &table)
            .unwrap();
        // Reject immediately after the valid hit, with all cached state intact.
        assert!(state.chapters_cached && state.cache_started && state.ch_cached[0]);
        assert_ne!(state.chapter_table[0], (0, 0));
        assert!(
            !state
                .check_cache(&mut kernel.handle(), &mut scratch)
                .unwrap()
        );
        assert!(!state.chapters_cached);
        assert!(!state.cache_started);
        assert!(state.ch_cached.iter().all(|&cached| !cached));
        assert!(state.chapter_table.iter().all(|&range| range == (0, 0)));
    }

    #[test]
    fn preexisting_oversized_cache_is_not_a_hit() {
        let (mut state, mut kernel) = state_and_kernel(METHOD_STORED, 512 * 1024);
        block_on(state.cache_chapter_async(&mut kernel.handle(), 0, "TEST.EPUB")).unwrap();
        state
            .finish_cache(&mut kernel.handle(), b"Test", b"TEST.EPUB")
            .unwrap();
        let cf = state.cache_file;
        let file = cache::cache_filename_str(&cf);
        let mut valid_prefix = [0; cache::HEADER_SIZE + cache::CHAPTER_ENTRY_SIZE];
        kernel
            .handle()
            .read_cache_chunk(file, 0, &mut valid_prefix)
            .unwrap();
        // A sparse host file exercises the real file-size guard without
        // allocating 64 MiB of chapter text just to reject it.
        let root = std::env::temp_dir().join(format!("pulp-epub-limit-{}", std::process::id()));
        let pulp = root.join("_PULP");
        std::fs::create_dir_all(&pulp).unwrap();
        let cache_path = pulp.join(file);
        let mut output = std::fs::File::create(&cache_path).unwrap();
        std::io::Write::write_all(&mut output, &valid_prefix).unwrap();
        output.set_len(MAX_TEXT_CACHE_SIZE as u64 + 1).unwrap();
        drop(output);
        let card = VirtualStorage::host_dir(&root);
        let mut kernel = Kernel::new(SdStorage::new(card));
        let result = state
            .check_cache(&mut kernel.handle(), &mut [0; 2048])
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(!result);
        assert!(!state.chapters_cached);
        assert!(!state.cache_started);
        assert!(state.ch_cached.iter().all(|&cached| !cached));
        assert!(state.chapter_table.iter().all(|&range| range == (0, 0)));
    }

    #[test]
    fn short_positive_deadline_cancels_ready_stored_io() {
        struct ReadyReader {
            header: [u8; 30],
        }
        impl smol_epub::async_io::AsyncReadAt for ReadyReader {
            async fn read_at(
                &mut self,
                offset: u32,
                bytes: &mut [u8],
            ) -> Result<usize, &'static str> {
                // Synchronous work remains Ready, but consumes real elapsed time.
                std::thread::sleep(std::time::Duration::from_millis(1));
                if offset == 0 {
                    bytes.copy_from_slice(&self.header);
                } else {
                    bytes.fill(b'x');
                }
                Ok(bytes.len())
            }
        }
        struct Output(usize);
        impl AsyncWriteChunk for Output {
            async fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
                self.0 += bytes.len();
                Ok(())
            }
        }
        let mut reader = ReadyReader { header: [0; 30] };
        reader.header[..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
        let mut output = Output(0);
        let entry = smol_epub::zip::ZipEntry {
            comp_size: 128 * 1024,
            uncomp_size: 128 * 1024,
            method: METHOD_STORED,
            ..smol_epub::zip::ZipEntry::EMPTY
        };
        let result = block_on(chapter_with_deadline(
            Duration::from_millis(1),
            smol_epub::async_io::stream_strip_entry_async(&entry, 0, &mut reader, &mut output),
        ));
        assert_eq!(result, Err("cache: chapter streaming timed out"));
        assert!(output.0 > 0 && output.0 < entry.uncomp_size as usize);
    }

    #[test]
    fn unsupported_chapter_count_fails_normally_before_cache_slices() {
        let (mut state, mut kernel) = state_and_kernel(METHOD_STORED, 512 * 1024);
        state.spine.count = (cache::MAX_CACHE_CHAPTERS + 1) as u16;
        assert!(
            block_on(state.cache_chapter_async(&mut kernel.handle(), 256, "TEST.EPUB")).is_err()
        );
        assert!(state.finish_cache(&mut kernel.handle(), b"", b"").is_err());
        assert!(
            state
                .check_cache(&mut kernel.handle(), &mut [0; 2048])
                .is_err()
        );
    }
}

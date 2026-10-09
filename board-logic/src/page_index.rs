// Page-table sizing and the persistent page-index record of an EPUB chapter.
//
// HAL-free and allocation-free. Two independent pieces of pure policy live here
// so the host tests reach the exact code the firmware runs:
//
// 1. Capacity. The page table is dynamic: HR8 tries `HR8_PAGES`, everything
//    else (and an HR8 refusal) uses the `SMALL_PAGES` profile. Indexing then
//    ends in one of two ways that must never be confused: the text ended
//    (`PageStep::Done`, the index is complete) or the table filled up first
//    (`PageStep::Truncated`, pages exist beyond the table). `fully_indexed` is
//    only ever set for the first.
//
// 2. The record. Laying out a chapter is the expensive part of opening it, so
//    the offsets, style flags and indents of a completely indexed chapter are
//    stored on the card and reused. A record is trusted only if
//      - the header carries the whole `LayoutKey` of the layout that produced
//        it (book identity, chapter, text size, fonts, geometry, layout
//        version) and equals the key of the layout wanted now,
//      - the file has exactly the size the header's page count implies,
//      - header, payload and footer checksums hold,
//      - the footer (written last) commits the record,
//      - the entries are plausible (first page at 0, offsets increasing and
//        inside the text),
//      - the page count fits the table in use (a record bigger than the table
//        would be a capacity truncation, not a hit).
//    Anything else is a miss and the chapter is laid out again.

use crate::memory::{PSRAM_LARGE_MIN_BYTES, PsramStatus};
use crate::source_id::{Fnv64, SourceId};

// ---------------------------------------------------------------------------
// capacity

/// Pages of the small profile: X4, HR2, degraded builds and an HR8 whose
/// larger table was refused.
pub const SMALL_PAGES: usize = 512;
/// Pages of the HR8 profile (6 B per page: 24 KiB of the page-table class).
pub const HR8_PAGES: usize = 4096;

/// Table sizes to try for a PSRAM state, largest first; the last one is always
/// `SMALL_PAGES`.
pub const fn table_plan(status: PsramStatus) -> &'static [usize] {
    match status {
        PsramStatus::Ready { bytes } if bytes >= PSRAM_LARGE_MIN_BYTES => &[HR8_PAGES, SMALL_PAGES],
        _ => &[SMALL_PAGES],
    }
}

/// Size of the first table of `plan` that `alloc` accepts.
pub fn acquire_table<T, E>(
    status: PsramStatus,
    mut alloc: impl FnMut(usize) -> Result<T, E>,
) -> Result<(usize, T), E> {
    let plan = table_plan(status);
    let mut last = None;
    for &pages in plan {
        match alloc(pages) {
            Ok(v) => return Ok((pages, v)),
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("table plan is not empty"))
}

/// What the page that was just laid out means for the index.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PageStep {
    /// More text follows and the table has room: record `next_offset`.
    Append,
    /// The text ends here (or no progress is possible): the index is complete.
    Done,
    /// More text follows but the table is full: pages exist that the table
    /// cannot hold. NOT complete.
    Truncated,
}

/// Classify the end of the page that consumed `consumed` bytes of a text of
/// `text_len` bytes, ending at `next_offset`, with `total_pages` entries used of
/// `capacity`.
pub const fn next_page(
    next_offset: usize,
    text_len: usize,
    consumed: usize,
    total_pages: usize,
    capacity: usize,
) -> PageStep {
    if next_offset < text_len && consumed > 0 {
        if total_pages < capacity {
            PageStep::Append
        } else {
            PageStep::Truncated
        }
    } else {
        PageStep::Done
    }
}

// ---------------------------------------------------------------------------
// record

pub const FORMAT_VERSION: u16 = 1;
const MAGIC: [u8; 4] = *b"PGIX";
const FOOTER_MAGIC: [u8; 4] = *b"PGOK";
const FLAG_COMPLETE: u16 = 1;

/// Bytes of one page entry: offset (u32), style flags (u8), indent (u8).
pub const ENTRY_BYTES: usize = 6;
pub const KEY_BYTES: usize = 50;

/// `LayoutKey::banks` bits: the layout used the body / heading fallback pack
/// (bits 0, 1) and that pack was installed (bits 2, 3).
pub const BANK_BODY_USED: u8 = 1 << 0;
pub const BANK_HEADING_USED: u8 = 1 << 1;
pub const BANK_BODY_INSTALLED: u8 = 1 << 2;
pub const BANK_HEADING_INSTALLED: u8 = 1 << 3;
// byte of the key that holds `banks`: source 8, chapter 2, text size 4, layout
// version 2, latin signature 4, two pixel sizes 4, two font ids 16
const BANKS_AT_IN_KEY: usize = 8 + 2 + 4 + 2 + 4 + 2 + 2 + 8 + 8;
pub const HEADER_BYTES: usize = 4 + 2 + 2 + KEY_BYTES + 4 + 4;
pub const FOOTER_BYTES: usize = 8 + 4 + 4;

/// Everything the layout of a chapter depends on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayoutKey {
    /// Persistent identity of the book (central directory hash).
    pub source: SourceId,
    pub chapter: u16,
    /// Byte size of the chapter's stripped text.
    pub text_size: u32,
    /// Pagination algorithm version.
    pub layout_version: u16,
    /// Signature of the built-in font metrics used for the book font size.
    pub latin_sig: u32,
    pub body_px: u16,
    pub heading_px: u16,
    /// Fallback packs the chapter's layout used, see the `BANK_*` bits. A
    /// chapter of Latin text uses none, so its record does not depend on any
    /// pack and is valid without opening one.
    pub banks: u8,
    /// Font ids of the packs in use (0 for a bank not in use or not installed).
    pub body_font: u64,
    pub heading_font: u64,
    /// Geometry: text width, text area height, line height, lines per page.
    pub text_w: u32,
    pub text_area_h: u16,
    pub line_h: u16,
    pub max_lines: u8,
}

impl LayoutKey {
    fn encode(&self, out: &mut [u8; KEY_BYTES]) {
        let mut at = 0;
        let mut put = |bytes: &[u8]| {
            out[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(&self.source.raw().to_le_bytes());
        put(&self.chapter.to_le_bytes());
        put(&self.text_size.to_le_bytes());
        put(&self.layout_version.to_le_bytes());
        put(&self.latin_sig.to_le_bytes());
        put(&self.body_px.to_le_bytes());
        put(&self.heading_px.to_le_bytes());
        put(&self.body_font.to_le_bytes());
        put(&self.heading_font.to_le_bytes());
        put(&[self.banks]);
        put(&self.text_w.to_le_bytes());
        put(&self.text_area_h.to_le_bytes());
        put(&self.line_h.to_le_bytes());
        put(&[self.max_lines]);
        debug_assert_eq!(at, KEY_BYTES);
    }
}

/// File name of a chapter's record inside the book's cache directory:
/// `PG000.IDX` .. `PG999.IDX`.
pub fn record_name(chapter: u16) -> [u8; 9] {
    let mut n = *b"PG000.IDX";
    n[2] = b'0' + ((chapter / 100) % 10) as u8;
    n[3] = b'0' + ((chapter / 10) % 10) as u8;
    n[4] = b'0' + (chapter % 10) as u8;
    n
}

/// Exact size of a record of `count` pages.
pub const fn record_len(count: usize) -> Option<usize> {
    match count.checked_mul(ENTRY_BYTES) {
        Some(p) => match p.checked_add(HEADER_BYTES + FOOTER_BYTES) {
            Some(n) => Some(n),
            None => None,
        },
        None => None,
    }
}

fn fold32(h: u64) -> u32 {
    (h ^ (h >> 32)) as u32
}

fn header_checksum(bytes: &[u8]) -> u32 {
    let mut h = Fnv64::new();
    h.update(bytes);
    fold32(h.finish())
}

/// Header of a complete record of `count` pages (the page count must be known
/// before the first entry is written; records of a truncated index are never
/// written, so there is no incomplete form).
pub fn encode_header(key: &LayoutKey, count: usize) -> [u8; HEADER_BYTES] {
    let mut out = [0u8; HEADER_BYTES];
    out[0..4].copy_from_slice(&MAGIC);
    out[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&FLAG_COMPLETE.to_le_bytes());
    let mut k = [0u8; KEY_BYTES];
    key.encode(&mut k);
    out[8..8 + KEY_BYTES].copy_from_slice(&k);
    let at = 8 + KEY_BYTES;
    out[at..at + 4].copy_from_slice(&(count as u32).to_le_bytes());
    let sum = header_checksum(&out[..at + 4]);
    out[at + 4..at + 8].copy_from_slice(&sum.to_le_bytes());
    out
}

/// Entry encoder with the running payload checksum.
pub struct Writer {
    hash: Fnv64,
    count: usize,
}

impl Writer {
    pub const fn new() -> Self {
        Self {
            hash: Fnv64::new(),
            count: 0,
        }
    }

    /// Encode one entry into `out[..ENTRY_BYTES]`.
    pub fn push(&mut self, out: &mut [u8], offset: u32, style: u8, indent: u8) {
        out[0..4].copy_from_slice(&offset.to_le_bytes());
        out[4] = style;
        out[5] = indent;
        self.hash.update(&out[..ENTRY_BYTES]);
        self.count += 1;
    }

    pub const fn count(&self) -> usize {
        self.count
    }

    /// The commit footer, written last; `None` if `count` entries were not
    /// pushed.
    pub fn footer(&self, count: usize) -> Option<[u8; FOOTER_BYTES]> {
        if self.count != count {
            return None;
        }
        let total = record_len(count)?;
        let mut out = [0u8; FOOTER_BYTES];
        out[0..8].copy_from_slice(&self.hash.finish().to_le_bytes());
        out[8..12].copy_from_slice(&(total as u32).to_le_bytes());
        out[12..16].copy_from_slice(&FOOTER_MAGIC);
        Some(out)
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Reject {
    /// Shorter than a header, or a chunk that is not whole entries.
    Truncated,
    Magic,
    Version,
    HeaderChecksum,
    /// The record was not marked complete.
    NotComplete,
    /// Written for another layout (book, chapter, fonts, geometry, version).
    KeyMismatch,
    /// File size is not what the page count implies.
    Size,
    /// More pages than the table in use holds.
    Capacity,
    /// Missing commit footer or wrong footer fields.
    Footer,
    PayloadChecksum,
    /// First page not at 0, offsets not increasing, or beyond the text.
    Order,
}

/// The packs a stored record's layout used, read from an otherwise unchecked
/// header (magic, version and header checksum are verified, nothing else). The
/// caller builds the wanted key from this: it must look up the identity of
/// exactly the packs the record depended on, and of no other.
pub fn header_banks(hdr: &[u8]) -> Result<u8, Reject> {
    if hdr.len() < HEADER_BYTES {
        return Err(Reject::Truncated);
    }
    if hdr[0..4] != MAGIC {
        return Err(Reject::Magic);
    }
    let at = 8 + KEY_BYTES;
    let stored = u32::from_le_bytes([hdr[at + 4], hdr[at + 5], hdr[at + 6], hdr[at + 7]]);
    if stored != header_checksum(&hdr[..at + 4]) {
        return Err(Reject::HeaderChecksum);
    }
    if u16::from_le_bytes([hdr[4], hdr[5]]) != FORMAT_VERSION {
        return Err(Reject::Version);
    }
    Ok(hdr[8 + BANKS_AT_IN_KEY])
}

/// Validate a header against the wanted `key` and the file length; returns the
/// page count. `capacity` is the entry count of the table in use.
pub fn parse_header(
    hdr: &[u8],
    want: &LayoutKey,
    file_len: u64,
    capacity: usize,
) -> Result<usize, Reject> {
    if hdr.len() < HEADER_BYTES {
        return Err(Reject::Truncated);
    }
    if hdr[0..4] != MAGIC {
        return Err(Reject::Magic);
    }
    let at = 8 + KEY_BYTES;
    let stored = u32::from_le_bytes([hdr[at + 4], hdr[at + 5], hdr[at + 6], hdr[at + 7]]);
    if stored != header_checksum(&hdr[..at + 4]) {
        return Err(Reject::HeaderChecksum);
    }
    if u16::from_le_bytes([hdr[4], hdr[5]]) != FORMAT_VERSION {
        return Err(Reject::Version);
    }
    if u16::from_le_bytes([hdr[6], hdr[7]]) & FLAG_COMPLETE == 0 {
        return Err(Reject::NotComplete);
    }
    let mut k = [0u8; KEY_BYTES];
    want.encode(&mut k);
    if hdr[8..8 + KEY_BYTES] != k {
        return Err(Reject::KeyMismatch);
    }
    let count = u32::from_le_bytes([hdr[at], hdr[at + 1], hdr[at + 2], hdr[at + 3]]) as usize;
    if count == 0 {
        return Err(Reject::Size);
    }
    match record_len(count) {
        Some(n) if n as u64 == file_len => {}
        _ => return Err(Reject::Size),
    }
    if count > capacity {
        return Err(Reject::Capacity);
    }
    Ok(count)
}

/// Streaming validator of the payload and footer of a record whose header was
/// accepted. Entries are handed to `sink` as they are checked; a caller that
/// gets an error must discard what the sink received.
pub struct Reader {
    hash: Fnv64,
    count: usize,
    seen: usize,
    previous: Option<u32>,
    text_size: u32,
}

impl Reader {
    pub const fn new(count: usize, text_size: u32) -> Self {
        Self {
            hash: Fnv64::new(),
            count,
            seen: 0,
            previous: None,
            text_size,
        }
    }

    pub const fn seen(&self) -> usize {
        self.seen
    }

    /// Check a run of whole entries.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        sink: &mut impl FnMut(usize, u32, u8, u8),
    ) -> Result<(), Reject> {
        if bytes.len() % ENTRY_BYTES != 0 || self.seen + bytes.len() / ENTRY_BYTES > self.count {
            return Err(Reject::Truncated);
        }
        self.hash.update(bytes);
        for e in bytes.chunks_exact(ENTRY_BYTES) {
            let offset = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
            let ok = match self.previous {
                None => offset == 0,
                Some(p) => offset > p,
            };
            if !ok || (offset >= self.text_size && self.seen > 0) {
                return Err(Reject::Order);
            }
            self.previous = Some(offset);
            sink(self.seen, offset, e[4], e[5]);
            self.seen += 1;
        }
        Ok(())
    }

    /// Check the footer after every entry was fed.
    pub fn finish(self, footer: &[u8]) -> Result<(), Reject> {
        if self.seen != self.count {
            return Err(Reject::Truncated);
        }
        if footer.len() != FOOTER_BYTES || footer[12..16] != FOOTER_MAGIC {
            return Err(Reject::Footer);
        }
        let total = record_len(self.count).ok_or(Reject::Size)?;
        let stored_total = u32::from_le_bytes([footer[8], footer[9], footer[10], footer[11]]);
        if stored_total as usize != total {
            return Err(Reject::Footer);
        }
        let mut sum = [0u8; 8];
        sum.copy_from_slice(&footer[0..8]);
        if u64::from_le_bytes(sum) != self.hash.finish() {
            return Err(Reject::PayloadChecksum);
        }
        Ok(())
    }
}

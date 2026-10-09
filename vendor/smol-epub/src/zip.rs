//! ZIP central-directory parser and streaming entry extraction.
//!
//! [`ZipIndex`] defaults to 256 inline entries; callers may select their own
//! entry and name storage policy. DEFLATE decompression streams in 4 KB
//! chunks; `try_reserve` is used throughout for graceful OOM handling.

use alloc::vec;
use alloc::vec::Vec;

const MAX_ENTRY_SIZE: u32 = 192 * 1024; // max uncompressed entry size (OOM guard)

const EOCD_SIG: u32 = 0x0605_4b50;
const CD_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

/// ZIP compression method: stored (no compression).
pub const METHOD_STORED: u16 = 0;
/// ZIP compression method: DEFLATE.
pub const METHOD_DEFLATE: u16 = 8;

#[inline]
fn le_u16(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

#[inline]
fn le_u32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// A single entry in the ZIP central directory.
#[derive(Clone, Copy)]
pub struct ZipEntry {
    /// Byte offset into the name pool where this entry's name starts.
    pub name_start: u16,
    /// Length of the entry name in bytes.
    pub name_len: u16,
    /// Byte offset of the local file header in the ZIP file.
    pub local_offset: u32,
    /// Compressed size in bytes.
    pub comp_size: u32,
    /// Uncompressed size in bytes.
    pub uncomp_size: u32,
    /// Compression method ([`METHOD_STORED`] or [`METHOD_DEFLATE`]).
    pub method: u16,
}

impl ZipEntry {
    /// Empty initialized entry for caller-owned backing.
    pub const EMPTY: Self = Self {
        name_start: 0,
        name_len: 0,
        local_offset: 0,
        comp_size: 0,
        uncomp_size: 0,
        method: 0,
    };
}

/// Maximum number of entries the [`ZipIndex`] can hold.
pub const MAX_ENTRIES: usize = 256;

/// Storage policy for ZIP metadata. `prepare` must provide initialized slices
/// of the requested lengths, or return an error. Owners release all backing in
/// `clear`; they may choose a smaller runtime capacity before parsing.
pub trait ZipStorage {
    /// Maximum indexed entries for this policy.
    fn capacity(&self) -> usize;
    /// The default inline policy preserves the historical first-256 behavior.
    /// Strict policies refuse archives that exceed their capacity.
    fn truncate_entries(&self) -> bool {
        false
    }
    /// Allocate or initialize backing before entries become visible.
    fn prepare(&mut self, entries: usize, names: usize) -> Result<(), &'static str>;
    /// Initialized entry backing.
    fn entries(&self) -> &[ZipEntry];
    /// Exclusively borrowed initialized entry backing.
    fn entries_mut(&mut self) -> &mut [ZipEntry];
    /// Initialized name byte backing.
    fn names(&self) -> &[u8];
    /// Exclusively borrowed name byte backing.
    fn names_mut(&mut self) -> &mut [u8];
    /// Release dynamic backing and forget the previous source.
    fn clear(&mut self);
}

/// Host/X4 metadata: fixed entries and a fallibly allocated name pool.
pub struct InlineZipStorage {
    entries: [ZipEntry; MAX_ENTRIES],
    names: Vec<u8>,
}
impl ZipStorage for InlineZipStorage {
    fn capacity(&self) -> usize {
        MAX_ENTRIES
    }
    fn truncate_entries(&self) -> bool {
        true
    }
    fn prepare(&mut self, _: usize, names: usize) -> Result<(), &'static str> {
        self.names
            .try_reserve_exact(names)
            .map_err(|_| "zip: name allocation failed")?;
        self.names.resize(names, 0);
        Ok(())
    }
    fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }
    fn entries_mut(&mut self) -> &mut [ZipEntry] {
        &mut self.entries
    }
    fn names(&self) -> &[u8] {
        &self.names
    }
    fn names_mut(&mut self) -> &mut [u8] {
        &mut self.names
    }
    fn clear(&mut self) {
        self.names = Vec::new();
    }
}

/// In-memory index, with caller-selected backing for entries and names.
pub struct ZipIndex<S = InlineZipStorage> {
    storage: S,
    count: u16,
}
impl Default for ZipIndex {
    fn default() -> Self {
        Self::new()
    }
}
impl ZipIndex {
    /// Create an empty host/X4 index without allocating.
    pub const fn new() -> Self {
        Self::with_storage(InlineZipStorage {
            entries: [ZipEntry::EMPTY; MAX_ENTRIES],
            names: Vec::new(),
        })
    }
    /// Parse the End-of-Central-Directory record from the last bytes of a
    /// ZIP file. Returns `(cd_offset, cd_size)`.
    ///
    /// `tail` should be the final ≤ 65557 bytes of the file (22 bytes is
    /// the minimum for a ZIP with no comment).
    pub fn parse_eocd(tail: &[u8], file_size: u32) -> Result<(u32, u32), &'static str> {
        if tail.len() < 22 {
            return Err("zip: tail too short for EOCD");
        }

        let mut i = tail.len() - 22;
        loop {
            if le_u32(tail, i) == EOCD_SIG {
                break;
            }
            if i == 0 {
                return Err("zip: EOCD signature not found");
            }
            i -= 1;
        }

        let cd_size = le_u32(tail, i + 12);
        let cd_offset = le_u32(tail, i + 16);

        if cd_offset.saturating_add(cd_size) > file_size {
            return Err("zip: CD extends past EOF");
        }

        Ok((cd_offset, cd_size))
    }

    /// Given the first 30+ bytes of a local file header, return the number
    /// of bytes to skip past the header to reach the entry's data.
    pub fn local_header_data_skip(header: &[u8]) -> Result<u32, &'static str> {
        if header.len() < 30 {
            return Err("zip: local header too short");
        }
        if le_u32(header, 0) != LOCAL_SIG {
            return Err("zip: bad local header signature");
        }
        let name_len = le_u16(header, 26) as u32;
        let extra_len = le_u16(header, 28) as u32;
        Ok(30 + name_len + extra_len)
    }
}

impl<S: ZipStorage> ZipIndex<S> {
    /// Create an allocation-free index with a caller-selected policy.
    pub const fn with_storage(storage: S) -> Self {
        Self { storage, count: 0 }
    }
    /// Release all metadata backing and forget the previous source.
    pub fn clear(&mut self) {
        self.count = 0;
        self.storage.clear();
    }

    /// Validate the entire directory before allocation or mutation. Failures
    /// leave an empty index, never an apparently complete partial archive.
    pub fn parse_central_directory(&mut self, cd: &[u8]) -> Result<(), &'static str> {
        self.clear();
        let result = self.parse_directory(cd);
        if result.is_err() {
            self.clear();
        }
        result
    }
    fn parse_directory(&mut self, cd: &[u8]) -> Result<(), &'static str> {
        let capacity = self.storage.capacity().min(u16::MAX as usize);
        let mut pos = 0;
        let mut total = 0;
        let mut names = 0usize;
        while pos < cd.len() {
            if cd.len() - pos < 46 || le_u32(cd, pos) != CD_SIG {
                return Err("zip: malformed central directory");
            }
            let name_len = le_u16(cd, pos + 28) as usize;
            let end =
                pos + 46 + name_len + le_u16(cd, pos + 30) as usize + le_u16(cd, pos + 32) as usize;
            if end > cd.len() {
                return Err("zip: CD entry extends past buffer");
            }
            if total < capacity {
                names = names
                    .checked_add(name_len)
                    .ok_or("zip: name pool overflow")?;
                if names > u16::MAX as usize {
                    return Err("zip: name pool overflow");
                }
            }
            total += 1;
            pos = end;
        }
        if total == 0 {
            return Err("zip: no entries in CD");
        }
        if total > capacity && !self.storage.truncate_entries() {
            return Err("zip: entry capacity exceeded");
        }
        let count = total.min(capacity);
        self.storage.prepare(count, names)?;
        if self.storage.entries().len() < count || self.storage.names().len() < names {
            return Err("zip: insufficient metadata storage");
        }
        pos = 0;
        let mut ns = 0;
        for idx in 0..count {
            let name_len = le_u16(cd, pos + 28) as usize;
            self.storage.names_mut()[ns..ns + name_len]
                .copy_from_slice(&cd[pos + 46..pos + 46 + name_len]);
            self.storage.entries_mut()[idx] = ZipEntry {
                name_start: ns as u16,
                name_len: name_len as u16,
                local_offset: le_u32(cd, pos + 42),
                comp_size: le_u32(cd, pos + 20),
                uncomp_size: le_u32(cd, pos + 24),
                method: le_u16(cd, pos + 10),
            };
            ns += name_len;
            pos += 46 + name_len + le_u16(cd, pos + 30) as usize + le_u16(cd, pos + 32) as usize;
        }
        self.count = count as u16;
        if total > capacity {
            log::warn!(
                "zip: {} entries, only {} indexed (inline capacity)",
                total,
                count
            );
        }
        Ok(())
    }
    /// Number of entries in the index.
    #[inline]
    pub fn count(&self) -> usize {
        self.count as usize
    }

    /// Return a reference to the entry at `idx`. Panics if out of range.
    #[inline]
    pub fn entry(&self, idx: usize) -> &ZipEntry {
        assert!(idx < self.count as usize);
        &self.storage.entries()[idx]
    }

    /// Return the filename of the entry at `idx` as a `&str`.
    pub fn entry_name(&self, idx: usize) -> &str {
        let e = self.entry(idx);
        let start = e.name_start as usize;
        let end = start + e.name_len as usize;
        core::str::from_utf8(&self.storage.names()[start..end]).unwrap_or("")
    }

    /// Find an entry by exact (case-sensitive) name. Returns its index.
    pub fn find(&self, name: &str) -> Option<usize> {
        let name_bytes = name.as_bytes();
        for i in 0..self.count as usize {
            let e = &self.storage.entries()[i];
            let start = e.name_start as usize;
            let end = start + e.name_len as usize;
            if &self.storage.names()[start..end] == name_bytes {
                return Some(i);
            }
        }
        None
    }

    /// Find an entry by case-insensitive ASCII name. Returns its index.
    pub fn find_icase(&self, name: &str) -> Option<usize> {
        let target = name.as_bytes();
        for i in 0..self.count as usize {
            let e = &self.storage.entries()[i];
            let start = e.name_start as usize;
            let end = start + e.name_len as usize;
            let entry_name = &self.storage.names()[start..end];
            if entry_name.eq_ignore_ascii_case(target) {
                return Some(i);
            }
        }
        None
    }
}

// ── entry extraction ────────────────────────────────────────────────

/// Extract a complete ZIP entry into a heap-allocated `Vec<u8>`.
///
/// Supports both stored and DEFLATE-compressed entries. The `read_fn`
/// closure reads bytes at a given absolute offset.
pub fn extract_entry<E, F>(
    entry: &ZipEntry,
    local_offset: u32,
    read_fn: F,
) -> Result<Vec<u8>, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, E>,
{
    extract_entry_with_scratch(
        entry,
        local_offset,
        read_fn,
        crate::scratch::HeapScratch::zeroed,
    )
}

/// Extract an entry with caller-selected layout-aware inflate scratch.
pub fn extract_entry_with_scratch<E, F, S, C>(
    entry: &ZipEntry,
    local_offset: u32,
    mut read_fn: F,
    mut scratch: C,
) -> Result<Vec<u8>, &'static str>
where
    S: crate::scratch::ScratchStorage,
    C: FnMut(core::alloc::Layout) -> Result<S, &'static str>,
    F: FnMut(u32, &mut [u8]) -> Result<usize, E>,
{
    let mut header = [0u8; 30];
    read_fn(local_offset, &mut header).map_err(|_| "zip: read local header failed")?;
    let skip = ZipIndex::local_header_data_skip(&header)?;
    let data_offset = local_offset + skip;

    if entry.uncomp_size > MAX_ENTRY_SIZE {
        return Err("zip: entry too large");
    }

    match entry.method {
        METHOD_STORED => extract_stored(entry, data_offset, &mut read_fn),
        METHOD_DEFLATE => extract_deflate(entry, data_offset, &mut read_fn, &mut scratch),
        _ => Err("zip: unsupported compression method"),
    }
}

fn extract_stored<E, F>(
    entry: &ZipEntry,
    data_offset: u32,
    read_fn: &mut F,
) -> Result<Vec<u8>, &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, E>,
{
    let size = entry.uncomp_size as usize;
    log::info!("zip: stored entry ({} bytes)", size);

    let mut out = Vec::new();
    out.try_reserve_exact(size)
        .map_err(|_| "zip: chapter too large for memory")?;
    out.resize(size, 0);
    read_all(data_offset, &mut out, read_fn)?;
    Ok(out)
}

const DEFLATE_READ_BUF: usize = 4096;

fn extract_deflate<E, F, S, C>(
    entry: &ZipEntry,
    data_offset: u32,
    read_fn: &mut F,
    scratch: &mut C,
) -> Result<Vec<u8>, &'static str>
where
    S: crate::scratch::ScratchStorage,
    C: FnMut(core::alloc::Layout) -> Result<S, &'static str>,
    F: FnMut(u32, &mut [u8]) -> Result<usize, E>,
{
    use miniz_oxide::inflate::TINFLStatus;
    use miniz_oxide::inflate::core::decompress;
    use miniz_oxide::inflate::core::inflate_flags;

    let comp_size = entry.comp_size as usize;
    let uncomp_size = entry.uncomp_size as usize;

    log::info!("zip: deflate stream {} -> {} bytes", comp_size, uncomp_size);

    let mut output = Vec::new();
    output
        .try_reserve_exact(uncomp_size)
        .map_err(|_| "zip: chapter too large for memory")?;
    output.resize(uncomp_size, 0);

    let mut decomp = crate::scratch::Decompressor::new(scratch)?;
    let mut out_pos: usize = 0;

    let mut rbuf = vec![0u8; DEFLATE_READ_BUF];
    let mut in_avail: usize = 0;
    let mut file_pos = data_offset;
    let mut comp_left = comp_size;

    loop {
        // top up compressed read buffer
        if in_avail < DEFLATE_READ_BUF && comp_left > 0 {
            let space = DEFLATE_READ_BUF - in_avail;
            let want = space.min(comp_left);
            match read_fn(file_pos, &mut rbuf[in_avail..in_avail + want]) {
                Ok(n) if n > 0 => {
                    file_pos += n as u32;
                    comp_left -= n;
                    in_avail += n;
                }
                Ok(_) => {
                    comp_left = 0;
                }
                Err(_) => return Err("zip: read failed during deflate"),
            }
        }

        if in_avail == 0 && out_pos == 0 {
            return Err("zip: empty deflate stream");
        }

        let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
            | if comp_left > 0 {
                inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
            } else {
                0
            };

        let (status, consumed, produced) =
            decompress(&mut *decomp, &rbuf[..in_avail], &mut output, out_pos, flags);

        out_pos += produced;

        if consumed > 0 && consumed < in_avail {
            rbuf.copy_within(consumed..in_avail, 0);
        }
        in_avail -= consumed;

        match status {
            TINFLStatus::Done => break,
            TINFLStatus::NeedsMoreInput => {
                if comp_left == 0 && in_avail == 0 {
                    return Err("zip: truncated deflate stream");
                }
                if consumed == 0 && produced == 0 && in_avail >= DEFLATE_READ_BUF {
                    return Err("zip: deflate stream stuck");
                }
            }
            TINFLStatus::HasMoreOutput => {
                // the output can be complete while the last input bytes (the
                // end of the final block) are still unread; stuck is an error
                if consumed == 0 && produced == 0 {
                    return Err("zip: deflate output exceeds declared size");
                }
            }
            _ => return Err("zip: deflate decompression error"),
        }
    }

    output.truncate(out_pos);
    Ok(output)
}

fn read_all<E, F>(offset: u32, buf: &mut [u8], read_fn: &mut F) -> Result<(), &'static str>
where
    F: FnMut(u32, &mut [u8]) -> Result<usize, E>,
{
    let mut total = 0usize;
    while total < buf.len() {
        let n =
            read_fn(offset + total as u32, &mut buf[total..]).map_err(|_| "zip: read failed")?;
        if n == 0 {
            return Err("zip: unexpected EOF");
        }
        total += n;
    }
    Ok(())
}

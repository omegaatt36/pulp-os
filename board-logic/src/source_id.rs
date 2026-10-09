// Persistent source identity of a book, and the incremental hash it is built with.
//
// A file name and a byte size do not identify a book: a replacement under the
// same name with the same size would reuse stale caches. The identity hashes the
// ZIP central directory, which names every entry and carries its CRC-32 and
// sizes, together with the archive size and the directory position. The
// firmware already holds those bytes while it indexes the archive, so no extra
// card read is needed.
//
// The identity is persistent (stored in cache headers and page-index records).
// It is not a handle for pending work: that is the transient generation of
// `chapter_ring::ChapterRing`.

/// 64-bit FNV-1a, incremental so large inputs can be fed in chunks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fnv64(u64);

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(Self::PRIME);
        }
        self.0 = h;
    }

    pub fn update_u32(&mut self, v: u32) {
        self.update(&v.to_le_bytes());
    }

    pub const fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Identity of one archive's content. Zero is reserved for "no source".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(u64);

impl SourceId {
    pub const NONE: SourceId = SourceId(0);

    /// Identity of an archive from its size, the position and size of the
    /// central directory, and the directory bytes themselves.
    pub fn from_archive(archive_size: u32, cd_offset: u32, cd: &[u8]) -> SourceId {
        let mut h = Fnv64::new();
        h.update(b"pulp-src1");
        h.update_u32(archive_size);
        h.update_u32(cd_offset);
        h.update_u32(cd.len() as u32);
        h.update(cd);
        // the hash of a real archive can be zero with probability 2^-64; map it
        // so that "no source" stays distinguishable
        match h.finish() {
            0 => SourceId(1),
            raw => SourceId(raw),
        }
    }

    /// Wrap a stored value. A stored zero stays "no source".
    pub const fn from_raw(raw: u64) -> SourceId {
        SourceId(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// 32-bit fold, for the 8.3 directory name of the book's cache.
    pub const fn dir_hash(self) -> u32 {
        (self.0 ^ (self.0 >> 32)) as u32
    }
}

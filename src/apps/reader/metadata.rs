// EPUB metadata has an explicit owner and placement policy. The C61 reader
// and Files title scan use these small handles, never inline metadata arrays.
use crate::kernel::{BigBuf, BufClass, TypedBuf};
use pulp_board_logic::chapter_ring::RingConfig;
use pulp_board_logic::image_lru::LruConfig;
use smol_epub::epub;
use smol_epub::epub::TocEntry;
use smol_epub::zip;
use smol_epub::zip::{ZipEntry, ZipStorage};

/// Only the larger validated PSRAM profile increases metadata capacities.
/// A 2 MiB chip and degraded startup retain the established 256-entry limits.
pub(crate) const fn metadata_capacity(status: pulp_board_logic::memory::PsramStatus) -> usize {
    match status {
        pulp_board_logic::memory::PsramStatus::Ready { bytes }
            if bytes >= pulp_board_logic::memory::PSRAM_LARGE_MIN_BYTES =>
        {
            512
        }
        _ => 256,
    }
}

pub(crate) struct ClassZipStorage {
    entries: TypedBuf<ZipEntry>,
    names: BigBuf,
    capacity: usize,
}
impl ClassZipStorage {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            entries: TypedBuf::empty(),
            names: BigBuf::empty(),
            capacity,
        }
    }
}
impl ZipStorage for ClassZipStorage {
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn prepare(&mut self, entries: usize, names: usize) -> Result<(), &'static str> {
        if entries > self.capacity || names > u16::MAX as usize {
            return Err("zip: metadata capacity exceeded");
        }
        let entry_buf = TypedBuf::filled(BufClass::ZipToc, entries, ZipEntry::EMPTY)
            .map_err(|_| "zip: entry allocation refused")?;
        let name_buf =
            BigBuf::zeroed(BufClass::ZipToc, names).map_err(|_| "zip: name allocation refused")?;
        self.entries = entry_buf;
        self.names = name_buf;
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
        self.entries = TypedBuf::empty();
        self.names = BigBuf::empty();
    }
}

pub(crate) type ZipIndex = zip::ZipIndex<ClassZipStorage>;

// Startup remains const and allocation-free; choose the runtime capacity only
// immediately before parsing, after the board has validated its PSRAM.
pub(crate) const fn empty_zip() -> ZipIndex {
    ZipIndex::with_storage(ClassZipStorage::new(256))
}
pub(crate) fn new_zip() -> ZipIndex {
    ZipIndex::with_storage(ClassZipStorage::new(metadata_capacity(
        pulp_kernel::board_c61::memory::status(),
    )))
}

pub(crate) type Toc = epub::EpubToc<TypedBuf<TocEntry>>;

pub(crate) fn new_toc() -> Result<Toc, &'static str> {
    class_toc(metadata_capacity(pulp_kernel::board_c61::memory::status()))
}
pub(crate) fn class_toc(
    capacity: usize,
) -> Result<epub::EpubToc<TypedBuf<TocEntry>>, &'static str> {
    let entries = TypedBuf::filled(BufClass::ZipToc, capacity, TocEntry::EMPTY)
        .map_err(|_| "epub: TOC allocation refused")?;
    Ok(epub::EpubToc::with_entries(entries))
}

/// Working-set policy chosen once per book from the validated PSRAM state: the
/// chapter ring (previous / current / next) and the decoded-image LRU. HR2
/// and degraded builds keep the single 96 KiB chapter and no image cache,
/// and neither read nor write stored page indexes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct MemProfile {
    pub(crate) ring: RingConfig,
    pub(crate) images: LruConfig,
    /// Persist complete EPUB page indexes (PGnnn.IDX) on the card. Only the
    /// profiles with the large page table and the chapter ring set it.
    pub(crate) persist_index: bool,
}

impl MemProfile {
    pub(crate) const SMALL: Self = Self {
        ring: RingConfig::SMALL,
        images: LruConfig::OFF,
        persist_index: false,
    };

    pub(crate) const fn for_status(status: pulp_board_logic::memory::PsramStatus) -> Self {
        Self {
            ring: RingConfig::for_status(status),
            images: LruConfig::for_status(status),
            persist_index: matches!(
                status,
                pulp_board_logic::memory::PsramStatus::Ready { bytes }
                    if bytes >= pulp_board_logic::memory::PSRAM_LARGE_MIN_BYTES
            ),
        }
    }
}

pub(crate) fn detect_profile() -> MemProfile {
    MemProfile::for_status(pulp_kernel::board_c61::memory::status())
}

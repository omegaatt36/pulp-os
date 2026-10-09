// Reusable immutable font-pack indices and the opt-in flash font set.
//
// HAL-free and allocation-free: the registry owns opaque payloads (`T` is a
// `RamIndex<BigBuf>` in the firmware, a `Vec<u8>` in the tests) and only does
// the accounting, the identity checks and the eviction policy.
//
// Why a registry. A whole pack index (about 272 KiB per pixel size) is loaded
// in around 0.6 s on the C61 and never changes while the pack does not. The
// state that used to own it was cleared on every suspend and book open, so a
// warm app switch paid the load again; the page-visible glyph caches are
// transient, the index is not. The registry keeps completed indices apart from
// them, so they survive `clear`, and shares them with the auxiliary surfaces.
//
// Identity. An entry is found by its whole `IndexKey`: the pack's font ID, pixel
// size, record count and bitmap size together with the source generation. The
// generation (`SourceGeneration`) changes whenever the card is replaced and
// whenever the font directory is written to, so a pack replaced under the same
// name and size never answers from an index of its predecessor.
//
// Ownership. The registry never hands out an alias. `lease` moves the index out
// to its single user and `give_back` returns it; a leased entry cannot be
// evicted or leased again, and an entry retired while it is out is dropped by
// `give_back`. All calls are meant to be short borrows under a critical
// section; anything that allocates, reads or drops a payload happens outside
// it: `begin_load` hands out a `LoadToken`, the caller allocates and reads, and
// `publish` accepts the result only if the token is still current. Evicted and
// stale payloads come back in a `Retired` sink for the caller to drop after
// the borrow ended.
//
// Failure. A load that failed is remembered for its key (no retry until
// `forget_failures`, or until the generation changes) so a page does not repeat
// a 0.6 s attempt that cannot succeed.

use crate::memory::PSRAM_LARGE_FONT_GLYPHS_BYTES;
use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

/// Whole indices held at once.
pub const INDEX_BANKS: usize = 2;
/// Bytes of all resident (and loading) indices together: half of the large
/// profile's `FontGlyphs` class, the other half is for slot tables and bitmaps.
/// The class limit still applies to every single allocation.
pub const INDEX_REGISTRY_BYTES: usize = PSRAM_LARGE_FONT_GLYPHS_BYTES / 2;

const _: () = assert!(INDEX_REGISTRY_BYTES * 2 <= PSRAM_LARGE_FONT_GLYPHS_BYTES);

/// Counter of the font source: bumped when the card is replaced and when the
/// font directory is written to. Only load and store are used (the X4 core has
/// no atomic read-modify-write); every bump comes from the kernel and app
/// context, which does not run concurrently with itself.
pub struct SourceGeneration(AtomicU32);

impl SourceGeneration {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    pub fn get(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn bump(&self) {
        self.0.store(
            self.0.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

impl Default for SourceGeneration {
    fn default() -> Self {
        Self::new()
    }
}

/// The font source of the running firmware.
pub static FONT_SOURCE: SourceGeneration = SourceGeneration::new();

/// What an index is valid for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IndexKey {
    pub font_id: u64,
    pub pixel_size: u16,
    pub glyph_count: u32,
    pub bitmap_len: u32,
    /// `SourceGeneration` the pack was read under.
    pub source: u32,
}

/// Handle of one load in flight.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LoadToken(u32);

/// What `lease` found.
#[derive(Debug, PartialEq, Eq)]
pub enum Lease<T> {
    /// The index, now owned by the caller until `give_back`.
    Hit(T),
    /// Nothing resident for the key.
    Miss,
    /// A load of this key failed; do not try again.
    Failed,
    /// Resident but out with another user.
    Busy,
    /// The key belongs to a source older than the registry knows.
    Stale,
}

/// Why `begin_load` did not start a load.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Zero bytes, or more than the whole registry holds.
    Size,
    /// An earlier load of this key failed.
    Failed,
    /// Another load is in flight.
    Loading,
    /// The key is already resident (possibly out with a user).
    Present,
    /// Everything that would have to be evicted is out with a user.
    InUse,
    /// The key belongs to a source older than the registry knows.
    Stale,
}

/// Payloads that left the registry; the caller drops them after the borrow.
#[must_use = "dropping a payload may free external memory; do it outside the critical section"]
pub struct Retired<T>([Option<T>; INDEX_BANKS]);

impl<T> Retired<T> {
    pub const fn new() -> Self {
        Self([const { None }; INDEX_BANKS])
    }

    pub fn len(&self) -> usize {
        self.0.iter().filter(|v| v.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn push(&mut self, value: T) {
        if let Some(slot) = self.0.iter_mut().find(|v| v.is_none()) {
            *slot = Some(value);
        }
    }
}

impl<T> Default for Retired<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct Entry<T> {
    key: IndexKey,
    bytes: usize,
    stamp: u32,
    // None while leased
    value: Option<T>,
}

struct Loading {
    token: LoadToken,
    key: IndexKey,
    bytes: usize,
}

pub struct IndexRegistry<T> {
    cap: usize,
    source: u32,
    banks: [Option<Entry<T>>; INDEX_BANKS],
    failed: [Option<IndexKey>; INDEX_BANKS],
    failed_next: usize,
    loading: Option<Loading>,
    next_token: u32,
    clock: u32,
}

impl<T> IndexRegistry<T> {
    /// `cap`: bytes of resident and loading indices together.
    pub const fn new(cap: usize) -> Self {
        Self {
            cap,
            source: 0,
            banks: [const { None }; INDEX_BANKS],
            failed: [None; INDEX_BANKS],
            failed_next: 0,
            loading: None,
            next_token: 0,
            clock: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// The newest source generation seen.
    pub fn source(&self) -> u32 {
        self.source
    }

    /// Bytes held by resident entries, leased or not.
    pub fn resident_bytes(&self) -> usize {
        self.banks.iter().flatten().map(|e| e.bytes).sum()
    }

    /// Bytes held by resident entries and the load in flight.
    pub fn committed_bytes(&self) -> usize {
        self.resident_bytes() + self.loading.as_ref().map_or(0, |l| l.bytes)
    }

    pub fn resident(&self) -> usize {
        self.banks.iter().flatten().count()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Whether `key` is resident (leased or not).
    pub fn contains(&self, key: &IndexKey) -> bool {
        self.banks.iter().flatten().any(|e| e.key == *key)
    }

    // Adopts a newer source: everything bound to the old one goes, a load in
    // flight can no longer publish. An older source than the known one is not
    // adopted (`false`): its caller read the generation before a change.
    fn sync(&mut self, source: u32, retired: &mut Retired<T>) -> bool {
        if source == self.source {
            return true;
        }
        if source.wrapping_sub(self.source) >= 1 << 31 {
            return false;
        }
        self.source = source;
        for bank in &mut self.banks {
            if let Some(entry) = bank.take()
                && let Some(value) = entry.value
            {
                retired.push(value);
            }
        }
        self.failed = [None; INDEX_BANKS];
        self.loading = None;
        true
    }

    fn tick(&mut self) -> u32 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    /// Moves the index of `key` out to the caller.
    pub fn lease(&mut self, key: IndexKey, retired: &mut Retired<T>) -> Lease<T> {
        if !self.sync(key.source, retired) {
            return Lease::Stale;
        }
        let stamp = self.tick();
        if let Some(entry) = self.banks.iter_mut().flatten().find(|e| e.key == key) {
            return match entry.value.take() {
                Some(value) => {
                    entry.stamp = stamp;
                    Lease::Hit(value)
                }
                None => Lease::Busy,
            };
        }
        if self.failed.contains(&Some(key)) {
            Lease::Failed
        } else {
            Lease::Miss
        }
    }

    /// Returns a leased index. `Err` hands it back when its entry is gone
    /// (retired while it was out), for the caller to drop.
    pub fn give_back(&mut self, key: IndexKey, value: T) -> Result<(), T> {
        match self
            .banks
            .iter_mut()
            .flatten()
            .find(|e| e.key == key && e.value.is_none())
        {
            Some(entry) => {
                entry.value = Some(value);
                Ok(())
            }
            None => Err(value),
        }
    }

    /// Reserves room for an index of `bytes` and starts its load, evicting the
    /// least recently used resident indices that are not out with a user.
    pub fn begin_load(
        &mut self,
        key: IndexKey,
        bytes: usize,
        retired: &mut Retired<T>,
    ) -> Result<LoadToken, Refusal> {
        if !self.sync(key.source, retired) {
            return Err(Refusal::Stale);
        }
        if bytes == 0 || bytes > self.cap {
            return Err(Refusal::Size);
        }
        if self.failed.contains(&Some(key)) {
            return Err(Refusal::Failed);
        }
        if self.loading.is_some() {
            return Err(Refusal::Loading);
        }
        if self.contains(&key) {
            return Err(Refusal::Present);
        }
        loop {
            let fits = self.resident_bytes() + bytes <= self.cap;
            if fits && self.banks.iter().any(Option::is_none) {
                break;
            }
            let victim = self
                .banks
                .iter()
                .enumerate()
                .filter_map(|(i, b)| b.as_ref().map(|e| (i, e)))
                .filter(|(_, e)| e.value.is_some())
                .min_by_key(|(_, e)| e.stamp)
                .map(|(i, _)| i);
            let Some(victim) = victim else {
                return Err(Refusal::InUse);
            };
            if let Some(value) = self.banks[victim].take().and_then(|e| e.value) {
                retired.push(value);
            }
        }
        self.next_token = self.next_token.wrapping_add(1).max(1);
        let token = LoadToken(self.next_token);
        self.loading = Some(Loading { token, key, bytes });
        Ok(token)
    }

    /// Stores a completed load. `Err` hands the index back when the token is no
    /// longer current (source changed, registry cleared), for the caller to drop.
    pub fn publish(&mut self, token: LoadToken, value: T) -> Result<(), T> {
        let current = self.loading.as_ref().is_some_and(|l| l.token == token);
        let free = self.banks.iter().position(Option::is_none);
        match (current, free) {
            (true, Some(at)) => {
                if let Some(loading) = self.loading.take() {
                    let stamp = self.tick();
                    self.banks[at] = Some(Entry {
                        key: loading.key,
                        bytes: loading.bytes,
                        stamp,
                        value: Some(value),
                    });
                }
                Ok(())
            }
            _ => Err(value),
        }
    }

    /// Ends a load that failed and remembers its key.
    pub fn fail(&mut self, token: LoadToken) {
        if self.loading.as_ref().is_some_and(|l| l.token == token)
            && let Some(loading) = self.loading.take()
        {
            self.failed[self.failed_next] = Some(loading.key);
            self.failed_next = (self.failed_next + 1) % INDEX_BANKS;
        }
    }

    /// Ends a load without remembering a failure.
    pub fn abandon(&mut self, token: LoadToken) {
        if self.loading.as_ref().is_some_and(|l| l.token == token) {
            self.loading = None;
        }
    }

    /// Allows the keys that failed to be loaded again.
    pub fn forget_failures(&mut self) {
        self.failed = [None; INDEX_BANKS];
    }

    /// Drops every resident index and cancels the load in flight. An index out
    /// with a user is dropped when it comes back.
    pub fn clear(&mut self, retired: &mut Retired<T>) {
        for bank in &mut self.banks {
            if let Some(entry) = bank.take()
                && let Some(value) = entry.value
            {
                retired.push(value);
            }
        }
        self.loading = None;
        self.failed = [None; INDEX_BANKS];
    }
}

/// Pixel sizes of the CJK packs (the reader's body and heading sizes).
pub const PACK_PIXEL_SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];
/// Bytes of font packs the optional flash source may link into the image. The
/// C61 ROM region is 4 MiB and the Wi-Fi image already maps 1.4 MB of it, so
/// the cap is conservative; the linker still refuses an image that does not fit.
pub const FLASH_FONT_MAX_TOTAL_BYTES: usize = 2 * 1024 * 1024;
/// Build-time list of font pack files for the flash source (path list, the
/// platform's path separator between entries).
pub const FLASH_FONT_ENV: &str = "PULP_C61_FLASH_FONTS";

/// One configured pack, after its file was read and parsed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FlashPackInfo {
    pub pixel_size: u16,
    pub len: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlashSetError {
    /// The variable is set, but lists no pack.
    Empty,
    /// A pack of a pixel size the reader never asks for.
    UnknownSize { pixel_size: u16 },
    /// Two packs of the same pixel size.
    Duplicate { pixel_size: u16 },
    /// One pack alone exceeds the cap.
    PackTooLarge { pixel_size: u16, len: usize },
    /// The packs together exceed the cap.
    TotalTooLarge { total: usize },
}

impl fmt::Display for FlashSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => write!(f, "{FLASH_FONT_ENV} is set but lists no font pack"),
            Self::UnknownSize { pixel_size } => write!(
                f,
                "a {pixel_size} px pack is not one of the sizes the reader uses {PACK_PIXEL_SIZES:?}"
            ),
            Self::Duplicate { pixel_size } => {
                write!(f, "more than one {pixel_size} px pack is configured")
            }
            Self::PackTooLarge { pixel_size, len } => write!(
                f,
                "the {pixel_size} px pack is {len} bytes, more than the {FLASH_FONT_MAX_TOTAL_BYTES} byte limit"
            ),
            Self::TotalTooLarge { total } => write!(
                f,
                "the packs total {total} bytes, more than the {FLASH_FONT_MAX_TOTAL_BYTES} byte limit"
            ),
        }
    }
}

/// Checks a configured set of packs; returns their total size.
pub fn validate_flash_set(packs: &[FlashPackInfo]) -> Result<usize, FlashSetError> {
    if packs.is_empty() {
        return Err(FlashSetError::Empty);
    }
    let mut total = 0usize;
    for (i, pack) in packs.iter().enumerate() {
        let pixel_size = pack.pixel_size;
        if !PACK_PIXEL_SIZES.contains(&pixel_size) {
            return Err(FlashSetError::UnknownSize { pixel_size });
        }
        if packs[..i].iter().any(|p| p.pixel_size == pixel_size) {
            return Err(FlashSetError::Duplicate { pixel_size });
        }
        if pack.len > FLASH_FONT_MAX_TOTAL_BYTES {
            return Err(FlashSetError::PackTooLarge {
                pixel_size,
                len: pack.len,
            });
        }
        total = total.saturating_add(pack.len);
    }
    if total > FLASH_FONT_MAX_TOTAL_BYTES {
        return Err(FlashSetError::TotalTooLarge { total });
    }
    Ok(total)
}

/// The configured pack of `pixel_size`, if there is one.
pub fn select_flash_pack<'a, B: ?Sized>(
    packs: &'a [(u16, &'static B)],
    pixel_size: u16,
) -> Option<&'static B> {
    packs
        .iter()
        .find(|(px, _)| *px == pixel_size)
        .map(|(_, bytes)| *bytes)
}

// sd card over SPI: sync SdCard + async volume manager
//
// sync SdCard handles the SD protocol (CMD0, init, sector I/O)
// using embedded_hal SpiDevice + DelayNs traits
//
// BlockDeviceAdapter bridges sync BlockDevice to AsyncBlockDevice
// so AsyncVolumeManager can consume it
//
// poll_once drives file-I/O futures to completion in a single poll
// (SPI bus is blocking, so every .await resolves immediately)

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
#[cfg(feature = "board-x4")]
use embedded_hal::delay::DelayNs;

#[cfg(feature = "board-onepage-c61")]
use core::sync::atomic::{AtomicBool, Ordering};
use embedded_sdmmc::{
    AsyncBlockDevice, AsyncVolumeManager, Block, BlockCount, BlockDevice, BlockIdx, RawDirectory,
    RawFile, RawVolume, SdCard, TimeSource, Timestamp, VolumeIdx,
};
use log::info;
#[cfg(feature = "board-onepage-c61")]
use pulp_board_logic::sd_cache::{
    CachedDevice, Meta, SECTOR_BYTES, Sector, SectorCacheStats, SectorDevice, build_cache,
    sd_cache_budget,
};

#[cfg(feature = "board-onepage-c61")]
use crate::kernel::bigbuf::{BufClass, TypedBuf};
use crate::util::{CloseBorrow, CloseCell, CloseHandle, CloseToken};

// the SPI device type is board-specific (X4: CriticalSectionDevice over a raw
// GPIO12 CS; C61: arbiter-managed device, see board_c61::spi)
#[cfg(feature = "board-x4")]
use crate::board::SdSpiDevice;
#[cfg(feature = "board-onepage-c61")]
use crate::board_c61::spi::SdSpiDevice;

// sync BlockDevice -> AsyncBlockDevice adapter
//
// sync SdCard uses RefCell internally, takes &self for BlockDevice
// methods; we delegate AsyncBlockDevice &mut self to the inner &self
// methods. All resolve immediately since SPI is DMA-blocking
//
// X4: a plain pass-through, exactly as before.
//
// C61: a bounded read cache (pulp_board_logic::sd_cache) sits in this adapter,
// below the volume manager (FAT ownership is untouched). Write-through, one
// cache per adapter, so a mount (card insertion) always starts with a fresh,
// empty cache. Backing is `BufClass::StorageCache` (PSRAM, fallible, tags and
// contents both charged); if it cannot be allocated, or PSRAM is degraded,
// the adapter is a plain pass-through too. The DMA bounce buffer of the SPI
// driver is internal and unrelated to this cache. Every operation still
// completes inside one poll.

#[cfg(feature = "board-x4")]
pub(crate) struct BlockDeviceAdapter<D: BlockDevice>(D);

#[cfg(feature = "board-x4")]
impl<D: BlockDevice> BlockDeviceAdapter<D> {
    fn new(card: D) -> Self {
        Self(card)
    }
}

#[cfg(feature = "board-x4")]
impl<D: BlockDevice> AsyncBlockDevice for BlockDeviceAdapter<D> {
    type Error = D::Error;

    async fn read(
        &mut self,
        blocks: &mut [Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Self::Error> {
        self.0.read(blocks, start_block_idx)
    }

    async fn write(
        &mut self,
        blocks: &[Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Self::Error> {
        self.0.write(blocks, start_block_idx)
    }

    async fn num_blocks(&mut self) -> Result<BlockCount, Self::Error> {
        self.0.num_blocks()
    }
}

// the sync card as a `SectorDevice` (Block is Clone, not Copy, and has no layout
// guarantee: access goes through its `contents`)
#[cfg(feature = "board-onepage-c61")]
struct SdIo<D: BlockDevice>(D);

#[cfg(feature = "board-onepage-c61")]
impl<D: BlockDevice> SectorDevice for SdIo<D> {
    type Elem = Block;
    type Error = D::Error;

    fn bytes(e: &Block) -> &Sector {
        &e.contents
    }

    fn bytes_mut(e: &mut Block) -> &mut Sector {
        &mut e.contents
    }

    fn read(&mut self, dst: &mut [Block], start: u32) -> Result<(), D::Error> {
        self.0.read(dst, BlockIdx(start))
    }

    fn write(&mut self, src: &[Block], start: u32) -> Result<(), D::Error> {
        self.0.write(src, BlockIdx(start))
    }
}

#[cfg(feature = "board-onepage-c61")]
type CacheTags = TypedBuf<Meta>;
#[cfg(feature = "board-onepage-c61")]
type CacheData = TypedBuf<Sector>;

// set by a failed write; the volume manager keeps the (modified) sector of a
// failed write in its own one-block cache, so the next storage borrow, or the
// failing write site itself, must drop that block (see
// `SdStorageInner::discard_block_cache`)
#[cfg(feature = "board-onepage-c61")]
static WRITE_FAILED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "board-onepage-c61")]
pub(crate) struct BlockDeviceAdapter<D: BlockDevice> {
    io: CachedDevice<SdIo<D>, CacheTags, CacheData>,
}

#[cfg(feature = "board-onepage-c61")]
impl<D: BlockDevice> BlockDeviceAdapter<D> {
    fn new(card: D) -> Self {
        let budget = sd_cache_budget(crate::board_c61::memory::status());
        let cache = build_cache(
            budget,
            |n| TypedBuf::filled(BufClass::StorageCache, n, Meta::EMPTY),
            |n| TypedBuf::filled(BufClass::StorageCache, n, [0u8; SECTOR_BYTES]),
        );
        match &cache {
            Some(c) => info!(
                "SD cache: {} sectors ({} KiB data) in {:?}",
                c.capacity(),
                c.capacity() * SECTOR_BYTES / 1024,
                BufClass::StorageCache
            ),
            None => info!("SD cache: off (budget {} B)", budget),
        }
        Self {
            io: CachedDevice::new(SdIo(card), cache),
        }
    }

    fn stats(&self) -> Option<SectorCacheStats> {
        self.io.stats()
    }
}

#[cfg(feature = "board-onepage-c61")]
impl<D: BlockDevice> AsyncBlockDevice for BlockDeviceAdapter<D> {
    type Error = D::Error;

    async fn read(
        &mut self,
        blocks: &mut [Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Self::Error> {
        self.io.read(blocks, start_block_idx.0)
    }

    async fn write(
        &mut self,
        blocks: &[Block],
        start_block_idx: BlockIdx,
    ) -> Result<(), Self::Error> {
        let r = self.io.write(blocks, start_block_idx.0);
        if r.is_err() {
            WRITE_FAILED.store(true, Ordering::Release);
        }
        r
    }

    async fn num_blocks(&mut self) -> Result<BlockCount, Self::Error> {
        self.io.device().0.num_blocks()
    }
}

// no RTC on this board

pub(crate) struct NullTimeSource;

impl TimeSource for NullTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

// type aliases

pub type SyncSdCard = SdCard<SdSpiDevice, esp_hal::delay::Delay>;
pub(crate) type SdBlockDev = BlockDeviceAdapter<SyncSdCard>;
// 6 dirs: root and _PULP stay open (see `SdStorageInner::pulp`), a path walk and
// the transient handles of one operation need up to 3 more. 8 files: up to
// `HELD_SLOTS` held read-only files plus the one an operation opens itself
pub(crate) const OPEN_FILES: usize = 8;
pub(crate) type VolMgr = AsyncVolumeManager<SdBlockDev, NullTimeSource, 6, OPEN_FILES, 1>;
pub(crate) type StorageBorrow<'a> = CloseBorrow<'a, SdStorageInner, OPEN_FILES>;
pub(crate) type WriterHandle<'a> = CloseToken<'a, SdStorageInner, OPEN_FILES>;

// persistent volume manager state, held behind RefCell for interior
// mutability (AsyncVolumeManager requires &mut self)

pub(crate) struct SdStorageInner {
    pub(crate) mgr: VolMgr,
    #[allow(dead_code)]
    pub(crate) vol: RawVolume,
    pub(crate) root: RawDirectory,
    // _PULP, opened on first use and kept: every cache, settings and font access
    // starts with it, and finding it in a big root directory is a linear scan.
    // A directory handle is only the first cluster, so entries added or removed
    // meanwhile do not make it stale
    pub(crate) pulp: Option<RawDirectory>,
    pub(crate) held: HeldFiles,
}

impl SdStorageInner {
    /// Drop the volume manager's one-block cache if a device write failed since
    /// the last call. That cache keeps the already-modified bytes of a block
    /// whose write-back failed; `device()` clears it (the next access re-reads
    /// the card, through the sector cache, which dropped the sector too).
    /// Call right after a failed write and at the start of every storage
    /// borrow. No-op on X4.
    #[inline]
    pub(crate) fn discard_block_cache(&mut self) {
        #[cfg(feature = "board-onepage-c61")]
        if WRITE_FAILED.load(Ordering::Acquire) {
            WRITE_FAILED.store(false, Ordering::Release);
            let _ = self.mgr.device();
        }
    }
}

impl CloseHandle for SdStorageInner {
    type Handle = RawFile;
    type Error = crate::error::Error;

    fn close_handle(&mut self, file: RawFile) -> Result<(), Self::Error> {
        // The pinned FAT manager removes the handle even if flushing fails.
        let result = poll_once(self.mgr.close_file(file));
        if result.is_err() {
            self.discard_block_cache();
            log::warn!("storage: writer close failed");
        }
        result.map_err(|_| crate::error::Error::new(crate::error::ErrorKind::WriteFailed, "close"))
    }
}

// read-only files kept open across calls (see storage::with_pulp_subdir_file and
// storage::held_open): a dir walk plus open costs 60-190 ms on the C61 card (every
// directory on the way is scanned linearly), far more than the reads themselves.
// lives in the inner state so a removed or replaced card drops it with the
// volume. FAT refuses to open a file that is already open, so every write, delete
// or other open of a held file releases its slot first
pub(crate) const HELD_KEY_LEN: usize = 24;
pub(crate) const HELD_SLOTS: usize = 6;

// where a held file lives: the root directory, _PULP, or _PULP/<dir>
// (the key of the last one is "<dir>/<name>")
pub(crate) const SCOPE_ROOT: u8 = 0;
pub(crate) const SCOPE_PULP: u8 = 1;
pub(crate) const SCOPE_SUB: u8 = 2;

#[derive(Clone, Copy)]
pub(crate) struct HeldFile {
    pub(crate) scope: u8,
    pub(crate) key: [u8; HELD_KEY_LEN],
    pub(crate) key_len: u8,
    pub(crate) file: RawFile,
}

pub(crate) struct HeldFiles {
    pub(crate) slots: [Option<HeldFile>; HELD_SLOTS],
    pub(crate) next_evict: usize,
}

impl HeldFiles {
    pub(crate) const fn new() -> Self {
        Self {
            slots: [None; HELD_SLOTS],
            next_evict: 0,
        }
    }
}

// holds a persistently-mounted AsyncVolumeManager with volume 0 and
// root directory kept open for the device lifetime; RefCell provides
// interior mutability so storage functions can take &SdStorage

pub struct SdStorage {
    inner: Option<CloseCell<SdStorageInner, OPEN_FILES>>,
}

impl SdStorage {
    pub fn empty() -> Self {
        Self { inner: None }
    }

    // init SD card at 400 kHz (SD spec init frequency)
    //
    // sync SdCard auto-initialises on first method call; we call
    // num_bytes() to force init and verify the card responds
    //
    // pub so Board::init can run this before other SPI peripherals
    // touch the bus - SD spec requires a clean 400 kHz bus for CMD0
    //
    // X4 only: the C61 init goes through board_c61::sd::init (requires the
    // GPIO27 SdInitPermit and classifies failures instead of returning None)
    #[cfg(feature = "board-x4")]
    pub fn init_card(spi_device: SdSpiDevice) -> Option<SyncSdCard> {
        let sd = SdCard::new(spi_device, esp_hal::delay::Delay::new());

        for attempt in 1..=5 {
            match sd.num_bytes() {
                Ok(size) => {
                    info!("SD card: initialised (attempt {})", attempt);
                    info!("SD card: {} bytes ({} MB)", size, size / 1024 / 1024);
                    return Some(sd);
                }
                Err(e) => {
                    info!("SD card: init attempt {} failed: {:?}", attempt, e);
                    sd.mark_card_uninit();
                    esp_hal::delay::Delay::new().delay_ms(50);
                }
            }
        }

        info!("SD card: all init attempts failed");
        None
    }

    // mount FAT filesystem on an already-initialised SD card
    //
    // opens volume 0 (first MBR partition) and keeps the root
    // directory open for the device lifetime
    pub async fn mount(sd: SyncSdCard) -> Self {
        let adapter = BlockDeviceAdapter::new(sd);
        // the limits of `VolMgr`; 5000 is the id offset `AsyncVolumeManager::new` uses
        let mut mgr: VolMgr = AsyncVolumeManager::new_with_limits(adapter, NullTimeSource, 5000);

        let vol = match mgr.open_raw_volume(VolumeIdx(0)).await {
            Ok(v) => v,
            Err(e) => {
                info!("SD card: open volume failed: {}", e);
                return Self { inner: None };
            }
        };

        let root = match mgr.open_root_dir(vol) {
            Ok(d) => d,
            Err(e) => {
                info!("SD card: open root dir failed: {}", e);
                let _ = mgr.close_volume(vol).await;
                return Self { inner: None };
            }
        };

        info!("SD card: filesystem mounted");
        Self {
            inner: Some(CloseCell::new(SdStorageInner {
                mgr,
                vol,
                root,
                pulp: None,
                held: HeldFiles::new(),
            })),
        }
    }

    #[inline]
    pub fn probe_ok(&self) -> bool {
        self.inner.is_some()
    }

    /// Sector read cache counters (C61 with the cache enabled); `None` on X4,
    /// without a mounted volume, with the cache off, or while a storage
    /// operation holds the volume.
    #[cfg(feature = "board-onepage-c61")]
    pub fn cache_stats(&self) -> Option<SectorCacheStats> {
        // `device()` also drops the volume manager's one-block cache: harmless
        // (a re-read at worst, served by the sector cache)
        self.inner.as_ref()?.try_borrow_mut()?.mgr.device().stats()
    }

    #[inline]
    pub(crate) fn borrow_inner(&self) -> Option<StorageBorrow<'_>> {
        self.inner.as_ref().map(|c| c.borrow_mut())
    }

    // Reserve before FAT open: even a reentrant Drop has a bounded slot for
    // its raw handle. The borrow guard drains queued closes on release.
    pub(crate) fn reserve_writer(&self) -> Option<WriterHandle<'_>> {
        self.inner.as_ref()?.reserve()
    }

    // flush pending writes and close fat handles; best-effort before halt.
    // after this call no further sd i/o is possible until mcu reset
    pub fn flush_and_close(&self) {
        if let Some(ref cell) = self.inner {
            let mut guard = cell.borrow_mut();
            let inner = &mut *guard;
            if let Some(pulp) = inner.pulp.take() {
                let _ = inner.mgr.close_dir(pulp);
            }
            let _ = inner.mgr.close_dir(inner.root);
            poll_once(async {
                for slot in &mut inner.held.slots {
                    if let Some(held) = slot.take() {
                        let _ = inner.mgr.close_file(held.file).await;
                    }
                }
                let _ = inner.mgr.close_volume(inner.vol).await;
            });
        }
    }
}

// drive a future to completion in exactly one poll
//
// correct because the SPI bus is blocking and the sync SdCard
// completes every operation before returning - no inner .await
// ever returns Pending
//
// only use for file-level operations (open, read, write, seek,
// iterate); mount runs inside the real Embassy executor

pub fn poll_once<T>(fut: impl Future<Output = T>) -> T {
    let waker: &Waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("poll_once: future pended -- SPI must be in Blocking mode"),
    }
}

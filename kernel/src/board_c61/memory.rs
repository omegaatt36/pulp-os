// OnePage C61 memory placement: esp-hal PSRAM bring-up, a separate
// PSRAM heaps, and budget-checked allocation entry points.
//
// Policy (rules, limits and the failure state machine are in
// `pulp_board_logic::memory`, host-tested; this file only maps them onto
// esp-hal 1.2 / esp-alloc 0.11):
//
//   * The global allocator (`esp_alloc::HEAP`) only ever gets INTERNAL
//     regions (`heap_allocator!`). PSRAM uses private heaps: PSRAM_HEAP for
//     resident/cache backing, plus SCRATCH_HEAP on HR8. Never register it
//     through `esp_alloc::psram_allocator!`:
//     that macro adds an External region to the global heap, and a plain
//     `Box`/`Vec` would then fall into PSRAM as soon as the internal region is
//     full (esp-alloc tries every region when no capability is requested).
//     Runtime, executor, ISR-visible data and every DMA buffer would silently
//     lose their internal-memory guarantee.
//   * PSRAM is reachable only through `alloc_external(ExternalClass, ..)`
//     (the enum has no DMA/ISR/runtime variant, so asking for those does not
//     compile) or `alloc(MemClass, ..)`, which places by the budget rule.
//     Every allocation is charged against one `MemoryBudget`; over budget is
//     an `Err`, never a panic and never an out-of-memory spiral.
//   * Statics (SPI DMA `BUFFER`/`DESCRIPTORS`, the main heap, task arena)
//     are internal by construction (the linker never maps PSRAM) and
//     esp-hal's `DmaRxBuf::new` rejects descriptors outside DRAM; the ELF check
//     (harness/tests/c61_memory_budget.rs) proves their addresses.
//   * PSRAM failure: `init` returns `Degraded(..)`, nothing is registered, the
//     budget switches to the internal limits. Large allocations can then be
//     refused; bring-up failure itself does not panic.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::RefCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::Mutex;
use esp_alloc::{EspHeap, HeapRegion, MemoryCapability};
use esp_hal::peripherals::PSRAM;
use esp_hal::psram::{FlashFreq, Psram, PsramConfig, SpiRamFreq};
use log::{error, info, warn};
use pulp_board_logic::memory::{
    AllocationKind, HeapKind, PSRAM_LARGE_MIN_BYTES, SCRATCH_ARENA_BYTES, external_heap_kind,
};
pub use pulp_board_logic::memory::{
    ExternalClass, INTERNAL_HEAP_RECLAIMED_BYTES, MemClass, MemError, MemoryBudget,
    PSRAM_CHAPTER_TEXT_BYTES, PsramFault, PsramStatus, Region, Reservation,
};
use pulp_board_logic::memory::{
    FLASH_MHZ, PSRAM_MAX_BYTES, PSRAM_MHZ, PSRAM_MIN_BYTES, WordMem, evaluate_psram,
    internal_heap_main_bytes, range_is_internal, selftest, window_fault,
};

/// This build is the Wi-Fi variant (the radio's static RAM leaves room for a
/// smaller main heap, see board-logic `INTERNAL_HEAP_MAIN_BYTES_WIFI`).
const WIFI: bool = cfg!(feature = "wifi");

/// Main-RAM internal heap the firmware must register (`heap_allocator!`). The
/// firmware and `BUDGET` both take the variant from `WIFI`, so the budget can
/// never admit more internal memory than the heap that was actually added.
pub const INTERNAL_HEAP_MAIN_BYTES: usize = internal_heap_main_bytes(WIFI);

/// PSRAM-only heap. Never the global allocator (see the policy above).
static PSRAM_HEAP: EspHeap = EspHeap::empty();
static SCRATCH_HEAP: EspHeap = EspHeap::empty();
static SCRATCH_READY: AtomicBool = AtomicBool::new(false);
static BUDGET: Mutex<RefCell<MemoryBudget>> =
    Mutex::new(RefCell::new(MemoryBudget::for_build(WIFI)));
static INIT_DONE: AtomicBool = AtomicBool::new(false);

/// DMA buffers are 4-byte aligned on the C61: internal RAM is not cached
/// (`soc.internal_memory_cached = false` in esp-metadata), same value as
/// `esp_alloc::DmaCompatibleInternalMemory` uses for such chips.
const DMA_ALIGN: usize = 4;

// Flash and PSRAM clocks must match bring-up configuration (40 MHz).
const _: () = assert!(FLASH_MHZ == 40 && PSRAM_MHZ == 40);

unsafe extern "C" {
    // ROM cache maintenance, the same functions esp-hal's DMA code calls
    // (`esp_hal::soc::cache_writeback_addr` is private to the HAL).
    fn Cache_WriteBack_Addr(addr: u32, size: u32);
    fn Cache_Invalidate_Addr(addr: u32, size: u32);
}

/// Push `[addr, addr+size)` out of the data cache and drop the cached copy so
/// the next read really comes from the chip.
#[esp_hal::ram]
fn cache_flush(addr: usize, size: usize) {
    // SAFETY: ROM functions on an address range inside the PSRAM window that
    // this module owns while the smoke test runs.
    unsafe {
        Cache_WriteBack_Addr(addr as u32, size as u32);
        Cache_Invalidate_Addr(addr as u32, size as u32);
    }
}

/// Volatile word access to the PSRAM window for `selftest`.
struct PsramWords {
    base: *mut u32,
}

impl WordMem for PsramWords {
    fn write(&mut self, word: usize, value: u32) {
        // SAFETY: `selftest` only addresses words below the length it was
        // given, which is inside the window `init` validated.
        unsafe {
            let p = self.base.add(word);
            p.write_volatile(value);
            cache_flush(p as usize, 4);
        }
    }

    fn read(&mut self, word: usize) -> u32 {
        // SAFETY: as in `write`.
        unsafe { self.base.add(word).read_volatile() }
    }
}

/// Bring PSRAM up (40 MHz flash and RAM), validate and smoke-test it, register
/// it on the PSRAM heap and record the result in the budget. Returns the
/// status; every failure is a `Degraded(..)` status, never a panic. Call once,
/// early, after the internal heap exists. A second call returns the stored
/// status.
pub fn init(peripheral: PSRAM<'static>) -> PsramStatus {
    if INIT_DONE.swap(true, Ordering::AcqRel) {
        return status();
    }

    // esp-hal's default is flash 80 MHz and would re-clock the flash the
    // image header set to 40 MHz (80 MHz flash boot-loops on this board per the
    // BSP README), so both clocks are spelled out.
    let config = PsramConfig {
        flash_frequency: FlashFreq::FlashFreq40m,
        ram_frequency: SpiRamFreq::Freq40m,
        ..PsramConfig::default()
    };
    let psram = Psram::new(peripheral, config);
    let (start, size) = psram.raw_parts();
    // Keep the peripheral token for the whole run. `Psram` has no `Drop` today;
    // the explicit forget keeps that true if esp-hal ever adds one.
    #[allow(clippy::forget_non_drop)]
    core::mem::forget(psram);
    let start = start as usize;

    let status = if let Some(fault) = window_fault(start, size) {
        PsramStatus::Degraded(fault)
    } else if size < PSRAM_MIN_BYTES {
        // 0 = chip id unknown / absent; below the minimum = not worth it.
        evaluate_psram(size, Ok(()))
    } else {
        let usable = size.min(PSRAM_MAX_BYTES);
        let mut mem = PsramWords {
            base: start as *mut u32,
        };
        let test = selftest(&mut mem, usable / 4);
        evaluate_psram(size, test)
    };

    if let PsramStatus::Ready { bytes } = status {
        // SAFETY: the window is exclusively ours (nothing else maps it), it is
        // 'static, non-empty (>= PSRAM_MIN_BYTES) and no allocator has
        // touched it yet; the smoke test above only left test patterns in it.
        unsafe {
            let scratch_bytes = if bytes >= PSRAM_LARGE_MIN_BYTES {
                SCRATCH_ARENA_BYTES
            } else {
                0
            };
            if scratch_bytes != 0 {
                SCRATCH_HEAP.add_region(HeapRegion::new(
                    start as *mut u8,
                    scratch_bytes,
                    MemoryCapability::External.into(),
                ));
                SCRATCH_READY.store(true, Ordering::Release);
            }
            PSRAM_HEAP.add_region(HeapRegion::new(
                (start + scratch_bytes) as *mut u8,
                bytes - scratch_bytes,
                MemoryCapability::External.into(),
            ));
        }
        info!("psram: {} B at {:#010x}, heap registered", bytes, start);
    } else {
        warn!(
            "{} (window {:#010x}+{:#x}); running on internal memory",
            status.describe(),
            start,
            size
        );
    }

    let stored = critical_section::with(|cs| BUDGET.borrow_ref_mut(cs).set_status(status));
    if let Err(e) = stored {
        // Cannot happen at boot (nothing reserved yet); keep the safe default.
        error!("psram: budget refused status {:?}: {:?}", status, e);
        return PsramStatus::NotInitialised;
    }
    status
}

pub fn status() -> PsramStatus {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).status())
}

/// Bytes currently charged to `class` in `region`.
pub fn used(region: Region, class: MemClass) -> usize {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).used(region, class))
}

pub fn peak(region: Region, class: MemClass) -> usize {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).peak(region, class))
}

pub fn pool_used(region: Region) -> usize {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).pool_used(region))
}

pub fn pool_peak(region: Region) -> usize {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).pool_peak(region))
}

pub fn pool_limit(region: Region) -> usize {
    critical_section::with(|cs| BUDGET.borrow_ref(cs).pool_limit(region))
}

/// A budget-charged, zero-initialised block. Dropping it frees the block and
/// returns the charge.
pub struct MemBuf {
    ptr: NonNull<u8>,
    layout: Layout,
    res: Reservation,
}

// SAFETY: the block is exclusively owned and the allocators it came from are
// critical-section protected.
unsafe impl Send for MemBuf {}

// SAFETY: shared references expose initialized bytes only; writes require &mut self.
unsafe impl Sync for MemBuf {}

impl MemBuf {
    pub fn region(&self) -> Region {
        self.res.region
    }

    pub fn class(&self) -> MemClass {
        self.res.class
    }

    pub fn addr(&self) -> usize {
        self.ptr.as_ptr() as usize
    }

    pub fn len(&self) -> usize {
        self.layout.size()
    }

    pub fn is_empty(&self) -> bool {
        self.layout.size() == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `len` bytes, initialised (zeroed at allocation), owned.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.layout.size()) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` is unique.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.layout.size()) }
    }
}

impl Drop for MemBuf {
    fn drop(&mut self) {
        // SAFETY: allocated from the heap matching `res.region` with `layout`.
        unsafe {
            match self.res.region {
                Region::Internal => {
                    GlobalAlloc::dealloc(&esp_alloc::HEAP, self.ptr.as_ptr(), self.layout)
                }
                Region::Psram => GlobalAlloc::dealloc(
                    external_heap(self.res.kind).0,
                    self.ptr.as_ptr(),
                    self.layout,
                ),
            }
        }
        let released = critical_section::with(|cs| BUDGET.borrow_ref_mut(cs).release(self.res));
        if let Err(e) = released {
            error!("memory: release of {:?} refused: {:?}", self.res, e);
        }
    }
}

fn external_heap(kind: AllocationKind) -> (&'static EspHeap, HeapKind) {
    let heap = external_heap_kind(kind, SCRATCH_READY.load(Ordering::Acquire));
    if heap == HeapKind::Scratch {
        (&SCRATCH_HEAP, heap)
    } else {
        (&PSRAM_HEAP, heap)
    }
}

fn alloc_in(
    region: Option<Region>,
    class: MemClass,
    kind: Option<AllocationKind>,
    size: usize,
    align: usize,
) -> Result<MemBuf, MemError> {
    let res = critical_section::with(|cs| {
        let mut b = BUDGET.borrow_ref_mut(cs);
        let region = region.unwrap_or_else(|| b.region_for(class));
        b.reserve_in_with_kind(
            region,
            class,
            kind.unwrap_or_else(|| AllocationKind::for_class(class)),
            size,
            align,
        )
    })?;

    let give_back = |e: MemError| {
        let _ = critical_section::with(|cs| BUDGET.borrow_ref_mut(cs).release(res));
        e
    };

    let layout = Layout::from_size_align(size, align).map_err(|_| give_back(MemError::Overflow))?;
    // SAFETY: `layout` has non-zero size (the budget rejected size 0).
    let raw = unsafe {
        match res.region {
            // Internal capability only: even if an External region were ever
            // added to the global heap, this request cannot land in it.
            Region::Internal => {
                esp_alloc::HEAP.alloc_caps(MemoryCapability::Internal.into(), layout)
            }
            Region::Psram => GlobalAlloc::alloc(external_heap(res.kind).0, layout),
        }
    };
    let Some(ptr) = NonNull::new(raw) else {
        return Err(give_back(MemError::HeapExhausted {
            heap: if res.region == Region::Internal {
                HeapKind::Internal
            } else {
                external_heap(res.kind).1
            },
        }));
    };
    // SAFETY: `size` bytes just allocated and owned.
    unsafe { ptr.as_ptr().write_bytes(0, size) };
    Ok(MemBuf { ptr, layout, res })
}

/// Allocate with the budget's own placement: PSRAM for PSRAM-capable classes
/// while it is `Ready`, internal (with the degraded internal limits) otherwise.
/// Classes that may not live in PSRAM are always internal.
pub fn alloc(class: MemClass, size: usize, align: usize) -> Result<MemBuf, MemError> {
    alloc_in(None, class, None, size, align)
}

pub fn alloc_with_kind(
    class: MemClass,
    kind: AllocationKind,
    size: usize,
    align: usize,
) -> Result<MemBuf, MemError> {
    alloc_in(None, class, Some(kind), size, align)
}

/// Allocate in PSRAM only. Takes `ExternalClass`, which has no DMA / ISR /
/// runtime variant; `Err(ClassLimit { limit: 0, .. })` when PSRAM is not
/// `Ready`.
pub fn alloc_external(class: ExternalClass, size: usize, align: usize) -> Result<MemBuf, MemError> {
    alloc_in(Some(Region::Psram), class.into(), None, size, align)
}

/// Allocate a DMA buffer: internal RAM, `DMA_ALIGN`, and the returned address
/// is re-checked against the internal range before it is handed out.
pub fn alloc_dma(size: usize) -> Result<MemBuf, MemError> {
    let buf = alloc_in(
        Some(Region::Internal),
        MemClass::DmaBuffer,
        None,
        size,
        DMA_ALIGN,
    )?;
    if !range_is_internal(buf.addr(), buf.len()) {
        // dropping `buf` frees it and returns the charge
        return Err(MemError::RegionForbidden {
            class: MemClass::DmaBuffer,
            region: Region::Psram,
        });
    }
    Ok(buf)
}

/// Log class reservations and peaks across all memory classes.
/// Answers "which class ran out" at a glance; `BigBuf` calls it when the budget refuses a request.
/// Never logs secrets.
pub fn log_classes() {
    for class in MemClass::ALL {
        let (region, used, peak, limit) = critical_section::with(|cs| {
            let b = BUDGET.borrow_ref(cs);
            let region = b.region_for(class);
            (
                region,
                b.used(region, class),
                b.peak(region, class),
                b.class_limit(region, class),
            )
        });
        info!(
            "memory class {:<14} {:?}: reserved {} / {} B, peak {} B",
            class.name(),
            region,
            used,
            limit,
            peak
        );
    }
}

/// Physical internal and PSRAM heap usage, stack watermark, and pool reservation peaks.
/// Internal heap: size / free / current / peak via esp-alloc APIs (acknowledging
/// no public largest-free-block API). Stack watermark: high water mark and free margin.
/// Never logs secrets.
pub fn log_usage() {
    let internal_stats = esp_alloc::HEAP.stats();
    let internal_free = internal_stats
        .size
        .saturating_sub(internal_stats.current_usage);
    let psram_stats = PSRAM_HEAP.stats();
    let scratch_stats = SCRATCH_HEAP.stats();
    let psram_size = psram_stats.size + scratch_stats.size;
    let psram_current = psram_stats.current_usage + scratch_stats.current_usage;
    let psram_free = psram_size.saturating_sub(psram_current);
    let (int_used, int_peak, int_limit, psram_used, psram_peak, psram_limit) =
        critical_section::with(|cs| {
            let b = BUDGET.borrow_ref(cs);
            (
                b.pool_used(Region::Internal),
                b.pool_peak(Region::Internal),
                b.pool_limit(Region::Internal),
                b.pool_used(Region::Psram),
                b.pool_peak(Region::Psram),
                b.pool_limit(Region::Psram),
            )
        });
    info!(
        "heap internal: size {} B, free {} B, used {} B, peak {} B (note: no public largest-free-block API); reserved {} / {} B, peak {} B",
        internal_stats.size,
        internal_free,
        internal_stats.current_usage,
        internal_stats.max_usage,
        int_used,
        int_limit,
        int_peak
    );
    info!(
        "heap psram: size {} B, free {} B, used {} B, sum of heap peaks {} B; reserved {} / {} B, peak {} B",
        psram_size,
        psram_free,
        psram_current,
        psram_stats.max_usage + scratch_stats.max_usage,
        psram_used,
        psram_limit,
        psram_peak
    );
    for kind in AllocationKind::ALL {
        let (used, peak) = critical_section::with(|cs| {
            let b = BUDGET.borrow_ref(cs);
            (b.kind_used(kind), b.kind_peak(kind))
        });
        info!(
            "psram {:?}: reserved {} B, peak {} B (HR8 payload limit {} B)",
            kind,
            used,
            peak,
            kind.limit()
        );
    }
    let stack_hwm = crate::ui::stack_high_water_mark();
    let stack_free = crate::ui::free_stack_bytes();
    info!(
        "stack watermark: hwm used {} B, free margin {} B",
        stack_hwm, stack_free
    );
}

/// One log line per region, the class table, heaps, and stack watermark (for
/// bring-up logs). Never logs secrets.
pub fn log_report() {
    info!("{}", status().describe());
    for region in [Region::Internal, Region::Psram] {
        info!(
            "memory {:?}: pool {} / {} B, peak {} B",
            region,
            pool_used(region),
            pool_limit(region),
            pool_peak(region),
        );
    }
    log_classes();
    log_usage();
}

//! Firmware write-operation macros over the real vendored async FAT manager.
//!
//! The manager wrapper only counts calls; block-device failures exercise actual
//! FAT writes, deferred directory updates, handle release, and block-cache reload.
//! A synthetic close error covers seek failure and empty write/append, which leave the
//! real FAT handle clean and therefore cannot trigger a metadata device failure.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use embedded_sdmmc::{
    AsyncBlockDevice, AsyncVolumeManager, Block, BlockCount, BlockIdx, Mode, RawDirectory, RawFile,
    TimeSource, Timestamp, VolumeIdx,
};
use pulp_host::error::{Error, ErrorKind};

include!("../../kernel/src/drivers/storage_write_ops.rs");

const ROOT: u32 = 34;
const DATA: u32 = 35;
const NAME: &str = "STATE.BIN";
const SEED: &[u8] = b"seed";

type FatResult<T> = Result<T, embedded_sdmmc::Error<InjectedFailure>>;
type FatManager = AsyncVolumeManager<MemoryCard, Clock>;

#[derive(Debug, Clone, Copy)]
struct InjectedFailure;

impl std::fmt::Display for InjectedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("injected device failure")
    }
}

impl std::error::Error for InjectedFailure {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Open,
    Seek,
    Write,
    Close,
    Discard,
}

struct CardState {
    blocks: Vec<Block>,
    // Fail the nth write to each specified sector, without changing the disk.
    failures: Vec<(u32, usize)>,
    reads: Vec<u32>,
    writes: Vec<(u32, usize)>,
    write_failed: bool,
}

struct MemoryCard(Rc<RefCell<CardState>>);

impl AsyncBlockDevice for MemoryCard {
    type Error = InjectedFailure;

    async fn read(&mut self, dst: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
        state.reads.push(start.0);
        dst.clone_from_slice(&state.blocks[start.0 as usize..start.0 as usize + dst.len()]);
        Ok(())
    }

    async fn write(&mut self, src: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let mut state = self.0.borrow_mut();
        state.writes.push((start.0, src.len()));
        let mut failed_at = None;
        for (sector, remaining) in &mut state.failures {
            if *sector >= start.0 && *sector < start.0 + src.len() as u32 && *remaining > 0 {
                *remaining -= 1;
                if *remaining == 0 {
                    failed_at = Some((*sector - start.0) as usize);
                    break;
                }
            }
        }
        if let Some(prefix) = failed_at {
            state.blocks[start.0 as usize..start.0 as usize + prefix]
                .clone_from_slice(&src[..prefix]);
            state.write_failed = true;
            return Err(InjectedFailure);
        }
        state.blocks[start.0 as usize..start.0 as usize + src.len()].clone_from_slice(src);
        Ok(())
    }

    async fn num_blocks(&mut self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(self.0.borrow().blocks.len() as u32))
    }
}

struct Clock;

impl TimeSource for Clock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2026, 10, 14, 12, 0, 0).unwrap()
    }
}

struct CountingManager {
    fat: FatManager,
    events: Rc<RefCell<Vec<Event>>>,
    fail_after_close: Cell<bool>,
}

impl CountingManager {
    async fn open_file_in_dir(
        &mut self,
        dir: RawDirectory,
        name: &str,
        mode: Mode,
    ) -> FatResult<RawFile> {
        self.events.borrow_mut().push(Event::Open);
        self.fat.open_file_in_dir(dir, name, mode).await
    }

    fn file_seek_from_start(&mut self, file: RawFile, offset: u32) -> FatResult<()> {
        self.events.borrow_mut().push(Event::Seek);
        self.fat.file_seek_from_start(file, offset)
    }

    async fn write(&mut self, file: RawFile, data: &[u8]) -> FatResult<()> {
        self.events.borrow_mut().push(Event::Write);
        self.fat.write(file, data).await
    }

    async fn close_file(&mut self, file: RawFile) -> FatResult<()> {
        self.events.borrow_mut().push(Event::Close);
        let closed = self.fat.close_file(file).await;
        if self.fail_after_close.replace(false) {
            return Err(embedded_sdmmc::Error::DeviceError(InjectedFailure));
        }
        closed
    }
}

struct Inner {
    mgr: CountingManager,
    card: Rc<RefCell<CardState>>,
    root: RawDirectory,
}

impl Inner {
    // Mirror SdStorageInner's device-failure gate and actual cache invalidation.
    fn discard_block_cache(&mut self) {
        self.mgr.events.borrow_mut().push(Event::Discard);
        let failed = std::mem::take(&mut self.card.borrow_mut().write_failed);
        if failed {
            let _ = self.mgr.fat.device();
        }
    }

    fn new() -> Self {
        Self::with_cluster_sectors(1)
    }

    fn with_cluster_sectors(cluster_sectors: u8) -> Self {
        let total = 8192u32 * cluster_sectors as u32;
        let fat_sectors = 32u16 * cluster_sectors as u16;
        let root_sector = 2 + fat_sectors as usize;
        let data_sector = root_sector + 1;
        let mut blocks = vec![Block::new(); total as usize + 1];
        blocks[0][450] = 0x06;
        blocks[0][454..458].copy_from_slice(&1u32.to_le_bytes());
        blocks[0][458..462].copy_from_slice(&total.to_le_bytes());
        blocks[0][510..512].copy_from_slice(&0xaa55u16.to_le_bytes());
        let boot = &mut blocks[1];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = cluster_sectors;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[17..19].copy_from_slice(&16u16.to_le_bytes());
        if let Ok(small) = u16::try_from(total) {
            boot[19..21].copy_from_slice(&small.to_le_bytes());
        } else {
            boot[32..36].copy_from_slice(&total.to_le_bytes());
        }
        boot[21] = 0xf8;
        boot[22..24].copy_from_slice(&fat_sectors.to_le_bytes());
        boot[510..512].copy_from_slice(&0xaa55u16.to_le_bytes());
        blocks[2][..6].copy_from_slice(&[0xf8, 0xff, 0xff, 0xff, 0xff, 0xff]);
        blocks[root_sector][..11].copy_from_slice(b"STATE   BIN");
        blocks[root_sector][11] = 0x20;
        blocks[root_sector][26..28].copy_from_slice(&2u16.to_le_bytes());
        blocks[root_sector][28..32].copy_from_slice(&(SEED.len() as u32).to_le_bytes());
        blocks[data_sector][..SEED.len()].copy_from_slice(SEED);
        let card = Rc::new(RefCell::new(CardState {
            blocks,
            failures: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            write_failed: false,
        }));
        let mut fat = FatManager::new(MemoryCard(card.clone()), Clock);
        let volume = ready(fat.open_raw_volume(VolumeIdx(0))).unwrap();
        let root = fat.open_root_dir(volume).unwrap();
        Self {
            mgr: CountingManager {
                fat,
                events: Rc::new(RefCell::new(Vec::new())),
                fail_after_close: Cell::new(false),
            },
            card,
            root,
        }
    }

    fn fail_write(&mut self, sector: u32, nth: usize) {
        self.card.borrow_mut().failures.push((sector, nth));
    }

    fn events(&self) -> Vec<Event> {
        self.mgr.events.borrow().clone()
    }

    fn assert_released(&mut self) {
        // Reopening the same file verifies real handle release even after flush
        // failure; the wrapper event count checks close was attempted once.
        assert_eq!(
            self.events().iter().filter(|&&e| e == Event::Close).count(),
            1
        );
        let file = ready(
            self.mgr
                .fat
                .open_file_in_dir(self.root, NAME, Mode::ReadOnly),
        )
        .unwrap();
        ready(self.mgr.fat.close_file(file)).unwrap();
    }

    fn contents(&mut self) -> Vec<u8> {
        let file = ready(
            self.mgr
                .fat
                .open_file_in_dir(self.root, NAME, Mode::ReadOnly),
        )
        .unwrap();
        let mut bytes = vec![0; self.mgr.fat.file_length(file).unwrap() as usize];
        assert_eq!(
            ready(self.mgr.fat.read(file, &mut bytes)).unwrap(),
            bytes.len()
        );
        ready(self.mgr.fat.close_file(file)).unwrap();
        bytes
    }

    fn assert_root_cache_reloaded(&mut self) {
        let before = self
            .card
            .borrow()
            .reads
            .iter()
            .filter(|&&s| s == ROOT)
            .count();
        let entry = ready(self.mgr.fat.find_directory_entry(self.root, NAME)).unwrap();
        let after = self
            .card
            .borrow()
            .reads
            .iter()
            .filter(|&&s| s == ROOT)
            .count();
        assert_eq!(
            after,
            before + 1,
            "failed metadata must be read from disk again"
        );
        let persisted_size = u32::from_le_bytes(
            self.card.borrow().blocks[ROOT as usize][28..32]
                .try_into()
                .unwrap(),
        );
        assert_eq!(entry.size, persisted_size);
        assert!(!self.card.borrow().write_failed);
    }
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Write,
    Append,
    Positioned,
}

const OPERATIONS: [Operation; 3] = [Operation::Write, Operation::Append, Operation::Positioned];

impl Operation {
    fn run(self, inner: &mut Inner, offset: u32, data: &[u8]) -> Result<(), Error> {
        let dir = inner.root;
        ready(async {
            match self {
                Self::Write => op_write!(inner, dir, NAME, data),
                Self::Append => op_append!(inner, dir, NAME, data),
                Self::Positioned => op_write_at!(inner, dir, NAME, offset, data),
            }
        })
    }

    fn source(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Append => "append",
            Self::Positioned => "write_at",
        }
    }

    fn close_source(self) -> &'static str {
        match self {
            Self::Write => "write:close",
            Self::Append => "append:close",
            Self::Positioned => "write_at:close",
        }
    }

    fn close_write_number(self) -> usize {
        if matches!(self, Self::Write) { 2 } else { 1 }
    }
}

fn ready<T>(future: impl Future<Output = T>) -> T {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("in-memory FAT I/O must complete in one poll"),
    }
}

fn assert_error(result: Result<(), Error>, kind: ErrorKind, source: &str) {
    let error = result.unwrap_err();
    assert_eq!(error.kind(), kind);
    assert_eq!(error.source_tag(), source);
}

#[test]
fn successful_write_append_and_positioned_write_persist_and_close_once() {
    for (operation, expected) in [
        (Operation::Write, &b"xy"[..]),
        (Operation::Append, &b"seedxy"[..]),
        (Operation::Positioned, &b"sxyd"[..]),
    ] {
        let mut inner = Inner::new();
        operation.run(&mut inner, 1, b"xy").unwrap();
        assert!(!inner.events().contains(&Event::Discard));
        inner.assert_released();
        assert_eq!(inner.contents(), expected, "{operation:?}");
    }
}

#[test]
fn deferred_metadata_failure_is_reported_and_cache_is_discarded() {
    for operation in OPERATIONS {
        let mut inner = Inner::new();
        inner.fail_write(ROOT, operation.close_write_number());
        assert_error(
            operation.run(&mut inner, 1, b"xy"),
            ErrorKind::WriteFailed,
            operation.close_source(),
        );
        assert!(inner.events().ends_with(&[Event::Close, Event::Discard]));
        inner.assert_root_cache_reloaded();
        inner.assert_released();
    }
}

#[test]
fn write_failure_is_preserved_when_metadata_close_also_fails() {
    for operation in OPERATIONS {
        let mut inner = Inner::new();
        inner.fail_write(DATA, 1);
        inner.fail_write(ROOT, operation.close_write_number());
        assert_error(
            operation.run(&mut inner, 1, b"xy"),
            ErrorKind::WriteFailed,
            operation.source(),
        );
        assert!(inner.events().ends_with(&[
            Event::Write,
            Event::Discard,
            Event::Close,
            Event::Discard
        ]));
        inner.assert_root_cache_reloaded();
        inner.assert_released();
    }
}

#[test]
fn write_failure_still_discards_cache_and_closes_once_when_close_succeeds() {
    for operation in OPERATIONS {
        let mut inner = Inner::new();
        inner.fail_write(DATA, 1);
        assert_error(
            operation.run(&mut inner, 1, b"xy"),
            ErrorKind::WriteFailed,
            operation.source(),
        );
        assert!(
            inner
                .events()
                .ends_with(&[Event::Write, Event::Discard, Event::Close])
        );
        assert!(!inner.card.borrow().write_failed);
        inner.assert_released();
    }
}

#[test]
fn seek_failure_is_preserved_and_close_is_attempted_once() {
    for fail_close in [false, true] {
        let mut inner = Inner::new();
        inner.mgr.fail_after_close.set(fail_close);
        assert_error(
            Operation::Positioned.run(&mut inner, 5, b"xy"),
            ErrorKind::SeekFailed,
            "write_at",
        );
        let mut expected = vec![Event::Open, Event::Seek, Event::Discard, Event::Close];
        if fail_close {
            expected.push(Event::Discard);
        }
        assert_eq!(inner.events(), expected);
        inner.assert_released();
        assert_eq!(inner.contents(), SEED);
    }
}

#[test]
fn empty_input_preserves_operation_semantics_and_closes_once() {
    for operation in OPERATIONS {
        let mut inner = Inner::new();
        operation.run(&mut inner, 1, b"").unwrap();
        inner.assert_released();
        let expected = if matches!(operation, Operation::Write) {
            &b""[..]
        } else {
            SEED
        };
        assert_eq!(inner.contents(), expected);
        assert!(!inner.events().contains(&Event::Discard));
    }
}

#[test]
fn empty_input_still_reports_close_failure() {
    for operation in OPERATIONS {
        let mut inner = Inner::new();
        if !matches!(operation, Operation::Positioned) {
            // Empty write/append leave a clean handle: truncate already wrote
            // its metadata at open. Inject close after real handle release.
            inner.mgr.fail_after_close.set(true);
        } else {
            inner.fail_write(ROOT, operation.close_write_number());
        }
        assert_error(
            operation.run(&mut inner, 1, b""),
            ErrorKind::WriteFailed,
            operation.close_source(),
        );
        assert!(inner.events().ends_with(&[Event::Close, Event::Discard]));
        if matches!(operation, Operation::Positioned) {
            inner.assert_root_cache_reloaded();
        }
        inner.assert_released();
    }
}

#[test]
fn open_failure_does_not_attempt_close() {
    let mut inner = Inner::new();
    inner.fail_write(ROOT, 1); // truncate's initial directory write fails
    assert_error(
        Operation::Write.run(&mut inner, 1, b"xy"),
        ErrorKind::OpenFile,
        "write",
    );
    assert_eq!(inner.events(), [Event::Open]);
}

#[test]
fn aligned_cluster_write_is_one_bounded_device_run() {
    let mut inner = Inner::with_cluster_sectors(8);
    let data = vec![0x5a; 4096];
    Operation::Positioned.run(&mut inner, 0, &data).unwrap();
    let runs: Vec<_> = inner
        .card
        .borrow()
        .writes
        .iter()
        .filter(|(_, len)| *len > 1)
        .copied()
        .collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1, 8);
    assert_eq!(inner.contents(), data);
}

#[test]
fn partial_device_run_failure_keeps_old_length_and_retry_reloads_cache() {
    let mut inner = Inner::with_cluster_sectors(8);
    let data = vec![0x6a; 4096];
    inner.fail_write(259 + 3, 1);
    assert_error(
        Operation::Positioned.run(&mut inner, 0, &data),
        ErrorKind::WriteFailed,
        "write_at",
    );
    assert_eq!(inner.contents().len(), SEED.len());
    inner.mgr.events.borrow_mut().clear();
    Operation::Positioned.run(&mut inner, 0, &data).unwrap();
    assert_eq!(inner.contents(), data);
    inner.assert_released();
}

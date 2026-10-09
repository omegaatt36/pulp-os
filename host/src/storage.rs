// virtual SD card for host tests: the function set of
// kernel/src/drivers/storage.rs (minus the `sd` argument, plus interior
// mutability) over a memory or a host-directory backend, with a read log
// (virtual storage) and one-shot short-read / error injection (storage failure injection).
//
// One implementation: backends (storage/fs.rs) only store file contents; path
// rules, error kinds, the read counter and injections live here, so both
// backends behave identically. Error kinds follow the firmware: a missing
// file is OpenFile for reads and sizes, DeleteFailed for deletes; a missing
// directory is OpenDir; a read offset beyond the end is SeekFailed.

mod fs;

use std::cell::RefCell;
use std::path::Path;

pub use crate::dir_entry::DirEntry;
use crate::dir_entry::{PULP_DIR, is_listed_name};
use crate::error::{Error, ErrorKind};
use fs::{Fs, HostFs, MemFs};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRecord {
    pub path: String,
    pub offset: u32,
    pub requested: usize,
    pub returned: usize,
    pub outcome: ReadOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    Ok,
    ShortInjected,
    ErrorInjected(ErrorKind),
    Failed(ErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOp {
    Read,
    Write,
    Append,
    Delete,
    FileSize,
    List,
}

#[derive(Clone, Copy)]
enum Action {
    Short(usize),
    Error(ErrorKind),
}

struct Injection {
    op: StorageOp,
    path: String,
    // matching calls left until it fires (1 = the next one)
    remaining: usize,
    action: Action,
}

struct State {
    fs: Box<dyn Fs>,
    log: Vec<ReadRecord>,
    opens: usize,
    held: usize,
    injections: Vec<Injection>,
}

impl State {
    // advance every injection matching (op, path); at most one fires per call,
    // a second one that is also due stays pending for the next matching call
    fn fire(&mut self, op: StorageOp, path: &str) -> Option<Action> {
        let mut fired = None;
        self.injections.retain_mut(|inj| {
            if inj.op != op || inj.path != path {
                return true;
            }
            if inj.remaining > 1 {
                inj.remaining -= 1;
                return true;
            }
            if fired.is_some() {
                return true;
            }
            fired = Some(inj.action);
            false
        });
        fired
    }
}

// where an operation lands: the directory that must exist and the normalized
// file path (also the key for the read log and injections)
struct Target {
    dir: String,
    path: String,
}

impl Target {
    fn root(name: &str) -> Self {
        Self {
            dir: String::new(),
            path: name.to_string(),
        }
    }

    fn in_dir(dir: &str, name: &str) -> Self {
        Self {
            dir: dir.to_string(),
            path: format!("{dir}/{name}"),
        }
    }

    fn pulp(name: &str) -> Self {
        Self::in_dir(PULP_DIR, name)
    }

    fn pulp_sub(dir: &str, name: &str) -> Self {
        Self::in_dir(&format!("{PULP_DIR}/{dir}"), name)
    }
}

fn check_dir(fs: &dyn Fs, t: &Target) -> Result<()> {
    if fs.is_dir(&t.dir) {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::OpenDir, "in_dir"))
    }
}

// the read itself, after injected errors; `short` caps the returned length
fn read_natural(
    fs: &dyn Fs,
    t: &Target,
    tag: &'static str,
    offset: u32,
    buf: &mut [u8],
    short: Option<usize>,
) -> Result<(u32, usize, bool)> {
    check_dir(fs, t)?;
    let size = fs
        .len(&t.path)
        .ok_or(Error::new(ErrorKind::OpenFile, tag))?;
    if offset > size {
        return Err(Error::new(ErrorKind::SeekFailed, tag));
    }
    let normal = buf.len().min((size - offset) as usize);
    let take = short.map_or(normal, |s| s.min(normal));
    let n = fs
        .read_at(&t.path, offset, &mut buf[..take])
        .map_err(|_| Error::new(ErrorKind::ReadFailed, tag))?;
    Ok((size, n, take < normal))
}

fn file_size(fs: &mut dyn Fs, t: &Target) -> Result<u32> {
    check_dir(fs, t)?;
    fs.len(&t.path)
        .ok_or(Error::new(ErrorKind::OpenFile, "file_size"))
}

fn write(fs: &mut dyn Fs, t: &Target, data: &[u8]) -> Result<()> {
    check_dir(fs, t)?;
    fs.write(&t.path, data)
        .map_err(|_| Error::new(ErrorKind::OpenFile, "write"))
}

fn append(fs: &mut dyn Fs, t: &Target, data: &[u8]) -> Result<()> {
    check_dir(fs, t)?;
    fs.append(&t.path, data)
        .map_err(|_| Error::new(ErrorKind::OpenFile, "append"))
}

fn delete(fs: &mut dyn Fs, t: &Target) -> Result<()> {
    check_dir(fs, t)?;
    fs.remove(&t.path)
        .map_err(|_| Error::new(ErrorKind::DeleteFailed, "delete"))
}

// firmware opens with create-or-append, so a missing file is created; seeking
// beyond the end fails
fn write_at(fs: &mut dyn Fs, t: &Target, offset: u32, data: &[u8]) -> Result<()> {
    check_dir(fs, t)?;
    if fs.len(&t.path).is_none() {
        fs.append(&t.path, &[])
            .map_err(|_| Error::new(ErrorKind::OpenFile, "write_at"))?;
    }
    let size = fs.len(&t.path).unwrap_or(0);
    if offset > size {
        return Err(Error::new(ErrorKind::SeekFailed, "write_at"));
    }
    fs.write_at(&t.path, offset, data)
        .map_err(|_| Error::new(ErrorKind::WriteFailed, "write_at"))
}

// create `path` under `parent` (which must exist); already existing is Ok
fn ensure(fs: &mut dyn Fs, parent: &str, path: &str, tag: &'static str) -> Result<()> {
    if !fs.is_dir(parent) {
        return Err(Error::new(ErrorKind::OpenDir, "in_dir"));
    }
    if fs.is_dir(path) {
        return Ok(());
    }
    fs.mkdir(path)
        .map_err(|_| Error::new(ErrorKind::WriteFailed, tag))
}

// firmware filter (dir_entry::is_listed_name): files only, no directories
fn list_root(fs: &dyn Fs, buf: &mut [DirEntry]) -> Result<usize> {
    let mut entries = fs
        .list("")
        .map_err(|_| Error::new(ErrorKind::ReadFailed, "list_root_files"))?;
    entries.sort();
    let mut count = 0;
    for (name, is_dir, size) in entries {
        let bytes = name.as_bytes();
        // names beyond the 13-byte 8.3 slot cannot exist on a FAT card
        if is_dir || bytes.len() > 13 || !is_listed_name(bytes) {
            continue;
        }
        if count == buf.len() {
            break;
        }
        let mut entry = DirEntry::EMPTY;
        entry.name[..bytes.len()].copy_from_slice(bytes);
        entry.name_len = bytes.len() as u8;
        entry.size = size;
        buf[count] = entry;
        count += 1;
    }
    Ok(count)
}

pub struct VirtualStorage {
    state: RefCell<State>,
}

impl VirtualStorage {
    fn with_fs(fs: impl Fs + 'static) -> Self {
        Self {
            state: RefCell::new(State {
                fs: Box::new(fs),
                log: Vec::new(),
                opens: 0,
                held: 0,
                injections: Vec::new(),
            }),
        }
    }

    // empty in-memory card
    pub fn memory() -> Self {
        Self::with_fs(MemFs::default())
    }

    // in-memory card seeded from (path, bytes); directories are implied
    pub fn memory_with(files: &[(&str, &[u8])]) -> Self {
        Self::with_fs(MemFs::with_files(files))
    }

    // the existing host directory `root` is the SD root (not deleted on drop)
    pub fn host_dir(root: &Path) -> Self {
        Self::with_fs(HostFs::new(root))
    }

    // -- read counter ------------------------------------------------------

    pub fn read_count(&self) -> usize {
        self.state.borrow().log.len()
    }

    pub fn read_log(&self) -> Vec<ReadRecord> {
        self.state.borrow().log.clone()
    }

    // scoped opens of a _PULP/<dir>/<name> file (with_pulp_subdir_file)
    pub fn open_count(&self) -> usize {
        self.state.borrow().opens
    }

    // pack files held open right now (with_pulp_subdir_file closures running);
    // 0 whenever control is outside a storage call
    pub fn held_handles(&self) -> usize {
        self.state.borrow().held
    }

    pub(crate) fn note_open(&self) {
        let mut st = self.state.borrow_mut();
        st.opens += 1;
        st.held += 1;
    }

    pub(crate) fn note_close(&self) {
        let mut st = self.state.borrow_mut();
        st.held = st.held.saturating_sub(1);
    }

    // clears counts and log only; pending injections are kept
    pub fn reset_reads(&self) {
        let mut st = self.state.borrow_mut();
        st.log.clear();
        st.opens = 0;
    }

    // -- injection -----------------------------------------------------------

    // the `nth` (1-based) read of `path` made from now on returns only
    // `returned` bytes (as Ok)
    pub fn inject_short_read(&self, path: &str, nth: usize, returned: usize) {
        self.register(StorageOp::Read, path, nth, Action::Short(returned));
    }

    // the `nth` (1-based) `op` call on `path` made from now on fails with `kind`
    // and has no effect on the storage
    pub fn inject_error(&self, op: StorageOp, path: &str, nth: usize, kind: ErrorKind) {
        self.register(op, path, nth, Action::Error(kind));
    }

    fn register(&self, op: StorageOp, path: &str, nth: usize, action: Action) {
        assert!(nth >= 1, "injection nth is 1-based");
        self.state.borrow_mut().injections.push(Injection {
            op,
            path: path.to_string(),
            remaining: nth,
            action,
        });
    }

    pub fn pending_injections(&self) -> usize {
        self.state.borrow().injections.len()
    }

    pub fn clear_injections(&self) {
        self.state.borrow_mut().injections.clear();
    }

    // -- dispatch ------------------------------------------------------------

    // every read variant: log it, honour injections, then the natural read
    fn read(&self, t: &Target, start: bool, offset: u32, buf: &mut [u8]) -> Result<(u32, usize)> {
        let tag = if start { "read_start" } else { "read_chunk" };
        let offset = if start { 0 } else { offset };
        let requested = buf.len();
        let mut st = self.state.borrow_mut();
        let st = &mut *st;

        let (outcome, returned, result) = match st.fire(StorageOp::Read, &t.path) {
            Some(Action::Error(kind)) => (
                ReadOutcome::ErrorInjected(kind),
                0,
                Err(Error::new(kind, "injected")),
            ),
            fired => {
                let short = match fired {
                    Some(Action::Short(n)) => Some(n),
                    _ => None,
                };
                match read_natural(&*st.fs, t, tag, offset, buf, short) {
                    Ok((size, n, shortened)) => {
                        let outcome = if shortened {
                            ReadOutcome::ShortInjected
                        } else {
                            ReadOutcome::Ok
                        };
                        (outcome, n, Ok((size, n)))
                    }
                    Err(e) => (ReadOutcome::Failed(e.kind()), 0, Err(e)),
                }
            }
        };
        st.log.push(ReadRecord {
            path: t.path.clone(),
            offset,
            requested,
            returned,
            outcome,
        });
        result
    }

    // every non-read operation: injected error first (no effect), else `f`
    fn run<T>(
        &self,
        op: StorageOp,
        t: &Target,
        f: impl FnOnce(&mut dyn Fs, &Target) -> Result<T>,
    ) -> Result<T> {
        let mut st = self.state.borrow_mut();
        if let Some(Action::Error(kind)) = st.fire(op, &t.path) {
            return Err(Error::new(kind, "injected"));
        }
        f(&mut *st.fs, t)
    }

    // directory creation is not an injectable class
    fn ensure_dir_at(&self, parent: &str, path: &str, tag: &'static str) -> Result<()> {
        ensure(&mut *self.state.borrow_mut().fs, parent, path, tag)
    }

    fn size(&self, t: &Target) -> Result<u32> {
        self.run(StorageOp::FileSize, t, file_size)
    }

    fn write_to(&self, t: &Target, data: &[u8]) -> Result<()> {
        self.run(StorageOp::Write, t, |fs, t| write(fs, t, data))
    }

    fn append_to(&self, t: &Target, data: &[u8]) -> Result<()> {
        self.run(StorageOp::Append, t, |fs, t| append(fs, t, data))
    }

    fn delete_at(&self, t: &Target) -> Result<()> {
        self.run(StorageOp::Delete, t, delete)
    }

    // -- root file operations ---------------------------------------------

    pub fn file_size(&self, name: &str) -> Result<u32> {
        self.size(&Target::root(name))
    }

    pub fn read_file_chunk(&self, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize> {
        self.read(&Target::root(name), false, offset, buf)
            .map(|(_, n)| n)
    }

    pub fn read_file_start(&self, name: &str, buf: &mut [u8]) -> Result<(u32, usize)> {
        self.read(&Target::root(name), true, 0, buf)
    }

    pub fn write_file(&self, name: &str, data: &[u8]) -> Result<()> {
        self.write_to(&Target::root(name), data)
    }

    pub fn append_root_file(&self, name: &str, data: &[u8]) -> Result<()> {
        self.append_to(&Target::root(name), data)
    }

    pub fn delete_file(&self, name: &str) -> Result<()> {
        self.delete_at(&Target::root(name))
    }

    pub fn list_root_files(&self, buf: &mut [DirEntry]) -> Result<usize> {
        self.run(StorageOp::List, &Target::root(""), |fs, _| {
            list_root(&*fs, buf)
        })
    }

    pub fn ensure_dir(&self, name: &str) -> Result<()> {
        self.ensure_dir_at("", name, "ensure_dir")
    }

    // -- single-directory file operations ---------------------------------

    pub fn write_file_in_dir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()> {
        self.write_to(&Target::in_dir(dir, name), data)
    }

    pub fn append_file_in_dir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()> {
        self.append_to(&Target::in_dir(dir, name), data)
    }

    pub fn read_file_chunk_in_dir(
        &self,
        dir: &str,
        name: &str,
        offset: u32,
        buf: &mut [u8],
    ) -> Result<usize> {
        self.read(&Target::in_dir(dir, name), false, offset, buf)
            .map(|(_, n)| n)
    }

    pub fn read_file_start_in_dir(
        &self,
        dir: &str,
        name: &str,
        buf: &mut [u8],
    ) -> Result<(u32, usize)> {
        self.read(&Target::in_dir(dir, name), true, 0, buf)
    }

    // -- _PULP/<dir>/ operations -------------------------------------------

    pub fn ensure_pulp_dir(&self) -> Result<()> {
        self.ensure_dir_at("", PULP_DIR, "ensure_pulp_dir")
    }

    pub fn ensure_pulp_subdir(&self, name: &str) -> Result<()> {
        self.ensure_dir_at(
            PULP_DIR,
            &format!("{PULP_DIR}/{name}"),
            "ensure_pulp_subdir",
        )
    }

    pub fn write_in_pulp_subdir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()> {
        self.write_to(&Target::pulp_sub(dir, name), data)
    }

    pub fn append_in_pulp_subdir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()> {
        self.append_to(&Target::pulp_sub(dir, name), data)
    }

    pub fn read_chunk_in_pulp_subdir(
        &self,
        dir: &str,
        name: &str,
        offset: u32,
        buf: &mut [u8],
    ) -> Result<usize> {
        self.read(&Target::pulp_sub(dir, name), false, offset, buf)
            .map(|(_, n)| n)
    }

    pub fn optional_file_size_in_pulp_subdir(&self, dir: &str, name: &str) -> Result<Option<u32>> {
        self.run(
            StorageOp::FileSize,
            &Target::pulp_sub(dir, name),
            |fs, t| {
                // Natural absence is distinct from the errors injected by run.
                Ok(fs.len(&t.path))
            },
        )
    }

    pub fn file_size_in_pulp_subdir(&self, dir: &str, name: &str) -> Result<u32> {
        self.size(&Target::pulp_sub(dir, name))
    }

    pub fn delete_in_pulp_subdir(&self, dir: &str, name: &str) -> Result<()> {
        self.delete_at(&Target::pulp_sub(dir, name))
    }

    // -- _PULP/ direct file operations -------------------------------------

    pub fn read_chunk_in_pulp(&self, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize> {
        self.read(&Target::pulp(name), false, offset, buf)
            .map(|(_, n)| n)
    }

    pub fn write_in_pulp(&self, name: &str, data: &[u8]) -> Result<()> {
        self.write_to(&Target::pulp(name), data)
    }

    pub fn append_in_pulp(&self, name: &str, data: &[u8]) -> Result<()> {
        self.append_to(&Target::pulp(name), data)
    }

    pub fn file_size_in_pulp(&self, name: &str) -> Result<u32> {
        self.size(&Target::pulp(name))
    }

    pub fn delete_in_pulp(&self, name: &str) -> Result<()> {
        self.delete_at(&Target::pulp(name))
    }

    pub fn write_at_in_pulp(&self, name: &str, offset: u32, data: &[u8]) -> Result<()> {
        self.run(StorageOp::Write, &Target::pulp(name), |fs, t| {
            write_at(fs, t, offset, data)
        })
    }
}

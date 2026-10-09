// sd card file operations
//
// all I/O through embedded-sdmmc AsyncVolumeManager; functions are
// synchronous, wrapping async ops with poll_once (SPI bus is blocking
// so every .await resolves immediately)
//
// returns the unified Error type (re-exported as StorageError for
// backward compat); apps receive it through KernelHandle

use core::ops::ControlFlow;

use embedded_sdmmc::{Mode, RawFile};

use crate::drivers::dir_entry::{DirEntry, TITLE_CAP, is_listed_name, title_line};
pub use crate::drivers::dir_entry::{DirPage, PULP_DIR, TITLES_FILE};
use crate::drivers::sdcard::{
    HELD_KEY_LEN, HeldFile, SCOPE_PULP, SCOPE_ROOT, SCOPE_SUB, SdStorage, SdStorageInner,
    StorageBorrow, WriterHandle, poll_once,
};
use crate::error::{Error, ErrorKind};
use crate::util::CloseError;
use pulp_board_logic::font_index::FONT_SOURCE;
use pulp_board_logic::upload::next_file_len;
use pulp_fontpack::PACK_DIR;

fn font_path_changed(dir: &str) {
    if dir == PACK_DIR {
        FONT_SOURCE.bump();
    }
}

// backward-compatible alias
pub type StorageError = Error;

// build "NAME.EXT" bytes from a ShortFileName

fn sfn_to_bytes(name: &embedded_sdmmc::ShortFileName, out: &mut [u8; 13]) -> u8 {
    let base = name.base_name();
    let ext = name.extension();
    let mut pos = 0usize;
    let blen = base.len().min(8);
    out[..blen].copy_from_slice(&base[..blen]);
    pos += blen;
    if !ext.is_empty() {
        out[pos] = b'.';
        pos += 1;
        let elen = ext.len().min(3);
        out[pos..pos + elen].copy_from_slice(&ext[..elen]);
        pos += elen;
    }
    pos as u8
}

// file-operation macros; each evaluates to Result<T, Error>
// none use ? internally so caller cleanup is never bypassed

macro_rules! op_file_size {
    ($inner:expr, $dir:expr, $name:expr) => {
        $inner
            .mgr
            .find_directory_entry($dir, $name)
            .await
            .map(|e| e.size)
            .map_err(|_| Error::new(ErrorKind::OpenFile, "file_size"))
    };
}

macro_rules! op_read_chunk {
    ($inner:expr, $dir:expr, $name:expr, $offset:expr, $buf:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadOnly)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "read_chunk")),
            Ok(file) => {
                let result = match $inner.mgr.file_seek_from_start(file, $offset) {
                    Ok(()) => $inner
                        .mgr
                        .read(file, $buf)
                        .await
                        .map_err(|_| Error::new(ErrorKind::ReadFailed, "read_chunk")),
                    Err(_) => Err(Error::new(ErrorKind::SeekFailed, "read_chunk")),
                };
                let _ = $inner.mgr.close_file(file).await;
                result
            }
        }
    };
}

macro_rules! op_read_start {
    ($inner:expr, $dir:expr, $name:expr, $buf:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadOnly)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "read_start")),
            Ok(file) => {
                let size = $inner.mgr.file_length(file).unwrap_or(0);
                let result = $inner
                    .mgr
                    .read(file, $buf)
                    .await
                    .map_err(|_| Error::new(ErrorKind::ReadFailed, "read_start"));
                let _ = $inner.mgr.close_file(file).await;
                result.map(|n| (size, n))
            }
        }
    };
}

macro_rules! op_write {
    ($inner:expr, $dir:expr, $name:expr, $data:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadWriteCreateOrTruncate)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "write")),
            Ok(file) => {
                let result = if ($data).is_empty() {
                    Ok(())
                } else {
                    $inner
                        .mgr
                        .write(file, $data)
                        .await
                        .map_err(|_| Error::new(ErrorKind::WriteFailed, "write"))
                };
                if result.is_err() {
                    $inner.discard_block_cache();
                }
                let _ = $inner.mgr.close_file(file).await;
                result
            }
        }
    };
}

macro_rules! op_append {
    ($inner:expr, $dir:expr, $name:expr, $data:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadWriteCreateOrAppend)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "append")),
            Ok(file) => {
                let result = if ($data).is_empty() {
                    Ok(())
                } else {
                    $inner
                        .mgr
                        .write(file, $data)
                        .await
                        .map_err(|_| Error::new(ErrorKind::WriteFailed, "append"))
                };
                if result.is_err() {
                    $inner.discard_block_cache();
                }
                let _ = $inner.mgr.close_file(file).await;
                result
            }
        }
    };
}

macro_rules! op_delete {
    ($inner:expr, $dir:expr, $name:expr) => {{
        $inner
            .mgr
            .delete_entry_in_dir($dir, $name)
            .await
            .map_err(|_| Error::new(ErrorKind::DeleteFailed, "delete"))
    }};
}

// the _PULP directory handle: opened on first use, then kept (and never closed
// by the scoping macros below), so no operation walks the root directory again
macro_rules! pulp_dir {
    ($inner:expr) => {
        match $inner.pulp {
            Some(_d) => Ok(_d),
            None => match $inner.mgr.open_dir($inner.root, PULP_DIR).await {
                Ok(_d) => {
                    $inner.pulp = Some(_d);
                    Ok(_d)
                }
                Err(e) => Err(e),
            },
        }
    };
}

// dir-scoping macros; open subdir, execute body, close handle (the kept _PULP
// handle is used and left open)

macro_rules! in_dir {
    ($inner:expr, $dirname:expr, |$dir:ident| $body:expr) => {{
        let _kept = $dirname == PULP_DIR;
        let _opened = if _kept {
            pulp_dir!($inner)
        } else {
            $inner.mgr.open_dir($inner.root, $dirname).await
        };
        match _opened {
            Err(_) => Err(Error::new(ErrorKind::OpenDir, "in_dir")),
            Ok($dir) => {
                let _r = $body;
                if !_kept {
                    let _ = $inner.mgr.close_dir($dir);
                }
                _r
            }
        }
    }};
}

// first level is always _PULP
macro_rules! in_subdir {
    ($inner:expr, $d1:expr, $d2:expr, |$dir:ident| $body:expr) => {
        match pulp_dir!($inner) {
            Err(_) => Err(Error::new(ErrorKind::OpenDir, "in_subdir")),
            Ok(_mid) => match $inner.mgr.open_dir(_mid, $d2).await {
                Err(_) => Err(Error::new(ErrorKind::OpenDir, "in_subdir")),
                Ok($dir) => {
                    let _r = $body;
                    let _ = $inner.mgr.close_dir($dir);
                    _r
                }
            },
        }
    };
}

fn borrow(sd: &SdStorage) -> core::result::Result<StorageBorrow<'_>, Error> {
    let mut inner = sd
        .borrow_inner()
        .ok_or(Error::new(ErrorKind::NoCard, "storage::borrow"))?;
    // a write that failed earlier (close/flush paths included) must not leave
    // its modified block in the volume manager's cache
    inner.discard_block_cache();
    Ok(inner)
}

// root file operations

pub fn file_size(sd: &SdStorage, name: &str) -> crate::error::Result<u32> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let (slot, file) = held_open(inner, SCOPE_ROOT, name, "file_size").await?;
        match inner.mgr.file_length(file) {
            Ok(len) => Ok(len),
            Err(_) => {
                release_held(inner, slot).await;
                Err(Error::new(ErrorKind::OpenFile, "file_size"))
            }
        }
    })
}

pub fn read_file_chunk(
    sd: &SdStorage,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> crate::error::Result<usize> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        read_held_chunk(inner, SCOPE_ROOT, name, offset, buf).await
    })
}

pub fn read_file_start(
    sd: &SdStorage,
    name: &str,
    buf: &mut [u8],
) -> crate::error::Result<(u32, usize)> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let (slot, file) = held_open(inner, SCOPE_ROOT, name, "read_start").await?;
        let size = inner.mgr.file_length(file).unwrap_or(0);
        let result = match inner.mgr.file_seek_from_start(file, 0) {
            Ok(()) => inner
                .mgr
                .read(file, buf)
                .await
                .map_err(|_| Error::new(ErrorKind::ReadFailed, "read_start")),
            Err(_) => Err(Error::new(ErrorKind::SeekFailed, "read_start")),
        };
        if result.is_err() {
            release_held(inner, slot).await;
        }
        result.map(|n| (size, n))
    })
}

pub fn write_file(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_ROOT, "", name).await;
        op_write!(inner, inner.root, name, data)
    })
}

pub fn append_root_file(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_ROOT, "", name).await;
        op_append!(inner, inner.root, name, data)
    })
}

// one root file open for writing across many short storage borrows (an upload)
//
// `create_root_writer` opens it once, create-or-truncate; every `append` then
// borrows the storage only for the duration of its write, never across an
// await of the caller, so reads and listings stay possible in between. The
// raw handle is closed exactly once: by `close` (which reports a failed
// metadata flush), or by Drop on an error, timeout or dropped future. The
// volume manager removes the handle even when that flush fails, so a failed
// close never leaks a file slot. Not Clone: the handle has one owner.
//
// The token borrows the `SdStorage`, so the card cannot be replaced
// (`Kernel::replace_storage` needs `&mut Kernel`) while it is alive; a card
// that fails meanwhile makes the writes and the close return errors.
pub struct RootWriter<'a> {
    sd: &'a SdStorage,
    file: WriterHandle<'a>,
    len: u32,
}

pub fn create_root_writer<'a>(
    sd: &'a SdStorage,
    name: &str,
) -> crate::error::Result<RootWriter<'a>> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let mut file_owner = sd
            .reserve_writer()
            .ok_or(Error::new(ErrorKind::OpenFile, "write: writer slots full"))?;
        // FAT refuses to open a file twice: let go of a held reader of `name`
        release_held_named(inner, SCOPE_ROOT, "", name).await;
        match inner
            .mgr
            .open_file_in_dir(inner.root, name, Mode::ReadWriteCreateOrTruncate)
            .await
        {
            Ok(file) => {
                // A fresh reservation always accepts its first handle. If
                // that invariant changes, close the unaccepted raw handle
                // while still holding the manager rather than abandon it.
                if let Err(file) = file_owner.attach(file) {
                    let _ = inner.mgr.close_file(file).await;
                    inner.discard_block_cache();
                    return Err(Error::new(ErrorKind::OpenFile, "write: writer ownership"));
                }
                Ok(RootWriter {
                    sd,
                    file: file_owner,
                    len: 0,
                })
            }
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "write")),
        }
    })
}

impl RootWriter<'_> {
    /// Bytes appended so far.
    pub fn written(&self) -> u32 {
        self.len
    }

    /// Append `data` at the end of the file in one storage borrow. Refused
    /// before writing when the file would pass FAT's u32 size (the volume
    /// manager would truncate such a write and still report success).
    pub fn append(&mut self, data: &[u8]) -> crate::error::Result<()> {
        let Some(file) = self.file.handle() else {
            return Err(Error::new(ErrorKind::WriteFailed, "append"));
        };
        if data.is_empty() {
            return Ok(());
        }
        let len = next_file_len(self.len, data.len(), embedded_sdmmc::MAX_FILE_SIZE)
            .ok_or_else(|| Error::new(ErrorKind::WriteFailed, "append: file size limit"))?;
        poll_once(async {
            let mut guard = borrow(self.sd)?;
            let inner = &mut *guard;
            match inner.mgr.write(file, data).await {
                Ok(()) => Ok(()),
                Err(_) => {
                    inner.discard_block_cache();
                    Err(Error::new(ErrorKind::WriteFailed, "append"))
                }
            }
        })?;
        self.len = len;
        Ok(())
    }

    /// Flush the directory entry and close. Inside a storage callback, report
    /// an error and defer the close until that callback's borrow is released.
    pub fn close(mut self) -> crate::error::Result<()> {
        self.file.close().map_err(|error| match error {
            CloseError::Deferred => Error::new(ErrorKind::WriteFailed, "close: deferred"),
            CloseError::Failed(error) => error,
        })
    }
}

pub fn delete_file(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_ROOT, "", name).await;
        op_delete!(inner, inner.root, name)
    })
}

// directory listing

pub fn list_root_files(sd: &SdStorage, buf: &mut [DirEntry]) -> crate::error::Result<usize> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;

        let mut count = 0usize;
        let mut total = 0usize;

        inner
            .mgr
            .iterate_dir(inner.root, |entry| {
                if entry.attributes.is_volume() || entry.attributes.is_directory() {
                    return ControlFlow::Continue(());
                }

                let mut name_buf = [0u8; 13];
                let name_len = sfn_to_bytes(&entry.name, &mut name_buf);
                let sfn = &name_buf[..name_len as usize];

                if !is_listed_name(sfn) {
                    return ControlFlow::Continue(());
                }

                total += 1;

                if count < buf.len() {
                    buf[count] = DirEntry {
                        name: name_buf,
                        name_len,
                        is_dir: false,
                        size: entry.size,
                        title: [0u8; TITLE_CAP],
                        title_len: 0,
                    };
                    count += 1;
                }
                ControlFlow::Continue(())
            })
            .await
            .map_err(|_| Error::new(ErrorKind::ReadFailed, "list_root_files"))?;

        if total > count {
            log::warn!(
                "dir: {} supported files on SD, only {} fit in buffer (max {})",
                total,
                count,
                buf.len(),
            );
        }
        Ok(count)
    })
}

// directory management

pub fn ensure_dir(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    // two poll_once calls so the large make_dir future never shares
    // a stack frame with open_dir, halving peak stack usage
    let exists = poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        match inner.mgr.open_dir(inner.root, name).await {
            Ok(dir) => {
                let _ = inner.mgr.close_dir(dir);
                Ok::<_, Error>(true)
            }
            Err(_) => Ok(false),
        }
    })?;

    if exists {
        return Ok(());
    }

    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        match inner.mgr.make_dir_in_dir(inner.root, name).await {
            Ok(()) => Ok(()),
            Err(embedded_sdmmc::Error::DirAlreadyExists) => Ok(()),
            Err(_) => Err(Error::new(ErrorKind::WriteFailed, "ensure_dir")),
        }
    })
}

// single-directory file operations

pub fn write_file_in_dir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    data: &[u8],
) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in_dir(inner, dir, name).await;
        in_dir!(inner, dir, |dir_h| op_write!(inner, dir_h, name, data))
    })
}

pub fn append_file_in_dir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    data: &[u8],
) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in_dir(inner, dir, name).await;
        in_dir!(inner, dir, |dir_h| op_append!(inner, dir_h, name, data))
    })
}

pub fn read_file_chunk_in_dir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> crate::error::Result<usize> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in_dir(inner, dir, name).await;
        in_dir!(inner, dir, |dir_h| op_read_chunk!(
            inner, dir_h, name, offset, buf
        ))
    })
}

pub fn read_file_start_in_dir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    buf: &mut [u8],
) -> crate::error::Result<(u32, usize)> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in_dir(inner, dir, name).await;
        in_dir!(inner, dir, |dir_h| op_read_start!(inner, dir_h, name, buf))
    })
}

// async boot path (runs inside the real executor)

pub async fn ensure_pulp_dir_async(sd: &SdStorage) -> crate::error::Result<()> {
    let mut guard = borrow(sd)?;
    let inner = &mut *guard;

    if let Ok(dir) = inner.mgr.open_dir(inner.root, PULP_DIR).await {
        let _ = inner.mgr.close_dir(dir);
        return Ok(());
    }
    match inner.mgr.make_dir_in_dir(inner.root, PULP_DIR).await {
        Ok(()) => Ok(()),
        Err(embedded_sdmmc::Error::DirAlreadyExists) => Ok(()),
        Err(_) => Err(Error::new(ErrorKind::WriteFailed, "ensure_pulp_dir_async")),
    }
}

// _PULP subdirectory operations

pub fn ensure_pulp_subdir(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    let exists = poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_dir!(inner, PULP_DIR, |pulp_h| {
            match inner.mgr.open_dir(pulp_h, name).await {
                Ok(sub) => {
                    let _ = inner.mgr.close_dir(sub);
                    Ok::<_, Error>(true)
                }
                Err(_) => Ok(false),
            }
        })
    })?;

    if exists {
        return Ok(());
    }

    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_dir!(inner, PULP_DIR, |pulp_h| {
            match inner.mgr.make_dir_in_dir(pulp_h, name).await {
                Ok(()) => Ok::<_, Error>(()),
                Err(embedded_sdmmc::Error::DirAlreadyExists) => Ok(()),
                Err(_) => Err(Error::new(ErrorKind::WriteFailed, "ensure_pulp_subdir")),
            }
        })
    })
}

pub fn write_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    data: &[u8],
) -> crate::error::Result<()> {
    let result = poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_write!(
            inner, sub_h, name, data
        ))
    });
    font_path_changed(dir);
    result
}

pub fn append_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    data: &[u8],
) -> crate::error::Result<()> {
    let result = poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_append!(
            inner, sub_h, name, data
        ))
    });
    font_path_changed(dir);
    result
}

pub fn read_chunk_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> crate::error::Result<usize> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_SUB, dir, name).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_read_chunk!(
            inner, sub_h, name, offset, buf
        ))
    })
}

// one read-only file held open for a whole caller closure; repeated
// positioned reads skip the per-read dir walk, open and FAT chain re-walk.
// borrows the SdStorage RefCell for the closure's duration, so the closure
// must not call other storage functions
pub struct SubdirFile<'a> {
    inner: &'a mut SdStorageInner,
    file: RawFile,
}

impl SubdirFile<'_> {
    pub fn len(&self) -> crate::error::Result<u32> {
        self.inner
            .mgr
            .file_length(self.file)
            .map_err(|_| Error::new(ErrorKind::OpenFile, "subdir_file_len"))
    }

    // seek + one read; may return fewer bytes than asked, like read_chunk
    pub fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> crate::error::Result<usize> {
        let (mgr, file) = (&mut self.inner.mgr, self.file);
        poll_once(async {
            match mgr.file_seek_from_start(file, offset) {
                Ok(()) => mgr
                    .read(file, buf)
                    .await
                    .map_err(|_| Error::new(ErrorKind::ReadFailed, "read_chunk")),
                Err(_) => Err(Error::new(ErrorKind::SeekFailed, "read_chunk")),
            }
        })
    }
}

// "<dir>/<name>" of a held file; None when it does not fit the slot key
fn held_key(dir: &str, name: &str) -> Option<([u8; HELD_KEY_LEN], u8)> {
    let len = dir.len() + 1 + name.len();
    if len > HELD_KEY_LEN {
        return None;
    }
    let mut key = [0u8; HELD_KEY_LEN];
    key[..dir.len()].copy_from_slice(dir.as_bytes());
    key[dir.len()] = b'/';
    key[dir.len() + 1..len].copy_from_slice(name.as_bytes());
    Some((key, len as u8))
}

// close the held file in `slot` (if any); a failed close still frees the handle
async fn release_held(inner: &mut SdStorageInner, slot: usize) {
    if let Some(held) = inner.held.slots[slot].take() {
        let _ = inner.mgr.close_file(held.file).await;
    }
}

// a held file points at the cluster chain it was opened with: drop every one
// under `dir` before that directory is written or deleted
async fn release_held_in(inner: &mut SdStorageInner, dir: &str) {
    for slot in 0..inner.held.slots.len() {
        let under = inner.held.slots[slot].is_some_and(|h| {
            let key = &h.key[..usize::from(h.key_len)];
            h.scope == SCOPE_SUB
                && key.len() > dir.len()
                && key.starts_with(dir.as_bytes())
                && key[dir.len()] == b'/'
        });
        if under {
            release_held(inner, slot).await;
        }
    }
}

// slot key of a held file: "<dir>/<name>" under _PULP, the plain name in the
// root directory or _PULP itself
fn held_name_key(scope: u8, dir: &str, name: &str) -> Option<([u8; HELD_KEY_LEN], u8)> {
    if scope == SCOPE_SUB {
        return held_key(dir, name);
    }
    let len = name.len();
    if len > HELD_KEY_LEN {
        return None;
    }
    let mut key = [0u8; HELD_KEY_LEN];
    key[..len].copy_from_slice(name.as_bytes());
    Some((key, len as u8))
}

fn held_slot(
    inner: &SdStorageInner,
    scope: u8,
    key: &[u8; HELD_KEY_LEN],
    len: u8,
) -> Option<usize> {
    inner.held.slots.iter().position(|h| {
        h.is_some_and(|h| {
            h.scope == scope
                && h.key_len == len
                && h.key[..usize::from(len)] == key[..usize::from(len)]
        })
    })
}

// FAT refuses to open a file that is open already, so whatever is about to
// write, delete or open `name` itself first lets go of the held handle
async fn release_held_named(inner: &mut SdStorageInner, scope: u8, dir: &str, name: &str) {
    if let Some((key, len)) = held_name_key(scope, dir, name)
        && let Some(slot) = held_slot(inner, scope, &key, len)
    {
        release_held(inner, slot).await;
    }
}

// the same for the `*_in_dir` operations: only _PULP's own files are ever held
async fn release_held_in_dir(inner: &mut SdStorageInner, dir: &str, name: &str) {
    if dir == PULP_DIR {
        release_held_named(inner, SCOPE_PULP, "", name).await;
    }
}

// `name` of the root directory (SCOPE_ROOT) or of _PULP (SCOPE_PULP), open
// read-only and kept in a held slot: a held one is reused, so repeated reads of
// the same file (an EPUB's chunks, a chapter cache) skip the directory scans and
// the open. Errors are the ones the transient open would have given
async fn held_open(
    inner: &mut SdStorageInner,
    scope: u8,
    name: &str,
    what: &'static str,
) -> crate::error::Result<(usize, RawFile)> {
    let Some((key, len)) = held_name_key(scope, "", name) else {
        return Err(Error::new(ErrorKind::OpenFile, what));
    };
    if let Some(slot) = held_slot(inner, scope, &key, len)
        && let Some(held) = inner.held.slots[slot]
    {
        return Ok((slot, held.file));
    }
    let dir = if scope == SCOPE_PULP {
        match pulp_dir!(inner) {
            Ok(dir) => dir,
            Err(_) => return Err(Error::new(ErrorKind::OpenDir, what)),
        }
    } else {
        inner.root
    };
    let file = match inner.mgr.open_file_in_dir(dir, name, Mode::ReadOnly).await {
        Ok(file) => file,
        Err(_) => return Err(Error::new(ErrorKind::OpenFile, what)),
    };
    let free = inner.held.slots.iter().position(Option::is_none);
    let slot = free.unwrap_or(inner.held.next_evict);
    release_held(inner, slot).await;
    inner.held.next_evict = (slot + 1) % inner.held.slots.len();
    inner.held.slots[slot] = Some(HeldFile {
        scope,
        key,
        key_len: len,
        file,
    });
    Ok((slot, file))
}

// seek + one read on a held file; a failed seek or read lets go of the handle
// so the next call starts from a fresh open
async fn read_held_chunk(
    inner: &mut SdStorageInner,
    scope: u8,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> crate::error::Result<usize> {
    let (slot, file) = held_open(inner, scope, name, "read_chunk").await?;
    let result = match inner.mgr.file_seek_from_start(file, offset) {
        Ok(()) => inner
            .mgr
            .read(file, buf)
            .await
            .map_err(|_| Error::new(ErrorKind::ReadFailed, "read_chunk")),
        Err(_) => Err(Error::new(ErrorKind::SeekFailed, "read_chunk")),
    };
    if result.is_err() {
        release_held(inner, slot).await;
    }
    result
}

// run `f` on _PULP/<dir>/<name>; `f` gets None when FAT reports NotFound for
// any of the three, other open errors are Err. the file stays open in a held
// slot afterwards (reuse skips the ~190 ms dir walk and open on the C61); an Err
// from `f` or a failed read releases it so the next call starts from a fresh
// open. a card that fails mid-closure surfaces as Err from read_at, never a
// panic
pub fn with_pulp_subdir_file<T>(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    f: impl FnOnce(Option<&mut SubdirFile<'_>>) -> crate::error::Result<T>,
) -> crate::error::Result<T> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let key = held_key(dir, name);
        let held = key.and_then(|(k, len)| held_slot(inner, SCOPE_SUB, &k, len));
        let (slot, file) = match held {
            Some(slot) => (slot, inner.held.slots[slot].map(|h| h.file)),
            None => {
                let pulp = match pulp_dir!(inner) {
                    Ok(handle) => handle,
                    Err(embedded_sdmmc::Error::NotFound) => return f(None),
                    Err(_) => return Err(Error::new(ErrorKind::OpenDir, "subdir_file")),
                };
                let sub = match inner.mgr.open_dir(pulp, dir).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        return match e {
                            embedded_sdmmc::Error::NotFound => f(None),
                            _ => Err(Error::new(ErrorKind::OpenDir, "subdir_file")),
                        };
                    }
                };
                let opened = inner.mgr.open_file_in_dir(sub, name, Mode::ReadOnly).await;
                let _ = inner.mgr.close_dir(sub);
                match opened {
                    Ok(file) => match key {
                        Some((k, len)) => {
                            let free = inner.held.slots.iter().position(Option::is_none);
                            let slot = free.unwrap_or(inner.held.next_evict);
                            release_held(inner, slot).await;
                            inner.held.next_evict = (slot + 1) % inner.held.slots.len();
                            inner.held.slots[slot] = Some(HeldFile {
                                scope: SCOPE_SUB,
                                key: k,
                                key_len: len,
                                file,
                            });
                            (slot, Some(file))
                        }
                        None => {
                            // no slot key: one-shot, closed below
                            let r = f(Some(&mut SubdirFile {
                                inner: &mut *inner,
                                file,
                            }));
                            let _ = inner.mgr.close_file(file).await;
                            return r;
                        }
                    },
                    Err(embedded_sdmmc::Error::NotFound) => return f(None),
                    Err(_) => return Err(Error::new(ErrorKind::OpenFile, "subdir_file")),
                }
            }
        };
        let Some(file) = file else {
            return Err(Error::new(ErrorKind::OpenFile, "subdir_file"));
        };
        let result = f(Some(&mut SubdirFile {
            inner: &mut *inner,
            file,
        }));
        if result.is_err() {
            release_held(inner, slot).await;
        }
        result
    })
}

/// An optional file is absent only when FAT reports NotFound. Opening or
/// reading metadata can otherwise fail even when the file exists.
pub fn optional_file_size_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
) -> crate::error::Result<Option<u32>> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let pulp = match pulp_dir!(inner) {
            Ok(handle) => handle,
            Err(embedded_sdmmc::Error::NotFound) => return Ok(None),
            Err(_) => return Err(Error::new(ErrorKind::OpenDir, "optional_file_size")),
        };
        let fonts = match inner.mgr.open_dir(pulp, dir).await {
            Ok(handle) => handle,
            Err(e) => {
                return match e {
                    embedded_sdmmc::Error::NotFound => Ok(None),
                    _ => Err(Error::new(ErrorKind::OpenDir, "optional_file_size")),
                };
            }
        };
        let result = match inner.mgr.find_directory_entry(fonts, name).await {
            Ok(entry) => Ok(Some(entry.size)),
            Err(embedded_sdmmc::Error::NotFound) => Ok(None),
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "optional_file_size")),
        };
        let _ = inner.mgr.close_dir(fonts);
        result
    })
}

pub fn file_size_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
) -> crate::error::Result<u32> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_file_size!(
            inner, sub_h, name
        ))
    })
}

pub fn delete_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str) -> crate::error::Result<()> {
    let result = poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_delete!(inner, sub_h, name))
    });
    font_path_changed(dir);
    result
}

// _PULP/ direct file operations (cache files live directly in _PULP/)

pub fn read_chunk_in_pulp(
    sd: &SdStorage,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> crate::error::Result<usize> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        read_held_chunk(inner, SCOPE_PULP, name, offset, buf).await
    })
}

pub fn write_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_PULP, "", name).await;
        in_dir!(inner, PULP_DIR, |dir_h| op_write!(inner, dir_h, name, data))
    })
}

pub fn append_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_PULP, "", name).await;
        in_dir!(inner, PULP_DIR, |dir_h| op_append!(
            inner, dir_h, name, data
        ))
    })
}

pub fn file_size_in_pulp(sd: &SdStorage, name: &str) -> crate::error::Result<u32> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        let (slot, file) = held_open(inner, SCOPE_PULP, name, "file_size").await?;
        match inner.mgr.file_length(file) {
            Ok(len) => Ok(len),
            Err(_) => {
                release_held(inner, slot).await;
                Err(Error::new(ErrorKind::OpenFile, "file_size"))
            }
        }
    })
}

pub fn delete_in_pulp(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_PULP, "", name).await;
        in_dir!(inner, PULP_DIR, |dir_h| op_delete!(inner, dir_h, name))
    })
}

// seek+write: open existing file, seek to offset, write data, close
// used to update the chapter offset table after all chapters are appended
pub fn write_at_in_pulp(
    sd: &SdStorage,
    name: &str,
    offset: u32,
    data: &[u8],
) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_named(inner, SCOPE_PULP, "", name).await;
        in_dir!(inner, PULP_DIR, |dir_h| {
            match inner
                .mgr
                .open_file_in_dir(dir_h, name, Mode::ReadWriteCreateOrAppend)
                .await
            {
                Err(_) => Err(Error::new(ErrorKind::OpenFile, "write_at")),
                Ok(file) => {
                    let result = match inner.mgr.file_seek_from_start(file, offset) {
                        Ok(()) => inner
                            .mgr
                            .write(file, data)
                            .await
                            .map_err(|_| Error::new(ErrorKind::WriteFailed, "write_at")),
                        Err(_) => Err(Error::new(ErrorKind::SeekFailed, "write_at")),
                    };
                    if result.is_err() {
                        inner.discard_block_cache();
                    }
                    let _ = inner.mgr.close_file(file).await;
                    result
                }
            }
        })
    })
}

// title mapping

// append a title line to _PULP/TITLES.BIN
pub fn save_title(sd: &SdStorage, filename: &str, title: &str) -> crate::error::Result<()> {
    let mut line = [0u8; 128];
    let Some(line_len) = title_line(filename, title, &mut line) else {
        return Err(Error::new(
            ErrorKind::WriteFailed,
            "save_title: line too long",
        ));
    };

    append_file_in_dir(sd, PULP_DIR, TITLES_FILE, &line[..line_len])
}

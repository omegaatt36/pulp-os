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
use crate::drivers::sdcard::{HELD_KEY_LEN, HeldFile, SdStorage, SdStorageInner, poll_once};
use crate::error::{Error, ErrorKind};

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

// dir-scoping macros; open subdir, execute body, close handle

macro_rules! in_dir {
    ($inner:expr, $dirname:expr, |$dir:ident| $body:expr) => {
        match $inner.mgr.open_dir($inner.root, $dirname).await {
            Err(_) => Err(Error::new(ErrorKind::OpenDir, "in_dir")),
            Ok($dir) => {
                let _r = $body;
                let _ = $inner.mgr.close_dir($dir);
                _r
            }
        }
    };
}

macro_rules! in_subdir {
    ($inner:expr, $d1:expr, $d2:expr, |$dir:ident| $body:expr) => {
        match $inner.mgr.open_dir($inner.root, $d1).await {
            Err(_) => Err(Error::new(ErrorKind::OpenDir, "in_subdir")),
            Ok(_mid) => match $inner.mgr.open_dir(_mid, $d2).await {
                Err(_) => {
                    let _ = $inner.mgr.close_dir(_mid);
                    Err(Error::new(ErrorKind::OpenDir, "in_subdir"))
                }
                Ok($dir) => {
                    let _r = $body;
                    let _ = $inner.mgr.close_dir($dir);
                    let _ = $inner.mgr.close_dir(_mid);
                    _r
                }
            },
        }
    };
}

fn borrow(sd: &SdStorage) -> core::result::Result<core::cell::RefMut<'_, SdStorageInner>, Error> {
    sd.borrow_inner()
        .ok_or(Error::new(ErrorKind::NoCard, "storage::borrow"))
}

// root file operations

pub fn file_size(sd: &SdStorage, name: &str) -> crate::error::Result<u32> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        op_file_size!(inner, inner.root, name)
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
        op_read_chunk!(inner, inner.root, name, offset, buf)
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
        op_read_start!(inner, inner.root, name, buf)
    })
}

pub fn write_file(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        op_write!(inner, inner.root, name, data)
    })
}

pub fn append_root_file(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        op_append!(inner, inner.root, name, data)
    })
}

pub fn delete_file(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
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
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_write!(
            inner, sub_h, name, data
        ))
    })
}

pub fn append_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    data: &[u8],
) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_append!(
            inner, sub_h, name, data
        ))
    })
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
            key.len() > dir.len() && key.starts_with(dir.as_bytes()) && key[dir.len()] == b'/'
        });
        if under {
            release_held(inner, slot).await;
        }
    }
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
        let held = key.and_then(|(k, len)| {
            inner.held.slots.iter().position(|h| {
                h.is_some_and(|h| {
                    h.key_len == len && h.key[..usize::from(len)] == k[..usize::from(len)]
                })
            })
        });
        let (slot, file) = match held {
            Some(slot) => (slot, inner.held.slots[slot].map(|h| h.file)),
            None => {
                let pulp = match inner.mgr.open_dir(inner.root, PULP_DIR).await {
                    Ok(handle) => handle,
                    Err(embedded_sdmmc::Error::NotFound) => return f(None),
                    Err(_) => return Err(Error::new(ErrorKind::OpenDir, "subdir_file")),
                };
                let sub = match inner.mgr.open_dir(pulp, dir).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        let _ = inner.mgr.close_dir(pulp);
                        return match e {
                            embedded_sdmmc::Error::NotFound => f(None),
                            _ => Err(Error::new(ErrorKind::OpenDir, "subdir_file")),
                        };
                    }
                };
                let opened = inner.mgr.open_file_in_dir(sub, name, Mode::ReadOnly).await;
                let _ = inner.mgr.close_dir(sub);
                let _ = inner.mgr.close_dir(pulp);
                match opened {
                    Ok(file) => match key {
                        Some((k, len)) => {
                            let free = inner.held.slots.iter().position(Option::is_none);
                            let slot = free.unwrap_or(inner.held.next_evict);
                            release_held(inner, slot).await;
                            inner.held.next_evict = (slot + 1) % inner.held.slots.len();
                            inner.held.slots[slot] = Some(HeldFile {
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
        let pulp = match inner.mgr.open_dir(inner.root, PULP_DIR).await {
            Ok(handle) => handle,
            Err(embedded_sdmmc::Error::NotFound) => return Ok(None),
            Err(_) => return Err(Error::new(ErrorKind::OpenDir, "optional_file_size")),
        };
        let fonts = match inner.mgr.open_dir(pulp, dir).await {
            Ok(handle) => handle,
            Err(e) => {
                let _ = inner.mgr.close_dir(pulp);
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
        let _ = inner.mgr.close_dir(pulp);
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
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        release_held_in(inner, dir).await;
        in_subdir!(inner, PULP_DIR, dir, |sub_h| op_delete!(inner, sub_h, name))
    })
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
        in_dir!(inner, PULP_DIR, |dir_h| op_read_chunk!(
            inner, dir_h, name, offset, buf
        ))
    })
}

pub fn write_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_dir!(inner, PULP_DIR, |dir_h| op_write!(inner, dir_h, name, data))
    })
}

pub fn append_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_dir!(inner, PULP_DIR, |dir_h| op_append!(
            inner, dir_h, name, data
        ))
    })
}

pub fn file_size_in_pulp(sd: &SdStorage, name: &str) -> crate::error::Result<u32> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
        in_dir!(inner, PULP_DIR, |dir_h| op_file_size!(inner, dir_h, name))
    })
}

pub fn delete_in_pulp(sd: &SdStorage, name: &str) -> crate::error::Result<()> {
    poll_once(async {
        let mut guard = borrow(sd)?;
        let inner = &mut *guard;
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

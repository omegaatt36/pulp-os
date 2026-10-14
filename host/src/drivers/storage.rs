// host stand-in for kernel/src/drivers/storage.rs: the function names and
// signatures the kernel files call, each forwarding to the VirtualStorage
// method of the same shape (crate::storage). save_title builds its line with the
// firmware's own dir_entry::title_line.
use crate::drivers::dir_entry::title_line;
pub use crate::drivers::dir_entry::{DirEntry, DirPage, PULP_DIR, TITLES_FILE};
use crate::drivers::sdcard::SdStorage;
use crate::error::{Error, ErrorKind};
use pulp_board_logic::upload::next_file_len;

pub type StorageError = Error;

type R<T> = crate::error::Result<T>;

fn borrow(sd: &SdStorage) -> R<&crate::storage::VirtualStorage> {
    sd.borrow_card()
        .ok_or(Error::new(ErrorKind::NoCard, "storage::borrow"))
}

pub fn file_size(sd: &SdStorage, name: &str) -> R<u32> {
    borrow(sd)?.file_size(name)
}

pub fn read_file_chunk(sd: &SdStorage, name: &str, offset: u32, buf: &mut [u8]) -> R<usize> {
    borrow(sd)?.read_file_chunk(name, offset, buf)
}

pub fn read_file_start(sd: &SdStorage, name: &str, buf: &mut [u8]) -> R<(u32, usize)> {
    borrow(sd)?.read_file_start(name, buf)
}

pub fn write_file(sd: &SdStorage, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.write_file(name, data)
}

pub fn append_root_file(sd: &SdStorage, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.append_root_file(name, data)
}

// stand-in for the firmware's upload writer: the same open-once / append /
// close-once contract over the virtual card, which counts opens, closes and
// append sizes (VirtualStorage::writer_*)
pub struct RootWriter<'a> {
    card: &'a crate::storage::VirtualStorage,
    name: String,
    open: bool,
    len: u32,
}

pub fn create_root_writer<'a>(sd: &'a SdStorage, name: &str) -> R<RootWriter<'a>> {
    let card = borrow(sd)?;
    card.open_root_writer(name)?;
    Ok(RootWriter {
        card,
        name: name.to_string(),
        open: true,
        len: 0,
    })
}

impl RootWriter<'_> {
    pub fn written(&self) -> u32 {
        self.len
    }

    pub fn append(&mut self, data: &[u8]) -> R<()> {
        if !self.open {
            return Err(Error::new(ErrorKind::WriteFailed, "append"));
        }
        if data.is_empty() {
            return Ok(());
        }
        let len = next_file_len(self.len, data.len(), self.card.max_file_len()).ok_or(
            Error::new(ErrorKind::WriteFailed, "append: file size limit"),
        )?;
        self.card.writer_append(&self.name, data)?;
        self.len = len;
        Ok(())
    }

    pub fn close(mut self) -> R<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> R<()> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        self.card.close_root_writer(&self.name)
    }
}

impl Drop for RootWriter<'_> {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

/// A collision is distinct from an SD failure; uploads never replace a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadError {
    AlreadyExists,
    Storage(Error),
}

impl From<Error> for UploadError {
    fn from(error: Error) -> Self {
        Self::Storage(error)
    }
}

/// Owns a newly created file. Only a successful commit preserves it.
pub struct UploadWriter<'a> {
    sd: &'a SdStorage,
    name: String,
    writer: Option<RootWriter<'a>>,
    len: u32,
    pending_delete: bool,
    failed: bool,
}

impl UploadWriter<'_> {
    pub fn written(&self) -> u32 {
        self.len
    }

    pub fn append(&mut self, data: &[u8]) -> core::result::Result<(), UploadError> {
        let result = match self.writer.as_mut() {
            Some(writer) if !self.failed => writer.append(data),
            _ => Err(Error::new(ErrorKind::WriteFailed, "upload: failed writer")),
        };
        match result {
            Ok(()) => {
                self.len = self.writer.as_ref().unwrap().written();
                Ok(())
            }
            Err(error) => {
                self.failed = true;
                // Cleanup failure takes precedence: callers must not claim the
                // partial file was removed when the SD card refused deletion.
                self.abort_inner()?;
                Err(error.into())
            }
        }
    }

    pub fn commit(mut self) -> core::result::Result<(), UploadError> {
        if self.failed {
            return Err(Error::new(ErrorKind::WriteFailed, "upload: failed writer").into());
        }
        let result = self.writer.take().unwrap().close();
        match result {
            Ok(()) => {
                self.pending_delete = false;
                Ok(())
            }
            Err(error) => {
                self.abort_inner()?;
                Err(error.into())
            }
        }
    }

    /// Close before deleting. Report cleanup errors to explicit abort callers.
    pub fn abort(mut self) -> core::result::Result<(), UploadError> {
        self.abort_inner()
    }

    fn abort_inner(&mut self) -> core::result::Result<(), UploadError> {
        let closed = self.writer.take().map_or(Ok(()), RootWriter::close);
        if self.pending_delete {
            // One attempt only: a failed deletion leaves the name reserved on
            // disk, and a later upload will reject that collision.
            self.pending_delete = false;
            delete_file(self.sd, &self.name)?;
        }
        closed.map_err(UploadError::Storage)
    }
}

impl Drop for UploadWriter<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.abort_inner() {
            log::warn!("storage: upload cleanup failed: {:?}", error);
        }
    }
}

pub fn create_upload_writer<'a>(
    sd: &'a SdStorage,
    name: &str,
) -> core::result::Result<UploadWriter<'a>, UploadError> {
    let card = borrow(sd)?;
    let name = name.to_ascii_uppercase();
    card.open_upload_writer(&name)?;
    Ok(UploadWriter {
        sd,
        name: name.clone(),
        writer: Some(RootWriter {
            card,
            name,
            open: true,
            len: 0,
        }),
        len: 0,
        pending_delete: true,
        failed: false,
    })
}

pub fn delete_file(sd: &SdStorage, name: &str) -> R<()> {
    borrow(sd)?.delete_file(name)
}

pub fn list_root_files(sd: &SdStorage, buf: &mut [DirEntry]) -> R<usize> {
    borrow(sd)?.list_root_files(buf)
}

pub fn write_file_in_dir(sd: &SdStorage, dir: &str, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.write_file_in_dir(dir, name, data)
}

pub fn append_file_in_dir(sd: &SdStorage, dir: &str, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.append_file_in_dir(dir, name, data)
}

pub fn read_file_start_in_dir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    buf: &mut [u8],
) -> R<(u32, usize)> {
    borrow(sd)?.read_file_start_in_dir(dir, name, buf)
}

pub fn ensure_pulp_subdir(sd: &SdStorage, name: &str) -> R<()> {
    borrow(sd)?.ensure_pulp_subdir(name)
}

pub fn write_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.write_in_pulp_subdir(dir, name, data)
}

pub fn append_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.append_in_pulp_subdir(dir, name, data)
}

pub fn read_chunk_in_pulp_subdir(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    offset: u32,
    buf: &mut [u8],
) -> R<usize> {
    borrow(sd)?.read_chunk_in_pulp_subdir(dir, name, offset, buf)
}

pub fn optional_file_size_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str) -> R<Option<u32>> {
    borrow(sd)?.optional_file_size_in_pulp_subdir(dir, name)
}

// stand-in for the firmware's open handle: a positioned read is one logged
// read of the virtual card, an open is counted once per with_pulp_subdir_file
pub struct SubdirFile<'a> {
    card: &'a crate::storage::VirtualStorage,
    dir: &'a str,
    name: &'a str,
    len: u32,
}

impl SubdirFile<'_> {
    pub fn len(&self) -> R<u32> {
        Ok(self.len)
    }

    pub fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> R<usize> {
        self.card
            .read_chunk_in_pulp_subdir(self.dir, self.name, offset, buf)
    }
}

pub fn with_pulp_subdir_file<T>(
    sd: &SdStorage,
    dir: &str,
    name: &str,
    f: impl FnOnce(Option<&mut SubdirFile<'_>>) -> R<T>,
) -> R<T> {
    let card = borrow(sd)?;
    match card.optional_file_size_in_pulp_subdir(dir, name)? {
        Some(len) => {
            card.note_open();
            let result = f(Some(&mut SubdirFile {
                card,
                dir,
                name,
                len,
            }));
            card.note_close();
            result
        }
        None => f(None),
    }
}

pub fn file_size_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str) -> R<u32> {
    borrow(sd)?.file_size_in_pulp_subdir(dir, name)
}

pub fn purge_pulp_subdir(sd: &SdStorage, dir: &str) -> R<u32> {
    borrow(sd)?.purge_pulp_subdir(dir)
}

pub fn delete_in_pulp_subdir(sd: &SdStorage, dir: &str, name: &str) -> R<()> {
    borrow(sd)?.delete_in_pulp_subdir(dir, name)
}

pub fn read_chunk_in_pulp(sd: &SdStorage, name: &str, offset: u32, buf: &mut [u8]) -> R<usize> {
    borrow(sd)?.read_chunk_in_pulp(name, offset, buf)
}

pub fn write_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.write_in_pulp(name, data)
}

pub fn append_in_pulp(sd: &SdStorage, name: &str, data: &[u8]) -> R<()> {
    borrow(sd)?.append_in_pulp(name, data)
}

pub fn file_size_in_pulp(sd: &SdStorage, name: &str) -> R<u32> {
    borrow(sd)?.file_size_in_pulp(name)
}

pub fn delete_in_pulp(sd: &SdStorage, name: &str) -> R<()> {
    borrow(sd)?.delete_in_pulp(name)
}

pub fn write_at_in_pulp(sd: &SdStorage, name: &str, offset: u32, data: &[u8]) -> R<()> {
    borrow(sd)?.write_at_in_pulp(name, offset, data)
}

pub fn save_title(sd: &SdStorage, filename: &str, title: &str) -> R<()> {
    let mut line = [0u8; 128];
    let Some(len) = title_line(filename, title, &mut line) else {
        return Err(Error::new(
            ErrorKind::WriteFailed,
            "save_title: line too long",
        ));
    };
    append_file_in_dir(sd, PULP_DIR, TITLES_FILE, &line[..len])
}

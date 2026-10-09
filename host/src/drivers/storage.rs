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

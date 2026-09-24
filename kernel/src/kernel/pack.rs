// the font pack as the app layer sees it: random access to a root file
//
// render::font_pack owns the format and the per-page cache strategy and knows
// nothing about storage. This file is the only place that knows a pack is a
// file on the SD card, and it goes through KernelHandle rather than
// SdStorage so apps never touch a driver.
//
// why a PackReader is needed at all: the loader streams the 512-byte-aligned
// index in one pass and then reads a glyph bitmap per lookup. Holding the pack
// in RAM is not an option (a 24 px CJK pack is ~1 MB) and re-opening the file
// per lookup is not either, so the file name is held open across a page's
// worth of reads and every read is a seek plus a FAT-sector copy.

use pulp_render::font_pack::{FontPack, IoError, LoadError, PackReader};

use crate::kernel::handle::KernelHandle;

/// Random access to one file on the SD card, through the app-facing API.
///
/// Borrows the handle mutably because every read is a storage operation, and
/// the SPI bus may not be re-entered while one is in flight.
pub struct HandlePackReader<'a, 'k> {
    handle: &'a mut KernelHandle<'k>,
    /// 8.3 file name, exactly as the card spells it (no NUL padding: the
    /// kernel's storage API takes `&str` and length-delimited names).
    name: [u8; 12],
    name_len: usize,
    size: u32,
}

impl<'a, 'k> HandlePackReader<'a, 'k> {
    /// Open `name` and read its length. The card is touched exactly once
    /// here and not again until a glyph lookup, so a pack that fails to open
    /// costs the reader a single sector.
    pub fn open(
        handle: &'a mut KernelHandle<'k>,
        name: &[u8],
        name_len: usize,
    ) -> Result<Self, LoadError> {
        let mut padded = [0u8; 12];
        if name_len == 0 || name_len > padded.len() {
            return Err(LoadError::NotFound);
        }
        padded[..name_len].copy_from_slice(&name[..name_len]);
        if core::str::from_utf8(&padded[..name_len]).is_err() {
            return Err(LoadError::NotFound);
        }

        let file_name =
            core::str::from_utf8(&padded[..name_len]).map_err(|_| LoadError::NotFound)?;
        let size = handle
            .file_size(file_name)
            .map_err(|_| LoadError::NotFound)?;

        Ok(Self {
            handle,
            name: padded,
            name_len,
            size,
        })
    }

    /// Load and validate a pack in one step, the common case.
    ///
    /// Every failure -- no card, absent file, bad magic, failed CRC -- comes
    /// back as a `LoadError` so the caller makes one decision: fall back to
    /// the built-in fonts. Storage and data errors are deliberately not
    /// distinguished here; a reader that cannot get a pack renders with what
    /// it has, and the fallback is identical either way.
    pub fn load(
        handle: &'a mut KernelHandle<'k>,
        name: &[u8],
        name_len: usize,
        pixel_size: u16,
    ) -> Result<FontPack, LoadError> {
        let mut reader = Self::open(handle, name, name_len)?;
        FontPack::load(&mut reader, pixel_size)
    }
}

impl PackReader for HandlePackReader<'_, '_> {
    fn size(&mut self) -> Result<u32, IoError> {
        Ok(self.size)
    }

    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, IoError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let file_name =
            core::str::from_utf8(&self.name[..self.name_len]).map_err(|_| IoError::NotFound)?;
        self.handle
            .read_chunk(file_name, offset, buf)
            .map_err(|_| IoError::Io)
    }
}

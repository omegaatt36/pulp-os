//! Shared size and offset checks for streamed chapter processing.

use crate::zip::{METHOD_STORED, ZipEntry};

/// Maximum compressed bytes read for one streamed chapter (16 MiB).
pub const MAX_COMPRESSED_CHAPTER_SIZE: u32 = 16 * 1024 * 1024;
/// Maximum decompressed bytes processed for one streamed chapter (8 MiB).
pub const MAX_DECOMPRESSED_CHAPTER_SIZE: u32 = 8 * 1024 * 1024;
/// Maximum stripped bytes emitted for one streamed chapter (8 MiB).
pub const MAX_STRIPPED_CHAPTER_SIZE: u32 = 8 * 1024 * 1024;

pub(crate) fn validate_entry(entry: &ZipEntry) -> Result<(), &'static str> {
    if entry.comp_size > MAX_COMPRESSED_CHAPTER_SIZE {
        return Err("cache: compressed chapter too large");
    }
    if entry.uncomp_size > MAX_DECOMPRESSED_CHAPTER_SIZE {
        return Err("cache: decompressed chapter too large");
    }
    if entry.method == METHOD_STORED && entry.comp_size != entry.uncomp_size {
        return Err("cache: stored size mismatch");
    }
    Ok(())
}

pub(crate) fn advance(offset: u32, amount: usize) -> Result<u32, &'static str> {
    offset
        .checked_add(u32::try_from(amount).map_err(|_| "cache: byte count overflow")?)
        .ok_or("cache: offset overflow")
}

pub(crate) fn read_count(n: usize, want: usize) -> Result<(), &'static str> {
    if n > want {
        return Err("cache: invalid read count");
    }
    Ok(())
}

pub(crate) fn decompressed_next(total: u32, n: usize, declared: u32) -> Result<u32, &'static str> {
    let next = advance(total, n)?;
    if next > declared {
        return Err("cache: deflate output exceeds declared size");
    }
    if next > MAX_DECOMPRESSED_CHAPTER_SIZE {
        return Err("cache: decompressed chapter too large");
    }
    Ok(next)
}

pub(crate) fn written_next(total: u32, n: usize) -> Result<u32, &'static str> {
    let next = advance(total, n)?;
    if next > MAX_STRIPPED_CHAPTER_SIZE {
        return Err("cache: stripped chapter too large");
    }
    Ok(next)
}

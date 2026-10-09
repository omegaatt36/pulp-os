// Wi-Fi upload sizing: the socket/work buffer profile and the storage write
// batching rules. Pure arithmetic, no allocation: the firmware allocates the
// buffers (src/apps/upload/mod.rs) and the host tests exercise these rules.
//
// A profile is one all-or-nothing block for the TCP receive buffer, the TCP
// transmit buffer and the HTTP work buffer. HR8-class PSRAM tries the large one
// first; any refusal (class limit, fragmentation) falls back to the small one,
// which is the profile of the X4 and of degraded/HR2 builds. Only when that
// fails too does the session end, before the radio starts.

use crate::memory::{PSRAM_LARGE_MIN_BYTES, PsramStatus};
use crate::sd_cache::SECTOR_BYTES;

/// Upper bound on the HTTP request state kept next to the buffers (directory
/// listing plus header bytes). The firmware asserts its `HttpScratch` fits.
pub const HTTP_FIXED_MAX_BYTES: usize = 8 * 1024;

/// A storage append never carries more than this: one SD write slice, after
/// which the upload yields to the executor, so one flush cannot block the
/// radio tasks for a whole work buffer.
pub const FLUSH_SLICE_BYTES: usize = 4 * 1024;

const _: () = assert!(FLUSH_SLICE_BYTES % SECTOR_BYTES == 0);

/// The work buffer must hold the part headers, the delimiter holdback
/// (`CRLF--` plus up to 120 boundary bytes, plus the two bytes that decide
/// final or more parts) and two sectors, so every flush has a full sector.
pub const MIN_WORK_BYTES: usize = 2 * SECTOR_BYTES + 120 + 4 + 2;

/// Sizes of the three buffers of one upload session.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NetProfile {
    pub rx: usize,
    pub tx: usize,
    pub work: usize,
}

impl NetProfile {
    /// X4, HR2 and degraded builds (the sizes the firmware always used).
    pub const SMALL: NetProfile = NetProfile {
        rx: 2048,
        tx: 1536,
        work: 2048,
    };

    /// HR8: 16 KiB receive window, 4 KiB transmit, 16 KiB work.
    pub const LARGE: NetProfile = NetProfile {
        rx: 16 * 1024,
        tx: 4 * 1024,
        work: 16 * 1024,
    };

    pub const fn total(&self) -> usize {
        self.rx + self.tx + self.work
    }
}

const _: () = assert!(NetProfile::SMALL.work >= MIN_WORK_BYTES);
const _: () = assert!(NetProfile::LARGE.work >= MIN_WORK_BYTES);

/// Profiles to try for a PSRAM state, preferred first; the last one is always
/// `SMALL`. Large only on a part of at least `PSRAM_LARGE_MIN_BYTES` (HR8).
pub const fn profile_order(status: PsramStatus) -> &'static [NetProfile] {
    match status {
        PsramStatus::Ready { bytes } if bytes >= PSRAM_LARGE_MIN_BYTES => {
            &[NetProfile::LARGE, NetProfile::SMALL]
        }
        _ => &[NetProfile::SMALL],
    }
}

/// Allocate for the first profile of `profile_order` that `alloc` accepts. The
/// error of the last attempt is returned when none does.
pub fn acquire<T, E>(
    status: PsramStatus,
    mut alloc: impl FnMut(NetProfile) -> Result<T, E>,
) -> Result<(NetProfile, T), E> {
    let order = profile_order(status);
    let mut last = None;
    for &profile in order {
        match alloc(profile) {
            Ok(v) => return Ok((profile, v)),
            Err(e) => last = Some(e),
        }
    }
    // `profile_order` is never empty
    Err(last.expect("profile order is not empty"))
}

/// Bytes of `n` that make up whole sectors; the rest waits for more payload so
/// storage appends stay sector-aligned until the final tail.
pub const fn sector_aligned(n: usize) -> usize {
    n - n % SECTOR_BYTES
}

/// File length after appending `add` bytes, or `None` when it would pass `max`
/// (FAT's u32 file size; the volume manager silently truncates a write that
/// crosses it, so the check has to come first).
pub fn next_file_len(len: u32, add: usize, max: u32) -> Option<u32> {
    let add = u32::try_from(add).ok()?;
    len.checked_add(add).filter(|&n| n <= max)
}

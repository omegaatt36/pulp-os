//! Bounded read-through blocks for chapters too large for the whole-chapter ring.
use crate::{
    error::{Error, Result},
    kernel::{BigBuf, BufClass},
};
use pulp_board_logic::{chapter_ring::ChapterKey, memory::KIB};

const BLOCK: usize = 16 * KIB;
const SLOTS: usize = 16;

struct Entry {
    chapter: ChapterKey,
    block: u32,
    age: u32,
    data: BigBuf,
}

pub(super) struct TextCache {
    entries: [Option<Entry>; SLOTS],
    clock: u32,
}

impl TextCache {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; SLOTS],
            clock: 0,
        }
    }

    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = None;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }

    pub fn read(
        &mut self,
        key: ChapterKey,
        offset: u32,
        out: &mut [u8],
        mut read: impl FnMut(u32, &mut [u8]) -> Result<usize>,
    ) -> Result<usize> {
        let want = out.len().min(key.size.saturating_sub(offset) as usize);
        let mut done = 0;
        while done < want {
            let at = offset as usize + done;
            let block = (at / BLOCK * BLOCK) as u32;
            let pos = self.entries.iter().position(|e| {
                e.as_ref()
                    .is_some_and(|e| e.chapter == key && e.block == block)
            });
            let slot = match pos {
                Some(slot) => slot,
                None => {
                    let slot = self
                        .entries
                        .iter()
                        .position(Option::is_none)
                        .unwrap_or_else(|| {
                            self.entries
                                .iter()
                                .enumerate()
                                .max_by_key(|(_, e)| {
                                    self.clock.wrapping_sub(e.as_ref().unwrap().age)
                                })
                                .unwrap()
                                .0
                        });
                    // Drop the victim before allocating its replacement.
                    self.entries[slot] = None;
                    let len = BLOCK.min((key.size - block) as usize);
                    let mut data = match BigBuf::zeroed(BufClass::ChapterText, len) {
                        Ok(data) => data,
                        Err(_) => {
                            let absolute = key
                                .offset
                                .checked_add(offset)
                                .and_then(|v| v.checked_add(done as u32))
                                .ok_or(Error::READ_FAILED)?;
                            let n = read(absolute, &mut out[done..want])?;
                            if n > want - done {
                                return Err(Error::READ_FAILED);
                            }
                            return Ok(done + n);
                        }
                    };
                    let mut filled = 0;
                    while filled < len {
                        let absolute = key
                            .offset
                            .checked_add(block)
                            .and_then(|v| v.checked_add(filled as u32))
                            .ok_or(Error::READ_FAILED)?;
                        let n = read(absolute, &mut data[filled..])?;
                        if n == 0 || n > len - filled {
                            return Err(Error::READ_FAILED);
                        }
                        filled += n;
                    }
                    self.entries[slot] = Some(Entry {
                        chapter: key,
                        block,
                        age: self.clock,
                        data,
                    });
                    slot
                }
            };
            self.clock = self.clock.wrapping_add(1);
            let entry = self.entries[slot].as_mut().unwrap();
            entry.age = self.clock;
            let within = at - block as usize;
            let n = (want - done).min(entry.data.len() - within);
            out[done..done + n].copy_from_slice(&entry.data[within..within + n]);
            done += n;
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulp_board_logic::source_id::SourceId;

    #[test]
    fn long_chapter_boundary_reads_are_reused_and_identity_is_checked() {
        let mut cache = TextCache::new();
        let mut key = ChapterKey {
            source: SourceId::from_raw(1),
            chapter: 0,
            offset: 17,
            size: 400_000,
        };
        let mut calls = 0;
        let mut read = |at, out: &mut [u8]| {
            calls += 1;
            for (i, v) in out.iter_mut().enumerate() {
                *v = (at as usize + i) as u8;
            }
            Ok(out.len())
        };
        let mut out = [0; 32];
        for _ in 0..2 {
            assert_eq!(
                cache
                    .read(key, BLOCK as u32 - 5, &mut out, &mut read)
                    .unwrap(),
                32
            );
            assert_eq!(out[0], (key.offset as usize + BLOCK - 5) as u8);
        }
        assert_eq!(calls, 2);
        key.source = SourceId::from_raw(2);
        cache
            .read(key, BLOCK as u32 - 5, &mut out, |_, dst| {
                dst.fill(42);
                Ok(dst.len())
            })
            .unwrap();
        assert!(out.iter().all(|&v| v == 42));
    }

    #[test]
    fn failed_fill_is_never_published_and_reads_stop_at_chapter_end() {
        let mut cache = TextCache::new();
        let key = ChapterKey {
            source: SourceId::from_raw(1),
            chapter: 0,
            offset: 0,
            size: 11,
        };
        let mut out = [0; 32];
        assert!(cache.read(key, 0, &mut out, |_, _| Ok(0)).is_err());
        assert!(cache.is_empty());
        let n = cache
            .read(key, 0, &mut out, |_, dst| {
                dst.fill(7);
                Ok(dst.len())
            })
            .unwrap();
        assert_eq!(n, 11);
        assert_eq!(&out[..n], &[7; 11]);
    }
}

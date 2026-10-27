// Leitner progress for the flash cards, kept in answered cards, not in time:
// the device has no clock, so a card falls due a number of answers after it
// was last answered. Pure data; the app reads and writes the files.

use crate::kernel::{BigBuf, BufClass, TypedBuf};

use super::table::KANA;

// one face per script; the reverse (romaji to kana) faces follow the forward ones
pub const SLOTS: usize = KANA.len() * 2;
pub const PROGRESS_SLOTS: usize = SLOTS * 2;

// KNP1 held the forward faces only; it is still read, and written as KNP2
const KANA_MAGIC_V1: [u8; 4] = *b"KNP1";
const KANA_MAGIC: [u8; 4] = *b"KNP2";
const FILE_LEN_V1: usize = 8 + SLOTS + 4 * SLOTS;
pub const FILE_LEN: usize = 8 + PROGRESS_SLOTS + 4 * PROGRESS_SLOTS;

const WORDS_MAGIC: [u8; 4] = *b"KNV1";
const WORDS_HEADER: usize = 12;
const WORDS_RECORD: usize = 9;

const UNSEEN: u8 = 0xff;
const TOP_BOX: u8 = 5;
const KNOWN_BOX: u8 = 3;

const INTERVAL: [u32; TOP_BOX as usize + 1] = [4, 10, 25, 60, 150, 400];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    New,
    Learning,
    Known,
}

pub struct Progress {
    answers: u32,
    boxes: TypedBuf<u8>,
    due: TypedBuf<u32>,
}

impl Progress {
    pub const fn empty() -> Self {
        Self {
            answers: 0,
            boxes: TypedBuf::empty(),
            due: TypedBuf::empty(),
        }
    }

    pub fn new(slots: usize) -> Option<Self> {
        Some(Self {
            answers: 0,
            boxes: TypedBuf::filled(BufClass::PageTable, slots, UNSEEN).ok()?,
            due: TypedBuf::filled(BufClass::PageTable, slots, 0).ok()?,
        })
    }

    pub fn kana() -> Option<Self> {
        Self::new(PROGRESS_SLOTS)
    }

    // The faces a file holds: all of them, or the forward half of a KNP1.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let faces = match (bytes.len(), bytes.get(..4)?) {
            (FILE_LEN, m) if m == KANA_MAGIC => PROGRESS_SLOTS,
            (FILE_LEN_V1, m) if m == KANA_MAGIC_V1 => SLOTS,
            _ => return None,
        };
        let mut p = Self::kana()?;
        p.answers = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let (boxes, dues) = bytes[8..].split_at(faces);
        if boxes.iter().any(|&b| b > TOP_BOX && b != UNSEEN) {
            return None;
        }
        p.boxes[..faces].copy_from_slice(boxes);
        for (due, raw) in p.due.iter_mut().zip(dues.as_chunks::<4>().0) {
            *due = u32::from_le_bytes(*raw);
        }
        Some(p)
    }

    pub fn encode(&self, out: &mut [u8; FILE_LEN]) {
        out[..4].copy_from_slice(&KANA_MAGIC);
        out[4..8].copy_from_slice(&self.answers.to_le_bytes());
        out[8..8 + PROGRESS_SLOTS].copy_from_slice(&self.boxes);
        for (raw, due) in out[8 + PROGRESS_SLOTS..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.due.iter())
        {
            raw.copy_from_slice(&due.to_le_bytes());
        }
    }

    pub fn from_words(bytes: &[u8], hashes: &[u32]) -> Option<Self> {
        if bytes.len() < WORDS_HEADER || bytes[..4] != WORDS_MAGIC {
            return None;
        }
        let count = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
        let expected = count
            .checked_mul(WORDS_RECORD)
            .and_then(|n| n.checked_add(WORDS_HEADER))?;
        if bytes.len() != expected {
            return None;
        }
        let mut p = Self::new(hashes.len())?;
        p.answers = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let mut by_hash = TypedBuf::filled(BufClass::PageTable, hashes.len(), (0u32, 0u32)).ok()?;
        for (i, &hash) in hashes.iter().enumerate() {
            by_hash[i] = (hash, u32::try_from(i).ok()?);
        }
        by_hash.sort_unstable();
        for rec in bytes[WORDS_HEADER..].as_chunks::<WORDS_RECORD>().0 {
            let hash = u32::from_le_bytes(rec[..4].try_into().ok()?);
            let boxed = rec[4];
            if boxed > TOP_BOX {
                return None;
            }
            let first = by_hash.partition_point(|&(h, _)| h < hash);
            if let Some(&(_, slot)) = by_hash.get(first).filter(|&&(h, _)| h == hash) {
                p.boxes[slot as usize] = boxed;
                p.due[slot as usize] = u32::from_le_bytes(rec[5..].try_into().ok()?);
            }
        }
        Some(p)
    }

    pub fn words_file(&self, hashes: &[u32]) -> Option<BigBuf> {
        if hashes.len() != self.boxes.len() {
            return None;
        }
        let seen = self.boxes.iter().filter(|&&b| b != UNSEEN).count();
        let mut out =
            BigBuf::zeroed(BufClass::ChapterText, WORDS_HEADER + WORDS_RECORD * seen).ok()?;
        out[..4].copy_from_slice(&WORDS_MAGIC);
        out[4..8].copy_from_slice(&self.answers.to_le_bytes());
        out[8..12].copy_from_slice(&(seen as u32).to_le_bytes());
        let mut offset = WORDS_HEADER;
        for (slot, &boxed) in self.boxes.iter().enumerate() {
            if boxed != UNSEEN {
                out[offset..offset + 4].copy_from_slice(&hashes[slot].to_le_bytes());
                out[offset + 4] = boxed;
                out[offset + 5..offset + WORDS_RECORD]
                    .copy_from_slice(&self.due[slot].to_le_bytes());
                offset += WORDS_RECORD;
            }
        }
        Some(out)
    }

    pub fn answers(&self) -> u32 {
        self.answers
    }

    pub fn reset(&mut self) {
        self.answers = 0;
        self.boxes.fill(UNSEEN);
        self.due.fill(0);
    }

    pub fn is_known(&self, slot: usize) -> bool {
        self.boxes
            .get(slot)
            .is_some_and(|b| (KNOWN_BOX..=TOP_BOX).contains(b))
    }

    // Hits still to go before the card counts as known; 0 once it does.
    pub fn hits_to_known(&self, slot: usize) -> u8 {
        match self.boxes.get(slot).copied().unwrap_or(UNSEEN) {
            UNSEEN => KNOWN_BOX,
            b => KNOWN_BOX.saturating_sub(b),
        }
    }

    pub fn status(&self, slot: usize) -> Status {
        match self.boxes.get(slot).copied().unwrap_or(UNSEEN) {
            UNSEEN => Status::New,
            _ if self.is_known(slot) => Status::Known,
            _ => Status::Learning,
        }
    }

    pub fn tally(&self, slots: &[u16]) -> (usize, usize, usize) {
        slots.iter().fold((0, 0, 0), |(new, learning, known), &s| {
            match self.status(usize::from(s)) {
                Status::New => (new + 1, learning, known),
                Status::Learning => (new, learning + 1, known),
                Status::Known => (new, learning, known + 1),
            }
        })
    }

    pub fn known_count(&self, slots: &[u16]) -> usize {
        slots
            .iter()
            .filter(|&&s| self.is_known(usize::from(s)))
            .count()
    }

    pub fn answer(&mut self, slot: usize, got_it: bool) {
        let boxed = self.boxes[slot];
        let next = match (got_it, boxed) {
            (false, _) => 0,
            (true, UNSEEN) => 1,
            (true, b) => (b + 1).min(TOP_BOX),
        };
        self.answers = self.answers.wrapping_add(1);
        self.boxes[slot] = next;
        self.due[slot] = self.answers.wrapping_add(INTERVAL[usize::from(next)]);
    }

    // The next card: the most overdue one, else a new one, else the soonest due.
    // `weak_only` leaves out the new and the known cards, to drill what was missed.
    // The card just shown (`avoid`) is skipped unless it is the only one left.
    pub fn pick(
        &self,
        slots: &[u16],
        seed: u32,
        avoid: Option<usize>,
        weak_only: bool,
    ) -> Option<usize> {
        self.pick_except(slots, seed, avoid, weak_only)
            .or_else(|| avoid.and_then(|_| self.pick_except(slots, seed, None, weak_only)))
    }

    fn pick_except(
        &self,
        slots: &[u16],
        seed: u32,
        avoid: Option<usize>,
        weak_only: bool,
    ) -> Option<usize> {
        let candidates = || {
            slots
                .iter()
                .map(|&s| usize::from(s))
                .filter(|&s| s < self.boxes.len())
                .filter(|&s| !weak_only || self.status(s) == Status::Learning)
                .filter(|&s| avoid != Some(s))
        };
        candidates()
            .filter(|&s| self.boxes[s] != UNSEEN && self.due[s] <= self.answers)
            .min_by_key(|&s| self.due[s])
            .or_else(|| {
                candidates()
                    .filter(|&s| self.boxes[s] == UNSEEN)
                    .min_by_key(|&s| mix(s as u32 ^ seed))
            })
            .or_else(|| candidates().min_by_key(|&s| self.due[s]))
    }
}

fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

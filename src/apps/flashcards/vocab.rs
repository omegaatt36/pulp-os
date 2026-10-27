// Vocabulary decks read from the SD card: _PULP/VOCAB/<DECK>.TSV, UTF-8, one
// word per line, tab separated:
//
//     kanji <TAB> kana <TAB> gloss [<TAB> pitch [<TAB> anything further, ignored]]
//
// pitch is the accent number of the kana ("0", "2", "0,2"), see pitch.rs.
//
// Blank lines and lines starting with `#` are skipped, as is any line without
// a kanji and a kana column. Nothing of the sort ships with the firmware: the
// deck is whatever the user puts on the card.

use alloc::vec::Vec;

pub const MAX_BYTES: usize = 1 << 20;
pub const MAX_WORDS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub kanji: &'a str,
    pub kana: &'a str,
    pub gloss: &'a str,
    pub pitch: &'a str,
}

impl Entry<'_> {
    pub fn headword(&self) -> &str {
        if self.kanji.is_empty() {
            self.kana
        } else {
            self.kanji
        }
    }

    pub fn has_reading(&self) -> bool {
        !self.kanji.is_empty() && self.kanji != self.kana
    }

    pub fn hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let bytes = self
            .kanji
            .bytes()
            .chain(core::iter::once(b'\t'))
            .chain(self.kana.bytes());
        for b in bytes {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        h
    }
}

pub fn parse_line(line: &str) -> Option<Entry<'_>> {
    let line = line.trim_end_matches('\r');
    if line.starts_with('#') {
        return None;
    }
    let mut cols = line.split('\t').map(str::trim);
    let (kanji, kana) = (cols.next()?, cols.next()?);
    let (gloss, pitch) = (cols.next().unwrap_or(""), cols.next().unwrap_or(""));
    if kana.is_empty() {
        return None;
    }
    Some(Entry {
        kanji,
        kana,
        gloss,
        pitch,
    })
}

pub struct Vocab {
    text: Vec<u8>,
    lines: Vec<u32>,
    hashes: Vec<u32>,
}

impl Vocab {
    pub const fn new() -> Self {
        Self {
            text: Vec::new(),
            lines: Vec::new(),
            hashes: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn reserve(&mut self, size: usize) -> bool {
        self.clear();
        size <= MAX_BYTES && self.text.try_reserve_exact(size).is_ok()
    }

    pub fn fill(&mut self, max: usize, read: impl FnOnce(&mut [u8]) -> usize) -> usize {
        let room = max.min(MAX_BYTES.saturating_sub(self.text.len()));
        let old = self.text.len();
        self.text
            .resize(old + room.min(self.text.capacity() - old), 0);
        let got = read(&mut self.text[old..]).min(self.text.len() - old);
        self.text.truncate(old + got);
        got
    }

    pub fn loaded_bytes(&self) -> usize {
        self.text.len()
    }

    pub fn index(&mut self) {
        self.lines.clear();
        self.hashes.clear();
        let mut start = 0usize;
        while start < self.text.len() && self.lines.len() < MAX_WORDS {
            let end = self.text[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(self.text.len(), |n| start + n);
            let entry = core::str::from_utf8(&self.text[start..end])
                .ok()
                .and_then(parse_line);
            if let Some(entry) = entry {
                self.lines.push(start as u32);
                self.hashes.push(entry.hash());
            }
            start = end + 1;
        }
        self.lines.shrink_to_fit();
        self.hashes.shrink_to_fit();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn hashes(&self) -> &[u32] {
        &self.hashes
    }

    pub fn entry(&self, i: usize) -> Option<Entry<'_>> {
        let start = *self.lines.get(i)? as usize;
        let rest = &self.text[start..];
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        core::str::from_utf8(&rest[..end]).ok().and_then(parse_line)
    }
}

impl Default for Vocab {
    fn default() -> Self {
        Self::new()
    }
}

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

use crate::kernel::{BigBuf, BufClass, TypedBuf};

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
    text: BigBuf,
    loaded: usize,
    lines: TypedBuf<u32>,
    hashes: TypedBuf<u32>,
}

impl Vocab {
    pub const fn new() -> Self {
        Self {
            text: BigBuf::empty(),
            loaded: 0,
            lines: TypedBuf::empty(),
            hashes: TypedBuf::empty(),
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn reserve(&mut self, size: usize) -> bool {
        self.clear();
        if size > MAX_BYTES {
            return false;
        }
        match BigBuf::zeroed(BufClass::ChapterText, size) {
            Ok(text) => {
                self.text = text;
                true
            }
            Err(_) => false,
        }
    }

    pub fn fill(&mut self, max: usize, read: impl FnOnce(&mut [u8]) -> usize) -> usize {
        let old = self.loaded;
        let room = max.min(self.text.len() - old);
        let got = read(&mut self.text[old..old + room]).min(room);
        self.loaded += got;
        got
    }

    pub fn loaded_bytes(&self) -> usize {
        self.loaded
    }

    pub fn index(&mut self) -> bool {
        self.lines = TypedBuf::empty();
        self.hashes = TypedBuf::empty();
        let count = self.entries().count();
        let Ok(mut lines) = TypedBuf::filled(BufClass::PageTable, count, 0) else {
            return false;
        };
        let Ok(mut hashes) = TypedBuf::filled(BufClass::PageTable, count, 0) else {
            return false;
        };
        for (i, (offset, entry)) in self.entries().enumerate() {
            lines[i] = offset as u32;
            hashes[i] = entry.hash();
        }
        self.lines = lines;
        self.hashes = hashes;
        true
    }

    fn entries(&self) -> impl Iterator<Item = (usize, Entry<'_>)> {
        let mut offset = 0;
        self.text[..self.loaded]
            .split(|&b| b == b'\n')
            .filter_map(move |line| {
                let start = offset;
                offset += line.len() + 1;
                core::str::from_utf8(line)
                    .ok()
                    .and_then(parse_line)
                    .map(|entry| (start, entry))
            })
            .take(MAX_WORDS)
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
        let rest = &self.text[start..self.loaded];
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        core::str::from_utf8(&rest[..end]).ok().and_then(parse_line)
    }
}

impl Default for Vocab {
    fn default() -> Self {
        Self::new()
    }
}

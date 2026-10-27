// Card selection and order for the kana flash cards. Pure data, no drawing.

use super::progress::{PROGRESS_SLOTS, SLOTS};
use super::table::{KANA, Kana, Kind};

const VOWELS: &str = "aiueo";
// distinct row keys of the largest set; the table has fewer
const MAX_GROUPS: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Hiragana,
    Katakana,
    Both,
}

impl Script {
    pub const COUNT: usize = 3;

    const ALL: [Script; Self::COUNT] = [Self::Hiragana, Self::Katakana, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hiragana => "Hiragana",
            Self::Katakana => "Katakana",
            Self::Both => "Both",
        }
    }

    pub fn step(self, forward: bool) -> Self {
        Self::ALL[step_index(self as usize, Self::COUNT, forward)]
    }
}

// Which way a kana card is asked. The reverse faces sit above the forward ones
// in the progress: slot + SLOTS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
    Mixed,
}

impl Direction {
    pub const COUNT: usize = 3;

    const ALL: [Direction; Self::COUNT] = [Self::Forward, Self::Reverse, Self::Mixed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Forward => "Kana > Romaji",
            Self::Reverse => "Romaji > Kana",
            Self::Mixed => "Mixed",
        }
    }

    pub fn step(self, forward: bool) -> Self {
        Self::ALL[step_index(self as usize, Self::COUNT, forward)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Set {
    Basic,
    Voiced,
    Contracted,
    Loanword,
    All,
}

impl Set {
    pub const COUNT: usize = 5;

    const ALL: [Set; Self::COUNT] = [
        Self::Basic,
        Self::Voiced,
        Self::Contracted,
        Self::Loanword,
        Self::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Voiced => "Voiced",
            Self::Contracted => "Contracted",
            Self::Loanword => "Loanword",
            Self::All => "All",
        }
    }

    pub fn step(self, forward: bool) -> Self {
        Self::ALL[step_index(self as usize, Self::COUNT, forward)]
    }

    fn contains(self, kind: Kind) -> bool {
        match self {
            Self::Basic => kind == Kind::Seion,
            Self::Voiced => matches!(kind, Kind::Dakuon | Kind::Handakuon),
            Self::Contracted => kind == Kind::Yoon,
            Self::Loanword => kind == Kind::Gaikoku,
            Self::All => true,
        }
    }
}

// The row a kana belongs to: its romaji without the final vowel (ka, ki, ... are
// "k"; the plain vowels are ""; ん joins the na row). The five irregular
// spellings are put back in their row by hand: shi, chi, tsu, fu and ji.
pub fn group_key(kana: &Kana) -> &'static str {
    let romaji = kana.romaji;
    match romaji {
        "shi" => "s",
        "chi" | "tsu" => "t",
        "fu" => "h",
        "ji" => "z",
        _ => romaji
            .strip_suffix(|c| VOWELS.contains(c))
            .unwrap_or(romaji),
    }
}

pub struct Groups {
    keys: [&'static str; MAX_GROUPS],
    len: usize,
}

impl Groups {
    pub fn of(set: Set) -> Self {
        let mut groups = Self {
            keys: [""; MAX_GROUPS],
            len: 0,
        };
        for kana in KANA.iter().filter(|k| set.contains(k.kind)) {
            let key = group_key(kana);
            if !groups.keys().contains(&key) && groups.len < MAX_GROUPS {
                groups.keys[groups.len] = key;
                groups.len += 1;
            }
        }
        groups
    }

    pub fn keys(&self) -> &[&'static str] {
        &self.keys[..self.len]
    }
}

// Where a basic kana sits in the gojuon chart: (row, column), five columns by
// vowel; ん takes a row of its own. None for the other kinds.
pub fn chart_position(kana: &Kana) -> Option<(usize, usize)> {
    if kana.kind != Kind::Seion {
        return None;
    }
    let romaji = kana.romaji;
    if romaji == "n" {
        return Some((CHART_ROWS - 1, 0));
    }
    let column = VOWELS.find(romaji.chars().last()?)?;
    let row = match romaji.chars().next()? {
        _ if romaji.len() == 1 => 0,
        'k' => 1,
        's' => 2,
        't' | 'c' => 3,
        'n' => 4,
        'h' | 'f' => 5,
        'm' => 6,
        'y' => 7,
        'r' => 8,
        'w' => 9,
        _ => return None,
    };
    Some((row, column))
}

pub const CHART_ROWS: usize = 11;
pub const CHART_COLS: usize = 5;

fn step_index(i: usize, count: usize, forward: bool) -> usize {
    if forward {
        (i + 1) % count
    } else {
        (i + count - 1) % count
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Card {
    kana: &'static Kana,
    katakana: bool,
    reverse: bool,
}

impl Card {
    pub fn from_slot(slot: usize) -> Self {
        let face = slot % SLOTS;
        Self {
            kana: &KANA[face >> 1],
            katakana: face & 1 != 0,
            reverse: slot >= SLOTS,
        }
    }

    // asked as romaji, answered with the kana
    pub fn reverse(self) -> bool {
        self.reverse
    }

    pub fn glyph(self) -> &'static str {
        if self.katakana {
            self.kana.katakana
        } else {
            self.kana.hiragana
        }
    }

    pub fn romaji(self) -> &'static str {
        self.kana.romaji
    }

    pub fn script(self) -> Script {
        if self.katakana {
            Script::Katakana
        } else {
            Script::Hiragana
        }
    }
}

pub struct Deck {
    slots: [u16; PROGRESS_SLOTS],
    len: usize,
}

impl Deck {
    pub const fn empty() -> Self {
        Self {
            slots: [0; PROGRESS_SLOTS],
            len: 0,
        }
    }

    // Loanword sounds exist in katakana only: that set forces katakana, and
    // the hiragana script leaves them out of All. `group` keeps one row key.
    pub fn build(script: Script, set: Set, direction: Direction, group: Option<&str>) -> Self {
        let mut deck = Self::empty();
        for (i, kana) in KANA.iter().enumerate() {
            if !set.contains(kana.kind) || group.is_some_and(|g| g != group_key(kana)) {
                continue;
            }
            let slot = (i as u16) << 1;
            if kana.kind == Kind::Gaikoku {
                if script != Script::Hiragana || set == Set::Loanword {
                    deck.push_faces(slot | 1, direction);
                }
                continue;
            }
            if matches!(script, Script::Hiragana | Script::Both) {
                deck.push_faces(slot, direction);
            }
            if matches!(script, Script::Katakana | Script::Both) {
                deck.push_faces(slot | 1, direction);
            }
        }
        deck
    }

    fn push_faces(&mut self, face: u16, direction: Direction) {
        if direction != Direction::Reverse {
            self.push(face);
        }
        if direction != Direction::Forward {
            self.push(face + SLOTS as u16);
        }
    }

    fn push(&mut self, slot: u16) {
        self.slots[self.len] = slot;
        self.len += 1;
    }

    pub fn slots(&self) -> &[u16] {
        &self.slots[..self.len]
    }
}

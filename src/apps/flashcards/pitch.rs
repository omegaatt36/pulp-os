// Pitch accent of a reading, drawn as a contour over its kana: each mora sits
// high or low, and a drop falls after the accent's mora. The deck's fourth
// column carries the accent numbers ("0", "2", "0,2"): 0 is flat (low, then
// high), n is the mora after which the pitch falls (1 is high first).

pub const MAX_ACCENTS: usize = 2;
pub const MAX_MORAE: usize = 32;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Span {
    pub x0: u16,
    pub x1: u16,
}

// The accents of a deck's pitch column that fit a reading of `morae` morae (an
// accent past the last one is a mistake in the deck), in order; the count.
pub fn accents(field: &str, morae: usize, out: &mut [u8; MAX_ACCENTS]) -> usize {
    let mut n = 0;
    let fits = field
        .split(',')
        .filter_map(|v| v.trim().parse::<u8>().ok())
        .filter(|&v| usize::from(v) <= morae);
    for v in fits.take(MAX_ACCENTS) {
        out[n] = v;
        n += 1;
    }
    n
}

// A small kana folds into the mora before it (きょ, フォ); every other kana,
// the long-vowel mark and the sokuon included, is a mora of its own.
fn folds(ch: char) -> bool {
    matches!(
        ch,
        'ゃ' | 'ゅ'
            | 'ょ'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
    )
}

// The horizontal extent of each mora of `kana` when set from x = 0 with the
// given advance; the count, or 0 when the reading holds more than MAX_MORAE.
pub fn spans(kana: &str, advance: impl Fn(char) -> u16, out: &mut [Span; MAX_MORAE]) -> usize {
    let (mut n, mut x) = (0, 0u16);
    for ch in kana.chars() {
        let w = advance(ch);
        if folds(ch) && n > 0 {
            out[n - 1].x1 = x.saturating_add(w);
        } else {
            if n == MAX_MORAE {
                return 0;
            }
            out[n] = Span {
                x0: x,
                x1: x.saturating_add(w),
            };
            n += 1;
        }
        x = x.saturating_add(w);
    }
    n
}

// Whether mora `i` (0-based) is high under accent `drop`.
pub fn is_high(i: usize, drop: u8) -> bool {
    match (i, drop) {
        (0, d) => d == 1,
        (_, 0) => true,
        (i, d) => i < usize::from(d),
    }
}

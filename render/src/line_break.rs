// line-break opportunities between two adjacent scalars
//
// spaces are handled by the layout itself; this module decides breaks
// that need no space: CJK text breaks between any two characters, except
// where the prohibition rules forbid it
//
// prohibition sets follow W3C "Requirements for Chinese Text Layout"
// (clreq, W3C Group Note), §6.1.1, level "basic" (the recommended one):
//   not at line start: pause/stop marks 、，；：。！？, closing quotation
//     marks, closing parentheses and brackets, closing book-title marks
//     〉》, connector marks ～, interpuncts ·
//   not at line end: opening quotation marks, opening parentheses and
//     brackets, opening book-title marks 〈《
// plus JIS X 4051 cl-04 ‼⁇⁈⁉ and the ASCII counterparts of the
// brackets and pause/stop marks (as in W3C jlreq cl-01/02/04..07), which
// matter only where Latin text touches CJK text
//
// §6.1.2 (a two-em dash —— or ellipsis …… is not split) holds without a
// rule of its own: — and … are not CJK-class, so no break is ever
// offered between two of them

// line-start prohibited: closing punctuation
#[inline]
pub fn no_start(ch: char) -> bool {
    matches!(
        ch,
        // pause or stop marks, incl. fullwidth / halfwidth forms
        '、' | '，' | '；' | '：' | '。' | '！' | '？' | '．' | '｡' | '､'
        | '‼' | '⁇' | '⁈' | '⁉'
        // closing quotation marks
        | '”' | '’' | '」' | '』' | '｣' | '〞' | '〟'
        // closing parentheses, brackets, book-title marks
        | '）' | '］' | '｝' | '〕' | '】' | '〗' | '〙' | '〛' | '｠' | '〉' | '》'
        // connector marks and interpuncts
        | '～' | '〜' | '·' | '・' | '‧' | '･'
        // ASCII counterparts
        | ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}'
    )
}

// line-end prohibited: opening punctuation
#[inline]
pub fn no_end(ch: char) -> bool {
    matches!(
        ch,
        '“' | '‘'
            | '「'
            | '『'
            | '｢'
            | '〝'
            | '（'
            | '［'
            | '｛'
            | '〔'
            | '【'
            | '〖'
            | '〘'
            | '〚'
            | '｟'
            | '〈'
            | '《'
            | '('
            | '['
            | '{'
    )
}

// characters that break individually, without spaces: Han ideographs,
// kana, bopomofo, CJK symbols and punctuation, fullwidth forms
// (Hangul is excluded: Korean breaks at spaces)
#[inline]
pub fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2FDF     // CJK radicals, Kangxi radicals
        | 0x3000..=0x303F   // CJK symbols and punctuation
        | 0x3040..=0x30FF   // hiragana, katakana
        | 0x3100..=0x312F   // bopomofo
        | 0x31A0..=0x31FF   // bopomofo extended, CJK strokes, katakana ext.
        | 0x3400..=0x4DBF   // CJK extension A
        | 0x4E00..=0x9FFF   // CJK unified ideographs
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xFE30..=0xFE4F   // CJK compatibility forms
        | 0xFF00..=0xFFEF   // halfwidth and fullwidth forms
        | 0x20000..=0x3FFFF // CJK extensions B.., supplementary planes 2-3
    )
}

// true if a line may break between prev and next with no space: one of
// them is CJK, next may start a line, and prev may end one
#[inline]
pub fn break_between(prev: char, next: char) -> bool {
    if prev.is_ascii() && next.is_ascii() {
        return false;
    }
    (is_cjk(prev) || is_cjk(next)) && !no_start(next) && !no_end(prev)
}

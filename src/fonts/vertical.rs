// vertical writing (tategaki): which way each scalar is set in a column
//
// A column reads top to bottom, columns run right to left. The packs and the
// built-in fonts only hold horizontal glyphs, so a scalar is set one of three
// ways:
//   Upright   the horizontal glyph, centred in the column; advance is the em
//             (every ideograph, kana and fullwidth form)
//   Form      the font's own vertical presentation form (U+FE10.., U+FE30..)
//             when the pack has it, else the horizontal glyph turned a quarter
//             clockwise inside the em box (brackets, dashes, ellipsis)
//   Sideways  the horizontal glyph turned a quarter clockwise on its own
//             advance (Latin text and longer digit runs, long vowel mark, tildes)
//
// All glyph frames hang from one em box: its top is the pen position and the
// horizontal baseline sits `em_ascent(px)` below it.

/// Page-start state: the preceding page ended inside an ASCII digit run.
/// Kept outside the style bits in the reader's existing page flags.
pub const NUMBER_CONTINUED: u8 = 1 << 6;

/// Tate-chu-yoko policy: a complete run of one or two ASCII digits occupies
/// one em, upright and horizontal. Longer runs keep their sideways scalar
/// layout, including their tails after column, page and read-window breaks.
/// Markup terminates a run; ruby contents retain their own scalar layout.
/// This peeks at most three following bytes; callers must defer an unfinished
/// short run at a read edge.
pub fn short_number_len(buf: &[u8], at: usize, continued: bool) -> usize {
    if (at == 0 && continued)
        || (at > 0 && buf[at - 1].is_ascii_digit())
        || !buf.get(at).is_some_and(u8::is_ascii_digit)
    {
        return 0;
    }
    let len = buf[at..]
        .iter()
        .take(3)
        .take_while(|b| b.is_ascii_digit())
        .count();
    if len <= 2 { len } else { 0 }
}

pub enum Setting {
    Upright,
    Form { form: char, turn_if_absent: bool },
    // advance is the em
    Turned,
}

/// Distance from the top of the em box to the horizontal baseline. CJK fonts
/// put the ideographic em box from 0.12 em below to 0.88 em above the baseline.
pub const fn em_ascent(px: u16) -> i32 {
    (px as i32 * 7 + 4) / 8
}

/// How a scalar the CJK pack supplies is set in a column.
pub fn setting(ch: char) -> Setting {
    if let Some(form) = presentation_form(ch) {
        return Setting::Form {
            form,
            // a comma or full stop turned on its side reads wrong; the upright
            // glyph is what a font without the form would show anyway
            turn_if_absent: !matches!(ch, '，' | '、' | '。' | '：' | '；'),
        };
    }
    match ch {
        'ー' | '〜' | '～' | '─' | '━' | '⋯' => Setting::Turned,
        _ => Setting::Upright,
    }
}

/// The vertical presentation form of a scalar (Unicode `<vertical>`
/// decompositions of U+FE10..FE19 and U+FE30..FE48), where it has one.
pub fn presentation_form(ch: char) -> Option<char> {
    Some(match ch {
        '，' => '\u{FE10}',
        '、' => '\u{FE11}',
        '。' => '\u{FE12}',
        '：' => '\u{FE13}',
        '；' => '\u{FE14}',
        '…' => '\u{FE19}',
        '‥' => '\u{FE30}',
        '—' => '\u{FE31}',
        '–' => '\u{FE32}',
        '（' => '\u{FE35}',
        '）' => '\u{FE36}',
        '｛' => '\u{FE37}',
        '｝' => '\u{FE38}',
        '〔' => '\u{FE39}',
        '〕' => '\u{FE3A}',
        '【' => '\u{FE3B}',
        '】' => '\u{FE3C}',
        '《' => '\u{FE3D}',
        '》' => '\u{FE3E}',
        '〈' => '\u{FE3F}',
        '〉' => '\u{FE40}',
        '「' => '\u{FE41}',
        '」' => '\u{FE42}',
        '『' => '\u{FE43}',
        '』' => '\u{FE44}',
        _ => return None,
    })
}

/// Top-left of a bitmap turned a quarter clockwise (`StripCore::blit_1bpp_cw90`)
/// inside the em box whose top-left is (`left`, `top`) and whose side is `px`.
/// `offset_x`, `offset_y` and `height` are the horizontal glyph's: the turned
/// glyph keeps its baseline `em_ascent(px)` from the box edge, tops to the
/// right, and reads down from `top + offset_x`.
pub fn turned_origin(
    left: i32,
    top: i32,
    px: u16,
    offset_x: i32,
    offset_y: i32,
    height: i32,
) -> (i32, i32) {
    (
        left + i32::from(px) - em_ascent(px) - offset_y - height,
        top + offset_x,
    )
}

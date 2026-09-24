// line and page wrapping over fixed mixed-script fixtures
//
// oracle: every expected span is hand-derived from the fake measurer's
// advances (below) and UTF-8 lengths per RFC 3629 (CJK ideographs and
// CJK punctuation U+3000..U+30FF / U+FF00..U+FFEF are 3 bytes each,
// '…' U+2026 is 3 bytes, ASCII is 1 byte). the derivation is written
// next to each fixture as cursor arithmetic; nothing is copied from
// running the implementation
//
// break rules under test (see render/src/line_break.rs for the cited
// sets): a break opportunity exists between two scalars when either is
// CJK, unless the second is line-start prohibited (closing punctuation
// such as ，。」) or the first is line-end prohibited (opening
// punctuation such as 「); spaces are always break opportunities

use core::convert::Infallible;

use pulp_render::layout::{
    LineSpan, Markup, Measure, PageProgressError, Style, WrapParams, WrapState, Wrapped,
    line_glyphs, next_page_offset, page_has_content, wrap, wrap_stream,
};

// fake measurer, hand-chosen advances in px:
//   ' ' and NBSP: 3 in every style
//   other ASCII:  Regular 5, Bold 6, Italic 4, Heading 8
//   non-ASCII:    10 (every CJK ideograph and CJK punctuation)
struct Fake;

impl Measure for Fake {
    type Error = Infallible;

    fn advance(&mut self, ch: char, style: Style) -> Result<u32, Infallible> {
        Ok(match ch {
            ' ' | '\u{A0}' => 3,
            c if c.is_ascii() => match style {
                Style::Regular => 5,
                Style::Bold => 6,
                Style::Italic => 4,
                Style::Heading => 8,
            },
            _ => 10,
        })
    }

    fn line_height(&self, _style: Style) -> u16 {
        20
    }
}

// markup byte codes of the smol-epub html_strip stream format
// ([MARKER, code] pairs, [MARKER, IMG_REF, len, path..] images)
const M: u8 = 0x01;
const MARKUP: Markup = Markup {
    marker: M,
    img_ref: b'P',
    bold_on: b'B',
    bold_off: b'b',
    italic_on: b'I',
    italic_off: b'i',
    heading_on: b'H',
    heading_off: b'h',
    quote_on: b'Q',
    quote_off: b'q',
};

const INDENT_PX: u32 = 10;

fn params(max_lines: usize, width: u32) -> WrapParams<'static> {
    WrapParams {
        markup: MARKUP,
        max_lines,
        max_width_px: width,
        indent_px: INDENT_PX,
        img_heights: &[],
        default_img_h: 40,
    }
}

// wrap one chunk; returns the result and the emitted spans
fn run(buf: &[u8], eof: bool, max_lines: usize, width: u32) -> (Wrapped, Vec<LineSpan>) {
    let mut lines = vec![LineSpan::EMPTY; max_lines];
    let out = wrap(buf, eof, &mut Fake, &params(max_lines, width), &mut lines).unwrap();
    lines.truncate(out.line_count);
    (out, lines)
}

// spans as absolute (start, end) byte ranges
fn ranges(spans: &[LineSpan], base: usize) -> Vec<(usize, usize)> {
    spans
        .iter()
        .map(|s| {
            let st = base + s.start as usize;
            (st, st + s.len as usize)
        })
        .collect()
}

// separators the algorithm drops between two text spans: the newline
// (with a CR before it), a single space / NBSP that itself overflowed
// the line, or nothing (break inside text: CJK or after a kept space)
const DROPPED: &[&[u8]] = &[b"", b"\n", b"\r\n", b" ", "\u{A0}".as_bytes()];

// R7 oracle: spans are in order, each starts and ends on a scalar
// boundary, and spans plus the dropped separators between them cover
// text[from..to] exactly once: nothing omitted, nothing duplicated
fn assert_reconstructs(text: &[u8], spans: &[(usize, usize)], from: usize, to: usize) {
    let s = core::str::from_utf8(text).unwrap();
    let mut pos = from;
    let mut rebuilt: Vec<u8> = Vec::new();
    for &(st, en) in spans {
        assert!(st >= pos, "span {st}..{en} overlaps previous end {pos}");
        let gap = &text[pos..st];
        assert!(
            DROPPED.contains(&gap),
            "gap {gap:?} at {pos}..{st} is not a separator"
        );
        assert!(
            s.is_char_boundary(st) && s.is_char_boundary(en),
            "{st}..{en} splits a scalar"
        );
        rebuilt.extend_from_slice(gap);
        rebuilt.extend_from_slice(&text[st..en]);
        pos = en;
    }
    let tail = &text[pos..to];
    assert!(
        DROPPED.contains(&tail),
        "tail {tail:?} at {pos}..{to} dropped"
    );
    rebuilt.extend_from_slice(tail);
    assert_eq!(rebuilt, &text[from..to]);
}

// ---------------------------------------------------------------- R7

// byte offsets of the mixed fixture (3-byte CJK, 1-byte ASCII):
//   中0 文3 English6..13 混13 排16 ，19 測22 試25 。28 Hello31..36
//   ' '36 世37 界40 「43 引46 號49 」52  -> 55 bytes
const MIXED: &str = "中文English混排，測試。Hello 世界「引號」";

#[test]
fn mixed_script_width_55() {
    // line 1: 中10 文20 E25 n30 g35 l40 i45 s50 h55; 混 -> 65 > 55,
    //   break before 混 (Latin|CJK) -> 0..13
    // line 2: 混10 排20 ，30 測40 試50 。60 > 55; no break before 。,
    //   last opportunity before 試 (x=40) -> 13..25, carry 試。 = 20
    // line 3: H25 (opportunity 31) e30 l35 l40 o45 ' '48 (opportunity 37);
    //   世 -> 58 > 55, break after the space -> 25..37 (space kept)
    // line 4: 世10 界20 「30 (opportunity 43 at x=20) 引40 (none: after 「)
    //   號50 (opportunity 49 at x=40) 」60 > 55; no break before 」,
    //   last opportunity before 號 -> 37..49
    // line 5: 號」 -> 49..55
    let text = MIXED.as_bytes();
    let (out, spans) = run(text, true, 10, 55);
    let r = ranges(&spans, 0);
    assert_eq!(r, [(0, 13), (13, 25), (25, 37), (37, 49), (49, 55)]);
    assert_eq!(out.consumed, 55);
    assert_reconstructs(text, &r, 0, 55);
}

#[test]
fn mixed_script_width_60() {
    // line 1: as above, 混 -> 65 > 60 -> 0..13
    // line 2: 混10 排20 ，30 測40 試50 。60 (fits); H -> 65 > 60, break
    //   before H (after 。 is legal) -> 13..31
    // line 3: H5 e10 l15 l20 o25 ' '28 世38 界48 (opportunity 40)
    //   「58 (opportunity 43 at x=48); 引 -> 68 > 60, no break after 「,
    //   so break before 「 -> 31..43; a char break would end on 「
    // line 4: 「引號」 = 40 -> 43..55
    let text = MIXED.as_bytes();
    let (out, spans) = run(text, true, 10, 60);
    let r = ranges(&spans, 0);
    assert_eq!(r, [(0, 13), (13, 31), (31, 43), (43, 55)]);
    assert_eq!(out.consumed, 55);
    assert_reconstructs(text, &r, 0, 55);
}

#[test]
fn overflowing_space_is_the_only_dropped_byte() {
    // a0..j10 = 50 (fits); ' ' -> 53 > 50: the space is dropped -> 0..10,
    // next line starts at 11: 世界 = 20 -> 11..17
    let text = "abcdefghij 世界".as_bytes();
    let (_, spans) = run(text, true, 10, 50);
    let r = ranges(&spans, 0);
    assert_eq!(r, [(0, 10), (11, 17)]);
    assert_reconstructs(text, &r, 0, text.len());
}

#[test]
fn newlines_and_crlf_are_dropped_separators() {
    // 中0 文3 \r6 \n7 a8 b9 c10 \n11 \n12 世13 -> 16 bytes
    // \n at 7 ends 0..7, trailing CR trimmed -> 0..6; \n at 11 -> 8..11;
    // \n at 12 -> empty 12..12; end -> 13..16
    let text = "中文\r\nabc\n\n世".as_bytes();
    let (_, spans) = run(text, true, 10, 100);
    let r = ranges(&spans, 0);
    assert_eq!(r, [(0, 6), (8, 11), (12, 12), (13, 16)]);
    assert_reconstructs(text, &r, 0, text.len());
}

const CHUNK: usize = 8192;

// "a" + 3000 x 中: 中 k starts at 1 + 3k, so the scalar at 8191..8194
// straddles the 8192-byte chunk boundary; 9001 bytes total
fn straddle_text() -> Vec<u8> {
    let mut t = b"a".to_vec();
    for _ in 0..3000 {
        t.extend_from_slice("中".as_bytes());
    }
    t
}

// reader simulation: each page reads text[offset..offset + CHUNK]; the
// chunk is at EOF when it reaches the end of text; the next page starts
// at offset + consumed. returns (page offsets incl. final, all spans)
fn paginate(text: &[u8], max_lines: usize, width: u32) -> (Vec<usize>, Vec<(usize, usize)>) {
    let mut offsets = vec![0usize];
    let mut all = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let end = (offset + CHUNK).min(text.len());
        let (out, spans) = run(&text[offset..end], end == text.len(), max_lines, width);
        assert!(out.consumed > 0, "no progress at {offset}");
        all.extend(ranges(&spans, offset));
        offset = next_page_offset(offset, out.consumed, text.len())
            .expect("page must advance")
            .unwrap_or(text.len());
        offsets.push(offset);
    }
    (offsets, all)
}

#[test]
fn underfilled_chunk_with_split_scalar_still_has_a_next_page() {
    let mut text = vec![b'\r'; 8190];
    text.extend_from_slice("中A".as_bytes());

    let (first, _) = run(&text[..8192], false, 37, 100);
    assert_eq!(first.consumed, 8190);
    assert_eq!(first.line_count, 1);
    assert_eq!(
        next_page_offset(0, first.consumed, text.len()),
        Ok(Some(8190))
    );

    let (second, spans) = run(&text[8190..], true, 37, 100);
    assert_eq!(second.consumed, 4);
    assert_eq!(
        next_page_offset(8190, second.consumed, text.len()),
        Ok(None)
    );
    assert_eq!(spans.len(), 1);
    assert_eq!(&text[8190..8190 + spans[0].len as usize], "中A".as_bytes());
}

#[test]
fn closing_punctuation_after_a_full_chunk_stays_with_its_predecessor() {
    let mut text = vec![b'\r'; 8189];
    text.extend_from_slice("中，A".as_bytes());

    let mut lines = [LineSpan::EMPTY; 37];
    let first = wrap_stream(
        &text[..8192],
        false,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(first.line_count, 0);
    assert_eq!(first.consumed, 8189);

    // The reader advances the current page cursor rather than adding an
    // empty page, then re-reads from the deferred glyph.
    let second = wrap_stream(
        &text[8189..],
        true,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        first.next_state,
    )
    .unwrap();
    assert_eq!(second.consumed, "中，A".len());
    assert_eq!(second.line_count, 1);
    assert_eq!(
        line_glyphs(&text[8189..], &lines[0], MARKUP)
            .map(|v| v.0)
            .collect::<String>(),
        "中，A"
    );
}

#[test]
fn deferred_tail_keeps_bold_and_quote_state_at_its_page_offset() {
    let mut text = vec![b'\r'; 8185];
    text.extend_from_slice(&[M, b'B', M, b'Q']);
    text.extend_from_slice("中，A".as_bytes());
    let mut lines = [LineSpan::EMPTY; 37];
    let first = wrap_stream(
        &text[..8192],
        false,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(first.consumed, 8189);
    assert_eq!(first.line_count, 0);
    assert_eq!(first.next_state.flags, LineSpan::FLAG_BOLD);
    assert_eq!(first.next_state.indent, 1);

    let second = wrap_stream(
        &text[8189..],
        true,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        first.next_state,
    )
    .unwrap();
    assert_eq!(second.line_count, 1);
    assert_eq!(lines[0].flags, LineSpan::FLAG_BOLD);
    assert_eq!(lines[0].indent, 1);
    assert_eq!(
        line_glyphs(&text[8189..], &lines[0], MARKUP).collect::<Vec<_>>(),
        [('中', Style::Bold), ('，', Style::Bold), ('A', Style::Bold)]
    );
}

#[test]
fn streaming_checkpoint_uses_style_at_break_not_scanner_end() {
    let text = [b'a', b' ', M, b'B', b'b', b'c'];
    let mut lines = [LineSpan::EMPTY; 1];
    let out = wrap_stream(
        &text,
        false,
        &mut Fake,
        &params(1, 14),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(out.consumed, 2);
    assert_eq!(out.next_state.flags, 0);
    assert_eq!(out.line_count, 1);
}

#[test]
fn unbreakable_full_window_forces_a_bounded_line() {
    let text = vec![b'a'; CHUNK + 1];
    let mut lines = [LineSpan::EMPTY; 2];
    let out = wrap_stream(
        &text[..CHUNK],
        false,
        &mut Fake,
        &params(2, 100_000),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(out.consumed, CHUNK);
    assert_eq!(out.line_count, 1);
    assert_eq!(lines[0].len as usize, CHUNK);
}

#[test]
fn marker_only_chunk_advances_style_without_creating_a_line() {
    let mut text = vec![b'\r'; CHUNK - 4];
    text.extend_from_slice(&[M, b'B', M, b'Q']);
    text.extend_from_slice("中，".as_bytes());
    let mut lines = [LineSpan::EMPTY; 37];
    let first = wrap_stream(
        &text[..CHUNK],
        false,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(first.consumed, CHUNK);
    assert_eq!(first.line_count, 0);
    assert_eq!(first.next_state.flags, LineSpan::FLAG_BOLD);
    assert_eq!(first.next_state.indent, 1);
    let second = wrap_stream(
        &text[CHUNK..],
        true,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        first.next_state,
    )
    .unwrap();
    assert_eq!(second.line_count, 1);
    assert_eq!(lines[0].indent, 1);
    assert_eq!(lines[0].flags, LineSpan::FLAG_BOLD);
}

#[test]
fn resumed_quote_applies_indent_width_from_first_scalar() {
    let mut lines = [LineSpan::EMPTY; 1];
    let out = wrap_stream(
        b"abc",
        true,
        &mut Fake,
        &params(1, 20),
        &mut lines,
        WrapState {
            flags: 0,
            indent: 1,
        },
    )
    .unwrap();
    assert_eq!(out.consumed, 2);
    assert_eq!(out.line_count, 1);
    assert_eq!(lines[0].indent, 1);
    assert_eq!(lines[0].len, 2);
}

#[test]
fn indexed_page_states_replay_after_a_styled_chunk_boundary() {
    let mut text = vec![b'\r'; 8185];
    text.extend_from_slice(&[M, b'B', M, b'Q']);
    text.extend_from_slice("中，A中、B".as_bytes());
    let mut pages = Vec::new();
    let mut offset = 0;
    let mut state = WrapState::default();
    while offset < text.len() {
        let end = (offset + CHUNK).min(text.len());
        let mut lines = [LineSpan::EMPTY; 1];
        let out = wrap_stream(
            &text[offset..end],
            end == text.len(),
            &mut Fake,
            &params(1, 40),
            &mut lines,
            state,
        )
        .unwrap();
        if out.line_count > 0 {
            let drawn: String = line_glyphs(&text[offset..end], &lines[0], MARKUP)
                .map(|(ch, _)| ch)
                .collect();
            pages.push((offset, state, lines[0], drawn));
        }
        offset = next_page_offset(offset, out.consumed, text.len())
            .unwrap()
            .unwrap_or(text.len());
        state = out.next_state;
    }

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].0, 8189);
    assert_eq!(pages[0].3, "中，A");
    assert_eq!(pages[1].3, "中、B");
    for (_, state, span, _) in &pages {
        assert_eq!(state.flags, LineSpan::FLAG_BOLD);
        assert_eq!(state.indent, 1);
        assert_eq!(span.flags, LineSpan::FLAG_BOLD);
        assert_eq!(span.indent, 1);
    }

    // A backward visit must reconstruct the same page from its own cursor.
    let (start, state, _, want) = &pages[0];
    let mut lines = [LineSpan::EMPTY; 1];
    let replay = wrap_stream(
        &text[*start..],
        true,
        &mut Fake,
        &params(1, 40),
        &mut lines,
        *state,
    )
    .unwrap();
    assert_eq!(replay.line_count, 1);
    let drawn: String = line_glyphs(&text[*start..], &lines[0], MARKUP)
        .map(|(ch, _)| ch)
        .collect();
    assert_eq!(&drawn, want);
}

#[test]
fn opening_punctuation_after_a_full_chunk_joins_the_prior_glyph() {
    let mut text = vec![b'\r'; 8189];
    text.extend_from_slice("中「A".as_bytes());
    let mut lines = [LineSpan::EMPTY; 37];
    let first = wrap_stream(
        &text[..CHUNK],
        false,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(first.consumed, 8189);
    let second = wrap_stream(
        &text[8189..],
        true,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        first.next_state,
    )
    .unwrap();
    assert_eq!(second.line_count, 1);
    assert_eq!(
        line_glyphs(&text[8189..], &lines[0], MARKUP)
            .map(|(ch, _)| ch)
            .collect::<String>(),
        "中「A"
    );
}

#[test]
fn trailing_newline_after_full_control_heavy_chunk_is_not_a_page() {
    let mut text = b"a".to_vec();
    text.extend(core::iter::repeat_n(b'\r', CHUNK - 1));
    text.push(b'\n');
    let mut lines = [LineSpan::EMPTY; 37];
    let first = wrap_stream(
        &text[..CHUNK],
        false,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        WrapState::default(),
    )
    .unwrap();
    assert_eq!(first.consumed, CHUNK);
    assert!(page_has_content(
        &text[..CHUNK],
        &lines[..first.line_count],
        MARKUP
    ));
    let second = wrap_stream(
        &text[CHUNK..],
        true,
        &mut Fake,
        &params(37, 100),
        &mut lines,
        first.next_state,
    )
    .unwrap();
    assert_eq!(second.consumed, 1);
    assert!(!page_has_content(
        &text[CHUNK..],
        &lines[..second.line_count],
        MARKUP
    ));
}

#[test]
fn page_indexing_reports_no_progress_before_eof() {
    assert_eq!(
        next_page_offset(0, 0, 8194),
        Err(PageProgressError::Stalled)
    );
    assert_eq!(
        next_page_offset(8190, 8, 8194),
        Err(PageProgressError::BeyondEnd)
    );
}

#[test]
fn page_offsets_stay_on_scalars_across_the_chunk_boundary() {
    // width 100 = ten CJK. page 1 (chunk 0..8192, not EOF): the complete
    // prefix is 8191 (中 at 8191 is cut). line 0 = a5 + 9 CJK = 95, the
    // 10th CJK overflows -> 0..28; then 30-byte lines 28+30(k-1)..28+30k.
    // 8191 - 28 = 8163 = 272*30 + 3 -> 272 full lines ending at 8188 plus
    // the unfinished line 8188..8191. consumed = 8191, 274 lines
    // page 2 (8191..9001, EOF): 810 bytes = 270 CJK = 27 lines of 30
    let text = straddle_text();
    let (offsets, spans) = paginate(&text, 300, 100);
    assert_eq!(offsets, [0, 8191, 9001]);

    let mut expect = vec![(0, 28)];
    for k in 1..=272 {
        expect.push((28 + 30 * (k - 1), 28 + 30 * k));
    }
    expect.push((8188, 8191));
    for j in 0..27 {
        expect.push((8191 + 30 * j, 8191 + 30 * (j + 1)));
    }
    assert_eq!(spans, expect);
    assert_reconstructs(&text, &spans, 0, text.len());
}

#[test]
fn full_pages_end_on_line_starts() {
    // max_lines 100, width 100: page 1 = line 0 (28 bytes) + 99 x 30
    // = 2998; pages 2 and 3 = 100 x 30 = 3000 each -> 5998, 8998;
    // page 4 = the last 中 (3 bytes) -> 9001
    let text = straddle_text();
    let (offsets, spans) = paginate(&text, 100, 100);
    assert_eq!(offsets, [0, 2998, 5998, 8998, 9001]);
    let s = core::str::from_utf8(&text).unwrap();
    assert!(offsets.iter().all(|&o| s.is_char_boundary(o)));
    assert_eq!(spans.len(), 301);
    assert_reconstructs(&text, &spans, 0, text.len());
}

#[test]
fn truncated_tail_is_held_back_before_eof_and_kept_at_eof() {
    // 中文 (6 bytes) + the first 2 bytes of 中 (E4 B8)
    let mut text = "中文".as_bytes().to_vec();
    text.extend_from_slice(&"中".as_bytes()[..2]);

    let (out, spans) = run(&text, false, 10, 100);
    assert_eq!(out.consumed, 6);
    assert_eq!(ranges(&spans, 0), [(0, 6)]);

    // at EOF the cut sequence is one U+FFFD and stays in the span
    let (out, spans) = run(&text, true, 10, 100);
    assert_eq!(out.consumed, 8);
    assert_eq!(ranges(&spans, 0), [(0, 8)]);
}

// ---------------------------------------------------------------- R8
// width 100 = exactly ten CJK; the 11th scalar overflows. a naive
// character break would end line 1 at byte 30 (ten scalars x 3)

#[test]
fn comma_does_not_start_a_line() {
    // 一..十 = 100, ， -> 110; no break before ，; last opportunity is
    // before 十 (byte 27) -> 0..27, 27..33 (十，)
    let (_, spans) = run("一二三四五六七八九十，".as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 27), (27, 33)]);
}

#[test]
fn full_stop_does_not_start_a_line() {
    let (_, spans) = run("一二三四五六七八九十。".as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 27), (27, 33)]);
}

#[test]
fn closing_bracket_does_not_start_a_line() {
    // 「一..九 = 100, 」 -> 110; no break before 」, last opportunity
    // before 九 (byte 27) -> 「一二三四五六七八 0..27, 九」 27..33
    let (_, spans) = run("「一二三四五六七八九」".as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 27), (27, 33)]);
}

#[test]
fn opening_bracket_does_not_end_a_line() {
    // 一..九 = 90, 「 = 100 (fits), 十 -> 110; no break after 「, so the
    // break goes before 「 (byte 27) -> 0..27, 「十」 27..36
    let (_, spans) = run("一二三四五六七八九「十」".as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 27), (27, 36)]);
}

#[test]
fn ellipsis_pair_is_not_split() {
    // 一..九 = 90, … = 100, second … -> 110; no break between the two
    // (inseparable), last opportunity before the first … (27)
    // -> 0..27, …… 27..33
    let (_, spans) = run("一二三四五六七八九……".as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 27), (27, 33)]);
}

#[test]
fn only_punctuation_falls_back_to_a_character_break() {
    // twelve ， : no legal opportunity anywhere; the 11th overflows and
    // is force-broken -> 0..30, 30..36
    let text = "，".repeat(12);
    let (out, spans) = run(text.as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 30), (30, 36)]);
    assert_eq!(out.consumed, 36);
}

#[test]
fn ideograph_then_punctuation_run_falls_back_too() {
    // 一 + ten ， : the only opportunity is before 一 (the line start,
    // unusable); ， #10 (byte 30) -> 110 overflows, forced -> 0..30, 30..33
    let text = format!("一{}", "，".repeat(10));
    let (_, spans) = run(text.as_bytes(), true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 30), (30, 33)]);
}

// ---------------------------------------------------------- regression
// Latin text keeps the pre-extraction break positions: words break at
// spaces (the space stays at the end of the line unless it overflows
// itself), a word longer than the line breaks at the overflowing char

#[test]
fn latin_words_break_at_spaces() {
    // T5 h10 e15 ' '18 q23 u28 i33 c38 k43 ' '46 (opportunity 10);
    // b -> 51 > 50 -> 0..10 ("The quick "), carry b = 5; rest fits
    let (_, spans) = run(b"The quick brown fox", true, 10, 50);
    assert_eq!(ranges(&spans, 0), [(0, 10), (10, 19)]);
    assert!(spans.iter().all(|s| s.flags == 0 && s.indent == 0));
}

#[test]
fn long_latin_word_breaks_at_the_overflowing_char() {
    // six 5-px letters = 30 per line; the 7th overflows
    let (_, spans) = run(b"abcdefghijklm", true, 10, 30);
    assert_eq!(ranges(&spans, 0), [(0, 6), (6, 12), (12, 13)]);
}

#[test]
fn bold_markers_measure_in_style_and_newline_splits() {
    // a0 b1 ' '2 M3 B4 c5 d6 e7 f8 M9 b10 ' '11 g12 h13 \n14 i15 j16
    // a5 b10 ' '13 (opportunity 3); bold on: c19 d25 e31 > 30 -> 0..3,
    // carry cde = 18; f24; bold off; ' '27 (opportunity 12); g32 > 30
    // -> 3..12, carry g = 5; h10; \n -> 12..14; end -> 15..17
    // flags = style at the line's first byte: line 3..12 starts before
    // its bold-on marker, so every line is Regular (the renderer applies
    // in-line markers itself)
    let text = [
        b'a', b'b', b' ', M, b'B', b'c', b'd', b'e', b'f', M, b'b', b' ', b'g', b'h', b'\n', b'i',
        b'j',
    ];
    let (_, spans) = run(&text, true, 10, 30);
    assert_eq!(ranges(&spans, 0), [(0, 3), (3, 12), (12, 14), (15, 17)]);
    assert!(spans.iter().all(|s| s.flags == 0));
}

#[test]
fn italic_carries_into_the_wrapped_line() {
    // M0 I1 a2 b3 c4 ' '5 d6 e7 f8 M9 i10
    // italic at line start: a4 b8 c12 ' '15 (opportunity 6); d19 e23 > 20
    // -> 0..6, carry de = 8; f12; italic off; end -> 6..11
    // both lines start in italic
    let text = [M, b'I', b'a', b'b', b'c', b' ', b'd', b'e', b'f', M, b'i'];
    let (_, spans) = run(&text, true, 10, 20);
    assert_eq!(ranges(&spans, 0), [(0, 6), (6, 11)]);
    assert!(spans.iter().all(|s| s.flags == LineSpan::FLAG_ITALIC));
}

#[test]
fn quote_indent_narrows_and_covers_the_whole_quote() {
    // M0 Q1 a2 b3 c4 ' '5 d6 e7 f8 M9 q10 \n11 g12
    // quote on at line start: width 40 - 10 = 30. a5 b10 c15 ' '18
    // (opportunity 6); d23 e28 f33 > 30 -> 0..6; def\x01q -> 6..11 still
    // indented (quote closes after its text); g -> 12..13 unindented
    let text = [
        M, b'Q', b'a', b'b', b'c', b' ', b'd', b'e', b'f', M, b'q', b'\n', b'g',
    ];
    let (_, spans) = run(&text, true, 10, 40);
    assert_eq!(ranges(&spans, 0), [(0, 6), (6, 11), (12, 13)]);
    let indents: Vec<u8> = spans.iter().map(|s| s.indent).collect();
    assert_eq!(indents, [1, 1, 0]);
}

#[test]
fn image_reserves_lines_and_drops_its_header() {
    // x0 M1 P2 len3=2 p4 q5 y6: text x, then image path 4..6 with
    // default height 40 / line height 20 = 2 lines (origin + filler),
    // then y
    let text = [b'x', M, b'P', 2, b'p', b'q', b'y'];
    let (out, spans) = run(&text, true, 10, 100);
    assert_eq!(ranges(&spans, 0), [(0, 1), (4, 6), (0, 0), (6, 7)]);
    assert!(spans[1].is_image_origin());
    assert!(spans[2].is_image() && !spans[2].is_image_origin());
    assert_eq!(out.consumed, 7);
}

#[test]
fn page_fills_at_max_lines() {
    // width 30: "aaaaaa" per line; max_lines 2 stops after 12 bytes
    let (out, spans) = run(b"aaaaaaaaaaaaaaaaaa", true, 2, 30);
    assert_eq!(ranges(&spans, 0), [(0, 6), (6, 12)]);
    assert_eq!(out.consumed, 12);
    assert_eq!(out.line_count, 2);
}

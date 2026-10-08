// utf8 regression -- production UTF-8 decoding.
//
// Run: cargo test-host --test utf8
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// host/src/lib.rs must expose, as a PUBLIC module, the firmware's own
// kernel/src/util/utf8.rs (included with #[path], never a copy):
//
//   pub mod utf8;                       // currently a private `mod utf8;`
//
//   pulp_host::utf8::decode_utf8_char(buf: &[u8], pos: usize) -> (char, usize)
//       (decoded char, number of bytes consumed from `pos`)
//   pulp_host::utf8::Utf8Iter<'a>
//       Utf8Iter::new(data: &'a [u8]) -> Utf8Iter<'a>
//       impl Iterator<Item = char>
//       .position(&self) -> usize        // bytes consumed so far
//       .remaining(&self) -> &'a [u8]    // data[position()..]
//
// Oracle: RFC 3629 (the UTF-8 definition). Whether a byte sequence is valid
// UTF-8 is decided by `core::str::from_utf8` (an independent implementation of
// RFC 3629: no overlongs, no surrogates, nothing above U+10FFFF); expected
// characters come from `char::encode_utf8` / `str::chars`. Nothing here asks
// the code under test what it expects.
//
// What is deliberately NOT asserted: how many bytes an *invalid* sequence
// consumes beyond "at least one, never past the end of the buffer" (RFC 3629
// says such input must be rejected, not how a decoder resynchronises).
// ============================================================================

use pulp_host::utf8::{Utf8Iter, decode_utf8_char};

const FFFD: char = '\u{FFFD}';

// every Unicode scalar value, in order, skipping the surrogate range
fn all_scalars() -> impl Iterator<Item = char> {
    (0u32..=0x10FFFF).filter_map(char::from_u32)
}

#[test]
fn every_scalar_value_round_trips_with_its_exact_byte_length() {
    let mut buf = [0u8; 4];
    for c in all_scalars() {
        let s = c.encode_utf8(&mut buf);
        let n = s.len();
        assert_eq!(decode_utf8_char(s.as_bytes(), 0), (c, n), "U+{:04X}", c as u32);
    }
}

#[test]
fn decoding_honours_the_start_position_inside_a_larger_buffer() {
    let text = "ab\u{e9}\u{2014}c\u{20ac}\u{1f600}d";
    let bytes = text.as_bytes();
    let mut pos = 0;
    let mut got = Vec::new();
    for want in text.chars() {
        let (c, n) = decode_utf8_char(bytes, pos);
        assert_eq!(c, want, "at byte {pos}");
        assert_eq!(n, want.len_utf8(), "length of {want:?}");
        got.push(c);
        pos += n;
    }
    assert_eq!(pos, bytes.len());
    assert_eq!(got.iter().collect::<String>(), text);
}

#[test]
fn utf8_iter_yields_exactly_the_chars_of_valid_text() {
    let texts = [
        "",
        "plain ascii",
        "caf\u{e9} na\u{ef}ve \u{dc}ber se\u{f1}or \u{c5}ngstr\u{f6}m",
        "\u{201c}quoted\u{201d} \u{2014} it\u{2019}s wait\u{2026} \u{20ac}5",
        "mixed \u{4e2d}\u{6587} and \u{1f600} four-byte",
    ];
    for t in texts {
        let it = Utf8Iter::new(t.as_bytes());
        assert_eq!(it.position(), 0);
        assert_eq!(it.remaining(), t.as_bytes());
        let got: Vec<char> = it.collect();
        assert_eq!(got, t.chars().collect::<Vec<_>>(), "{t:?}");
    }
}

#[test]
fn utf8_iter_position_and_remaining_track_the_consumed_bytes() {
    let text = "a\u{e9}\u{2014}\u{1f600}z";
    let bytes = text.as_bytes();
    let mut it = Utf8Iter::new(bytes);
    let mut consumed = 0;
    for want in text.chars() {
        assert_eq!(it.next(), Some(want));
        consumed += want.len_utf8();
        assert_eq!(it.position(), consumed, "after {want:?}");
        assert_eq!(it.remaining(), &bytes[consumed..], "after {want:?}");
    }
    assert_eq!(it.next(), None);
    assert_eq!(it.next(), None, "stays exhausted");
    assert_eq!(it.position(), bytes.len());
    assert!(it.remaining().is_empty());
}

// For every 1..=4 byte window the verdict of core::str::from_utf8 decides.
//
// Valid-prefix rule: if the window starts with a valid character (some prefix of
// the window is valid UTF-8), decode returns exactly that first character and its
// byte length. This is the "never mangle legal text" half of RFC 3629 and has no
// room for interpretation.
fn check_valid_prefix(w: &[u8]) {
    let valid_prefix = (1..=w.len()).find(|&m| core::str::from_utf8(&w[..m]).is_ok());
    if let Some(m) = valid_prefix {
        let first = core::str::from_utf8(&w[..m]).unwrap().chars().next().unwrap();
        assert_eq!(decode_utf8_char(w, 0), (first, first.len_utf8()), "valid start in {w:02X?}");
    }
}

// Containment rule for windows that do NOT start with a valid character: decode
// never reads outside the window and consumes at least one byte (progress), so a
// decoder loop terminates and never indexes past the end. When it reports an
// error it is U+FFFD. (Forbidden-but-decodable forms are pinned separately below.)
fn check_invalid_window_contained(w: &[u8]) {
    if (1..=w.len()).any(|m| core::str::from_utf8(&w[..m]).is_ok()) {
        return;
    }
    let (_, n) = decode_utf8_char(w, 0);
    assert!((1..=w.len()).contains(&n), "{w:02X?}: consumed {n}");
}

#[test]
fn every_two_byte_window_with_a_valid_start_decodes_exactly() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            check_valid_prefix(&[a, b]);
            check_invalid_window_contained(&[a, b]);
        }
    }
}

#[test]
fn three_byte_windows_with_a_valid_start_decode_exactly() {
    for a in 0xE0..=0xEFu8 {
        for b in 0..=255u8 {
            for c in 0..=255u8 {
                check_valid_prefix(&[a, b, c]);
                check_invalid_window_contained(&[a, b, c]);
            }
        }
    }
    let edge = [0x00u8, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xFF];
    for a in 0..=255u8 {
        for &b in &edge {
            for &c in &edge {
                check_valid_prefix(&[a, b, c]);
                check_invalid_window_contained(&[a, b, c]);
            }
        }
    }
}

#[test]
fn four_byte_windows_with_a_valid_start_decode_exactly() {
    let edge = [0x00u8, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xFF];
    for a in 0xF0..=0xFFu8 {
        for b in 0..=255u8 {
            for &c in &edge {
                for &d in &edge {
                    check_valid_prefix(&[a, b, c, d]);
                    check_invalid_window_contained(&[a, b, c, d]);
                }
            }
        }
    }
}

// Invalid-window rule: when NO prefix of the window is valid UTF-8 (per
// core::str::from_utf8), the decoded character must be U+FFFD, and it consumes
// 1..=window length (the exact count is not specified by RFC 3629). Covers a
// lead byte followed by a byte that is not 10xxxxxx, and lead bytes that RFC 3629
// removed (F5..FF), however long the claimed sequence.
fn check_invalid_is_replacement(w: &[u8]) {
    if (1..=w.len()).any(|m| core::str::from_utf8(&w[..m]).is_ok()) {
        return;
    }
    let (c, n) = decode_utf8_char(w, 0);
    assert_eq!(c, FFFD, "{w:02X?} is not UTF-8 but decoded to {c:?} ({n} bytes)");
    assert!((1..=w.len()).contains(&n), "{w:02X?}: consumed {n}");
}

// bytes that are not continuation bytes (not 10xxxxxx), plus the continuation
// range edges and a spread of valid ones
const NOT_CONT: &[u8] = &[0x00, 0x01, 0x20, 0x3F, 0x40, 0x41, 0x7E, 0x7F, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xEF, 0xF0, 0xF4, 0xF8, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF];
const CONT: &[u8] = &[0x80, 0x81, 0x8F, 0x90, 0x9F, 0xA0, 0xBF];

#[test]
fn every_two_byte_window_that_is_not_utf8_decodes_to_replacement() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            check_invalid_is_replacement(&[a, b]);
        }
    }
}

#[test]
fn every_three_byte_window_that_is_not_utf8_decodes_to_replacement() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            for c in 0..=255u8 {
                check_invalid_is_replacement(&[a, b, c]);
            }
        }
    }
}

#[test]
fn four_byte_windows_that_are_not_utf8_decode_to_replacement() {
    let all: Vec<u8> = NOT_CONT.iter().chain(CONT).copied().collect();
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            for &c in &all {
                for &d in &all {
                    check_invalid_is_replacement(&[a, b, c, d]);
                }
            }
        }
    }
}

#[test]
fn a_continuation_position_holding_a_non_continuation_byte_is_replacement() {
    // lead of a 2/3/4-byte sequence, every position in turn replaced by a non-10xxxxxx byte
    let leads: [(u8, usize); 6] = [(0xC2, 2), (0xDF, 2), (0xE1, 3), (0xEE, 3), (0xF1, 4), (0xF3, 4)];
    for (lead, len) in leads {
        for pos in 1..len {
            for &bad in NOT_CONT {
                let mut w = vec![lead];
                w.extend(std::iter::repeat_n(0x80u8, len - 1));
                w[pos] = bad;
                check_invalid_is_replacement(&w);
                // and with the damage followed by more valid-looking bytes and by ASCII
                let mut longer = w.clone();
                longer.extend_from_slice(&[0x80, 0x80, b'a']);
                check_invalid_is_replacement(&longer);
            }
        }
    }
    // the cases named in review
    for w in [&[0xE4u8, 0xC1, 0x80][..], &[0xE4, 0x40, 0x80], &[0xE4, 0x80, 0xC1], &[0xE4, 0x80, 0x40], &[0xF0, 0x90, 0x80, 0x40], &[0xF0, 0x90, 0x40, 0x80], &[0xC2, 0x40], &[0xC2, 0xC1]] {
        assert_eq!(decode_utf8_char(w, 0).0, FFFD, "{w:02X?}");
    }
}

#[test]
fn lead_bytes_f8_to_ff_are_replacement_with_any_continuation_pattern() {
    // 5- and 6-byte forms were removed by RFC 3629; F8..FF never occur in UTF-8
    for a in 0xF8..=0xFFu8 {
        check_invalid_is_replacement(&[a]);
        for b in 0..=255u8 {
            check_invalid_is_replacement(&[a, b]);
            for &c in NOT_CONT.iter().chain(CONT) {
                check_invalid_is_replacement(&[a, b, c]);
                for &d in NOT_CONT.iter().chain(CONT) {
                    check_invalid_is_replacement(&[a, b, c, d]);
                    check_invalid_is_replacement(&[a, b, c, d, 0xBF]);
                    check_invalid_is_replacement(&[a, b, c, d, 0x80, 0x80]);
                }
            }
        }
    }
    for w in [
        &[0xF8u8, 0xBF, 0xBF, 0xBF][..],
        &[0xF8, 0xBF, 0xBF, 0xBF, 0xBF],
        &[0xF8, 0x88, 0x80, 0x80, 0x80],
        &[0xFC, 0x80, 0x80, 0x80, 0x80, 0x80],
        &[0xFC, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF],
        &[0xFD, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF],
        &[0xFE, 0x80, 0x80, 0x80],
        &[0xFF, 0x80, 0x80, 0x80],
    ] {
        let (c, n) = decode_utf8_char(w, 0);
        assert_eq!(c, FFFD, "{w:02X?} decoded to {c:?}");
        assert!((1..=w.len()).contains(&n), "{w:02X?}: consumed {n}");
    }
}

// RFC 3629 s3 / s10: these byte forms are NOT UTF-8 and a conforming decoder must
// not turn them into a character. One test per class so a decision about any one
// class (harden the shared decoder, or document the deviation) is isolated.
const OVERLONG: &[&[u8]] = &[
    &[0xC0, 0x80], &[0xC0, 0xAF], &[0xC1, 0xBF], &[0xE0, 0x80, 0x80], &[0xE0, 0x9F, 0xBF], &[0xF0, 0x80, 0x80, 0x80], &[0xF0, 0x8F, 0xBF, 0xBF],
];
const SURROGATE: &[&[u8]] = &[&[0xED, 0xA0, 0x80], &[0xED, 0xAF, 0xBF], &[0xED, 0xB0, 0x80], &[0xED, 0xBF, 0xBF]];
const OUT_OF_RANGE: &[&[u8]] = &[&[0xF4, 0x90, 0x80, 0x80], &[0xF5, 0x80, 0x80, 0x80], &[0xF7, 0xBF, 0xBF, 0xBF], &[0xF8, 0x88, 0x80, 0x80, 0x80]];
const NEVER_IN_UTF8: &[&[u8]] = &[&[0xFE], &[0xFF], &[0x80], &[0xBF]];

fn assert_all_replacement(class: &str, seqs: &[&[u8]]) {
    for seq in seqs {
        let (c, n) = decode_utf8_char(seq, 0);
        assert_eq!(c, FFFD, "{class} {seq:02X?} decoded to {c:?} ({n} bytes)");
        assert!((1..=seq.len()).contains(&n), "{class} {seq:02X?}: consumed {n}");
    }
}

#[test]
fn overlong_encodings_are_rejected_per_rfc_3629() {
    assert_all_replacement("overlong", OVERLONG);
}

#[test]
fn utf16_surrogate_code_points_are_rejected_per_rfc_3629() {
    assert_all_replacement("surrogate", SURROGATE);
}

#[test]
fn code_points_above_10ffff_are_rejected_per_rfc_3629() {
    assert_all_replacement("out-of-range", OUT_OF_RANGE);
}

#[test]
fn bytes_that_never_occur_in_utf8_and_stray_continuations_are_replacement() {
    assert_all_replacement("never-in-utf8", NEVER_IN_UTF8);
    // a lone continuation byte consumes exactly itself (also pinned by the firmware regression suite)
    assert_eq!(decode_utf8_char(&[0x80], 0), (FFFD, 1));
    assert_eq!(decode_utf8_char(&[0xBF], 0), (FFFD, 1));
}

#[test]
fn a_sequence_cut_off_by_the_end_of_the_buffer_is_replacement_and_never_reads_past_it() {
    // every proper prefix of a multi-byte character, at the very end of the buffer
    for c in ['\u{e9}', '\u{2014}', '\u{20ac}', '\u{4e2d}', '\u{1f600}'] {
        let mut buf = [0u8; 4];
        let full = c.encode_utf8(&mut buf).as_bytes().to_vec();
        for cut in 1..full.len() {
            let part = &full[..cut];
            let (ch, n) = decode_utf8_char(part, 0);
            assert_eq!(ch, FFFD, "{c:?} cut to {cut} byte(s)");
            assert!((1..=cut).contains(&n), "{c:?} cut to {cut}: consumed {n}");
        }
    }
    // same with preceding text: the cut sits at the end of a longer buffer
    let mut b = b"abc ".to_vec();
    b.extend_from_slice(&[0xE4, 0xB8]); // first two bytes of U+4E2D
    let (ch, n) = decode_utf8_char(&b, 4);
    assert_eq!(ch, FFFD);
    assert!((1..=2).contains(&n));
}

#[test]
fn utf8_iter_on_damaged_input_terminates_and_keeps_the_ascii_text() {
    // classes that are damage under any reading: stray continuation, never-valid byte,
    // truncation. (Overlong / surrogate / out-of-range forms have their own tests above.)
    let bad: [&[u8]; 4] = [&[0x80], &[0xFF], &[0xE4, 0xB8], &[0xF0, 0x9F, 0x98]];
    for b in bad {
        let mut data = b"alpha ".to_vec();
        data.extend_from_slice(b);
        data.extend_from_slice(b" omega");
        let chars: Vec<char> = Utf8Iter::new(&data).collect();
        assert!(chars.len() <= data.len(), "never yields more chars than bytes");
        assert!(chars.contains(&FFFD), "{b:02X?}: damaged bytes become U+FFFD");
        let ascii: String = chars.iter().filter(|c| c.is_ascii_alphabetic()).collect();
        assert_eq!(ascii, "alphaomega", "{b:02X?}: surrounding text survives");
    }
    // truncated sequence as the last bytes of the buffer
    let mut data = b"tail ".to_vec();
    data.extend_from_slice(&[0xE4, 0xB8]);
    let chars: Vec<char> = Utf8Iter::new(&data).collect();
    assert_eq!(chars.iter().filter(|c| c.is_ascii_alphabetic()).collect::<String>(), "tail");
    assert!(chars.contains(&FFFD));
    assert_eq!(chars.last(), Some(&FFFD));
}

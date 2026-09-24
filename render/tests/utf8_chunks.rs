// chunk-safe UTF-8 decoding around the reader's 8,192-byte PAGE_BUF
//
// every expected offset is derived by arithmetic from how the text is
// built: ASCII padding length plus UTF-8 sequence lengths per RFC 3629
// ('é' U+00E9 = 2 bytes C3 A9, '中' U+4E2D = 3 bytes E4 B8 AD,
// '𠀀' U+20000 = 4 bytes F0 A0 80 80)

use pulp_render::utf8::{Decoded, complete_prefix_len, decode_step, decode_utf8_char};

const CHUNK: usize = 8192;

// simulate the reader: read text[offset..offset+CHUNK], decode what the
// chunk can hold, advance offset by the bytes consumed, repeat to EOF
// returns (decoded chars, every chunk start offset plus the final offset)
//
// caller rule: before EOF, decode only the complete prefix so a split
// scalar is re-read whole by the next chunk; at EOF (offset + len ==
// total, as the reader knows file_size) decode everything, so a truncated
// tail decodes as one U+FFFD instead of being held back forever
fn read_in_chunks(text: &[u8]) -> (Vec<char>, Vec<usize>) {
    let total = text.len();
    let mut chars = Vec::new();
    let mut offsets = vec![0];
    let mut offset = 0;
    while offset < total {
        let chunk = &text[offset..(offset + CHUNK).min(total)];
        let at_eof = offset + chunk.len() == total;
        let end = if at_eof {
            chunk.len()
        } else {
            complete_prefix_len(chunk)
        };
        let buf = &chunk[..end];
        let mut pos = 0;
        while pos < buf.len() {
            let (ch, len) = decode_utf8_char(buf, pos);
            chars.push(ch);
            pos += len;
        }
        assert!(pos > 0, "no progress at offset {offset}");
        offset += pos;
        offsets.push(offset);
    }
    (chars, offsets)
}

// `pad` ASCII 'a' bytes, then `scalar`, then "tail"
fn padded(pad: usize, scalar: char) -> String {
    let mut s = "a".repeat(pad);
    s.push(scalar);
    s.push_str("tail");
    s
}

// a scalar of n bytes with k of them (1..n) in the first chunk starts at
// byte CHUNK - k, so pad = CHUNK - k; the first chunk must stop before it
// (consumed = CHUNK - k) and the second chunk starts at the lead byte and
// reaches EOF at pad + n + 4 ("tail")
fn assert_split_at_every_point(scalar: char, n: usize) {
    assert_eq!(scalar.len_utf8(), n, "RFC 3629 length of {scalar:?}");
    for k in 1..n {
        let pad = CHUNK - k;
        let text = padded(pad, scalar);
        let (chars, offsets) = read_in_chunks(text.as_bytes());
        let expected: Vec<char> = text.chars().collect();
        assert_eq!(chars, expected, "{scalar:?} with {k} byte(s) in chunk 1");
        assert_eq!(
            offsets,
            vec![0, CHUNK - k, CHUNK - k + n + 4],
            "{scalar:?} with {k} byte(s) in chunk 1"
        );
    }
}

#[test]
fn two_byte_scalar_split_at_the_chunk_boundary_decodes_once() {
    assert_split_at_every_point('é', 2);
}

#[test]
fn three_byte_scalar_split_at_the_chunk_boundary_decodes_once() {
    assert_split_at_every_point('中', 3);
}

#[test]
fn four_byte_scalar_split_at_the_chunk_boundary_decodes_once() {
    assert_split_at_every_point('𠀀', 4);
}

#[test]
fn zhong_starting_at_byte_8191_moves_the_next_chunk_to_8191() {
    // 8191 'a' bytes put '中' at 8191..8194; 1 of its 3 bytes fits
    let text = padded(8191, '中');
    let (chars, offsets) = read_in_chunks(text.as_bytes());
    assert_eq!(chars.len(), 8191 + 1 + 4);
    assert_eq!(chars[8191], '中');
    assert_eq!(offsets, vec![0, 8191, 8191 + 3 + 4]);
}

#[test]
fn scalar_ending_exactly_at_the_boundary_fills_the_chunk() {
    // 8188 'a' + 4-byte '𠀀' ends at byte 8192: nothing to hold back
    let text = padded(8188, '𠀀');
    let (chars, offsets) = read_in_chunks(text.as_bytes());
    assert_eq!(chars, text.chars().collect::<Vec<_>>());
    assert_eq!(offsets, vec![0, 8192, 8192 + 4]);
}

#[test]
fn mixed_cjk_and_latin_text_survives_three_chunks() {
    // pattern "中a𠀀é" = 3 + 1 + 4 + 2 = 10 bytes; 2000 repeats = 20000
    // 8192 = 819 * 10 + 2, so pattern 819 starts at 8190 and its '中'
    // straddles with 2 bytes in: consumed 8190; chunk 2 spans 8190..16382,
    // again 2 bytes into a pattern: next start 16380; chunk 3 is 3620
    // bytes and reaches EOF at 20000
    let text = "中a𠀀é".repeat(2000);
    assert_eq!(text.len(), 20000);
    let (chars, offsets) = read_in_chunks(text.as_bytes());
    assert_eq!(chars, text.chars().collect::<Vec<_>>());
    assert_eq!(offsets, vec![0, 8190, 16380, 20000]);
}

#[test]
fn truncated_sequence_at_eof_yields_one_replacement() {
    // 10 'a' + E4 B8 (first 2 of 3 bytes of '中'), 12 bytes, one chunk
    let mut text = b"a".repeat(10);
    text.extend_from_slice(&[0xE4, 0xB8]);
    let (chars, offsets) = read_in_chunks(&text);
    let mut expected = vec!['a'; 10];
    expected.push('\u{FFFD}');
    assert_eq!(chars, expected);
    assert_eq!(offsets, vec![0, 12]);
}

#[test]
fn truncated_sequence_at_eof_after_a_boundary_yields_one_replacement() {
    // 8191 'a' + F0 A0 80 (3 of 4 bytes of '𠀀'), 8194 bytes: chunk 1
    // holds back the lead byte (consumed 8191); chunk 2 is the 3 bytes
    // at EOF and decodes as one U+FFFD
    let mut text = b"a".repeat(8191);
    text.extend_from_slice(&[0xF0, 0xA0, 0x80]);
    let (chars, offsets) = read_in_chunks(&text);
    let mut expected = vec!['a'; 8191];
    expected.push('\u{FFFD}');
    assert_eq!(chars, expected);
    assert_eq!(offsets, vec![0, 8191, 8194]);
}

#[test]
fn invalid_bytes_at_the_boundary_are_not_held_back() {
    // 8191 'a' + stray continuation 0x80 at byte 8191 + 'b': 0x80 can
    // never complete, so chunk 1 consumes all 8192 bytes
    let mut text = b"a".repeat(8191);
    text.push(0x80);
    text.push(b'b');
    let (chars, offsets) = read_in_chunks(&text);
    let mut expected = vec!['a'; 8191];
    expected.extend(['\u{FFFD}', 'b']);
    assert_eq!(chars, expected);
    assert_eq!(offsets, vec![0, 8192, 8193]);
}

// '𠀀' U+20000 = F0 A0 80 80 (RFC 3629: 4-byte form for U+10000..U+10FFFF)
const EXT_B: [u8; 4] = [0xF0, 0xA0, 0x80, 0x80];

#[test]
fn decode_step_reports_incomplete_for_every_proper_prefix_of_a_4_byte_scalar() {
    for k in 1..4 {
        assert_eq!(
            decode_step(&EXT_B[..k], 0),
            Decoded::Incomplete,
            "{k} bytes"
        );
    }
    assert_eq!(decode_step(&EXT_B, 0), Decoded::Scalar('𠀀', 4));
}

#[test]
fn decode_step_reports_invalid_with_the_bytes_to_skip() {
    // stray continuation and invalid lead byte skip 1 (RFC 3629 section 3)
    assert_eq!(decode_step(&[0x80], 0), Decoded::Invalid(1));
    assert_eq!(decode_step(&[0xFF], 0), Decoded::Invalid(1));
    // E4 lead followed by ASCII 'x': the lead alone is invalid, skip 1
    assert_eq!(decode_step(&[0xE4, b'x'], 0), Decoded::Invalid(1));
}

#[test]
fn complete_prefix_len_trims_only_an_incomplete_trailing_sequence() {
    // "ab" + first k bytes of '𠀀': prefix stops at the lead byte (2)
    for k in 1..4 {
        let mut buf = b"ab".to_vec();
        buf.extend_from_slice(&EXT_B[..k]);
        assert_eq!(complete_prefix_len(&buf), 2, "{k} bytes of U+20000");
    }
    // whole scalar: nothing trimmed (2 + 4)
    let mut buf = b"ab".to_vec();
    buf.extend_from_slice(&EXT_B);
    assert_eq!(complete_prefix_len(&buf), 6);
    // bytes that can never complete are not trimmed
    assert_eq!(complete_prefix_len(b"ab\x80"), 3);
    assert_eq!(complete_prefix_len(b"ab\xFF"), 3);
    assert_eq!(complete_prefix_len(b"ab\xE4x"), 4);
    // 3 trailing continuations after ASCII cannot be one sequence's tail
    assert_eq!(complete_prefix_len(b"ab\x80\x80\x80"), 5);
    assert_eq!(complete_prefix_len(b""), 0);
}

#[test]
fn overlong_encodings_are_invalid_instead_of_aliasing_ascii() {
    // U+002F ('/') has the one-byte encoding 2F. None of these longer
    // encodings may produce a scalar or an ASCII path separator.
    for bytes in [
        &[0xC0, 0xAF][..],
        &[0xE0, 0x80, 0xAF][..],
        &[0xF0, 0x80, 0x80, 0xAF][..],
    ] {
        assert!(
            matches!(decode_step(bytes, 0), Decoded::Invalid(_)),
            "accepted overlong encoding {bytes:02X?}"
        );
        assert_eq!(decode_utf8_char(bytes, 0).0, '\u{FFFD}');
    }
    assert_eq!(decode_step(&[0xC2, 0xAF], 0), Decoded::Scalar('¯', 2));
    assert_eq!(decode_step(&[0xE0, 0xA0, 0x80], 0), Decoded::Scalar('ࠀ', 3));
    assert_eq!(
        decode_step(&[0xF0, 0x90, 0x80, 0x80], 0),
        Decoded::Scalar('𐀀', 4)
    );
}

#[test]
fn impossible_overlong_prefix_at_chunk_end_is_not_held_back() {
    // C0 cannot start a legal UTF-8 sequence, even without its next byte.
    assert_eq!(decode_step(&[0xC0], 0), Decoded::Invalid(1));
    assert_eq!(complete_prefix_len(b"ab\xC0"), 3);
}

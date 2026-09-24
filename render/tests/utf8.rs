use pulp_render::utf8::{Utf8Iter, decode_utf8_char};

// "中" is U+4E2D: a 3-byte sequence E4 B8 AD (RFC 3629); 'a' is 1 byte
const ZHONG_A: &[u8] = "中a".as_bytes();

#[test]
fn decode_utf8_char_reports_char_and_byte_length() {
    assert_eq!(decode_utf8_char(ZHONG_A, 0), ('中', 3));
    assert_eq!(decode_utf8_char(ZHONG_A, 3), ('a', 1));
}

#[test]
fn utf8_iter_yields_chars_in_order() {
    let chars: Vec<char> = Utf8Iter::new(ZHONG_A).collect();
    assert_eq!(chars, ['中', 'a']);
}

#[test]
fn stray_continuation_byte_decodes_to_replacement_char() {
    // 0x80 is a continuation byte with no lead byte (RFC 3629 section 3)
    assert_eq!(decode_utf8_char(&[0x80], 0), ('\u{FFFD}', 1));
}

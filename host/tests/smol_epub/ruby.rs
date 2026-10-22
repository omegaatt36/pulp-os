// Ruby annotations (`<rt>` readings, `<rp>` fallback parentheses) are not body text: only the
// base text is stripped, however the input is cut into chunks and on every stripping route.
use crate::smol_common;

use smol_common::*;
use smol_epub::html_strip::strip_html_inplace;

const RUBY: &str =
    "<p><ruby>漢<rp>(</rp><rt>かん</rt><rp>)</rp>字<rp>(</rp><rt>じ</rt><rp>)</rp></ruby>です</p>";

fn streamed(html: &str, chunk: usize) -> String {
    String::from_utf8(plain(&route_feed(html.as_bytes(), chunk, 64))).unwrap()
}

#[test]
fn the_stream_drops_ruby_readings_and_fallback_parentheses() {
    assert_eq!(streamed(RUBY, 4096), "漢字です");
}

#[test]
fn the_stream_drops_ruby_annotations_at_every_chunk_size() {
    for chunk in 1..=24 {
        assert_eq!(streamed(RUBY, chunk), "漢字です", "chunk {chunk}");
    }
}

#[test]
fn markup_inside_a_reading_is_dropped_with_it() {
    let html = "<p><ruby>漢<rt><span>かん</span><b>x</b></rt></ruby>字</p>";
    assert_eq!(streamed(html, 4096), "漢字");
    assert_eq!(streamed(html, 1), "漢字");
}

#[test]
fn ruby_tag_names_match_in_any_case() {
    let html = "<p><RUBY>漢<RT>かん</RT></RUBY>字</p>";
    assert_eq!(streamed(html, 4096), "漢字");
}

#[test]
fn text_after_a_reading_continues_the_line() {
    let html = "<p>前<ruby>漢<rt>かん</rt></ruby>後</p>";
    assert_eq!(streamed(html, 4096), "前漢後");
}

#[test]
fn the_in_place_stripper_drops_ruby_annotations() {
    let mut buf = RUBY.as_bytes().to_vec();
    strip_html_inplace(&mut buf);
    assert_eq!(String::from_utf8(plain(&buf)).unwrap(), "漢字です");
}

#[test]
fn the_buffer_route_drops_ruby_annotations() {
    let out = smol_epub::cache::strip_html_buf(RUBY.as_bytes()).unwrap();
    assert_eq!(String::from_utf8(plain(&out)).unwrap(), "漢字です");
}

// Ruby annotations: `<ruby>` base `<rt>` reading `</rt>` becomes a marker-delimited group
// (`{R}` base `{T}` reading `{r}`), however the input is cut into chunks. `<rp>` fallback
// parentheses are not text. The in-place stripper, which keeps no markers, drops the readings.
use crate::smol_common;

use smol_common::*;
use smol_epub::html_strip::{
    MARKER, RUBY_BASE_MAX, RUBY_CLOSE, RUBY_OPEN, RUBY_READING, RUBY_READING_MAX,
    strip_html_inplace,
};

const RUBY: &str =
    "<p><ruby>漢<rp>(</rp><rt>かん</rt><rp>)</rp>字<rp>(</rp><rt>じ</rt><rp>)</rp></ruby>です</p>";
const RUBY_GROUPS: &str = "{R}漢{T}かん{r}{R}字{T}じ{r}です";

// the stripped stream with its markers spelled out, edges trimmed
fn shown(out: &[u8]) -> String {
    let mut text = Vec::new();
    let mut i = 0;
    while i < out.len() {
        if out[i] == MARKER {
            text.push(b'{');
            text.push(out[i + 1]);
            text.push(b'}');
            i += 2;
        } else {
            text.push(out[i]);
            i += 1;
        }
    }
    String::from_utf8(text).unwrap().trim().to_string()
}

fn streamed(html: &str, chunk: usize) -> String {
    shown(&route_feed(html.as_bytes(), chunk, 64))
}

#[test]
fn the_marker_letters_are_the_ones_the_expectations_spell() {
    assert_eq!(RUBY_OPEN, b'R');
    assert_eq!(RUBY_READING, b'T');
    assert_eq!(RUBY_CLOSE, b'r');
}

#[test]
fn every_base_with_a_reading_becomes_a_group_and_fallback_parentheses_go() {
    assert_eq!(streamed(RUBY, 4096), RUBY_GROUPS);
}

#[test]
fn the_groups_are_the_same_at_every_chunk_size() {
    for chunk in 1..=24 {
        assert_eq!(streamed(RUBY, chunk), RUBY_GROUPS, "chunk {chunk}");
    }
}

#[test]
fn the_groups_are_the_same_at_every_output_size() {
    for cap in [3, 4, 5, 8, 13] {
        let out = route_feed(RUBY.as_bytes(), 4096, cap);
        assert_eq!(shown(&out), RUBY_GROUPS, "output {cap}");
    }
}

#[test]
fn a_reading_is_text_only() {
    let html = "<p><ruby>漢<rt><span>かん</span><b>x</b><br/>y</rt></ruby>字</p>";
    assert_eq!(streamed(html, 4096), "{R}漢{T}かんxy{r}字");
    assert_eq!(streamed(html, 1), "{R}漢{T}かんxy{r}字");
}

#[test]
fn a_reading_keeps_its_entities() {
    let html = "<p><ruby>漢<rt>a&amp;b&#x3042;</rt></ruby></p>";
    assert_eq!(streamed(html, 4096), "{R}漢{T}a&bあ{r}");
}

#[test]
fn ruby_tag_names_match_in_any_case() {
    let html = "<p><RUBY>漢<RT>かん</RT></RUBY>字</p>";
    assert_eq!(streamed(html, 4096), "{R}漢{T}かん{r}字");
}

#[test]
fn text_around_a_group_continues_the_line() {
    let html = "<p>前<ruby>漢<rt>かん</rt></ruby>後</p>";
    assert_eq!(streamed(html, 4096), "前{R}漢{T}かん{r}後");
}

#[test]
fn style_markers_of_the_base_precede_its_group() {
    let html = "<p><b><ruby>漢<rt>かん</rt></ruby></b></p>";
    assert_eq!(streamed(html, 4096), "{B}{R}漢{T}かん{r}{b}");
}

#[test]
fn a_base_the_markup_gave_no_reading_is_a_group_without_one() {
    assert_eq!(streamed("<p><ruby>漢字</ruby></p>", 4096), "{R}漢字{r}");
}

#[test]
fn a_reading_that_follows_no_base_is_dropped() {
    assert_eq!(
        streamed("<p><ruby><rt>かん</rt>漢</ruby></p>", 4096),
        "{R}漢{r}"
    );
    assert_eq!(streamed("<p>漢<rt>かん</rt>字</p>", 4096), "漢字");
}

#[test]
fn a_second_annotation_row_is_dropped() {
    let html = "<p><ruby>漢<rt>かん</rt><rtc><rt>han</rt></rtc></ruby></p>";
    assert_eq!(streamed(html, 4096), "{R}漢{T}かん{r}");
}

#[test]
fn an_element_the_input_leaves_open_is_closed_at_the_end() {
    assert_eq!(streamed("<p><ruby>漢<rt>かん", 4096), "{R}漢{T}かん{r}");
    assert_eq!(streamed("<p><ruby>漢", 4096), "{R}漢{r}");
}

#[test]
fn a_group_never_holds_a_paragraph_break() {
    let html = "<ruby>漢</p><p>字<rt>じ</rt></ruby>";
    assert_eq!(streamed(html, 4096), "{R}漢{r}\n\n{R}字{T}じ{r}");
}

#[test]
fn a_long_base_is_split_into_groups_of_at_most_the_cap() {
    let base = "漢".repeat(RUBY_BASE_MAX / 3 + 4);
    let html = format!("<p><ruby>{base}<rt>かん</rt></ruby></p>");
    let first = "漢".repeat(RUBY_BASE_MAX / 3);
    let want = format!("{{R}}{first}{{r}}{{R}}漢漢漢漢{{T}}かん{{r}}");
    assert_eq!(streamed(&html, 4096), want);
    assert_eq!(streamed(&html, 1), want);
}

#[test]
fn a_long_reading_is_cut_to_the_cap() {
    let reading = "あ".repeat(RUBY_READING_MAX / 3 + 4);
    let html = format!("<p><ruby>漢<rt>{reading}</rt></ruby></p>");
    let kept = "あ".repeat(RUBY_READING_MAX / 3);
    let want = format!("{{R}}漢{{T}}{kept}{{r}}");
    assert_eq!(streamed(&html, 4096), want);
    assert_eq!(streamed(&html, 1), want);
}

#[test]
fn the_in_place_stripper_drops_ruby_annotations() {
    let mut buf = RUBY.as_bytes().to_vec();
    strip_html_inplace(&mut buf);
    assert_eq!(String::from_utf8(plain(&buf)).unwrap(), "漢字です");
}

#[test]
fn the_buffer_route_keeps_the_groups() {
    let out = smol_epub::cache::strip_html_buf(RUBY.as_bytes()).unwrap();
    assert_eq!(shown(&out), RUBY_GROUPS);
}

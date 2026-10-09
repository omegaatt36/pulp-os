// Numeric character references in chapter text: a reference to a scalar becomes that
// scalar; a reference to no scalar becomes one U+FFFD; the text around it is untouched.
//
// Run: cargo test-host --test smol_entity
//
// Oracle: the HTML5 numeric character reference rules (surrogates D800..=DFFF, values above
// 10FFFF and zero are U+FFFD; one reference gives one U+FFFD, a surrogate pair written as
// two references gives two) and `char::from_u32` for the valid ones: the expected text is
// computed from the number written in the reference. Overflowing numbers (they do not fit a
// u32, and not even the stripper's small reference buffer) are references to no scalar.
// What HTML5 leaves to the parser's error recovery (`&#xZZ;`, `&#;`, `&#x;`, a missing
// semicolon) is only held to: valid UTF-8, and the text around it intact.

use crate::smol_common;

use smol_common::*;

// the text `a`, the reference, `b`: no white space anywhere, nothing to collapse
fn doc_of(reference: &str) -> Vec<u8> {
    p_doc(format!("a{reference}b").as_bytes())
}

fn want(replacement: &str) -> String {
    format!("a{replacement}b")
}

// reference -> the text it stands for
fn valid_cases() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    for cp in [
        0x41u32, 0xE9, 0x81FA, 0x20BB7, 0x1F600, 0x10FFFF, 0xD7FF, 0xE000, 0xFFFD,
    ] {
        let ch = char::from_u32(cp).unwrap().to_string();
        v.push((format!("&#x{cp:X};"), ch.clone()));
        v.push((format!("&#x{cp:x};"), ch.clone()));
        v.push((format!("&#{cp};"), ch));
    }
    // leading zeros are still the number 0x81FA
    v.push(("&#x0081FA;".to_string(), "\u{81fa}".to_string()));
    v.push(("&#033274;".to_string(), "\u{81fa}".to_string()));
    v
}

// references to no scalar
fn invalid_cases() -> Vec<String> {
    [
        "&#xD800;",
        "&#xDFFF;",
        "&#xDBFF;",
        "&#xDC00;",
        "&#xd800;",
        "&#55296;",
        "&#57343;",
        "&#x110000;",
        "&#1114112;",
        "&#0;",
        "&#x0;",
        "&#00000;",
        "&#xFFFFFFFF;",
        "&#4294967295;",
        // 2^32: one more than a u32 holds
        "&#4294967296;",
        "&#x100000000;",
        "&#99999999999999999999;",
        "&#xFFFFFFFFFFFFFFFFFF;",
        "&#x0000000000000000000000000000000000000000FFFFFFFFFFFFFFFFFF;",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

// the routes a whole chapter document goes through; each yields the stripped text
fn routes(xhtml: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    for deflate in [false, true] {
        let bytes = chapter_zip(xhtml, deflate);
        let zip = open_zip(&bytes).unwrap();
        v.push((
            format!("stream, deflate {deflate}"),
            route_stream(&bytes, &zip, "c.xhtml"),
        ));
        if let Some(b) = route_buf(&bytes, &zip, "c.xhtml") {
            v.push((format!("buf, deflate {deflate}"), b));
        }
        v.push((
            format!("async, deflate {deflate}"),
            route_async(&bytes, &zip, "c.xhtml"),
        ));
    }
    v.push((
        "feed whole".to_string(),
        route_feed(xhtml, xhtml.len(), 4096),
    ));
    v.push(("feed 1 byte".to_string(), route_feed(xhtml, 1, 4096)));
    v.push(("feed 3 bytes".to_string(), route_feed(xhtml, 3, 64)));
    v
}

#[test]
fn a_numeric_reference_to_a_valid_scalar_becomes_that_scalar() {
    for (reference, ch) in valid_cases() {
        for (route, out) in routes(&doc_of(&reference)) {
            check_text(&out, &want(&ch), &format!("{reference} via {route}"));
        }
    }
    finish();
}

#[test]
fn a_numeric_reference_to_no_scalar_becomes_one_replacement_character() {
    for reference in invalid_cases() {
        for (route, out) in routes(&doc_of(&reference)) {
            check_text(&out, &want("\u{FFFD}"), &format!("{reference} via {route}"));
        }
    }
    finish();
}

#[test]
fn the_two_halves_of_a_surrogate_pair_written_as_references_are_two_replacements() {
    // U+1F600 as its UTF-16 pair D83D DE00: not a scalar when split into two references
    for reference in ["&#xD83D;&#xDE00;", "&#55357;&#56832;", "&#xD800;&#xDFFF;"] {
        for (route, out) in routes(&doc_of(reference)) {
            check_text(
                &out,
                &want("\u{FFFD}\u{FFFD}"),
                &format!("{reference} via {route}"),
            );
        }
    }
    finish();
}

#[test]
fn references_in_a_row_each_keep_their_own_outcome() {
    let doc = doc_of("&#x81FA;&#xD800;&#x20BB7;&#x110000;&#0;&#x7063;");
    for (route, out) in routes(&doc) {
        check_text(
            &out,
            &want("\u{81fa}\u{FFFD}\u{20bb7}\u{FFFD}\u{FFFD}\u{7063}"),
            &route,
        );
    }
    finish();
}

#[test]
fn the_four_xml_references_still_give_their_characters() {
    for (reference, ch) in [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
    ] {
        for (route, out) in routes(&doc_of(reference)) {
            check_text(&out, &want(ch), &format!("{reference} via {route}"));
        }
    }
    finish();
}

// error recovery of unparsable references is the parser's choice: only the invariants
#[test]
fn an_unparsable_reference_leaves_valid_utf8_and_the_text_around_it() {
    for reference in [
        "&#xZZ;", "&#;", "&#x;", "&#xG1;", "&#12a;", "&#x81FA ", "&#33274 ",
    ] {
        for (route, out) in routes(&doc_of(reference)) {
            let t = plain(&out);
            match std::str::from_utf8(&t) {
                Err(e) => soft_fail(format!(
                    "{reference} via {route}: invalid UTF-8 ({e}): {}",
                    hex(&t)
                )),
                Ok(s) if !(s.starts_with('a') && s.ends_with('b')) => soft_fail(format!(
                    "{reference} via {route}: the text around it changed: {s:?}"
                )),
                Ok(_) => {}
            }
        }
    }
    finish();
}

// the same references with the buffer boundaries moving over them
#[test]
fn references_survive_every_buffer_alignment() {
    let mut cases: Vec<(String, String)> = valid_cases()
        .into_iter()
        .filter(|(r, _)| matches!(r.as_str(), "&#x81FA;" | "&#x20BB7;" | "&#33274;"))
        .collect();
    for r in ["&#xD800;", "&#x110000;", "&#0;", "&#99999999999999999999;"] {
        cases.push((r.to_string(), "\u{FFFD}".to_string()));
    }
    for (reference, shown) in cases {
        for pad in boundary_pads(DOC_HEAD.len(), reference.len()) {
            let f = filler(pad, 11 + pad as u64);
            let xhtml = p_doc(format!("{f}{reference}z").as_bytes());
            let expected = format!("{f}{shown}z");
            for deflate in [false, true] {
                let bytes = chapter_zip(&xhtml, deflate);
                let zip = open_zip(&bytes).unwrap();
                check_text(
                    &route_stream(&bytes, &zip, "c.xhtml"),
                    &expected,
                    &format!("{reference}, pad {pad}, deflate {deflate}"),
                );
            }
        }
    }
    finish();
}

// the in-place stripper (container / OPF / TOC documents): tags out, no markers
#[test]
fn the_in_place_stripper_decodes_references_the_same_way() {
    let mut cases: Vec<(String, String)> = valid_cases();
    for r in invalid_cases() {
        cases.push((r, "\u{FFFD}".to_string()));
    }
    for (reference, shown) in cases {
        let mut buf = format!("<p>a{reference}b</p>").into_bytes();
        smol_epub::html_strip::strip_html_inplace(&mut buf);
        check_text(
            &buf,
            &want(&shown),
            &format!("{reference} via strip_html_inplace"),
        );
    }
    finish();
}

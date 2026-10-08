// Malformed UTF-8 in chapter text, OPF title / creator and TOC labels: smol-epub's output is
// always valid UTF-8, every malformed stretch is U+FFFD in the number String::from_utf8_lossy
// gives (maximal subpart, RFC 3629 section 3 / WHATWG), and the valid text around it is
// unchanged, at every buffer alignment and every split of the input.
//
// Run: cargo test-host --test smol_malformed
//
// Oracle: `String::from_utf8_lossy` of the whole input text (ASCII `a` / `b` around the
// malformed bytes end any sequence, so lossy of the whole equals "a" + lossy(bytes) + "b").
// Titles are kept inside their capacity for the exact comparisons; the truncated ones are
// held to: valid UTF-8, within capacity, a whole-scalar prefix of the lossy text.

mod smol_common;

use smol_common::*;
use smol_epub::epub::{AUTHOR_CAP, TITLE_CAP, TOC_TITLE_CAP};

fn samples() -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |name: &str, b: &[u8]| v.push((name.to_string(), b.to_vec()));
    add("stray continuation 80", &[0x80]);
    add("stray continuation BF", &[0xBF]);
    add("two stray continuations", &[0x80, 0x80]);
    add("three stray continuations", &[0xBF, 0x80, 0xBF]);
    add("overlong C0 80", &[0xC0, 0x80]);
    add("overlong C1 BF", &[0xC1, 0xBF]);
    add("overlong E0 80 80", &[0xE0, 0x80, 0x80]);
    add("overlong E0 9F BF", &[0xE0, 0x9F, 0xBF]);
    add("overlong F0 80 80 80", &[0xF0, 0x80, 0x80, 0x80]);
    add("overlong F0 8F BF BF", &[0xF0, 0x8F, 0xBF, 0xBF]);
    add("surrogate ED A0 80", &[0xED, 0xA0, 0x80]);
    add("surrogate ED BF BF", &[0xED, 0xBF, 0xBF]);
    add("surrogate pair", &[0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80]);
    add("above 10FFFF F4 90 80 80", &[0xF4, 0x90, 0x80, 0x80]);
    add("above 10FFFF F5 80 80 80", &[0xF5, 0x80, 0x80, 0x80]);
    add("above 10FFFF F7 BF BF BF", &[0xF7, 0xBF, 0xBF, 0xBF]);
    add("truncated E8 87", &[0xE8, 0x87]);
    add("truncated E8", &[0xE8]);
    add("truncated F0 A0 AE", &[0xF0, 0xA0, 0xAE]);
    add("truncated F0 A0", &[0xF0, 0xA0]);
    add("truncated F0", &[0xF0]);
    add("truncated C3", &[0xC3]);
    add("truncated ED A0", &[0xED, 0xA0]);
    add("lead then lead then valid", &[0xE8, 0xE8, 0x87, 0xBA]);
    add(
        "truncated 4-byte then valid 3-byte",
        &[0xF0, 0xA0, 0xAE, 0xE8, 0x87, 0xBA],
    );
    add("stray then valid", &[0x80, 0xE8, 0x87, 0xBA]);
    add("valid then stray", &[0xE8, 0x87, 0xBA, 0x80]);
    add("valid then truncated", &[0xE8, 0x87, 0xBA, 0xE8, 0x87]);
    add("overlong then valid", &[0xC0, 0x80, 0xC3, 0xA9]);
    add("F8 88 80 80 80", &[0xF8, 0x88, 0x80, 0x80, 0x80]);
    for b in 0xF8..=0xFFu8 {
        add(&format!("byte {b:02X}"), &[b]);
    }
    add("valid text only", "\u{81fa}\u{7063}\u{20bb7}".as_bytes());
    v
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn with_around(seg: &[u8]) -> Vec<u8> {
    [b"a".as_slice(), seg, b"b"].concat()
}

#[test]
fn malformed_bytes_in_chapter_text_become_replacement_characters() {
    for (name, seg) in samples() {
        let input = with_around(&seg);
        let expected = lossy(&input);
        let xhtml = p_doc(&input);
        for deflate in [false, true] {
            let bytes = chapter_zip(&xhtml, deflate);
            let zip = open_zip(&bytes).unwrap();
            check_text(
                &route_stream(&bytes, &zip, "c.xhtml"),
                &expected,
                &format!("{name}: stream, deflate {deflate}"),
            );
            if let Some(b) = route_buf(&bytes, &zip, "c.xhtml") {
                check_text(&b, &expected, &format!("{name}: buf, deflate {deflate}"));
            }
            check_text(
                &route_async(&bytes, &zip, "c.xhtml"),
                &expected,
                &format!("{name}: async, deflate {deflate}"),
            );
        }
    }
    finish();
}

// the stripper fed in pieces: a sequence cut between two `feed` calls is still one stretch
#[test]
fn malformed_bytes_split_across_feed_calls_give_the_same_replacements() {
    for (name, seg) in samples() {
        let input = with_around(&seg);
        let expected = lossy(&input);
        let xhtml = p_doc(&input);
        for chunk in [1, 2, 3, 4, 5, 7, 100] {
            for out_cap in [4, 16, 4096] {
                check_text(
                    &route_feed(&xhtml, chunk, out_cap),
                    &expected,
                    &format!("{name}: feed chunk {chunk}, out {out_cap}"),
                );
            }
        }
    }
    finish();
}

// the document ends inside the malformed stretch: the end of the input closes it
#[test]
fn malformed_bytes_at_the_very_end_of_the_chapter_become_replacement_characters() {
    for (name, seg) in samples() {
        let mut xhtml = format!("{DOC_HEAD}a").into_bytes();
        xhtml.extend_from_slice(&seg);
        let expected = lossy(&[b"a".as_slice(), &seg].concat());
        for deflate in [false, true] {
            let bytes = chapter_zip(&xhtml, deflate);
            let zip = open_zip(&bytes).unwrap();
            check_text(
                &route_stream(&bytes, &zip, "c.xhtml"),
                &expected,
                &format!("{name} at EOF: stream, deflate {deflate}"),
            );
            if let Some(b) = route_buf(&bytes, &zip, "c.xhtml") {
                check_text(
                    &b,
                    &expected,
                    &format!("{name} at EOF: buf, deflate {deflate}"),
                );
            }
        }
        check_text(
            &route_feed(&xhtml, xhtml.len(), 4096),
            &expected,
            &format!("{name} at EOF: feed"),
        );
        check_text(
            &route_feed(&xhtml, 1, 4096),
            &expected,
            &format!("{name} at EOF: feed 1 byte"),
        );
    }
    finish();
}

#[test]
fn malformed_bytes_get_the_same_replacements_at_every_buffer_alignment() {
    for (name, seg) in samples() {
        let span = seg.len() + 2;
        let pads = boundary_pads(DOC_HEAD.len(), span);
        for pad in pads {
            let f = filler(pad, 5 + pad as u64);
            let mut inner = f.clone().into_bytes();
            inner.extend_from_slice(&with_around(&seg));
            let expected = lossy(&inner);
            let xhtml = p_doc(&inner);
            for deflate in [false, true] {
                let bytes = chapter_zip(&xhtml, deflate);
                let zip = open_zip(&bytes).unwrap();
                check_text(
                    &route_stream(&bytes, &zip, "c.xhtml"),
                    &expected,
                    &format!("{name}: pad {pad}, deflate {deflate}"),
                );
            }
        }
    }
    finish();
}

// ---------------------------------------------------------------------------
// OPF title / creator and TOC labels
// ---------------------------------------------------------------------------

fn book_with(ver: Ver, title: &[u8], creator: &[u8], toc: &[&[u8]]) -> Parsed {
    let mut b = Book::new(ver, vec![p_doc(b"x")]);
    b.title = title.to_vec();
    b.creator = creator.to_vec();
    b.toc_titles = toc.iter().map(|t| t.to_vec()).collect();
    parse(&build_book(&b))
}

#[test]
fn malformed_bytes_in_the_opf_title_and_creator_become_replacement_characters() {
    for ver in [Ver::V2, Ver::V3] {
        for (name, seg) in samples() {
            let raw = with_around(&seg);
            let expected = lossy(&raw);
            let p = book_with(ver, &raw, &raw, &[b"x"]);
            check_stored(
                &meta_title_bytes(&p),
                &expected,
                &format!("{ver:?} {name}: dc:title"),
            );
            check_stored(
                &meta_author_bytes(&p),
                &expected,
                &format!("{ver:?} {name}: dc:creator"),
            );
        }
    }
    finish();
}

#[test]
fn malformed_bytes_in_a_toc_label_become_replacement_characters() {
    for ver in [Ver::V2, Ver::V3] {
        for (name, seg) in samples() {
            let raw = with_around(&seg);
            let expected = lossy(&raw);
            let p = book_with(ver, b"T", b"A", &[&raw, "\u{7e41}".as_bytes()]);
            assert_eq!(p.toc.len(), 2, "{ver:?} {name}: both entries are kept");
            check_stored(
                &toc_title_bytes(&p, 0),
                &expected,
                &format!("{ver:?} {name}: toc label"),
            );
            // the entry after it is untouched
            check_stored(
                &toc_title_bytes(&p, 1),
                "\u{7e41}",
                &format!("{ver:?} {name}: next label"),
            );
        }
    }
    finish();
}

// malformed bytes at the capacity edge: whatever is stored is valid, fits and is a prefix
#[test]
fn malformed_bytes_at_a_title_capacity_edge_leave_a_valid_title_that_fits() {
    let some: Vec<_> = samples()
        .into_iter()
        .filter(|(n, _)| {
            [
                "truncated E8 87",
                "stray continuation 80",
                "surrogate ED A0 80",
                "overlong E0 80 80",
                "F8 88 80 80 80",
                "truncated F0 A0 AE",
                "valid then truncated",
                "byte FF",
            ]
            .contains(&n.as_str())
        })
        .collect();
    assert_eq!(some.len(), 8);
    for ver in [Ver::V2, Ver::V3] {
        for (name, seg) in &some {
            for pad_back in 0..=8usize {
                let check = |cap: usize, field: &str| {
                    let mut raw = "a".repeat(cap - pad_back).into_bytes();
                    raw.extend_from_slice(seg);
                    raw.extend_from_slice(b"zzzz");
                    let full = lossy(&raw);
                    let p = match field {
                        "toc" => book_with(ver, b"T", b"A", &[&raw]),
                        "title" => book_with(ver, &raw, b"A", &[b"x"]),
                        _ => book_with(ver, b"T", &raw, &[b"x"]),
                    };
                    let stored = match field {
                        "toc" => toc_title_bytes(&p, 0),
                        "title" => meta_title_bytes(&p),
                        _ => meta_author_bytes(&p),
                    };
                    let label =
                        format!("{ver:?} {field}, {name}, {pad_back} ASCII bytes before the cap");
                    match std::str::from_utf8(&stored) {
                        Err(e) => {
                            soft_fail(format!("{label}: not valid UTF-8 ({e}): {}", hex(&stored)))
                        }
                        Ok(s) => {
                            if stored.len() > cap {
                                soft_fail(format!("{label}: {} bytes > cap {cap}", stored.len()));
                            }
                            if !full.starts_with(s) {
                                soft_fail(format!("{label}: {s:?} is not a prefix of {full:?}"));
                            }
                            if s.is_empty() {
                                soft_fail(format!("{label}: title lost"));
                            }
                        }
                    }
                };
                check(TOC_TITLE_CAP, "toc");
                check(TITLE_CAP, "title");
                check(AUTHOR_CAP, "author");
            }
        }
    }
    finish();
}

// the in-place stripper (container / OPF / TOC documents)
#[test]
fn the_in_place_stripper_turns_malformed_bytes_into_replacement_characters() {
    for (name, seg) in samples() {
        let input = with_around(&seg);
        let mut buf = [b"<p>".as_slice(), &input, b"</p>"].concat();
        smol_epub::html_strip::strip_html_inplace(&mut buf);
        check_text(
            &buf,
            &lossy(&input),
            &format!("{name} via strip_html_inplace"),
        );
    }
    finish();
}

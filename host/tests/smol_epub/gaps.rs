// Holes the first round left in the guard of smol-epub's repairs: wrapped numeric references,
// white space after a cut sequence in a nav label, a cut sequence at the end of a chapter on
// the async in-memory route, async inflate at the sizes sync inflate once failed at, genuinely
// oversize entries, a cut sequence directly before `&`, `finish` with a short output slice and
// input that ends inside a reference.
//
// Run: cargo test-host --test smol_gaps
//
// Oracle: HTML5 numeric character reference rules (a number above U+10FFFF, however written,
// is U+FFFD; leading zeros do not change the number), `String::from_utf8_lossy` (one U+FFFD
// per maximal subpart; an ASCII byte or the end of input ends a sequence), HTML white space
// collapsing (a run of white space is one space), `char::from_u32`, and the documented
// contracts of `HtmlStripStream::finish` / `extract_entry`.

use crate::smol_common;

use smol_common::*;

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

// ---------------------------------------------------------------------------
// numeric references past u32
// ---------------------------------------------------------------------------

fn check_ref(reference: &str, shown: &str) {
    let xhtml = p_doc(format!("a{reference}b").as_bytes());
    let want = format!("a{shown}b");
    let label = if reference.len() > 40 {
        format!("{}...({} bytes)", &reference[..30], reference.len())
    } else {
        reference.to_string()
    };
    for (route, out) in all_routes(&xhtml) {
        check_text(&out, &want, &format!("{label} via {route}"));
    }
    let mut buf = format!("<p>a{reference}b</p>").into_bytes();
    smol_epub::html_strip::strip_html_inplace(&mut buf);
    check_text(&buf, &want, &format!("{label} via strip_html_inplace"));
}

// 2^k + low is above U+10FFFF; reduced modulo 2^32 it is `low`, a scalar the wrong answer
// would show
#[test]
fn a_numeric_reference_that_overflows_u32_is_a_replacement_character_not_the_wrapped_value() {
    for low in [0x41u128, 0x81FA, 0x20BB7, 0x1F600, 0x3042] {
        for shift in [32u32, 33, 40, 64, 96] {
            let v = (1u128 << shift) + low;
            check_ref(&format!("&#{v};"), "\u{FFFD}");
            check_ref(&format!("&#x{v:X};"), "\u{FFFD}");
            check_ref(&format!("&#x{v:x};"), "\u{FFFD}");
        }
    }
    // the exact cases of the bug: 2^32 + 1 is not U+0001, 2^32 + 0x41 is not "A"
    check_ref("&#4294967297;", "\u{FFFD}");
    check_ref("&#x100000041;", "\u{FFFD}");
    finish();
}

#[test]
fn a_numeric_reference_of_any_length_above_10ffff_is_one_replacement_character() {
    for n in [7usize, 10, 11, 12, 13, 20, 30, 64, 65, 100, 300] {
        check_ref(&format!("&#{};", "9".repeat(n)), "\u{FFFD}");
        check_ref(&format!("&#1{};", "0".repeat(n)), "\u{FFFD}");
        check_ref(&format!("&#x{};", "F".repeat(n.max(6))), "\u{FFFD}");
        check_ref(&format!("&#x1{};", "0".repeat(n.max(6))), "\u{FFFD}");
    }
    finish();
}

#[test]
fn a_numeric_reference_with_many_leading_zeros_is_still_its_number() {
    for zeros in [1usize, 5, 11, 12, 13, 30, 64, 65, 100, 300] {
        let z = "0".repeat(zeros);
        check_ref(&format!("&#{z}33274;"), "\u{81fa}");
        check_ref(&format!("&#x{z}81FA;"), "\u{81fa}");
        check_ref(&format!("&#x{z}20BB7;"), "\u{20bb7}");
        check_ref(&format!("&#{z}65;"), "A");
        // zeros then no scalar
        check_ref(&format!("&#{z};"), "\u{FFFD}");
        check_ref(&format!("&#x{z}D800;"), "\u{FFFD}");
        check_ref(&format!("&#x{z}110000;"), "\u{FFFD}");
        check_ref(&format!("&#x{z}100000041;"), "\u{FFFD}");
    }
    finish();
}

// a long reference cut by the buffer boundaries, and followed by more references
#[test]
fn a_long_numeric_reference_survives_every_buffer_alignment() {
    let long_ok = format!("&#x{}81FA;", "0".repeat(70));
    let long_bad = format!("&#{};", "9".repeat(70));
    for (reference, shown) in [(long_ok, "\u{81fa}"), (long_bad, "\u{FFFD}")] {
        for pad in boundary_pads(DOC_HEAD.len(), reference.len()) {
            let f = filler(pad, 21 + pad as u64);
            let xhtml = p_doc(format!("{f}{reference}&#x81FA;z").as_bytes());
            let want = format!("{f}{shown}\u{81fa}z");
            for deflate in [false, true] {
                let bytes = chapter_zip(&xhtml, deflate);
                let zip = open_zip(&bytes).unwrap();
                check_text(
                    &route_stream(&bytes, &zip, "c.xhtml"),
                    &want,
                    &format!(
                        "pad {pad}, deflate {deflate}, {} byte reference",
                        reference.len()
                    ),
                );
            }
        }
    }
    finish();
}

// ---------------------------------------------------------------------------
// nav label: white space after a cut sequence
// ---------------------------------------------------------------------------

// a run of HTML white space is one space; applied to the input bytes first (white space is
// ASCII, so it ends a sequence either way), then the lossy decoding, then the end trimmed
fn nav_expected(raw: &[u8]) -> String {
    let mut collapsed = Vec::new();
    let mut in_ws = false;
    for &b in raw {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
            if !in_ws {
                collapsed.push(b' ');
            }
            in_ws = true;
        } else {
            collapsed.push(b);
            in_ws = false;
        }
    }
    lossy(&collapsed).trim_end_matches(' ').to_string()
}

const CUT: [(&str, &[u8]); 7] = [
    ("E8 87", &[0xE8, 0x87]),
    ("E8", &[0xE8]),
    ("F0 A0 AE", &[0xF0, 0xA0, 0xAE]),
    ("F0 A0", &[0xF0, 0xA0]),
    ("C3", &[0xC3]),
    ("ED A0", &[0xED, 0xA0]),
    ("E8 E8 87", &[0xE8, 0xE8, 0x87]),
];

fn nav_label(raw: &[u8]) -> Vec<u8> {
    let mut b = Book::new(Ver::V3, vec![p_doc(b"x")]);
    b.toc_titles = vec![raw.to_vec()];
    let p = parse(&build_book(&b));
    assert_eq!(p.toc.len(), 1);
    toc_title_bytes(&p, 0)
}

#[test]
fn white_space_after_a_cut_sequence_in_a_nav_label_ends_it_before_the_space() {
    for (name, seg) in CUT {
        for ws in [" ", "\t", "\n", "  ", " \n\t ", "\r\n"] {
            // text, the cut sequence, white space, more text
            let raw = [b"x".as_slice(), seg, ws.as_bytes(), b"A"].concat();
            check_stored(
                &nav_label(&raw),
                &nav_expected(&raw),
                &format!("nav, {name}, ws {ws:?}"),
            );
            // the cut sequence then white space at the end of the label
            let raw = [b"xA".as_slice(), seg, ws.as_bytes()].concat();
            check_stored(
                &nav_label(&raw),
                &nav_expected(&raw),
                &format!("nav end, {name}, ws {ws:?}"),
            );
            // two cut sequences apart by white space
            let raw = [seg, b"x", seg, ws.as_bytes(), seg, b"y"].concat();
            check_stored(
                &nav_label(&raw),
                &nav_expected(&raw),
                &format!("nav twice, {name}, ws {ws:?}"),
            );
        }
    }
    finish();
}

// the same bytes with one space in an NCX label (no collapsing involved)
#[test]
fn a_single_space_after_a_cut_sequence_in_a_ncx_label_ends_it_before_the_space() {
    for (name, seg) in CUT {
        let raw = [b"x".as_slice(), seg, b" A"].concat();
        let mut b = Book::new(Ver::V2, vec![p_doc(b"x")]);
        b.toc_titles = vec![raw.clone()];
        let p = parse(&build_book(&b));
        check_stored(
            &toc_title_bytes(&p, 0),
            &lossy(&raw),
            &format!("ncx, {name}"),
        );
    }
    finish();
}

// ---------------------------------------------------------------------------
// the end of input: async in-memory route, `finish` with short slices, a reference cut off
// ---------------------------------------------------------------------------

#[test]
fn the_async_in_memory_strip_closes_a_sequence_cut_by_the_end_of_the_chapter() {
    for (name, seg) in CUT {
        // no closing tags: the document ends inside the sequence
        let xhtml = [format!("{DOC_HEAD}a").into_bytes(), seg.to_vec()].concat();
        let want = lossy(&[b"a".as_slice(), seg].concat());
        let a = route_buf_async(&xhtml);
        check_text(&a, &want, &format!("{name}: strip_html_buf_async at EOF"));
        // and the sync route agrees byte for byte
        let s = smol_epub::cache::strip_html_buf(&xhtml).unwrap();
        if a != s {
            soft_fail(format!("{name}: async {} != sync {}", hex(&a), hex(&s)));
        }
    }
    finish();
}

#[test]
fn finish_with_a_short_slice_gives_the_same_text_when_called_again() {
    for (name, seg) in CUT {
        let xhtml = [format!("{DOC_HEAD}a").into_bytes(), seg.to_vec()].concat();
        let whole = route_feed(&xhtml, xhtml.len(), 4096);
        check_text(
            &whole,
            &lossy(&[b"a".as_slice(), seg].concat()),
            &format!("{name}: whole"),
        );
        for chunk in [1, xhtml.len()] {
            for out_cap in [1, 2, 3, 4096] {
                for finish_cap in [1, 2, 3, 4, 5] {
                    let got = route_feed_caps(&xhtml, chunk, out_cap, finish_cap);
                    if got != whole {
                        soft_fail(format!(
                            "{name}: chunk {chunk}, out {out_cap}, finish {finish_cap}: {} != {}",
                            hex(&got),
                            hex(&whole)
                        ));
                    }
                }
            }
        }
    }
    finish();
}

// the input ends inside a reference: nothing to hold the stripper to but valid UTF-8 and the
// text before it
#[test]
fn input_that_ends_inside_a_reference_gives_valid_utf8_and_the_text_before_it() {
    for tail in [
        "&#x81",
        "&#x81FA",
        "&#33274",
        "&#",
        "&#x",
        "&am",
        "&amp",
        "&#xD800",
        "&#x110000",
    ] {
        let xhtml = format!("{DOC_HEAD}ab {tail}").into_bytes();
        let whole = route_feed(&xhtml, xhtml.len(), 4096);
        for (route, out) in all_routes(&xhtml) {
            let t = plain(&out);
            match std::str::from_utf8(&t) {
                Err(e) => soft_fail(format!(
                    "{tail:?} via {route}: invalid UTF-8 ({e}): {}",
                    hex(&t)
                )),
                Ok(s) if !s.starts_with("ab") => soft_fail(format!(
                    "{tail:?} via {route}: text before it changed: {s:?}"
                )),
                Ok(_) => {}
            }
        }
        for finish_cap in [1, 2, 3] {
            let got = route_feed_caps(&xhtml, 1, 1, finish_cap);
            if got != whole {
                soft_fail(format!(
                    "{tail:?}: finish {finish_cap}: {} != {}",
                    hex(&got),
                    hex(&whole)
                ));
            }
        }
    }
    finish();
}

// ---------------------------------------------------------------------------
// a cut sequence directly before `&`
// ---------------------------------------------------------------------------

// reference -> what it stands for
const FOLLOWERS: [(&str, &str); 9] = [
    ("&amp;", "&"),
    ("&lt;", "<"),
    ("&#x81FA;", "\u{81fa}"),
    ("&#x20BB7;", "\u{20bb7}"),
    ("&#xD800;", "\u{FFFD}"),
    ("&#0;", "\u{FFFD}"),
    ("&#xZZ;", "&"),
    ("<b>x</b>", "x"),
    (" ", " "),
];

#[test]
fn a_cut_sequence_before_a_reference_is_replaced_before_what_the_reference_gives() {
    for (name, seg) in CUT {
        for (follower, shown) in FOLLOWERS {
            let inner = [b"a".as_slice(), seg, follower.as_bytes(), b"b"].concat();
            let want = format!("a{}{shown}b", lossy(seg));
            let xhtml = p_doc(&inner);
            for (route, out) in all_routes(&xhtml) {
                check_text(&out, &want, &format!("{name} + {follower} via {route}"));
            }
        }
    }
    finish();
}

#[test]
fn a_cut_sequence_before_a_reference_is_replaced_in_order_at_every_buffer_alignment() {
    for (follower, shown) in [("&#x81FA;", "\u{81fa}"), ("&amp;", "&")] {
        for seg in [[0xE8u8, 0x87].as_slice(), [0xF0, 0xA0, 0xAE].as_slice()] {
            for pad in boundary_pads(DOC_HEAD.len(), seg.len() + follower.len()) {
                let f = filler(pad, 31 + pad as u64);
                let inner = [f.as_bytes(), seg, follower.as_bytes(), b"z"].concat();
                let want = format!("{f}{}{shown}z", lossy(seg));
                let xhtml = p_doc(&inner);
                for deflate in [false, true] {
                    let bytes = chapter_zip(&xhtml, deflate);
                    let zip = open_zip(&bytes).unwrap();
                    check_text(
                        &route_stream(&bytes, &zip, "c.xhtml"),
                        &want,
                        &format!(
                            "{follower}, {} byte seq, pad {pad}, deflate {deflate}",
                            seg.len()
                        ),
                    );
                }
            }
        }
    }
    finish();
}

// ---------------------------------------------------------------------------
// async inflate, and entries that really are too big
// ---------------------------------------------------------------------------

#[test]
fn async_extract_returns_a_whole_deflate_entry_at_every_compressed_size() {
    let mut bad = Vec::new();
    let mut near_buffer_multiple = 0;
    for pad in 0..20000 {
        let xhtml = p_doc(filler(pad, 7 + pad as u64).as_bytes());
        let bytes = chapter_zip(&xhtml, true);
        let zip = open_zip(&bytes).unwrap();
        // compressed size 4096 k + 1 or + 2: the sizes sync inflate failed at
        if matches!(zip.entry(0).comp_size % 4096, 1 | 2) {
            near_buffer_multiple += 1;
        }
        match extract_async(&bytes, &zip, "c.xhtml") {
            Ok(v) if v == xhtml => {}
            other => bad.push((pad, zip.entry(0).comp_size, other.err())),
        }
    }
    assert!(
        near_buffer_multiple >= 3,
        "the sweep reached only {near_buffer_multiple} sizes of 4096 k + 1 / + 2"
    );
    assert!(
        bad.is_empty(),
        "{} entries not returned whole, e.g. (pad, compressed size, error) {:?}",
        bad.len(),
        &bad[..bad.len().min(6)]
    );
}

#[test]
fn the_named_compressed_sizes_extract_on_both_routes() {
    // compressed sizes 4098, 8193, 8194, 12289 found by sweeping the filler length
    let mut wanted = vec![4098u32, 8193, 8194, 12289];
    for pad in 0..20000 {
        if wanted.is_empty() {
            break;
        }
        let xhtml = p_doc(filler(pad, 7 + pad as u64).as_bytes());
        let bytes = chapter_zip(&xhtml, true);
        let zip = open_zip(&bytes).unwrap();
        let comp = zip.entry(0).comp_size;
        if let Some(i) = wanted.iter().position(|&w| w == comp) {
            wanted.remove(i);
            assert_eq!(
                extract_sync(&bytes, &zip, "c.xhtml").as_deref(),
                Ok(xhtml.as_slice()),
                "sync, compressed {comp}"
            );
            assert_eq!(
                extract_async(&bytes, &zip, "c.xhtml").as_deref(),
                Ok(xhtml.as_slice()),
                "async, compressed {comp}"
            );
        }
    }
    assert!(
        wanted.is_empty(),
        "the sweep did not reach compressed sizes {wanted:?}"
    );
}

// declared size smaller than what the stream inflates to: an error, and it ends
#[test]
fn an_entry_that_inflates_past_its_declared_size_is_an_error_on_both_routes() {
    for n in [100usize, 5000, 9000, 20000] {
        let xhtml = p_doc(filler(n, 5).as_bytes());
        let good = chapter_zip(&xhtml, true);
        for declared in [xhtml.len() as u32 - 1, xhtml.len() as u32 / 2, 1] {
            let bytes = declare_uncompressed_size(&good, declared);
            for route in ["sync", "async"] {
                let b = bytes.clone();
                let r = with_timeout(20, move || {
                    let zip = open_zip(&b).unwrap();
                    if route == "sync" {
                        extract_sync(&b, &zip, "c.xhtml")
                    } else {
                        extract_async(&b, &zip, "c.xhtml")
                    }
                });
                match r {
                    None => soft_fail(format!(
                        "{route}: {n} bytes declared {declared}: did not finish in 20 s"
                    )),
                    Some(Ok(v)) => soft_fail(format!(
                        "{route}: {n} bytes declared {declared}: accepted, {} bytes returned",
                        v.len()
                    )),
                    Some(Err(_)) => {}
                }
            }
        }
    }
    finish();
}

// the helper leaves an archive that declares its true size untouched
#[test]
fn declaring_the_true_size_changes_nothing() {
    let xhtml = p_doc(filler(5000, 5).as_bytes());
    let good = chapter_zip(&xhtml, true);
    let same = declare_uncompressed_size(&good, xhtml.len() as u32);
    assert_eq!(good, same);
    let zip = open_zip(&same).unwrap();
    assert_eq!(extract_sync(&same, &zip, "c.xhtml").unwrap(), xhtml);
    assert_eq!(extract_async(&same, &zip, "c.xhtml").unwrap(), xhtml);
}

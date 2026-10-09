// Chapter text through smol-epub's stripping pipelines: whole Unicode scalars survive every
// buffer boundary.
//
// Run: cargo test-host --test smol_stream
//
// A chapter is one paragraph: `pad` ASCII bytes, then a text of 2-, 3- and 4-byte scalars.
// The pad moves the text over every byte alignment of the internal buffers (the private
// 4096-byte read / strip buffers, the 3968-byte flush point and the 32768-byte window:
// `pad` runs 0..=8300 and then through windows around every multiple of 3968 and 4096 up
// to ~40 KB and around 32768). Both STORED and DEFLATE entries; routes: `stream_strip_entry`,
// `extract_entry` + `strip_html_buf`, `stream_strip_entry_async`, and `HtmlStripStream::feed`
// with 1..=9-byte input chunks.
//
// Oracle: RFC 3629 (`str::from_utf8` judges), and the expected text written by hand from the
// document: `<p>X</p>` strips to X (markers aside, white space at the ends trimmed); filler
// has single spaces only, so nothing else collapses. Entity forms `&#xHHHH;` of the same
// scalars are written from `char` code points.

use crate::smol_common;

use smol_common::*;

fn sample_entities(s: &str) -> String {
    s.chars().map(|c| format!("&#x{:X};", c as u32)).collect()
}

// the text after `pad` filler bytes, as the document body and as the text it must strip to
struct Case {
    body: String,
    want: String,
}

fn case(pad: usize, text: &str, tail: usize, as_entities: bool) -> Case {
    let f = filler(pad, 7 + pad as u64);
    let t = filler(tail, 99);
    let shown = if as_entities {
        sample_entities(text)
    } else {
        text.to_string()
    };
    Case {
        body: format!("{f}{shown}{t}"),
        want: format!("{f}{text}{t}"),
    }
}

fn check(out: &[u8], want: &str, label: &str) {
    let text = plain(out);
    let s = std::str::from_utf8(&text).unwrap_or_else(|e| {
        panic!(
            "{label}: output is not valid UTF-8 ({e}): ...{}",
            hex(&text[text.len().saturating_sub(12)..])
        )
    });
    assert!(!s.contains(FFFD), "{label}: output holds U+FFFD");
    assert_eq!(s, want, "{label}");
}

fn sweep(text: &str, tail: usize, as_entities: bool, pads: &[usize]) {
    for &pad in pads {
        let c = case(pad, text, tail, as_entities);
        let xhtml = p_doc(c.body.as_bytes());
        for deflate in [false, true] {
            let bytes = chapter_zip(&xhtml, deflate);
            let zip = open_zip(&bytes).unwrap();
            let label = |r: &str| {
                format!("{r}, deflate {deflate}, pad {pad}, tail {tail}, entities {as_entities}")
            };
            check(
                &route_stream(&bytes, &zip, "c.xhtml"),
                &c.want,
                &label("stream"),
            );
            if let Some(out) = route_buf(&bytes, &zip, "c.xhtml") {
                check(&out, &c.want, &label("buf"));
            }
        }
    }
}

#[test]
fn mixed_width_text_at_the_end_of_a_chapter_survives_every_buffer_alignment() {
    let pads = boundary_pads(DOC_HEAD.len(), MIXED_UNIT.len());
    sweep(MIXED_UNIT, 0, false, &pads);
}

#[test]
fn mixed_width_text_followed_by_more_text_survives_every_buffer_alignment() {
    let pads = boundary_pads(DOC_HEAD.len(), MIXED_UNIT.len());
    sweep(MIXED_UNIT, 300, false, &pads);
}

#[test]
fn each_scalar_width_alone_survives_every_buffer_alignment() {
    for unit in [E_ACUTE, TAI, YOSHI] {
        let text = unit.repeat(3);
        let pads = boundary_pads(DOC_HEAD.len(), text.len());
        sweep(&text, 40, false, &pads);
    }
}

#[test]
fn numeric_entities_of_mixed_width_scalars_survive_every_buffer_alignment() {
    let span = sample_entities(MIXED_UNIT).len();
    let pads = boundary_pads(DOC_HEAD.len(), span);
    sweep(MIXED_UNIT, 0, true, &pads);
    sweep(MIXED_UNIT, 300, true, &pads);
}

#[test]
fn the_async_stream_reassembles_mixed_width_text_at_every_buffer_alignment() {
    let pads = boundary_pads(DOC_HEAD.len(), MIXED_UNIT.len());
    for pad in pads {
        let c = case(pad, MIXED_UNIT, 20, false);
        let xhtml = p_doc(c.body.as_bytes());
        for deflate in [false, true] {
            let bytes = chapter_zip(&xhtml, deflate);
            let zip = open_zip(&bytes).unwrap();
            check(
                &route_async(&bytes, &zip, "c.xhtml"),
                &c.want,
                &format!("async, deflate {deflate}, pad {pad}"),
            );
        }
    }
}

// the same document through the three whole-chapter routes: byte-identical output
#[test]
fn stream_buffer_and_async_routes_return_the_same_bytes() {
    for pad in (0..9000).step_by(37) {
        let c = case(pad, MIXED_UNIT, 50, false);
        let xhtml = p_doc(c.body.as_bytes());
        for deflate in [false, true] {
            let bytes = chapter_zip(&xhtml, deflate);
            let zip = open_zip(&bytes).unwrap();
            let a = route_stream(&bytes, &zip, "c.xhtml");
            if let Some(b) = route_buf(&bytes, &zip, "c.xhtml") {
                assert_eq!(a, b, "buf, pad {pad}, deflate {deflate}");
            }
            assert_eq!(
                a,
                route_async(&bytes, &zip, "c.xhtml"),
                "async, pad {pad}, deflate {deflate}"
            );
        }
    }
}

// the chunked stripper: any split of the input, any output slice size
#[test]
fn a_stripper_fed_in_small_pieces_reassembles_mixed_width_text() {
    for pad in 0..24 {
        for as_entities in [false, true] {
            let c = case(pad, MIXED_UNIT, 10, as_entities);
            let xhtml = p_doc(c.body.as_bytes());
            for chunk in 1..=9 {
                for out_cap in [4, 8, 16, 64, 4096] {
                    check(
                        &route_feed(&xhtml, chunk, out_cap),
                        &c.want,
                        &format!(
                            "feed, chunk {chunk}, out {out_cap}, pad {pad}, entities {as_entities}"
                        ),
                    );
                }
            }
        }
    }
}

#[test]
fn a_stripper_fed_one_byte_at_a_time_matches_one_whole_feed() {
    for pad in [0, 1, 2, 3, 4000, 4090] {
        let c = case(pad, MIXED_UNIT, 100, false);
        let xhtml = p_doc(c.body.as_bytes());
        let whole = route_feed(&xhtml, xhtml.len(), 8192);
        assert_eq!(route_feed(&xhtml, 1, 8192), whole, "pad {pad}");
        assert_eq!(plain(&whole), c.want.as_bytes(), "pad {pad}");
    }
}

// `extract_entry` hands back the entry whole, whatever its compressed size relative to the
// 4096-byte read buffer (oracle: the bytes that went into the archive)
#[test]
fn extract_entry_returns_a_whole_deflate_entry_at_every_compressed_size() {
    let mut bad = Vec::new();
    for pad in 0..20000 {
        let xhtml = p_doc(filler(pad, 7 + pad as u64).as_bytes());
        let bytes = chapter_zip(&xhtml, true);
        let zip = open_zip(&bytes).unwrap();
        let comp = zip.entry(0).comp_size;
        match entry_bytes(&bytes, &zip, "c.xhtml") {
            Ok(v) if v == xhtml => {}
            other => bad.push((pad, comp, other.err())),
        }
    }
    assert!(
        bad.is_empty(),
        "{} of 20000 entries not returned whole, e.g. (pad, compressed size, error) {:?}",
        bad.len(),
        &bad[..bad.len().min(6)]
    );
}

#[test]
fn extract_entry_returns_a_whole_stored_entry_at_every_size() {
    for pad in 0..9000 {
        let xhtml = p_doc(filler(pad, 3 + pad as u64).as_bytes());
        let bytes = chapter_zip(&xhtml, false);
        let zip = open_zip(&bytes).unwrap();
        assert_eq!(
            entry_bytes(&bytes, &zip, "c.xhtml").unwrap(),
            xhtml,
            "pad {pad}"
        );
    }
}

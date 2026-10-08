// Titles that outgrow their fixed field: a TOC label (NCX and EPUB 3 nav, `TOC_TITLE_CAP`
// bytes), the OPF dc:title (`TITLE_CAP`) and dc:creator (`AUTHOR_CAP`). The cut falls on a
// scalar boundary at every byte alignment of 2-, 3- and 4-byte scalars.
//
// Run: cargo test-host --test smol_toc
//
// Each title is `pad` ASCII bytes, then a unit repeated, and every whole-scalar prefix of that
// from CAP - 8 bytes (fits whole) to CAP + 12 bytes (cut) is written to the book.
//
// Oracle: the stored title is the longest whole-scalar prefix of the written title that fits
// the capacity (`str::is_char_boundary` finds it; no ellipsis), valid UTF-8 (RFC 3629;
// `str::from_utf8` judges), not empty while the title is not. Capacities are the public
// constants of smol_epub::epub.

mod smol_common;

use smol_common::*;
use smol_epub::epub::{AUTHOR_CAP, TITLE_CAP, TOC_TITLE_CAP};

const UNITS: [&str; 4] = [E_ACUTE, TAI, YOSHI, MIXED_UNIT];

// every title of the sweep for capacity `cap`
fn titles(cap: usize) -> Vec<String> {
    let mut v = Vec::new();
    for unit in UNITS {
        for pad in 0..8 {
            let base = long_title(pad, unit, cap + 12);
            for len in cap - 8..=cap + 12 {
                if len <= base.len() && base.is_char_boundary(len) {
                    v.push(base[..len].to_string());
                }
            }
        }
    }
    v
}

fn check_cut(stored: &[u8], title: &str, cap: usize, label: &str) {
    let want = fitting_prefix(title, cap);
    if stored.len() > cap {
        soft_fail(format!("{label}: {} bytes > cap {cap}", stored.len()));
    }
    if stored.is_empty() {
        soft_fail(format!("{label}: the title was lost"));
    }
    check_stored(
        stored,
        want,
        &format!("{label} ({} byte title)", title.len()),
    );
}

#[test]
fn a_toc_label_longer_than_its_field_is_cut_on_a_scalar_boundary() {
    for ver in [Ver::V2, Ver::V3] {
        for title in titles(TOC_TITLE_CAP) {
            let mut b = Book::new(ver, vec![p_doc(b"x"), p_doc(b"y")]);
            // neighbours on both sides of the long label stay whole
            b.toc_titles = vec![
                "\u{524d}".as_bytes().to_vec(),
                title.clone().into_bytes(),
                "\u{5f8c}".as_bytes().to_vec(),
            ];
            let p = parse(&build_book(&b));
            if p.toc.len() != 3 {
                soft_fail(format!(
                    "{ver:?}: {} entries, want 3 ({title:?})",
                    p.toc.len()
                ));
                continue;
            }
            check_cut(
                &toc_title_bytes(&p, 1),
                &title,
                TOC_TITLE_CAP,
                &format!("{ver:?} toc label"),
            );
            check_stored(
                &toc_title_bytes(&p, 0),
                "\u{524d}",
                &format!("{ver:?} label before"),
            );
            check_stored(
                &toc_title_bytes(&p, 2),
                "\u{5f8c}",
                &format!("{ver:?} label after"),
            );
            // `title_str` is what the reader shows: it must not come back empty either
            let shown = p.toc.entries[1].title_str();
            if shown != fitting_prefix(&title, TOC_TITLE_CAP) {
                soft_fail(format!("{ver:?} title_str() {shown:?} for {title:?}"));
            }
        }
    }
    finish();
}

#[test]
fn a_toc_label_longer_than_its_field_is_cut_the_same_in_a_deflated_book() {
    for ver in [Ver::V2, Ver::V3] {
        for title in titles(TOC_TITLE_CAP).iter().step_by(5) {
            let mut b = Book::new(ver, vec![p_doc(b"x")]);
            b.deflate = true;
            b.toc_titles = vec![title.clone().into_bytes()];
            let p = parse(&build_book(&b));
            check_cut(
                &toc_title_bytes(&p, 0),
                title,
                TOC_TITLE_CAP,
                &format!("{ver:?} deflated toc label"),
            );
        }
    }
    finish();
}

#[test]
fn an_opf_title_longer_than_its_field_is_cut_on_a_scalar_boundary() {
    for ver in [Ver::V2, Ver::V3] {
        for title in titles(TITLE_CAP) {
            let mut b = Book::new(ver, vec![p_doc(b"x")]);
            b.title = title.clone().into_bytes();
            b.toc_titles = vec![b"x".to_vec()];
            let p = parse(&build_book(&b));
            check_cut(
                &meta_title_bytes(&p),
                &title,
                TITLE_CAP,
                &format!("{ver:?} dc:title"),
            );
            let shown = p.meta.title_str();
            if shown != fitting_prefix(&title, TITLE_CAP) {
                soft_fail(format!("{ver:?} title_str() {shown:?} for {title:?}"));
            }
        }
    }
    finish();
}

#[test]
fn an_opf_creator_longer_than_its_field_is_cut_on_a_scalar_boundary() {
    for ver in [Ver::V2, Ver::V3] {
        for title in titles(AUTHOR_CAP) {
            let mut b = Book::new(ver, vec![p_doc(b"x")]);
            b.creator = title.clone().into_bytes();
            b.toc_titles = vec![b"x".to_vec()];
            let p = parse(&build_book(&b));
            check_cut(
                &meta_author_bytes(&p),
                &title,
                AUTHOR_CAP,
                &format!("{ver:?} dc:creator"),
            );
            let shown = p.meta.author_str();
            if shown != fitting_prefix(&title, AUTHOR_CAP) {
                soft_fail(format!("{ver:?} author_str() {shown:?} for {title:?}"));
            }
        }
    }
    finish();
}

// a title that fits exactly (no cut at all) is kept whole, in every width
#[test]
fn a_title_that_fits_its_field_exactly_is_kept_whole() {
    for ver in [Ver::V2, Ver::V3] {
        for unit in UNITS {
            for pad in 0..8 {
                let toc = fitting_prefix(&long_title(pad, unit, TOC_TITLE_CAP), TOC_TITLE_CAP)
                    .to_string();
                let name = fitting_prefix(&long_title(pad, unit, TITLE_CAP), TITLE_CAP).to_string();
                let by = fitting_prefix(&long_title(pad, unit, AUTHOR_CAP), AUTHOR_CAP).to_string();
                let mut b = Book::new(ver, vec![p_doc(b"x")]);
                b.title = name.clone().into_bytes();
                b.creator = by.clone().into_bytes();
                b.toc_titles = vec![toc.clone().into_bytes()];
                let p = parse(&build_book(&b));
                check_stored(
                    &toc_title_bytes(&p, 0),
                    &toc,
                    &format!("{ver:?} toc, pad {pad}"),
                );
                check_stored(
                    &meta_title_bytes(&p),
                    &name,
                    &format!("{ver:?} title, pad {pad}"),
                );
                check_stored(
                    &meta_author_bytes(&p),
                    &by,
                    &format!("{ver:?} author, pad {pad}"),
                );
            }
        }
    }
    finish();
}

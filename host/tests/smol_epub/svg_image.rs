// The picture of an SVG page (`<svg><image xlink:href="..."/></svg>`, what a
// fixed-layout cover, frontispiece or title page is made of) is an image
// reference like the `src` of an `<img>`.
use smol_epub::html_strip::{HtmlStripStream, IMG_REF, MARKER};

fn strip(html: &str) -> Vec<u8> {
    let mut s = HtmlStripStream::new();
    let mut out = vec![0u8; html.len() + 64];
    let (mut r, mut w) = (0, 0);
    while r < html.len() {
        let (used, put) = s.feed(&html.as_bytes()[r..], &mut out[w..]);
        assert!(used > 0 || put > 0, "the stream makes progress");
        r += used;
        w += put;
    }
    w += s.finish(&mut out[w..]);
    out.truncate(w);
    out
}

// every [MARKER, IMG_REF, len, path...] of the stripped text
fn references(text: &[u8]) -> Vec<String> {
    let mut found = vec![];
    let mut i = 0;
    while i + 2 < text.len() {
        if text[i] == MARKER && text[i + 1] == IMG_REF {
            let len = usize::from(text[i + 2]);
            found.push(String::from_utf8(text[i + 3..i + 3 + len].to_vec()).unwrap());
            i += 3 + len;
        } else {
            i += 1;
        }
    }
    found
}

#[test]
fn an_svg_image_with_xlink_href_is_an_image_reference() {
    let html = r#"<body><div class="main"><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" width="100%" height="100%" viewBox="0 0 1382 2048">
<image width="1382" height="2048" xlink:href="../image/cover.jpg"/>
</svg></div></body>"#;
    assert_eq!(references(&strip(html)), ["../image/cover.jpg"]);
}

#[test]
fn an_svg_image_with_a_plain_href_is_an_image_reference() {
    let html = r#"<svg><image href="a.png" width="1" height="1"></image></svg>"#;
    assert_eq!(references(&strip(html)), ["a.png"]);
}

#[test]
fn a_tag_with_two_sources_keeps_the_first() {
    let html = r#"<svg><image xlink:href="first.png" href="second.png"/></svg>"#;
    assert_eq!(references(&strip(html)), ["first.png"]);
}

#[test]
fn an_img_keeps_its_src_and_ignores_other_attributes() {
    let html = r#"<p><img class="fit" src="../image/ch1.jpg" alt="ch1"/></p>"#;
    assert_eq!(references(&strip(html)), ["../image/ch1.jpg"]);
}

#[test]
fn text_around_an_svg_image_stays() {
    let html = r#"<p>before</p><svg><image xlink:href="p.jpg"/></svg><p>after</p>"#;
    let text = strip(html);
    assert_eq!(references(&text), ["p.jpg"]);
    let plain: String = String::from_utf8_lossy(&text).replace('\n', "");
    assert!(plain.contains("before") && plain.contains("after"));
}

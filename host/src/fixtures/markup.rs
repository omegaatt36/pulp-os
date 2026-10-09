// The XML parts of a fixture EPUB: container.xml, the OPF package, the NCX
// (EPUB 2) or nav document (EPUB 3), and the chapter XHTML. Every text node
// and attribute value goes through `escape`, so any spec text yields
// well-formed XML. Documents carry no timestamps or generated ids: the output
// is a pure function of the spec.

use std::fmt::Write;

use super::{Block, Chapter, EpubSpec, EpubVersion, ImageKind, MODIFIED, RIGHTS, Run, TocItem};

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

// & < > " are always escaped; with `numeric`, every non-ASCII character
// becomes a decimal character reference
fn escape(s: &str, numeric: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if numeric && !c.is_ascii() => {
                let _ = write!(out, "&#{};", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

fn esc(s: &str) -> String {
    escape(s, false)
}

// href of chapter `i` relative to OEBPS/
pub(super) fn chapter_href(i: usize) -> String {
    format!("text/chapter{:02}.xhtml", i + 1)
}

pub(super) fn container() -> String {
    format!(
        "{XML_DECL}<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\
         <rootfiles>\n\
         <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\n\
         </rootfiles>\n\
         </container>\n"
    )
}

fn media_type(kind: ImageKind) -> &'static str {
    match kind {
        ImageKind::Jpeg => "image/jpeg",
        _ => "image/png",
    }
}

pub(super) fn opf(spec: &EpubSpec) -> String {
    let v3 = spec.version == EpubVersion::V3;
    let mut x = String::from(XML_DECL);
    let _ = write!(
        x,
        "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"{}\" unique-identifier=\"bookid\">\n\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <dc:title>{}</dc:title>\n\
         <dc:creator>{}</dc:creator>\n\
         <dc:language>en</dc:language>\n\
         <dc:identifier id=\"bookid\">{}</dc:identifier>\n\
         <dc:rights>{}</dc:rights>\n",
        if v3 { "3.0" } else { "2.0" },
        esc(&spec.title),
        esc(&spec.author),
        esc(&spec.identifier),
        esc(RIGHTS),
    );
    if v3 {
        let _ = writeln!(x, "<meta property=\"dcterms:modified\">{MODIFIED}</meta>");
    } else if let Some(c) = spec.cover {
        let _ = writeln!(x, "<meta name=\"cover\" content=\"img{c}\"/>");
    }
    x.push_str("</metadata>\n<manifest>\n");
    if v3 {
        x.push_str("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    } else {
        x.push_str("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
    }
    for i in 0..spec.chapters.len() {
        let _ = writeln!(
            x,
            "<item id=\"ch{}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>",
            i + 1,
            chapter_href(i)
        );
    }
    for (i, im) in spec.images.iter().enumerate() {
        let cover = if v3 && spec.cover == Some(i) {
            " properties=\"cover-image\""
        } else {
            ""
        };
        let _ = writeln!(
            x,
            "<item id=\"img{i}\" href=\"{}\" media-type=\"{}\"{cover}/>",
            esc(&im.path),
            media_type(im.kind)
        );
    }
    x.push_str("</manifest>\n");
    if v3 {
        x.push_str("<spine>\n");
    } else {
        x.push_str("<spine toc=\"ncx\">\n");
    }
    for i in 0..spec.chapters.len() {
        let _ = writeln!(x, "<itemref idref=\"ch{}\"/>", i + 1);
    }
    x.push_str("</spine>\n</package>\n");
    x
}

fn ncx_points(x: &mut String, items: &[TocItem], order: &mut usize) {
    for it in items {
        *order += 1;
        let _ = write!(
            x,
            "<navPoint id=\"np{o}\" playOrder=\"{o}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/>",
            esc(&it.title),
            chapter_href(it.chapter),
            o = *order,
        );
        if !it.children.is_empty() {
            x.push('\n');
            ncx_points(x, &it.children, order);
        }
        x.push_str("</navPoint>\n");
    }
}

pub(super) fn ncx(spec: &EpubSpec) -> String {
    let mut x = String::from(XML_DECL);
    let _ = write!(
        x,
        "<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
         <head><meta name=\"dtb:uid\" content=\"{}\"/></head>\n\
         <docTitle><text>{}</text></docTitle>\n\
         <navMap>\n",
        esc(&spec.identifier),
        esc(&spec.title),
    );
    ncx_points(&mut x, &spec.toc, &mut 0);
    x.push_str("</navMap>\n</ncx>\n");
    x
}

fn nav_list(x: &mut String, items: &[TocItem]) {
    x.push_str("<ol>\n");
    for it in items {
        let _ = write!(
            x,
            "<li><a href=\"{}\">{}</a>",
            chapter_href(it.chapter),
            esc(&it.title)
        );
        if !it.children.is_empty() {
            x.push('\n');
            nav_list(x, &it.children);
        }
        x.push_str("</li>\n");
    }
    x.push_str("</ol>\n");
}

pub(super) fn nav(spec: &EpubSpec) -> String {
    let mut x = String::from(XML_DECL);
    let _ = write!(
        x,
        "<html xmlns=\"{XHTML_NS}\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
         <head><title>{}</title></head>\n\
         <body>\n\
         <nav epub:type=\"toc\">\n",
        esc(&spec.title),
    );
    nav_list(&mut x, &spec.toc);
    x.push_str("</nav>\n</body>\n</html>\n");
    x
}

fn runs(x: &mut String, rs: &[Run], numeric: bool) {
    for r in rs {
        match r {
            Run::Text(t) => x.push_str(&escape(t, numeric)),
            Run::Bold(t) => {
                let _ = write!(x, "<b>{}</b>", escape(t, numeric));
            }
            Run::Italic(t) => {
                let _ = write!(x, "<i>{}</i>", escape(t, numeric));
            }
            Run::Break => x.push_str("<br/>"),
        }
    }
}

pub(super) fn chapter(spec: &EpubSpec, ch: &Chapter) -> String {
    let n = spec.numeric_entities;
    let title = escape(&ch.title, n);
    let mut x = String::from(XML_DECL);
    let _ = write!(
        x,
        "<html xmlns=\"{XHTML_NS}\">\n<head><title>{title}</title></head>\n<body>\n<h1>{title}</h1>\n"
    );
    for b in &ch.blocks {
        match b {
            Block::Paragraph(rs) => {
                x.push_str("<p>");
                runs(&mut x, rs, n);
                x.push_str("</p>\n");
            }
            Block::Heading(t) => {
                let _ = writeln!(x, "<h2>{}</h2>", escape(t, n));
            }
            Block::Image(i) => {
                let _ = writeln!(
                    x,
                    "<p><img src=\"../{}\" alt=\"\"/></p>",
                    escape(&spec.images[*i].path, n)
                );
            }
        }
    }
    x.push_str("</body>\n</html>\n");
    x
}

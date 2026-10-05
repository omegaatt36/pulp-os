// The standard fixture set: five TXT files and six EPUBs (EPUB 2 / EPUB 3 x
// stored / deflate / mixed), named as 8.3 uppercase FAT files ("EPU" is what
// the firmware lists for a card that only keeps the short alias).
//
// All text is original and synthetic, composed from the sentence pool below.
// The pool deliberately carries the characters the readers must handle: Latin-1
// letters (2-byte UTF-8), typographic quotes, dashes and the ellipsis (3-byte),
// and the XML specials & < > ". Book title, author and TOC titles stay free of
// the specials: the production OPF / NCX / nav readers return those texts
// without decoding entities.

use super::{
    Block, Chapter, Compression, EpubSpec, EpubVersion, ImageKind, ImageSpec, Newline, Pattern,
    Run, TocItem, TxtSpec, build_epub, build_txt,
};
use crate::storage::VirtualStorage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    Txt(TxtSpec),
    Epub(EpubSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub name: &'static str,
    pub spec: Spec,
    pub bytes: Vec<u8>,
}

const PROVENANCE: &str = "Synthetic test fixture:";
const ID_PREFIX: &str = "urn:pulp-os:fixture:";

const SENTENCES: [&str; 41] = [
    "The ferry left the quay at six, trailing a ribbon of pale smoke across the harbor.",
    "Marta kept the lamp room spotless, though nobody had climbed the stairs since spring.",
    "A gull perched on the rail and watched the nets dry with the patience of an auditor.",
    "By noon the market smelled of rye bread, wet rope, and oranges from somewhere far warmer.",
    "Nobody could say who had painted the blue door, only that it had never needed repainting.",
    "The tide table, pinned beside the post office window, was wrong twice a week and trusted anyway.",
    "Anselm wrote his letters in pencil, because ink, he said, made promises that pencil could still revise.",
    "Rain arrived sideways, which the townsfolk considered a mild form of conversation.",
    "In the bakery window, a handwritten sign read \"Fresh today \u{2014} ask about yesterday\" in careful capitals.",
    "She counted the steps to the cellar again: twenty-two, the same as the year before.",
    "The map on the wall showed the coast as it had been drawn by someone who had never walked it.",
    "At the chandler\u{2019}s shop, brass compasses pointed in several slightly different directions.",
    "The clerk\u{2019}s ledger noted that 3 < 5 and 9 > 4, a finding that pleased nobody.",
    "A pot of tea went cold on the windowsill while the barometer made up its mind.",
    "Fish & chips, the sign promised, and for once the sign told the truth.",
    "\u{201c}You will want a coat,\u{201d} said the harbormaster, \u{201c}and perhaps a second opinion.\u{201d}",
    "The lighthouse beam swept the bay every nine seconds, as regular as a metronome with opinions.",
    "Children chased the last of the afternoon light down the breakwater, shouting in two languages.",
    "He remembered the caf\u{e9} on the corner, where the coffee arrived with a small, serious biscuit.",
    "Somewhere beyond the fog, a bell rang once, then thought better of it\u{2026}",
    "The postman, who knew every secret and kept most of them, whistled past the empty house.",
    "Lena sorted buttons by color, then by size, then by the stories she imagined for them.",
    "Old nets hung from the rafters like the sleeves of a very large, very patient coat.",
    "By evening the wind had shifted, and the weathervane, a copper fish, turned its back on the sea.",
    "A crate of lemons rolled down the ramp and stopped, as if embarrassed, at the harbormaster\u{2019}s boots.",
    "The choir practiced in the net loft, where every note came back with a faint smell of tar.",
    "Nothing in the ledger explained the missing teaspoon, but everyone had a theory.",
    "The schooner Wren came in on the evening tide, low in the water and visibly proud of it.",
    "On Sundays the quay belonged to the fiddlers, and the gulls were politely asked to leave.",
    "A thin line of gold stretched along the horizon, then folded itself away for the night.",
    "He read the same page three times, and each time it improved slightly.",
    "The bridge, by local agreement, was never crossed during an argument.",
    "Under the stairs, a trunk of faded signal flags waited for a festival postponed since the winter.",
    "Soup is better on the second day\u{2014}a rule the cook defended with considerable passion.",
    "The ferry\u{2019}s horn gave two short blasts, which meant: wait, we are almost there.",
    "Every window along the lane held a lamp, each a slightly different shade of patient.",
    "The tailor measured twice, sighed once, and cut anyway.",
    "A cold front, a warm front, and a very confused kite all met above the pier.",
    "Her grandmother\u{2019}s recipe called for a pinch of salt, a pinch of pepper, and a pinch of faith.",
    "The last boat tied up just before the dark, and the harbor exhaled.",
    "A visiting painter praised the na\u{ef}ve honesty of the harbor light and charged nothing for the compliment.",
];

// `n` consecutive-by-three sentences starting at `seed`; 41 is prime, so a
// passage never repeats a sentence and different seeds start in different places
fn sentence(seed: usize, i: usize) -> String {
    SENTENCES[(seed * 7 + i * 3) % SENTENCES.len()].to_string()
}

fn passage(seed: usize, n: usize) -> String {
    (0..n)
        .map(|i| sentence(seed, i))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- TXT ------------------------------------------------------------------

// header, a short title line, then `paragraphs` long lines separated by blank
// lines and an occasional short line; the last line is never blank
fn txt_lines(header: &str, seed: usize, paragraphs: usize) -> Vec<String> {
    let mut lines = vec![
        format!("{PROVENANCE} {header}"),
        String::new(),
        "LANTERN COVE".to_string(),
        String::new(),
    ];
    for p in 0..paragraphs {
        if p > 0 {
            lines.push(String::new());
            if p % 4 == 0 {
                lines.push("* * *".to_string());
                lines.push(String::new());
            }
        }
        lines.push(passage(seed + p, 4 + p % 3));
    }
    lines
}

fn txt_fixture(
    name: &'static str,
    lines: Vec<String>,
    newline: Newline,
    trailing_newline: bool,
) -> Fixture {
    let spec = TxtSpec {
        lines,
        newline,
        trailing_newline,
    };
    let bytes = build_txt(&spec);
    Fixture {
        name,
        spec: Spec::Txt(spec),
        bytes,
    }
}

fn utf8_lines() -> Vec<String> {
    let mut lines = txt_lines("multi-byte UTF-8 text.", 17, 24);
    let accents = [
        "Accents: caf\u{e9} cr\u{e8}me na\u{ef}ve se\u{f1}or \u{fc}ber Stra\u{df}e fa\u{e7}ade \u{e0} la carte no\u{eb}l g\u{f6}teborg",
        "Punctuation: \u{2018}single\u{2019} \u{201c}double\u{201d} en\u{2013}dash em\u{2014}dash ellipsis\u{2026}",
    ];
    for (i, line) in accents.into_iter().enumerate() {
        lines.insert(4 + i * 2, line.to_string());
        lines.insert(5 + i * 2, String::new());
    }
    lines
}

// ---- EPUB -----------------------------------------------------------------

fn text(t: String) -> Run {
    Run::Text(t)
}

// one paragraph; `kind` cycles through plain, bold, italic and line-break forms
fn paragraph(seed: usize, kind: usize) -> Block {
    let s = |i| sentence(seed, i);
    Block::Paragraph(match kind % 4 {
        0 => vec![text(passage(seed, 5))],
        1 => vec![
            text(format!("{} ", s(0))),
            Run::Bold(s(1)),
            text(format!(" {} {}", s(2), s(3))),
        ],
        2 => vec![Run::Italic(s(0)), text(format!(" {} {}", s(1), s(2)))],
        _ => vec![
            text(s(0)),
            Run::Break,
            text(s(1)),
            Run::Break,
            Run::Italic(s(2)),
        ],
    })
}

// `count` paragraphs with a heading before every `heading_every`-th one
fn body(seed: usize, count: usize, heading_every: usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    for k in 0..count {
        if k % heading_every == 0 {
            blocks.push(Block::Heading(format!(
                "Section {}",
                seed * 100 + k / heading_every + 1
            )));
        }
        blocks.push(paragraph(seed * 31 + k, k));
    }
    blocks
}

fn chapter(title: &str, blocks: Vec<Block>) -> Chapter {
    Chapter {
        title: title.to_string(),
        blocks,
    }
}

fn toc(title: &str, chapter: usize, children: Vec<TocItem>) -> TocItem {
    TocItem {
        title: title.to_string(),
        chapter,
        children,
    }
}

fn image(path: &str, kind: ImageKind, width: u16, height: u16, pattern: Pattern) -> ImageSpec {
    ImageSpec {
        path: path.to_string(),
        kind,
        width,
        height,
        pattern,
    }
}

// image indices used by `chapters`
const COVER: usize = 0;
const FIGURE_1BIT: usize = 1;
const FIGURE_PALETTE: usize = 2;
const PHOTO: usize = 3;

// the 1-bit, grey and palette PNGs and the JPEG each have their own size
fn images() -> Vec<ImageSpec> {
    vec![
        image(
            "images/cover.png",
            ImageKind::PngGray8,
            96,
            128,
            Pattern::HorizontalGradient,
        ),
        image(
            "images/figure1.png",
            ImageKind::PngGray1,
            128,
            64,
            Pattern::Checker { cell: 8 },
        ),
        image(
            "images/figure2.png",
            ImageKind::PngPalette8,
            80,
            48,
            Pattern::Checker { cell: 4 },
        ),
        image(
            "images/photo.jpg",
            ImageKind::Jpeg,
            96,
            64,
            Pattern::HorizontalGradient,
        ),
    ]
}

fn chapters() -> Vec<Chapter> {
    // the cover is also shown inline, at the top of the first chapter
    let mut road = vec![
        Block::Heading("Setting Out".to_string()),
        Block::Image(COVER),
    ];
    road.extend(body(1, 48, 12));

    let mut salt = body(2, 3, 3);
    salt.push(Block::Image(FIGURE_1BIT));
    salt.extend(body(3, 4, 4));
    salt.push(Block::Image(FIGURE_PALETTE));
    salt.push(paragraph(97, 3));

    let mut pictures = body(4, 2, 2);
    pictures.push(Block::Image(PHOTO));
    pictures.extend(body(5, 5, 5));

    vec![
        chapter("The Long Road", road),
        chapter("Salt & Pepper", salt),
        chapter("Pictures and Figures", pictures),
        chapter("Caf\u{e9} Days", body(6, 10, 5)),
        chapter("Last Words", body(7, 7, 7)),
    ]
}

// nested; the titles are free of & < > " (see the header comment)
fn toc_items() -> Vec<TocItem> {
    vec![
        toc(
            "Part One",
            0,
            vec![
                toc("Salt and Pepper", 1, vec![]),
                toc("Pictures and Figures", 2, vec![]),
            ],
        ),
        toc(
            "Part Two: Caf\u{e9} Days",
            3,
            vec![toc("Last Words", 4, vec![])],
        ),
    ]
}

fn epub_fixture(
    name: &'static str,
    version: EpubVersion,
    compression: Compression,
    label: &str,
) -> Fixture {
    let spec = EpubSpec {
        version,
        title: format!("Lantern Cove - {label}"),
        author: "Ren\u{e9}e Marlow".to_string(),
        identifier: format!(
            "{ID_PREFIX}{}",
            name.trim_end_matches(".EPU").to_lowercase()
        ),
        chapters: chapters(),
        toc: toc_items(),
        images: images(),
        cover: Some(COVER),
        compression,
        // both chapter encodings are exercised: raw UTF-8 and numeric references
        numeric_entities: compression == Compression::Deflate,
    };
    let bytes = build_epub(&spec).expect("standard EPUB spec is valid");
    Fixture {
        name,
        spec: Spec::Epub(spec),
        bytes,
    }
}

pub fn standard() -> Vec<Fixture> {
    use Compression::{Deflate, Mixed, Stored};
    use EpubVersion::{V2, V3};
    vec![
        txt_fixture(
            "PLAINLF.TXT",
            txt_lines("plain text, LF line endings.", 0, 24),
            Newline::Lf,
            true,
        ),
        txt_fixture(
            "PLAINCR.TXT",
            txt_lines("plain text, CRLF line endings.", 5, 24),
            Newline::CrLf,
            true,
        ),
        txt_fixture(
            "NOEOL.TXT",
            txt_lines("plain text, no final line ending.", 11, 24),
            Newline::Lf,
            false,
        ),
        txt_fixture("UTF8.TXT", utf8_lines(), Newline::Lf, true),
        txt_fixture(
            "TINY.TXT",
            vec![
                format!("{PROVENANCE} tiny."),
                "One short page of text.".to_string(),
            ],
            Newline::Lf,
            true,
        ),
        epub_fixture("E2STORED.EPU", V2, Stored, "EPUB 2, stored"),
        epub_fixture("E2DEFL.EPU", V2, Deflate, "EPUB 2, deflate"),
        epub_fixture("E2MIXED.EPU", V2, Mixed, "EPUB 2, mixed"),
        epub_fixture("E3STORED.EPU", V3, Stored, "EPUB 3, stored"),
        epub_fixture("E3DEFL.EPU", V3, Deflate, "EPUB 3, deflate"),
        epub_fixture("E3MIXED.EPU", V3, Mixed, "EPUB 3, mixed"),
    ]
}

// an in-memory card holding every standard fixture at the root, with `_PULP/`
// created the way the firmware boots it
pub fn standard_card() -> VirtualStorage {
    let fixtures = standard();
    let files: Vec<(&str, &[u8])> = fixtures
        .iter()
        .map(|f| (f.name, f.bytes.as_slice()))
        .collect();
    let card = VirtualStorage::memory_with(&files);
    card.ensure_pulp_dir().expect("fresh card has no _PULP yet");
    card
}

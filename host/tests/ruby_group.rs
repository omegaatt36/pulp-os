// Ruby groups of the stripped text: how a group is read out of a line and where its parts
// are placed. Pure functions of `fonts::ruby`; the reader's use of them is in `ruby_layout`.
use pulp_host::fonts::ruby::{self, Group, Read};
use pulp_host::fonts::{Style, StyleState};

const M: u8 = 1;

fn text(parts: &[&str]) -> Vec<u8> {
    // "{R}" style markers: {X} is [MARKER, X]
    let mut out = Vec::new();
    for part in parts {
        let bytes = part.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                out.extend_from_slice(&[M, bytes[i + 1]]);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    out
}

fn group(buf: &[u8], at: usize) -> Group<'_> {
    match ruby::read(buf, at) {
        Read::Group(g) => g,
        Read::Cut => panic!("cut"),
        Read::Stray => panic!("stray"),
    }
}

fn base_of(g: &Group<'_>) -> (String, Vec<Style>) {
    let mut s = String::new();
    let mut styles = Vec::new();
    g.each_base(StyleState::from_flags(0), |ch, sty| {
        s.push(ch);
        styles.push(sty);
    });
    (s, styles)
}

fn reading_of(g: &Group<'_>) -> String {
    let mut s = String::new();
    g.each_reading(|ch| s.push(ch));
    s
}

#[test]
fn a_group_is_read_from_its_open_marker_to_its_close_marker() {
    let buf = text(&["前{R}漢字{T}かんじ{r}後"]);
    let at = "前".len();
    let g = group(&buf, at);
    assert_eq!(base_of(&g).0, "漢字");
    assert_eq!(reading_of(&g), "かんじ");
    assert_eq!(g.len, 2 + "漢字".len() + 2 + "かんじ".len() + 2);
    assert_eq!(&buf[at + g.len..], "後".as_bytes());
    assert!(g.has_reading());
    assert_eq!(g.first(), '漢');
}

#[test]
fn a_group_without_a_reading_has_an_empty_one() {
    let buf = text(&["{R}漢{r}"]);
    let g = group(&buf, 0);
    assert_eq!(base_of(&g).0, "漢");
    assert_eq!(reading_of(&g), "");
    assert!(!g.has_reading());
}

#[test]
fn the_edges_of_a_reading_are_trimmed() {
    let buf = text(&["{R}漢{T} かん {r}"]);
    assert_eq!(reading_of(&group(&buf, 0)), "かん");
}

#[test]
fn the_style_markers_inside_a_base_style_its_scalars_and_carry_on_after_it() {
    let buf = text(&["{R}漢{b}字{T}かん{r}"]);
    let g = group(&buf, 0);
    // bold on before the group: the first scalar is bold, the second is not
    let bold = StyleState::from_flags(1);
    let mut seen = Vec::new();
    let after = g.each_base(bold, |ch, sty| seen.push((ch, sty)));
    assert_eq!(seen, [('漢', Style::Bold), ('字', Style::Regular)]);
    assert_eq!(after.style(), Style::Regular);
}

#[test]
fn a_window_that_ends_inside_a_group_is_cut() {
    let buf = text(&["{R}漢字{T}かんじ{r}"]);
    for len in 0..buf.len() {
        match ruby::read(&buf[..len], 0) {
            Read::Cut => {}
            Read::Stray if len < 2 => {}
            Read::Group(_) => panic!("a group out of {len} of {} bytes", buf.len()),
            Read::Stray => panic!("stray at {len}"),
        }
    }
    assert!(matches!(ruby::read(&buf, 0), Read::Group(_)));
}

#[test]
fn a_marker_that_starts_no_well_formed_group_is_stray() {
    let long = format!("{{R}}{}{{T}}かん{{r}}", "漢".repeat(45));
    let cases: [&str; 6] = [
        // a line break
        "{R}漢\n字{T}か{r}",
        // a block marker
        "{R}漢{Q}字{T}か{r}",
        // a style marker in a reading
        "{R}漢{T}か{B}ん{r}",
        // a second reading
        "{R}漢{T}か{T}ん{r}",
        // not an open marker
        "漢{T}か{r}",
        // longer than the stripper ever writes one
        &long,
    ];
    for case in cases {
        let buf = text(&[case]);
        assert!(
            matches!(ruby::read(&buf, 0), Read::Stray),
            "{:?}",
            String::from_utf8_lossy(&buf)
        );
    }
}

#[test]
fn the_longer_part_sets_the_length_and_the_shorter_is_centred_on_it() {
    let s = ruby::span(24, 52);
    assert_eq!((s.len, s.base_at, s.reading_at), (52, 14, 0));
    let s = ruby::span(48, 26);
    assert_eq!((s.len, s.base_at, s.reading_at), (48, 0, 11));
    let s = ruby::span(24, 0);
    assert_eq!((s.len, s.base_at, s.reading_at), (24, 0, 12));
    let s = ruby::span(24, 13);
    assert_eq!((s.len, s.base_at, s.reading_at), (24, 0, 5));
}

#[test]
fn a_reading_takes_its_size_and_a_gap_beside_the_em_box_of_the_base() {
    assert_eq!(ruby::room(12), 12 + ruby::GAP);
    assert_eq!(ruby::content(23, 12), 23 + 12 + ruby::GAP);
    assert_eq!(ruby::min_pitch(23, 12), ruby::content(23, 12) + 6);
    // a row: the reading's em box is the top of the line, its baseline 7/8 down
    assert_eq!(ruby::row_reading_baseline(12), 11);
}

#[test]
fn a_column_holds_the_base_and_the_reading_side_by_side_centred_in_its_pitch() {
    // odd and even sizes alike: a glyph box hangs from `center - px / 2`
    for (body, reading, pitch) in [(23u16, 12u16, 40u16), (28, 14, 45), (16, 10, 30)] {
        let content = i32::from(ruby::content(body, reading));
        let right = 500;
        let (base_c, reading_c) = ruby::column_centers(right, pitch, body, reading);
        let base_left = base_c - i32::from(body) / 2;
        let base_right = base_left + i32::from(body);
        let reading_left = reading_c - i32::from(reading) / 2;
        let reading_right = reading_left + i32::from(reading);
        // the reading is right of the base, a gap between the two em boxes
        assert_eq!(reading_left - base_right, i32::from(ruby::GAP), "{body}");
        // the content is centred in the column
        assert_eq!(reading_right - base_left, content, "{body}");
        assert_eq!(
            right - reading_right,
            (i32::from(pitch) - content) / 2,
            "{body}"
        );
    }
}

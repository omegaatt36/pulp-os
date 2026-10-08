// The production UTF-8 decoder (`Utf8Iter`, `decode_utf8_char`) against the replacement
// policy: a malformed sequence becomes one U+FFFD per maximal subpart, exactly as std's
// `String::from_utf8_lossy` does (the Unicode / WHATWG practice). Decoding always
// advances, never reads past the end, and a sequence cut off by the end of the buffer is
// one U+FFFD.
//
// Run: cargo test-host --test utf8_decoder
//
// Oracle: `String::from_utf8_lossy` for the text, and `<[u8]>::utf8_chunks` (the same
// std implementation of the rule, exposing the length of each maximal subpart) for how
// many bytes one call consumes. Inputs are the exhaustive set over a byte alphabet that
// holds every boundary of RFC 3629 (the lead bytes and the continuation ranges that
// separate overlong, surrogate and above-U+10FFFF forms) and a deterministic
// pseudo-random set. Nothing is taken from the code under test.

use pulp_host::utf8::{Utf8Iter, decode_utf8_char};

const FFFD: char = '\u{FFFD}';

// NUL, ASCII, DEL, continuation range edges, C0/C1 (overlong leads), C2/DF, E0/E1/E8/ED/EF,
// F0/F1/F4 (4-byte leads), F5 (above U+10FFFF), F8/FF (never in UTF-8), and the second-byte
// boundaries A0 (E0), 90 (F0), 8F (F0 / F4), 9F (ED)
const ALPHABET: [u8; 24] = [
    0x00, 0x41, 0x7F, 0x80, 0xBF, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xE8, 0xED, 0xEF, 0xF0, 0xF1,
    0xF4, 0xF5, 0xF8, 0xFF, 0xA0, 0x90, 0x8F, 0x9F,
];

fn collected(bytes: &[u8]) -> String {
    Utf8Iter::new(bytes).collect()
}

// every sequence of 1..=max_len bytes over the alphabet, in order
fn for_each_sequence(max_len: usize, mut f: impl FnMut(&[u8])) {
    let mut buf = Vec::with_capacity(max_len);
    fn go(buf: &mut Vec<u8>, max_len: usize, f: &mut dyn FnMut(&[u8])) {
        for &b in &ALPHABET {
            buf.push(b);
            f(buf);
            if buf.len() < max_len {
                go(buf, max_len, f);
            }
            buf.pop();
        }
    }
    go(&mut buf, max_len, &mut f);
}

// xorshift64*: deterministic, no dependency
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n
    }
}

// length 1..=64; bytes drawn half from the boundary alphabet, a quarter from valid
// scalars of every width, a quarter uniform
fn random_sequences(seed: u64, count: usize) -> Vec<Vec<u8>> {
    const VALID: [&str; 6] = ["a", "\u{e9}", "\u{81fa}", "\u{300c}", "\u{20bb7}", " "];
    let mut rng = Rng(seed);
    (0..count)
        .map(|_| {
            let len = 1 + rng.below(64);
            let mut v = Vec::new();
            while v.len() < len {
                match rng.below(4) {
                    0 | 1 => v.push(ALPHABET[rng.below(ALPHABET.len())]),
                    2 => v.extend_from_slice(VALID[rng.below(VALID.len())].as_bytes()),
                    _ => v.push(rng.below(256) as u8),
                }
            }
            v.truncate(len);
            v
        })
        .collect()
}

// what one decode_utf8_char call at the start of `bytes` must return: the first scalar of a
// valid prefix, else U+FFFD for the first maximal subpart, with the bytes it covers
fn expected_first(bytes: &[u8]) -> (char, usize) {
    let chunk = bytes.utf8_chunks().next().expect("non-empty input");
    match chunk.valid().chars().next() {
        Some(c) => (c, c.len_utf8()),
        None => (FFFD, chunk.invalid().len()),
    }
}

fn check_text(bytes: &[u8]) {
    assert_eq!(
        collected(bytes),
        String::from_utf8_lossy(bytes),
        "{bytes:02X?}"
    );
}

// decoding from every position of `bytes`: the call returns the first scalar or one
// U+FFFD per maximal subpart, consumes at least one byte and never more than remains; and
// the consumed lengths of a left-to-right pass add up to the input
fn check_progress(bytes: &[u8]) {
    for pos in 0..bytes.len() {
        let got = decode_utf8_char(bytes, pos);
        assert_eq!(got, expected_first(&bytes[pos..]), "{bytes:02X?} at {pos}");
        assert!(
            (1..=bytes.len() - pos).contains(&got.1),
            "{bytes:02X?} at {pos}: consumed {}",
            got.1
        );
    }
    let mut pos = 0;
    let mut steps = 0;
    let mut it = Utf8Iter::new(bytes);
    while pos < bytes.len() {
        let (_, n) = decode_utf8_char(bytes, pos);
        pos += n;
        steps += 1;
        assert!(it.next().is_some(), "{bytes:02X?}: Utf8Iter ended early");
        assert_eq!(it.position(), pos, "{bytes:02X?}: Utf8Iter position");
    }
    assert_eq!(
        pos,
        bytes.len(),
        "{bytes:02X?}: consumed lengths add up to the input"
    );
    assert!(steps <= bytes.len());
    assert_eq!(
        it.next(),
        None,
        "{bytes:02X?}: Utf8Iter is exhausted with the input"
    );
    assert!(it.remaining().is_empty());
}

#[test]
fn utf8_iter_equals_lossy_decoding_for_every_sequence_up_to_four_bytes_of_the_alphabet() {
    let mut count = 0usize;
    for_each_sequence(4, |s| {
        check_text(s);
        count += 1;
    });
    assert_eq!(count, 24 + 24 * 24 + 24 * 24 * 24 + 24 * 24 * 24 * 24);
}

#[test]
fn utf8_iter_equals_lossy_decoding_for_pseudo_random_sequences() {
    for s in random_sequences(0x9E3779B97F4A7C15, 40_000) {
        check_text(&s);
    }
}

#[test]
fn decoding_advances_over_the_whole_input_and_matches_the_maximal_subpart_at_every_position() {
    for_each_sequence(4, check_progress);
    for s in random_sequences(0xD1B54A32D192ED03, 20_000) {
        check_progress(&s);
    }
}

// the named cases of the policy, with their counts worked out by hand
#[test]
fn malformed_sequences_decode_to_the_hand_counted_number_of_replacements() {
    let cases: &[(&[u8], usize)] = &[
        (&[0x80], 1),
        (&[0xC0, 0x80], 2),
        (&[0xC1, 0xBF], 2),
        (&[0xE0, 0x80, 0x80], 3),
        (&[0xE0, 0x9F, 0xBF], 3),
        (&[0xF0, 0x80, 0x80, 0x80], 4),
        (&[0xF0, 0x8F, 0xBF, 0xBF], 4),
        (&[0xED, 0xA0, 0x80], 3),
        (&[0xF4, 0x90, 0x80, 0x80], 4),
        (&[0xF5, 0x80, 0x80, 0x80], 4),
        (&[0xF8], 1),
        (&[0xFF], 1),
        (&[0xF8, 0x88, 0x80, 0x80, 0x80], 5),
        // cut off, followed by a lead byte or ASCII: the valid prefix is one subpart
        (&[0xE8, 0x87, b'x'], 1),
        (&[0xF0, 0x9F, 0x98, b'x'], 1),
        (&[0xC3, 0xE8, 0x87, 0xBA], 1),
    ];
    for (bytes, want) in cases {
        let s = collected(bytes);
        assert_eq!(s.matches(FFFD).count(), *want, "{bytes:02X?}: {s:?}");
        assert_eq!(s, String::from_utf8_lossy(bytes), "{bytes:02X?}");
    }
}

// a sequence cut off by the end of the buffer: one U+FFFD covering the rest, text before
// it intact, whatever precedes it
#[test]
fn a_sequence_cut_off_at_the_end_of_the_buffer_is_one_replacement() {
    let mut cuts: Vec<Vec<u8>> = vec![vec![0xE8, 0x87], vec![0xF0, 0x9F, 0x98]];
    for c in ['\u{e9}', '\u{81fa}', '\u{20bb7}', '\u{2014}', '\u{1f600}'] {
        let mut b = [0u8; 4];
        let full = c.encode_utf8(&mut b).as_bytes().to_vec();
        for cut in 1..full.len() {
            cuts.push(full[..cut].to_vec());
        }
    }
    for tail in cuts {
        for prefix in ["", "a", "abc ", "\u{81fa}\u{7063}", "\u{20bb7}"] {
            let mut data = prefix.as_bytes().to_vec();
            data.extend_from_slice(&tail);
            let s = collected(&data);
            let label = format!("{prefix:?} + {tail:02X?}");
            assert_eq!(s, format!("{prefix}\u{FFFD}"), "{label}");
            let (c, n) = decode_utf8_char(&data, prefix.len());
            assert_eq!(
                (c, n),
                (FFFD, tail.len()),
                "{label}: one call covers the cut sequence"
            );
        }
    }
}

// valid text is never touched, whatever its widths
#[test]
fn valid_text_decodes_unchanged_next_to_malformed_bytes() {
    let valid = ["a", "\u{e9}", "\u{81fa}", "\u{20bb7}"];
    for before in valid {
        for after in valid {
            for bad in [
                &[0x80u8][..],
                &[0xC0, 0x80],
                &[0xE0, 0x80, 0x80],
                &[0xE8, 0x87],
                &[0xFF],
            ] {
                let mut data = before.as_bytes().to_vec();
                data.extend_from_slice(bad);
                data.extend_from_slice(after.as_bytes());
                let s = collected(&data);
                assert!(
                    s.starts_with(before) && s.ends_with(after),
                    "{before:?} {bad:02X?} {after:?}: {s:?}"
                );
                assert_eq!(
                    s,
                    String::from_utf8_lossy(&data),
                    "{before:?} {bad:02X?} {after:?}"
                );
            }
        }
    }
}

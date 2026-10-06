//! Every error kind must be diagnosable from its text alone.
use pulp_fontpack::PackError;

fn every_kind() -> Vec<(&'static str, PackError)> {
    vec![
        ("TooShort", PackError::TooShort),
        ("BadMagic", PackError::BadMagic),
        (
            "UnsupportedVersion",
            PackError::UnsupportedVersion { found: 2 },
        ),
        ("LengthMismatch", PackError::LengthMismatch),
        ("BadLayout", PackError::BadLayout),
        ("InvalidCodepoint", PackError::InvalidCodepoint { index: 1 }),
        ("CodepointOrder", PackError::CodepointOrder { index: 1 }),
        ("GlyphBitmapRange", PackError::GlyphBitmapRange { index: 1 }),
        (
            "GlyphSizeMismatch",
            PackError::GlyphSizeMismatch { index: 1 },
        ),
    ]
}

#[test]
fn every_error_kind_has_a_non_empty_display_text() {
    for (name, e) in every_kind() {
        assert!(
            !format!("{e}").is_empty(),
            "{name} has an empty Display text"
        );
    }
}

#[test]
fn every_pair_of_error_kinds_has_different_display_text() {
    let all = every_kind();
    for (i, (a_name, a)) in all.iter().enumerate() {
        for (b_name, b) in &all[i + 1..] {
            assert_ne!(
                format!("{a}"),
                format!("{b}"),
                "{a_name} and {b_name} share the same Display text"
            );
        }
    }
}

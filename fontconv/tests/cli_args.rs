//! Command line argument strictness: repeated options, options without a
//! value, and the `--upstream-url` value rule. All are usage errors (exit 2)
//! that leave no output.
mod common;

use common::*;
use pulp_fontconv::Input;

const OPTIONS: [&str; 6] = [
    "--font",
    "--license",
    "--upstream-url",
    "--out",
    "--sizes",
    "--require-chars",
];

/// A full, valid option set: (flag, value) in a fixed order.
fn full_set(tmp: &TempDir) -> Vec<(&'static str, std::ffi::OsString)> {
    let lic = tmp.write("license.txt", &license_bytes());
    let req = tmp.write("require.txt", b"A");
    vec![
        ("--font", bookerly_path().into_os_string()),
        ("--license", lic.into_os_string()),
        ("--upstream-url", UPSTREAM.into()),
        ("--out", tmp.join("out").into_os_string()),
        ("--sizes", "16".into()),
        ("--require-chars", req.into_os_string()),
    ]
}

fn args_from(pairs: &[(&str, std::ffi::OsString)]) -> Args {
    pairs.iter().fold(Args::empty(), |a, (f, v)| a.with(f, v))
}

#[test]
fn the_full_valid_option_set_succeeds() {
    // control for the tests below: all six options once each, valid values -> exit 0
    let tmp = TempDir::new();
    let r = args_from(&full_set(&tmp)).run();
    assert_eq!(r.code, 0, "{}", r.stderr);
}

#[test]
fn giving_any_option_twice_is_a_usage_error_with_no_output() {
    // each of the six options, repeated with the very same valid value, exits 2 and creates nothing
    for flag in OPTIONS {
        let tmp = TempDir::new();
        let set = full_set(&tmp);
        let dup = set.iter().find(|(f, _)| *f == flag).unwrap().clone();
        let r = args_from(&set).with(dup.0, &dup.1).run();
        assert_clean_failure(&r, 2, &format!("{flag} twice"));
        assert!(!tmp.join("out").exists(), "{flag} twice: no output");
    }
}

#[test]
fn repeating_an_option_with_a_different_valid_value_is_also_a_usage_error() {
    // no first-wins or last-wins: --sizes 16 then --sizes 23, and --out a then --out b, both exit 2 with nothing written
    let tmp = TempDir::new();
    let set = full_set(&tmp);
    let other_out = tmp.join("other-out");
    let r = args_from(&set).with("--sizes", "23").run();
    assert_clean_failure(&r, 2, "--sizes twice, different values");
    let r = args_from(&set).with("--out", &other_out).run();
    assert_clean_failure(&r, 2, "--out twice, different values");
    assert!(!tmp.join("out").exists() && !other_out.exists());
}

#[test]
fn a_trailing_option_without_a_value_is_a_usage_error_for_every_option() {
    // all other options valid and present; the flag under test is the final argument with no value
    for flag in OPTIONS {
        let tmp = TempDir::new();
        let set: Vec<_> = full_set(&tmp)
            .into_iter()
            .filter(|(f, _)| *f != flag)
            .collect();
        let r = args_from(&set).flag(flag).run();
        assert_clean_failure(&r, 2, &format!("trailing {flag}"));
        assert!(!tmp.join("out").exists(), "trailing {flag}: no output");
    }
}

#[test]
fn a_valueless_trailing_option_is_an_error_even_if_it_was_given_a_value_earlier() {
    // --x value ... --x (no value): must not be forgiven by the earlier value; exit 2, nothing written
    for flag in OPTIONS {
        let tmp = TempDir::new();
        let r = args_from(&full_set(&tmp)).flag(flag).run();
        assert_clean_failure(&r, 2, &format!("earlier value then trailing {flag}"));
        assert!(!tmp.join("out").exists(), "{flag}: no output");
    }
}

#[test]
fn upstream_url_with_surrounding_whitespace_is_a_usage_error_with_no_output() {
    // leading/trailing space, tab-led and newline-trailed values would break "no surrounding whitespace" in PROV.TXT
    for bad in [
        " https://example.invalid/f",
        "https://example.invalid/f ",
        "  https://example.invalid/f  ",
        " ",
        "\thttps://example.invalid/f",
        "https://example.invalid/f\n",
    ] {
        let tmp = TempDir::new();
        let mut set = full_set(&tmp);
        set[2].1 = bad.into();
        let r = args_from(&set).run();
        assert_clean_failure(&r, 2, &format!("url {bad:?}"));
        assert!(!tmp.join("out").exists(), "url {bad:?}: no output");
    }
}

#[test]
fn upstream_url_with_non_ascii_characters_is_a_usage_error_with_no_output() {
    // PROV.TXT is ASCII-only: accented, CJK, NBSP and an emoji in the value are all refused
    for bad in [
        "https://exämple.invalid/f",
        "https://例え.invalid/f",
        "https://example.invalid/臺灣",
        "https://example.invalid/f\u{A0}",
        "https://example.invalid/\u{1F600}",
    ] {
        let tmp = TempDir::new();
        let mut set = full_set(&tmp);
        set[2].1 = bad.into();
        let r = args_from(&set).run();
        assert_clean_failure(&r, 2, &format!("url {bad:?}"));
        assert!(!tmp.join("out").exists(), "url {bad:?}: no output");
    }
}

#[test]
fn ordinary_urls_are_accepted_and_written_verbatim_to_provenance() {
    // https urls with path, query, fragment and '=' / '&' characters pass and come back unchanged
    for url in [
        "https://github.com/ButTaiwan/iansui",
        "https://example.invalid/a/b.ttf?x=1&y=2#frag",
        "http://localhost:8080/",
    ] {
        let tmp = TempDir::new();
        let mut set = full_set(&tmp);
        set[2].1 = url.into();
        let r = args_from(&set).run();
        assert_eq!(r.code, 0, "{url}: {}", r.stderr);
        let prov = kv(&std::fs::read(tmp.join("out").join("PROV.TXT")).unwrap());
        assert_eq!(get(&prov, "font_upstream_url"), url);
    }
}

#[test]
fn library_applies_the_same_url_rule() {
    // convert refuses the values the CLI refuses and accepts ordinary ones
    let font = bookerly_bytes();
    let lic = license_bytes();
    let try_url = |url: &str| {
        pulp_fontconv::convert(&Input {
            font: &font,
            sizes: &[16],
            license: &lic,
            upstream_url: url,
            require_chars: None,
        })
    };
    assert!(try_url("https://github.com/ButTaiwan/iansui").is_ok());
    for bad in [
        "",
        " https://a.invalid",
        "https://a.invalid ",
        "https://exämple.invalid",
        "https://a.invalid/\u{3000}",
        "https://a.invalid/\n",
    ] {
        assert!(try_url(bad).is_err(), "{bad:?} must be rejected");
    }
}

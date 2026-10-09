//! Failure handling: every bad input exits non-zero with a message on stderr,
//! never panics, and leaves no output behind. Exit codes: 1 runtime failure,
//! 2 usage error.
mod common;

use common::*;
use pulp_fontconv::Input;

fn run_with(tmp: &TempDir, font: &std::path::Path) -> (Args, std::path::PathBuf) {
    let lic = tmp.write("license.txt", &license_bytes());
    let out = tmp.join("out");
    (Args::base(font, &lic, &out), out)
}

// ------------------------------------------------------------ bad fonts

#[test]
fn nonexistent_font_file_fails_with_exit_1_and_no_output() {
    // the path does not exist: runtime failure, out path never created
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, &tmp.join("no-such.ttf"));
    assert_clean_failure(&args.run(), 1, "missing font");
    assert!(!out.exists());
}

#[test]
fn files_that_are_not_fonts_fail_with_exit_1_and_no_output() {
    // text, empty, random bytes and a truncated real TTF are all unparsable
    let bookerly = bookerly_bytes();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("text", b"this is not a font\n".to_vec()),
        ("empty", Vec::new()),
        (
            "random",
            (0..4096u32)
                .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
                .collect(),
        ),
        ("truncated", bookerly[..1000].to_vec()),
    ];
    for (what, bytes) in cases {
        let tmp = TempDir::new();
        let font = tmp.write("bad.ttf", &bytes);
        let (args, out) = run_with(&tmp, &font);
        assert_clean_failure(&args.run(), 1, what);
        assert!(!out.exists(), "{what}: no output");
    }
}

#[test]
fn a_directory_given_as_the_font_fails_cleanly() {
    // reading a directory is an I/O error, not a panic
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, tmp.path());
    assert_clean_failure(&args.run(), 1, "directory as font");
    assert!(!out.exists());
}

#[test]
fn library_rejects_garbage_font_bytes_with_a_message_instead_of_panicking() {
    // convert returns Err with non-empty Display for unparsable fonts
    let bookerly = bookerly_bytes();
    let lic = license_bytes();
    for bytes in [
        &b""[..],
        &b"not a font"[..],
        &bookerly[..1000],
        &[0u8; 64][..],
    ] {
        let r = pulp_fontconv::convert(&Input {
            font: bytes,
            sizes: &[16],
            license: &lic,
            upstream_url: UPSTREAM,
            require_chars: None,
        });
        let e = r.err().expect("garbage font must be rejected");
        assert!(!e.to_string().trim().is_empty());
    }
}

// ----------------------------------------------------------------- sizes

#[test]
fn invalid_size_lists_are_usage_errors_and_leave_no_output() {
    // 0, 256, duplicates, non-numeric, signs, spaces, empty items, overflow, fractions: all exit 2
    let bad = [
        "0",
        "256",
        "1000",
        "16,16",
        "16,0",
        "abc",
        "16,",
        ",16",
        "16,,19",
        "",
        "-1",
        "+16",
        "16 23",
        "16;23",
        "1.5",
        "99999999999999999999",
        "16,abc",
        "0x10",
    ];
    for s in bad {
        let tmp = TempDir::new();
        let (args, out) = run_with(&tmp, &bookerly_path());
        let r = args.with("--sizes", s).run();
        assert_clean_failure(&r, 2, &format!("--sizes {s:?}"));
        assert!(!out.exists(), "--sizes {s:?}: no output");
    }
}

#[test]
fn smallest_and_largest_allowed_sizes_are_accepted() {
    // 1 and 255 are the inclusive bounds
    let cli = cli_run(&bookerly_path(), Some("1,255"), None);
    assert_eq!(cli.run.code, 0, "{}", cli.run.stderr);
    let names: Vec<String> = cli.files().into_keys().collect();
    assert_eq!(
        names,
        [
            "COVERAGE.TXT",
            "F00001.PFN",
            "F00255.PFN",
            "OFL.TXT",
            "PROV.TXT"
        ]
    );
}

#[test]
fn library_rejects_empty_zero_oversized_and_duplicate_sizes() {
    // same rules at the library boundary: Err with a message, no panic
    let font = bookerly_bytes();
    let lic = license_bytes();
    let try_sizes = |sizes: &[u16]| {
        pulp_fontconv::convert(&Input {
            font: &font,
            sizes,
            license: &lic,
            upstream_url: UPSTREAM,
            require_chars: None,
        })
    };
    for bad in [&[][..], &[0], &[256], &[65535], &[16, 16], &[16, 23, 16]] {
        let e = try_sizes(bad)
            .err()
            .unwrap_or_else(|| panic!("{bad:?} must be rejected"));
        assert!(!e.to_string().trim().is_empty());
    }
    assert!(try_sizes(&[1]).is_ok());
}

// ------------------------------------------------------- requirement file

#[test]
fn requirement_file_that_is_not_utf8_fails_with_exit_1_and_no_output() {
    // 0xFF 0xFE 0x80 is not valid UTF-8
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, &bookerly_path());
    let req = tmp.write("require.txt", &[b'A', 0xFF, 0xFE, 0x80]);
    let r = args.with("--require-chars", req).run();
    assert_clean_failure(&r, 1, "non-UTF-8 requirement file");
    assert!(!out.exists());
}

#[test]
fn nonexistent_requirement_file_fails_with_exit_1_and_no_output() {
    // the path does not exist
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, &bookerly_path());
    let r = args.with("--require-chars", tmp.join("nope.txt")).run();
    assert_clean_failure(&r, 1, "missing requirement file");
    assert!(!out.exists());
}

// ------------------------------------------------------ output directory

#[test]
fn output_directory_missing_with_parents_is_created() {
    // out = tmp/a/b/c does not exist: created including parents
    let tmp = TempDir::new();
    let lic = tmp.write("license.txt", &license_bytes());
    let out = tmp.join("a").join("b").join("c");
    let r = Args::base(&bookerly_path(), &lic, &out)
        .with("--sizes", "16")
        .run();
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(out.join("F00016.PFN").is_file());
}

#[test]
fn existing_empty_output_directory_is_used() {
    // an empty directory is a valid target
    let tmp = TempDir::new();
    let lic = tmp.write("license.txt", &license_bytes());
    let out = tmp.join("out");
    std::fs::create_dir(&out).unwrap();
    let r = Args::base(&bookerly_path(), &lic, &out)
        .with("--sizes", "16")
        .run();
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(read_dir_files(&out).len(), 4);
}

#[test]
fn non_empty_output_directory_is_refused_and_left_untouched() {
    // stale files must never be merged with a new run: exit 1, directory content unchanged
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, &bookerly_path());
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("STALE.TXT"), b"keep me").unwrap();
    let r = args.with("--sizes", "16").run();
    assert_clean_failure(&r, 1, "non-empty out dir");
    let files = read_dir_files(&out);
    assert_eq!(files.keys().collect::<Vec<_>>(), ["STALE.TXT"]);
    assert_eq!(files["STALE.TXT"], b"keep me");
}

#[test]
fn output_path_that_is_a_file_is_refused() {
    // out names an existing regular file: exit 1, file untouched
    let tmp = TempDir::new();
    let (args, _) = run_with(&tmp, &bookerly_path());
    let file = tmp.write("out", b"i am a file");
    let r = args.with("--sizes", "16").run();
    assert_clean_failure(&r, 1, "out is a file");
    assert_eq!(std::fs::read(&file).unwrap(), b"i am a file");
}

#[test]
fn output_directory_that_cannot_be_created_fails_cleanly() {
    // parent component is a regular file, so the directory cannot exist
    let tmp = TempDir::new();
    let lic = tmp.write("license.txt", &license_bytes());
    let blocker = tmp.write("blocker", b"x");
    let out = blocker.join("sub");
    let r = Args::base(&bookerly_path(), &lic, &out)
        .with("--sizes", "16")
        .run();
    assert_clean_failure(&r, 1, "uncreatable out dir");
}

// ------------------------------------------------------------ usage errors

#[test]
fn no_arguments_is_a_usage_error_with_a_message() {
    // nothing given: exit 2, usage on stderr
    let r = Args::empty().run();
    assert_clean_failure(&r, 2, "no args");
}

#[test]
fn each_required_option_is_enforced() {
    // dropping any one of the four required options is a usage error and creates nothing
    for flag in ["--font", "--license", "--upstream-url", "--out"] {
        let tmp = TempDir::new();
        let (args, out) = run_with(&tmp, &bookerly_path());
        let r = args.without(flag).run();
        assert_clean_failure(&r, 2, &format!("without {flag}"));
        assert!(!out.exists(), "without {flag}: no output");
    }
}

#[test]
fn unknown_option_and_missing_option_value_are_usage_errors() {
    // --bogus is unknown; a trailing --out with no value is incomplete
    let tmp = TempDir::new();
    let (args, out) = run_with(&tmp, &bookerly_path());
    let r = args.with("--bogus", "1").run();
    assert_clean_failure(&r, 2, "unknown option");
    assert!(!out.exists());

    let lic = tmp.write("l2.txt", &license_bytes());
    let r = Args::empty()
        .with("--font", bookerly_path())
        .with("--license", &lic)
        .with("--upstream-url", UPSTREAM)
        .flag("--out")
        .run();
    assert_clean_failure(&r, 2, "option without value");
}

#[test]
fn help_prints_usage_on_stdout_and_exits_0() {
    // --help and -h: exit 0, stdout names the tool and the required options
    for flag in ["--help", "-h"] {
        let r = Args::empty().flag(flag).run();
        assert_eq!(r.code, 0, "{flag}: {}", r.stderr);
        for needle in ["pulp-fontconv", "--font", "--license", "--out"] {
            assert!(r.stdout.contains(needle), "{flag}: stdout lacks {needle}");
        }
    }
}

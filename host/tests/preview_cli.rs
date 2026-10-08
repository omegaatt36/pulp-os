// Acceptance gates for the host-preview CLI: every gate drives the real binary
// and checks the PBM artifacts it writes, byte for byte.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pulp_host::fonts::FONT_SIZE_COUNT;
use pulp_host::kernel::config::NUM_READING_THEMES;

const RASTER_BYTES: usize = 480 * 800 / 8;

// one temp dir per test, removed on drop the way the shell script's EXIT trap did
struct WorkDir(PathBuf);

impl WorkDir {
    fn new(test: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("pulp-preview-check-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create work dir");
        Self(dir)
    }

    fn sub(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn preview(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_host-preview"))
        .args(args)
        .output()
        .expect("run host-preview")
}

fn log_of(out: &Output) -> String {
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    text
}

fn expect_run(args: &[&str]) {
    let out = preview(args);
    assert!(
        out.status.success(),
        "preview command {args:?} failed:\n{}",
        log_of(&out)
    );
}

fn expect_reject(args: &[&str]) {
    let out = preview(args);
    assert!(
        !out.status.success(),
        "invalid arguments accepted: {args:?}"
    );
    assert!(
        !out.stdout.is_empty() || !out.stderr.is_empty(),
        "invalid arguments lack diagnostic: {args:?}"
    );
}

fn pbm_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{dir:?}: {e}"))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pbm"))
        .collect();
    files.sort();
    files
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

// P4 header: three whitespace-separated tokens ('#' comment lines allowed),
// then one whitespace byte before the raster.
fn pbm_raster<'a>(path: &Path, data: &'a [u8]) -> &'a [u8] {
    let mut pos = 0;
    let mut tokens = Vec::new();
    while tokens.len() < 3 {
        while data.get(pos).copied().is_some_and(is_ws) {
            pos += 1;
        }
        if data.get(pos) == Some(&b'#') {
            pos += data[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(data.len() - pos, |i| i + 1);
            continue;
        }
        let start = pos;
        while data.get(pos).copied().is_some_and(|b| !is_ws(b)) {
            pos += 1;
        }
        assert!(pos > start, "{path:?}: header ends before three tokens");
        tokens.push(&data[start..pos]);
    }
    assert_eq!(
        tokens,
        [b"P4".as_slice(), b"480".as_slice(), b"800".as_slice()],
        "{path:?}: header tokens"
    );
    assert!(
        data.get(pos).copied().is_some_and(is_ws),
        "{path:?}: header terminator"
    );
    pos += if data[pos..].starts_with(b"\r\n") {
        2
    } else {
        1
    };
    let raster = &data[pos..];
    assert_eq!(
        raster.len(),
        RASTER_BYTES,
        "{path:?}: complete raster (file is {} bytes)",
        data.len()
    );
    raster
}

// a frame must carry both ink and paper (1 = black in PBM)
fn assert_not_blank(path: &Path, raster: &[u8]) {
    assert!(
        raster.iter().any(|&b| b != 0) && raster.iter().any(|&b| b != 0xFF),
        "{path:?}: blank frame"
    );
}

// every artifact in `dir` must be a complete, non-blank 480x800 P4 PBM
fn validate(dir: &Path, expected: usize) {
    let files = pbm_files(dir);
    assert_eq!(files.len(), expected, "{dir:?}: artifact count");
    for path in files {
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let raster = pbm_raster(&path, &data);
        assert_not_blank(&path, raster);
    }
}

// the two dirs hold the same PBM names with byte-identical contents
fn compare(a: &Path, b: &Path) {
    let read = |dir: &Path| -> BTreeMap<String, Vec<u8>> {
        pbm_files(dir)
            .into_iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned();
                let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                (name, data)
            })
            .collect()
    };
    let (ma, mb) = (read(a), read(b));
    assert!(!ma.is_empty(), "{a:?}: no PBMs");
    let only_a: Vec<&String> = ma.keys().filter(|name| !mb.contains_key(*name)).collect();
    let only_b: Vec<&String> = mb.keys().filter(|name| !ma.contains_key(*name)).collect();
    assert!(
        only_a.is_empty() && only_b.is_empty(),
        "PBM names differ: {only_a:?} only in {a:?}, {only_b:?} only in {b:?}"
    );
    for (name, ba) in &ma {
        let bb = &mb[name];
        assert_eq!(
            ba.len(),
            bb.len(),
            "{name}: byte length differs between {a:?} and {b:?}"
        );
        if let Some(offset) = ba.iter().zip(bb).position(|(x, y)| x != y) {
            panic!("{name}: bytes differ at offset {offset} between {a:?} and {b:?}");
        }
    }
}

// the one PBM of a single-fixture run, raster only (so a changed file name or
// header cannot mask identical pixels)
fn single_raster(dir: &Path) -> Vec<u8> {
    let files = pbm_files(dir);
    assert_eq!(files.len(), 1, "{dir:?}: artifact count");
    let path = &files[0];
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let raster = pbm_raster(path, &data).to_vec();
    assert_not_blank(path, &raster);
    raster
}

#[test]
fn help_documents_every_option() {
    let out = preview(&["--help"]);
    let text = log_of(&out);
    assert!(out.status.success(), "--help failed:\n{text}");
    for option in ["--output-dir", "--fixture", "--page", "--font", "--theme"] {
        assert!(text.contains(option), "help lacks {option}");
    }
}

#[test]
fn all_standard_fixtures_render_deterministic_pbms() {
    let work = WorkDir::new("all-fixtures");
    let a = work.sub("all-a");
    let b = work.sub("all-b");
    expect_run(&["--output-dir", a.to_str().unwrap()]);
    validate(&a, 11);
    expect_run(&["--output-dir", b.to_str().unwrap()]);
    validate(&b, 11);
    compare(&a, &b);
}

#[test]
fn defaults_are_applied() {
    let work = WorkDir::new("defaults");
    let default = work.sub("default");
    let explicit = work.sub("explicit");
    expect_run(&[
        "--output-dir",
        default.to_str().unwrap(),
        "--fixture",
        "PLAINLF.TXT",
    ]);
    expect_run(&[
        "--output-dir",
        explicit.to_str().unwrap(),
        "--fixture",
        "PLAINLF.TXT",
        "--page",
        "0",
        "--font",
        "2",
        "--theme",
        "1",
    ]);
    validate(&default, 1);
    validate(&explicit, 1);
    compare(&default, &explicit);
}

#[test]
fn each_option_changes_pixels() {
    let work = WorkDir::new("option-effect");
    let default = work.sub("default");
    expect_run(&[
        "--output-dir",
        default.to_str().unwrap(),
        "--fixture",
        "PLAINLF.TXT",
    ]);
    let base = single_raster(&default);
    for (flag, value) in [("--page", "1"), ("--font", "0"), ("--theme", "0")] {
        let dir = work.sub(&format!("effect-{}", &flag[2..]));
        expect_run(&[
            "--output-dir",
            dir.to_str().unwrap(),
            "--fixture",
            "PLAINLF.TXT",
            flag,
            value,
        ]);
        let changed = single_raster(&dir);
        assert_ne!(base, changed, "{flag} ignored: identical pixels");
    }
}

#[test]
fn selected_config_is_reproducible_per_fixture() {
    let work = WorkDir::new("selected-config");
    for fixture in ["PLAINLF.TXT", "E2STORED.EPU", "E3STORED.EPU"] {
        let a = work.sub(&format!("{fixture}-a"));
        let b = work.sub(&format!("{fixture}-b"));
        for dir in [&a, &b] {
            expect_run(&[
                "--output-dir",
                dir.to_str().unwrap(),
                "--fixture",
                fixture,
                "--page",
                "1",
                "--font",
                "0",
                "--theme",
                "0",
            ]);
        }
        validate(&a, 1);
        validate(&b, 1);
        compare(&a, &b);
    }
}

#[test]
fn cli_rejects_missing_and_unknown_arguments() {
    expect_reject(&[]);
    expect_reject(&["--unknown"]);
    expect_reject(&["--output-dir"]);
}

#[test]
fn cli_rejects_unknown_fixtures() {
    let work = WorkDir::new("reject-fixture");
    let bad = work.sub("bad");
    expect_reject(&[
        "--output-dir",
        bad.to_str().unwrap(),
        "--fixture",
        "UNKNOWN.TXT",
    ]);
}

#[test]
fn cli_rejects_malformed_option_values() {
    let work = WorkDir::new("reject-values");
    let bad = work.sub("bad");
    for flag in ["--page", "--font", "--theme"] {
        for value in ["-1", "abc"] {
            expect_reject(&[
                "--output-dir",
                bad.to_str().unwrap(),
                "--fixture",
                "PLAINLF.TXT",
                flag,
                value,
            ]);
        }
        expect_reject(&[
            "--output-dir",
            bad.to_str().unwrap(),
            "--fixture",
            "PLAINLF.TXT",
            flag,
        ]);
    }
}

#[test]
fn cli_rejects_a_page_beyond_the_book() {
    let work = WorkDir::new("reject-page-range");
    let bad = work.sub("bad");
    expect_reject(&[
        "--output-dir",
        bad.to_str().unwrap(),
        "--fixture",
        "TINY.TXT",
        "--page",
        "999999",
    ]);
}

#[test]
fn cli_rejects_font_and_theme_at_the_count_bound() {
    let work = WorkDir::new("reject-option-range");
    let bad = work.sub("bad");
    // the bounds are the production constants the CLI validates against
    expect_reject(&[
        "--output-dir",
        bad.to_str().unwrap(),
        "--fixture",
        "PLAINLF.TXT",
        "--font",
        &FONT_SIZE_COUNT.to_string(),
    ]);
    expect_reject(&[
        "--output-dir",
        bad.to_str().unwrap(),
        "--fixture",
        "PLAINLF.TXT",
        "--theme",
        &NUM_READING_THEMES.to_string(),
    ]);
}

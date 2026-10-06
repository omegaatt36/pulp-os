//! Shared test support for the converter.
//!
//! The oracle here is an independent small rasteriser: it asks `fontdue` for
//! coverage of the same font at the same size and packs bits itself, following
//! the written convention (coverage >= 100 is black, MSB-first, row-major,
//! offset_y = baseline to top row with y down, advance rounded half up). It
//! shares nothing with the converter implementation. Literal expectations are
//! hand-derived or come from other tools (`shasum`), never from converter output.
#![allow(dead_code, unused_macros)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use pulp_fontconv::{Input, Output};
use sha2::{Digest, Sha256};

/// Union of firmware body 16/19/23/28/35 and heading 23/27/32/38/46, deduplicated.
pub const DEFAULT_SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];

pub const UPSTREAM: &str = "https://example.invalid/upstream/font";
pub const IANSUI_UPSTREAM: &str = "https://github.com/ButTaiwan/iansui";

/// sha-256 of assets/fonts/Bookerly-Regular.ttf, from `shasum -a 256`.
pub const BOOKERLY_SHA256: &str =
    "5db64039fd7cfe1eca01fec76814aa2c6191bc627e53e2c67e8acc9e88bf8c73";
pub const BOOKERLY_LEN: u64 = 450_048;
/// sha-256 of the pinned Iansui-Regular.ttf, from `shasum -a 256`.
pub const IANSUI_SHA256: &str = "7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5";
pub const IANSUI_LEN: u64 = 9_447_424;

// ---------------------------------------------------------------- locations

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("fontconv has a parent directory")
        .to_path_buf()
}

pub fn bookerly_path() -> PathBuf {
    repo_root().join("assets/fonts/Bookerly-Regular.ttf")
}

pub fn bookerly_bytes() -> Vec<u8> {
    std::fs::read(bookerly_path()).expect("assets/fonts/Bookerly-Regular.ttf is part of the repo")
}

pub fn bookerly_italic_bytes() -> Vec<u8> {
    std::fs::read(repo_root().join("assets/fonts/Bookerly-Italic.ttf"))
        .expect("assets/fonts/Bookerly-Italic.ttf is part of the repo")
}

// ------------------------------------------------------- real font and SKIPs

static SKIPPED: AtomicUsize = AtomicUsize::new(0);

/// How many real-font tests skipped themselves in this test process.
pub fn skipped_count() -> usize {
    SKIPPED.load(Ordering::SeqCst)
}

/// Locate Iansui-Regular.ttf: `$IANSUI_TTF`, else `<repo>/Iansui-Regular.ttf`.
/// When absent, prints `SKIPPED(no-iansui-font): <test>` straight to the stderr
/// handle (the test harness cannot capture that, so it is visible in a normal
/// `cargo test` run) and returns None, unless `IANSUI_REQUIRED=1`, which panics.
pub fn iansui_path(test: &str) -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("IANSUI_TTF") {
        let p = PathBuf::from(v);
        assert!(p.is_file(), "IANSUI_TTF is set but {p:?} is not a file");
        return Some(p);
    }
    let p = repo_root().join("Iansui-Regular.ttf");
    if p.is_file() {
        return Some(p);
    }
    if std::env::var("IANSUI_REQUIRED").as_deref() == Ok("1") {
        panic!("IANSUI_REQUIRED=1 but no Iansui-Regular.ttf found (set IANSUI_TTF): {test}");
    }
    SKIPPED.fetch_add(1, Ordering::SeqCst);
    let _ = writeln!(std::io::stderr(), "SKIPPED(no-iansui-font): {test}");
    None
}

/// Import with `#[macro_use] mod common;`. `let path = require_iansui!();` at the top of a real-font test: returns the
/// font path, or prints the SKIPPED line and returns from the test.
macro_rules! require_iansui {
    () => {{
        fn here() {}
        let name = std::any::type_name_of_val(&here).trim_end_matches("::here");
        match $crate::common::iansui_path(name) {
            Some(p) => p,
            None => return,
        }
    }};
}

// ------------------------------------------------------------- temp + files

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pulp-fontconv-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        // canonical: macOS temp is behind a symlink, and the "no absolute path in
        // output" tests search for the path text
        TempDir(p.canonicalize().unwrap())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    pub fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A licence text with a BOM, CRLF and non-UTF-8 bytes, so any normalisation
/// (re-encoding, newline rewriting, trimming) would change it.
pub fn license_bytes() -> Vec<u8> {
    b"\xEF\xBB\xBFTest licence text\r\nline two  \r\n\x00\xFF binary tail\n".to_vec()
}

/// All regular files directly in `dir`, name -> bytes. Fails on sub directories.
pub fn read_dir_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        assert!(
            e.file_type().unwrap().is_file(),
            "unexpected non-file {e:?}"
        );
        m.insert(
            e.file_name().into_string().unwrap(),
            std::fs::read(e.path()).unwrap(),
        );
    }
    m
}

// ---------------------------------------------------------------- hashing

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let d = Sha256::digest(bytes);
    let mut a = [0u8; 32];
    a.copy_from_slice(&d);
    a
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&sha256(bytes))
}

// -------------------------------------------------------------------- CLI

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Argument list under construction; `run` executes the built binary.
pub struct Args {
    v: Vec<OsString>,
    cwd: Option<PathBuf>,
}

impl Args {
    pub fn empty() -> Args {
        Args {
            v: Vec::new(),
            cwd: None,
        }
    }
    /// The four required options with the default upstream URL.
    pub fn base(font: &Path, license: &Path, out: &Path) -> Args {
        Args::empty()
            .with("--font", font)
            .with("--license", license)
            .with("--upstream-url", UPSTREAM)
            .with("--out", out)
    }
    pub fn with(mut self, flag: &str, value: impl AsRef<OsStr>) -> Args {
        self.v.push(flag.into());
        self.v.push(value.as_ref().to_os_string());
        self
    }
    pub fn flag(mut self, flag: &str) -> Args {
        self.v.push(flag.into());
        self
    }
    /// Replace the value of an existing option (first occurrence).
    pub fn replace(mut self, flag: &str, value: impl AsRef<OsStr>) -> Args {
        let i = self
            .v
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("no {flag} in args"));
        self.v[i + 1] = value.as_ref().to_os_string();
        self
    }
    pub fn without(mut self, flag: &str) -> Args {
        let i = self
            .v
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("no {flag} in args"));
        self.v.drain(i..i + 2);
        self
    }
    pub fn cwd(mut self, dir: &Path) -> Args {
        self.cwd = Some(dir.to_path_buf());
        self
    }
    pub fn run(&self) -> Run {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pulp-fontconv"));
        cmd.args(&self.v);
        if let Some(d) = &self.cwd {
            cmd.current_dir(d);
        }
        let o = cmd.output().expect("spawn pulp-fontconv");
        Run {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
}

/// Assert the run failed cleanly with `code`: that exit code, a message on
/// stderr, and no panic.
pub fn assert_clean_failure(r: &Run, code: i32, what: &str) {
    assert_eq!(r.code, code, "{what}: exit code (stderr: {})", r.stderr);
    assert!(!r.stderr.trim().is_empty(), "{what}: stderr must explain");
    assert!(
        !r.stderr.contains("panicked"),
        "{what}: must not panic: {}",
        r.stderr
    );
}

// ------------------------------------------------------------ lib shortcuts

pub fn convert_with(font: &[u8], sizes: &[u16], require: Option<&str>) -> Output {
    let license = license_bytes();
    pulp_fontconv::convert(&Input {
        font,
        sizes,
        license: &license,
        upstream_url: UPSTREAM,
        require_chars: require,
    })
    .expect("convert must succeed")
}

pub fn pack_name(px: u16) -> String {
    format!("F{px:05}.PFN")
}

pub fn pack_bytes(out: &Output, px: u16) -> &[u8] {
    out.file(&pack_name(px))
        .unwrap_or_else(|| panic!("no pack for size {px}"))
}

// -------------------------------------------------------------- key=value

/// Parse a PROV/COVERAGE text file, asserting the shared text format: ASCII,
/// LF only, final newline, no blank lines, `key=value` with no surrounding space.
pub fn kv(bytes: &[u8]) -> Vec<(String, String)> {
    let text = std::str::from_utf8(bytes).expect("text file must be UTF-8");
    assert!(text.is_ascii(), "text file must be ASCII");
    assert!(text.ends_with('\n'), "text file must end with a newline");
    assert!(!text.contains('\r'), "text file must use LF only");
    let body = &text[..text.len() - 1];
    body.split('\n')
        .map(|line| {
            assert!(!line.is_empty(), "blank line in text file");
            let (k, v) = line
                .split_once('=')
                .unwrap_or_else(|| panic!("line without '=': {line:?}"));
            assert!(
                !k.is_empty() && k.trim() == k && v.trim() == v,
                "bad spacing in {line:?}"
            );
            (k.to_owned(), v.to_owned())
        })
        .collect()
}

pub fn keys(kv: &[(String, String)]) -> Vec<&str> {
    kv.iter().map(|(k, _)| k.as_str()).collect()
}

/// The single value of `key`; fails if absent or repeated.
pub fn get<'a>(kv: &'a [(String, String)], key: &str) -> &'a str {
    let all = all(kv, key);
    assert_eq!(all.len(), 1, "key {key} must appear exactly once: {all:?}");
    all[0]
}

pub fn all<'a>(kv: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    kv.iter()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// `U+` + upper-case hex, at least 4 digits.
pub fn u_plus(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

// ------------------------------------------------------------------ oracle

#[derive(Debug, PartialEq, Eq)]
pub struct OracleGlyph {
    pub advance: u16,
    pub offset_x: i16,
    pub offset_y: i16,
    pub width: u16,
    pub height: u16,
    pub bitmap: Vec<u8>,
}

pub fn load_font(bytes: &[u8]) -> fontdue::Font {
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("oracle font parse")
}

/// Independent rasteriser: coverage >= 100 is black; bit (7 - x%8) of byte
/// y*stride + x/8; padding bits stay 0.
pub fn oracle_glyph(font: &fontdue::Font, c: char, px: u16) -> OracleGlyph {
    let (m, cov) = font.rasterize(c, px as f32);
    let (w, h) = (m.width, m.height);
    let stride = w.div_ceil(8);
    let mut bitmap = vec![0u8; stride * h];
    for y in 0..h {
        for x in 0..w {
            if cov[y * w + x] >= 100 {
                bitmap[y * stride + x / 8] |= 0x80u8 >> (x % 8);
            }
        }
    }
    OracleGlyph {
        advance: (m.advance_width + 0.5).floor() as u16,
        offset_x: m.xmin as i16,
        // top row above the baseline is negative: -(ymin + height)
        offset_y: -(m.ymin + h as i32) as i16,
        width: w as u16,
        height: h as u16,
        bitmap,
    }
}

/// (line_height, ascent): ceil of fontdue's horizontal line metrics.
pub fn oracle_line_metrics(font: &fontdue::Font, px: u16) -> (u16, u16) {
    let lm = font
        .horizontal_line_metrics(px as f32)
        .expect("line metrics");
    (lm.new_line_size.ceil() as u16, lm.ascent.ceil() as u16)
}

/// The whole cmap, ascending.
pub fn oracle_cmap(font: &fontdue::Font) -> Vec<char> {
    let mut v: Vec<char> = font.chars().keys().copied().collect();
    v.sort();
    v
}

/// cmap minus control characters, ascending.
pub fn oracle_included(font: &fontdue::Font) -> Vec<char> {
    oracle_cmap(font)
        .into_iter()
        .filter(|c| !c.is_control())
        .collect()
}

/// Compare one pack glyph with the oracle, with a readable failure message.
pub fn assert_glyph_matches(c: char, px: u16, got: &pulp_fontpack::Glyph<'_>, want: &OracleGlyph) {
    let m = &got.metrics;
    let ctx = format!("U+{:04X} at {px}px", c as u32);
    assert_eq!(m.advance, want.advance, "{ctx}: advance");
    assert_eq!(m.offset_x, want.offset_x, "{ctx}: offset_x");
    assert_eq!(m.offset_y, want.offset_y, "{ctx}: offset_y");
    assert_eq!(m.width, want.width, "{ctx}: width");
    assert_eq!(m.height, want.height, "{ctx}: height");
    assert_eq!(got.bitmap, &want.bitmap[..], "{ctx}: bitmap bytes");
}

/// Check every char of `chars` in the pack against the oracle at `px`.
pub fn assert_pack_matches_oracle(
    pack: &pulp_fontpack::Pack<'_>,
    font: &fontdue::Font,
    px: u16,
    chars: &[char],
) {
    for &c in chars {
        let got = pack
            .find(c)
            .unwrap_or_else(|| panic!("U+{:04X} missing at {px}px", c as u32));
        assert_glyph_matches(c, px, &got, &oracle_glyph(font, c, px));
    }
}

// -------------------------------------------------------------- CLI fixtures

/// One CLI run of a font with the standard test licence: a temp dir holding
/// `license.txt` (and `require.txt` when given), output in `<tmp>/out`.
pub struct Cli {
    pub tmp: TempDir,
    pub out: PathBuf,
    pub run: Run,
}

impl Cli {
    pub fn files(&self) -> BTreeMap<String, Vec<u8>> {
        read_dir_files(&self.out)
    }
    pub fn file(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.out.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }
}

/// `sizes` is the raw `--sizes` value (None = converter default).
pub fn cli_run(font: &Path, sizes: Option<&str>, require: Option<&[u8]>) -> Cli {
    let tmp = TempDir::new();
    let license = tmp.write("license.txt", &license_bytes());
    let out = tmp.join("out");
    let mut a = Args::base(font, &license, &out);
    if let Some(s) = sizes {
        a = a.with("--sizes", s);
    }
    if let Some(r) = require {
        let req = tmp.write("require.txt", r);
        a = a.with("--require-chars", req);
    }
    let run = a.run();
    Cli { tmp, out, run }
}

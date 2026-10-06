# T2 contract: Iansui host converter (`pulp-fontconv`) and `pack_file_name`

Source of truth for the implementer. The tests in `fontconv/tests/**` and
`fontpack/tests/names.rs` pin everything below; where this file says "pinned" a test asserts it.

Crate: `/Users/raiven_kao/dev/pulp-os/fontconv` (package `pulp-fontconv`, lib `pulp_fontconv`, bin
`pulp-fontconv`), edition 2024, rust-version 1.95, workspace member, host-only (std).
Dependencies (no others): `pulp-fontpack` (path `../fontpack`, `features = ["builder"]`),
`fontdue = "0.9"`, `sha2 = { version = "0.11", default-features = false }` (the lock has sha2 0.11.0
only with default features off; default features would pull `const-oid`, a new lock entry).
Argument parsing is hand written on `std::env::args_os`.
The converter must not modify `build.rs` or any firmware file.

## 1. `pulp-fontpack` additions (additive, always available, no_std, no alloc)

```rust
pub const PACK_DIR: &str = "FONTS";                    // sub directory under _PULP on the SD card
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFileName { /* [u8; 10] inline buffer */ }
impl PackFileName { pub fn as_str(&self) -> &str }     // e.g. "F00016.PFN"
impl core::fmt::Display for PackFileName                // writes exactly as_str()
pub fn pack_file_name(pixel_size: u16) -> PackFileName
```

Naming rule (pinned): `"F" + pixel_size as exactly 5 zero-padded decimal digits + ".PFN"`, so
`16 -> F00016.PFN`, `255 -> F00255.PFN`, `65535 -> F65535.PFN`. Stem is 6 chars (<= 8), extension
3 chars, alphabet `A-Z0-9` (stem) and `.` separator: valid FAT 8.3 short name, injective over all of
`u16`. Install path on the card: `_PULP/FONTS/F00016.PFN` (`_PULP` is `PULP_DIR` in
`kernel/src/drivers/dir_entry.rs`; `FONTS` is 5 chars). Every file the converter writes is itself
8.3 (see section 4), so the output directory can be copied verbatim into `_PULP/FONTS/`.

## 2. Library API (`pulp_fontconv`), pure bytes in / bytes out, no filesystem

```rust
pub const CONVERTER_NAME: &str = "pulp-fontconv";
pub const CONVERTER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CONVENTION_VERSION: u32 = 1;            // rasterisation convention version (section 6)
pub const LICENSE_NAME: &str = "SIL OFL 1.1";
pub const DEFAULT_SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];
// union of firmware body 16/19/23/28/35 and heading 23/27/32/38/46, deduplicated, ascending

pub struct Input<'a> {
    pub font: &'a [u8],                 // TTF/OTF bytes
    pub sizes: &'a [u16],               // pixel sizes, any order, each 1..=255, no duplicates, non-empty
    pub license: &'a [u8],              // licence text, copied verbatim, must be non-empty
    pub upstream_url: &'a str,          // non-empty, ASCII only, no control characters, no leading/trailing whitespace (so one clean `key=value` line)
    pub require_chars: Option<&'a str>, // required character set source text
}
pub struct OutFile { pub name: String, pub bytes: Vec<u8> }
pub struct Output {
    pub files: Vec<OutFile>,   // ascending by `name` (byte order); exactly the files of section 4
    pub missing: Vec<char>,    // required chars not in the packs; ascending, distinct; empty if none/not asked
}
impl Output { pub fn file(&self, name: &str) -> Option<&[u8]> }

#[derive(Debug)] pub struct ConvError { .. }     // or enum; implements Display (non-empty) + std::error::Error
pub fn convert(input: &Input) -> Result<Output, ConvError>;
pub fn derive_font_id(font_sha256: &[u8; 32], pixel_size: u16, convention_version: u32) -> u64;
```

`convert` is deterministic and total: it never panics on any input (bad font bytes, 0 sizes, ...) and
returns `Err` instead. Everything in `Output` is a function of `(font, set of sizes, license,
upstream_url, require_chars)` only (no time, paths, host, map iteration order).

Which inputs affect which files (pinned by tests):
* pack files depend only on `font` and that pack's own size (not on other sizes, licence, url, require);
* `OFL.TXT` depends only on `license`; `PROV.TXT` on font, sizes, license, url; `COVERAGE.TXT` on font,
  sizes, require_chars.

## 3. Included / excluded characters and coverage definitions

`cmap` = the map `fontdue::Font::chars()` returns (Unicode scalar -> non-`.notdef` glyph). Its length is
`cmap_total`. (Surrogates cannot be `char`, so fontdue never reports them and they are not counted
anywhere; the pack format cannot hold them either.)

Excluded reasons (the only one): `control` = `char::is_control(c)` (U+0000..=U+001F, U+007F..=U+009F).
**Included set = cmap minus excluded.** Nothing outside the cmap is ever synthesised: no `.notdef`
substitute, no ASCII back-fill (deliberately unlike `build.rs`, which rasterises 0x20..=0x7E
regardless). Every pack has the same included set, in ascending codepoint order. Whitespace glyphs
(e.g. U+0020, U+3000) are included (blank glyph, `width == height == 0`, empty bitmap, advance kept).

A font whose included set is empty (no usable character, e.g. a truncated file that still parses) is an
error; the converter never emits a 0-glyph pack.

Required set (from `require_chars` text): every distinct scalar `c` for which
`char::is_whitespace(c)` is false; no BOM stripping, no normalisation. `missing` = required set minus
included set (so a control character that is in the cmap is *missing*: it is not in the packs).

## 4. Output directory layout (flat; all names are 8.3, upper case)

| file | content |
|------|---------|
| `F00016.PFN` ... | one pack per size, name = `pulp_fontpack::pack_file_name(size)` |
| `PROV.TXT` | provenance (section 5.1) |
| `OFL.TXT` | byte-for-byte copy of the licence input |
| `COVERAGE.TXT` | coverage report (section 5.2) |

Exactly those files, nothing else (no temp files left). Files are always written for every requested
size, ascending order of size.

## 5. Text file formats

Text file format (`PROV.TXT` and `COVERAGE.TXT`): UTF-8, ASCII only, LF line endings, one `key=value` per line,
key and value without surrounding spaces, no blank lines, no comments, ends with a final `\n`. The key
order below is exact. Keys may repeat only where stated.

### 5.1 `PROV.TXT` (key order exact; `<px>` = decimal pixel size, packs ascending)

```
format=pulp-fontconv-provenance
format_version=1
converter=pulp-fontconv
converter_version=<CARGO_PKG_VERSION>
convention_version=1
pack_format_version=1                       (pulp_fontpack::FORMAT_VERSION)
font_sha256=<64 lower-case hex>             sha-256 of the font input bytes
font_size=<decimal bytes>                   length of the font input
font_upstream_url=<upstream_url>            verbatim; the url rule of section 7 guarantees ASCII and no surrounding space
license_name=SIL OFL 1.1
license_file=OFL.TXT
license_sha256=<64 lower-case hex>          sha-256 of the licence input bytes
pack_count=<n>
pack.<px>.file=F000xx.PFN
pack.<px>.pixel_size=<px>
pack.<px>.font_id=<16 lower-case hex digits, {:016x} of the u64>
pack.<px>.glyph_count=<included set size>
pack.<px>.size=<pack file length in bytes>
pack.<px>.sha256=<64 lower-case hex of the pack file bytes>
```
(the six `pack.<px>.*` lines repeat per pack, ascending `<px>`.) No file names of the inputs, no output
directory, no paths, no time.

### 5.2 `COVERAGE.TXT`

```
format=pulp-fontconv-coverage
format_version=1
font_sha256=<64 hex>
cmap_total=<n>                              |cmap|
excluded_total=<n>
excluded.control=<n>
included_total=<n>                          cmap_total - excluded_total
excluded=U+0008:control                     one line per excluded cmap char, ascending (may repeat; zero lines if none)
pack.<px>.glyph_count=<n>                   per pack, ascending <px>; equals included_total
require_total=<n>                           |required set|        } only when require_chars is Some
require_missing=<n>                         |missing|             }
missing=U+2A6A5                             one line per missing char, ascending (may repeat; zero if none)
```
Codepoint notation: `U+` + upper-case hex, at least 4 digits, no further padding (`U+0008`, `U+81FA`,
`U+2A6A5`). The key sequence is exactly the order shown above (the `excluded=` lines come after `included_total`,
the `pack.*` lines after them, and the three `require`/`missing` groups only if required chars were
given). With no `--require-chars` there is no `require_*` and no `missing=` line.

## 6. Pack content and the rasterisation convention (version 1)

Convention = the existing firmware generator (`build.rs` `rasterize_char`/`emit_font`), with the 8-bit
clamps removed. For size `px` (pixel size as f32), `font = fontdue::Font::from_bytes(bytes,
FontSettings::default())`:

* `lm = font.horizontal_line_metrics(px)` (None -> error); `line_height = ceil(lm.new_line_size)`,
  `ascent = ceil(lm.ascent)`, both `as u16` (saturating cast). `pixel_size = px`.
* per included char `c`: `(m, cov) = font.rasterize(c, px)`;
  * `width = m.width`, `height = m.height` (u16, clamped to 65535);
  * pixel (x, y) is black iff `cov[y*width + x] >= 100` (`THRESHOLD`);
  * bitmap: 1 bpp, MSB-first, row-major, `stride = ceil(width/8)`, row padding bits are 0
    (so `bitmap.len() == stride*height`);
  * `offset_x = m.xmin` clamped to i16; `offset_y = -m.ymin - m.height` (baseline to top row, y down,
    negative = above baseline) clamped to i16;
  * `advance = (m.advance_width + 0.5) as u16` (round half up, saturating cast).
* header `font_id = derive_font_id(font_sha256, px, CONVENTION_VERSION)`.

`font_id` derivation (pinned by known-answer vectors):
`font_id = u64::from_le_bytes(SHA256(font_sha256 (32 raw bytes) || px as u16 LE || convention_version as
u32 LE)[0..8])`. Examples (font bytes `"abc"`, so `font_sha256 =
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`): `(px 16, conv 1) =
0x45f1f9b2a1197eef`, `(px 23, conv 1) = 0x995891a073f4548f`, `(px 16, conv 2) = 0xff2e79c166899647`.
Any change of font bytes, pixel size or convention version changes it.

Every produced pack passes `Pack::parse`, and `Pack::find(c)` hits for every included char and misses
for everything else.

## 7. CLI

```
pulp-fontconv --font <ttf> --license <file> --upstream-url <url> --out <dir>
              [--sizes <n,n,...>] [--require-chars <file>]
pulp-fontconv --help | -h
```
Only the `--name value` form (no `--name=value`). Required: `--font --license --upstream-url --out`.
`--sizes`: comma separated decimal integers, ASCII digits only (no sign, spaces, empty items), each
1..=255, no duplicates; order is irrelevant (output is the same for any order); default =
`DEFAULT_SIZES`. `--require-chars <file>`: UTF-8 file, interpreted per section 3. `--help`/`-h`: usage
on stdout (contains `pulp-fontconv`), exit 0.
Every option takes exactly one value: giving an option twice (even with the same value) is a usage
error, and an option that is the last argument with no value is a usage error (also when the same
option was already given earlier with a value).
`--upstream-url` rule (also enforced by the library `convert`): non-empty; every char is ASCII and not
a control character (so no non-ASCII text, no tab/newline/NUL); no leading or trailing whitespace
(space included). Spaces inside the value are not constrained (unspecified, untested). The rule keeps
`PROV.TXT` ASCII-only with values that have no surrounding whitespace.

Order of work: parse args, read all inputs, convert completely **in memory**, and only then touch the
filesystem. Consequently any failure before the write leaves no output at all: the `--out` path is not
created (pinned: no pack file and, if it did not exist, no directory).

Output directory policy: missing -> created including parents; existing empty directory -> used;
existing non-empty directory, or a path that is a file, or not creatable -> failure, nothing written
or modified (never merges into a stale directory).

Exit codes (pinned):

| code | meaning |
|-----:|---------|
| 0 | success; no required chars, or all required chars present |
| 1 | runtime failure: unreadable/nonexistent input file, font not parsable, empty licence file, `--require-chars` file not valid UTF-8, output directory problem, write error |
| 2 | usage error: unknown or repeated option, missing required option, missing option value, bad `--sizes`, bad `--upstream-url` value (empty, non-ASCII, contains a control character, or has leading/trailing whitespace) |
| 3 | conversion succeeded and **all files were written**, but at least one required char is missing; stderr contains the word `missing` |

All error messages go to stderr (non-empty), never a panic (exit 101 / "panicked" in stderr is a
failure); stdout on success is a free-form human summary (unpinned).

## 8. Test knobs

* `IANSUI_TTF=<path>`: real-font tests use it; unset -> `<repo>/Iansui-Regular.ttf`
  (`scripts/host-test.sh` exports it when the file exists).
* Real-font tests, when no font is found, write the line `SKIPPED(no-iansui-font): <test name>` to
  stderr (direct handle write, so `cargo test` output capture does not hide it) and return. A test
  asserts, when the font is present, that the helper returns it and the skip counter stays 0.
  `IANSUI_REQUIRED=1` turns a missing font into a test failure instead of a skip.

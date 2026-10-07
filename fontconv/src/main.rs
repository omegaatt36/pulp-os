// pulp-fontconv command line: argument parsing, file reading and writing, exit
// codes. All conversion work lives in the library and finishes in memory
// before anything is written.
//
// exit codes: 0 ok, 1 runtime failure, 2 usage error, 3 written but required
// characters are missing.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pulp_fontconv::{DEFAULT_SIZES, Input, Output};

const USAGE: &str = "\
usage: pulp-fontconv --font <ttf> --license <file> --upstream-url <url> --out <dir>
                     [--sizes <n,n,...>] [--require-chars <file>] [--license-name <name>]
       pulp-fontconv bundle [--manifest <json>] [--out <dir>] [--cache <dir>]
       pulp-fontconv pbm-to-png <dir>
       pulp-fontconv --help

Converts a TTF/OTF font into SD font packs (one F000nn.PFN per pixel size) and
writes PROV.TXT, a license copy and COVERAGE.TXT next to them. The output directory
must not exist or be empty; copy its content to _PULP/FONTS on the SD card.

Subcommands:
  bundle                   build verified font bundle from manifest with cache
  pbm-to-png <dir>         convert binary P4 PBM snapshots in <dir> to grayscale PNG

Options:
  --font <ttf>             font file
  --license <file>         licence text, copied verbatim
  --license-name <name>    default SIL OFL 1.1 (OFL.TXT); others use LICENSE.TXT
  --upstream-url <url>     where the font comes from (recorded in PROV.TXT)
  --out <dir>              output directory
  --sizes <n,n,...>        pixel sizes, each 1..=255 (default 16,19,23,27,28,32,35,38,46)
  --require-chars <file>   UTF-8 text whose non-whitespace characters must be covered

exit codes: 0 ok, 1 failure, 2 usage error, 3 written, but required characters missing
";

struct Args {
    font: PathBuf,
    license: PathBuf,
    license_name: String,
    upstream_url: String,
    out: PathBuf,
    sizes: Vec<u16>,
    require_chars: Option<PathBuf>,
}

enum Parsed {
    Help,
    Bundle {
        manifest: PathBuf,
        out: PathBuf,
        cache: PathBuf,
    },
    PbmToPng {
        dir: PathBuf,
    },
    Run(Args),
}

enum Failure {
    Usage(String),
    Runtime(String),
}

fn usage<T>(msg: impl Into<String>) -> Result<T, Failure> {
    Err(Failure::Usage(msg.into()))
}

fn runtime<E: std::fmt::Display>(what: &str, path: &Path) -> impl FnOnce(E) -> Failure {
    let ctx = format!("{what} {}", path.display());
    move |e| Failure::Runtime(format!("{ctx}: {e}"))
}

fn parse_sizes(text: &str) -> Result<Vec<u16>, Failure> {
    let mut sizes = Vec::new();
    for item in text.split(',') {
        if item.is_empty() || !item.bytes().all(|b| b.is_ascii_digit()) {
            return usage(format!("--sizes: bad item {item:?}"));
        }
        match item.parse::<u16>() {
            Ok(n) => sizes.push(n),
            Err(_) => return usage(format!("--sizes: {item} is out of range")),
        }
    }
    pulp_fontconv::validate_sizes(&sizes).map_err(|e| Failure::Usage(e.to_string()))?;
    Ok(sizes)
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, Failure> {
    let mut it = args.into_iter();
    let first = match it.next() {
        Some(f) => f,
        None => return usage("missing required options"),
    };

    let first_str = first.to_string_lossy();
    if first_str == "bundle" {
        let mut manifest = PathBuf::from("fonts/cjk.json");
        let mut out = PathBuf::from("target/cjk-sd");
        let mut cache = PathBuf::from("target/cjk-cache");

        while let Some(arg) = it.next() {
            let name = arg.to_string_lossy().into_owned();
            match name.as_str() {
                "--manifest" => {
                    let v = it
                        .next()
                        .ok_or_else(|| Failure::Usage("--manifest needs value".into()))?;
                    manifest = PathBuf::from(v);
                }
                "--out" => {
                    let v = it
                        .next()
                        .ok_or_else(|| Failure::Usage("--out needs value".into()))?;
                    out = PathBuf::from(v);
                }
                "--cache" => {
                    let v = it
                        .next()
                        .ok_or_else(|| Failure::Usage("--cache needs value".into()))?;
                    cache = PathBuf::from(v);
                }
                "--help" | "-h" => return Ok(Parsed::Help),
                other => return usage(format!("unknown bundle option {other:?}")),
            }
        }
        return Ok(Parsed::Bundle {
            manifest,
            out,
            cache,
        });
    }

    if first_str == "pbm-to-png" {
        let dir = it
            .next()
            .ok_or_else(|| Failure::Usage("pbm-to-png requires directory path".into()))?;
        return Ok(Parsed::PbmToPng {
            dir: PathBuf::from(dir),
        });
    }

    let mut full_it = std::iter::once(first).chain(it);
    let (mut font, mut license, mut url, mut out) = (None, None, None, None);
    let (mut sizes, mut require, mut license_name) = (None, None, None);
    while let Some(flag) = full_it.next() {
        let name = flag.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Parsed::Help);
        }
        let slot = match name.as_str() {
            "--font" | "--license" | "--license-name" | "--upstream-url" | "--out" | "--sizes"
            | "--require-chars" => name.as_str(),
            _ => return usage(format!("unknown option {name:?}")),
        };
        let Some(value) = full_it.next() else {
            return usage(format!("{slot} needs a value"));
        };
        let taken = match slot {
            "--font" => font.replace(value).is_some(),
            "--license" => license.replace(value).is_some(),
            "--license-name" => license_name.replace(value).is_some(),
            "--upstream-url" => url.replace(value).is_some(),
            "--out" => out.replace(value).is_some(),
            "--sizes" => sizes.replace(value).is_some(),
            _ => require.replace(value).is_some(),
        };
        if taken {
            return usage(format!("{slot} given more than once"));
        }
    }

    let need = |v: Option<OsString>, flag: &str| {
        v.map_or_else(|| usage(format!("missing required option {flag}")), Ok)
    };
    let text = |v: OsString, flag: &str| {
        v.into_string()
            .or_else(|_| usage(format!("{flag}: value is not valid UTF-8")))
    };
    let upstream_url = text(need(url, "--upstream-url")?, "--upstream-url")?;
    pulp_fontconv::validate_upstream_url(&upstream_url)
        .map_err(|e| Failure::Usage(e.to_string()))?;
    let sizes = match sizes {
        Some(v) => parse_sizes(&text(v, "--sizes")?)?,
        None => DEFAULT_SIZES.to_vec(),
    };
    let license_name = match license_name {
        Some(v) => text(v, "--license-name")?,
        None => pulp_fontconv::LICENSE_NAME.into(),
    };
    pulp_fontconv::validate_license_name(&license_name)
        .map_err(|e| Failure::Usage(e.to_string()))?;
    Ok(Parsed::Run(Args {
        font: need(font, "--font")?.into(),
        license: need(license, "--license")?.into(),
        license_name,
        upstream_url,
        out: need(out, "--out")?.into(),
        sizes,
        require_chars: require.map(PathBuf::from),
    }))
}

// a missing directory is created, an empty one is used, anything else is refused
fn prepare_out_dir(out: &Path) -> Result<(), Failure> {
    match fs::metadata(out) {
        Ok(meta) if meta.is_dir() => {
            let mut entries = fs::read_dir(out).map_err(runtime("cannot read", out))?;
            if entries.next().is_some() {
                return Err(Failure::Runtime(format!(
                    "output directory {} is not empty",
                    out.display()
                )));
            }
            Ok(())
        }
        Ok(_) => Err(Failure::Runtime(format!(
            "output path {} exists and is not a directory",
            out.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(out).map_err(runtime("cannot create", out))
        }
        Err(e) => Err(runtime("cannot inspect", out)(e)),
    }
}

// a half written directory must not pass for a complete pack set
fn write_files(out: &Path, output: &Output) -> Result<(), Failure> {
    let mut written = Vec::new();
    for f in &output.files {
        let path = out.join(&f.name);
        written.push(path.clone());
        if let Err(e) = fs::write(&path, &f.bytes) {
            for p in &written {
                let _ = fs::remove_file(p);
            }
            return Err(runtime("cannot write", &path)(e));
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<ExitCode, Failure> {
    let font = fs::read(&args.font).map_err(runtime("cannot read", &args.font))?;
    let license = fs::read(&args.license).map_err(runtime("cannot read", &args.license))?;
    let require = match &args.require_chars {
        Some(p) => Some(fs::read_to_string(p).map_err(runtime("cannot read", p))?),
        None => None,
    };

    let output = pulp_fontconv::convert_with_options(
        &Input {
            font: &font,
            sizes: &args.sizes,
            license: &license,
            upstream_url: &args.upstream_url,
            require_chars: require.as_deref(),
        },
        &pulp_fontconv::ConversionOptions {
            license_name: &args.license_name,
        },
    )
    .map_err(|e| Failure::Runtime(e.to_string()))?;

    prepare_out_dir(&args.out)?;
    write_files(&args.out, &output)?;

    println!(
        "wrote {} files to {}",
        output.files.len(),
        args.out.display()
    );
    if output.missing.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    // the full list is in COVERAGE.TXT
    let shown: Vec<String> = output
        .missing
        .iter()
        .take(20)
        .map(|c| format!("U+{:04X}", u32::from(*c)))
        .collect();
    eprintln!(
        "{} required characters missing from the font (see COVERAGE.TXT), first: {}",
        output.missing.len(),
        shown.join(" ")
    );
    Ok(ExitCode::from(3))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn make_png_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    chunk.extend_from_slice(tag);
    chunk.extend_from_slice(payload);
    let mut crc_data = Vec::new();
    crc_data.extend_from_slice(tag);
    crc_data.extend_from_slice(payload);
    let crc = crc32(&crc_data);
    chunk.extend_from_slice(&crc.to_be_bytes());
    chunk
}

// Convert uncompressed scanlines to standard DEFLATE uncompressed blocks
fn deflate_uncompressed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // zlib header: CMF=0x78 (deflate, 32K window), FLG=0x01 (check bits)
    out.extend_from_slice(&[0x78, 0x01]);
    let mut offset = 0;
    while offset < data.len() {
        let chunk_len = (data.len() - offset).min(65535);
        let is_last = (offset + chunk_len) == data.len();
        out.push(if is_last { 0x01 } else { 0x00 });
        let len_u16 = chunk_len as u16;
        let nlen_u16 = !len_u16;
        out.extend_from_slice(&len_u16.to_le_bytes());
        out.extend_from_slice(&nlen_u16.to_le_bytes());
        out.extend_from_slice(&data[offset..offset + chunk_len]);
        offset += chunk_len;
    }
    // Adler32 checksum
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let adler = (s2 << 16) | s1;
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn convert_pbm_to_png(dir: &Path) -> Result<(), Failure> {
    let entries = fs::read_dir(dir).map_err(runtime("read dir", dir))?;
    for entry in entries {
        let entry = entry.map_err(runtime("dir entry", dir))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "pbm") {
            let bytes = fs::read(&path).map_err(runtime("read file", &path))?;
            // parse P4\n<width> <height>\n<data>
            let mut iter = bytes.splitn(3, |&b| b == b'\n');
            let magic = iter
                .next()
                .ok_or_else(|| Failure::Runtime("truncated pbm".into()))?;
            if magic != b"P4" {
                return Err(Failure::Runtime(format!(
                    "unsupported pbm magic: {:?}",
                    magic
                )));
            }
            let dims = iter
                .next()
                .ok_or_else(|| Failure::Runtime("truncated pbm dims".into()))?;
            let dim_str = std::str::from_utf8(dims).map_err(|e| Failure::Runtime(e.to_string()))?;
            let mut parts = dim_str.split_whitespace();
            let width: usize = parts
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| Failure::Runtime("bad width".into()))?;
            let height: usize = parts
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| Failure::Runtime("bad height".into()))?;
            let data = iter
                .next()
                .ok_or_else(|| Failure::Runtime("truncated pbm data".into()))?;

            let stride = (width + 7) / 8;
            if data.len() != stride * height {
                return Err(Failure::Runtime(format!(
                    "pbm data size mismatch: {} vs {}",
                    data.len(),
                    stride * height
                )));
            }

            let mut scanlines = Vec::with_capacity(height * (width + 1));
            for y in 0..height {
                scanlines.push(0u8); // filter type None
                for x in 0..width {
                    let byte = data[y * stride + (x / 8)];
                    let bit = (byte & (128 >> (x % 8))) != 0;
                    scanlines.push(if bit { 0u8 } else { 255u8 });
                }
            }

            let mut png = Vec::new();
            png.extend_from_slice(b"\x89PNG\r\n\x1a\n");

            let mut ihdr = Vec::new();
            ihdr.extend_from_slice(&(width as u32).to_be_bytes());
            ihdr.extend_from_slice(&(height as u32).to_be_bytes());
            ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
            png.extend_from_slice(&make_png_chunk(b"IHDR", &ihdr));

            let idat = deflate_uncompressed(&scanlines);
            png.extend_from_slice(&make_png_chunk(b"IDAT", &idat));

            png.extend_from_slice(&make_png_chunk(b"IEND", &[]));

            let out_png = path.with_extension("png");
            fs::write(&out_png, &png).map_err(runtime("write png", &out_png))?;
            println!("converted {} -> {}", path.display(), out_png.display());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let result = match parse_args(std::env::args_os().skip(1)) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Bundle {
            manifest,
            out,
            cache,
        }) => match pulp_fontconv::bundle::build_bundle(&manifest, &out, &cache) {
            Ok(_) => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("bundle build failed: {e}");
                return ExitCode::from(1);
            }
        },
        Ok(Parsed::PbmToPng { dir }) => match convert_pbm_to_png(&dir) {
            Ok(_) => return ExitCode::SUCCESS,
            Err(e) => {
                match e {
                    Failure::Usage(m) => eprintln!("pulp-fontconv: {m}"),
                    Failure::Runtime(m) => eprintln!("pulp-fontconv: {m}"),
                }
                return ExitCode::from(1);
            }
        },
        Ok(Parsed::Run(args)) => run(args),
        Err(f) => Err(f),
    };
    match result {
        Ok(code) => code,
        Err(Failure::Usage(msg)) => {
            eprintln!("pulp-fontconv: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Runtime(msg)) => {
            eprintln!("pulp-fontconv: {msg}");
            ExitCode::from(1)
        }
    }
}

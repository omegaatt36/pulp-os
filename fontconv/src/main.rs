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
       pulp-fontconv --help

Converts a TTF/OTF font into SD font packs (one F000nn.PFN per pixel size) and
writes PROV.TXT, a license copy and COVERAGE.TXT next to them. The output directory
must not exist or be empty; copy its content to _PULP/FONTS on the SD card.

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
    let (mut font, mut license, mut url, mut out) = (None, None, None, None);
    let (mut sizes, mut require, mut license_name) = (None, None, None);
    let mut it = args.into_iter();
    while let Some(flag) = it.next() {
        let name = flag.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Parsed::Help);
        }
        let slot = match name.as_str() {
            "--font" | "--license" | "--license-name" | "--upstream-url" | "--out" | "--sizes"
            | "--require-chars" => name.as_str(),
            _ => return usage(format!("unknown option {name:?}")),
        };
        let Some(value) = it.next() else {
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

fn main() -> ExitCode {
    let result = match parse_args(std::env::args_os().skip(1)) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
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

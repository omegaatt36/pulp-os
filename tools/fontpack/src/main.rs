//! pulp-fontpack: build a `.PFP` font pack plus its license sidecar and a
//! build manifest from a TTF (docs/font-pack.txt). The pack is self-verified
//! with the device loader before anything is written. Reported write errors
//! roll back previous outputs; after a forced kill during the three-file
//! publication, rerun the converter before copying files to SD.
//!
//! cargo run -p pulp-fontpack --target host-tuple --release -- \
//!     --ttf Iansui-Regular.ttf --license OFL.txt --size 24 --family IANSUI --out out/FONTS/ \
//!     [--expect-sha256 HEX]

use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pulp_fontpack::{convert, coverage, names};
use sha2::{Digest, Sha256};

const USAGE: &str = "usage: pulp-fontpack --ttf FONT.ttf --license OFL.txt --size PX --family NAME --out DIR [--fallback U+25A1] [--expect-sha256 HEX]";
const DEFAULT_FALLBACK: u32 = 0x25A1;

struct Args {
    ttf: PathBuf,
    license: PathBuf,
    size: u16,
    family: String,
    out: PathBuf,
    fallback: u32,
    /// Required SHA-256 of the TTF, lowercase hex.
    expect_sha256: Option<String>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut ttf = None;
    let mut license = None;
    let mut size = None;
    let mut family = None;
    let mut out = None;
    let mut fallback = DEFAULT_FALLBACK;
    let mut expect_sha256 = None;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--ttf" => ttf = Some(PathBuf::from(value()?)),
            "--license" => license = Some(PathBuf::from(value()?)),
            "--size" => {
                let v = value()?;
                size = Some(v.parse().map_err(|_| format!("bad --size {v:?}"))?);
            }
            "--family" => family = Some(value()?),
            "--out" => {
                let v = value()?;
                if v.is_empty() {
                    return Err("--out must name a directory".to_owned());
                }
                out = Some(PathBuf::from(v));
            }
            "--fallback" => {
                let v = value()?;
                let hex = v.trim_start_matches("U+").trim_start_matches("u+");
                fallback =
                    u32::from_str_radix(hex, 16).map_err(|_| format!("bad --fallback {v:?}"))?;
            }
            "--expect-sha256" => {
                let v = value()?.to_ascii_lowercase();
                if v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(format!("bad --expect-sha256 {v:?} (64 hex digits)"));
                }
                expect_sha256 = Some(v);
            }
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let missing =
        |name: &str| format!("missing {name} (the license is mandatory, docs/font-pack.txt §8)");
    Ok(Some(Args {
        ttf: ttf.ok_or_else(|| missing("--ttf"))?,
        license: license.ok_or_else(|| missing("--license"))?,
        size: size.ok_or_else(|| missing("--size"))?,
        family: family.ok_or_else(|| missing("--family"))?,
        out: out.ok_or_else(|| missing("--out"))?,
        fallback,
        expect_sha256,
    }))
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

fn run(args: &Args) -> Result<String, Box<dyn std::error::Error>> {
    let pack_name = names::pack_file_name(&args.family, args.size)?;
    let license_name = names::license_file_name(&args.family)?;
    let manifest_name = names::manifest_file_name(&args.family, args.size)?;

    let ttf = std::fs::read(&args.ttf).map_err(|e| format!("{}: {e}", args.ttf.display()))?;
    let license =
        std::fs::read(&args.license).map_err(|e| format!("{}: {e}", args.license.display()))?;
    if license.is_empty() {
        return Err(format!("{} is empty", args.license.display()).into());
    }

    let source_sha = sha256_hex(&ttf);
    if let Some(want) = &args.expect_sha256
        && *want != source_sha
    {
        return Err(format!(
            "{}: SHA-256 is {source_sha}, --expect-sha256 wants {want}",
            args.ttf.display()
        )
        .into());
    }

    let conv = convert::convert(&ttf, args.size, args.fallback, &license)?;

    let mut m = String::new();
    writeln!(
        m,
        "pulp-fontpack {} build manifest",
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(m, "source: {} ({} bytes)", file_name(&args.ttf), ttf.len())?;
    writeln!(m, "source SHA-256: {source_sha}")?;
    writeln!(
        m,
        "license: {} ({} bytes), embedded in {pack_name} and copied to {license_name}",
        file_name(&args.license),
        license.len()
    )?;
    writeln!(
        m,
        "pixel size: {}  line_height: {}  ascent: {}",
        args.size, conv.line_height, conv.ascent
    )?;
    writeln!(m, "fallback: U+{:04X}", conv.fallback_cp)?;
    writeln!(
        m,
        "cmap code points: {}  packed: {}  skipped (invalid scalar): {}",
        conv.cmap_count,
        conv.code_points.len(),
        conv.invalid.len()
    )?;
    for cp in &conv.invalid {
        writeln!(m, "  skipped U+{cp:04X}: not a Unicode scalar value")?;
    }
    writeln!(
        m,
        "pack: {pack_name} ({} bytes, glyphs {}, bitmaps {} bytes, largest glyph {} bytes)",
        conv.pack.len(),
        conv.code_points.len(),
        conv.bitmap_len,
        conv.max_glyph_len
    )?;
    writeln!(m, "pack SHA-256: {}", sha256_hex(&conv.pack))?;
    writeln!(
        m,
        "self-verify: passed (pulp-render loader, {} glyphs round-tripped)",
        conv.code_points.len()
    )?;
    m.push_str(&coverage::render(&conv.code_points));

    std::fs::create_dir_all(&args.out)?;
    write_outputs(
        &args.out,
        [
            (&pack_name, &conv.pack),
            (&license_name, &license),
            (&manifest_name, m.as_bytes()),
        ],
    )?;
    Ok(m)
}

struct StageDir {
    path: PathBuf,
    keep: bool,
}

impl StageDir {
    fn new(out: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..128 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = out.join(format!(
                ".pulp-fontpack-{}-{stamp}-{id}",
                std::process::id()
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique staging directory",
        ))
    }
}

impl Drop for StageDir {
    fn drop(&mut self) {
        if !self.keep {
            // Backups can contain an unexpected directory if another process
            // replaces a destination after preflight. Never recurse into it.
            for i in 0..3 {
                let _ = std::fs::remove_file(self.path.join(format!("new-{i}")));
                let _ = std::fs::remove_file(self.path.join(format!("old-{i}")));
            }
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

struct OutputLock {
    _file: Option<std::fs::File>,
    #[cfg(not(unix))]
    path: PathBuf,
}

impl OutputLock {
    fn acquire(out: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;

            // Lock the directory itself. The kernel releases this lock
            // when the process exits, even after a forced termination.
            // Keeping one directory inode avoids stale lock files and
            // unlink/recreate races between converter processes.
            let file = std::fs::File::open(out)?;
            // SAFETY: file owns a live directory descriptor. flock does
            // not retain the pointer or descriptor after this call.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { _file: Some(file) })
        }
        #[cfg(not(unix))]
        {
            let path = out.join(".pulp-fontpack.lock");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
            Ok(Self {
                path,
                _file: Some(file),
            })
        }
    }
}

#[cfg(not(unix))]
impl Drop for OutputLock {
    fn drop(&mut self) {
        drop(self._file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Stage all three files in an exclusively created directory. Publish the
/// pack last; if any rename fails, restore the prior files from that directory.
fn write_outputs(out: &Path, files: [(&str, &[u8]); 3]) -> Result<(), Box<dyn std::error::Error>> {
    // Covers the entire preflight/stage/publish window across converter processes.
    let _lock = OutputLock::acquire(out)?;
    let destinations = files.map(|(name, _)| out.join(name));
    let mut existed = [false; 3];
    for (i, path) in destinations.iter().enumerate() {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                existed[i] = true;
            }
            Ok(_) => return Err(format!("{}: output is not a file", path.display()).into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    let mut stage = StageDir::new(out)?;
    let staged = std::array::from_fn::<_, 3, _>(|i| stage.path.join(format!("new-{i}")));
    for (i, (_, bytes)) in files.iter().enumerate() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged[i])?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if std::fs::read(&staged[i])? != *bytes {
            return Err(format!("{}: read-back differs", staged[i].display()).into());
        }
    }

    publish_outputs(&mut stage, &destinations, existed)
}

fn publish_outputs(
    stage: &mut StageDir,
    destinations: &[PathBuf; 3],
    existed: [bool; 3],
) -> Result<(), Box<dyn std::error::Error>> {
    let staged = std::array::from_fn::<_, 3, _>(|i| stage.path.join(format!("new-{i}")));
    let backups = std::array::from_fn::<_, 3, _>(|i| stage.path.join(format!("old-{i}")));
    // The pack becomes visible only after the attribution and manifest do.
    let order = [1, 2, 0];
    let mut backed_up = [false; 3];
    let mut installed = [false; 3];
    let publish = (|| -> io::Result<()> {
        for &i in &order {
            if existed[i] {
                std::fs::rename(&destinations[i], &backups[i])?;
                backed_up[i] = true;
                let kind = std::fs::symlink_metadata(&backups[i])?.file_type();
                if !kind.is_file() && !kind.is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{}: output changed type during publish",
                            destinations[i].display()
                        ),
                    ));
                }
            }
            std::fs::rename(&staged[i], &destinations[i])?;
            installed[i] = true;
        }
        Ok(())
    })();

    if let Err(error) = publish {
        let mut rollback_errors = Vec::new();
        for &i in order.iter().rev() {
            if installed[i]
                && let Err(e) = std::fs::remove_file(&destinations[i])
            {
                rollback_errors.push(format!("{}: {e}", destinations[i].display()));
            }
            if backed_up[i]
                && let Err(e) = std::fs::rename(&backups[i], &destinations[i])
            {
                rollback_errors.push(format!("{}: {e}", destinations[i].display()));
            }
        }
        if !rollback_errors.is_empty() {
            stage.keep = true;
            return Err(format!(
                "publish failed: {error}; rollback incomplete (backups in {}): {}",
                stage.path.display(),
                rollback_errors.join("; ")
            )
            .into());
        }
        return Err(error.into());
    }
    Ok(())
}

fn write_stdout(bytes: &[u8]) -> ExitCode {
    let mut out = io::stdout().lock();
    match out.write_all(bytes).and_then(|_| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error writing stdout: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return write_stdout(format!("{USAGE}\n").as_bytes()),
        Err(e) => {
            eprintln!("error: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let start = Instant::now();
    match run(&args) {
        Ok(manifest) => write_stdout(
            format!(
                "{manifest}wrote {} in {:.2} s\n",
                args.out.display(),
                start.elapsed().as_secs_f64()
            )
            .as_bytes(),
        ),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_cleanup_does_not_remove_an_unexpected_directory() {
        let out = std::env::temp_dir().join(format!(
            "pulp-fontpack-stage-cleanup-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&out).unwrap();
        let stage = StageDir::new(&out).unwrap();
        let unexpected = stage.path.join("old-0");
        std::fs::create_dir(&unexpected).unwrap();
        let sentinel = unexpected.join("keep.txt");
        std::fs::write(&sentinel, b"keep").unwrap();

        drop(stage);
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn raced_directory_is_restored_without_publishing_artifacts() {
        let out = std::env::temp_dir().join(format!(
            "pulp-fontpack-raced-destination-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&out).unwrap();
        let destinations = [
            out.join("PACK.PFP"),
            out.join("LICENSE.TXT"),
            out.join("MANIFEST.TXT"),
        ];
        std::fs::write(&destinations[0], b"previous pack").unwrap();
        // This is the preflight observation; another process then swaps the
        // destination for a nonempty directory before the publish step.
        let existed = [true, false, false];
        std::fs::remove_file(&destinations[0]).unwrap();
        std::fs::create_dir(&destinations[0]).unwrap();
        let sentinel = destinations[0].join("keep.txt");
        std::fs::write(&sentinel, b"keep").unwrap();
        let mut stage = StageDir::new(&out).unwrap();
        for i in 0..3 {
            std::fs::write(stage.path.join(format!("new-{i}")), b"new").unwrap();
        }

        let error = publish_outputs(&mut stage, &destinations, existed)
            .expect_err("a raced directory must stop publication");
        assert!(error.to_string().contains("changed type"), "{error}");
        drop(stage);
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
        assert!(!destinations[1].exists());
        assert!(!destinations[2].exists());
        std::fs::remove_dir_all(&out).unwrap();
    }
}

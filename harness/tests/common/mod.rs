// Shared helpers of the build-boundary tests. The ELF tests check the images built by
// `task build` into `target/accept/<image>/`; a missing image is a failure, never a skip.
#![allow(dead_code)]

use object::{Object, ObjectSection, ObjectSymbol, SectionFlags, SymbolSection};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const X4_TARGET: &str = "riscv32imc-unknown-none-elf";
pub const C61_TARGET: &str = "riscv32imac-unknown-none-elf";
/// build-std override of the host-side cargo commands (std + test instead of core + alloc)
pub const HOST_BUILD_STD: &str = "unstable.build-std=[\"std\",\"test\"]";

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[derive(Clone, Copy)]
pub enum Image {
    X4,
    X4Wifi,
    C61,
    C61Boot,
    C61Wifi,
}

/// Path of a built image; panics when it has not been built.
pub fn image(img: Image) -> PathBuf {
    let (dir, target, bin) = match img {
        Image::X4 => ("x4", X4_TARGET, "pulp-os"),
        Image::X4Wifi => ("x4-wifi", X4_TARGET, "pulp-os"),
        Image::C61 => ("c61", C61_TARGET, "pulp-os-c61"),
        Image::C61Boot => ("c61", C61_TARGET, "pulp-os-c61-boot"),
        Image::C61Wifi => ("c61-wifi", C61_TARGET, "pulp-os-c61"),
    };
    let path = workspace_root()
        .join("target/accept")
        .join(dir)
        .join(target)
        .join("release")
        .join(bin);
    assert!(
        path.is_file(),
        "missing image {}: build it first with `task build`",
        path.display()
    );
    path
}

pub fn read_elf(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

pub fn parse_elf(bytes: &[u8]) -> object::File<'_> {
    object::File::parse(bytes).expect("parse elf")
}

/// Names of the symbols that are really defined (nm: not `U`, not absolute `A`).
/// Absolute symbols are ROM addresses defined by linker scripts, not linked code.
pub fn defined_symbols<'a>(obj: &'a object::File<'_>) -> Vec<&'a str> {
    obj.symbols()
        .filter(|s| {
            !matches!(
                s.section(),
                SymbolSection::Absolute | SymbolSection::Undefined
            )
        })
        .filter_map(|s| s.name().ok())
        .collect()
}

pub fn section_flags(flags: SectionFlags) -> u64 {
    match flags {
        SectionFlags::Elf { sh_flags } => sh_flags,
        _ => 0,
    }
}

/// Printable-ASCII runs of at least `min` bytes (what `strings -a` prints).
pub fn ascii_runs(data: &[u8], min: usize) -> Vec<&[u8]> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, &b) in data.iter().enumerate() {
        let printable = (0x20..0x7f).contains(&b) || b == b'\t';
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min {
                    runs.push(&data[s..i]);
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if data.len() - s >= min {
            runs.push(&data[s..]);
        }
    }
    runs
}

/// Every allocated section's contents (the image bytes the firmware carries).
pub fn section_data<'a>(obj: &'a object::File<'_>) -> Vec<&'a [u8]> {
    obj.sections().filter_map(|s| s.data().ok()).collect()
}

/// `cargo <args>` in the workspace root; returns (success, stdout + stderr).
pub fn cargo(args: &[&str]) -> (bool, String) {
    let out = Command::new("cargo")
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("run cargo");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

/// All `.rs` files under `path` (a file is returned as is).
pub fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path).unwrap_or_else(|e| panic!("read_dir {}: {e}", path.display())) {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            files.extend(rust_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            files.push(p);
        }
    }
    files
}

/// Lines of non-comment code as (path, 1-based line, text).
pub fn code_lines(files: &[PathBuf]) -> Vec<(PathBuf, usize, String)> {
    let mut out = Vec::new();
    for f in files {
        let text = fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        for (i, line) in text.lines().enumerate() {
            if !line.trim_start().starts_with("//") {
                out.push((f.clone(), i + 1, line.to_string()));
            }
        }
    }
    out
}

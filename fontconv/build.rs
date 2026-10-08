//! Embeds a digest of everything that determines converter output into the
//! crate (`PULP_FONTCONV_BUILD_ID`), so the bundle cache key changes whenever
//! the converter, the fontpack format, or any dependency changes.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Paths relative to the workspace root. `Cargo.lock` pins the exact versions
/// of the rasteriser (fontdue) and every other dependency; it also moves with
/// unrelated firmware crates, which only costs an extra rebuild of the packs.
const INPUTS: &[&str] = &[
    "fontconv/src",
    "fontconv/build.rs",
    "fontpack/src",
    "fontconv/Cargo.toml",
    "fontpack/Cargo.toml",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/config.toml",
];

fn collect(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let path = root.join(rel);
    if path.is_dir() {
        for entry in fs::read_dir(&path)? {
            collect(root, &rel.join(entry?.file_name()), out)?;
        }
    } else {
        // Missing inputs are an error: a silent skip would weaken the key.
        fs::metadata(&path)?;
        out.push(rel.to_path_buf());
    }
    Ok(())
}

/// SHA-256 over the sorted (relative path, content) pairs of `inputs`.
pub fn build_identity(root: &Path, inputs: &[&str]) -> io::Result<String> {
    let mut files = Vec::new();
    for input in inputs {
        collect(root, Path::new(input), &mut files)?;
    }
    files.sort();

    let mut hasher = Sha256::new();
    for rel in &files {
        let name = rel.to_string_lossy();
        let bytes = fs::read(root.join(rel))?;
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    for input in INPUTS {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let id = build_identity(root, INPUTS).expect("hash converter build inputs");
    println!("cargo:rustc-env=PULP_FONTCONV_BUILD_ID={id}");
}

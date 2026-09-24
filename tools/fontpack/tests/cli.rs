//! End-to-end CLI run on the tracked Iansui subset: pack + byte-identical
//! license sidecar + manifest with the source SHA-256, coverage and the
//! self-verification result (R1, R12, R14).
mod common;

use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pulp-fontpack"))
}

fn run(license: Option<&Path>, out: &Path, extra: &[&str]) -> Output {
    let mut cmd = bin();
    cmd.args(["--ttf", common::subset_ttf_path().to_str().unwrap()]);
    if let Some(l) = license {
        cmd.args(["--license", l.to_str().unwrap()]);
    }
    cmd.args(["--size", "16", "--family", "IANSUI"])
        .args(["--out", out.to_str().unwrap()])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn writes_pack_sidecar_and_manifest() {
    let dir = common::temp_dir("cli-ok");
    let license_path = dir.join("LICENSE-in.txt");
    let license = b"\xEF\xBB\xBFCopyright 2025 Example.\r\nSIL OFL 1.1\n\xE4\xBA\x86\n".to_vec();
    std::fs::write(&license_path, &license).unwrap();
    let out = dir.join("FONTS");

    let res = run(Some(&license_path), &out, &[]);
    let stdout = String::from_utf8_lossy(&res.stdout);
    assert!(
        res.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&res.stderr)
    );

    let pack = std::fs::read(out.join("IANSUI16.PFP")).unwrap();
    let sidecar = std::fs::read(out.join("IANSUOFL.TXT")).unwrap();
    let manifest = std::fs::read_to_string(out.join("IANSUI16.TXT")).unwrap();

    assert_eq!(sidecar, license, "sidecar is byte-identical to --license");
    pulp_render::font_pack::FontPack::load(&mut pulp_render::font_pack::SliceReader(&pack), 16)
        .expect("the written file loads with the device loader");
    assert_eq!(
        common::license_section(&pack),
        license.as_slice(),
        "embedded license is byte-exact"
    );
    let names: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 3, "no temporary files left behind: {names:?}");
    assert!(!out.join(".pulp-fontpack.lock").exists());

    let src_sha = hex(&Sha256::digest(common::subset_ttf()));
    let pack_sha = hex(&Sha256::digest(&pack));
    for text in [&*stdout, manifest.as_str()] {
        assert!(text.contains(&src_sha), "source SHA-256 recorded:\n{text}");
        assert!(text.contains(&pack_sha), "pack SHA-256 recorded:\n{text}");
        assert!(text.contains("Hiragana"), "coverage table present");
        assert!(
            text.contains("CJK Unified Ideographs"),
            "coverage table present"
        );
        assert!(text.contains("Hangul"), "Hangul coverage reported");
        assert!(text.contains("U+25A1"), "default fallback reported");
        assert!(
            text.contains("self-verify: passed"),
            "verification reported"
        );
    }
}

#[test]
fn refuses_to_run_without_a_license() {
    // §8: the converter refuses to run without --license
    let dir = common::temp_dir("cli-nolicense");
    let out = dir.join("FONTS");
    let res = run(None, &out, &[]);
    assert!(!res.status.success());
    assert!(!out.join("IANSUI16.PFP").exists());
}

#[test]
fn refuses_an_empty_license_file() {
    let dir = common::temp_dir("cli-emptylicense");
    let license_path = dir.join("empty.txt");
    std::fs::write(&license_path, b"").unwrap();
    let out = dir.join("FONTS");
    let res = run(Some(&license_path), &out, &[]);
    assert!(!res.status.success());
    assert!(!out.join("IANSUI16.PFP").exists());
}

/// R4: an invisible (U+0020) or absent (U+D55C: Iansui has no Hangul)
/// fallback fails generation and writes no pack.
#[test]
fn refuses_an_invisible_or_absent_fallback() {
    for (name, fallback, reason) in [
        ("cli-fb-space", "U+0020", "no ink"),
        ("cli-fb-absent", "U+D55C", "not in the font"),
    ] {
        let dir = common::temp_dir(name);
        let out = dir.join("FONTS");
        let res = run(
            Some(&common::subset_license_path()),
            &out,
            &["--fallback", fallback],
        );
        let stderr = String::from_utf8_lossy(&res.stderr);
        assert!(!res.status.success(), "{fallback} accepted");
        assert!(
            stderr.contains(fallback) && stderr.contains(reason),
            "{fallback}: {stderr}"
        );
        assert!(!out.join("IANSUI16.PFP").exists(), "{fallback}");
    }
}

/// --expect-sha256 pins the source TTF: a match converts, a mismatch fails
/// before anything is written (the check against the supplied full Iansui).
#[test]
fn expect_sha256_pins_the_source() {
    let sha = hex(&Sha256::digest(common::subset_ttf()));

    let dir = common::temp_dir("cli-sha-ok");
    let out = dir.join("FONTS");
    let res = run(
        Some(&common::subset_license_path()),
        &out,
        &["--expect-sha256", &sha.to_uppercase()],
    );
    assert!(
        res.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert!(out.join("IANSUI16.PFP").exists());

    let dir = common::temp_dir("cli-sha-bad");
    let out = dir.join("FONTS");
    let wrong = "0".repeat(64);
    let res = run(
        Some(&common::subset_license_path()),
        &out,
        &["--expect-sha256", &wrong],
    );
    let stderr = String::from_utf8_lossy(&res.stderr);
    assert!(!res.status.success());
    assert!(stderr.contains(&wrong) && stderr.contains(&sha), "{stderr}");
    assert!(!out.exists(), "nothing written on a SHA-256 mismatch");
}

#[test]
fn sidecar_conflict_leaves_no_new_pack() {
    let dir = common::temp_dir("cli-sidecar-conflict");
    let out = dir.join("FONTS");
    std::fs::create_dir_all(out.join("IANSUOFL.TXT")).unwrap();
    let res = run(Some(&common::subset_license_path()), &out, &[]);
    assert!(!res.status.success());
    assert!(!out.join("IANSUI16.PFP").exists());
    assert!(!out.join("IANSUI16.TXT").exists());
    assert!(!out.join(".pulp-fontpack.lock").exists());
}

#[test]
fn manifest_conflict_preserves_existing_pack_and_sidecar() {
    let dir = common::temp_dir("cli-manifest-conflict");
    let out = dir.join("FONTS");
    assert!(
        run(Some(&common::subset_license_path()), &out, &[])
            .status
            .success()
    );
    let old_pack = std::fs::read(out.join("IANSUI16.PFP")).unwrap();
    let old_license = std::fs::read(out.join("IANSUOFL.TXT")).unwrap();
    std::fs::remove_file(out.join("IANSUI16.TXT")).unwrap();
    std::fs::create_dir(out.join("IANSUI16.TXT")).unwrap();
    let new_license = dir.join("new-license.txt");
    std::fs::write(&new_license, b"New license text").unwrap();

    let res = run(Some(&new_license), &out, &[]);
    assert!(!res.status.success());
    assert!(
        std::fs::read(out.join("IANSUI16.PFP")).unwrap() == old_pack,
        "the previous valid pack must survive"
    );
    assert!(
        std::fs::read(out.join("IANSUOFL.TXT")).unwrap() == old_license,
        "the previous license must survive"
    );
    assert!(out.join("IANSUI16.TXT").is_dir());
}

#[test]
fn preexisting_fixed_tmp_directory_does_not_delete_previous_pack() {
    let dir = common::temp_dir("cli-tmp-directory");
    let out = dir.join("FONTS");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("IANSUI16.PFP"), b"previous pack").unwrap();
    std::fs::create_dir(out.join("IANSUI16.TMP")).unwrap();

    let res = run(Some(&common::subset_license_path()), &out, &[]);
    assert!(
        res.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert!(out.join("IANSUI16.TMP").is_dir());
    let pack = std::fs::read(out.join("IANSUI16.PFP")).unwrap();
    pulp_render::font_pack::FontPack::load(&mut pulp_render::font_pack::SliceReader(&pack), 16)
        .expect("the previous pack is replaced with a valid new pack");
}

#[cfg(unix)]
#[test]
fn preexisting_fixed_tmp_symlink_cannot_clobber_another_file() {
    use std::os::unix::fs::symlink;

    let dir = common::temp_dir("cli-tmp-symlink");
    let out = dir.join("FONTS");
    std::fs::create_dir_all(&out).unwrap();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"keep this content").unwrap();
    symlink(&victim, out.join("IANSUI16.TMP")).unwrap();

    let res = run(Some(&common::subset_license_path()), &out, &[]);
    assert!(
        res.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep this content");
    assert!(
        std::fs::symlink_metadata(out.join("IANSUI16.TMP"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let pack = std::fs::read(out.join("IANSUI16.PFP")).unwrap();
    pulp_render::font_pack::FontPack::load(&mut pulp_render::font_pack::SliceReader(&pack), 16)
        .expect("the new pack is a regular valid file");
}

#[cfg(unix)]
#[test]
fn stale_lock_file_does_not_block_a_new_conversion() {
    let dir = common::temp_dir("cli-output-lock");
    let out = dir.join("FONTS");
    std::fs::create_dir_all(&out).unwrap();
    let lock = out.join(".pulp-fontpack.lock");
    std::fs::write(&lock, b"left by a killed converter").unwrap();

    let res = run(Some(&common::subset_license_path()), &out, &[]);
    assert!(
        res.status.success(),
        "{}",
        String::from_utf8_lossy(&res.stderr)
    );
    assert!(out.join("IANSUI16.PFP").exists());
    assert_eq!(std::fs::read(&lock).unwrap(), b"left by a killed converter");
}

#[cfg(unix)]
#[test]
fn directory_lock_prevents_a_second_conversion() {
    use std::os::fd::AsRawFd;

    let dir = common::temp_dir("cli-directory-lock");
    let out = dir.join("FONTS");
    std::fs::create_dir_all(&out).unwrap();
    let held = std::fs::File::open(&out).unwrap();
    assert_eq!(
        unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );

    let res = run(Some(&common::subset_license_path()), &out, &[]);
    assert!(!res.status.success());
    assert!(!out.join("IANSUI16.PFP").exists());
    assert!(!out.join("IANSUOFL.TXT").exists());
    assert!(!out.join("IANSUI16.TXT").exists());
}

#[test]
fn help_exits_successfully_without_arguments() {
    for flag in ["-h", "--help"] {
        let output = bin().arg(flag).output().unwrap();
        assert!(output.status.success(), "{flag} must exit 0");
        assert!(String::from_utf8_lossy(&output.stdout).contains("usage: pulp-fontpack"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn empty_output_directory_is_rejected_without_writing_to_cwd() {
    let dir = common::temp_dir("cli-empty-out");
    let output = bin()
        .args(["--ttf", common::subset_ttf_path().to_str().unwrap()])
        .args(["--license", common::subset_license_path().to_str().unwrap()])
        .args(["--size", "16", "--family", "IANSUI", "--out", ""])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.join("IANSUI16.PFP").exists());
    assert!(!dir.join("IANSUOFL.TXT").exists());
    assert!(!dir.join("IANSUI16.TXT").exists());
}

#[test]
fn closed_stdout_does_not_panic_after_a_successful_conversion() {
    let dir = common::temp_dir("cli-closed-stdout");
    let out = dir.join("FONTS");
    let mut child = bin()
        .args(["--ttf", common::subset_ttf_path().to_str().unwrap()])
        .args(["--license", common::subset_license_path().to_str().unwrap()])
        .args(["--size", "16", "--family", "IANSUI"])
        .args(["--out", out.to_str().unwrap()])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.join("IANSUI16.PFP").exists());
}

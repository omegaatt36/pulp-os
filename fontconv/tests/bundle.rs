// Pure Rust test suite for font bundle building and caching (replaces scripts/tests/test_build_cjk_fonts.py).

use pulp_fontconv::bundle::{TempDir, build_bundle, sha256_bytes, sha256_file};
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

struct TestFixture {
    _tmp: TempDir,
    base: PathBuf,
    manifest: PathBuf,
    license: PathBuf,
    out: PathBuf,
    cache: PathBuf,
    font: PathBuf,
}

impl TestFixture {
    fn new() -> Self {
        let root = workspace_root();
        let target_dir = root.join("target");
        fs::create_dir_all(&target_dir).unwrap();
        let tmp = TempDir::new_in(&target_dir, "bundle-test-").unwrap();
        let base = tmp.path().to_path_buf();

        let font = root.join("assets/fonts/Bookerly-Regular.ttf");
        assert!(font.is_file(), "Bookerly font fixture must exist");

        let license = base.join("license.txt");
        fs::write(&license, b"Test fixture only; no redistribution.\n").unwrap();

        let manifest = base.join("font.json");
        let out = base.join("sd");
        let cache = base.join("cache");

        let fixture = Self {
            _tmp: tmp,
            base,
            manifest,
            license,
            out,
            cache,
            font,
        };
        fixture.write_manifest(None, None);
        fixture
    }

    fn write_manifest(&self, override_font_hash: Option<&str>, override_sizes: Option<&[u16]>) {
        let font_hash = match override_font_hash {
            Some(h) => h.to_string(),
            None => sha256_file(&self.font).unwrap(),
        };
        let license_hash = sha256_file(&self.license).unwrap();
        let sizes_str = match override_sizes {
            Some(sizes) => {
                let items: Vec<String> = sizes.iter().map(|s| s.to_string()).collect();
                format!("[{}]", items.join(","))
            }
            None => "[16]".to_string(),
        };

        let json = format!(
            r#"{{
  "schema_version": 1,
  "name": "alternate-fixture",
  "version": "fixture",
  "upstream_url": "https://example.invalid/fixture",
  "license_name": "Test fixture permission",
  "sizes": {},
  "font": {{
    "path": "{}",
    "sha256": "{}"
  }},
  "license": {{
    "path": "license.txt",
    "sha256": "{}"
  }}
}}"#,
            sizes_str,
            self.font.display(),
            font_hash,
            license_hash
        );
        fs::write(&self.manifest, json).unwrap();
    }

    fn build(&self) -> Result<bool, Box<dyn std::error::Error>> {
        build_bundle(&self.manifest, &self.out, &self.cache)
    }
}

#[test]
fn single_font_bundle_has_exact_license_and_verified_cache_hit() {
    let f = TestFixture::new();
    // First build: cache miss
    let hit1 = f.build().expect("first build");
    assert!(!hit1, "first build must be cache miss");

    let fonts_dir = f.out.join("_PULP/FONTS");
    assert!(fonts_dir.join("F00016.PFN").is_file());
    assert_eq!(
        fs::read(fonts_dir.join("LICENSE.TXT")).unwrap(),
        fs::read(&f.license).unwrap()
    );
    assert!(fonts_dir.join("COVERAGE.TXT").is_file());
    let prov = fs::read_to_string(fonts_dir.join("PROV.TXT")).unwrap();
    assert!(prov.contains("license_name=Test fixture permission"));

    // Second build: verified cache hit
    let hit2 = f.build().expect("second build");
    assert!(hit2, "second build must be cache hit");

    // Stale file is cleaned up on cache hit reinstall
    fs::write(fonts_dir.join("STALE.PFN"), b"stale").unwrap();
    let hit3 = f.build().expect("third build");
    assert!(hit3, "third build must be cache hit");
    assert!(!fonts_dir.join("STALE.PFN").exists());
}

#[test]
fn corrupt_cached_pack_is_rebuilt() {
    let f = TestFixture::new();
    f.build().unwrap();

    let mut pack_found = None;
    for entry in fs::read_dir(&f.cache).unwrap().flatten() {
        let p = entry.path().join("F00016.PFN");
        if p.is_file() {
            pack_found = Some(p);
            break;
        }
    }
    let pack = pack_found.expect("cached pack");
    let original = fs::read(&pack).unwrap();
    fs::write(&pack, b"corrupt").unwrap();

    let hit = f.build().unwrap();
    assert!(!hit, "corrupt pack must trigger rebuild");
    assert_eq!(fs::read(&pack).unwrap(), original);
}

#[test]
fn changed_verified_license_invalidates_cache() {
    let f = TestFixture::new();
    f.build().unwrap();

    fs::write(&f.license, b"Changed fixture permission.\n").unwrap();
    f.write_manifest(None, None);

    let hit = f.build().unwrap();
    assert!(!hit, "license change must invalidate cache");
}

#[test]
fn source_hash_mismatch_is_rejected_before_output() {
    let f = TestFixture::new();
    let bogus_hash = "0".repeat(64);
    f.write_manifest(Some(&bogus_hash), None);

    let res = f.build();
    assert!(res.is_err());
    let err = res.err().unwrap().to_string();
    assert!(err.contains("SHA256 mismatch"));
    assert!(!f.out.exists());
}

#[test]
fn unowned_font_directory_is_preserved_even_with_unrelated_marker() {
    let f = TestFixture::new();
    let fonts_dir = f.out.join("_PULP/FONTS");
    fs::create_dir_all(&fonts_dir).unwrap();
    fs::write(fonts_dir.join("BUNDLE.JSON"), b"{}").unwrap();
    fs::write(fonts_dir.join("important.txt"), b"preserve").unwrap();

    let res = f.build();
    assert!(res.is_err());
    let err = res.err().unwrap().to_string();
    assert!(err.contains("unowned"));
    assert_eq!(
        fs::read_to_string(fonts_dir.join("important.txt")).unwrap(),
        "preserve"
    );
}

#[test]
fn missing_required_characters_do_not_publish_a_bundle() {
    let f = TestFixture::new();
    let req = f.base.join("required.txt");
    fs::write(&req, "\u{2a6a5}").unwrap();
    let req_hash = sha256_bytes("\u{2a6a5}".as_bytes());

    let json = format!(
        r#"{{
  "schema_version": 1,
  "name": "alternate-fixture",
  "version": "fixture",
  "upstream_url": "https://example.invalid/fixture",
  "license_name": "Test fixture permission",
  "sizes": [16],
  "font": {{
    "path": "{}",
    "sha256": "{}"
  }},
  "license": {{
    "path": "license.txt",
    "sha256": "{}"
  }},
  "require_chars": {{
    "path": "required.txt",
    "sha256": "{}"
  }}
}}"#,
        f.font.display(),
        sha256_file(&f.font).unwrap(),
        sha256_file(&f.license).unwrap(),
        req_hash
    );
    fs::write(&f.manifest, json).unwrap();

    let res = f.build();
    assert!(res.is_err());
    assert!(!f.out.exists());
}

#[test]
fn cache_artifact_symlink_is_rejected_without_touching_target() {
    let f = TestFixture::new();
    f.build().unwrap();

    let mut artifact_found = None;
    for entry in fs::read_dir(&f.cache).unwrap().flatten() {
        if entry.path().is_dir() {
            artifact_found = Some(entry.path());
            break;
        }
    }
    let artifact = artifact_found.expect("artifact");
    let victim = f.base.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("important.txt"), b"preserve").unwrap();

    fs::remove_dir_all(&artifact).unwrap();
    std::os::unix::fs::symlink(&victim, &artifact).unwrap();

    let res = f.build();
    assert!(res.is_err());
    let err = res.err().unwrap().to_string();
    assert!(err.contains("symlink"));
    assert_eq!(
        fs::read_to_string(victim.join("important.txt")).unwrap(),
        "preserve"
    );
}

#[test]
fn sd_parent_symlink_is_rejected_without_touching_target() {
    let f = TestFixture::new();
    let victim = f.base.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::create_dir_all(&f.out).unwrap();
    std::os::unix::fs::symlink(&victim, f.out.join("_PULP")).unwrap();

    let res = f.build();
    assert!(res.is_err());
    let err = res.err().unwrap().to_string();
    assert!(err.contains("symlink"));
    assert_eq!(fs::read_dir(&victim).unwrap().count(), 0);
}

#[test]
fn duplicate_or_invalid_sizes_are_rejected() {
    let f = TestFixture::new();
    for bad_sizes in [&[16u16, 16][..], &[][..], &[0][..], &[256][..]] {
        f.write_manifest(None, Some(bad_sizes));
        let res = f.build();
        assert!(res.is_err(), "Expected error for sizes {:?}", bad_sizes);
        let err = res.err().unwrap().to_string();
        assert!(err.contains("sizes"), "Error should mention sizes: {err}");
    }
}

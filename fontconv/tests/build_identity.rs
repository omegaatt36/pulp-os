// R2: the converter/fontpack build identity and the cache key derived from it.
// Expected outcomes come from the requirement: identity or key differs when
// sources/dependencies/manifest/rustc change, and is equal when none changed.

use pulp_fontconv::bundle::{BUILD_ID, JsonValue, TempDir, cache_identity, cache_key};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

fn manifest(name: &str) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("name".to_string(), JsonValue::String(name.to_string()));
    JsonValue::Object(m)
}

fn key(manifest_name: &str, rustc: &str, build_id: &str) -> String {
    cache_key(&cache_identity(&manifest(manifest_name), rustc, build_id))
}

#[test]
fn changed_build_identity_changes_cache_key() {
    assert_ne!(
        key("f", "rustc 1", "build-a"),
        key("f", "rustc 1", "build-b")
    );
}

#[test]
fn unchanged_manifest_rustc_and_build_identity_keep_cache_key() {
    assert_eq!(
        key("f", "rustc 1", "build-a"),
        key("f", "rustc 1", "build-a")
    );
}

#[test]
fn changed_manifest_or_rustc_still_changes_cache_key() {
    let base = key("f", "rustc 1", "build-a");
    assert_ne!(base, key("g", "rustc 1", "build-a"));
    assert_ne!(base, key("f", "rustc 2", "build-a"));
}

#[test]
fn embedded_build_id_is_a_sha256_digest() {
    assert_eq!(BUILD_ID.len(), 64, "{BUILD_ID}");
    assert!(
        BUILD_ID.bytes().all(|b| b.is_ascii_hexdigit()),
        "{BUILD_ID}"
    );
}

struct Tree {
    tmp: TempDir,
}

impl Tree {
    fn new() -> Self {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
        fs::create_dir_all(&parent).unwrap();
        let tmp = TempDir::new_in(&parent, "build-identity-").unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("fontconv/src")).unwrap();
        fs::create_dir_all(root.join("fontpack/src/nested")).unwrap();
        fs::write(root.join("fontconv/src/lib.rs"), b"converter").unwrap();
        fs::write(root.join("fontpack/src/nested/format.rs"), b"fontpack").unwrap();
        fs::write(root.join("Cargo.lock"), b"fontdue 0.9.0").unwrap();
        Self { tmp }
    }

    fn id(&self) -> String {
        build_script::build_identity(
            self.tmp.path(),
            &["fontconv/src", "fontpack/src", "Cargo.lock"],
        )
        .unwrap()
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        fs::write(self.tmp.path().join(rel), bytes).unwrap();
    }
}

#[test]
fn unchanged_inputs_keep_identity() {
    let t = Tree::new();
    assert_eq!(t.id(), t.id());
}

#[test]
fn changed_converter_source_changes_identity() {
    let t = Tree::new();
    let before = t.id();
    t.write("fontconv/src/lib.rs", b"converter!");
    assert_ne!(before, t.id());
}

#[test]
fn changed_nested_fontpack_source_changes_identity() {
    let t = Tree::new();
    let before = t.id();
    t.write("fontpack/src/nested/format.rs", b"fontpack!");
    assert_ne!(before, t.id());
}

#[test]
fn changed_dependency_lock_changes_identity() {
    let t = Tree::new();
    let before = t.id();
    t.write("Cargo.lock", b"fontdue 0.9.1");
    assert_ne!(before, t.id());
}

#[test]
fn added_source_file_changes_identity() {
    let t = Tree::new();
    let before = t.id();
    t.write("fontconv/src/extra.rs", b"");
    assert_ne!(before, t.id());
}

#[test]
fn renamed_source_file_changes_identity() {
    let t = Tree::new();
    let before = t.id();
    fs::rename(
        t.tmp.path().join("fontconv/src/lib.rs"),
        t.tmp.path().join("fontconv/src/main.rs"),
    )
    .unwrap();
    assert_ne!(before, t.id());
}

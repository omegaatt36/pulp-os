// Pure Rust verification of the host build boundary (replaces scripts/check-host-boundary.sh).

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn host_dependency_graph_has_no_espressif_crates() {
    let root = workspace_root();

    let output = Command::new("cargo")
        .current_dir(&root)
        .args([
            "tree",
            "--locked",
            "-p",
            "pulp-host",
            "-e",
            "normal,build,dev",
            "--prefix",
            "none",
        ])
        .output()
        .expect("cargo tree");

    assert!(output.status.success(), "cargo tree failed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut forbidden_crates = Vec::new();
    for line in stdout.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        if (name.starts_with("esp-") || name.starts_with("esp_"))
            && !name.starts_with("esp-hal-shim")
            && !name.starts_with("board-hal-shim")
        {
            forbidden_crates.push(name.to_string());
        }
    }

    assert!(
        forbidden_crates.is_empty(),
        "pulp-host dependency graph contains forbidden Espressif hardware crates:\n{:?}",
        forbidden_crates
    );
}

#[test]
fn host_build_uses_no_embedded_linker_scripts() {
    let root = workspace_root();

    let output = Command::new("cargo")
        .current_dir(&root)
        .args([
            "test",
            "-p",
            "pulp-host",
            "--no-run",
            "-vv",
            "--config",
            "unstable.build-std=[\"std\",\"test\"]",
        ])
        .output()
        .expect("cargo test");

    let text = String::from_utf8_lossy(&output.stderr) + String::from_utf8_lossy(&output.stdout);

    for line in text.lines() {
        if line.contains("-Tlinkall.x")
            || line.contains("-Tmemory.x")
            || line.contains("-Trom_functions.x")
        {
            panic!(
                "Host build unexpectedly passed embedded linker script:\n{}",
                line
            );
        }
    }
}

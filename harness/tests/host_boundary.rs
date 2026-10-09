// Host build boundary: the host crate must not pull in an Espressif hardware runtime
// or apply an embedded linker script.

mod common;

use common::*;

/// Names of the esp-* / esp_* packages in `cargo tree --prefix none --format {p}` output.
fn esp_packages(tree: &str) -> Vec<String> {
    let mut found: Vec<String> = tree
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| n.starts_with("esp-") || n.starts_with("esp_"))
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found
}

#[test]
fn host_dependency_graph_has_no_espressif_crates() {
    let (ok, tree) = cargo(&[
        "tree",
        "--locked",
        "-p",
        "pulp-host",
        "-e",
        "normal,build,dev",
        "--target",
        "host-tuple",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    assert!(ok, "cargo tree for pulp-host failed:\n{tree}");
    assert!(
        tree.starts_with("pulp-host "),
        "unexpected graph root:\n{tree}"
    );
    let found = esp_packages(&tree);
    assert!(
        found.is_empty(),
        "host graph contains Espressif crates: {found:?}"
    );
}

#[test]
fn espressif_probe_control_sees_esp_hal_in_the_firmware_graph() {
    let (ok, tree) = cargo(&[
        "tree",
        "--locked",
        "-e",
        "normal,build",
        "--target",
        X4_TARGET,
        "--features",
        "board-x4",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    assert!(ok, "cargo tree for the X4 firmware failed:\n{tree}");
    assert!(
        esp_packages(&tree).iter().any(|n| n == "esp-hal"),
        "probe broken: the X4 firmware graph did not show esp-hal"
    );
}

/// An embedded linker-script argument: any `-T<script>.x`, or linkall.x itself.
fn applies_linker_script(line: &str) -> bool {
    line.contains("linkall.x")
        || line
            .split(|c: char| c.is_whitespace() || c == '=' || c == '\'' || c == '"')
            .any(|w| w.starts_with("-T") && w.ends_with(".x"))
}

#[test]
fn linker_script_probe_recognizes_its_targets() {
    assert!(applies_linker_script("-C link-arg=-Tlinkall.x"));
    assert!(applies_linker_script(
        "rustc -C link-arg=-Tmemory.x --crate-name x"
    ));
    assert!(!applies_linker_script(
        "-C link-arg=-fuse-ld=lld --target aarch64-apple-darwin"
    ));
}

#[test]
fn host_build_applies_no_embedded_linker_script() {
    // a dedicated target dir; only pulp-host is rebuilt so that every rustc / build-script
    // command of the host crate is printed by -vv
    let dir = workspace_root().join("target/host-boundary");
    let dir = dir.to_str().expect("utf-8 path");
    let run = |args: &[&str]| {
        let out = std::process::Command::new("cargo")
            .current_dir(workspace_root())
            .env("CARGO_TARGET_DIR", dir)
            .args(args)
            .output()
            .expect("run cargo");
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };
    let (ok, text) = run(&["clean", "-p", "pulp-host", "--target", "host-tuple"]);
    assert!(ok, "cargo clean failed:\n{text}");
    let (ok, text) = run(&[
        "test",
        "--locked",
        "-p",
        "pulp-host",
        "--no-run",
        "-vv",
        "--target",
        "host-tuple",
        "--config",
        HOST_BUILD_STD,
    ]);
    assert!(ok, "the host build failed:\n{text}");
    assert!(
        text.contains("--crate-name pulp_host"),
        "no rustc command of pulp-host was printed; the check saw nothing"
    );
    for line in text.lines() {
        assert!(
            !applies_linker_script(line),
            "host build applies an embedded linker script:\n{line}"
        );
        assert!(
            !line.contains("--target riscv32") && !line.contains("--target=riscv32"),
            "host build targets a firmware triple:\n{line}"
        );
    }
}

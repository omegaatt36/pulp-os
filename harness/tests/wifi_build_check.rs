// Pure Rust verification of the Wi-Fi build requirements (replaces scripts/check-wifi-build.sh).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cargo_tree_pkgs(target: &str, features: &str) -> Vec<(String, String, String)> {
    let root = workspace_root();
    let output = Command::new("cargo")
        .current_dir(&root)
        .args([
            "tree",
            "--locked",
            "-e",
            "normal",
            "--target",
            target,
            "--features",
            features,
            "--prefix",
            "none",
            "--format",
            "{p}|{f}",
        ])
        .output()
        .expect("cargo tree");

    assert!(output.status.success(), "cargo tree failed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut pkgs = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 2 {
            let p_parts: Vec<&str> = parts[0].split_whitespace().collect();
            if p_parts.len() >= 2 {
                let name = p_parts[0].to_string();
                let ver = p_parts[1].trim_start_matches('v').to_string();
                let feats = parts[1].trim_end_matches(" (*)").to_string();
                pkgs.push((name, ver, feats));
            }
        }
    }
    pkgs
}

#[test]
fn c61_wifi_dependency_set_matches_pinned_spec() {
    let pkgs = cargo_tree_pkgs("riscv32imac-unknown-none-elf", "board-onepage-c61,wifi");

    let required = [
        ("esp-hal", "1.2.0"),
        ("esp-rtos", "0.4.0"),
        ("esp-alloc", "0.11.0"),
        ("esp-radio", "1.0.0-beta.1"),
        ("esp-bootloader-esp-idf", "0.6.0"),
        ("embassy-net", "0.8.0"),
        ("esp-radio-rtos-driver", "0.4.2"),
        ("esp-phy", "0.3.0"),
        ("esp-wifi-sys-esp32c61", "0.3.0"),
    ];

    for (name, expected_ver) in required {
        let found = pkgs.iter().find(|(n, _, _)| n == name);
        assert!(
            found.is_some(),
            "Required crate {} not found in C61 wifi dependency graph",
            name
        );
        let (_, ver, _) = found.unwrap();
        assert_eq!(
            ver, expected_ver,
            "Crate {} version mismatch in C61 wifi graph",
            name
        );
    }

    // Feature assertions
    let esp_radio = pkgs.iter().find(|(n, _, _)| n == "esp-radio").unwrap();
    assert!(
        esp_radio.2.split(',').any(|f| f.trim() == "esp32c61"),
        "esp-radio must have feature esp32c61"
    );
    assert!(
        !esp_radio.2.split(',').any(|f| f.trim() == "esp32c3"),
        "esp-radio must NOT have feature esp32c3 on C61"
    );

    let smoltcp = pkgs.iter().find(|(n, _, _)| n == "smoltcp").unwrap();
    assert!(
        !smoltcp.2.split(',').any(|f| f.trim() == "proto-ipv6"),
        "smoltcp must not have proto-ipv6 (IPv4 only)"
    );
}

#[test]
fn x4_wifi_dependency_set_matches_pinned_spec() {
    let pkgs = cargo_tree_pkgs("riscv32imc-unknown-none-elf", "board-x4,wifi");

    let esp_radio = pkgs.iter().find(|(n, _, _)| n == "esp-radio").unwrap();
    assert_eq!(esp_radio.1, "1.0.0-beta.1");

    let esp_rtos = pkgs.iter().find(|(n, _, _)| n == "esp-rtos").unwrap();
    assert_eq!(esp_rtos.1, "0.4.0");
}

#[test]
fn cargo_toml_pins_esp_radio_optimization_level() {
    let root = workspace_root();
    let toml = fs::read_to_string(root.join("Cargo.toml")).expect("read Cargo.toml");

    assert!(
        toml.contains("[profile.dev.package.esp-radio]\nopt-level = 3")
            || toml.contains("[profile.dev.package.esp-radio]\nopt-level = 2"),
        "dev profile must optimize esp-radio with level 2 or 3"
    );

    assert!(
        toml.contains("[profile.release.package.esp-radio]\nopt-level = 3")
            || toml.contains("[profile.release.package.esp-radio]\nopt-level = 2"),
        "release profile must optimize esp-radio with level 2 or 3"
    );
}

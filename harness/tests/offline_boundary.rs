// Pure Rust verification that offline firmware does not link radio/network stack
// and carries no upload UI strings (replaces scripts/check-offline-boundary.sh).

use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

const RADIO_SYMBOLS: &[&str] = &["esp_radio", "esp_wifi", "embassy_net", "smoltcp"];
const UPLOAD_STRINGS: &[&str] = &[
    "pulp.local",
    "WiFi config error!",
    "No WiFi credentials!",
    "Connection failed!",
];

fn probe_elf_file(elf_path: &Path) -> (usize, usize, bool) {
    let bytes = fs::read(elf_path).expect("read elf");
    let obj = object::File::parse(&*bytes).expect("parse elf");

    let mut radio_syms = 0;
    for sym in obj.symbols() {
        // Ignore absolute symbols (e.g. ROM symbols)
        if sym.kind() == SymbolKind::Data || sym.kind() == SymbolKind::Text {
            if let Ok(name) = sym.name() {
                for pat in RADIO_SYMBOLS {
                    if name.contains(pat) {
                        radio_syms += 1;
                        break;
                    }
                }
            }
        }
    }

    let mut found_upload_strings = 0;
    let mut found_upload_menu = false;

    // Scan all data/rodata/text sections for raw string occurrences
    for section in obj.sections() {
        if let Ok(data) = section.data() {
            for pat in UPLOAD_STRINGS {
                if data.windows(pat.len()).any(|w| w == pat.as_bytes()) {
                    found_upload_strings += 1;
                }
            }
            // Check for exact "\0Upload\0" or menu entry
            if data
                .windows(b"\0Upload\0".len())
                .any(|w| w == b"\0Upload\0")
            {
                found_upload_menu = true;
            }
        }
    }

    (radio_syms, found_upload_strings, found_upload_menu)
}

#[test]
fn offline_c61_elf_has_no_radio_symbols_or_strings() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/c61/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            let (syms, strs, menu) = probe_elf_file(elf);
            assert_eq!(
                syms,
                0,
                "Offline C61 ELF {} contained radio symbols",
                elf.display()
            );
            assert_eq!(
                strs,
                0,
                "Offline C61 ELF {} contained upload strings",
                elf.display()
            );
            assert!(
                !menu,
                "Offline C61 ELF {} contained 'Upload' menu entry",
                elf.display()
            );
            return;
        }
    }
}

#[test]
fn offline_x4_elf_has_no_radio_symbols_or_strings() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/x4/riscv32imc-unknown-none-elf/release/pulp-os"),
        root.join("target/riscv32imc-unknown-none-elf/release/pulp-os"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            let (syms, strs, menu) = probe_elf_file(elf);
            assert_eq!(
                syms,
                0,
                "Offline X4 ELF {} contained radio symbols",
                elf.display()
            );
            assert_eq!(
                strs,
                0,
                "Offline X4 ELF {} contained upload strings",
                elf.display()
            );
            assert!(
                !menu,
                "Offline X4 ELF {} contained 'Upload' menu entry",
                elf.display()
            );
            return;
        }
    }
}

#[test]
fn wifi_c61_elf_control_has_radio_symbols() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/wifi-build/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/wifi-build/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/accept/offline-boundary/c61-wifi/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            let (syms, strs, _) = probe_elf_file(elf);
            assert!(
                syms > 0,
                "Wifi C61 ELF {} should contain radio symbols",
                elf.display()
            );
            assert!(
                strs > 0,
                "Wifi C61 ELF {} should contain upload strings",
                elf.display()
            );
            return;
        }
    }
}

#[test]
fn offline_dependency_tree_excludes_radio() {
    let root = workspace_root();
    let configs = [
        ("riscv32imac-unknown-none-elf", "board-onepage-c61"),
        ("riscv32imc-unknown-none-elf", "board-x4"),
    ];

    for (target, features) in configs {
        let output = Command::new("cargo")
            .current_dir(&root)
            .args([
                "tree",
                "--locked",
                "-e",
                "normal",
                "-i",
                "esp-radio",
                "--target",
                target,
                "--features",
                features,
            ])
            .output()
            .expect("cargo tree");

        let text =
            String::from_utf8_lossy(&output.stderr) + String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains("did not match any packages"),
            "Target {} features {} unexpectedly includes esp-radio:\n{}",
            target,
            features,
            text
        );
    }
}

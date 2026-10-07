// Pure Rust verification that OnePage C61 firmware has zero dependency on C3 raw GPIO
// register paths (replaces scripts/check-c61-no-c3-raw-gpio.sh).

use object::{Object, ObjectSection, ObjectSymbol};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn c61_source_tree_free_of_c3_raw_gpio() {
    let root = workspace_root();
    let src_paths = [
        "kernel/src/board_c61",
        "board-logic/src",
        "kernel/src/drivers/sdcard.rs",
        "kernel/src/drivers/storage.rs",
        "kernel/src/drivers/mod.rs",
        "kernel/src/error.rs",
        "src/bin/c61_boot.rs",
        "src/bin/main_c61.rs",
        "kernel/src/kernel",
        "kernel/src/ui",
        "kernel/src/util",
        "kernel/src/drivers/strip.rs",
        "src/apps",
        "src/ui",
    ];

    let patterns = [
        "raw_gpio",
        "RawOutputPin",
        "GPIO_OUT_W1TS",
        "GPIO_OUT_W1TC",
        "GPIO_ENABLE_W1TS",
        "IO_MUX_BASE",
    ];

    let mut violations = Vec::new();

    fn check_file(path: &Path, patterns: &[&str], violations: &mut Vec<String>) {
        let content = fs::read_to_string(path).expect("read file");
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for pat in patterns {
                if line.contains(pat) {
                    violations.push(format!("{}:{}: {}", path.display(), line_no + 1, line));
                }
            }
            // Check for C3 GPIO register literal 0x60004...
            if line.contains("0x6000_4") || line.contains("0x60004") {
                violations.push(format!("{}:{}: {}", path.display(), line_no + 1, line));
            }
        }
    }

    fn walk_dir(dir: &Path, patterns: &[&str], violations: &mut Vec<String>) {
        if dir.is_file() {
            check_file(dir, patterns, violations);
            return;
        }
        for entry in fs::read_dir(dir).expect("read dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, patterns, violations);
            } else if path.extension().is_some_and(|e| e == "rs") {
                check_file(&path, patterns, violations);
            }
        }
    }

    for p in &src_paths {
        walk_dir(&root.join(p), &patterns, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "Found C3 raw GPIO references in C61 compile set:\n{}",
        violations.join("\n")
    );
}

#[test]
fn kernel_module_structure_gates_raw_gpio_to_x4() {
    let root = workspace_root();
    let kernel_lib = fs::read_to_string(root.join("kernel/src/lib.rs")).expect("read lib.rs");

    let mut found_cfg = false;
    let lines: Vec<&str> = kernel_lib.lines().collect();
    for i in 0..lines.len() {
        if lines[i].trim_start().starts_with("pub mod board;") {
            if i > 0 && lines[i - 1].contains("feature = \"board-x4\"") {
                found_cfg = true;
                break;
            }
        }
    }
    assert!(found_cfg, "kernel::board must be cfg-gated to board-x4");

    let board_mod =
        fs::read_to_string(root.join("kernel/src/board/mod.rs")).expect("read board/mod.rs");
    assert!(
        board_mod.contains("pub mod raw_gpio;"),
        "raw_gpio must only be declared in board/mod.rs"
    );
}

fn probe_elf_file(elf_path: &Path) -> (usize, usize) {
    let bytes = fs::read(elf_path).expect("read elf");
    let obj = object::File::parse(&*bytes).expect("parse elf");

    let mut raw_gpio_syms = 0;
    for sym in obj.symbols() {
        if let Ok(name) = sym.name() {
            if name.contains("raw_gpio") || name.contains("RawOutputPin") {
                raw_gpio_syms += 1;
            }
        }
    }

    let mut c3_lui_count = 0;
    for section in obj.sections() {
        if section.name() == Ok(".text") {
            let data = section.data().unwrap_or(&[]);
            let mut i = 0;
            while i + 4 <= data.len() {
                let inst = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
                // RISC-V lui rd, imm: opcode = 0b0110111 (0x37), imm = inst[31:12]
                if (inst & 0x7f) == 0x37 && (inst >> 12) == 0x60004 {
                    c3_lui_count += 1;
                }
                // RISC-V 16-bit compressed instruction support: if lowest 2 bits != 0b11, it's 16-bit
                if (inst & 0x3) != 0x3 {
                    i += 2;
                } else {
                    i += 4;
                }
            }
        }
    }

    (raw_gpio_syms, c3_lui_count)
}

#[test]
fn c61_elfs_contain_no_c3_raw_gpio() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/c61/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/accept/c61/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot"),
        root.join("target/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot"),
    ];

    let mut checked_any = false;
    for elf in &candidates {
        if elf.is_file() {
            checked_any = true;
            let (syms, lui) = probe_elf_file(elf);
            assert_eq!(
                syms,
                0,
                "ELF {} unexpectedly contained raw_gpio symbols",
                elf.display()
            );
            assert_eq!(
                lui,
                0,
                "ELF {} unexpectedly contained C3 lui 0x60004 instructions",
                elf.display()
            );
        }
    }

    assert!(checked_any, "No C61 ELF found to verify");
}

#[test]
fn x4_control_trips_probes_if_present() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/x4/riscv32imc-unknown-none-elf/release/pulp-os"),
        root.join("target/riscv32imc-unknown-none-elf/release/pulp-os"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            let (syms, lui) = probe_elf_file(elf);
            assert!(
                syms > 0,
                "X4 control ELF {} expected to have raw_gpio symbols",
                elf.display()
            );
            assert!(
                lui > 0,
                "X4 control ELF {} expected to have lui 0x60004 instructions",
                elf.display()
            );
            return;
        }
    }
}

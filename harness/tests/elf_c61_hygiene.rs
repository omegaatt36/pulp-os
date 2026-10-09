// The C61 build must not depend on the C3-only raw GPIO register path
// (kernel/src/board/raw_gpio.rs: GPIO_OUT_W1TS 0x6000_4008, GPIO_ENABLE 0x6000_4024,
// IO_MUX 0x6000_9000, out-sel 0x6000_4554, used for X4's GPIO12 SD CS). Four probes:
//   1. source: no C3 raw register constants / raw_gpio users in any file the C61 build
//      compiles;
//   2. structure: `mod board;` (which owns raw_gpio) is gated to board-x4;
//   3. ELF symbols: no raw_gpio / RawOutputPin symbol in the C61 images;
//   4. ELF code: no `lui rd, 0x60004` (C3 GPIO block) in the C61 images.
// Positive control: the X4 image must trip probes 3 and 4.

mod common;

use common::*;
use object::{Object, ObjectSection, ObjectSymbol};
use std::fs;

/// Every file the C61 build compiles (the full firmware also compiles the scheduler,
/// the apps and the UI).
const C61_SOURCES: &[&str] = &[
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
const C3_NAMES: &[&str] = &[
    "raw_gpio",
    "RawOutputPin",
    "GPIO_OUT_W1TS",
    "GPIO_OUT_W1TC",
    "GPIO_ENABLE_W1TS",
    "IO_MUX_BASE",
];

/// A `0x6000_xxxx` / `0x6000xxxx` register literal (4 hex digits after the prefix).
fn has_peripheral_literal(line: &str) -> bool {
    let mut rest = line;
    while let Some(i) = rest.find("0x6000") {
        let tail = rest[i + 6..].strip_prefix('_').unwrap_or(&rest[i + 6..]);
        if tail
            .chars()
            .take(4)
            .filter(|c| c.is_ascii_hexdigit())
            .count()
            == 4
        {
            return true;
        }
        rest = &rest[i + 6..];
    }
    false
}

#[test]
fn source_probe_recognizes_its_targets() {
    assert!(has_peripheral_literal("const A: u32 = 0x6000_4008;"));
    assert!(has_peripheral_literal("let p = 0x60004024 as *mut u32;"));
    assert!(!has_peripheral_literal("let x = 0x6000;"));
}

#[test]
fn c61_compile_set_has_no_c3_raw_gpio_source() {
    let root = workspace_root();
    let files: Vec<_> = C61_SOURCES
        .iter()
        .flat_map(|p| rust_files(&root.join(p)))
        .collect();
    let hits: Vec<String> = code_lines(&files)
        .into_iter()
        .filter(|(_, _, l)| C3_NAMES.iter().any(|n| l.contains(n)) || has_peripheral_literal(l))
        .map(|(p, n, l)| format!("{}:{n}: {l}", p.display()))
        .collect();
    assert!(
        hits.is_empty(),
        "C3 raw GPIO references in the C61 compile set:\n{}",
        hits.join("\n")
    );
}

#[test]
fn raw_gpio_is_reachable_only_through_the_x4_gated_board_module() {
    let root = workspace_root();
    let lib = fs::read_to_string(root.join("kernel/src/lib.rs")).expect("read kernel/src/lib.rs");
    let lines: Vec<&str> = lib.lines().collect();
    let gated = lines.iter().enumerate().any(|(i, l)| {
        l.trim_start().starts_with("pub mod board;")
            && i > 0
            && lines[i - 1].contains("feature = \"board-x4\"")
    });
    assert!(
        gated,
        "kernel `mod board` (owner of raw_gpio) is not cfg(board-x4)"
    );
    let board =
        fs::read_to_string(root.join("kernel/src/board/mod.rs")).expect("read board/mod.rs");
    assert!(
        board.contains("pub mod raw_gpio;"),
        "raw_gpio declaration moved"
    );
}

/// (raw_gpio symbols, `lui rd, 0x60004` instructions in executable sections)
fn probe_elf(img: Image) -> (usize, usize) {
    let bytes = read_elf(&image(img));
    let obj = parse_elf(&bytes);
    let syms = obj
        .symbols()
        .filter_map(|s| s.name().ok())
        .filter(|n| n.contains("raw_gpio") || n.contains("RawOutputPin"))
        .count();
    let mut lui = 0;
    for sec in obj.sections() {
        if section_flags(sec.flags()) & u64::from(object::elf::SHF_EXECINSTR) == 0 {
            continue;
        }
        let data = sec.data().unwrap_or(&[]);
        let mut i = 0;
        while i + 2 <= data.len() {
            if data[i] & 0x3 != 0x3 {
                i += 2; // 16-bit compressed instruction
                continue;
            }
            if i + 4 > data.len() {
                break;
            }
            let inst = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
            // lui rd, imm: opcode 0b0110111, imm = inst[31:12]
            if inst & 0x7f == 0x37 && inst >> 12 == 0x60004 {
                lui += 1;
            }
            i += 4;
        }
    }
    (syms, lui)
}

#[test]
fn c61_images_contain_no_c3_raw_gpio() {
    for img in [
        Image::C61,
        Image::C61Boot,
        Image::C61Partial,
        Image::C61PartialWifi,
    ] {
        let (syms, lui) = probe_elf(img);
        assert_eq!(syms, 0, "raw_gpio symbols in a C61 image");
        assert_eq!(lui, 0, "`lui 0x60004` (C3 GPIO block) in a C61 image");
    }
}

#[test]
fn x4_control_trips_both_elf_probes() {
    let (syms, lui) = probe_elf(Image::X4);
    assert!(
        syms > 0,
        "control probe finds no raw_gpio symbols (probe broken?)"
    );
    assert!(
        lui > 0,
        "control probe finds no `lui 0x60004` (probe broken?)"
    );
}

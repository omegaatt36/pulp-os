// Pure Rust memory budget and layout verification for the OnePage C61 image
// (replaces scripts/report-c61-memory.sh).

use object::{Object, ObjectSection, ObjectSymbol};
use pulp_board_logic::memory::{
    INTERNAL_HEAP_RECLAIMED_BYTES, ImageMemory, check_image, range_is_internal,
    section_placement_ok,
};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn verify_c61_elf_memory(elf_path: &Path) {
    let bytes = fs::read(elf_path).expect("read elf");
    let obj = object::File::parse(&*bytes).expect("parse elf");

    let ram_lo = 0x4080_0000usize;
    let ram_hi = 0x4083_EA70usize;

    let mut static_end = 0usize;
    let mut stack_addr = 0usize;
    let mut stack_size = 0usize;

    for sec in obj.sections() {
        let name = sec.name().unwrap_or("");
        let addr = sec.address() as usize;
        let size = sec.size() as usize;
        if size == 0 {
            continue;
        }

        let (is_alloc, is_writable) = match sec.flags() {
            object::SectionFlags::Elf { sh_flags } => (
                (sh_flags & (object::elf::SHF_ALLOC as u64)) != 0,
                (sh_flags & (object::elf::SHF_WRITE as u64)) != 0,
            ),
            _ => (false, false),
        };

        // Only inspect allocated sections (loaded into memory at runtime)
        if !is_alloc {
            continue;
        }

        if addr >= ram_lo && addr < ram_hi {
            if name == ".stack" {
                stack_addr = addr;
                stack_size = size;
            } else {
                let end = addr + size;
                if end > static_end {
                    static_end = end;
                }
            }
        }

        // Section placement rules from board-logic: writable data must be in internal RAM
        assert!(
            section_placement_ok(addr, size, is_writable),
            "ELF {} section {} at 0x{:08x} (+{} B, writable={}) placement failed",
            elf_path.display(),
            name,
            addr,
            size,
            is_writable
        );
    }

    // Image budget rules (statics + stack)
    let img = ImageMemory {
        static_end,
        stack_addr,
        stack_size,
    };
    let img_check = check_image(&img);
    assert!(
        img_check.is_ok(),
        "ELF {} image budget check failed: {:?}",
        elf_path.display(),
        img_check.err()
    );

    // Verify SPI DMA buffer symbols are in internal RAM
    let mut dma_symbols = 0;
    for sym in obj.symbols() {
        if let Ok(name) = sym.name() {
            if name.contains("board_c61")
                && name.contains("spi")
                && (name.contains("BUFFER") || name.contains("DESCRIPTORS"))
            {
                dma_symbols += 1;
                let addr = sym.address() as usize;
                let size = sym.size() as usize;
                assert!(
                    range_is_internal(addr, size.max(1)),
                    "DMA symbol {} at 0x{:08x} (+{}) is not in internal RAM",
                    name,
                    addr,
                    size
                );
            }
        }
    }
    assert!(
        dma_symbols >= 4,
        "Expected >= 4 SPI DMA symbols, found {}",
        dma_symbols
    );

    // Check .dram2_uninit equals bootloader-reclaimed region (64,000 B)
    if let Some(dram2) = obj.section_by_name(".dram2_uninit") {
        assert_eq!(
            dram2.size() as usize,
            INTERNAL_HEAP_RECLAIMED_BYTES,
            ".dram2_uninit section size must match INTERNAL_HEAP_RECLAIMED_BYTES"
        );
    }
}

#[test]
fn c61_offline_elf_memory_budget() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/c61/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            verify_c61_elf_memory(elf);
            return;
        }
    }
}

#[test]
fn c61_wifi_elf_memory_budget() {
    let root = workspace_root();
    let candidates = [
        root.join("target/accept/wifi-build/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
        root.join("target/wifi-build/riscv32imac-unknown-none-elf/release/pulp-os-c61"),
    ];

    for elf in &candidates {
        if elf.is_file() {
            verify_c61_elf_memory(elf);
            return;
        }
    }
}

#[test]
fn c61_source_has_no_external_psram_leaks() {
    let root = workspace_root();
    let memory_rs = fs::read_to_string(root.join("kernel/src/board_c61/memory.rs"))
        .expect("read board_c61/memory.rs");

    let ext_count = memory_rs
        .lines()
        .filter(|l| l.contains("External.into()"))
        .count();
    assert_eq!(
        ext_count, 1,
        "Expected exactly one External.into() in board_c61/memory.rs"
    );

    assert!(
        memory_rs.contains("PSRAM_HEAP.add_region"),
        "PSRAM_HEAP.add_region must exist"
    );

    // Any HEAP.add_region that is NOT PSRAM_HEAP.add_region is forbidden
    let other_heap_add = memory_rs
        .lines()
        .filter(|l| l.contains("HEAP.add_region") && !l.contains("PSRAM_HEAP.add_region"))
        .count();
    assert_eq!(
        other_heap_add, 0,
        "PSRAM must never be added to global HEAP"
    );
}

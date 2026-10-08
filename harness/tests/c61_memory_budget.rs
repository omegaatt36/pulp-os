// Memory budget and layout of the OnePage C61 images. The rules are
// `pulp_board_logic::memory` (the code the firmware links), applied to the linked ELF.

mod common;

use common::*;
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind, SymbolSection};
use pulp_board_logic::memory::{
    C61_EXTMEM_END, C61_EXTMEM_START, C61_RAM_END, C61_RAM_LEN, C61_RAM_START, C61_RECLAIMED_START,
    INTERNAL_HEAP_RECLAIMED_BYTES, ImageMemory, check_image, internal_heap_main_bytes,
    range_is_internal, section_placement_ok,
};
use std::fs;
use std::path::PathBuf;

fn symbol_addr(obj: &object::File<'_>, name: &str) -> usize {
    obj.symbols()
        .find(|s| s.name() == Ok(name))
        .unwrap_or_else(|| panic!("symbol {name} missing"))
        .address() as usize
}

fn verify_c61_image(img: Image, wifi: bool) {
    let path = image(img);
    let bytes = read_elf(&path);
    let obj = parse_elf(&bytes);
    let ram_hi = C61_RAM_START + C61_RAM_LEN;

    // statics end / `.stack`, and the placement rule of every allocated section
    let (mut static_end, mut stack_addr, mut stack_size) = (0usize, 0usize, 0usize);
    for sec in obj.sections() {
        let flags = section_flags(sec.flags());
        let (addr, size) = (sec.address() as usize, sec.size() as usize);
        if size == 0 || flags & u64::from(object::elf::SHF_ALLOC) == 0 {
            continue;
        }
        let name = sec.name().unwrap_or("");
        if (C61_RAM_START..ram_hi).contains(&addr) {
            if name == ".stack" {
                (stack_addr, stack_size) = (addr, size);
            } else {
                static_end = static_end.max(addr + size);
            }
        }
        let writable = flags & u64::from(object::elf::SHF_WRITE) != 0;
        assert!(
            section_placement_ok(addr, size, writable),
            "{}: section {name} at 0x{addr:08x} (+{size} B, writable={writable}) is misplaced",
            path.display()
        );
    }
    let layout = ImageMemory {
        static_end,
        stack_addr,
        stack_size,
    };
    check_image(&layout).unwrap_or_else(|e| panic!("{}: image budget: {e:?}", path.display()));

    // `_stack_start` / `_stack_end` agree with the `.stack` section and the memory map
    assert_eq!(
        symbol_addr(&obj, "_stack_end"),
        stack_addr,
        "_stack_end != .stack start"
    );
    assert_eq!(
        symbol_addr(&obj, "_stack_start"),
        stack_addr + stack_size,
        "_stack_start != .stack end"
    );
    assert_eq!(
        symbol_addr(&obj, "_stack_start"),
        C61_RECLAIMED_START,
        "_stack_start != C61_RECLAIMED_START"
    );

    // the planned internal heaps are really in the image
    let main_bytes = internal_heap_main_bytes(wifi);
    let main_heaps = obj
        .symbols()
        .filter(|s| s.kind() == SymbolKind::Data && s.size() as usize == main_bytes)
        .filter(|s| s.name().is_ok_and(|n| n.contains("HEAP")))
        .count();
    assert!(
        main_heaps >= 1,
        "no HEAP static of {main_bytes} B (variant wifi={wifi})"
    );
    let dram2 = obj
        .section_by_name(".dram2_uninit")
        .expect(".dram2_uninit missing");
    assert_eq!(
        dram2.size() as usize,
        INTERNAL_HEAP_RECLAIMED_BYTES,
        ".dram2_uninit != INTERNAL_HEAP_RECLAIMED_BYTES"
    );

    // SPI DMA buffers and descriptors live in internal RAM
    let dma: Vec<_> = obj
        .symbols()
        .filter(|s| {
            s.name().is_ok_and(|n| {
                n.contains("board_c61")
                    && n.contains("spi")
                    && (n.contains("BUFFER") || n.contains("DESCRIPTORS"))
            })
        })
        .collect();
    assert!(
        dma.len() >= 4,
        "expected >= 4 SPI DMA symbols (2 BUFFER + 2 DESCRIPTORS), found {}",
        dma.len()
    );
    for s in &dma {
        let (addr, size) = (s.address() as usize, (s.size() as usize).max(1));
        assert!(
            range_is_internal(addr, size),
            "DMA symbol {:?} at 0x{addr:08x} is not in internal RAM",
            s.name()
        );
    }

    // no writable data symbol inside the flash/PSRAM window
    let in_window: Vec<_> = obj
        .symbols()
        .filter(|s| s.kind() == SymbolKind::Data)
        .filter(|s| (C61_EXTMEM_START as u64..C61_EXTMEM_END as u64).contains(&s.address()))
        .filter(|s| match s.section() {
            SymbolSection::Section(i) => obj.section_by_index(i).is_ok_and(|sec| {
                section_flags(sec.flags()) & u64::from(object::elf::SHF_WRITE) != 0
            }),
            _ => false,
        })
        .collect();
    assert!(
        in_window.is_empty(),
        "{} writable data symbols inside the flash/PSRAM window",
        in_window.len()
    );
}

#[test]
fn c61_offline_image_meets_the_memory_budget() {
    verify_c61_image(Image::C61, false);
}

#[test]
fn c61_boot_image_meets_the_memory_budget() {
    verify_c61_image(Image::C61Boot, false);
}

#[test]
fn c61_wifi_image_meets_the_memory_budget() {
    verify_c61_image(Image::C61Wifi, true);
}

/// `0x...` literal after `marker` in `text`
fn hex_after(text: &str, marker: &str) -> usize {
    let i = text
        .find(marker)
        .unwrap_or_else(|| panic!("`{marker}` not found in memory.x"));
    let rest = &text[i + marker.len()..];
    let digits: String = rest
        .trim_start()
        .trim_start_matches("0x")
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    usize::from_str_radix(&digits, 16).unwrap_or_else(|_| panic!("no hex number after `{marker}`"))
}

fn esp_hal_memory_x() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cargo"));
    for registry in
        fs::read_dir(cargo_home.join("registry/src")).expect("cargo registry (run `cargo fetch`)")
    {
        let p = registry
            .expect("entry")
            .path()
            .join("esp-hal-1.2.0/ld/esp32c61/memory.x");
        if p.is_file() {
            return p;
        }
    }
    panic!("esp-hal-1.2.0 memory.x not found in the cargo registry (run `cargo fetch`)");
}

#[test]
fn board_logic_memory_map_literals_match_esp_hal_memory_x() {
    let text = fs::read_to_string(esp_hal_memory_x()).expect("read memory.x");
    let ram = text
        .lines()
        .find(|l| l.trim_start().starts_with("RAM : ORIGIN"))
        .expect("RAM line");
    assert_eq!(hex_after(ram, "ORIGIN ="), C61_RAM_START, "RAM ORIGIN");
    assert_eq!(hex_after(ram, "LENGTH ="), C61_RAM_LEN, "RAM LENGTH");
    let dram2 = text
        .lines()
        .find(|l| l.contains("dram2_seg") && l.contains("0x4084"))
        .expect("dram2_seg line");
    assert_eq!(hex_after(dram2, "len ="), C61_RAM_END, "dram2 end");
    let drom = text
        .lines()
        .find(|l| l.contains("\"DROM\""))
        .expect("DROM line");
    // `[0x42000000, 0x46000000, "DROM"],`
    let bounds: Vec<&str> = drom
        .trim()
        .trim_start_matches('[')
        .split(',')
        .map(str::trim)
        .collect();
    let parse =
        |s: &str| usize::from_str_radix(s.trim_start_matches("0x"), 16).expect("DROM bound");
    assert_eq!(parse(bounds[0]), C61_EXTMEM_START, "DROM start");
    assert_eq!(parse(bounds[1]), C61_EXTMEM_END, "DROM end");
}

#[test]
fn psram_is_registered_only_in_the_private_psram_heap() {
    let root = workspace_root();
    let memory_rs = root.join("kernel/src/board_c61/memory.rs");
    // PSRAM must stay off the global heap (`psram_allocator!` would add an External region
    // to esp_alloc::HEAP, where plain Box/Vec could land in it)
    let files: Vec<_> = ["src", "kernel/src"]
        .iter()
        .flat_map(|p| rust_files(&root.join(p)))
        .filter(|f| f != &memory_rs)
        .collect();
    let leaks: Vec<String> = code_lines(&files)
        .into_iter()
        .filter(|(_, _, l)| {
            l.contains("psram_allocator!") || l.contains("MemoryCapability::External")
        })
        .map(|(p, n, _)| format!("{}:{n}", p.display()))
        .collect();
    assert!(
        leaks.is_empty(),
        "PSRAM registered outside board_c61::memory: {leaks:?}"
    );

    let lines: Vec<_> = code_lines(&[memory_rs])
        .into_iter()
        .map(|(_, _, l)| l)
        .collect();
    let externals = lines
        .iter()
        .filter(|l| l.contains("External.into()"))
        .count();
    assert_eq!(
        externals, 1,
        "expected exactly one External region registration"
    );
    assert!(
        lines.iter().any(|l| l.contains("PSRAM_HEAP.add_region")),
        "PSRAM_HEAP.add_region missing"
    );
    let other = lines
        .iter()
        .filter(|l| l.contains("HEAP.add_region") && !l.contains("PSRAM_HEAP.add_region"))
        .count();
    assert_eq!(
        other, 0,
        "a region was added to a heap other than PSRAM_HEAP"
    );
}

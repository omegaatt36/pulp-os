// Human-readable report of the C61 memory budget (the rules themselves are checked
// against the linked images by harness/tests/c61_memory_budget.rs).
//
//   cargo run -p pulp-board-logic --example memreport --target host-tuple \
//     --config 'unstable.build-std=["std","test"]' -- constants offline|wifi
//   ... -- inventory      the large-allocation inventory as a markdown table
//
// `constants` prints the budget constants as NAME=value; the internal heap values
// are those of the chosen build variant.

use pulp_board_logic::memory::*;

fn constants(wifi: bool) {
    let kv: &[(&str, usize)] = &[
        ("C61_RAM_START", C61_RAM_START),
        ("C61_RAM_LEN", C61_RAM_LEN),
        ("C61_RECLAIMED_START", C61_RECLAIMED_START),
        ("C61_RECLAIMED_LEN", C61_RECLAIMED_LEN),
        ("C61_RAM_END", C61_RAM_END),
        ("C61_EXTMEM_START", C61_EXTMEM_START),
        ("C61_EXTMEM_END", C61_EXTMEM_END),
        ("STACK_MIN_BYTES", STACK_MIN_BYTES),
        ("STATIC_RAM_MAX_BYTES", STATIC_RAM_MAX_BYTES),
        ("INTERNAL_HEAP_MAIN_BYTES", internal_heap_main_bytes(wifi)),
        (
            "INTERNAL_HEAP_RECLAIMED_BYTES",
            INTERNAL_HEAP_RECLAIMED_BYTES,
        ),
        ("INTERNAL_HEAP_BYTES", internal_heap_bytes(wifi)),
        ("PSRAM_HW_BYTES", PSRAM_HW_BYTES),
        ("PSRAM_MAX_BYTES", PSRAM_MAX_BYTES),
        ("PSRAM_MIN_BYTES", PSRAM_MIN_BYTES),
        ("PSRAM_RESERVE_BYTES", PSRAM_RESERVE_BYTES),
        ("PSRAM_CHAPTER_TEXT_BYTES", PSRAM_CHAPTER_TEXT_BYTES),
        ("PSRAM_IMAGE_DATA_BYTES", PSRAM_IMAGE_DATA_BYTES),
        ("PSRAM_PAGE_TABLE_BYTES", PSRAM_PAGE_TABLE_BYTES),
        ("PSRAM_ZIP_TOC_BYTES", PSRAM_ZIP_TOC_BYTES),
        ("PSRAM_FONT_GLYPHS_BYTES", PSRAM_FONT_GLYPHS_BYTES),
        ("PSRAM_NET_SCRATCH_BYTES", PSRAM_NET_SCRATCH_BYTES),
        ("INTERNAL_DMA_BYTES", INTERNAL_DMA_BYTES),
        ("INTERNAL_ISR_BYTES", INTERNAL_ISR_BYTES),
        ("INTERNAL_RUNTIME_BYTES", INTERNAL_RUNTIME_BYTES),
        ("INTERNAL_CHAPTER_TEXT_BYTES", INTERNAL_CHAPTER_TEXT_BYTES),
        ("INTERNAL_IMAGE_DATA_BYTES", INTERNAL_IMAGE_DATA_BYTES),
        ("INTERNAL_PAGE_TABLE_BYTES", INTERNAL_PAGE_TABLE_BYTES),
        ("INTERNAL_ZIP_TOC_BYTES", INTERNAL_ZIP_TOC_BYTES),
        ("INTERNAL_FONT_GLYPHS_BYTES", INTERNAL_FONT_GLYPHS_BYTES),
        ("INTERNAL_NET_SCRATCH_BYTES", INTERNAL_NET_SCRATCH_BYTES),
        ("FLASH_MHZ", FLASH_MHZ as usize),
        ("PSRAM_MHZ", PSRAM_MHZ as usize),
    ];
    println!("BUILD_VARIANT={}", if wifi { "wifi" } else { "offline" });
    for (k, v) in kv {
        println!("{k}={v}");
    }
}

fn inventory() {
    println!("| # | allocation | where | kind | bytes | class | placement | note |");
    println!("|---|---|---|---|---|---|---|---|");
    for (i, it) in INVENTORY.iter().enumerate() {
        println!(
            "| {} | {} | {} | {:?} | {} | {} | {:?} | {} |",
            i + 1,
            it.name,
            it.source,
            it.kind,
            it.bytes,
            it.class.name(),
            it.placement,
            it.note
        );
    }
    println!();
    println!(
        "PSRAM candidates per class (rows marked 'in READER' included; they are limits-relevant when moved):"
    );
    let st = PsramStatus::Ready {
        bytes: PSRAM_HW_BYTES,
    };
    for class in [
        MemClass::ChapterText,
        MemClass::ImageData,
        MemClass::PageTable,
        MemClass::ZipToc,
        MemClass::FontGlyphs,
        MemClass::NetScratch,
    ] {
        let sum: usize = INVENTORY
            .iter()
            .filter(|i| i.placement == Placement::Candidate && i.class == class)
            .map(|i| i.bytes)
            .sum();
        println!(
            "- {}: candidates {} B of PSRAM limit {} B",
            class.name(),
            sum,
            class_limit(st, Region::Psram, class)
        );
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("constants") => match std::env::args().nth(2).as_deref() {
            Some("offline") => constants(false),
            Some("wifi") => constants(true),
            _ => {
                eprintln!("usage: memreport constants offline|wifi");
                std::process::exit(2);
            }
        },
        Some("inventory") => inventory(),
        _ => {
            eprintln!("usage: memreport constants offline|wifi | inventory");
            std::process::exit(2);
        }
    }
}

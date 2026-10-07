// Pure Rust Static Call Graph Stack Analyzer for RISC-V 32-bit Firmware.
// Reads ELF .stack_sizes, .symtab, and disassembles direct calls to calculate
// exact call graph depth, worst-case stack consumption, and bottlenecks.

use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct Func {
    addr: u32,
    size: u32,
    name: String,
    frame_size: u32,
    callees: BTreeSet<u32>,
}

fn decode_jal_offset(inst: u32) -> i32 {
    let imm20 = ((inst >> 31) & 1) as i32;
    let imm10_1 = ((inst >> 21) & 0x3ff) as i32;
    let imm11 = ((inst >> 20) & 1) as i32;
    let imm19_12 = ((inst >> 12) & 0xff) as i32;
    let val = (imm20 << 20) | (imm19_12 << 12) | (imm11 << 11) | (imm10_1 << 1);
    // sign extend from bit 20
    if (val & (1 << 20)) != 0 {
        val | !0x1f_ffff
    } else {
        val
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let elf_path = args.get(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("target/riscv32imac-unknown-none-elf/release/pulp-os-c61")
    });

    if !elf_path.is_file() {
        eprintln!("ELF not found: {}", elf_path.display());
        std::process::exit(1);
    }

    let bytes = fs::read(&elf_path)?;
    let obj = object::File::parse(&*bytes)?;

    let mut sym_map: BTreeMap<u32, (String, u32)> = BTreeMap::new();
    for sym in obj.symbols() {
        if sym.kind() == SymbolKind::Text && sym.size() > 0 {
            if let Ok(name) = sym.name() {
                if !name.starts_with('$') && !name.starts_with(".L") {
                    sym_map.insert(sym.address() as u32, (name.to_string(), sym.size() as u32));
                }
            }
        }
    }

    // Parse .stack_sizes section
    let mut stack_sizes: BTreeMap<u32, u32> = BTreeMap::new();
    if let Some(sec) = obj.section_by_name(".stack_sizes") {
        let data = sec.data()?;
        let mut cursor = 0;
        while cursor + 4 <= data.len() {
            let addr = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let mut val: u32 = 0;
            let mut shift = 0;
            while cursor < data.len() {
                let b = data[cursor];
                cursor += 1;
                val |= ((b & 0x7f) as u32) << shift;
                if (b & 0x80) == 0 {
                    break;
                }
                shift += 7;
            }
            stack_sizes.insert(addr, val);
        }
    } else {
        eprintln!(
            "Warning: .stack_sizes section missing. Rebuild with RUSTFLAGS=\"-Z emit-stack-sizes\""
        );
    }

    let text_sec = obj.section_by_name(".text").expect("missing .text section");
    let text_data = text_sec.data()?;
    let text_addr = text_sec.address() as u32;

    let mut functions: BTreeMap<u32, Func> = BTreeMap::new();
    for (&addr, (name, size)) in &sym_map {
        let frame = stack_sizes.get(&addr).copied().unwrap_or(0);
        functions.insert(
            addr,
            Func {
                addr,
                size: *size,
                name: name.clone(),
                frame_size: frame,
                callees: BTreeSet::new(),
            },
        );
    }

    // Scan instructions inside each function to find direct JAL calls
    for (&addr, func) in &mut functions {
        if addr < text_addr || addr + func.size > text_addr + text_data.len() as u32 {
            continue;
        }
        let start = (addr - text_addr) as usize;
        let end = start + func.size as usize;
        let code = &text_data[start..end];

        let mut i = 0;
        while i + 4 <= code.len() {
            let pc = addr + i as u32;
            let inst = u32::from_le_bytes(code[i..i + 4].try_into().unwrap());

            // 32-bit JAL: opcode 0x6f (0b1101111)
            if (inst & 0x7f) == 0x6f {
                let rd = (inst >> 7) & 0x1f;
                // rd == 1 (ra) or rd == 5 (t0) is a function call
                if rd == 1 || rd == 5 {
                    let offset = decode_jal_offset(inst);
                    let target = (pc as i32 + offset) as u32;
                    if sym_map.contains_key(&target) {
                        func.callees.insert(target);
                    }
                }
            }

            // Next instruction: 16-bit compressed or 32-bit regular
            if (inst & 0x3) != 0x3 {
                i += 2;
            } else {
                i += 4;
            }
        }
    }

    println!("================================================================================");
    println!(
        "RISC-V 32 Static Call Graph & Stack Analysis for: {}",
        elf_path.display()
    );
    println!("================================================================================");
    println!("Total analyzed functions: {}", functions.len());
    println!(
        "Total functions with .stack_sizes metadata: {}",
        stack_sizes.len()
    );

    // Compute Worst-Case Stack Depth (WCSD) via memoization / DFS
    fn compute_wcsd(
        addr: u32,
        funcs: &BTreeMap<u32, Func>,
        memo: &mut BTreeMap<u32, (u32, Vec<u32>)>,
        visiting: &mut BTreeSet<u32>,
    ) -> (u32, Vec<u32>) {
        if let Some(res) = memo.get(&addr) {
            return res.clone();
        }
        if visiting.contains(&addr) {
            // recursion detected
            return (funcs.get(&addr).map_or(0, |f| f.frame_size), vec![addr]);
        }
        visiting.insert(addr);

        let func = match funcs.get(&addr) {
            Some(f) => f,
            None => {
                visiting.remove(&addr);
                return (0, vec![]);
            }
        };

        let mut max_callee_depth = 0;
        let mut max_path = Vec::new();

        for &callee in &func.callees {
            let (cdepth, cpath) = compute_wcsd(callee, funcs, memo, visiting);
            if cdepth > max_callee_depth {
                max_callee_depth = cdepth;
                max_path = cpath;
            }
        }

        visiting.remove(&addr);
        let total_depth = func.frame_size + max_callee_depth;
        let mut full_path = vec![addr];
        full_path.extend(max_path);

        memo.insert(addr, (total_depth, full_path.clone()));
        (total_depth, full_path)
    }

    let mut memo = BTreeMap::new();
    let mut visiting = BTreeSet::new();

    let mut depths: Vec<(u32, u32, Vec<u32>)> = Vec::new(); // (total_stack, root_addr, path)
    for &addr in functions.keys() {
        let (depth, path) = compute_wcsd(addr, &functions, &mut memo, &mut visiting);
        depths.push((depth, addr, path));
    }
    depths.sort_by(|a, b| b.0.cmp(&a.0));

    println!("\nTop 15 Call Chains with Largest Worst-Case Cumulative Stack Depth:");
    println!("--------------------------------------------------------------------------------");
    for (depth, root, path) in depths.iter().take(15) {
        let root_name = &functions[root].name;
        println!("\n▶ Cumulative Stack: {:5} B | Entry: {}", depth, root_name);
        println!("  Call path (depth {} hops):", path.len());
        for (idx, &hop) in path.iter().enumerate() {
            let f = &functions[&hop];
            println!("    [{:2}] +{:5} B  {}", idx, f.frame_size, f.name);
        }
    }

    // Specific entry point evaluations:
    println!("\n================================================================================");
    println!("Key Firmware Subsystem Stack High-Water Projections:");
    println!("================================================================================");

    let subsystems = [
        (
            "Embassy Main Task Poll",
            "embassy_executor::raw::TaskStorage",
        ),
        ("Reader App Image Decode", "decode_image_streaming"),
        ("Files App EPUB Scan", "scan_one_epub_title"),
        ("Directory Cache Load", "DirCache::ensure_loaded"),
        ("SD Card Init/Bringup", "sd::bring_up"),
        ("Wi-Fi HTTP Process", "HttpServer::process_bytes"),
    ];

    for (label, pattern) in subsystems {
        let matching = depths
            .iter()
            .filter(|(_, addr, _)| functions[addr].name.contains(pattern))
            .max_by_key(|(d, _, _)| *d);

        if let Some((depth, addr, path)) = matching {
            let f = &functions[addr];
            println!(
                "• {:<26}: Projected Max: {:5} B (Frame: {:5} B, Callees: {:5} B, Path Hops: {})",
                label,
                depth,
                f.frame_size,
                depth - f.frame_size,
                path.len()
            );
        } else {
            println!("• {:<26}: (not directly identified in symbol table)", label);
        }
    }

    let overall_max = depths.first().map(|d| d.0).unwrap_or(0);
    println!("\nAbsolute Worst-Case Call Stack Depth: {} B", overall_max);
    println!("Assigned Linker Stack Allocation:     51,888 B");
    println!(
        "Projected Headroom over Worst Stack:  {} B ({:.1}% unused margin)",
        51888 - overall_max as usize,
        (51888 - overall_max as usize) as f64 / 51888.0 * 100.0
    );

    Ok(())
}

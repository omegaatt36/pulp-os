// stack-analyzer must never report a stack verdict from an ELF without `.stack_sizes`
// (R3): missing metadata means the stack usage is unknown, not zero. The fixtures are
// minimal hand-built ELF32 little-endian RISC-V objects, one function, with or without
// the `.stack_sizes` section.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const FUNC_ADDR: u32 = 0x1000;
const FRAME_SIZE: u8 = 32;

struct Section {
    name: &'static str,
    kind: u32,
    addr: u32,
    data: Vec<u8>,
    link: u32,
    info: u32,
    entsize: u32,
}

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// One function `f` at FUNC_ADDR (a single `ret`), optionally with a `.stack_sizes`
/// entry giving it a FRAME_SIZE byte frame.
fn build_elf(with_stack_sizes: bool) -> Vec<u8> {
    let mut symtab = vec![0u8; 16]; // null symbol
    u32le(&mut symtab, 1); // st_name -> "f"
    u32le(&mut symtab, FUNC_ADDR);
    u32le(&mut symtab, 4); // st_size
    symtab.push(0x12); // GLOBAL | FUNC
    symtab.push(0);
    u16le(&mut symtab, 1); // st_shndx = .text

    let mut sections = vec![
        Section {
            name: ".text",
            kind: 1,
            addr: FUNC_ADDR,
            data: 0x0000_8067u32.to_le_bytes().to_vec(), // ret
            link: 0,
            info: 0,
            entsize: 0,
        },
        Section {
            name: ".symtab",
            kind: 2,
            addr: 0,
            data: symtab,
            link: 3,
            info: 1,
            entsize: 16,
        },
        Section {
            name: ".strtab",
            kind: 3,
            addr: 0,
            data: b"\0f\0".to_vec(),
            link: 0,
            info: 0,
            entsize: 0,
        },
    ];
    if with_stack_sizes {
        let mut data = FUNC_ADDR.to_le_bytes().to_vec();
        data.push(FRAME_SIZE); // ULEB128
        sections.push(Section {
            name: ".stack_sizes",
            kind: 1,
            addr: 0,
            data,
            link: 0,
            info: 0,
            entsize: 0,
        });
    }

    // .shstrtab is last; section index 0 is the null section.
    let mut shstrtab = vec![0u8];
    let mut name_offs = Vec::new();
    for s in &sections {
        name_offs.push(shstrtab.len() as u32);
        shstrtab.extend_from_slice(s.name.as_bytes());
        shstrtab.push(0);
    }
    let shstrtab_name = shstrtab.len() as u32;
    shstrtab.extend_from_slice(b".shstrtab\0");
    sections.push(Section {
        name: ".shstrtab",
        kind: 3,
        addr: 0,
        data: shstrtab,
        link: 0,
        info: 0,
        entsize: 0,
    });
    name_offs.push(shstrtab_name);

    let mut elf = vec![0u8; 52];
    let mut offsets = Vec::new();
    for s in &sections {
        elf.resize(elf.len().next_multiple_of(4), 0);
        offsets.push(elf.len() as u32);
        elf.extend_from_slice(&s.data);
    }
    elf.resize(elf.len().next_multiple_of(4), 0);
    let shoff = elf.len() as u32;
    elf.extend_from_slice(&[0u8; 40]); // null section header
    for (i, s) in sections.iter().enumerate() {
        u32le(&mut elf, name_offs[i]);
        u32le(&mut elf, s.kind);
        u32le(&mut elf, 0); // flags
        u32le(&mut elf, s.addr);
        u32le(&mut elf, offsets[i]);
        u32le(&mut elf, s.data.len() as u32);
        u32le(&mut elf, s.link);
        u32le(&mut elf, s.info);
        u32le(&mut elf, if s.entsize == 16 { 4 } else { 1 }); // addralign
        u32le(&mut elf, s.entsize);
    }

    let mut header = Vec::new();
    header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 1, 1, 1, 0]);
    header.extend_from_slice(&[0u8; 8]);
    u16le(&mut header, 2); // ET_EXEC
    u16le(&mut header, 243); // EM_RISCV
    u32le(&mut header, 1); // e_version
    u32le(&mut header, 0); // e_entry
    u32le(&mut header, 0); // e_phoff
    u32le(&mut header, shoff);
    u32le(&mut header, 0); // e_flags
    u16le(&mut header, 52); // e_ehsize
    u16le(&mut header, 0); // e_phentsize
    u16le(&mut header, 0); // e_phnum
    u16le(&mut header, 40); // e_shentsize
    u16le(&mut header, sections.len() as u16 + 1); // e_shnum
    u16le(&mut header, sections.len() as u16); // e_shstrndx (last)
    elf[..52].copy_from_slice(&header);
    elf
}

fn run_analyzer(with_stack_sizes: bool) -> Output {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("stack_analyzer_fixture_{with_stack_sizes}.elf"));
    fs::write(&path, build_elf(with_stack_sizes)).expect("write fixture ELF");
    Command::new(env!("CARGO_BIN_EXE_stack-analyzer"))
        .arg(&path)
        .output()
        .expect("run stack-analyzer")
}

#[test]
fn missing_stack_sizes_exits_nonzero_and_reports_unknown() {
    let out = run_analyzer(false);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "ELF without .stack_sizes must exit non-zero, got {:?}",
        out.status
    );
    assert!(
        stderr.contains("unknown"),
        "stderr must report the stack usage as unknown, got: {stderr}"
    );
}

#[test]
fn missing_stack_sizes_prints_no_stack_verdict() {
    let out = run_analyzer(false);
    let stdout = String::from_utf8_lossy(&out.stdout);
    for forbidden in ["Worst", "Headroom", "margin", "%"] {
        assert!(
            !stdout.contains(forbidden),
            "stdout must not carry a stack verdict ({forbidden:?}), got:\n{stdout}"
        );
    }
}

#[test]
fn present_stack_sizes_still_analyzes() {
    let out = run_analyzer(true);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "ELF with .stack_sizes must succeed, got {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("Absolute Worst-Case Call Stack Depth: 32 B"),
        "worst stack of the single 32 B frame must be reported, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Projected Headroom over Worst Stack:  51856 B"),
        "headroom must be reported, got:\n{stdout}"
    );
}

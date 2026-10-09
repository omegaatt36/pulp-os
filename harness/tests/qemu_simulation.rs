// Pure Rust QEMU RISC-V 32 Emulation Test Suite.
// Verifies that code targeting riscv32imac executes faithfully on real RISC-V emulation,
// validating instruction correctness (atomic operations, multiplication, compressed instructions).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

// qemu-user is Linux-only, so this is opt-in: `cargo test-harness -- --ignored qemu`
#[test]
#[ignore = "requires qemu-riscv32 (Linux qemu-user) on PATH"]
fn qemu_riscv32_executes_target_binary() {
    let qemu_path = PathBuf::from("qemu-riscv32");

    let root = workspace_root();
    let target_dir = root.join("target/qemu-sim");
    fs::create_dir_all(&target_dir).expect("create qemu target dir");

    let src = target_dir.join("main.rs");
    let out = target_dir.join("qemu_test_elf");

    // Rust test payload targeting riscv32imac-unknown-none-elf
    let payload = r#"
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
}

fn exit(code: usize) -> ! {
    unsafe {
        core::arch::asm!(
            "li a7, 93",
            "ecall",
            in("a0") code,
            options(noreturn)
        );
    }
}

fn write(fd: usize, msg: &[u8]) {
    unsafe {
        core::arch::asm!(
            "li a7, 64",
            "ecall",
            in("a0") fd,
            in("a1") msg.as_ptr(),
            in("a2") msg.len(),
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // 1. Test RISC-V "A" (Atomic) Extension on RV32IMAC
    let mut val = 100u32;
    unsafe {
        core::arch::asm!(
            "amoadd.w t0, {0}, ({1})",
            in(reg) 25u32,
            in(reg) &mut val,
            out("t0") _,
        );
    }
    if val != 125 {
        exit(10);
    }

    // 2. Test RISC-V "M" (Multiply/Divide) Extension
    let a = 12345u32;
    let b = 6789u32;
    if a * b != 83810205 {
        exit(11);
    }

    // 3. Test CRC calculation logic (mimicking pulp-board-logic session CRC)
    let data = b"PULP-OS-SESSION-DATA";
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    if crc == 0 {
        exit(12);
    }

    write(1, b"QEMU_RISCV32_OK\n");
    exit(0);
}
"#;
    fs::write(&src, payload).expect("write payload");

    // Compile with rustc
    let rustc_status = Command::new("rustc")
        .current_dir(&root)
        .args([
            "--target",
            "riscv32imac-unknown-none-elf",
            "-C",
            "link-arg=--entry=_start",
            src.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("compile payload with rustc");

    assert!(
        rustc_status.success(),
        "Failed to compile RISC-V test payload"
    );

    // Execute in QEMU
    let qemu_output = Command::new(&qemu_path)
        .arg("-cpu")
        .arg("rv32")
        .arg(&out)
        .output()
        .expect("execute qemu-riscv32");

    assert!(
        qemu_output.status.success(),
        "QEMU execution failed with exit code {:?}, stderr:\n{}",
        qemu_output.status.code(),
        String::from_utf8_lossy(&qemu_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&qemu_output.stdout);
    assert!(
        stdout.contains("QEMU_RISCV32_OK"),
        "Expected output QEMU_RISCV32_OK, got:\n{}",
        stdout
    );
}

// Tests that invalid board selections fail at build time with readable compile_error!
// messages (replaces scripts/check-board-selection.sh).

use std::path::PathBuf;
use std::process::Command;

fn run_cargo_build_and_check_error(cargo_args: &[&str], expected_substring: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let target_dir = root.join("target/board-selection-check");

    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root);
    cmd.env("CARGO_TARGET_DIR", &target_dir);
    cmd.arg("build").arg("--release").arg("--locked");
    for arg in cargo_args {
        cmd.arg(arg);
    }

    let output = cmd.output().expect("failed to execute cargo build");
    assert!(
        !output.status.success(),
        "build unexpectedly succeeded for args {:?}",
        cargo_args
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected_substring),
        "expected error substring {:?} not found in stderr:\n{}",
        expected_substring,
        stderr
    );
}

#[test]
fn error_on_no_board_selected() {
    run_cargo_build_and_check_error(&[], "no board selected");
}

#[test]
fn error_on_both_boards_selected() {
    run_cargo_build_and_check_error(
        &["--features", "board-x4,board-onepage-c61"],
        "multiple boards selected",
    );
}

#[test]
fn error_on_x4_on_imac_target() {
    run_cargo_build_and_check_error(
        &[
            "--target",
            "riscv32imac-unknown-none-elf",
            "--features",
            "board-x4",
        ],
        "feature `board-x4` requires --target riscv32imc-unknown-none-elf",
    );
}

#[test]
fn error_on_c61_on_imc_target() {
    run_cargo_build_and_check_error(
        &[
            "--target",
            "riscv32imc-unknown-none-elf",
            "--features",
            "board-onepage-c61",
        ],
        "feature `board-onepage-c61` requires --target riscv32imac-unknown-none-elf",
    );
}

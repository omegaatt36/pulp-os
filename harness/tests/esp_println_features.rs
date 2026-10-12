// The console transport is fixed by the `esp-println` feature set in the resolved
// dependency graph. C61 uses `jtag-serial` only: `auto` picks UART0 or USB-Serial-JTAG at
// run time from the USB SOF flag, and a cold boot without USB falls to UART0, whose
// default pins GPIO10/GPIO11 are the charge-control / USB-detect lines of the board.

mod common;

use common::*;

/// Features of `esp-println` enabled in one build configuration (`cargo tree -e features`).
fn esp_println_features(target: &str) -> Vec<String> {
    let (ok, text) = cargo(&[
        "tree",
        "--locked",
        "-e",
        "features",
        "-i",
        "esp-println",
        "--target",
        target,
    ]);
    assert!(ok, "cargo tree failed:\n{text}");
    let mut features: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let rest = line.split("esp-println feature \"").nth(1)?;
            Some(rest.split('"').next()?.to_string())
        })
        .collect();
    features.sort();
    features.dedup();
    assert!(
        !features.is_empty(),
        "no esp-println features parsed:\n{text}"
    );
    features
}

#[test]
fn c61_console_is_jtag_serial_only() {
    let f = esp_println_features(C61_TARGET);
    assert!(
        f.iter().any(|x| x == "jtag-serial"),
        "esp-println lacks jtag-serial on C61: {f:?}"
    );
    for forbidden in ["auto", "uart"] {
        assert!(
            !f.iter().any(|x| x == forbidden),
            "esp-println enables `{forbidden}` on C61: {f:?}"
        );
    }
}

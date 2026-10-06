// X4 EPD driver wire-trace lock: the real kernel sources
// (kernel/src/drivers/{ssd1677,strip}.rs, included unmodified) are driven
// through init, a full frame, 7 partial regions (both phase-1 variants,
// phase 3), deep sleep and re-init, and the recorded SPI/DC/RST/delay trace
// is compared byte-for-byte against the committed golden file. The pin proves
// the driver sources still emit the pre-port trace (the pure sequences and
// the strip layout moved into pulp-board-logic). Was
// scripts/check-x4-driver-trace.sh; an intentional X4 change refreshes the
// golden deliberately (UPDATE_GOLDEN=1).
#![allow(dead_code)]

#[path = "../../kernel/src/drivers/ssd1677.rs"]
pub mod ssd1677;
#[path = "../../kernel/src/drivers/strip.rs"]
pub mod strip;
// the stim reaches the drivers through `crate::drivers`, as in the kernel
pub mod drivers {
    pub use crate::{ssd1677, strip};
}

#[path = "x4_driver_trace/stim.rs"]
mod stim;
#[path = "x4_driver_trace/ui.rs"]
pub mod ui;

#[path = "goldens/check.rs"]
mod goldens;

#[test]
fn driver_wire_trace_matches_golden() {
    goldens::check(
        "tests/goldens/x4_driver_trace.txt",
        include_str!("goldens/x4_driver_trace.txt"),
        &stim::run(),
    );
}

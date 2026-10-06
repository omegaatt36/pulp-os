// X4 input state-machine lock: the real kernel sources (drivers/input.rs,
// board/button.rs, kernel/timing.rs, included unmodified) are host-driven
// with a controllable microsecond clock and a fake ADC / power pin through
// scripted debounce / long-press / repeat scenarios plus a 300k-step seeded
// random walk, and the event/timing/hardware-read trace is compared
// byte-for-byte against the committed golden file (which also locks the
// pulp-board-logic `InputCore` the X4 source drives). Was
// scripts/check-x4-input-trace.sh; an intentional X4 change refreshes the
// golden deliberately (UPDATE_GOLDEN=1).
#![allow(dead_code)]

// the included sources' `crate::` paths, with the kernel's module tree
#[path = "x4_input_trace/board.rs"]
pub mod board;
#[path = "../../kernel/src/board/button.rs"]
pub mod button;
#[path = "../../kernel/src/drivers/input.rs"]
pub mod input;
#[path = "../../kernel/src/kernel/timing.rs"]
pub mod timing;
pub mod kernel {
    pub use crate::timing;
}
pub mod drivers {
    pub use crate::input;
}

#[path = "x4_input_trace/stim.rs"]
mod stim;

#[path = "goldens/check.rs"]
mod goldens;

#[test]
fn input_trace_matches_golden() {
    goldens::check(
        "tests/goldens/x4_input_trace.txt",
        include_str!("goldens/x4_input_trace.txt"),
        &stim::run(),
    );
}

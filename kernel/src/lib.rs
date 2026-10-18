// pulp-kernel -- hardware drivers, scheduling, and system core
//
// generic over AppLayer; never imports concrete apps or fonts
// ships a built-in mono font (FONT_9X18) for boot console and
// sleep screen; distros bring their own proportional fonts

#![no_std]
#![feature(allocator_api)]

extern crate alloc;

// The only board is the OnePage C61 (esp32c61, target riscv32imac); `cargo build-c61`.
// Hardware-facing modules. `board_c61` is the OnePage C61 board support
// (`board_c61::api`, `board_c61::hw`); `board` is the board-neutral surface the
// kernel and the apps compile against (screen size, key mapper, display type).
// Pure logic lives in pulp-board-logic.
pub mod board_c61;
pub use board_c61::api as board;
pub mod drivers;
pub mod error;
pub mod kernel;
pub mod ui;
pub mod util;

// re-export core error types at crate root
pub use error::{Error, ErrorKind, Result, ResultExt};

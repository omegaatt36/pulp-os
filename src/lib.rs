// pulp-os - e-reader firmware for the OnePage C61

#![no_std]

extern crate alloc;

// The board-specific parts are behind `pulp_kernel::board`
// (kernel/src/board_c61/api.rs); the apps are written against that surface.
pub use pulp_kernel::error;
pub use pulp_kernel::{board, drivers, kernel};

pub mod apps;
pub mod fonts;
pub mod ui;

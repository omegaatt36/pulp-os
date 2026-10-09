// pulp-board-logic -- HAL-free board logic, built for both the firmware
// targets and the host (host tests: `cargo test-board-logic`).
//
// Rule for this crate: no esp-hal, no alloc, no cfg on board features. Hardware
// is reached only through traits implemented by the kernel's board modules.

#![no_std]

pub mod action;
pub mod battery;
pub mod chapter_ring;
pub mod font_index;
pub mod image_lru;
pub mod input;
pub mod keys;
pub mod lifecycle;
pub mod memory;
pub mod page_index;
pub mod pins;
pub mod power;
pub mod sd;
pub mod sd_cache;
pub mod session;
pub mod sleep;
pub mod source_id;
pub mod spi;
pub mod ssd1677;
pub mod strip;
pub mod upload;
pub mod usb;

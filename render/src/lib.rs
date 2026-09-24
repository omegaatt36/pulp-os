// pulp-render -- hardware-independent text and rendering core
//
// no esp-* or embassy dependencies: builds for the firmware target
// and for the host, where `cargo host-test` exercises it

#![no_std]

pub mod crc32;
pub mod font_pack;
pub mod geometry;
pub mod layout;
pub mod line_break;
pub mod pack_file;
pub mod page;
pub mod panel;
pub mod strip;
pub mod utf8;

// Host validation entry. Production sources are included in place with
// `#[path]`, so the host tests exercise exactly the code the firmware links:
// kernel error / util / ui / board button+action+layout / app+bookmark+config+
// handle+work_queue, drivers dir_entry+strip, and the app layer's reader,
// widgets and fonts. Only the hardware edge is a host stand-in (apps.rs,
// board.rs, drivers.rs, kernel.rs, ui.rs, reader.rs, storage.rs); none of
// them decides paging, wrapping, layout or bookmark behaviour.
//
// The firmware is two crates (pulp-kernel, pulp-os) and its files address each
// other as `pulp_kernel::...` and `crate::...`; this one crate answers to both.

extern crate alloc;
// `pulp_kernel::util::*` (reader, fonts) resolves to this crate
extern crate self as pulp_kernel;
// kernel/src/kernel/app.rs names `esp_hal::delay::Delay` in the AppLayer trait
extern crate self as esp_hal;

mod delay {
    #[derive(Clone, Copy, Default)]
    pub struct Delay;
}

#[path = "../../kernel/src/error.rs"]
pub mod error;
#[path = "../../kernel/src/util/mod.rs"]
pub mod util;
// kernel/src/ui: the part of `crate::ui` the kernel files address
#[path = "../../src/fonts/mod.rs"]
#[allow(dead_code)]
pub mod fonts;
#[path = "../../kernel/src/ui/mod.rs"]
#[allow(dead_code)]
mod kernel_ui;

pub mod apps;
pub mod board;
pub mod drivers;
pub mod fixtures;
pub mod kernel;
pub mod reader;
pub mod render;
pub mod storage;
pub mod ui;

pub use drivers::dir_entry;
pub use error::{Error, ErrorKind, Result, ResultExt};

pub mod utf8 {
    pub use crate::util::{Utf8Iter, decode_utf8_char};
}

#[cfg(test)]
mod tests {
    use super::utf8::{Utf8Iter, decode_utf8_char};

    #[test]
    fn decodes_multibyte_with_production_utf8() {
        let s = "a\u{e9}\u{4e2d}\u{1f600}";
        let chars: Vec<char> = Utf8Iter::new(s.as_bytes()).collect();
        assert_eq!(chars, s.chars().collect::<Vec<_>>());
        assert_eq!(decode_utf8_char(s.as_bytes(), 1), ('\u{e9}', 2));
        assert_eq!(decode_utf8_char(&[0xe4, 0xb8], 0), ('\u{FFFD}', 2));
    }
}

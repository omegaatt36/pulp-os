// utility modules: small, reusable components without hardware dependencies

mod close_cell;
mod shared;
mod utf8;

pub use close_cell::{CloseBorrow, CloseCell, CloseError, CloseHandle, CloseToken};
pub use shared::Shared;
pub use utf8::{Utf8Iter, decode_utf8_char, utf8_incomplete_tail_len, utf8_prefix_len};

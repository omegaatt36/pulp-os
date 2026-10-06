// utility modules: small, reusable components without hardware dependencies

mod utf8;

pub use utf8::{Utf8Iter, decode_utf8_char, utf8_incomplete_tail_len, utf8_prefix_len};

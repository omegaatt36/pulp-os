// SD file naming. The firmware SD driver has no long file name support, so a
// pack file name must be a valid FAT 8.3 short name; it is derived from the
// pixel size alone, no alloc.

use core::fmt;

// sub directory under the _PULP directory that holds the packs
pub const PACK_DIR: &str = "FONTS";

const NAME_LEN: usize = 10;

// "F" + 5 decimal digits + ".PFN"; fixed width keeps the mapping injective
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFileName([u8; NAME_LEN]);

impl PackFileName {
    pub fn as_str(&self) -> &str {
        // always ASCII by construction
        core::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl fmt::Display for PackFileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn pack_file_name(pixel_size: u16) -> PackFileName {
    let mut name = *b"F00000.PFN";
    let mut rest = pixel_size;
    for slot in name[1..6].iter_mut().rev() {
        *slot = b'0' + (rest % 10) as u8;
        rest /= 10;
    }
    PackFileName(name)
}

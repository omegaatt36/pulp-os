//! Write the EPUB used to check vertical writing on the device.
//!
//! usage: device-fixtures [OUT_DIR]        (default: target/device-fixtures)
//!
//! Copy `VERTRUBY.EPU` to the root of the SD card (FAT 8.3 name). The book is
//! described in `pulp_host::fixtures::device`.
use std::path::PathBuf;

use pulp_host::fixtures::device::vertical_book;

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/device-fixtures".into()),
    );
    std::fs::create_dir_all(&out).expect("create the output directory");
    let path = out.join("VERTRUBY.EPU");
    let bytes = vertical_book();
    std::fs::write(&path, &bytes).expect("write the book");
    println!("{}: {} bytes", path.display(), bytes.len());
}

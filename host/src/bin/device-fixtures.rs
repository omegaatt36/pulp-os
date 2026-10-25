//! Write the EPUBs used to check ruby and vertical writing on the device.
//!
//! usage: device-fixtures [OUT_DIR]        (default: target/device-fixtures)
//!
//! Copy `VERTRUBY.EPU` (columns) and `HORIRUBY.EPU` (lines) to the root of the SD
//! card (FAT 8.3 names). The books are described in `pulp_host::fixtures::device`.
//! Readings need the ruby packs (`pulp-fontconv bundle`) and the Ruby setting On.
use std::path::PathBuf;

use pulp_host::fixtures::device::{horizontal_book, vertical_book};

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/device-fixtures".into()),
    );
    std::fs::create_dir_all(&out).expect("create the output directory");
    for (name, bytes) in [
        ("VERTRUBY.EPU", vertical_book()),
        ("HORIRUBY.EPU", horizontal_book()),
    ] {
        let path = out.join(name);
        std::fs::write(&path, &bytes).expect("write the book");
        println!("{}: {} bytes", path.display(), bytes.len());
    }
}

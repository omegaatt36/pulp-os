//! Write the EPUBs used to check vertical writing and progressive JPEG on the device.
//!
//! usage: device-fixtures [OUT_DIR]        (default: target/device-fixtures)
//!
//! Copy the `.EPU` files to the root of the SD card (FAT 8.3 names). The books are
//! described in `pulp_host::fixtures::device`; the progressive one needs the firmware
//! built with `--features progressive-jpeg` (`cargo run-c61-progressive`).
use std::path::PathBuf;

use pulp_host::fixtures::device::{jpeg_book, plasma, vertical_book};

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/device-fixtures".into()),
    );
    std::fs::create_dir_all(&out).expect("create the output directory");
    let rgb = plasma();
    for (name, bytes) in [
        ("VERTRUBY.EPU", vertical_book(&rgb)),
        ("PROGJPG.EPU", jpeg_book(&rgb)),
    ] {
        std::fs::write(out.join(name), &bytes).expect("write the book");
        println!("{}: {} bytes", out.join(name).display(), bytes.len());
    }
}

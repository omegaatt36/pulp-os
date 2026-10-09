// R6 (link-level evidence only): the full-firmware C61 images carry the periodic
// battery success log. The boot-time sample in `main_c61.rs` prints
// `battery: cell {} mV, {}%`; `log!` splits a format string into literal pieces,
// so the periodic `poll_battery` line is told apart by its own trailing piece
// `% (periodic)`. This proves the format string was linked into the image; it does
// NOT prove the line is emitted every 30 s -- only a hardware run shows that.

mod common;

use common::*;

const PERIODIC_PIECE: &[u8] = b"% (periodic)";

fn carries_periodic_piece(img: Image) -> bool {
    let bytes = read_elf(&image(img));
    let obj = parse_elf(&bytes);
    section_data(&obj)
        .into_iter()
        .flat_map(|d| ascii_runs(d, 4))
        .any(|run| run == PERIODIC_PIECE)
}

#[test]
fn full_firmware_c61_images_link_the_periodic_battery_success_log() {
    for img in [
        Image::C61,
        Image::C61Wifi,
        Image::C61Partial,
        Image::C61PartialWifi,
    ] {
        assert!(
            carries_periodic_piece(img),
            "no `{}` literal in the image: poll_battery has no success log",
            String::from_utf8_lossy(PERIODIC_PIECE)
        );
    }
}

#[test]
fn boot_image_keeps_its_own_battery_log_and_has_no_periodic_piece() {
    // control: the boot image has its own `battery: pin .. -> cell ..` log (R6 does
    // not touch it), so the piece must be specific to the full firmware
    assert!(!carries_periodic_piece(Image::C61Boot));
}

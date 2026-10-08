// The offline firmware (feature `wifi` off, the default) must not link any radio /
// network stack and must not carry the upload menu entry. The X4 and C61 images built
// with `--features wifi` are the positive controls: the same probes must find the radio
// there (and, on X4, the upload strings), otherwise the probes themselves are broken.

mod common;

use common::*;

const RADIO_CRATES: &[&str] = &[
    "esp-radio",
    "esp-radio-rtos-driver",
    "embassy-net",
    "smoltcp",
    "esp-wifi-sys",
    "esp-wifi-sys-esp32c61",
];
const RADIO_SYMBOLS: &[&str] = &["esp_radio", "esp_wifi", "embassy_net", "smoltcp"];
/// UI / server strings that exist only in upload mode
const UPLOAD_STRINGS: &[&str] = &[
    "pulp.local",
    "WiFi config error!",
    "No WiFi credentials!",
    "Connection failed!",
];

struct Probe {
    radio_symbols: usize,
    upload_strings: usize,
    upload_menu: bool,
}

fn probe(img: Image) -> Probe {
    let bytes = read_elf(&image(img));
    let obj = parse_elf(&bytes);
    let radio_symbols = defined_symbols(&obj)
        .iter()
        .filter(|n| RADIO_SYMBOLS.iter().any(|p| n.contains(p)))
        .count();
    let data = section_data(&obj);
    let upload_strings = UPLOAD_STRINGS
        .iter()
        .filter(|pat| {
            data.iter()
                .any(|d| d.windows(pat.len()).any(|w| w == pat.as_bytes()))
        })
        .count();
    let upload_menu = data
        .iter()
        .any(|d| ascii_runs(d, 4).iter().any(|run| *run == b"Upload"));
    Probe {
        radio_symbols,
        upload_strings,
        upload_menu,
    }
}

fn assert_no_radio(name: &str, img: Image) {
    let p = probe(img);
    assert_eq!(p.radio_symbols, 0, "{name}: radio/net symbols linked");
    assert_eq!(p.upload_strings, 0, "{name}: upload-mode strings present");
    assert!(!p.upload_menu, "{name}: 'Upload' menu label present");
}

#[test]
fn offline_images_link_no_radio_and_no_upload_ui() {
    assert_no_radio("x4 offline", Image::X4);
    assert_no_radio("c61 offline", Image::C61);
    assert_no_radio("c61 boot image", Image::C61Boot);
}

#[test]
fn x4_wifi_control_finds_radio_strings_and_menu() {
    let p = probe(Image::X4Wifi);
    assert!(
        p.radio_symbols > 0,
        "probe finds no radio symbols (probe broken?)"
    );
    assert!(
        p.upload_strings > 0,
        "probe finds no upload strings (probe broken?)"
    );
    assert!(
        p.upload_menu,
        "probe finds no 'Upload' menu label (probe broken?)"
    );
}

#[test]
fn c61_wifi_control_finds_radio_symbols() {
    let p = probe(Image::C61Wifi);
    assert!(
        p.radio_symbols > 0,
        "probe finds no radio symbols (probe broken?)"
    );
}

/// `cargo tree -i <krate>` for one build configuration.
fn tree_inverse(krate: &str, target: &str, features: &str) -> String {
    let (_, text) = cargo(&[
        "tree",
        "--locked",
        "-e",
        "normal",
        "-i",
        krate,
        "--target",
        target,
        "--features",
        features,
    ]);
    text
}

#[test]
fn offline_dependency_graphs_exclude_radio_crates() {
    for (target, features) in [(X4_TARGET, "board-x4"), (C61_TARGET, "board-onepage-c61")] {
        for krate in RADIO_CRATES {
            let text = tree_inverse(krate, target, features);
            assert!(
                text.contains("did not match any packages"),
                "{features} on {target}: {krate} is in the dependency graph:\n{text}"
            );
        }
    }
}

#[test]
fn wifi_dependency_graphs_contain_esp_radio() {
    for (target, features) in [
        (X4_TARGET, "board-x4,wifi"),
        (C61_TARGET, "board-onepage-c61,wifi"),
    ] {
        let text = tree_inverse("esp-radio", target, features);
        assert!(
            text.starts_with("esp-radio"),
            "{features} on {target}: esp-radio missing from the graph:\n{text}"
        );
    }
}

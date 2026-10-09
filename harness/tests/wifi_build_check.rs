// Wi-Fi build requirements: the fixed, C61-capable radio / RTOS set, radio optimization,
// IPv4-only network stack, and that the enabled C61 image really contains the radio driver.
// (The disabled-link and memory checks are offline_boundary.rs / c61_memory_budget.rs.)

mod common;

use common::*;

const C61_WIFI: (&str, &str) = (C61_TARGET, "board-onepage-c61,wifi");
const X4_WIFI: (&str, &str) = (X4_TARGET, "board-x4,wifi");

/// (name, version, features) of every package in the normal-dependency graph.
fn packages((target, features): (&str, &str)) -> Vec<(String, String, String)> {
    let (ok, text) = cargo(&[
        "tree",
        "--locked",
        "-e",
        "normal",
        "--target",
        target,
        "--features",
        features,
        "--prefix",
        "none",
        "--format",
        "{p}|{f}",
    ]);
    assert!(ok, "cargo tree failed:\n{text}");
    text.lines()
        .filter_map(|line| {
            let (pkg, feats) = line.split_once('|')?;
            let mut it = pkg.split_whitespace();
            let (name, version) = (it.next()?, it.next()?);
            Some((
                name.to_string(),
                version.trim_start_matches('v').to_string(),
                feats.trim_end_matches(" (*)").to_string(),
            ))
        })
        .collect()
}

fn version<'a>(pkgs: &'a [(String, String, String)], name: &str) -> &'a str {
    &pkgs
        .iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("{name} not in the dependency graph"))
        .1
}

fn has_feature(pkgs: &[(String, String, String)], name: &str, feature: &str) -> bool {
    let (_, _, feats) = pkgs
        .iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("{name} not in the dependency graph"));
    feats.split(',').any(|f| f.trim() == feature)
}

#[test]
fn c61_wifi_dependency_set_matches_the_pinned_set() {
    let pkgs = packages(C61_WIFI);
    for (name, want) in [
        ("esp-hal", "1.2.0"),
        ("esp-rtos", "0.4.0"),
        ("esp-alloc", "0.11.0"),
        ("esp-radio", "1.0.0-beta.1"),
        ("esp-bootloader-esp-idf", "0.6.0"),
        ("esp-radio-rtos-driver", "0.4.2"),
        ("esp-phy", "0.3.0"),
        ("esp-wifi-sys-esp32c61", "0.3.0"),
    ] {
        assert_eq!(version(&pkgs, name), want, "{name} version");
    }
    assert!(
        version(&pkgs, "embassy-net").starts_with("0.8."),
        "embassy-net must be 0.8.x"
    );
    assert!(
        has_feature(&pkgs, "esp-radio", "esp32c61"),
        "esp-radio needs feature esp32c61"
    );
    assert!(
        !has_feature(&pkgs, "esp-radio", "esp32c3"),
        "esp-radio must not have feature esp32c3 on C61"
    );
    // the service requires IPv4: the serving address is read with config_v4(), so an
    // IPv6-capable stack would let wait_config_up() return without an IPv4 address
    assert!(
        !has_feature(&pkgs, "smoltcp", "proto-ipv6"),
        "smoltcp must not have proto-ipv6"
    );
}

#[test]
fn x4_wifi_does_not_fall_back_to_the_stale_radio_set() {
    let pkgs = packages(X4_WIFI);
    assert_eq!(version(&pkgs, "esp-radio"), "1.0.0-beta.1");
    assert_eq!(version(&pkgs, "esp-rtos"), "0.4.0");
}

/// opt-level of every `esp_radio` library unit, from cargo's resolved unit graph.
fn radio_opt_levels((target, features): (&str, &str)) -> Vec<String> {
    let (ok, text) = cargo(&[
        "build",
        "--release",
        "--locked",
        "--unit-graph",
        "-Zunstable-options",
        "--target",
        target,
        "--features",
        features,
    ]);
    assert!(ok, "cargo build --unit-graph failed:\n{text}");
    // the graph is the JSON document; cargo may print notices before it
    let json = &text[text.find('{').expect("unit graph json")..];
    let graph: serde_json::Value = serde_json::Deserializer::from_str(json)
        .into_iter()
        .next()
        .expect("unit graph")
        .expect("parse unit graph");
    graph["units"]
        .as_array()
        .expect("units")
        .iter()
        .filter(|u| u["target"]["name"] == "esp_radio")
        .filter(|u| {
            u["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "lib"))
        })
        .map(|u| {
            u["profile"]["opt_level"]
                .as_str()
                .expect("opt_level")
                .to_string()
        })
        .collect()
}

#[test]
fn esp_radio_is_compiled_with_opt_level_2_or_3() {
    for cfg in [C61_WIFI, X4_WIFI] {
        let levels = radio_opt_levels(cfg);
        assert!(
            !levels.is_empty(),
            "{cfg:?}: no esp_radio unit in the unit graph"
        );
        for lvl in &levels {
            assert!(
                lvl == "2" || lvl == "3",
                "{cfg:?}: esp_radio opt-level {lvl} (want 2 or 3)"
            );
        }
    }
}

#[test]
fn c61_wifi_image_contains_the_radio_driver() {
    let bytes = read_elf(&image(Image::C61Wifi));
    let obj = parse_elf(&bytes);
    let names = defined_symbols(&obj);
    for sym in [
        "esp_wifi_start",
        "esp_wifi_init_internal",
        "esp_wifi_connect_internal",
    ] {
        assert!(
            names.iter().any(|n| *n == sym),
            "symbol {sym} missing (non-absolute)"
        );
    }
}

#[test]
fn c61_partial_wifi_image_contains_the_radio_driver() {
    let bytes = read_elf(&image(Image::C61PartialWifi));
    let obj = parse_elf(&bytes);
    let names = defined_symbols(&obj);
    for sym in [
        "esp_wifi_start",
        "esp_wifi_init_internal",
        "esp_wifi_connect_internal",
    ] {
        assert!(
            names.iter().any(|n| *n == sym),
            "symbol {sym} missing (non-absolute)"
        );
    }
}

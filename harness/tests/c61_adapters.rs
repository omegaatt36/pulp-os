// OnePage C61 shared-ADC / SD-probe adapter regression: the real
// board_c61/{adc,spi}.rs sources (included unmodified) host-compiled against
// the esp-hal seam.
#![allow(dead_code)]

use esp_hal::peripherals::*;
use std::marker::PhantomData;

#[path = "../../kernel/src/board_c61/adc.rs"]
pub mod adc;
#[path = "../../kernel/src/board_c61/spi.rs"]
pub mod spi;

// The seam's recorder is a process-wide resource shared by this file's tests,
// so every test body runs under the whole-test session lock (the default
// parallel harness is safe at any thread count).
#[test]
fn shared_adc_recovers_on_next_key_poll_without_channel_mixing() {
    esp_hal::session(|| {
        esp_hal::take_trace();
        let set = adc::init(ADC1(PhantomData), GPIO4(PhantomData), GPIO5(PhantomData)).unwrap();
        esp_hal::adc_ready(false);
        assert_eq!(set.adc.read_battery_mv(), None);
        assert_eq!(
            esp_hal::take_trace().len(),
            20_000,
            "battery wait must be bounded"
        );
        // Battery conversion finishes after its caller timed out. A key poll must
        // drain that conversion and return a NEW front-channel value.
        esp_hal::adc_ready(true);
        assert_eq!(set.adc.read_front_mv(), Some(1200));
        let trace = esp_hal::take_trace();
        assert_eq!(trace.first().map(String::as_str), Some("adc:3"));
        assert_eq!(trace.last().map(String::as_str), Some("adc:2"));
        // A hardware stall remains bounded and never leaks the battery result
        // into the key measurement, even across repeated key polls.
        esp_hal::adc_ready(false);
        assert_eq!(set.adc.read_battery_mv(), None);
        esp_hal::take_trace();
        for _ in 0..2 {
            assert_eq!(set.adc.read_front_mv(), None);
            let trace = esp_hal::take_trace();
            assert!(!trace.is_empty());
            assert!(trace.len() <= 20_000);
            assert!(trace.iter().all(|item| item == "adc:3"));
        }
        esp_hal::adc_ready(true);
        assert_eq!(set.adc.read_front_mv(), Some(1200));
    });
}

#[test]
fn each_sd_probe_prepares_clock_deselects_and_flushes_before_cmd0() {
    esp_hal::session(|| {
        let board = spi::init(
            SPI2(PhantomData),
            DMA_CH0(PhantomData),
            spi::SpiPins {
                sck: GPIO22(PhantomData),
                mosi: GPIO23(PhantomData),
                miso: GPIO24(PhantomData),
                epd_cs: GPIO25(PhantomData),
                sd_cs: GPIO26(PhantomData),
            },
        )
        .unwrap();
        esp_hal::take_trace();
        for _ in 0..2 {
            board.control.speed_up().unwrap();
            esp_hal::take_trace();
            board.control.prepare_sd_probe().unwrap();
            let trace = esp_hal::take_trace();
            assert_eq!(trace.first().map(String::as_str), Some("clock:400"));
            let write = trace
                .iter()
                .position(|s| s.starts_with("write:"))
                .expect("each probe needs at least 74 clocks, including hot insertion");
            assert!(
                trace[..write]
                    .iter()
                    .any(|s| s.starts_with("high:") && s.contains("GPIO25"))
            );
            assert!(
                trace[..write]
                    .iter()
                    .any(|s| s.starts_with("high:") && s.contains("GPIO26"))
            );
            assert!(!trace[..write].iter().any(|s| s.starts_with("low:")));
            assert_eq!(trace[write], format!("write:{:?}", [255u8; 10]));
            assert_eq!(trace.get(write + 1).map(String::as_str), Some("flush"));
        }
        esp_hal::fail_write(true);
        assert!(
            board.control.prepare_sd_probe().is_err(),
            "failed prelude must prevent card initialization"
        );
        esp_hal::fail_write(false);
        esp_hal::take_trace();
    });
}

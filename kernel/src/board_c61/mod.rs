// board support for the OnePage C61 (ESP32-C61)
//
// `pins`/`power`: pin ownership and the GPIO27 boot/runtime power contract.
// `spi`: SPI2/DMA bus with arbitrated EPD/SD devices, SD bring-up, card
//       detect and storage-error mapping (`sd`).
// `epd`: SSD1677 full refresh: esp-hal adapter over pulp_board_logic::ssd1677.
// `keys`: ADC ladder + 3 GPIO keys over pulp_board_logic::{keys, input}.
// `memory`: memory placement: PSRAM bring-up on a private heap, budget-checked
//       allocation, internal-only DMA buffers (policy in pulp_board_logic::memory).
// `adc`/`battery`/`usb`: shared ADC1 (keys GPIO4 + battery GPIO5 in one
//       AdcConfig), battery sampling with the charge-pause contract, USB detect.
// `session`: session persistence on SD: SessionStore adapter + save/restore
//       over pulp_board_logic::session (two slot files, CRC, SD-active gate).
// `sleep`: deep sleep: wake GPIO2, EPD park, line silencing and the
//       esp-hal sleep entry over pulp_board_logic::sleep (order + failure policy).
// `api` and `hw`: the `pulp_kernel::board` surface the kernel/apps compile
//       against, and what the kernel owns on this board (power rail, battery,
//       sleep parts, card detect).

pub mod adc;
pub mod api;
pub mod battery;
pub mod epd;
pub mod hw;
pub mod keys;
pub mod memory;
pub mod pins;
pub mod power;
pub mod sd;
pub mod session;
pub mod sleep;
pub mod spi;
pub mod usb;

pub use pins::Pins;
pub use pulp_board_logic::power::{
    DisplayReset, PeripheralPower, PowerError, RailState, SdInitPermit,
};

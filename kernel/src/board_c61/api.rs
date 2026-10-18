// Board-neutral surface of the OnePage C61.
//
// The kernel scheduler and the apps are written against `crate::board::*`
// `pulp_kernel::board` is this module:
//
//   board::{SCREEN_W, SCREEN_H}  logical portrait size (480 x 800)
//   board::Epd                   the full-refresh SSD1677 driver
//   board::button::Button        the physical key (C61 `Key`, no Power button)
//   board::action::ButtonMapper  key event -> semantic ActionEvent (swap setting)
//   board::layout::*             label slots of the front keys

use pulp_board_logic::ssd1677::{HEIGHT, WIDTH};

// app-lifecycle decisions the apps layer needs (quick-menu mapping)
pub use pulp_board_logic::lifecycle;

pub use super::epd::Epd;
pub use crate::drivers::sdcard::{SdStorage, SyncSdCard};
pub use crate::drivers::strip::StripBuffer;

use super::epd::FnStrips;

// Whole-screen refresh of a `draw` closure, so code outside the
// scheduler (the upload mode) can draw. The C61 has no
// partial refresh and the blocking driver needs no delay handle, so both are one
// full refresh. A failed refresh is only logged (no recovery outside the
// scheduler); the caller's next refresh tries again.
pub async fn full_refresh_screen<F>(
    epd: &mut Epd,
    strip: &mut StripBuffer<'_>,
    _delay: &mut esp_hal::delay::Delay,
    draw: &F,
) where
    F: Fn(&mut StripBuffer),
{
    let mut src = FnStrips { strip, draw };
    if let Err(e) = epd.full_refresh(&mut src) {
        log::warn!("display: full refresh failed: {}", e.as_str());
    }
}

pub async fn partial_refresh_screen<F>(
    epd: &mut Epd,
    strip: &mut StripBuffer<'_>,
    delay: &mut esp_hal::delay::Delay,
    draw: &F,
) where
    F: Fn(&mut StripBuffer),
{
    full_refresh_screen(epd, strip, delay, draw).await;
}

// logical screen size (portrait via 270-degree rotation of the 800x480 panel)
pub const SCREEN_W: u16 = HEIGHT; // 480
pub const SCREEN_H: u16 = WIDTH; // 800

pub use super::{action, button, layout};

// battery voltage estimation, generic over board calibration
// board-specific divider ratio and discharge curve live in board::battery;
// the voltage -> percent algorithm lives in pulp-board-logic (shared with the
// OnePage C61), pinned by the r16_* tests in board-logic/src/battery.rs.

use crate::board::battery::{DISCHARGE_CURVE, DIVIDER_MULT};

pub fn adc_to_battery_mv(adc_mv: u16) -> u16 {
    (adc_mv as u32 * DIVIDER_MULT) as u16
}

pub fn battery_percentage(battery_mv: u16) -> u8 {
    pulp_board_logic::battery::percentage_from_curve(DISCHARGE_CURVE, battery_mv as u32)
}

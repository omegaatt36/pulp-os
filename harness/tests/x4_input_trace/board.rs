// Stand-in for crate::board on the host: the same names input.rs uses
// (InputHw, power_button_is_low, button::*), with the real button.rs.
#[path = "../../../kernel/src/board/button.rs"]
pub mod button;

use std::sync::Mutex;

static INPUTS: Mutex<(bool, u16, u16)> = Mutex::new((false, 3300, 3300));
// (adc reads, power reads)
static COUNT: Mutex<(u64, u64)> = Mutex::new((0, 0));
pub fn set_inputs(p: bool, a: u16, b: u16) {
    *INPUTS.lock().unwrap_or_else(|e| e.into_inner()) = (p, a, b);
}
pub fn counters() -> (u64, u64) {
    *COUNT.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy)]
pub enum Row {
    One,
    Two,
    Battery,
}
pub struct AdcPin(pub Row);
pub struct Adc;
impl Adc {
    pub fn read_oneshot(&mut self, pin: &mut AdcPin) -> nb::Result<u16, ()> {
        {
            let mut c = COUNT.lock().unwrap_or_else(|e| e.into_inner());
            c.0 += 1;
        }
        let (_, a, b) = *INPUTS.lock().unwrap_or_else(|e| e.into_inner());
        Ok(match pin.0 {
            Row::One => a,
            Row::Two => b,
            Row::Battery => 4000,
        })
    }
}
pub struct InputHw {
    pub adc: Adc,
    pub row1: AdcPin,
    pub row2: AdcPin,
    pub battery: AdcPin,
}
impl InputHw {
    pub fn new() -> Self {
        InputHw {
            adc: Adc,
            row1: AdcPin(Row::One),
            row2: AdcPin(Row::Two),
            battery: AdcPin(Row::Battery),
        }
    }
}
pub fn power_button_is_low() -> bool {
    {
        let mut c = COUNT.lock().unwrap_or_else(|e| e.into_inner());
        c.1 += 1;
    }
    INPUTS.lock().unwrap_or_else(|e| e.into_inner()).0
}

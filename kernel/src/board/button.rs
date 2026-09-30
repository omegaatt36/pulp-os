// button definitions and ADC ladder decoding
//
// OnePage has a 4-way front-key ladder on GPIO4 (ADC1_CH2) plus three
// discrete active-low keys (GPIO6 PREV, GPIO9 NEXT, GPIO2 WAKE/power).
//
// The ladder node reads ~0 mV for ENTER (a hard short to ground) and ~3100 mV
// when idle. Bands are read as calibrated millivolts with dead zones between
// them, so a value in a gap decodes to no button.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Right,
    Left,
    Confirm,
    Back,
    VolUp,
    VolDown,
    Power,
}

impl Button {
    pub const fn name(self) -> &'static str {
        match self {
            Button::Right => "Right",
            Button::Left => "Left",
            Button::Confirm => "Confirm",
            Button::Back => "Back",
            Button::VolUp => "Vol Up",
            Button::VolDown => "Vol Down",
            Button::Power => "Power",
        }
    }
}

impl core::fmt::Display for Button {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// (low_mv, high_mv, button) for one front-key band, inclusive on both ends.
///
/// Ordered high-to-low to match the vendor tables; `decode_ladder` scans them
/// in order and returns the first band the reading falls in.
pub const KEY_LADDER_BANDS: &[(u16, u16, Button)] = &[
    (2400, 2800, Button::Back),
    (1780, 2140, Button::Left),
    (1140, 1500, Button::Right),
    (0, 250, Button::Confirm),
];

/// Ladder reading when no key is pressed (~3.1 V through the idle resistor).
pub const KEY_LADDER_IDLE_MV: u16 = 3100;

/// Decode one calibrated ladder reading. A reading inside a dead zone between
/// bands, or above the top band, is idle.
pub fn decode_ladder(mv: u16, bands: &[(u16, u16, Button)]) -> Option<Button> {
    for &(low, high, button) in bands {
        if mv >= low && mv <= high {
            return Some(button);
        }
    }
    None
}

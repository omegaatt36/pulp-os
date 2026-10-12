// hardware drivers: chip-level and protocol-level, board-independent

// The board drivers are board_c61::{battery, keys, epd}; the pure maths and
// state machines are shared through pulp_board_logic. The thin modules below
// give the scheduler, tasks and apps the `drivers::input::Event` /
// `drivers::battery::*` names.
pub(crate) mod datetime;
pub mod dir_entry;
pub mod sdcard;
pub mod storage;
// StripBuffer (DrawTarget over the shared StripCore) is board-neutral
pub mod strip;

// hardware event = the C61 key event type (no Power button)
pub mod input {
    pub type Event = pulp_board_logic::input::Event<crate::board::button::Button>;
}

// the monitor reports the cell voltage already (x2 divider is
// applied in pulp_board_logic::battery), so only the percentage is needed here
pub mod battery {
    pub fn battery_percentage(cell_mv: u16) -> u8 {
        pulp_board_logic::battery::percentage(cell_mv)
    }
}

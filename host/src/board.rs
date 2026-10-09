// host stand-in for the board surface (kernel/src/board/mod.rs on the X4,
// board_c61::api on the C61): the key / action / layout files are the real
// ones; the display type is a placeholder.
#[path = "../../kernel/src/board/action.rs"]
pub mod action;
#[path = "../../kernel/src/board/button.rs"]
pub mod button;
#[path = "../../kernel/src/board/layout.rs"]
pub mod layout;

pub use crate::drivers::sdcard::SdStorage;
pub use crate::drivers::strip::StripBuffer;
pub use button::Button;

// logical portrait size of the 800x480 panel, derived like the firmware does
pub const SCREEN_W: u16 = pulp_board_logic::ssd1677::HEIGHT; // 480
pub const SCREEN_H: u16 = pulp_board_logic::ssd1677::WIDTH; // 800

// display driver type named by AppLayer::run_special_mode; never constructed
pub struct Epd;

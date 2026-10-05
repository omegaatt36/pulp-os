// host stand-in for kernel/src/drivers/mod.rs: dir_entry and strip are the real
// files; the SD card is the virtual card (storage.rs); input only names the
// event type.
#[path = "../../kernel/src/drivers/dir_entry.rs"]
pub mod dir_entry;
#[path = "../../kernel/src/drivers/strip.rs"]
#[allow(dead_code)]
pub mod strip;

pub mod sdcard;
pub mod storage;

pub mod input {
    // C61 key event (kernel/src/drivers/mod.rs); the X4 one needs esp-hal
    pub type Event = pulp_board_logic::input::Event<crate::board::button::Button>;
}

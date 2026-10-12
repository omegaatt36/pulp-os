// key event -> semantic action with the optional left-handed swap (front row
// Back<->Left, Enter<->Right; side keys and WAKE never swap). The key -> Action
// table and the swap rule are pulp_board_logic::keys (host-tested).

use super::button::Button;
use crate::drivers::input::Event;
use pulp_board_logic::keys;

pub use pulp_board_logic::action::{Action, ActionEvent};

#[derive(Default)]
pub struct ButtonMapper {
    swap_buttons: bool,
}

impl ButtonMapper {
    pub const fn new() -> Self {
        Self {
            swap_buttons: false,
        }
    }

    pub fn set_swap(&mut self, swap: bool) {
        self.swap_buttons = swap;
    }

    pub fn is_swapped(&self) -> bool {
        self.swap_buttons
    }

    pub fn map_button(&self, button: Button) -> Action {
        button.action(self.swap_buttons)
    }

    pub fn map_event(&self, event: Event) -> ActionEvent {
        keys::map_event(event, self.swap_buttons)
    }
}

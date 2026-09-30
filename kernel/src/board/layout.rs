// physical button positions on the MoveCall OnePage bezel
// used by button_feedback to render labels at the correct screen edge

// center-x of the four bottom-edge front keys, left to right.
// These are the 4 keys of the OnePage front ladder: BACK, LEFT, RIGHT
// and ENTER, in the physical order the bezel puts them.
pub const CX_BACK: u16 = 84;
pub const CX_CONFIRM: u16 = 194;
pub const CX_LEFT: u16 = 286;
pub const CX_RIGHT: u16 = 396;

// center-y of the right-edge discrete keys (PREV / NEXT).
// The OnePage puts PREV and NEXT on the right edge as dedicated keys.
pub const CY_VOL_UP: u16 = 364;
pub const CY_VOL_DOWN: u16 = 484;

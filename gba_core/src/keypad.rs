use crate::BitIndex;
use crate::impl_save_state_tuple;

use crate::{Gba, InterruptKind};

/// Keypad State as a bitmask.
///
/// Bits: 0=A, 1=B, 2=Select, 3=Start, 4=Right, 5=Left, 6=Up, 7=Down, 8=R, 9=L
/// Set (1) = pressed; clear (0) = not pressed.
#[derive(Copy, Clone, Debug, Default)]
pub struct KeypadState(pub u16);

impl_save_state_tuple!(KeypadState);

impl KeypadState {
    pub const A: u16 = 1 << 0;
    pub const B: u16 = 1 << 1;
    pub const SELECT: u16 = 1 << 2;
    pub const START: u16 = 1 << 3;
    pub const RIGHT: u16 = 1 << 4;
    pub const LEFT: u16 = 1 << 5;
    pub const UP: u16 = 1 << 6;
    pub const DOWN: u16 = 1 << 7;
    pub const R: u16 = 1 << 8;
    pub const L: u16 = 1 << 9;
    pub const ALL: u16 = 0x3FF;
}

impl From<KeypadState> for u16 {
    fn from(state: KeypadState) -> u16 {
        // GBA KEYINPUT: 0 = pressed, 1 = not pressed (inverted)
        !state.0 & KeypadState::ALL
    }
}

impl Gba {
    pub fn set_keypad_state(&mut self, state: KeypadState) {
        self.keypad_state = state;

        let keycnt = self.io.keycnt;
        if keycnt.bit(14) {
            let irq_condition = keycnt.bit(15);
            let mask = keycnt & KeypadState::ALL;
            if mask != 0 {
                let pressed = !state.0 & KeypadState::ALL;
                let fire = if irq_condition {
                    (mask & pressed) == mask
                } else {
                    (mask & pressed) != 0
                };
                if fire {
                    self.interrupt_raise(InterruptKind::Keypad);
                }
            }
        }
    }
}

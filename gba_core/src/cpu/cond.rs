use crate::cpu::psr::ProgramStatusRegister;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Condition {
    EQ, NE, CS, CC, MI, PL, VS, VC,
    HI, LS, GE, LT, GT, LE, AL, Invalid,
}

const COND_FROM: [Condition; 16] = [
    Condition::EQ, Condition::NE, Condition::CS, Condition::CC,
    Condition::MI, Condition::PL, Condition::VS, Condition::VC,
    Condition::HI, Condition::LS, Condition::GE, Condition::LT,
    Condition::GT, Condition::LE, Condition::AL, Condition::Invalid,
];

/// Bitmask lookup table for branch-free condition evaluation.
/// Each entry is a 16-bit mask where bit `f` is set when the condition
/// is true for flag state `f = (N<<3)|(Z<<2)|(C<<1)|V`.
const COND_TABLE: [u16; 16] = [
    0xF0F0, // EQ (0): Z=1
    0x0F0F, // NE (1): Z=0
    0xCCCC, // CS (2): C=1
    0x3333, // CC (3): C=0
    0xFF00, // MI (4): N=1
    0x00FF, // PL (5): N=0
    0xAAAA, // VS (6): V=1
    0x5555, // VC (7): V=0
    0x0C0C, // HI (8): C=1 && Z=0
    0xF3F3, // LS (9): !C || Z
    0xAA55, // GE (10): N==V
    0x55AA, // LT (11): N!=V
    0x0A05, // GT (12): !Z && N==V
    0xF5FA, // LE (13): Z || N!=V
    0xFFFF, // AL (14): always
    0x0000, // Invalid (15): never
];

impl From<u32> for Condition {
    fn from(data: u32) -> Self {
        COND_FROM[(data & 0xF) as usize]
    }
}

impl Condition {
    pub fn evaluate(self, gba: &crate::Gba) -> bool {
        evaluate_condition(self as usize, &gba.cpu.cpsr)
    }
}

/// Evaluate a raw 4-bit ARM condition code against CPSR flags.
/// Uses a branch-free bitmask lookup table.
#[inline(always)]
pub fn evaluate_condition(cond: usize, cpsr: &ProgramStatusRegister) -> bool {
    let f = ((cpsr.cond_flag_n as u8) << 3)
          | ((cpsr.cond_flag_z as u8) << 2)
          | ((cpsr.cond_flag_c as u8) << 1)
          | (cpsr.cond_flag_v as u8);
    (COND_TABLE[cond] >> f) & 1 != 0
}

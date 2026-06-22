use crate::BitIndex;

#[derive(Copy, Clone, Debug, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum AluOpcode {
    AND = 0x0, EOR = 0x1, SUB = 0x2, RSB = 0x3,
    ADD = 0x4, ADC = 0x5, SBC = 0x6, RSC = 0x7,
    TST = 0x8, TEQ = 0x9, CMP = 0xA, CMN = 0xB,
    ORR = 0xC, MOV = 0xD, BIC = 0xE, MVN = 0xF,
}

const ALU_OPCODES: [AluOpcode; 16] = [
    AluOpcode::AND, AluOpcode::EOR, AluOpcode::SUB, AluOpcode::RSB,
    AluOpcode::ADD, AluOpcode::ADC, AluOpcode::SBC, AluOpcode::RSC,
    AluOpcode::TST, AluOpcode::TEQ, AluOpcode::CMP, AluOpcode::CMN,
    AluOpcode::ORR, AluOpcode::MOV, AluOpcode::BIC, AluOpcode::MVN,
];

impl AluOpcode {
    pub const fn from_u32(value: u32) -> AluOpcode {
        ALU_OPCODES[(value & 0xF) as usize]
    }

    pub const fn is_logical(self) -> bool {
        // Logical opcodes: AND(0), EOR(1), TST(8), TEQ(9), ORR(12), MOV(13), BIC(14), MVN(15)
        const MASK: u16 = 0b1111_0011_0000_0011;
        (MASK >> (self as u32)) & 1 != 0
    }

    pub const fn is_arithmetic(self) -> bool {
        !self.is_logical()
    }

    pub const fn is_test(self) -> bool {
        matches!(self, AluOpcode::TST | AluOpcode::TEQ | AluOpcode::CMP | AluOpcode::CMN)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum ThumbAluOpcode {
    AND = 0x0, EOR = 0x1, LSL = 0x2, LSR = 0x3,
    ASR = 0x4, ADC = 0x5, SBC = 0x6, ROR = 0x7,
    TST = 0x8, NEG = 0x9, CMP = 0xA, CMN = 0xB,
    ORR = 0xC, MUL = 0xD, BIC = 0xE, MVN = 0xF,
}

const THUMB_ALU_OPCODES: [ThumbAluOpcode; 16] = [
    ThumbAluOpcode::AND, ThumbAluOpcode::EOR, ThumbAluOpcode::LSL, ThumbAluOpcode::LSR,
    ThumbAluOpcode::ASR, ThumbAluOpcode::ADC, ThumbAluOpcode::SBC, ThumbAluOpcode::ROR,
    ThumbAluOpcode::TST, ThumbAluOpcode::NEG, ThumbAluOpcode::CMP, ThumbAluOpcode::CMN,
    ThumbAluOpcode::ORR, ThumbAluOpcode::MUL, ThumbAluOpcode::BIC, ThumbAluOpcode::MVN,
];

impl ThumbAluOpcode {
    pub const fn from_u16(value: u16) -> ThumbAluOpcode {
        THUMB_ALU_OPCODES[(value & 0xF) as usize]
    }

    pub const fn is_test(self) -> bool {
        matches!(self, ThumbAluOpcode::TST | ThumbAluOpcode::CMP | ThumbAluOpcode::CMN)
    }
}

/// ADD with carry/overflow detection.
#[inline(always)]
pub fn calc_add(op1: u32, op2: u32) -> (u32, bool, bool) {
    let (result, carry) = op1.overflowing_add(op2);
    let overflow = (op1 as i32).overflowing_add(op2 as i32).1;
    (result, carry, overflow)
}

/// SUB with carry/overflow detection.
#[inline(always)]
pub fn calc_sub(op1: u32, op2: u32) -> (u32, bool, bool) {
    let result = op1.wrapping_sub(op2);
    let carry = op2 <= op1;
    let overflow = (op1 as i32).overflowing_sub(op2 as i32).1;
    (result, carry, overflow)
}

/// ADC (add with carry) using chain of overflowing_add.
#[inline(always)]
pub fn calc_adc(op1: u32, op2: u32, carry: bool) -> (u32, bool, bool) {
    let (tmp, carry1) = op1.overflowing_add(carry as u32);
    let (result, carry2) = tmp.overflowing_add(op2);
    let carry = carry1 || carry2;
    let overflow = (!(op1 ^ op2) & (op2 ^ result)).bit(31);
    (result, carry, overflow)
}

/// SBC (subtract with carry) implemented as ADC(~op2).
#[inline(always)]
pub fn calc_sbc(op1: u32, op2: u32, carry: bool) -> (u32, bool, bool) {
    calc_adc(op1, !op2, carry)
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum AluShiftType {
    LSL = 0b00,
    LSR = 0b01,
    ASR = 0b10,
    ROR = 0b11,
}

const SHIFT_TYPES: [AluShiftType; 4] = [
    AluShiftType::LSL, AluShiftType::LSR,
    AluShiftType::ASR, AluShiftType::ROR,
];

impl AluShiftType {
    pub const fn from_u32(value: u32) -> AluShiftType {
        SHIFT_TYPES[(value & 0b11) as usize]
    }
}

#[inline(always)]
pub fn multiply_internal_cycles(operand: u32) -> u32 {
    let leading_same = u32::max(operand.leading_ones(), operand.leading_zeros());
    4 - (leading_same / 8)
}

/// Shift a 32-bit operand by an 11 bit immediate.
#[inline(always)]
pub fn shift_by_immediate(
    shift: AluShiftType,
    operand: u32,
    shift_amount: usize,
    carry_in: bool,
) -> (u32, bool) {
    use AluShiftType::*;
    match shift {
        LSL => {
            if shift_amount == 0 {
                (operand, carry_in)
            } else {
                (operand << shift_amount, operand.bit(32 - shift_amount))
            }
        }
        LSR => {
            if shift_amount == 0 {
                (0, operand.bit(31))
            } else {
                (operand >> shift_amount, operand.bit(shift_amount - 1))
            }
        }
        ASR => {
            if shift_amount == 0 {
                let sign = operand.bit(31);
                (if sign { 0xFFFFFFFF } else { 0 }, sign)
            } else {
                (((operand as i32) >> shift_amount) as u32, operand.bit(shift_amount - 1))
            }
        }
        ROR => {
            if shift_amount == 0 {
                (((carry_in as u32) << 31) | (operand >> 1), operand.bit(0))
            } else {
                (operand.rotate_right(shift_amount as u32), operand.bit(shift_amount - 1))
            }
        }
    }
}

/// Shift a 32-bit operand by a value loaded from a register.
#[inline(always)]
pub fn shift_by_register(
    shift: AluShiftType,
    operand: u32,
    shift_amount: u32,
    carry_in: bool,
) -> (u32, bool) {
    let shift_amount = shift_amount as usize;
    use AluShiftType::*;
    match shift {
        LSL => {
            if shift_amount == 0 {
                (operand, carry_in)
            } else if shift_amount < 32 {
                (operand << shift_amount, operand.bit(32 - shift_amount))
            } else if shift_amount == 32 {
                (0, operand.bit(0))
            } else {
                (0, false)
            }
        }
        LSR => {
            if shift_amount == 0 {
                (operand, carry_in)
            } else if shift_amount < 32 {
                (operand >> shift_amount, operand.bit(shift_amount - 1))
            } else if shift_amount == 32 {
                (0, operand.bit(31))
            } else {
                (0, false)
            }
        }
        ASR => {
            if shift_amount == 0 {
                (operand, carry_in)
            } else if shift_amount < 32 {
                (((operand as i32) >> shift_amount) as u32, operand.bit(shift_amount - 1))
            } else {
                let sign = operand.bit(31);
                (if sign { 0xFFFFFFFF } else { 0 }, sign)
            }
        }
        ROR => {
            let new_amount = shift_amount & 0x1F;
            if shift_amount == 0 {
                (operand, carry_in)
            } else if new_amount == 0 {
                (operand, operand.bit(31))
            } else {
                (operand.rotate_right(new_amount as u32), operand.bit(new_amount - 1))
            }
        }
    }
}

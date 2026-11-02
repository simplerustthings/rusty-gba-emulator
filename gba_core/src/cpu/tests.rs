use crate::BitIndex;

use super::alu::{
    self, calc_adc, calc_add, calc_sbc, calc_sub, shift_by_immediate, shift_by_register,
    AluShiftType,
};
use super::cond::evaluate_condition;
use super::psr::ProgramStatusRegister;

const OPERANDS: &[u32] = &[
    0x0000_0000,
    0x0000_0001,
    0x0000_0002,
    0x7FFF_FFFF,
    0x8000_0000,
    0x8000_0001,
    0xFFFF_FFFE,
    0xFFFF_FFFF,
    0x0000_FFFF,
    0xFFFF_0000,
    0x1234_5678,
    0xDEAD_BEEF,
];

fn ref_add(a: u32, b: u32) -> (u32, bool, bool) {
    let wide = (a as i32 as i64) + (b as i32 as i64);
    (
        a.wrapping_add(b),
        wide > i32::MAX as i64 || wide < i32::MIN as i64,
        a as u64 + b as u64 > u32::MAX as u64,
    )
}

fn ref_sub(a: u32, b: u32) -> (u32, bool, bool) {
    let wide = (a as i32 as i64) - (b as i32 as i64);
    (
        a.wrapping_sub(b),
        wide > i32::MAX as i64 || wide < i32::MIN as i64,
        (a as u64) >= (b as u64),
    )
}

fn ref_adc(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let carry = carry as u64;
    let wide = (a as i32 as i64) + (b as i32 as i64) + carry as i64;
    let sum = a as u64 + b as u64 + carry;
    (
        sum as u32,
        wide > i32::MAX as i64 || wide < i32::MIN as i64,
        sum > u32::MAX as u64,
    )
}

fn ref_sbc(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let borrow = !carry as u64;
    let wide = (a as i32 as i64) - (b as i32 as i64) - borrow as i64;
    (
        a.wrapping_sub(b).wrapping_sub(borrow as u32),
        wide > i32::MAX as i64 || wide < i32::MIN as i64,
        a as u64 >= b as u64 + borrow,
    )
}

#[test]
fn add_flags_match_reference() {
    for &a in OPERANDS {
        for &b in OPERANDS {
            let got = calc_add(a, b);
            let (result, overflow, carry) = ref_add(a, b);
            assert_eq!(got, (result, carry, overflow), "add {a:08X} + {b:08X}");
        }
    }
}

#[test]
fn sub_flags_match_reference() {
    for &a in OPERANDS {
        for &b in OPERANDS {
            let got = calc_sub(a, b);
            let (result, overflow, carry) = ref_sub(a, b);
            assert_eq!(got, (result, carry, overflow), "sub {a:08X} - {b:08X}");
        }
    }
}

#[test]
fn adc_flags_match_reference() {
    for &a in OPERANDS {
        for &b in OPERANDS {
            for carry in [false, true] {
                let got = calc_adc(a, b, carry);
                let (result, overflow, carry_out) = ref_adc(a, b, carry);
                assert_eq!(
                    got,
                    (result, carry_out, overflow),
                    "adc {a:08X} + {b:08X} + {carry}"
                );
            }
        }
    }
}

#[test]
fn sbc_flags_match_reference() {
    for &a in OPERANDS {
        for &b in OPERANDS {
            for carry in [false, true] {
                let got = calc_sbc(a, b, carry);
                let (result, overflow, carry_out) = ref_sbc(a, b, carry);
                assert_eq!(
                    got,
                    (result, carry_out, overflow),
                    "sbc {a:08X} - {b:08X} - {}",
                    !carry
                );
            }
        }
    }
}

fn ref_shift_imm(
    shift: AluShiftType,
    operand: u32,
    amount: usize,
    carry_in: bool,
) -> (u32, bool) {
    match shift {
        AluShiftType::LSL => {
            if amount == 0 {
                (operand, carry_in)
            } else if amount < 32 {
                (operand << amount, operand.bit(32 - amount))
            } else {
                (0, false)
            }
        }
        AluShiftType::LSR => {
            if amount == 0 {
                (0, operand.bit(31))
            } else if amount < 32 {
                (operand >> amount, operand.bit(amount - 1))
            } else {
                (0, false)
            }
        }
        AluShiftType::ASR => {
            if amount == 0 {
                let sign = operand.bit(31);
                (if sign { u32::MAX } else { 0 }, sign)
            } else if amount < 32 {
                (
                    ((operand as i32) >> amount) as u32,
                    operand.bit(amount - 1),
                )
            } else {
                let sign = operand.bit(31);
                (if sign { u32::MAX } else { 0 }, sign)
            }
        }
        AluShiftType::ROR => {
            if amount == 0 {
                (
                    ((carry_in as u32) << 31) | (operand >> 1),
                    operand.bit(0),
                )
            } else {
                (
                    operand.rotate_right(amount as u32),
                    operand.bit(amount - 1),
                )
            }
        }
    }
}

fn ref_shift_reg(
    shift: AluShiftType,
    operand: u32,
    amount: u32,
    carry_in: bool,
) -> (u32, bool) {
    match shift {
        AluShiftType::LSL => {
            if amount == 0 {
                (operand, carry_in)
            } else if amount < 32 {
                (operand << amount, operand.bit(32 - amount as usize))
            } else if amount == 32 {
                (0, operand.bit(0))
            } else {
                (0, false)
            }
        }
        AluShiftType::LSR => {
            if amount == 0 {
                (operand, carry_in)
            } else if amount < 32 {
                (operand >> amount, operand.bit(amount as usize - 1))
            } else if amount == 32 {
                (0, operand.bit(31))
            } else {
                (0, false)
            }
        }
        AluShiftType::ASR => {
            if amount == 0 {
                (operand, carry_in)
            } else if amount < 32 {
                (
                    ((operand as i32) >> amount) as u32,
                    operand.bit(amount as usize - 1),
                )
            } else {
                let sign = operand.bit(31);
                (if sign { u32::MAX } else { 0 }, sign)
            }
        }
        AluShiftType::ROR => {
            if amount == 0 {
                (operand, carry_in)
            } else {
                let amount = amount & 31;
                if amount == 0 {
                    (operand, operand.bit(31))
                } else {
                    (operand.rotate_right(amount), operand.bit(amount as usize - 1))
                }
            }
        }
    }
}

#[test]
fn immediate_shifts_match_reference() {
    const AMOUNTS: &[usize] = &[0, 1, 2, 7, 8, 15, 16, 23, 24, 30, 31];
    for &operand in OPERANDS {
        for &amount in AMOUNTS {
            for shift in [
                AluShiftType::LSL,
                AluShiftType::LSR,
                AluShiftType::ASR,
                AluShiftType::ROR,
            ] {
                for carry_in in [false, true] {
                    let got = shift_by_immediate(shift, operand, amount, carry_in);
                    let want = ref_shift_imm(shift, operand, amount, carry_in);
                    assert_eq!(
                        got, want,
                        "{shift:?} {operand:08X} #{amount} carry={carry_in}"
                    );
                }
            }
        }
    }
}

#[test]
fn register_shifts_match_reference() {
    const AMOUNTS: &[u32] = &[0, 1, 2, 7, 8, 31, 32, 33, 63, 64, 255, u32::MAX];
    for &operand in OPERANDS {
        for &amount in AMOUNTS {
            for shift in [
                AluShiftType::LSL,
                AluShiftType::LSR,
                AluShiftType::ASR,
                AluShiftType::ROR,
            ] {
                for carry_in in [false, true] {
                    let got = shift_by_register(shift, operand, amount, carry_in);
                    let want = ref_shift_reg(shift, operand, amount, carry_in);
                    assert_eq!(
                        got, want,
                        "{shift:?} {operand:08X} by {amount} carry={carry_in}"
                    );
                }
            }
        }
    }
}

#[test]
fn all_arm_conditions_match_reference() {
    for cond in 0..16 {
        for flags in 0..16 {
            let n = flags & 0b1000 != 0;
            let z = flags & 0b0100 != 0;
            let c = flags & 0b0010 != 0;
            let v = flags & 0b0001 != 0;
            let mut psr = ProgramStatusRegister::new();
            psr.cond_flag_n = n;
            psr.cond_flag_z = z;
            psr.cond_flag_c = c;
            psr.cond_flag_v = v;

            let expected = match cond {
                0 => z,
                1 => !z,
                2 => c,
                3 => !c,
                4 => n,
                5 => !n,
                6 => v,
                7 => !v,
                8 => c && !z,
                9 => !c || z,
                10 => n == v,
                11 => n != v,
                12 => !z && n == v,
                13 => z || n != v,
                14 => true,
                15 => false,
                _ => unreachable!(),
            };
            assert_eq!(
                evaluate_condition(cond, &psr),
                expected,
                "cond {cond} flags {flags:04b}"
            );
        }
    }
}

#[test]
fn multiply_internal_cycle_counts_are_in_range() {
    for operand in OPERANDS {
        let cycles = alu::multiply_internal_cycles(*operand);
        assert!(
            cycles <= 4,
            "multiply internal cycles should be at most 4, got {cycles} for {operand:08X}"
        );
    }
}

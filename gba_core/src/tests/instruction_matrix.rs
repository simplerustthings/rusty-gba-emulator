use crate::cpu::CpuMode;
use crate::tests::TestGba;

const CODE_BASE: u32 = 0x0300_0000;
const DATA_BASE: u32 = 0x0300_0100;

fn arm_case(inst: u32, setup: impl FnOnce(&mut TestGba)) -> TestGba {
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, inst);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, DATA_BASE);
    test.set_reg(1, 0x1122_3344);
    test.set_reg(2, 4);
    test.set_reg(3, 5);
    test.set_reg(13, DATA_BASE + 0x100);
    setup(&mut test);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_ne!(
        test.gba.cpu.cpsr.mode,
        CpuMode::Undefined,
        "ARM instruction {inst:08X} decoded as unimplemented"
    );
    test
}

fn thumb_case(inst: u16, setup: impl FnOnce(&mut TestGba)) -> TestGba {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, inst);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, DATA_BASE);
    test.set_reg(1, 0x1122_3344);
    test.set_reg(2, 4);
    test.set_reg(13, DATA_BASE + 0x100);
    test.set_reg(14, CODE_BASE + 1);
    setup(&mut test);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    assert_ne!(
        test.gba.cpu.cpsr.mode,
        CpuMode::Undefined,
        "Thumb instruction {inst:04X} decoded as unimplemented"
    );
    test
}

fn arm_word_byte_op(
    preindex: bool,
    up: bool,
    byte: bool,
    writeback: bool,
    load: bool,
    rn: u32,
    rd: u32,
    offset: u32,
) -> u32 {
    0xE400_0000
        | ((preindex as u32) << 24)
        | ((up as u32) << 23)
        | ((byte as u32) << 22)
        | ((writeback as u32) << 21)
        | ((load as u32) << 20)
        | (rn << 16)
        | (rd << 12)
        | offset
}

#[test]
fn arm_word_and_byte_load_store_matrix() {
    for preindex in [false, true] {
        for up in [false, true] {
            for byte in [false, true] {
                for writeback in [false, true] {
                    for load in [false, true] {
                        let inst = arm_word_byte_op(
                            preindex, up, byte, writeback, load, 0, 1, 4,
                        );
                        let test = arm_case(inst, |_| {});
                        // LDRT/STRT no-writeback and normal post-index both
                        // must decode and execute.
                        let _ = test;
                    }
                }
            }
        }
    }
}

fn arm_halfword_op(
    preindex: bool,
    up: bool,
    immediate: bool,
    writeback: bool,
    load: bool,
    signed: bool,
    halfword: bool,
    rn: u32,
    rd: u32,
) -> u32 {
    let offset = if immediate { 4 } else { 2 };
    0xE000_0000
        | ((preindex as u32) << 24)
        | ((up as u32) << 23)
        | ((immediate as u32) << 22)
        | ((writeback as u32) << 21)
        | ((load as u32) << 20)
        | (rn << 16)
        | (rd << 12)
        | ((offset & 0xF) << 8)
        | (1 << 7)
        | ((signed as u32) << 6)
        | ((halfword as u32) << 5)
        | (1 << 4)
        | (offset & 0xF)
}

#[test]
fn arm_halfword_and_signed_load_store_matrix() {
    let types = [(false, true, false), (true, false, true), (true, true, true)];
    for preindex in [false, true] {
        for up in [false, true] {
            for immediate in [false, true] {
                for writeback in [false, true] {
                    for (signed, halfword, load) in types {
                        let inst = arm_halfword_op(
                            preindex,
                            up,
                            immediate,
                            writeback,
                            load,
                            signed,
                            halfword,
                            0,
                            1,
                        );
                        let test = arm_case(inst, |test| {
                            if !immediate {
                                test.set_reg(2, 4);
                            }
                        });
                        let _ = test;
                    }
                }
            }
        }
    }
}

#[test]
fn arm_ldm_stm_matrix() {
    for preindex in [false, true] {
        for up in [false, true] {
            for s in [false, true] {
                for writeback in [false, true] {
                    for load in [false, true] {
                        let inst = 0xE800_0000
                            | ((preindex as u32) << 24)
                            | ((up as u32) << 23)
                            | ((s as u32) << 22)
                            | ((writeback as u32) << 21)
                            | ((load as u32) << 20)
                            | (0 << 16)
                            | 0b0000_0110; // r1, r2
                        let test = arm_case(inst, |_| {});
                        let _ = test;
                    }
                }
            }
        }
    }
}

#[test]
fn arm_multiply_and_psr_matrix() {
    // MUL, MLA, MULS, MLAS, UMULL, SMULL, UMLAL, SMLAL.
    for accumulate in [false, true] {
        for set_flags in [false, true] {
            let inst = 0xE000_0090
                | ((accumulate as u32) << 21)
                | ((set_flags as u32) << 20)
                | (2 << 16)
                | (3 << 12)
                | (1 << 8)
                | 0;
            let test = arm_case(inst, |_| {});
            let _ = test;
        }
    }
    for signed in [false, true] {
        for accumulate in [false, true] {
            for set_flags in [false, true] {
                let inst = 0xE080_0090
                    | ((signed as u32) << 22)
                    | ((accumulate as u32) << 21)
                    | ((set_flags as u32) << 20)
                    | (3 << 16)
                    | (2 << 12)
                    | (1 << 8)
                    | 0;
                let test = arm_case(inst, |_| {});
                let _ = test;
            }
        }
    }

    // MRS/MSR CPSR and SPSR, plus BX and SWP/SWPB.
    for inst in [
        0xE10F_0000, // mrs r0, cpsr
        0xE14F_0000, // mrs r0, spsr
        0xE128_F000, // msr cpsr_f, r0
        0xE168_F000, // msr spsr_f, r0
        0xE12F_FF10, // bx r0
        0xE100_1092, // swp r1, r2, [r0]
        0xE140_1092, // swpb r1, r2, [r0]
        0xEF00_0000, // swi
    ] {
        let test = arm_case(inst, |test| {
            if inst == 0xE12F_FF10 {
                test.set_reg(0, CODE_BASE + 0x10);
            }
            if inst == 0xE128_F000 || inst == 0xE168_F000 {
                test.set_reg(0, 0x1F);
            }
        });
        let _ = test;
    }
}

#[test]
fn thumb_decoder_matrix() {
    // Format 1: shift by immediate.
    for op in 0..4u16 {
        let inst = (op << 11) | (2 << 6) | (1 << 3);
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 2: add/subtract.
    for immediate in [false, true] {
        for subtract in [false, true] {
            let inst = 0x1800
                | ((immediate as u16) << 10)
                | ((subtract as u16) << 9)
                | (1 << 6)
                | (1 << 3);
            let test = thumb_case(inst, |_| {});
            let _ = test;
        }
    }

    // Format 3: move/compare/add/subtract immediate.
    for op in 0..4u16 {
        let inst = 0x2000 | (op << 11) | (1 << 8) | 7;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 4: ALU register.
    for op in 0..16u16 {
        let inst = 0x4000 | (op << 6) | (1 << 3) | 0;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 5: hi-register operations and BX.
    for op in 0..3u16 {
        let inst = 0x4400 | (op << 8) | (1 << 3) | 0;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }
    let test = thumb_case(0x4700, |test| {
        test.set_reg(0, CODE_BASE + 0x10);
    });
    let _ = test;

    // Format 6: PC-relative load.
    let test = thumb_case(0x4801, |_| {});
    let _ = test;

    // Format 7: load/store with register offset.
    for op in 0..8u16 {
        let inst = 0x5000 | (op << 9) | (1 << 6) | (0 << 3) | 1;
        let test = thumb_case(inst, |test| {
            test.set_reg(1, 4);
        });
        let _ = test;
    }

    // Format 8/9/10: load/store with immediate offsets.
    for base in [0x6000u16, 0x6800, 0x7000, 0x7800, 0x8000, 0x8800] {
        let inst = base | (0 << 6) | (0 << 3) | 1;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 11: SP-relative load/store.
    for base in [0x9000u16, 0x9800] {
        let inst = base | (1 << 8) | 4;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 12: address calculation.
    for base in [0xA000u16, 0xA800] {
        let inst = base | (1 << 8) | 4;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 13: adjust SP.
    for base in [0xB000u16, 0xB080] {
        let inst = base | 8;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 14: push/pop.
    for pop in [false, true] {
        for pc_lr in [false, true] {
            let inst = 0xB400 | ((pop as u16) << 11) | ((pc_lr as u16) << 8) | 0b0000_0110;
            let test = thumb_case(inst, |_| {});
            let _ = test;
        }
    }

    // Format 15: multiple load/store using a non-base register.
    for load in [false, true] {
        let inst = 0xC000 | ((load as u16) << 11) | (0 << 8) | 0b0000_0110;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 16: conditional branch, all architecturally valid conditions.
    // 0xDE is the undefined-instruction encoding, not a branch condition.
    for cond in 0..14u16 {
        let inst = 0xD000 | (cond << 8) | 2;
        let test = thumb_case(inst, |_| {});
        let _ = test;
    }

    // Format 17/18/19: SWI, branch, and BL.
    let test = thumb_case(0xDF00, |_| {});
    let _ = test;
    let test = thumb_case(0xE002, |_| {});
    let _ = test;

    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xF000);
    test.write_iwram16(0x0002, 0xF806);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    test.step_instruction();
    assert_ne!(
        test.gba.cpu.cpsr.mode,
        CpuMode::Undefined,
        "Thumb BL decoded as unimplemented"
    );
}

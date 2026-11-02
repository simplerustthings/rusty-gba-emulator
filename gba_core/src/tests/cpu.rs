use crate::cpu::{CpuExecutionState, CpuMode};
use crate::tests::TestGba;

const CODE_BASE: u32 = 0x0300_0000;

/// `MSR CPSR_fc, r0` is used by Pokemon Pinball R/S as an ARM-to-Thumb
/// interworking trampoline. The T bit must take effect for the next
/// instruction, and the pipeline must be refilled with the new width.
#[test]
fn msr_cpsr_control_can_switch_to_thumb() {
    let mut test = TestGba::new();

    // ARM: msr cpsr_fc, r0
    test.write_iwram32(0x0000, 0xE129_F000);
    // Thumb: movs r1, #7 ; bx lr
    test.write_iwram16(0x0004, 0x2107);
    test.write_iwram16(0x0006, 0x4770);

    // System mode, T clear, then request System mode with T set (0x3F).
    test.gba.cpu.cpsr.mode = crate::cpu::CpuMode::System;
    test.set_reg(0, 0x3F);
    test.set_arm_entry(CODE_BASE);

    test.step_instruction();

    assert_eq!(test.execution_state(), CpuExecutionState::Thumb);
    assert_eq!(test.gba.cpu.pc, CODE_BASE + 8);

    test.step_instruction();

    assert_eq!(test.reg(1), 7);
    assert_eq!(test.execution_state(), CpuExecutionState::Thumb);
}

/// A CPSR control write that leaves T clear must not change instruction width.
#[test]
fn msr_cpsr_control_without_t_bit_stays_arm() {
    let mut test = TestGba::new();

    // ARM: msr cpsr_fc, r0 ; mov r1, #7
    test.write_iwram32(0x0000, 0xE129_F000);
    test.write_iwram32(0x0004, 0xE3A0_1007);

    test.gba.cpu.cpsr.mode = crate::cpu::CpuMode::System;
    test.set_reg(0, 0x1F);
    test.set_arm_entry(CODE_BASE);

    test.step_instruction();

    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    // A normal ARM step advances the fetch PC by 4 (from +8 to +12).
    assert_eq!(test.gba.cpu.pc, CODE_BASE + 12);

    test.step_instruction();
    assert_eq!(test.reg(1), 7);
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
}

#[test]
fn ldrt_and_strt_do_not_write_back() {
    let base = 0x0300_0100;
    let mut test = TestGba::new();

    // strt r1, [r0], #4
    test.write_iwram32(0x0000, 0xE4A0_1004);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_reg(1, 0xDEAD_BEEF);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();

    assert_eq!(test.reg(0), base, "STRT must not write back");
    assert_eq!(test.read_mem32(base), 0xDEAD_BEEF);

    // ldrt r1, [r0], #4
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE4B0_1004);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, 0xCAFE_BABE);
    test.step_instruction();

    assert_eq!(test.reg(0), base, "LDRT must not write back");
    assert_eq!(test.reg(1), 0xCAFE_BABE);
}

#[test]
fn normal_post_index_still_writes_back() {
    let base = 0x0300_0100;
    let mut test = TestGba::new();

    // ldr r1, [r0], #4
    test.write_iwram32(0x0000, 0xE490_1004);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, 0x1234_5678);
    test.step_instruction();

    assert_eq!(test.reg(0), base + 4);
    assert_eq!(test.reg(1), 0x1234_5678);
}

#[test]
fn swp_swaps_memory_and_register() {
    let base = 0x0300_0100;
    let mut test = TestGba::new();

    // swp r1, r2, [r0]
    test.write_iwram32(0x0000, 0xE100_1092);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_reg(2, 0x5566_7788);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, 0x1122_3344);
    test.step_instruction();

    assert_eq!(test.reg(1), 0x1122_3344);
    assert_eq!(test.read_mem32(base), 0x5566_7788);
}

#[test]
fn undefined_arm_instruction_enters_undefined_mode() {
    let mut test = TestGba::new();

    // An unallocated ARM encoding that decodes to arm_unimplemented.
    test.write_iwram32(0x0000, 0xEEF0_00F0);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();

    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::Undefined);
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    assert_eq!(test.gba.cpu.pc, 0x0000_000C);
}

#[test]
fn undefined_thumb_instruction_enters_undefined_mode() {
    let mut test = TestGba::new();

    // Thumb 0xDE00 is the undefined-instruction encoding.
    test.write_iwram16(0x0000, 0xDE00);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();

    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::Undefined);
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    assert_eq!(test.gba.cpu.pc, 0x0000_000C);
}

fn execute_arm(inst: u32, setup: impl FnOnce(&mut TestGba)) -> TestGba {
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, inst);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    setup(&mut test);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    test
}

fn execute_thumb(inst: u16, setup: impl FnOnce(&mut TestGba)) -> TestGba {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, inst);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    setup(&mut test);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    test
}

#[test]
fn arm_data_processing_all_opcodes() {
    let a = 0x0F0F_0F0F;
    let b = 0x00FF_00FF;
    for opcode in 0..16u32 {
        // Data processing instruction, Rn=r0, Rd=r2, Rm=r1.
        // TST/TEQ/CMP/CMN are only valid with S=1; with S=0 those encodings
        // overlap the MRS/MSR space.
        let set_flags = (8..=11).contains(&opcode);
        let inst = 0xE000_0000
            | (opcode << 21)
            | ((set_flags as u32) << 20)
            | (2 << 12)
            | 1;
        let test = execute_arm(inst, |test| {
            test.set_reg(0, a);
            test.set_reg(1, b);
            test.set_reg(2, 0xDEAD_BEEF);
            test.gba.cpu.cpsr.cond_flag_c = false;
        });

        let expected = match opcode {
            0 => a & b,             // AND
            1 => a ^ b,             // EOR
            2 => a.wrapping_sub(b), // SUB
            3 => b.wrapping_sub(a), // RSB
            4 => a.wrapping_add(b), // ADD
            5 => a.wrapping_add(b), // ADC, carry initially clear
            6 => a.wrapping_sub(b).wrapping_sub(1), // SBC, carry initially clear
            7 => b.wrapping_sub(a).wrapping_sub(1), // RSC, carry initially clear
            8..=11 => 0xDEAD_BEEF,  // TST/TEQ/CMP/CMN do not write
            12 => a | b,            // ORR
            13 => b,                // MOV
            14 => a & !b,           // BIC
            15 => !b,               // MVN
            _ => unreachable!(),
        };
        assert_eq!(test.reg(2), expected, "data-processing opcode {opcode}");
    }
}

#[test]
fn arm_data_processing_flags_and_conditionals() {
    // CMP r0, r1 with r0 == r1 must set Z/C and not write a register.
    let test = execute_arm(0xE150_0001, |test| {
        test.set_reg(0, 0x1234_5678);
        test.set_reg(1, 0x1234_5678);
        test.set_reg(2, 0xCAFE_BABE);
    });
    assert_eq!(test.reg(2), 0xCAFE_BABE);
    assert_ne!(test.cpsr() & (1 << 30), 0, "CMP should set Z");
    assert_ne!(test.cpsr() & (1 << 29), 0, "CMP should set C");

    // MOVEQ must be skipped when Z is clear.
    let test = execute_arm(0x03A0_2001, |test| {
        test.gba.cpu.cpsr.cond_flag_z = false;
        test.set_reg(2, 0xAAAA_AAAA);
    });
    assert_eq!(test.reg(2), 0xAAAA_AAAA);

    // MOVEQ must execute when Z is set.
    let test = execute_arm(0x03A0_2001, |test| {
        test.gba.cpu.cpsr.cond_flag_z = true;
        test.set_reg(2, 0xAAAA_AAAA);
    });
    assert_eq!(test.reg(2), 1);
}

#[test]
fn arm_branch_and_link() {
    let mut test = TestGba::new();
    // b +0x10
    test.write_iwram32(0x0000, 0xEA00_0002);
    test.write_iwram32(0x0010, 0xE3A0_1007); // mov r1, #7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.gba.cpu.pc, CODE_BASE + 0x18);
    test.step_instruction();
    assert_eq!(test.reg(1), 7);

    let mut test = TestGba::new();
    // bl +0x10
    test.write_iwram32(0x0000, 0xEB00_0002);
    test.write_iwram32(0x0010, 0xE3A0_1007);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(14), CODE_BASE + 4);
    test.step_instruction();
    assert_eq!(test.reg(1), 7);
}

#[test]
fn arm_bx_switches_to_thumb() {
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE12F_FF10); // bx r0
    test.write_iwram16(0x0010, 0x2107); // movs r1, #7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, CODE_BASE + 0x11);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.execution_state(), CpuExecutionState::Thumb);
    test.step_instruction();
    assert_eq!(test.reg(1), 7);
}

#[test]
fn arm_ldm_stm_ia_with_writeback() {
    let base = 0x0300_0100;
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE8A0_0006); // stmia r0!, {r1, r2}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_reg(1, 0x1111_1111);
    test.set_reg(2, 0x2222_2222);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(0), base + 8);
    assert_eq!(test.read_mem32(base), 0x1111_1111);
    assert_eq!(test.read_mem32(base + 4), 0x2222_2222);

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE8B0_0006); // ldmia r0!, {r1, r2}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, 0x3333_3333);
    test.write_mem32(base + 4, 0x4444_4444);
    test.step_instruction();
    assert_eq!(test.reg(0), base + 8);
    assert_eq!(test.reg(1), 0x3333_3333);
    assert_eq!(test.reg(2), 0x4444_4444);
}

#[test]
fn arm_halfword_and_signed_loads() {
    let base = 0x0300_0100;
    let mut test = execute_arm(0xE1C0_10B4, |test| { // strh r1, [r0, #4]
        test.set_reg(0, base);
        test.set_reg(1, 0xABCD);
    });
    assert_eq!(test.read_mem16(base + 4), 0xABCD);

    let test = execute_arm(0xE1D0_10B4, |test| { // ldrh r1, [r0, #4]
        test.set_reg(0, base);
        test.write_mem16(base + 4, 0xABCD);
    });
    assert_eq!(test.reg(1), 0xABCD);

    let test = execute_arm(0xE1D0_10D4, |test| { // ldrsb r1, [r0, #4]
        test.set_reg(0, base);
        test.write_mem8(base + 4, 0x80);
    });
    assert_eq!(test.reg(1), 0xFFFF_FF80);

    let test = execute_arm(0xE1D0_10F4, |test| { // ldrsh r1, [r0, #4]
        test.set_reg(0, base);
        test.write_mem16(base + 4, 0x8000);
    });
    assert_eq!(test.reg(1), 0xFFFF_8000);
}

#[test]
fn thumb_shift_alu_and_immediate_instructions() {
    let test = execute_thumb(0x2007, |_| {}); // movs r0,#7
    assert_eq!(test.reg(0), 7);

    let test = execute_thumb(0x3003, |test| test.set_reg(0, 5)); // adds r0,#3
    assert_eq!(test.reg(0), 8);

    let test = execute_thumb(0x3803, |test| test.set_reg(0, 5)); // subs r0,#3
    assert_eq!(test.reg(0), 2);

    let test = execute_thumb(0x2807, |test| test.set_reg(0, 7)); // cmp r0,#7
    assert_ne!(test.cpsr() & (1 << 30), 0);

    let test = execute_thumb(0x1840, |test| { // adds r0,r0,r1
        test.set_reg(0, 5);
        test.set_reg(1, 7);
    });
    assert_eq!(test.reg(0), 12);

    let test = execute_thumb(0x1A40, |test| { // subs r0,r0,r1
        test.set_reg(0, 12);
        test.set_reg(1, 5);
    });
    assert_eq!(test.reg(0), 7);

    let test = execute_thumb(0x4008, |test| { // ands r0,r1
        test.set_reg(0, 0xF0F0);
        test.set_reg(1, 0x0FF0);
    });
    assert_eq!(test.reg(0), 0x00F0);

    let test = execute_thumb(0x4348, |test| { // muls r0,r1,r0
        test.set_reg(0, 3);
        test.set_reg(1, 4);
    });
    assert_eq!(test.reg(0), 12);

    let test = execute_thumb(0x0088, |test| { // lsls r0,r1,#2
        test.set_reg(1, 3);
    });
    assert_eq!(test.reg(0), 12);

    let test = execute_thumb(0x0888, |test| { // lsrs r0,r1,#2
        test.set_reg(1, 0x20);
    });
    assert_eq!(test.reg(0), 8);

    let test = execute_thumb(0x1088, |test| { // asrs r0,r1,#2
        test.set_reg(1, 0x8000_0020);
    });
    assert_eq!(test.reg(0), 0xE000_0008);
}

#[test]
fn thumb_pc_relative_load() {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0x4801); // ldr r0, [pc, #4]
    test.write_iwram32(0x0008, 0xCAFE_BABE);
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(0), 0xCAFE_BABE);
}

#[test]
fn thumb_multiple_load_store() {
    let base = 0x0300_0100;
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xC006); // stm r0!, {r1, r2}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_reg(1, 0x1111_1111);
    test.set_reg(2, 0x2222_2222);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(0), base + 8);
    assert_eq!(test.read_mem32(base), 0x1111_1111);
    assert_eq!(test.read_mem32(base + 4), 0x2222_2222);

    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xC806); // ldm r0!, {r1, r2}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_thumb_entry(CODE_BASE);
    test.write_mem32(base, 0x3333_3333);
    test.write_mem32(base + 4, 0x4444_4444);
    test.step_instruction();
    assert_eq!(test.reg(0), base + 8);
    assert_eq!(test.reg(1), 0x3333_3333);
    assert_eq!(test.reg(2), 0x4444_4444);
}

#[test]
fn thumb_push_and_conditional_branch() {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xB502); // push {r1, lr}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(13, 0x0300_0200);
    test.set_reg(1, 0x1234_5678);
    test.set_reg(14, 0xDEAD_BEEF);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(13), 0x0300_01F8);
    assert_eq!(test.read_mem32(0x0300_01F8), 0x1234_5678);
    assert_eq!(test.read_mem32(0x0300_01FC), 0xDEAD_BEEF);

    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xD001); // beq +6
    test.write_iwram16(0x0006, 0x2107); // movs r1,#7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.gba.cpu.cpsr.cond_flag_z = true;
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    test.step_instruction();
    assert_eq!(test.reg(1), 7);
}

#[test]
fn arm_byte_load_store_and_unaligned_ldr() {
    let base = 0x0300_0100;
    let mut test = execute_arm(0xE5C0_1004, |test| { // strb r1, [r0, #4]
        test.set_reg(0, base);
        test.set_reg(1, 0x1234_56AB);
    });
    assert_eq!(test.read_mem8(base + 4), 0xAB);
    assert_eq!(test.read_mem32(base), 0);

    let test = execute_arm(0xE5D0_1004, |test| { // ldrb r1, [r0, #4]
        test.set_reg(0, base);
        test.write_mem8(base + 4, 0xCD);
    });
    assert_eq!(test.reg(1), 0xCD);

    let mut test = execute_arm(0xE4C0_1001, |test| { // strb r1, [r0], #1
        test.set_reg(0, base);
        test.set_reg(1, 0x99);
    });
    assert_eq!(test.read_mem8(base), 0x99);
    assert_eq!(test.reg(0), base + 1);

    let test = execute_arm(0xE590_1001, |test| { // ldr r1, [r0, #1]
        test.set_reg(0, base);
        test.write_mem32(base, 0x1122_3344);
    });
    assert_eq!(test.reg(1), 0x4411_2233);
}

#[test]
fn arm_ldm_stm_db_with_writeback() {
    let base = 0x0300_0100;
    let mut test = execute_arm(0xE920_0006, |test| { // stmdb r0!, {r1, r2}
        test.set_reg(0, base);
        test.set_reg(1, 0x1111_1111);
        test.set_reg(2, 0x2222_2222);
    });
    assert_eq!(test.reg(0), base - 8);
    assert_eq!(test.read_mem32(base - 8), 0x1111_1111);
    assert_eq!(test.read_mem32(base - 4), 0x2222_2222);

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE930_0006); // ldmdb r0!, {r1, r2}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base - 8, 0x3333_3333);
    test.write_mem32(base - 4, 0x4444_4444);
    test.step_instruction();
    assert_eq!(test.reg(0), base - 8);
    assert_eq!(test.reg(1), 0x3333_3333);
    assert_eq!(test.reg(2), 0x4444_4444);
}

#[test]
fn thumb_load_store_and_stack_offsets() {
    let base = 0x0300_0100;
    let mut test = execute_thumb(0x6001, |test| { // str r1, [r0]
        test.set_reg(0, base);
        test.set_reg(1, 0x1234_5678);
    });
    assert_eq!(test.read_mem32(base), 0x1234_5678);

    let test = execute_thumb(0x6841, |test| { // ldr r1, [r0, #4]
        test.set_reg(0, base);
        test.write_mem32(base + 4, 0xCAFE_BABE);
    });
    assert_eq!(test.reg(1), 0xCAFE_BABE);

    let mut test = execute_thumb(0x8001, |test| { // strh r1, [r0]
        test.set_reg(0, base);
        test.set_reg(1, 0xABCD);
    });
    assert_eq!(test.read_mem16(base), 0xABCD);

    let test = execute_thumb(0x8801, |test| { // ldrh r1, [r0]
        test.set_reg(0, base);
        test.write_mem16(base, 0xABCD);
    });
    assert_eq!(test.reg(1), 0xABCD);

    let mut test = execute_thumb(0x7001, |test| { // strb r1, [r0]
        test.set_reg(0, base);
        test.set_reg(1, 0xEF);
    });
    assert_eq!(test.read_mem8(base), 0xEF);

    let test = execute_thumb(0x7801, |test| { // ldrb r1, [r0]
        test.set_reg(0, base);
        test.write_mem8(base, 0xCD);
    });
    assert_eq!(test.reg(1), 0xCD);

    let mut test = execute_thumb(0x9001, |test| { // str r0, [sp, #4]
        test.set_reg(0, 0xDEAD_BEEF);
        test.set_reg(13, base);
    });
    assert_eq!(test.read_mem32(base + 4), 0xDEAD_BEEF);

    let test = execute_thumb(0x9801, |test| { // ldr r0, [sp, #4]
        test.set_reg(13, base);
        test.write_mem32(base + 4, 0xFEED_FACE);
    });
    assert_eq!(test.reg(0), 0xFEED_FACE);

    let test = execute_thumb(0xA801, |test| { // add r0, sp, #4
        test.set_reg(13, base);
    });
    assert_eq!(test.reg(0), base + 4);

    let test = execute_thumb(0xB002, |test| { // add sp, #8
        test.set_reg(13, base);
    });
    assert_eq!(test.reg(13), base + 8);
}

#[test]
fn thumb_bl_and_pop_pc() {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xF000); // bl +0x10
    test.write_iwram16(0x0002, 0xF806);
    test.write_iwram16(0x0010, 0x2107); // movs r1,#7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    test.step_instruction();
    assert_eq!(test.reg(14), CODE_BASE + 5);
    test.step_instruction();
    assert_eq!(test.reg(1), 7);

    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0xBD02); // pop {r1, pc}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(13, 0x0300_0200);
    test.set_thumb_entry(CODE_BASE);
    test.write_mem32(0x0300_0200, 0x1234_5678);
    test.write_mem32(0x0300_0204, CODE_BASE + 0x11);
    test.step_instruction();
    assert_eq!(test.reg(1), 0x1234_5678);
    assert_eq!(test.reg(13), 0x0300_0208);
    assert_eq!(test.execution_state(), CpuExecutionState::Thumb);
    assert_eq!(test.gba.cpu.pc, CODE_BASE + 0x10 + 4);
}

#[test]
fn arm_multiply_and_multiply_accumulate() {
    let test = execute_arm(0xE002_0190, |test| { // mul r2, r0, r1
        test.set_reg(0, 6);
        test.set_reg(1, 7);
    });
    assert_eq!(test.reg(2), 42);

    let test = execute_arm(0xE022_3190, |test| { // mla r2, r0, r1, r3
        test.set_reg(0, 6);
        test.set_reg(1, 7);
        test.set_reg(3, 5);
    });
    assert_eq!(test.reg(2), 47);

    let test = execute_arm(0xE012_0190, |test| { // muls r2, r0, r1
        test.set_reg(0, 0);
        test.set_reg(1, 7);
        test.set_reg(2, 0xFFFF_FFFF);
    });
    assert_eq!(test.reg(2), 0);
    assert_ne!(test.cpsr() & (1 << 30), 0, "MULS should set Z");
}

#[test]
fn arm_mrs_and_msr_flags() {
    let test = execute_arm(0xE10F_0000, |test| { // mrs r0, apsr
        test.gba.cpu.cpsr.cond_flag_n = true;
        test.gba.cpu.cpsr.cond_flag_c = true;
    });
    assert_eq!(test.reg(0), test.cpsr());

    let test = execute_arm(0xE128_F000, |test| { // msr apsr_nzcvq, r0
        test.set_reg(0, 0xF000_0000);
    });
    assert_eq!(
        test.cpsr() & 0xF000_0000,
        0xF000_0000,
        "MSR flags should update N/Z/C/V"
    );
}

#[test]
fn arm_swi_enters_supervisor_mode() {
    let test = execute_arm(0xEF00_0000, |_| {});
    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::Supervisor);
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    assert_eq!(test.gba.cpu.pc, 0x0000_0010);
}

#[test]
fn arm_negative_offset_load_store() {
    let base = 0x0300_0100;
    let mut test = execute_arm(0xE500_1004, |test| { // str r1, [r0, #-4]
        test.set_reg(0, base);
        test.set_reg(1, 0xCAFE_BABE);
    });
    assert_eq!(test.read_mem32(base - 4), 0xCAFE_BABE);

    let test = execute_arm(0xE510_1004, |test| { // ldr r1, [r0, #-4]
        test.set_reg(0, base);
        test.write_mem32(base - 4, 0x1234_5678);
    });
    assert_eq!(test.reg(1), 0x1234_5678);
}

#[test]
fn thumb_hi_register_operations_and_bx() {
    let test = execute_thumb(0x4488, |test| { // add r8, r1
        test.set_reg(1, 5);
        test.set_reg(8, 7);
    });
    assert_eq!(test.reg(8), 12);

    let test = execute_thumb(0x4588, |test| { // cmp r8, r1
        test.set_reg(1, 7);
        test.set_reg(8, 7);
    });
    assert_eq!(test.reg(8), 7);
    assert_ne!(test.cpsr() & (1 << 30), 0, "CMP should set Z");

    let test = execute_thumb(0x4688, |test| { // mov r8, r1
        test.set_reg(1, 0xCAFE_BABE);
    });
    assert_eq!(test.reg(8), 0xCAFE_BABE);

    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0x4700); // bx r0
    test.write_iwram32(0x0010, 0xE3A0_1007); // mov r1, #7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, CODE_BASE + 0x10);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    test.step_instruction();
    assert_eq!(test.reg(1), 7);
}

#[test]
fn thumb_mov_pc_switches_pipeline() {
    let mut test = TestGba::new();
    test.write_iwram16(0x0000, 0x4687); // mov pc, r0
    test.write_iwram16(0x0010, 0x2107); // movs r1, #7
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, CODE_BASE + 0x10);
    test.set_thumb_entry(CODE_BASE);
    test.step_instruction();
    test.step_instruction();
    assert_eq!(test.reg(1), 7);
}

#[test]
fn arm_ldm_s_bit_with_pc_restores_spsr_and_state() {
    let base = 0x0300_0100;
    let target = 0x0300_0200;
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE8F0_8000); // ldmia r0!, {pc}^
    test.gba.cpu.cpsr.mode = CpuMode::Supervisor;
    test.gba.cpu.spsr = u32::from(CpuMode::System as u32); // System mode, ARM state
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, target);
    test.step_instruction();

    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::System);
    assert_eq!(test.execution_state(), CpuExecutionState::Arm);
    assert_eq!(test.gba.cpu.pc, target + 8);
    assert_eq!(test.reg(0), base + 4);
}

#[test]
fn arm_ldm_stm_empty_and_base_in_list() {
    let base = 0x0300_0100;
    let target = 0x0300_0200;

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE890_0000); // ldmia r0, {}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, target);
    test.step_instruction();
    assert_eq!(test.gba.cpu.pc, target + 8);

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE880_0000); // stmia r0, {}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.read_mem32(base), CODE_BASE + 12);

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE8B0_0003); // ldmia r0!, {r0, r1}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_arm_entry(CODE_BASE);
    test.write_mem32(base, 0xAAAA_AAAA);
    test.write_mem32(base + 4, 0xBBBB_BBBB);
    test.step_instruction();
    assert_eq!(test.reg(0), 0xAAAA_AAAA);
    assert_eq!(test.reg(1), 0xBBBB_BBBB);

    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE8A0_0003); // stmia r0!, {r0, r1}
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.set_reg(0, base);
    test.set_reg(1, 0xCCCC_CCCC);
    test.set_arm_entry(CODE_BASE);
    test.step_instruction();
    assert_eq!(test.reg(0), base + 8);
    // The stored value for a base register that is also in the list is
    // implementation-defined on ARM7TDMI; only the writeback is guaranteed.
    assert_eq!(test.read_mem32(base + 4), 0xCCCC_CCCC);
}

#[test]
fn irq_entry_and_exception_return() {
    let mut test = TestGba::new();
    test.write_iwram32(0x0000, 0xE3A0_0001); // mov r0, #1
    test.gba.cpu.cpsr.mode = CpuMode::System;
    test.gba.cpu.cpsr.interrupt_i = false;
    test.set_arm_entry(CODE_BASE);

    // IRQ vector: subs pc, lr, #4
    test.gba.bios_rom[0x18..0x1C].copy_from_slice(&0xE25E_F004u32.to_le_bytes());

    test.gba.interrupt.global_enabled = true;
    test.gba.interrupt.enabled = 1 << 0; // VBlank
    test.gba.interrupt.pending = 1 << 0;
    test.gba.cpu_irq();

    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::Irq);
    assert_eq!(test.gba.cpu.pc, 0x20);
    assert!(test.gba.cpu.cpsr.interrupt_i);

    // Return from the exception.
    test.step_instruction();
    assert_eq!(test.gba.cpu.cpsr.mode, CpuMode::System);
    assert_eq!(test.gba.cpu.pc, CODE_BASE + 8);

    // The interrupted instruction should now execute.
    test.step_instruction();
    assert_eq!(test.reg(0), 1);
}

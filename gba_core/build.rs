use std::io::Write;
use std::{fs::File, path::Path};

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn bit(val: u32, n: usize) -> bool {
    ((val >> n) & 1) != 0
}

fn bit_range(val: u32, start: usize, end: usize) -> u32 {
    (val >> start) & ((1 << (end - start)) - 1)
}

fn bit_u16(val: u16, n: usize) -> bool {
    ((val >> n) & 1) != 0
}

fn bit_range_u16(val: u16, start: usize, end: usize) -> u16 {
    (val >> start) & ((1 << (end - start)) - 1) as u16
}

fn decode_arm_entry(inst: u32) -> String {
    match bit_range(inst, 25, 28) {
        0b000 if bit(inst, 4) && bit(inst, 7) => {
            if bit_range(inst, 4, 8) == 0b1001 && bit_range(inst, 23, 28) == 0b00000 {
                format!(
                    "arm_exec_mul::<{ACCUMULATE}, {SET_FLAGS}>",
                    ACCUMULATE = bool_str(bit(inst, 21)),
                    SET_FLAGS = bool_str(bit(inst, 20)),
                )
            } else if bit_range(inst, 4, 8) == 0b1001 && bit_range(inst, 23, 28) == 0b00001 {
                format!(
                    "arm_exec_mul_long::<{SIGNED}, {ACCUMULATE}, {SET_FLAGS}>",
                    SIGNED = bool_str(bit(inst, 22)),
                    ACCUMULATE = bool_str(bit(inst, 21)),
                    SET_FLAGS = bool_str(bit(inst, 20)),
                )
            } else if bit_range(inst, 4, 8) == 0b1001 && bit_range(inst, 23, 28) == 0b00010 {
                format!("arm_exec_swap::<{BYTE}>", BYTE = bool_str(bit(inst, 22)))
            } else {
                format!(
                    "arm_exec_ld_st_halfword_byte::<{PREINDEX}, {UP}, {IMMEDIATE}, {WRITEBACK}, {LOAD}, {SIGNED}, {HALFWORD}>",
                    PREINDEX = bool_str(bit(inst, 24)),
                    UP = bool_str(bit(inst, 23)),
                    IMMEDIATE = bool_str(bit(inst, 22)),
                    WRITEBACK = bool_str(bit(inst, 21)),
                    LOAD = bool_str(bit(inst, 20)),
                    SIGNED = bool_str(bit(inst, 6)),
                    HALFWORD = bool_str(bit(inst, 5)),
                )
            }
        }
        0b000 | 0b001 if bit_range(inst, 23, 25) != 0b10 || bit(inst, 20) => {
            format!(
                "arm_exec_alu::<{OPCODE}, {IMM}, {SETCOND}, {SHIFT_TYPE}, {REGSHIFT}>",
                OPCODE = bit_range(inst, 21, 25),
                IMM = bool_str(bit(inst, 25)),
                SETCOND = bool_str(bit(inst, 20)),
                SHIFT_TYPE = bit_range(inst, 5, 7),
                REGSHIFT = bool_str(bit(inst, 4) && !bit(inst, 25)),
            )
        }
        0b000 if (bit_range(inst, 23, 25) == 0b10 && !bit(inst, 20)) => {
            match (bit_range(inst, 4, 8), bit_range(inst, 21, 23)) {
                (0b0000, 0b00 | 0b10) => format!("arm_exec_mrs::<{R}>", R = bool_str(bit(inst, 22))),
                (0b0000, 0b01 | 0b11) => format!("arm_exec_msr::<{R}, false>", R = bool_str(bit(inst, 22))),
                (0b0001, 0b01) => "arm_exec_branch_exchange".to_string(),
                _ => "arm_unimplemented".to_string(),
            }
        }
        0b001 if (bit_range(inst, 20, 25) & 0b11011) == 0b10010 => {
            format!("arm_exec_msr::<{R}, true>", R = bool_str(bit(inst, 22)))
        }
        0b101 => {
            format!("arm_exec_branch::<{LINK}>", LINK = bool_str(bit(inst, 24)))
        }
        0b010 | 0b011 => {
            format!(
                "arm_exec_ldr_str_word_byte::<{IMMEDIATE}, {PREINDEX}, {UP}, {BYTE}, {WRITEBACK}, {LOAD}, {SHIFT_TYPE}>",
                IMMEDIATE = bool_str(bit(inst, 25)),
                PREINDEX = bool_str(bit(inst, 24)),
                UP = bool_str(bit(inst, 23)),
                BYTE = bool_str(bit(inst, 22)),
                WRITEBACK = bool_str(bit(inst, 21)),
                LOAD = bool_str(bit(inst, 20)),
                SHIFT_TYPE = bit_range(inst, 5, 7),
            )
        }
        0b100 => {
            format!(
                "arm_exec_ldm_stm::<{PREINDEX}, {UP}, {S}, {WRITEBACK}, {LOAD}>",
                PREINDEX = bool_str(bit(inst, 24)),
                UP = bool_str(bit(inst, 23)),
                S = bool_str(bit(inst, 22)),
                WRITEBACK = bool_str(bit(inst, 21)),
                LOAD = bool_str(bit(inst, 20)),
            )
        }
        0b111 if bit(inst, 24) => {
            "arm_exec_swi".to_string()
        }
        _ => "arm_unimplemented".to_string(),
    }
}

fn decode_thumb_entry(inst: u16) -> String {
    if u16_matches(inst, "000 ** ***** *** ***") {
        let opcode = bit_range_u16(inst, 11, 13);
        if opcode != 0b11 {
            format!("thumb_exec_shift_imm::<{OPCODE}>", OPCODE = opcode)
        } else {
            format!(
                "thumb_exec_add_sub::<{IMM}, {SUB}>",
                IMM = bool_str(bit_u16(inst, 10)),
                SUB = bool_str(bit_u16(inst, 9)),
            )
        }
    } else if u16_matches(inst, "001 ** *** ********") {
        format!(
            "thumb_exec_alu_immediate::<{OPCODE}, {REG_D}>",
            OPCODE = bit_range_u16(inst, 11, 13),
            REG_D = bit_range_u16(inst, 8, 11),
        )
    } else if u16_matches(inst, "010000 **** *** ***") {
        format!(
            "thumb_exec_alu_register::<{OPCODE}>",
            OPCODE = bit_range_u16(inst, 6, 10)
        )
    } else if u16_matches(inst, "010001 ** * * *** ***") {
        let opcode = bit_range_u16(inst, 8, 10);
        if opcode == 0b11 {
            format!("thumb_exec_bx::<{MSB_REG_S}>", MSB_REG_S = bool_str(bit_u16(inst, 6)))
        } else {
            format!(
                "thumb_exec_hireg::<{OPCODE}, {MSB_REG_D}, {MSB_REG_S}>",
                OPCODE = bit_range_u16(inst, 8, 10),
                MSB_REG_D = bool_str(bit_u16(inst, 7)),
                MSB_REG_S = bool_str(bit_u16(inst, 6)),
            )
        }
    } else if u16_matches(inst, "01001 *** ********") {
        "thumb_exec_load_pc_relative".to_string()
    } else if u16_matches(inst, "0101 *** *** *** ***") {
        format!(
            "thumb_exec_ldr_str_reg_offset::<{OP}>",
            OP = bit_range_u16(inst, 9, 12)
        )
    } else if u16_matches(inst, "011 * * ***** *** ***") {
        format!(
            "thumb_exec_ldr_str_imm::<{BYTE}, false, {LOAD}>",
            BYTE = bool_str(bit_u16(inst, 12)),
            LOAD = bool_str(bit_u16(inst, 11)),
        )
    } else if u16_matches(inst, "1000 * ***** *** ***") {
        format!(
            "thumb_exec_ldr_str_imm::<false, true, {LOAD}>",
            LOAD = bool_str(bit_u16(inst, 11)),
        )
    } else if u16_matches(inst, "1001 * *** ********") {
        format!("thumb_exec_ldr_str_sp::<{LOAD}>", LOAD = bool_str(bit_u16(inst, 11)))
    } else if u16_matches(inst, "1010 * *** ********") {
        format!("thumb_exec_address_calc::<{SP}>", SP = bool_str(bit_u16(inst, 11)))
    } else if u16_matches(inst, "10110000 * *******") {
        format!("thumb_exec_adjust_sp::<{SUB}>", SUB = bool_str(bit_u16(inst, 7)))
    } else if u16_matches(inst, "1101 **** ********") {
        let middle = bit_range_u16(inst, 8, 12);
        match middle {
            0b1110 => "thumb_unimplemented".to_string(),
            0b1111 => "thumb_exec_swi".to_string(),
            _ => format!("thumb_exec_branch_conditional::<{COND}>", COND = middle),
        }
    } else if u16_matches(inst, "1011 * 10 * ********") {
        format!(
            "thumb_exec_push_pop::<{POP}, {PC_LR}>",
            POP = bool_str(bit_u16(inst, 11)),
            PC_LR = bool_str(bit_u16(inst, 8))
        )
    } else if u16_matches(inst, "1100 * *** ********") {
        format!("thumb_exec_ldr_str_multiple::<{LOAD}>", LOAD = bool_str(bit_u16(inst, 11)))
    } else if u16_matches(inst, "11100 ***********") {
        "thumb_exec_branch".to_string()
    } else if u16_matches(inst, "1111 * ***********") {
        format!("thumb_exec_branch_link::<{SUFFIX}>", SUFFIX = bool_str(bit_u16(inst, 11)))
    } else {
        "thumb_unimplemented".to_string()
    }
}

fn u16_matches(num: u16, pattern: &str) -> bool {
    let mut index = 15;
    for char in pattern.chars() {
        if char == '1' || char == '0' || char == '*' {
            let bit_val = ((num >> index) & 1) != 0;
            match char {
                '1' if !bit_val => {
                        return false;
                }
                '0' if bit_val => {
                        return false;
                }
                _ => {}
            }
            if index == 0 {
                return true;
            } else {
                index -= 1;
            }
        }
    }
    unreachable!("Got to the end of the pattern without hitting all bits");
}

fn generate_arm_table(file: &mut File) -> std::io::Result<()> {
    writeln!(file, "static ARM_HANDLERS: [ArmHandler; 4096] = [")?;
    for key in 0u32..4096 {
        let inst = ((key & 0xff0) << 16) | ((key & 0xf) << 4);
        let handler = decode_arm_entry(inst);
        writeln!(file, "    {},", handler)?;
    }
    writeln!(file, "];")?;
    Ok(())
}

fn generate_thumb_table(file: &mut File) -> std::io::Result<()> {
    writeln!(file, "static THUMB_HANDLERS: [ThumbHandler; 1024] = [")?;
    for key in 0u16..1024 {
        let inst = key << 6;
        let handler = decode_thumb_entry(inst);
        writeln!(file, "    {},", handler)?;
    }
    writeln!(file, "];")?;
    Ok(())
}

fn main() {
    let output_dir = std::env::var_os("OUT_DIR").unwrap();
    let arm_path = Path::new(&output_dir).join("arm_table.rs");
    let mut arm_file = File::create(&arm_path).unwrap();
    generate_arm_table(&mut arm_file).unwrap();
    let thumb_path = Path::new(&output_dir).join("thumb_table.rs");
    let mut thumb_file = File::create(&thumb_path).unwrap();
    generate_thumb_table(&mut thumb_file).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

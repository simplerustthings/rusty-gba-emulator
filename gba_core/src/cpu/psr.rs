use super::{CpuExecutionState, CpuMode};
use crate::impl_save_state_fields;

/// Program status register.
#[derive(Copy, Clone, Debug)]
pub struct ProgramStatusRegister {
    pub cond_flag_n: bool,
    pub cond_flag_z: bool,
    pub cond_flag_c: bool,
    pub cond_flag_v: bool,
    pub interrupt_i: bool,
    pub interrupt_f: bool,
    pub execution_state: CpuExecutionState,
    pub mode: CpuMode,
}

impl_save_state_fields!(ProgramStatusRegister {
    cond_flag_n,
    cond_flag_z,
    cond_flag_c,
    cond_flag_v,
    interrupt_i,
    interrupt_f,
    execution_state,
    mode,
});

impl ProgramStatusRegister {
    pub fn new() -> Self {
        ProgramStatusRegister {
            cond_flag_n: false,
            cond_flag_z: false,
            cond_flag_c: false,
            cond_flag_v: false,
            interrupt_i: true,
            interrupt_f: true,
            execution_state: CpuExecutionState::Arm,
            mode: CpuMode::Supervisor,
        }
    }
}

impl From<ProgramStatusRegister> for u32 {
    #[inline(always)]
    fn from(val: ProgramStatusRegister) -> u32 {
        (val.cond_flag_n as u32) << 31
            | (val.cond_flag_z as u32) << 30
            | (val.cond_flag_c as u32) << 29
            | (val.cond_flag_v as u32) << 28
            | (val.interrupt_i as u32) << 7
            | (val.interrupt_f as u32) << 6
            | (val.execution_state as u32) << 5
            | val.mode as u32
    }
}

impl From<u32> for ProgramStatusRegister {
    #[inline(always)]
    fn from(val: u32) -> Self {
        ProgramStatusRegister {
            cond_flag_n: (val >> 31) & 1 != 0,
            cond_flag_z: (val >> 30) & 1 != 0,
            cond_flag_c: (val >> 29) & 1 != 0,
            cond_flag_v: (val >> 28) & 1 != 0,
            interrupt_i: (val >> 7) & 1 != 0,
            interrupt_f: (val >> 6) & 1 != 0,
            execution_state: if (val >> 5) & 1 != 0 { CpuExecutionState::Thumb } else { CpuExecutionState::Arm },
            mode: CpuMode::from_u32(val & 0x1F),
        }
    }
}

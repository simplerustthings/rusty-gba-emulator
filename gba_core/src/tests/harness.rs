#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::bus::MemoryAccessType;
use crate::cpu::CpuExecutionState;
use crate::mem::Memory;
use crate::{Gba, KeypadState, Rom};

/// A lightweight headless harness for unit and integration tests.
pub(crate) struct TestGba {
    pub(crate) gba: Gba,
}

impl TestGba {
    /// New emulator with an empty ROM/BIOS, starting at the cartridge entry
    /// point in System mode.
    pub(crate) fn new() -> Self {
        let gba = Gba::builder(Box::new([0; 0x4000]), Rom::empty())
            .skip_bios(true)
            .build();
        Self { gba }
    }

    /// New emulator with the supplied BIOS and ROM.
    pub(crate) fn with_rom(bios: &[u8], rom: &[u8], skip_bios: bool) -> Self {
        let gba = Gba::builder(bios.into(), Rom::new(rom))
            .skip_bios(skip_bios)
            .build();
        Self { gba }
    }

    pub(crate) fn step_instruction(&mut self) -> usize {
        let before = self.gba.scheduler.timestamp();
        self.gba.cpu_step();
        self.gba.scheduler.timestamp() - before
    }

    /// Step while also processing scheduler events (IRQs, PPU, DMA, timers).
    pub(crate) fn step_emulated(&mut self) {
        self.gba.debug_step();
    }

    pub(crate) fn run_frames(&mut self, frames: usize, render: bool) {
        for _ in 0..frames {
            self.gba.emulate_frame(render);
        }
    }

    pub(crate) fn set_arm_entry(&mut self, address: u32) {
        self.gba.cpu.cpsr.execution_state = CpuExecutionState::Arm;
        self.gba.cpu_jump(address);
    }

    pub(crate) fn set_thumb_entry(&mut self, address: u32) {
        self.gba.cpu.cpsr.execution_state = CpuExecutionState::Thumb;
        self.gba.cpu_jump(address);
    }

    pub(crate) fn write_iwram8(&mut self, offset: u32, value: u8) {
        self.gba.iwram.write_8(offset & 0x7FFF, value);
    }

    pub(crate) fn write_iwram16(&mut self, offset: u32, value: u16) {
        self.gba.iwram.write_16(offset & 0x7FFF, value);
    }

    pub(crate) fn write_iwram32(&mut self, offset: u32, value: u32) {
        self.gba.iwram.write_32(offset & 0x7FFF, value);
    }

    pub(crate) fn write_mem8(&mut self, address: u32, value: u8) {
        self.gba
            .cpu_store8(address, value, MemoryAccessType::Sequential);
    }

    pub(crate) fn write_mem16(&mut self, address: u32, value: u16) {
        self.gba
            .cpu_store16(address, value, MemoryAccessType::Sequential);
    }

    pub(crate) fn write_mem32(&mut self, address: u32, value: u32) {
        self.gba
            .cpu_store32(address, value, MemoryAccessType::Sequential);
    }

    pub(crate) fn read_mem8(&mut self, address: u32) -> u8 {
        self.gba
            .cpu_load8(address, MemoryAccessType::Sequential)
    }

    pub(crate) fn read_mem16(&mut self, address: u32) -> u16 {
        self.gba
            .cpu_load16(address, MemoryAccessType::Sequential)
    }

    pub(crate) fn read_mem32(&mut self, address: u32) -> u32 {
        self.gba
            .cpu_load32(address, MemoryAccessType::Sequential)
    }

    pub(crate) fn reg(&self, index: usize) -> u32 {
        self.gba.cpu.gpr[index]
    }

    pub(crate) fn set_reg(&mut self, index: usize, value: u32) {
        self.gba.cpu.gpr[index] = value;
    }

    pub(crate) fn cpsr(&self) -> u32 {
        self.gba.cpu.cpsr.into()
    }

    pub(crate) fn execution_state(&self) -> CpuExecutionState {
        self.gba.cpu.cpsr.execution_state
    }

    pub(crate) fn cycles(&self) -> usize {
        self.gba.scheduler.timestamp()
    }

    pub(crate) fn set_keypad(&mut self, state: KeypadState) {
        self.gba.set_keypad_state(state);
    }

    pub(crate) fn framebuffer(&self) -> &[u32] {
        self.gba.framebuffer()
    }

    pub(crate) fn debug_messages(&self) -> &[String] {
        self.gba.debug_messages()
    }
}

/// Path to a file in the repository root (the parent of the crate directory).
pub(crate) fn workspace_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gba_core should be inside the workspace")
        .join(name)
}

/// Load `name` from the workspace root, returning `None` when the asset is
/// not available. Tests that need proprietary ROMs use this to skip cleanly
/// in a source-only checkout.
pub(crate) fn load_workspace_file(name: &str) -> Option<Vec<u8>> {
    match std::fs::read(workspace_file(name)) {
        Ok(data) => Some(data),
        Err(error) => {
            eprintln!("skipping test: could not read {name}: {error}");
            None
        }
    }
}

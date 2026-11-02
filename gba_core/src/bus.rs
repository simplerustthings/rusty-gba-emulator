#![allow(unused)]

use crate::{impl_save_state_fields, impl_save_state_enum};

use crate::{cpu::CpuExecutionState, io::WaitControl, Addr, Gba, Memory};

const BIOS_SIZE: u32 = 0x4000;

/// Map a VRAM address to the flat array index, aliasing the OBJ mirror (0x18000..0x1FFFF -> 0x10000..0x17FFF).
/// Branchless: subtracts 0x8000 when both bits 16 and 15 are set.
#[inline(always)]
fn vram_index(addr: Addr) -> usize {
    let offset = (addr & 0x1FFFF) as usize;
    let mask = ((offset & 0x18000) == 0x18000) as usize;
    offset.wrapping_sub(mask << 15)
}

/// State for the system memory bus.
pub struct Bus {
    pub(crate) wait_s16: [usize; 16],
    pub(crate) wait_n16: [usize; 16],
    pub(crate) wait_s32: [usize; 16],
    pub(crate) wait_n32: [usize; 16],

    /// Last 32-bit value on the data bus (used for DMA BIOS open bus).
    pub(crate) last_data_bus: u32,

    /// Whether the most recent bus access was performed by the DMA controller.
    /// Open-bus reads after a DMA return the DMA latch (NBA `last_access & Dma`).
    pub(crate) last_access_dma: bool,

    /// Last 32-bit value loaded from BIOS by the CPU (used for CPU BIOS open bus).
    pub(crate) bios_prefetch: u32,

    /// DMA nesting counter (SRAM reads return 0 when == 1, matching mGBA).
    pub(crate) performing_dma: u8,

    /// Set while the DMA controller is transferring (CPU prefetch does not
    /// apply to DMA bus accesses).
    pub(crate) in_dma: bool,

    /// Cartridge prefetch buffer state (NanoBoyAdvance `Bus::Prefetch`).
    pub(crate) prefetch_active: bool,
    pub(crate) prefetch_head: u32,
    pub(crate) prefetch_last: u32,
    pub(crate) prefetch_count: i32,
    pub(crate) prefetch_capacity: i32,
    pub(crate) prefetch_width: u32,
    pub(crate) prefetch_countdown: i32,
    pub(crate) prefetch_duty: i32,
    pub(crate) prefetch_thumb: bool,
    /// Set when WAITCNT transitions prefetch on -> off; the next ROM access is
    /// forced non-sequential.
    pub(crate) prefetch_was_disabled: bool,
}

impl_save_state_fields!(Bus {
    wait_s16,
    wait_n16,
    wait_s32,
    wait_n32,
    last_data_bus,
    last_access_dma,
    bios_prefetch,
    performing_dma,
    in_dma,
    prefetch_active,
    prefetch_head,
    prefetch_last,
    prefetch_count,
    prefetch_capacity,
    prefetch_width,
    prefetch_countdown,
    prefetch_duty,
    prefetch_thumb,
    prefetch_was_disabled,
});

/// Memory access types.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MemoryAccessType {
    Sequential,
    NonSequential,
}

impl_save_state_enum!(MemoryAccessType { Sequential, NonSequential });

#[derive(Copy, Clone, Debug)]
enum MemoryAccessSize {
    Mem8 = 0,
    Mem16 = 1,
    Mem32 = 2,
}

pub const REGION_BIOS: Addr = 0x0;
pub const REGION_EWRAM: Addr = 0x2;
pub const REGION_IWRAM: Addr = 0x3;
pub const REGION_IO: Addr = 0x4;
pub const REGION_PALETTE: Addr = 0x5;
pub const REGION_VRAM: Addr = 0x6;
pub const REGION_OAM: Addr = 0x7;
pub const REGION_CART_WS0_A: Addr = 0x8;
pub const REGION_CART_WS0_B: Addr = 0x9;
pub const REGION_CART_WS1_A: Addr = 0xA;
pub const REGION_CART_WS1_B: Addr = 0xB;
pub const REGION_CART_WS2_A: Addr = 0xC;
pub const REGION_CART_WS2_B: Addr = 0xD;
pub const REGION_SRAM: Addr = 0xE;
pub const REGION_CART_UNUSED: Addr = 0xF;

/// Address to region.
#[inline(always)]
pub fn region_from_address(addr: Addr) -> u32 {
    (addr & 0xFF00_0000) >> 24
}

impl Bus {
    /// New bus in the initial state.
    pub fn new() -> Bus {
        let mut bus = Bus {
            wait_s16: [1; 16],
            wait_n16: [1; 16],
            wait_s32: [1; 16],
            wait_n32: [1; 16],
            last_data_bus: 0,
            last_access_dma: false,
            bios_prefetch: 0,
            performing_dma: 0,
            in_dma: false,
            prefetch_active: false,
            prefetch_head: 0,
            prefetch_last: 0,
            prefetch_count: 0,
            prefetch_capacity: 4,
            prefetch_width: 4,
            prefetch_countdown: 0,
            prefetch_duty: 0,
            prefetch_thumb: false,
            prefetch_was_disabled: false,
        };

        bus.wait_s16[REGION_BIOS as usize] = 1;
        bus.wait_n16[REGION_BIOS as usize] = 1;
        bus.wait_s32[REGION_BIOS as usize] = 1;
        bus.wait_n32[REGION_BIOS as usize] = 1;

        bus.wait_s16[REGION_IWRAM as usize] = 1;
        bus.wait_n16[REGION_IWRAM as usize] = 1;
        bus.wait_s32[REGION_IWRAM as usize] = 1;
        bus.wait_n32[REGION_IWRAM as usize] = 1;

        bus.wait_s16[REGION_IO as usize] = 1;
        bus.wait_n16[REGION_IO as usize] = 1;
        bus.wait_s32[REGION_IO as usize] = 1;
        bus.wait_n32[REGION_IO as usize] = 1;

        bus.wait_s16[REGION_VRAM as usize] = 1;
        bus.wait_n16[REGION_VRAM as usize] = 1;
        bus.wait_s32[REGION_VRAM as usize] = 2;
        bus.wait_n32[REGION_VRAM as usize] = 2;

        bus.wait_s16[REGION_OAM as usize] = 1;
        bus.wait_n16[REGION_OAM as usize] = 1;
        bus.wait_s32[REGION_OAM as usize] = 1;
        bus.wait_n32[REGION_OAM as usize] = 1;

        bus.wait_s16[REGION_EWRAM as usize] = 3;
        bus.wait_n16[REGION_EWRAM as usize] = 3;
        bus.wait_s32[REGION_EWRAM as usize] = 6;
        bus.wait_n32[REGION_EWRAM as usize] = 6;

        bus.wait_s16[REGION_PALETTE as usize] = 1;
        bus.wait_n16[REGION_PALETTE as usize] = 1;
        bus.wait_s32[REGION_PALETTE as usize] = 2;
        bus.wait_n32[REGION_PALETTE as usize] = 2;

        bus.update_waitcnt(WaitControl(0));
        bus
    }

    /// Update cycle timing tables after WAITCNT is updated.
    pub(crate) fn update_waitcnt(&mut self, waitcnt: WaitControl) {
        let ws0_n = [4, 3, 2, 8][waitcnt.ws0_nonsequential() as usize];
        let ws0_s = [2, 1][waitcnt.ws0_sequential() as usize];
        let ws1_n = [4, 3, 2, 8][waitcnt.ws1_nonsequential() as usize];
        let ws1_s = [4, 1][waitcnt.ws1_sequential() as usize];
        let ws2_n = [4, 3, 2, 8][waitcnt.ws2_nonsequential() as usize];
        let ws2_s = [8, 1][waitcnt.ws2_sequential() as usize];

        let sram = [4, 3, 2, 8][waitcnt.sram() as usize];
        let wait_n = [ws0_n, ws1_n, ws2_n];
        let wait_s = [ws0_s, ws1_s, ws2_s];

        for region in REGION_CART_WS0_A..=REGION_CART_WS2_B {
            let ws = ((region - REGION_CART_WS0_A) / 2) as usize;
            self.wait_n16[region as usize] = 1 + wait_n[ws];
            self.wait_s16[region as usize] = 1 + wait_s[ws];
            self.wait_n32[region as usize] = 1 + wait_n[ws] + 1 + wait_s[ws];
            self.wait_s32[region as usize] = 1 + wait_s[ws] + 1 + wait_s[ws];
        }
        for region in REGION_SRAM..=REGION_CART_UNUSED {
            self.wait_n16[region as usize] = 1 + sram;
            self.wait_s16[region as usize] = 1 + sram;
            self.wait_n32[region as usize] = 1 + sram;
            self.wait_s32[region as usize] = 1 + sram;
        }
    }
}

impl Gba {
    /// Total cycles for one memory access (NanoBoyAdvance wait tables).
    /// `size>>1` maps Mem8|Mem16 -> 0, Mem32 -> 1.
    #[inline(always)]
    fn access_cycles(&self, region: u32, size: MemoryAccessSize, access: MemoryAccessType) -> usize {
        let r = (region as usize) & 0xF;
        match (size as usize >> 1, access as usize) {
            (0, 0) => self.bus.wait_s16[r],
            (0, 1) => self.bus.wait_n16[r],
            (1, 0) => self.bus.wait_s32[r],
            _ => self.bus.wait_n32[r],
        }
    }

    /// Advance the bus clock and let the cartridge prefetch unit fill its
    /// buffer during the elapsed cycles (NanoBoyAdvance `Bus::Step`).
    #[inline(always)]
    pub(crate) fn bus_step(&mut self, cycles: usize) {
        if cycles == 0 {
            return;
        }
        self.advance(cycles);
        if self.bus.prefetch_active {
            self.bus.prefetch_countdown -= cycles as i32;
            while self.bus.prefetch_countdown <= 0 {
                self.bus.prefetch_count += 1;
                if self.io.waitcnt.prefetch()
                    && self.bus.prefetch_count < self.bus.prefetch_capacity
                {
                    self.bus.prefetch_last =
                        self.bus.prefetch_last.wrapping_add(self.bus.prefetch_width);
                    self.bus.prefetch_countdown += self.bus.prefetch_duty;
                } else {
                    break;
                }
            }
        }
    }

    /// Discard the prefetch buffer on a non-code access
    /// (NanoBoyAdvance `Bus::StopPrefetch`). A halfword fetch that is about to
    /// complete costs one bus turnaround cycle.
    #[inline(always)]
    pub(crate) fn stop_prefetch(&mut self) {
        if self.bus.prefetch_active {
            let r15 = self.cpu.pc;
            if (0x0800_0000..=0x0DFF_FFFF).contains(&r15) {
                let half_duty_plus_one = (self.bus.prefetch_duty >> 1) + 1;
                let countdown = self.bus.prefetch_countdown;
                if countdown == 1
                    || (!self.bus.prefetch_thumb && countdown == half_duty_plus_one)
                {
                    self.bus_step(1);
                }
            }
            self.bus.prefetch_active = false;
        }
    }

    /// Charge an opcode fetch, consuming the prefetch buffer where possible
    /// (NanoBoyAdvance `Bus::Prefetch`).
    fn prefetch_fetch(&mut self, addr: Addr, cycles: usize) {
        // Bus handoff (NanoBoyAdvance Bus::Read): a CPU access waits for any
        // active DMA burst to finish before using the bus.
        if !self.bus.in_dma && self.dma_active() {
            self.dma_step();
        }
        let page = region_from_address(addr);
        let is_rom = (REGION_CART_WS0_A..=REGION_CART_WS2_B).contains(&page);

        if !is_rom {
            self.stop_prefetch();
            self.bus_step(cycles);
            return;
        }

        if self.bus.prefetch_active {
            // Requested address is the head of the buffer.
            if self.bus.prefetch_count != 0 && addr == self.bus.prefetch_head {
                self.bus.prefetch_count -= 1;
                self.bus.prefetch_head =
                    self.bus.prefetch_head.wrapping_add(self.bus.prefetch_width);
                self.bus_step(1);
                return;
            }
            // Requested address is currently being fetched.
            if self.bus.prefetch_countdown > 0 && addr == self.bus.prefetch_last {
                let countdown = self.bus.prefetch_countdown as usize;
                self.bus_step(countdown);
                self.bus.prefetch_head = self.bus.prefetch_last;
                self.bus.prefetch_count = 0;
                return;
            }
        }

        let mut cycles = cycles;
        self.stop_prefetch();

        // Disabling the buffer forces the next ROM access to be
        // non-sequential (NanoBoyAdvance's `prefetch_buffer_was_disabled`).
        if self.bus.prefetch_was_disabled {
            let p = page as usize;
            if cycles == self.bus.wait_s16[p] {
                cycles = self.bus.wait_n16[REGION_CART_WS0_A as usize];
            } else if cycles == self.bus.wait_s32[p] {
                cycles = self.bus.wait_n32[REGION_CART_WS0_A as usize];
            }
            self.bus.prefetch_was_disabled = false;
        }

        self.bus_step(cycles);

        if self.io.waitcnt.prefetch() {
            let thumb = matches!(
                self.cpu.cpsr.execution_state,
                crate::cpu::CpuExecutionState::Thumb
            );
            self.bus.prefetch_active = true;
            self.bus.prefetch_count = 0;
            self.bus.prefetch_thumb = thumb;
            if thumb {
                self.bus.prefetch_width = 2;
                self.bus.prefetch_capacity = 8;
                self.bus.prefetch_duty = self.bus.wait_s16[page as usize] as i32;
            } else {
                self.bus.prefetch_width = 4;
                self.bus.prefetch_capacity = 4;
                self.bus.prefetch_duty = self.bus.wait_s32[page as usize] as i32;
            }
            self.bus.prefetch_countdown = self.bus.prefetch_duty;
            self.bus.prefetch_last = addr.wrapping_add(self.bus.prefetch_width);
            self.bus.prefetch_head = self.bus.prefetch_last;
        }
    }

    /// Charge a data access: the prefetch unit continues on non-Game-Pak
    /// memory (it is a different bus) and is stopped by ROM/SRAM accesses.
    #[inline(always)]
    fn data_access_cycles(&mut self, region: u32, cycles: usize) {
        if !self.bus.in_dma && self.dma_active() {
            self.dma_step();
        }
        if region >= REGION_CART_WS0_A {
            self.stop_prefetch();
        }
        self.bus_step(cycles);
    }

    /// Read a 32 bit value from the bus as an instruction fetch.
    #[inline(always)]
    pub(crate) fn cpu_fetch32(&mut self, addr: Addr, access: MemoryAccessType) -> u32 {
        self.cpu_read32(addr, access, false)
    }

    /// Read a 16 bit value from the bus as an instruction fetch.
    #[inline(always)]
    pub(crate) fn cpu_fetch16(&mut self, addr: Addr, access: MemoryAccessType) -> u16 {
        self.cpu_read16(addr, access, false)
    }

    #[inline(always)]
    fn cpu_read32(&mut self, addr: Addr, access: MemoryAccessType, is_data: bool) -> u32 {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem32, access);
        if is_data {
            self.data_access_cycles(region, cycles);
        } else {
            self.prefetch_fetch(addr, cycles);
        }

        let aligned = addr & !0b11;
        let value = match region {
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_read_32(addr),
            REGION_IWRAM => self.iwram.read_32(aligned & 0x7FFF),
            REGION_EWRAM => self.ewram.read_32(aligned & 0x3FFFF),
            REGION_IO => self.io_read_32(aligned),
            REGION_VRAM => self.ppu.vram.read_32(vram_index(aligned) as Addr),
            REGION_PALETTE => self.ppu.palette.read_32(aligned & 0x3FF),
            REGION_OAM => self.ppu.oam.read_32(aligned & 0x3FF),
            REGION_BIOS if aligned <= (BIOS_SIZE - 4) => self.bios_load(aligned),
            _ => self.unused_load(aligned),
        };
        if region != REGION_BIOS {
            self.bus.last_data_bus = value;
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
        value
    }

    #[inline(always)]
    fn cpu_read16(&mut self, addr: Addr, access: MemoryAccessType, is_data: bool) -> u16 {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem16, access);
        if is_data {
            self.data_access_cycles(region, cycles);
        } else {
            self.prefetch_fetch(addr, cycles);
        }

        let aligned = addr & !0b1;
        let value = match region {
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_read_16(addr),
            REGION_IWRAM => self.iwram.read_16(aligned & 0x7FFF),
            REGION_EWRAM => self.ewram.read_16(aligned & 0x3FFFF),
            REGION_IO => self.io_read_16(aligned),
            REGION_VRAM => self.ppu.vram.read_16(vram_index(aligned) as Addr),
            REGION_PALETTE => self.ppu.palette.read_16(aligned & 0x3FF),
            REGION_OAM => self.ppu.oam.read_16(aligned & 0x3FF),
            REGION_BIOS if aligned <= (BIOS_SIZE - 2) => self.bios_load(aligned) as u16,
            _ => self.unused_load(aligned) as u16,
        };
        if region != REGION_BIOS {
            self.bus.last_data_bus = (self.bus.last_data_bus & 0xFFFF0000) | (value as u32);
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
        value
    }

    /// Read a 32 bit value from the bus.
    #[inline(always)]
    pub(crate) fn cpu_load32(&mut self, addr: Addr, access: MemoryAccessType) -> u32 {
        self.cpu_read32(addr, access, true)
    }

    /// Read a 16 bit value from the bus.
    #[inline(always)]
    pub(crate) fn cpu_load16(&mut self, addr: Addr, access: MemoryAccessType) -> u16 {
        self.cpu_read16(addr, access, true)
    }

    /// Read an 8 bit value from the bus.
    #[inline(always)]
    pub(crate) fn cpu_load8(&mut self, addr: Addr, access: MemoryAccessType) -> u8 {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem8, access);
        self.data_access_cycles(region, cycles);

        let value = match region {
            REGION_IWRAM => self.iwram.read_8(addr & 0x7FFF),
            REGION_EWRAM => self.ewram.read_8(addr & 0x3FFFF),
            REGION_IO => self.io_read_8(addr),
            REGION_VRAM => self.ppu.vram.read_8(vram_index(addr) as Addr),
            REGION_PALETTE => self.ppu.palette.read_8(addr & 0x3FF),
            REGION_OAM => self.ppu.oam.read_8(addr & 0x3FF),
            REGION_BIOS if addr <= (BIOS_SIZE - 1) => self.bios_load(addr) as u8,
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_read_8(addr),
            _ => self.unused_load(addr) as u8,
        };
        if region != REGION_BIOS {
            self.bus.last_data_bus = (self.bus.last_data_bus & 0xFFFFFF00) | (value as u32);
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
        value
    }

    /// Store a 32 bit value to the bus.
    #[inline(always)]
    pub(crate) fn cpu_store32(&mut self, addr: Addr, data: u32, access: MemoryAccessType) {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem32, access);
        self.data_access_cycles(region, cycles);

        match region {
            REGION_IWRAM => self.iwram.write_32(addr & 0x7FFF, data),
            REGION_EWRAM => self.ewram.write_32(addr & 0x3FFFF, data),
            REGION_IO => self.io_write_32(addr, data),
            REGION_VRAM => self.ppu.vram.write_32(vram_index(addr) as Addr, data),
            REGION_PALETTE => self.ppu.palette.write_32(addr & 0x3FF, data),
            REGION_OAM => self.ppu.oam.write_32(addr & 0x3FF, data),
            REGION_BIOS => {}
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_write_32(addr, data),
            _ => {
                #[cold]
                fn cold_store32(addr: Addr, data: u32, pc: u32) {
                    eprintln!("Bad memory store (32 bit) at {:X}, data {:X}, PC={:08X}", addr, data, pc);
                }
                cold_store32(addr, data, self.cpu.pc);
            }
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
    }

    /// Store a 16 bit value to the bus.
    #[inline(always)]
    pub(crate) fn cpu_store16(&mut self, addr: Addr, data: u16, access: MemoryAccessType) {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem16, access);
        self.data_access_cycles(region, cycles);

        match region {
            REGION_IWRAM => self.iwram.write_16(addr & 0x7FFF, data),
            REGION_EWRAM => self.ewram.write_16(addr & 0x3FFFF, data),
            REGION_IO => self.io_write_16(addr, data),
            REGION_VRAM => self.ppu.vram.write_16(vram_index(addr) as Addr, data),
            REGION_PALETTE => self.ppu.palette.write_16(addr & 0x3FF, data),
            REGION_OAM => self.ppu.oam.write_16(addr & 0x3FF, data),
            REGION_BIOS => {}
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_write_16(addr, data),
            _ => {
                #[cold]
                fn cold_store16(addr: Addr, data: u16) {
                    eprintln!("Bad memory store (16 bit) at {:X}, data {:X}", addr, data);
                }
                cold_store16(addr, data);
            }
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
    }

    /// Store an 8 bit value to the bus.
    #[inline(always)]
    pub(crate) fn cpu_store8(&mut self, addr: Addr, data: u8, access: MemoryAccessType) {
        let region = region_from_address(addr);
        let cycles = self.access_cycles(region, MemoryAccessSize::Mem8, access);
        self.data_access_cycles(region, cycles);

        match region {
            REGION_IWRAM => self.iwram.write_8(addr & 0x7FFF, data),
            REGION_EWRAM => self.ewram.write_8(addr & 0x3FFFF, data),
            REGION_IO => self.io_write_8(addr, data),
            REGION_VRAM => {
                let offset = addr & 0x1FFFF;
                if offset < 0x10000 {
                    // BG VRAM: 8-bit write duplicates to both halves of the halfword
                    let a = (addr & 0x1FFFE) as usize;
                    self.ppu.vram[a] = data;
                    self.ppu.vram[a | 1] = data;
                }
                // OBJ VRAM (0x10000-0x1FFFF): 8-bit writes are no-op
            }
            REGION_PALETTE => {
                let addr = (addr & 0x3FE) as usize;
                self.ppu.palette[addr] = data;
                self.ppu.palette[addr | 1] = data;
            }
            REGION_OAM => {
                // OAM does not support 8-bit writes
            }
            REGION_BIOS => {}
            REGION_CART_WS0_A..=REGION_CART_UNUSED => self.cart_write_8(addr, data),
            _ => {
                #[cold]
                fn cold_store8(addr: Addr, data: u8) {
                    eprintln!("Bad memory store (8 bit) at {:X}, data {:X}", addr, data);
                }
                cold_store8(addr, data);
            }
        }
        if !self.bus.in_dma {
            self.bus.last_access_dma = false;
        }
    }

    /// Load from the BIOS region.
    /// Returns actual BIOS content when PC is inside BIOS,
    /// or `bios_prefetch` (last prefetched BIOS instruction) when PC is outside.
    #[inline(always)]
    fn bios_load(&mut self, address: Addr) -> u32 {
        let in_bios = self.cpu.pc < BIOS_SIZE;
        let value = if in_bios {
            let v = self.bios_rom.read_32(address & !3);
            self.bus.last_data_bus = v;
            self.bus.bios_prefetch = v;
            v
        } else {
            self.bus.bios_prefetch
        };
        let shift = (address & 3) << 3;
        value >> shift
    }

    /// Do a read of unused memory (open bus).
    #[cold]
    #[inline(never)]
    fn unused_load(&mut self, addr: Addr) -> u32 {
        eprintln!("Bad load at PC={:08X}, ADDR={:08X}", self.cpu.pc, addr);

        // A read immediately following a DMA returns the DMA latch
        // (NanoBoyAdvance `Bus::ReadOpenBus`).
        if self.bus.last_access_dma {
            return self.dma.open_bus >> ((addr & 3) << 3);
        }

        let raw = match self.cpu.cpsr.execution_state {
            CpuExecutionState::Arm => {
                // ARM returns last fetched opcode.
                self.cpu_pipeline_get_prefetch()
            }
            CpuExecutionState::Thumb => {
                let pipe0 = self.cpu_pipeline_get_decode() & 0xFFFF;
                let pipe1 = self.cpu_pipeline_get_prefetch() & 0xFFFF;
                // Depends on the PC.
                let (lo, hi) = match region_from_address(self.cpu.pc) {
                    REGION_BIOS | REGION_OAM => {
                        if self.cpu.pc & 3 == 0 {
                            // 4-byte aligned:
                            // XXX: GBATEK says LSW = [$+4], MSW = [$+6], but we don't have [$+6].
                            (pipe1, pipe1)
                        } else {
                            (pipe0, pipe1)
                        }
                    }
                    REGION_IWRAM => {
                        if self.cpu.pc & 3 == 0 {
                            (pipe1, pipe0)
                        } else {
                            (pipe0, pipe1)
                        }
                    }
                    _ => {
                        // EWRAM, PALETTE, VRAM, CARTRIDGE ROM
                        (pipe1, pipe1)
                    }
                };
                (lo | (hi << 16))
            }
        };
        raw >> ((addr & 3) << 3)
    }
}

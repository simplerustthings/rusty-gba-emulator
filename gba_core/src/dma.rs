

use crate::{
    bus::MemoryAccessType,
    interrupt::InterruptKind,
    io::{REG_FIFO_A, REG_FIFO_B},
    Gba,
};
use crate::{impl_save_state_fields, impl_save_state_tuple};
use crate::BitIndex;

const NUM_CHANNELS: usize = 4;
const DMA_INTERRUPTS: [InterruptKind; 4] = [
    InterruptKind::Dma0,
    InterruptKind::Dma1,
    InterruptKind::Dma2,
    InterruptKind::Dma3,
];

/// State for the DMA controller.
pub struct Dma {
    channels: [DmaChannel; NUM_CHANNELS],
    /// Active channel bitfield.
    active: u8,
}

impl_save_state_fields!(Dma { channels, active });

/// A single DMA channel.
struct DmaChannel {
    /// Source address register.
    src: u32,
    /// Destination address register.
    dest: u32,
    /// Number of transfers (word count).
    count: u16,
    /// Control register.
    control: DmaChannelControl,

    /// Next access type.
    access_type: MemoryAccessType,
    /// Whether this is set up for FIFO.
    fifo: bool,

    /// Internal source address register.
    internal_src: u32,
    /// Internal destination address register.
    internal_dest: u32,
    /// Internal count register.
    internal_count: u32,

    /// Latch register (persists across transfers).
    /// Only updated when source >= EWRAM base.
    latch: u32,

    /// Whether the source was configured as SRAM/CART_UNUSED (DMA0 only, before 27-bit bus truncation).
    sram_src: bool,
}

impl_save_state_fields!(DmaChannel {
    src,
    dest,
    count,
    control,
    access_type,
    fifo,
    internal_src,
    internal_dest,
    internal_count,
    latch,
    sram_src,
});

impl Default for DmaChannel {
    fn default() -> Self {
        DmaChannel {
            src: 0,
            dest: 0,
            count: 0,
            control: DmaChannelControl(0),
            access_type: MemoryAccessType::NonSequential,
            fifo: false,
            internal_src: 0,
            internal_dest: 0,
            internal_count: 0,
            latch: 0,
            sram_src: false,
        }
    }
}

/// DMA control register.
#[derive(Copy, Clone)]
struct DmaChannelControl(u16);

impl_save_state_tuple!(DmaChannelControl);

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum AdjustmentMode {
    Increment,
    Decrement,
    Fixed,
    IncrementReload,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum TimingMode {
    Immediate,
    VBlank,
    HBlank,
    Special,
}

impl DmaChannelControl {
    fn dest_adjustment(self) -> AdjustmentMode {
        match self.0.bit_range(5..7) {
            0b00 => AdjustmentMode::Increment,
            0b01 => AdjustmentMode::Decrement,
            0b10 => AdjustmentMode::Fixed,
            0b11 => AdjustmentMode::IncrementReload,
            _ => unreachable!(),
        }
    }

    fn src_adjustment(self) -> AdjustmentMode {
        match self.0.bit_range(7..9) {
            0b00 => AdjustmentMode::Increment,
            0b01 => AdjustmentMode::Decrement,
            0b10 => AdjustmentMode::Fixed,
            0b11 => AdjustmentMode::IncrementReload,
            _ => unreachable!(),
        }
    }

    fn repeat(self) -> bool {
        self.0.bit(9)
    }

    fn word_size(self) -> usize {
        if self.0.bit(10) {
            4
        } else {
            2
        }
    }

    fn timing(self) -> TimingMode {
        match self.0.bit_range(12..14) {
            0b00 => TimingMode::Immediate,
            0b01 => TimingMode::VBlank,
            0b10 => TimingMode::HBlank,
            0b11 => TimingMode::Special,
            _ => unreachable!(),
        }
    }

    fn irq(self) -> bool {
        self.0.bit(14)
    }

    fn enabled(self) -> bool {
        self.0.bit(15)
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.0.set_bit(15, enabled);
    }
}

impl Dma {
    pub fn new() -> Dma {
        Dma {
            active: 0,
            channels: <[DmaChannel; 4]>::default(),
        }
    }

    /// Return the transfer size of the DMA channel, if it's active.
    pub fn transfer_size(&self, channel: usize) -> Option<u16> {
        if self.active.bit(channel) {
            Some(self.channels[channel].count)
        } else {
            None
        }
    }
}

impl Gba {
    /// Returns whether any DMA channel is *active*.
    /// This is different from *enabled*: active means it's taking control and transferring.
    #[inline(always)]
    pub(crate) fn dma_active(&self) -> bool {
        self.dma.active != 0
    }

    /// Performs the actual DMA transfer.
    #[inline(always)]
    pub(crate) fn dma_step(&mut self) {
        if self.dma.active.bit(0) { self.transfer_channel(0); }
        if self.dma.active.bit(1) { self.transfer_channel(1); }
        if self.dma.active.bit(2) { self.transfer_channel(2); }
        if self.dma.active.bit(3) { self.transfer_channel(3); }
    }

    /// Perform a DMA transfer for the given channel.
    fn transfer_channel(&mut self, index: usize) {
        let access;
        let src;
        let dest;
        let word_size;
        let src_adjust;
        let dest_adjust;

        // Read all we need from the channel upfront, then release the borrow.
        {
            let channel = &self.dma.channels[index];
            access = channel.access_type;
            src = channel.internal_src;
            dest = channel.internal_dest;

            if channel.fifo {
                word_size = 4;
                src_adjust = channel.control.src_adjustment();
                dest_adjust = AdjustmentMode::Fixed;
            } else {
                src_adjust = channel.control.src_adjustment();
                word_size = channel.control.word_size() as u32;
                dest_adjust = channel.control.dest_adjustment();
            }
        }

        const EWRAM_BASE: u32 = 0x0200_0000;

        let latch: u32;
        if src >= EWRAM_BASE {
            // DMA0 SRAM filter: return 0 when source was configured for SRAM/CART_UNUSED
            // (before 27-bit address bus truncation aliases it to VRAM/OAM).
            if index == 0 && self.dma.channels[index].sram_src {
                latch = 0;
            } else {
                self.bus.performing_dma = self.bus.performing_dma.wrapping_add(1);
                let load_addr = src & !(word_size - 1);
                if word_size == 2 {
                    let data = self.cpu_load16(load_addr, access);
                    latch = data as u32 | ((data as u32) << 16);
                } else {
                    latch = self.cpu_load32(load_addr, access);
                }
                self.bus.performing_dma = self.bus.performing_dma.wrapping_sub(1);
            }
        } else {
            // Source is below EWRAM (BIOS, unused, etc.): keep previous latch.
            latch = self.dma.channels[index].latch;
        }

        // Store from the latch value.
        if word_size == 2 {
            let store_val = (latch >> (8 * (dest & 2))) as u16;
            self.cpu_store16(dest & !0b1, store_val, access);
            self.bus.last_data_bus = (latch & 0xFFFF) | (latch << 16);
        } else {
            self.cpu_store32(dest & !0b11, latch, access);
            self.bus.last_data_bus = latch;
        }

        // Update channel state.
        let channel = &mut self.dma.channels[index];
        channel.latch = latch;
        channel.access_type = MemoryAccessType::Sequential;

        let src_mask = if index == 0 { 0x07FF_FFFF } else { 0x0FFF_FFFF };
        let dest_mask = if index == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };

        match src_adjust {
            AdjustmentMode::Fixed => {}
            AdjustmentMode::Decrement => channel.internal_src = (src.wrapping_sub(word_size)) & src_mask,
            AdjustmentMode::Increment | AdjustmentMode::IncrementReload => {
                channel.internal_src = (src.wrapping_add(word_size)) & src_mask
            }
        };
        match dest_adjust {
            AdjustmentMode::Fixed => {}
            AdjustmentMode::Decrement => channel.internal_dest = (dest.wrapping_sub(word_size)) & dest_mask,
            AdjustmentMode::Increment | AdjustmentMode::IncrementReload => {
                channel.internal_dest = (dest.wrapping_add(word_size)) & dest_mask
            }
        };

        channel.internal_count -= 1;
        if channel.internal_count == 0 {
            if channel.control.repeat() {
                if channel.fifo {
                    channel.internal_count = 4;
                } else {
                    if channel.control.dest_adjustment() == AdjustmentMode::IncrementReload {
                        channel.internal_dest = channel.dest;
                    }
                    channel.internal_count = if channel.count == 0 {
                        if index == 3 {
                            0x10000
                        } else {
                            0x4000
                        }
                    } else {
                        channel.count as u32
                    };
                }
            } else {
                channel.control.set_enabled(false);
            }

            if channel.control.irq() {
                self.interrupt_raise(DMA_INTERRUPTS[index]);
            }

            self.dma.active.set_bit(index, false);
        }
    }

    /// Activate a DMA channel (in response to an event).
    #[inline(always)]
    pub(crate) fn dma_activate_channel(&mut self, channel: usize) {
        self.dma.active.set_bit(channel, true);
        self.dma.channels[channel].access_type = MemoryAccessType::NonSequential;
    }

    /// Handle a 16-bit write to a DMA register.
    ///
    /// `reg` is relative to the start of the DMA register region, 0x0400_00B0.
    pub(crate) fn dma_reg_write(&mut self, reg: u32, value: u16) {
        let channel_index = (reg / 12) as usize;
        let reg = reg % 12;
        let mut should_activate = false;
        {
            let c = &mut self.dma.channels[channel_index];
            match reg {
                // Source Address
                0x0 => {
                    if channel_index == 0 {
                        c.internal_src = c.src;
                    }
                    c.src = (c.src & 0xFFFF_0000) | (value as u32)
                }
                0x2 => {
                    let mask = if channel_index == 0 { 0x07FF } else { 0x0FFF };
                    if channel_index == 0 {
                        let region = ((value as u32) >> 8) & 0xF;
                        c.sram_src = region == 0xE || region == 0xF;
                    }
                    c.src = (c.src & 0x0000_FFFF) | (((value as u32) & mask) << 16);
                }
                
                // Destination Address
                0x4 => {
                    if channel_index == 0 {
                        c.internal_dest = c.dest;
                    }
                    c.dest = (c.dest & 0xFFFF_0000) | (value as u32)
                }
                0x6 => {
                    let mask = if channel_index == 3 { 0x0FFF } else { 0x07FF };
                    c.dest = (c.dest & 0x0000_FFFF) | (((value as u32) & mask) << 16);
                }
                
                // Transfer count. (Channels 0/1/2 are 14-bit, Channel 3 is 16-bit)
                0x8 => {
                    let mask = if channel_index == 3 { 0xFFFF } else { 0x3FFF };
                    c.count = value & mask;
                }
                
                // Control register.
                0xA => {
                    let mask = if channel_index == 3 { 0xFFE0 } else { 0xF7E0 };
                    let value = value & mask;
                    let control = DmaChannelControl(value);
                    let enabled = !c.control.enabled() && control.enabled();

                    if enabled {
                        // Just enabled this channel. Copy registers to internal.
                        // DO NOT pre-align the registers. Hardware preserves the unaligned 
                        // bits in the latch and aligns on the fly during the bus transfer.
                        c.internal_src = c.src;
                        c.internal_dest = c.dest;

                        c.internal_count = if c.count == 0 {
                            if channel_index == 3 {
                                0x10000
                            } else {
                                0x4000
                            }
                        } else {
                            c.count as u32
                        };

                        // Handle special audio FIFO mode.
                        c.fifo = (control.timing() == TimingMode::Special)
                            && control.repeat()
                            && (channel_index == 1 || channel_index == 2)
                            && (c.dest == REG_FIFO_A || c.dest == REG_FIFO_B);
                        if c.fifo {
                            c.internal_count = 4;
                        }

                        if control.timing() == TimingMode::Immediate {
                            should_activate = true;
                        }
                    }

                    c.control = control;
                }
                _ => unreachable!(),
            }
        }
        if should_activate {
            self.dma_activate_channel(channel_index);
            if self.dma.channels[channel_index].control.timing() == TimingMode::Immediate {
                self.transfer_channel(channel_index);
            }
        }
    }

    /// Handle a 16-bit read from a DMA register.
    ///
    /// `reg` is relative to the start of the DMA register region, 0x0400_00B0.
    pub(crate) fn dma_reg_read(&mut self, reg: u32) -> u16 {
        let channel_index = (reg / 12) as usize;
        let reg = reg % 12;
        match reg {
            // DMA Control (CNT_HI) - readable
            0xA => self.dma.channels[channel_index].control.0,
            // DMA SAD (0-3) and DAD (4-7) - write-only, return open bus
            0..=7 => self.bus.last_data_bus as u16,
            // DMA CNT_LO (8-9) - write-only, returns 0
            _ => 0,
        }
    }

    /// Called by the PPU on vblank.
    pub(crate) fn dma_notify_vblank(&mut self) {
        if self.dma.channels[0].control.enabled() && self.dma.channels[0].control.timing() == TimingMode::VBlank { self.dma_activate_channel(0); }
        if self.dma.channels[1].control.enabled() && self.dma.channels[1].control.timing() == TimingMode::VBlank { self.dma_activate_channel(1); }
        if self.dma.channels[2].control.enabled() && self.dma.channels[2].control.timing() == TimingMode::VBlank { self.dma_activate_channel(2); }
        if self.dma.channels[3].control.enabled() && self.dma.channels[3].control.timing() == TimingMode::VBlank { self.dma_activate_channel(3); }
    }

    /// Called by the PPU on hblank (only during visible, non-vblank lines).
    pub(crate) fn dma_notify_hblank(&mut self) {
        if self.dma.channels[0].control.enabled() && self.dma.channels[0].control.timing() == TimingMode::HBlank { self.dma_activate_channel(0); }
        if self.dma.channels[1].control.enabled() && self.dma.channels[1].control.timing() == TimingMode::HBlank { self.dma_activate_channel(1); }
        if self.dma.channels[2].control.enabled() && self.dma.channels[2].control.timing() == TimingMode::HBlank { self.dma_activate_channel(2); }
        if self.dma.channels[3].control.enabled() && self.dma.channels[3].control.timing() == TimingMode::HBlank { self.dma_activate_channel(3); }
    }

    /// Called by the PPU on "video" updates (hblank during scanlines 2..162).
    pub(crate) fn dma_notify_video(&mut self) {
        // Only relevant for channel 3.
        let i = 3;
        let channel = &self.dma.channels[i];
        if channel.control.enabled() && channel.control.timing() == TimingMode::Special {
            self.dma_activate_channel(i);
        }
    }

    /// Called by the APU when it's requesting DMA to a FIFO.
    pub(crate) fn dma_notify_audio_fifo(&mut self, addr: u32) {
        if self.dma.channels[1].control.enabled()
            && self.dma.channels[1].control.timing() == TimingMode::Special
            && self.dma.channels[1].dest == addr
        {
            self.dma_activate_channel(1);
        }
        if self.dma.channels[2].control.enabled()
            && self.dma.channels[2].control.timing() == TimingMode::Special
            && self.dma.channels[2].dest == addr
        {
            self.dma_activate_channel(2);
        }
    }
}
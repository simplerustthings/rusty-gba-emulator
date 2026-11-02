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
    /// Most recently read value, exposed as open bus after a DMA access
    /// (NanoBoyAdvance's `DMA::latch` / `GetOpenBusValue`).
    pub(crate) open_bus: u32,
}

impl_save_state_fields!(Dma { channels, active, open_bus });

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
            open_bus: 0,
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
    #[inline(always)]
    pub(crate) fn dma_active(&self) -> bool {
        self.dma.active != 0
    }

    /// Performs the actual DMA transfer.
    ///
    /// The CPU is blocked for the whole burst, so every active channel is
    /// serviced to completion here. Per-unit timing follows mGBA's
    /// GBADMAService: the first unit pays the non-sequential wait states of
    /// source and destination, later units the sequential ones, plus two
    /// internal cycles per unit.
    /// Performs the actual DMA transfer.
    ///
    /// Follows NanoBoyAdvance's `DMA::Run`: one startup bus cycle, then every
    /// unit performs a real bus read and write (which charge the region's wait
    /// states), then one completion bus cycle. The CPU is blocked for the
    /// whole burst.
    #[inline(always)]
    pub(crate) fn dma_step(&mut self) {
        for index in 0..NUM_CHANNELS {
            if self.dma.active.bit(index) {
                self.run_dma_channel(index);
            }
        }
    }

    fn run_dma_channel(&mut self, index: usize) {
        self.bus_step(1);
        while self.dma.active.bit(index) {
            self.transfer_channel(index);
        }
        self.bus_step(1);
    }

    /// Perform a DMA transfer for the given channel.
    #[inline(always)]
    fn transfer_channel(&mut self, index: usize) {
        // DMA bus accesses are not affected by the CPU's prefetch buffer.
        self.bus.in_dma = true;
        let first_access;
        let src;
        let dest;
        let word_size;
        let src_adjust;
        let dest_adjust;

        {
            let channel = &self.dma.channels[index];
            first_access = channel.access_type == MemoryAccessType::NonSequential;
            src = channel.internal_src;
            dest = channel.internal_dest;

            if channel.fifo {
                word_size = 4;
                src_adjust = AdjustmentMode::Increment;
                dest_adjust = AdjustmentMode::Fixed;
            } else {
                // ROM sources always use incrementing addresses on the GBA,
                // regardless of the programmed source-address control.
                src_adjust = if index != 0
                    && src >= 0x0800_0000
                    && src < 0x0E00_0000
                {
                    AdjustmentMode::Increment
                } else {
                    channel.control.src_adjustment()
                };
                word_size = channel.control.word_size() as u32;
                dest_adjust = channel.control.dest_adjustment();
            }
        }

        // Only the first Game Pak access of a burst is non-sequential; the
        // other side (and later units) stay sequential (NBA `RunChannel`).
        let (src_access, dest_access) = if first_access {
            if src >= 0x0800_0000 {
                (MemoryAccessType::NonSequential, MemoryAccessType::Sequential)
            } else if dest >= 0x0800_0000 {
                (MemoryAccessType::Sequential, MemoryAccessType::NonSequential)
            } else {
                (MemoryAccessType::Sequential, MemoryAccessType::Sequential)
            }
        } else {
            (MemoryAccessType::Sequential, MemoryAccessType::Sequential)
        };

        const EWRAM_BASE: u32 = 0x0200_0000;

        let latch: u32;
        if src >= EWRAM_BASE {
            self.bus.performing_dma = self.bus.performing_dma.wrapping_add(1);
            let load_addr = src & !(word_size - 1);
            if word_size == 2 {
                let data = self.cpu_load16(load_addr, src_access);
                latch = data as u32 | ((data as u32) << 16);
            } else {
                latch = self.cpu_load32(load_addr, src_access);
            }
            self.bus.performing_dma = self.bus.performing_dma.wrapping_sub(1);
        } else {
            latch = self.dma.channels[index].latch;
        }

        if word_size == 2 {
            let store_val = (latch >> (8 * (dest & 2))) as u16;
            self.cpu_store16(dest & !0b1, store_val, dest_access);
            self.bus.last_data_bus = (latch & 0xFFFF) | (latch << 16);
        } else {
            self.cpu_store32(dest & !0b11, latch, dest_access);
            self.bus.last_data_bus = latch;
        }

        // NanoBoyAdvance exposes the most recently read value as the DMA
        // latch; open-bus reads immediately after a DMA observe it.
        self.dma.open_bus = latch;
        self.bus.last_access_dma = true;

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
        self.bus.in_dma = false;
    }

    /// Activate a DMA channel (in response to an event).
    #[inline(always)]
    pub(crate) fn dma_activate_channel(&mut self, channel: usize) {
        self.dma.active.set_bit(channel, true);
        self.dma.channels[channel].access_type = MemoryAccessType::NonSequential;
    }

    /// Handle a 16-bit write to a DMA register.
    #[inline(always)]
    pub(crate) fn dma_reg_write(&mut self, reg: u32, value: u16) {
        let channel_index = (reg / 12) as usize;
        let reg = reg % 12;
        let mut should_activate = false;
        {
            let c = &mut self.dma.channels[channel_index];
            match reg {
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
                0x8 => {
                    let mask = if channel_index == 3 { 0xFFFF } else { 0x3FFF };
                    c.count = value & mask;
                }
                0xA => {
                    let mask = if channel_index == 3 { 0xFFE0 } else { 0xF7E0 };
                    let value = value & mask;
                    let control = DmaChannelControl(value);
                    let enabled = !c.control.enabled() && control.enabled();

                    if enabled {
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
            // Immediate DMA starts two cycles after the trigger
            // (NanoBoyAdvance `ScheduleDMAs` adds 2); the CPU keeps running
            // until then.
            self.scheduler.push_event(
                crate::scheduler::Event::DmaActivate(channel_index as u8),
                2,
            );
        }
    }

    /// Handle a 16-bit read from a DMA register.
    #[inline(always)]
    pub(crate) fn dma_reg_read(&mut self, reg: u32) -> u16 {
        let channel_index = (reg / 12) as usize;
        let reg = reg % 12;
        match reg {
            0xA => self.dma.channels[channel_index].control.0,
            0..=7 => self.bus.last_data_bus as u16,
            _ => 0,
        }
    }

    /// Called by the PPU on vblank.
    #[inline(always)]
    pub(crate) fn dma_notify_vblank(&mut self) {
        if self.dma.channels[0].control.enabled() && self.dma.channels[0].control.timing() == TimingMode::VBlank { self.dma_activate_channel(0); }
        if self.dma.channels[1].control.enabled() && self.dma.channels[1].control.timing() == TimingMode::VBlank { self.dma_activate_channel(1); }
        if self.dma.channels[2].control.enabled() && self.dma.channels[2].control.timing() == TimingMode::VBlank { self.dma_activate_channel(2); }
        if self.dma.channels[3].control.enabled() && self.dma.channels[3].control.timing() == TimingMode::VBlank { self.dma_activate_channel(3); }
    }

    /// Called by the PPU on hblank (only during visible, non-vblank lines).
    #[inline(always)]
    pub(crate) fn dma_notify_hblank(&mut self) {
        if self.dma.channels[0].control.enabled() && self.dma.channels[0].control.timing() == TimingMode::HBlank { self.dma_activate_channel(0); }
        if self.dma.channels[1].control.enabled() && self.dma.channels[1].control.timing() == TimingMode::HBlank { self.dma_activate_channel(1); }
        if self.dma.channels[2].control.enabled() && self.dma.channels[2].control.timing() == TimingMode::HBlank { self.dma_activate_channel(2); }
        if self.dma.channels[3].control.enabled() && self.dma.channels[3].control.timing() == TimingMode::HBlank { self.dma_activate_channel(3); }
    }

    /// Called by the PPU on "video" updates (hblank during scanlines 2..162).
    #[inline(always)]
    pub(crate) fn dma_notify_video(&mut self) {
        let i = 3;
        let channel = &self.dma.channels[i];
        if channel.control.enabled() && channel.control.timing() == TimingMode::Special {
            self.dma_activate_channel(i);
        }
    }

    /// Called by the APU when it's requesting DMA to a FIFO.
    #[inline(always)]
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

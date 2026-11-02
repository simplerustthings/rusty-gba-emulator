use std::ops::DerefMut;

use crate::{
    cartridge::BackupType, interrupt::InterruptManager, io::CpuPowerState, save_state::SaveState,
    Apu, BackupFile, Bus, Cartridge, Cpu, Dma, Event, Io, KeypadState, Ppu, Rom, Scheduler,
    TimerManager,
};

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;

/// Game Boy Advance Emulator
pub struct Gba {
    /// The cartridge ROM.
    pub(crate) cart_rom: Rom,
    /// The 16 KiB BIOS ROM.
    pub(crate) bios_rom: Box<[u8]>,
    /// The cartridge backup file.
    pub(crate) cart_backup_file: Option<Box<dyn BackupFile>>,

    /// CPU state.
    pub(crate) cpu: Cpu,

    /// Memory bus state.
    pub(crate) bus: Bus,

    /// Scheduler state: controls when events fire.
    pub(crate) scheduler: Scheduler,

    /// Memory mapped I/O state.
    pub(crate) io: Io,

    /// PPU state.
    pub(crate) ppu: Ppu,

    /// APU (sound) state.
    pub(crate) apu: Apu,

    /// Interrupt manager state.
    pub(crate) interrupt: InterruptManager,

    /// DMA controller state.
    pub(crate) dma: Dma,

    /// Timer state.
    pub(crate) timer: TimerManager,

    /// The cartridge.
    pub(crate) cartridge: Cartridge,

    /// On-board ("external") work RAM.
    pub(crate) ewram: Box<[u8]>,

    /// On-chip ("internal") work RAM.
    pub(crate) iwram: Box<[u8]>,

    /// How much we overshot the last emulate_cycles request by.
    last_emulation_overshoot: usize,

    /// Current keypad state.
    pub(crate) keypad_state: KeypadState,

    /// Whether we should generate audio and video.
    ///
    /// This may be set to false during fast forwarding.
    pub(crate) should_render: bool,

    /// mGBA-compatible debug console state.
    ///
    /// This is used by the test suite ROMs (and is useful for homebrew
    /// diagnostics) to report test names and results.
    pub(crate) debug_enabled: bool,
    pub(crate) debug_flags: u16,
    pub(crate) debug_string: [u8; 0x100],
    pub(crate) debug_messages: Vec<String>,
}

/// Builder struct for [`Gba`].
pub struct GbaBuilder {
    bios_rom: Box<[u8]>,
    cart_rom: Rom,

    /// Whether we should skip the BIOS boot animation.
    skip_bios: bool,

    /// The backing storage for the cartridge backup.
    backup_file: Option<Box<dyn BackupFile>>,

    /// Backup type (or None for autodetection).
    backup_type: Option<BackupType>,
}

impl Gba {
    /// Create a new GBA emulator builder.
    pub fn builder(bios_rom: Box<[u8]>, cart_rom: Rom) -> GbaBuilder {
        GbaBuilder {
            bios_rom,
            cart_rom,
            skip_bios: false,
            backup_file: None,
            backup_type: None,
        }
    }

    /// Create a new GBA emulator from the builder.
    fn build(builder: GbaBuilder) -> Gba {
        let cartridge = Cartridge::new(&builder.cart_rom, builder.backup_type);
        let mut gba = Gba {
            cart_rom: builder.cart_rom,
            bios_rom: builder.bios_rom,
            cart_backup_file: builder.backup_file,

            cpu: Cpu::new(),
            bus: Bus::new(),
            scheduler: Scheduler::new(),
            io: Io::new(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            interrupt: InterruptManager::new(),
            dma: Dma::new(),
            timer: TimerManager::new(),
            cartridge,
            ewram: Box::new([0; 256 * 1024]),
            iwram: Box::new([0; 32 * 1024]),
            last_emulation_overshoot: 0,
            keypad_state: KeypadState::default(),
            should_render: false,
            debug_enabled: false,
            debug_flags: 0,
            debug_string: [0; 0x100],
            debug_messages: Vec::new(),
        };
        gba.ppu_init();
        gba.apu_init();

        // Load the backup file.
        if let Some(backup_file) = gba.cart_backup_file.as_mut() {
            gba.cartridge.backup_buffer.load(backup_file.deref_mut());
        }

        if builder.skip_bios {
            gba.cpu.skip_bios();
            gba.ppu.skip_bios();
        }

        gba
    }

    /// Advance the clock by `cycles`, dispatching any scheduled events at
    /// their exact timestamp. This is the equivalent of NanoBoyAdvance's
    /// `Scheduler::AddCycles`/`Step`: a multi-cycle bus or internal-cycle step
    /// observes timer/DMA/PPU events at the correct point in time.
    #[inline(always)]
    pub(crate) fn advance(&mut self, cycles: usize) {
        let target = self.scheduler.timestamp().saturating_add(cycles);
        loop {
            let Some(deadline) = self.scheduler.next_deadline() else {
                break;
            };
            if deadline > target {
                break;
            }
            // The run-loop terminator must stay in the queue for `run()`.
            if self.scheduler.peek_event() == Some(Event::StopRunning) {
                break;
            }
            self.scheduler.set_time(deadline);
            while let Some((event, lateness)) = self.scheduler.pop_event() {
                if event == Event::StopRunning {
                    break;
                }
                self.dispatch_event(event, lateness);
            }
        }
        if self.scheduler.timestamp() < target {
            self.scheduler.set_time(target);
        }
    }

    #[inline(always)]
    fn dispatch_event(&mut self, event: Event, lateness: usize) {
        match event {
            Event::StopRunning => {}
            Event::Ppu(ppu) => self.ppu_on_event(ppu, lateness),
            // TODO maybe handle lateness?
            Event::DmaActivate(channel) => self.dma_activate_channel(channel as usize),
            Event::TimerOverflow(channel) => self.timer_on_overflow(channel as usize),
            Event::TimerWriteReload(channel) => self.timer_on_reload_written(channel as usize),
            Event::TimerWriteControl(channel) => self.timer_on_control_written(channel as usize),
            Event::AudioSample => self.apu_on_sample_event(lateness),
            Event::AudioSequencerTick => self.apu_on_sequencer_event(lateness),
            Event::SioTransfer => self.sio_complete_transfer(),
            Event::IrqLine => self.interrupt_on_line_event(),
        }
    }

    /// Run the emulator for at least the given number of cycles.
    /// Returns the number of cycles actually ran for.
    #[inline(always)]
    fn run(&mut self, cycles: usize) -> usize {
        let start_time = self.scheduler.timestamp();
        self.scheduler.push_event(Event::StopRunning, cycles);

        'outer: loop {
            while self.scheduler.timestamp() < self.scheduler.deadline() {
                let cpu_active = self.io.power_state == CpuPowerState::Normal;
                let dma_active = self.dma_active();

                match (cpu_active, dma_active) {
                    (_, true) => {
                        self.dma_step();
                    }
                    (true, false) => {
                        if self.interrupt_pending() {
                            self.cpu_irq();
                        }

                        self.cpu_step();
                    }
                    (false, false) => {
                        if self.interrupt_pending() {
                            self.io.power_state = CpuPowerState::Normal;
                        } else {
                            self.scheduler.skip_to_next_event();
                            break;
                        }
                    }
                }
            }

            // Handle any events.
            while let Some((event, lateness)) = self.scheduler.pop_event() {
                if event == Event::StopRunning {
                    break 'outer;
                }
                self.dispatch_event(event, lateness);
            }
        }

        let end_time = self.scheduler.timestamp();
        end_time - start_time
    }

    /// Emulate a frame.
    #[inline(always)]
    pub fn emulate_frame(&mut self, should_render: bool) {
        self.should_render = should_render;
        const FRAME_CYCLES: usize = (240 + 68) * (160 + 68) * 4;
        self.emulate_cycles(FRAME_CYCLES);
        self.should_render = true;
    }

    /// Emulate for the given number of cycles.
    ///
    /// Starts by clearing the audio sample buffer.
    #[inline(always)]
    pub fn emulate_cycles(&mut self, cycles: usize) {
        self.apu_buffer_clear();

        if cycles <= self.last_emulation_overshoot {
            self.last_emulation_overshoot -= cycles;
        } else {
            let run_cycles = cycles - self.last_emulation_overshoot;
            let actually_ran = self.run(run_cycles);
            self.last_emulation_overshoot = actually_ran - run_cycles;
        }

        // Persist the backup buffer (if it's dirty).
        if let Some(backup_file) = self.cart_backup_file.as_mut() {
            self.cartridge.backup_buffer.save(backup_file.deref_mut());
        }
    }

    /// Get the frame buffer.
    /// (240 * 160) pixels, each pixel in ARGB format, row major.
    pub fn framebuffer(&self) -> &[u32] {
        &self.ppu.framebuffer
    }

    /// Temporary debugging helper: raw EWRAM contents.
    pub fn debug_ewram(&self) -> &[u8] {
        &self.ewram
    }

    /// Temporary debugging helper: raw VRAM contents.
    pub fn debug_vram(&self) -> &[u8] {
        &self.ppu.vram
    }

    /// Temporary debugging helper: raw palette contents.
    pub fn debug_palette(&self) -> &[u8] {
        &self.ppu.palette
    }

    /// Get the audio samples created during the last frame.
    /// This is a sequence of samples, interleaving the left and right channels.
    pub fn audio_buffer(&self) -> &[i16] {
        self.apu_buffer()
    }

    /// Messages written by the guest through the mGBA debug console.
    ///
    /// The test suite ROM uses this to report suite/test names and results.
    pub fn debug_messages(&self) -> &[String] {
        &self.debug_messages
    }

    /// Clear buffered mGBA debug console messages.
    pub fn clear_debug_messages(&mut self) {
        self.debug_messages.clear();
    }

    /// Run a tiny amount of emulation for debugging.
    ///
    /// This is intentionally not a stable API.
    #[doc(hidden)]
    pub fn debug_step(&mut self) {
        self.run(1);
    }

    /// Human-readable snapshot of the CPU and scheduler state.
    ///
    /// Intended for debugging and test diagnostics; not part of the stable
    /// emulation API.
    pub fn debug_status(&self) -> String {
        let mut regs = String::new();
        for (i, reg) in self.cpu.gpr.iter().enumerate() {
            regs.push_str(&format!("r{i}={reg:08X} "));
        }
        format!(
            "pc={:08X} cpsr={:08X} {regs}mode={:?} power={:?} vcount={} dispcnt={:04X} dma_active={} timestamp={}",
            self.cpu.pc,
            u32::from(self.cpu.cpsr),
            self.cpu.cpsr.mode,
            self.io.power_state,
            self.ppu.vcount,
            self.ppu.dispcnt.read(),
            self.dma_active(),
            self.scheduler.timestamp(),
        )
    }

    /// Serialize the current state of the emulator.
    ///
    /// This does not modify the cartridge or BIOS ROMs.
    pub fn save_state(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.cpu.save(&mut buf);
        self.bus.save(&mut buf);
        self.scheduler.save(&mut buf);
        self.io.save(&mut buf);
        self.ppu.save(&mut buf);
        self.apu.save(&mut buf);
        self.interrupt.save(&mut buf);
        self.dma.save(&mut buf);
        self.timer.save(&mut buf);
        self.cartridge.save(&mut buf);
        self.ewram.save(&mut buf);
        self.iwram.save(&mut buf);
        self.last_emulation_overshoot.save(&mut buf);
        self.keypad_state.save(&mut buf);
        self.should_render.save(&mut buf);
        buf
    }

    pub fn load_state(&mut self, data: &[u8]) {
        let mut buf = data;
        self.cpu = Cpu::load(&mut buf);
        self.bus = Bus::load(&mut buf);
        self.scheduler = Scheduler::load(&mut buf);
        self.io = Io::load(&mut buf);
        self.ppu = Ppu::load(&mut buf);
        self.apu = Apu::load(&mut buf);
        self.interrupt = InterruptManager::load(&mut buf);
        self.dma = Dma::load(&mut buf);
        self.timer = TimerManager::load(&mut buf);
        self.cartridge = Cartridge::load(&mut buf);
        self.ewram = Box::<[u8]>::load(&mut buf);
        self.iwram = Box::<[u8]>::load(&mut buf);
        self.last_emulation_overshoot = usize::load(&mut buf);
        self.keypad_state = KeypadState::load(&mut buf);
        self.should_render = bool::load(&mut buf);
    }
}

impl GbaBuilder {
    /// Set whether the BIOS boot animation should be skipped.
    pub fn skip_bios(mut self, should_skip: bool) -> Self {
        self.skip_bios = should_skip;
        self
    }

    /// Set the backup file.
    pub fn backup_file(mut self, backup_file: Box<dyn BackupFile>) -> Self {
        self.backup_file = Some(backup_file);
        self
    }

    /// Set the backup type. Overrides autodetection.
    pub fn backup_type(mut self, backup_type: BackupType) -> Self {
        self.backup_type = Some(backup_type);
        self
    }

    /// Build the GBA emulator with the current configuration.
    pub fn build(self) -> Gba {
        Gba::build(self)
    }
}

use crate::Gba;
use crate::BitIndex;
use crate::impl_save_state_fields;

/// State for the interrupt manager.
pub struct InterruptManager {
    /// Whether interrupts are enabled.
    pub global_enabled: bool,

    /// Individual interrupts that are enabled.
    pub enabled: u16,

    /// Individual interrupts that are pending (the committed IF register).
    pub pending: u16,

    /// Whether the CPU IRQ line is currently asserted.
    pub line_high: bool,

    /// Whether an IRQ-line visibility event is already scheduled.
    pub line_event_scheduled: bool,
}

impl_save_state_fields!(InterruptManager {
    global_enabled,
    enabled,
    pending,
    line_high,
    line_event_scheduled,
});

#[derive(Copy, Clone)]
#[allow(unused)]
pub enum InterruptKind {
    VBlank = 0,
    HBlank = 1,
    VCount = 2,
    Timer0 = 3,
    Timer1 = 4,
    Timer2 = 5,
    Timer3 = 6,
    Serial = 7,
    Dma0 = 8,
    Dma1 = 9,
    Dma2 = 10,
    Dma3 = 11,
    Keypad = 12,
    Gamepak = 13,
}

impl InterruptManager {
    pub fn new() -> InterruptManager {
        InterruptManager {
            global_enabled: false,
            enabled: 0,
            pending: 0,
            line_high: false,
            line_event_scheduled: false,
        }
    }
}

impl Gba {
    /// Handle a write to the "Interrupt Request / Acknowledge" register.
    /// Acknowledges some IRQs.
    #[inline(always)]
    pub(crate) fn interrupt_reg_if_write(&mut self, acked: u16) {
        // For each 1 in "acked", set corresponding pending to false.
        // This is the "not implies" boolean operation, (a & ~b)
        self.interrupt.pending &= !acked;
        self.interrupt_update_line();
    }

    /// Schedule the deferred update of the CPU IRQ line.
    #[inline(always)]
    fn interrupt_schedule_line(&mut self) {
        if !self.interrupt.line_event_scheduled {
            self.interrupt.line_event_scheduled = true;
            // NanoBoyAdvance's chain: IF commit (+1) then IRQ line (+2).
            self.scheduler
                .push_event(crate::scheduler::Event::IrqLine, 3);
        }
    }

    /// Recompute the CPU IRQ line after a change to IE/IME/IF.
    #[inline(always)]
    pub(crate) fn interrupt_update_line(&mut self) {
        let active =
            self.interrupt.global_enabled && (self.interrupt.pending & self.interrupt.enabled) != 0;
        if active && !self.interrupt.line_high {
            self.interrupt_schedule_line();
        } else if !active {
            self.interrupt.line_high = false;
        }
    }

    /// Scheduler event: commit the IRQ line state.
    pub(crate) fn interrupt_on_line_event(&mut self) {
        self.interrupt.line_event_scheduled = false;
        let active =
            self.interrupt.global_enabled && (self.interrupt.pending & self.interrupt.enabled) != 0;
        self.interrupt.line_high = active;
    }

    /// Checks whether an interrupt is pending (IRQ).
    #[inline(always)]
    pub(crate) fn interrupt_pending(&self) -> bool {
        self.interrupt.line_high
            && self.interrupt.global_enabled
            && ((self.interrupt.pending & self.interrupt.enabled) != 0)
    }

    /// Raise an interrupt.
    #[inline(always)]
    pub(crate) fn interrupt_raise(&mut self, kind: InterruptKind) {
        self.interrupt.pending.set_bit(kind as usize, true);
        if !self.interrupt.line_high {
            self.interrupt_schedule_line();
        }
    }

    /// Raise a timer interrupt (same visibility chain as any other source).
    #[inline(always)]
    pub(crate) fn interrupt_raise_timer(&mut self, kind: InterruptKind) {
        self.interrupt_raise(kind);
    }
}

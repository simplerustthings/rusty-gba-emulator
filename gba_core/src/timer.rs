use crate::{impl_save_state_fields, impl_save_state_tuple};
use crate::BitIndex;

use crate::{interrupt::InterruptKind, scheduler::Event, Gba};

const NUM_TIMERS: usize = 4;
const OVERFLOW: u32 = 0x1_0000;
const INTERRUPTS: [InterruptKind; 4] = [
    InterruptKind::Timer0,
    InterruptKind::Timer1,
    InterruptKind::Timer2,
    InterruptKind::Timer3,
];
const SHIFTS: [usize; 4] = [0, 6, 8, 10];
const MASKS: [usize; 4] = [0, 0x3F, 0xFF, 0x3FF];

/// State for the timers in the system.
///
/// This follows the event-driven model used by NanoBoyAdvance: each running
/// timer owns an overflow event scheduled at the exact cycle at which its
/// counter wraps. There is no lazy "integrate since last update" pass; reads
/// derive the current counter from the timestamp at which the channel was
/// started, and register writes are committed one cycle after the bus write.
pub struct TimerManager {
    timers: [Timer; NUM_TIMERS],
}

impl_save_state_fields!(TimerManager { timers });

/// A single timer.
#[derive(Default, Debug)]
struct Timer {
    /// Committed reload value.
    reload: u16,
    /// Reload value written this cycle, committed by `TimerWriteReload`.
    pending_reload: u16,
    /// Current counter (0..=0x10000; equal to OVERFLOW transiently).
    counter: u32,
    /// Committed control register.
    control: TimerControl,
    /// Control value written this cycle, committed by `TimerWriteControl`.
    pending_control: u16,
    /// Whether an overflow event is currently scheduled.
    running: bool,
    /// Timestamp (signed) at which `counter` was valid.
    timestamp_started: i64,
}

impl_save_state_fields!(Timer {
    reload,
    pending_reload,
    counter,
    control,
    pending_control,
    running,
    timestamp_started
});

/// Timer control register.
#[derive(Copy, Clone, Default)]
struct TimerControl(u16);

impl_save_state_tuple!(TimerControl);

impl TimerControl {
    /// Returns the prescaler period in cycles.
    fn period(self) -> usize {
        [1, 64, 256, 1024][self.0.bit_range(0..2) as usize]
    }

    fn shift(self) -> usize {
        SHIFTS[self.0.bit_range(0..2) as usize]
    }

    fn mask(self) -> usize {
        MASKS[self.0.bit_range(0..2) as usize]
    }

    /// Returns if this timer is in cascade mode.
    fn cascade(self) -> bool {
        self.0.bit(2)
    }

    /// Returns if this timer will raise an interrupt on overflow.
    fn irq(self) -> bool {
        self.0.bit(6)
    }

    /// Returns if this timer is enabled.
    fn enabled(self) -> bool {
        self.0.bit(7)
    }
}

impl std::fmt::Debug for TimerControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerControl")
            .field("period", &self.period())
            .field("cascade", &self.cascade())
            .field("irq", &self.irq())
            .field("enabled", &self.enabled())
            .finish()
    }
}

impl TimerManager {
    pub fn new() -> TimerManager {
        TimerManager {
            timers: <[Timer; NUM_TIMERS]>::default(),
        }
    }
}

impl Gba {
    /// Signed number of ticks elapsed since the counter snapshot was taken.
    #[inline(always)]
    fn timer_elapsed(&self, index: usize) -> u32 {
        let timer = &self.timer.timers[index];
        let now = self.scheduler.timestamp() as i64;
        let elapsed = now.saturating_sub(timer.timestamp_started).max(0) as u64;
        (elapsed >> timer.control.shift()) as u32
    }

    /// Start (or restart) a timer's overflow event.
    ///
    /// `cycle_offset` is the number of cycles already elapsed in the current
    /// prescaler period (signed: NanoBoyAdvance uses -1 on the enable edge to
    /// model the reload taking one cycle).
    fn timer_start(&mut self, index: usize, cycle_offset: i64) {
        let shift = self.timer.timers[index].control.shift();
        let counter = self.timer.timers[index].counter as i64;
        let cycles = (((OVERFLOW as i64 - counter) << shift) - cycle_offset).max(0) as usize;
        let now = self.scheduler.timestamp() as i64;

        self.timer.timers[index].running = true;
        self.timer.timers[index].timestamp_started = now - cycle_offset;
        self.scheduler.cancel_event(Event::TimerOverflow(index as u8));
        self.scheduler
            .push_event(Event::TimerOverflow(index as u8), cycles);
    }

    /// Stop a running timer, integrating any elapsed ticks into its counter.
    fn timer_stop(&mut self, index: usize) {
        if !self.timer.timers[index].running {
            return;
        }
        let elapsed = self.timer_elapsed(index);
        let timer = &mut self.timer.timers[index];
        timer.counter = timer.counter.wrapping_add(elapsed);
        timer.timestamp_started = self.scheduler.timestamp() as i64;
        timer.running = false;
        self.scheduler.cancel_event(Event::TimerOverflow(index as u8));

        if timer.counter >= OVERFLOW {
            self.timer_reload_cascade_irq(index);
        }
    }

    /// Reload a timer, raise its IRQ and cascade into the next timer.
    ///
    /// Cascading is recursive so that a chain of overflows on the same
    /// timestamp is processed in hardware order.
    fn timer_reload_cascade_irq(&mut self, index: usize) {
        self.timer.timers[index].counter = self.timer.timers[index].reload as u32;

        if self.timer.timers[index].control.irq() {
            self.interrupt_raise_timer(INTERRUPTS[index]);
        }
        if index <= 1 {
            self.apu_on_timer_overflow(index);
        }

        if index != 3 {
            let next = &mut self.timer.timers[index + 1];
            if next.control.enabled() && next.control.cascade() {
                next.counter = next.counter.wrapping_add(1);
                if next.counter == OVERFLOW {
                    self.timer_reload_cascade_irq(index + 1);
                }
            }
        }
    }

    /// Handle a scheduler overflow event at its exact timestamp.
    pub(crate) fn timer_on_overflow(&mut self, index: usize) {
        self.timer_reload_cascade_irq(index);
        self.timer_start(index, 0);
    }

    /// Commit a deferred TMxCNT_L write.
    pub(crate) fn timer_on_reload_written(&mut self, index: usize) {
        self.timer.timers[index].reload = self.timer.timers[index].pending_reload;
    }

    /// Commit a deferred TMxCNT_H write.
    pub(crate) fn timer_on_control_written(&mut self, index: usize) {
        let value = self.timer.timers[index].pending_control;
        let enable_previous = self.timer.timers[index].control.enabled();

        if self.timer.timers[index].running {
            self.timer_stop(index);
        }

        let new = TimerControl(value);
        self.timer.timers[index].control = new;

        if !new.enabled() {
            return;
        }

        // How many cycles are we into the current prescaler period?
        let prescaler_offset = (self.scheduler.timestamp() & new.mask()) as i64;

        if enable_previous {
            // Reconfiguration of an already-running timer: restart at the
            // current phase unless it is a cascade timer.
            if !new.cascade() {
                self.timer_start(index, prescaler_offset);
            }
        } else if new.cascade() {
            self.timer.timers[index].counter = self.timer.timers[index].reload as u32;
        } else if self.timer.timers[index].counter == 0xFFFF && prescaler_offset == 0 {
            // Enabling a timer whose reload is 0xFFFF: the reload takes one
            // cycle, during which the timer can already overflow.
            self.timer_start(index, 0);
        } else {
            self.timer.timers[index].counter = self.timer.timers[index].reload as u32;
            self.timer_start(index, prescaler_offset - 1);
        }
    }

    pub(crate) fn timer_write_counter(&mut self, index: usize, value: u16) {
        self.timer.timers[index].pending_reload = value;
        self.scheduler
            .push_event(Event::TimerWriteReload(index as u8), 1);
    }

    pub(crate) fn timer_write_control(&mut self, index: usize, value: u16) {
        self.timer.timers[index].pending_control = value;
        self.scheduler
            .push_event(Event::TimerWriteControl(index as u8), 1);
    }

    pub(crate) fn timer_read_counter(&mut self, index: usize) -> u16 {
        let mut counter = self.timer.timers[index].counter;
        if self.timer.timers[index].running {
            counter = counter.wrapping_add(self.timer_elapsed(index));
        }
        counter as u16
    }

    pub(crate) fn timer_read_control(&mut self, index: usize) -> u16 {
        self.timer.timers[index].control.0
    }
}

#[cfg(test)]
mod tests {
    use crate::io::{REG_TM0CNT_H, REG_TM0CNT_L, REG_TM1CNT_H, REG_TM1CNT_L};
    use crate::tests::TestGba;

    #[test]
    fn timer_prescaler_and_reload() {
        let mut test = TestGba::new();
        test.gba.io_write_16(REG_TM0CNT_L, 0xFFFE);
        test.gba.io_write_16(REG_TM0CNT_H, 0x0080);

        // Register writes commit one cycle after the bus write.
        test.gba.advance(1);

        test.gba.advance(2);
        assert_eq!(test.gba.timer_read_counter(0), 0xFFFF);

        test.gba.advance(1);
        // The overflow event fired at its exact timestamp and reloaded.
        assert_eq!(test.gba.timer_read_counter(0), 0xFFFE);
    }

    #[test]
    fn timer_cascade_counts_lower_overflows() {
        let mut test = TestGba::new();
        test.gba.io_write_16(REG_TM0CNT_L, 0xFFFE);
        test.gba.io_write_16(REG_TM0CNT_H, 0x0080);
        test.gba.io_write_16(REG_TM1CNT_L, 0x0000);
        test.gba.io_write_16(REG_TM1CNT_H, 0x0084); // enable + cascade

        test.gba.advance(1);
        test.gba.advance(5);
        assert_eq!(test.gba.timer_read_counter(1), 2);
    }

    #[test]
    fn timer_prescaler_period() {
        let mut test = TestGba::new();
        test.gba.io_write_16(REG_TM0CNT_L, 0xFFFE);
        test.gba.io_write_16(REG_TM0CNT_H, 0x0081); // enable, prescaler /64

        // Register writes commit one cycle after the bus write.
        test.gba.advance(1);

        // No tick has happened until the next 64-cycle boundary.
        test.gba.advance(63);
        assert_eq!(test.gba.timer_read_counter(0), 0xFFFE);

        test.gba.advance(64);
        assert_eq!(test.gba.timer_read_counter(0), 0xFFFF);
    }

    #[test]
    fn timer_overflow_raises_irq_pending_bit() {
        let mut test = TestGba::new();
        test.gba.io_write_16(REG_TM0CNT_L, 0xFFFF);
        test.gba.io_write_16(REG_TM0CNT_H, 0x00C0); // enable + irq

        test.gba.advance(4);
        assert_ne!(test.gba.interrupt.pending & (1 << 3), 0);
    }
}

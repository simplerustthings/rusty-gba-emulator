use crate::save_state::SaveState;
use crate::{impl_save_state_fields, impl_save_state_enum};

const MAX_EVENTS: usize = 32;

pub struct Scheduler {
    time: usize,
    events: [Event; MAX_EVENTS],
    deadlines: [usize; MAX_EVENTS],
    head: usize,
    count: usize,
}

impl_save_state_fields!(Scheduler {
    time,
    events,
    deadlines,
    head,
    count,
});

impl SaveState for Event {
    fn save(&self, buf: &mut Vec<u8>) {
        match self {
            Event::StopRunning => buf.push(0),
            Event::Ppu(e) => { buf.push(1); e.save(buf); }
            Event::DmaActivate(v) => { buf.push(2); v.save(buf); }
            Event::TimerOverflow(v) => { buf.push(3); v.save(buf); }
            Event::AudioSample => buf.push(4),
            Event::AudioSequencerTick => buf.push(5),
            Event::SioTransfer => buf.push(6),
            Event::TimerWriteReload(v) => { buf.push(7); v.save(buf); }
            Event::TimerWriteControl(v) => { buf.push(8); v.save(buf); }
            Event::IrqLine => buf.push(9),
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        let disc = buf[0];
        *buf = &buf[1..];
        match disc {
            0 => Event::StopRunning,
            1 => Event::Ppu(PpuEvent::load(buf)),
            2 => Event::DmaActivate(u8::load(buf)),
            3 => Event::TimerOverflow(u8::load(buf)),
            4 => Event::AudioSample,
            5 => Event::AudioSequencerTick,
            6 => Event::SioTransfer,
            7 => Event::TimerWriteReload(u8::load(buf)),
            8 => Event::TimerWriteControl(u8::load(buf)),
            9 => Event::IrqLine,
            _ => panic!("Unknown Event discriminant: {}", disc),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Event {
    StopRunning,
    Ppu(PpuEvent),
    DmaActivate(u8),
    /// A timer reached its counter overflow at this exact timestamp.
    TimerOverflow(u8),
    /// Deferred TMxCNT_L write commit (one cycle after the register write).
    TimerWriteReload(u8),
    /// Deferred TMxCNT_H write commit (one cycle after the register write).
    TimerWriteControl(u8),
    AudioSample,
    AudioSequencerTick,
    SioTransfer,
    /// CPU IRQ line becomes visible after the IF/IE/IME commit chain.
    IrqLine,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum PpuEvent {
    EndHDraw,
    EndHBlank,
    EndVBlankHDraw,
    EndVBlankHBlank,
    /// One cycle after VCOUNT increments: update the DISPSTAT match flag.
    UpdateVCountFlag,
    /// One cycle after the match flag rises: raise the VCOUNT IRQ.
    RaiseVCountIrq,
    /// 40 cycles into HDraw: shift the three-stage DISPCNT latch
    /// (NanoBoyAdvance `PPU_latch_dispcnt`).
    LatchDispcnt,
}

impl_save_state_enum!(PpuEvent {
    EndHDraw, EndHBlank, EndVBlankHDraw, EndVBlankHBlank, UpdateVCountFlag, RaiseVCountIrq,
    LatchDispcnt
});

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler {
            events: [Event::StopRunning; MAX_EVENTS],
            deadlines: [0; MAX_EVENTS],
            head: 0,
            count: 0,
            time: 0,
        }
    }

    #[inline(always)]
    pub fn timestamp(&self) -> usize {
        self.time
    }

    /// Earliest scheduled event deadline, if any.
    #[inline(always)]
    pub fn next_deadline(&self) -> Option<usize> {
        if self.count == 0 {
            None
        } else {
            Some(self.deadlines[self.head])
        }
    }

    /// The event at the head of the queue, if it is due now.
    #[inline(always)]
    pub fn peek_event(&self) -> Option<Event> {
        if self.count == 0 {
            None
        } else {
            Some(self.events[self.head])
        }
    }

    /// Move the clock to an absolute timestamp (used when dispatching events at
    /// their exact deadline).
    #[inline(always)]
    pub fn set_time(&mut self, time: usize) {
        self.time = time;
    }

    #[inline(always)]
    pub fn update(&mut self, delta: usize) {
        self.time += delta;
    }

    /// Adjust the timestamp by a signed number of cycles.
    ///
    /// The GBA prefetch buffer can hide data-access cycles, so an access may
    /// "consume" fewer cycles than the bus transfer nominally takes.
    #[inline(always)]
    pub fn update_signed(&mut self, delta: i64) {
        if delta >= 0 {
            self.time = self.time.saturating_add(delta as usize);
        } else {
            self.time = self.time.saturating_sub((-delta) as usize);
        }
    }

    /// Return the deadline of the next event (must have at least one).
    #[inline(always)]
    pub fn deadline(&self) -> usize {
        self.deadlines[self.head]
    }

    pub fn skip_to_next_event(&mut self) {
        if self.count > 0 {
            self.time = self.time.max(self.deadlines[self.head]);
        }
    }

    #[inline(always)]
    fn idx(&self, i: usize) -> usize {
        (self.head + i) & (MAX_EVENTS - 1)
    }

    pub fn pop_event(&mut self) -> Option<(Event, usize)> {
        if self.count == 0 {
            return None;
        }
        if self.deadlines[self.head] <= self.time {
            let event = self.events[self.head];
            let lateness = self.time - self.deadlines[self.head];
            self.head = (self.head + 1) & (MAX_EVENTS - 1);
            self.count -= 1;
            Some((event, lateness))
        } else {
            None
        }
    }

    pub fn cancel_event(&mut self, event: Event) {
        let mut i = 0;
        while i < self.count {
            if self.events[self.idx(i)] == event {
                for j in i..self.count - 1 {
                    let from = self.idx(j + 1);
                    let to = self.idx(j);
                    self.events[to] = self.events[from];
                    self.deadlines[to] = self.deadlines[from];
                }
                self.count -= 1;
            } else {
                i += 1;
            }
        }
    }

    pub fn push_event(&mut self, event: Event, when: usize) {
        if self.count >= MAX_EVENTS {
            if std::env::var_os("GBA_TRACE_SCHED").is_some() {
                eprintln!("scheduler full: dropping {:?} at +{}", event, when);
                for i in 0..self.count {
                    eprintln!(
                        "  {i}: {:?} deadline={} (in {})",
                        self.events[self.idx(i)],
                        self.deadlines[self.idx(i)],
                        self.deadlines[self.idx(i)].saturating_sub(self.time)
                    );
                }
            }
            return;
        }
        let deadline = self.time + when;
        let priority = event_priority(event);

        // Single-pass insert: stable by (deadline, priority), matching
        // NanoBoyAdvance's scheduler key. Lower priority runs first.
        let mut i = self.count;
        while i > 0 {
            let prev = self.idx(i - 1);
            if (self.deadlines[prev], event_priority(self.events[prev]))
                <= (deadline, priority)
            {
                break;
            }
            let cur = self.idx(i);
            self.events[cur] = self.events[prev];
            self.deadlines[cur] = self.deadlines[prev];
            i -= 1;
        }

        let idx = self.idx(i);
        self.events[idx] = event;
        self.deadlines[idx] = deadline;
        self.count += 1;
    }
}

/// Event priority within the same timestamp, following NanoBoyAdvance.
#[inline(always)]
fn event_priority(event: Event) -> u8 {
    match event {
        Event::TimerWriteReload(_) => 1,
        Event::TimerWriteControl(_) => 2,
        Event::Ppu(PpuEvent::RaiseVCountIrq) => 1,
        _ => 0,
    }
}

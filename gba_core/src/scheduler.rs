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
            Event::TimerUpdate => buf.push(3),
            Event::AudioSample => buf.push(4),
            Event::AudioSequencerTick => buf.push(5),
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        let disc = buf[0];
        *buf = &buf[1..];
        match disc {
            0 => Event::StopRunning,
            1 => Event::Ppu(PpuEvent::load(buf)),
            2 => Event::DmaActivate(u8::load(buf)),
            3 => Event::TimerUpdate,
            4 => Event::AudioSample,
            5 => Event::AudioSequencerTick,
            _ => panic!("Unknown Event discriminant: {}", disc),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Event {
    StopRunning,
    Ppu(PpuEvent),
    DmaActivate(u8),
    TimerUpdate,
    AudioSample,
    AudioSequencerTick,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum PpuEvent {
    EndHDraw,
    EndHBlank,
    EndVBlankHDraw,
    EndVBlankHBlank,
}

impl_save_state_enum!(PpuEvent {
    EndHDraw, EndHBlank, EndVBlankHDraw, EndVBlankHBlank
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

    #[inline(always)]
    pub fn update(&mut self, delta: usize) {
        self.time += delta;
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
            return;
        }
        let deadline = self.time + when;

        // Single-pass insert: find position while shifting elements back.
        let mut i = self.count;
        while i > 0 {
            let prev = self.idx(i - 1);
            if self.deadlines[prev] <= deadline {
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

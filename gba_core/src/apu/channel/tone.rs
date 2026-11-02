use crate::impl_save_state_fields;
use crate::BitIndex;

use crate::apu::channel::EnvelopeDirection;

use super::{Sequencer, SweepDirection};
pub struct ToneChannel {
    has_sweep: bool,
    pub sequencer: Sequencer,

    /// Wave duty type (0-3).
    duty: u8,

    /// Whether the channel is enabled (via the DAC).
    dac_enable: bool,

    /// Phase accumulator in time_8 units.
    phase_8: usize,
    /// Cached (2048 - freq) * 16 = period / 8, recomputed on freq change.
    period_div_8: usize,
    /// Cached waveform index (0-7), computed in advance().
    cached_index: usize,
}

impl_save_state_fields!(ToneChannel {
    has_sweep,
    sequencer,
    duty,
    dac_enable,
    phase_8,
    period_div_8,
    cached_index,
});

#[derive(Debug)]
pub enum ToneRegister {
    SweepL,
    DutyL,
    DutyH,
    FreqL,
    FreqH,
}

impl ToneChannel {
    pub fn new(has_sweep: bool) -> ToneChannel {
        ToneChannel {
            has_sweep,
            sequencer: Sequencer::new(64),
            duty: 0,
            dac_enable: false,
            phase_8: 0,
            period_div_8: 2048 * 16,
            cached_index: 0,
        }
    }

    #[inline(always)]
    pub fn enabled(&self) -> bool {
        self.sequencer.enabled && self.dac_enable
    }

    #[inline(always)]
    pub fn advance(&mut self) {
        self.period_div_8 = (2048 - self.sequencer.sweep_current_freq as usize) * 16;
        let wrap = self.period_div_8 * 8;
        self.phase_8 = (self.phase_8 + crate::apu::CYCLES_PER_SAMPLE) % wrap;
        self.cached_index = self.phase_8 / self.period_div_8;
    }

    #[inline(always)]
    pub fn sample(&self, _time: usize) -> i16 {
        if !self.enabled() {
            return 0;
        }
        const DUTY: [[i16; 8]; 4] = [
            [8, -8, -8, -8, -8, -8, -8, -8],
            [8, 8, -8, -8, -8, -8, -8, -8],
            [8, 8, 8, 8, -8, -8, -8, -8],
            [8, 8, 8, 8, 8, 8, -8, -8],
        ];
        DUTY[self.duty as usize][self.cached_index] * (self.sequencer.envelope_volume as i16)
    }

    pub fn read_register(&mut self, register: ToneRegister) -> u8 {
        match register {
            ToneRegister::SweepL if self.has_sweep => {
                self.sequencer.sweep_shift
                    | ((self.sequencer.sweep_direction as u8) << 3)
                    | (self.sequencer.sweep_time << 4)
            }
            ToneRegister::SweepL => 0,
            ToneRegister::DutyL => self.duty << 6,
            ToneRegister::DutyH => {
                self.sequencer.envelope_time
                    | ((self.sequencer.envelope_direction as u8) << 3)
                    | (self.sequencer.envelope_initial << 4)
            }
            ToneRegister::FreqL => 0,
            ToneRegister::FreqH => ((self.sequencer.length_enabled) as u8) << 6,
        }
    }

    pub fn write_register(&mut self, register: ToneRegister, value: u8) {
        match register {
            ToneRegister::SweepL if self.has_sweep => {
                self.sequencer.sweep_shift = value.bit_range(0..3);
                self.sequencer.sweep_direction = if value.bit(3) {
                    SweepDirection::Decrease
                } else {
                    SweepDirection::Increase
                };
                self.sequencer.sweep_time = value.bit_range(4..7);
            }
            ToneRegister::SweepL => {}
            ToneRegister::DutyL => {
                self.sequencer.length_counter = 64 - (value.bit_range(0..6) as u16);
                self.duty = value.bit_range(6..8);
            }
            ToneRegister::DutyH => {
                self.sequencer.envelope_time = value.bit_range(0..3);
                self.sequencer.envelope_direction = if value.bit(3) {
                    EnvelopeDirection::Increase
                } else {
                    EnvelopeDirection::Decrease
                };
                self.sequencer.envelope_initial = value.bit_range(4..8);
                self.dac_enable = value.bit_range(3..8) != 0;
            }
            ToneRegister::FreqL => {
                self.sequencer
                    .sweep_initial_freq
                    .set_bit_range(0..8, value.bit_range(0..8) as u16);
                self.sequencer.sweep_current_freq = self.sequencer.sweep_initial_freq;
            }
            ToneRegister::FreqH => {
                self.sequencer
                    .sweep_initial_freq
                    .set_bit_range(8..11, value.bit_range(0..3) as u16);
                self.sequencer.sweep_current_freq = self.sequencer.sweep_initial_freq;
                self.sequencer.length_enabled = value.bit(6);

                if value.bit(7) {
                    self.sequencer.restart();
                }
            }
        }
    }
}

mod channel;
mod registers;

use crate::{
    io::{REG_FIFO_A, REG_FIFO_B},
    scheduler::Event,
    Gba,
};
use channel::{DmaChannel, NoiseChannel, ToneChannel, WaveChannel};
use crate::impl_save_state_fields;

/// Audio samples per second.
pub const AUDIO_SAMPLE_RATE: usize = 32768;

/// Number of audio channels.
pub const AUDIO_CHANNELS: usize = 2;

/// Cycles per audio sample.
pub const CYCLES_PER_SAMPLE: usize = 512;

const CHANNEL_LEFT: usize = 0;
const CHANNEL_RIGHT: usize = 1;

/// Audio processing unit state.
pub struct Apu {
    /// Audio buffer: interleaving left/right samples.
    buffer: Vec<i16>,
    /// Current sample index.
    sample: usize,

    /// PSG Channel 1 - Tone & Sweep
    tone1: ToneChannel,
    /// PSG Channel 2 - Tone
    tone2: ToneChannel,
    /// PSG Channel 3 - Wave
    wave: WaveChannel,
    /// PSG Channel 4 - Noise
    noise: NoiseChannel,
    /// DMA audio channels
    dma: [DmaChannel; 2],

    /// Sound 1-4 Master Channel Volume (LEFT, RIGHT) (0-7)
    psg_channel_volume: [u8; 2],
    /// Sound 1-4 Channel Enable Flags (LEFT, RIGHT))
    psg_channel_enable: [[bool; 4]; 2],
    /// Sound 1-4 Mixer Volume (50% or 100%)
    psg_mixer_volume: u8,
    /// PSG/FIFO Master Enable
    master_enable: bool,

    /// Bias level.
    bias_level: u16,
    /// Amplitude Resolution / Sampling Cycle
    resolution: u8,
}

impl_save_state_fields!(Apu {
    buffer,
    sample,
    tone1,
    tone2,
    wave,
    noise,
    dma,
    psg_channel_volume,
    psg_channel_enable,
    psg_mixer_volume,
    master_enable,
    bias_level,
    resolution,
});

impl Apu {
    pub fn new() -> Apu {
        Apu {
            buffer: Vec::new(),
            sample: 0,

            tone1: ToneChannel::new(true),
            tone2: ToneChannel::new(false),
            wave: WaveChannel::new(),
            noise: NoiseChannel::new(),
            dma: [DmaChannel::new(), DmaChannel::new()],

            psg_channel_volume: [0; 2],
            psg_channel_enable: [[false; 4]; 2],
            psg_mixer_volume: 0,
            master_enable: false,
            bias_level: 0x200,
            resolution: 0,
        }
    }
}

impl Gba {
    pub(crate) fn apu_init(&mut self) {
        self.scheduler.push_event(Event::AudioSample, 0);
        self.scheduler.push_event(Event::AudioSequencerTick, 0);
    }

    pub(crate) fn apu_on_sample_event(&mut self, lateness: usize) {
        let samples = 1 + (lateness / CYCLES_PER_SAMPLE);
        let next_sample = CYCLES_PER_SAMPLE - (lateness % CYCLES_PER_SAMPLE);
        self.scheduler.push_event(Event::AudioSample, next_sample);

        if self.should_render {
            for _ in 0..samples {
                let (left, right) = self.emit_sample();
                self.apu.buffer.push(left);
                self.apu.buffer.push(right);
            }
        }
    }

    pub(crate) fn apu_on_sequencer_event(&mut self, lateness: usize) {
        const CYCLES_PER_TICK: usize = channel::Sequencer::CYCLES_PER_TICK;
        let ticks = 1 + (lateness / CYCLES_PER_TICK);
        let next_tick = CYCLES_PER_TICK - (lateness % CYCLES_PER_TICK);
        self.scheduler
            .push_event(Event::AudioSequencerTick, next_tick);

        for _ in 0..ticks {
            self.apu.tone1.sequencer.tick();
            self.apu.tone2.sequencer.tick();
            self.apu.wave.sequencer.tick();
            self.apu.noise.sequencer.tick();
        }
    }

    /// Called when a timer overflows.
    pub(crate) fn apu_on_timer_overflow(&mut self, timer: usize) {
        if self.apu.master_enable {
            if self.apu.dma[0].timer == timer as u8 {
                let channel = &mut self.apu.dma[0];
                channel.sample = channel.fifo.dequeue();
                if channel.fifo.len() <= 16 {
                    self.dma_notify_audio_fifo(REG_FIFO_A);
                }
            }
            if self.apu.dma[1].timer == timer as u8 {
                let channel = &mut self.apu.dma[1];
                channel.sample = channel.fifo.dequeue();
                if channel.fifo.len() <= 16 {
                    self.dma_notify_audio_fifo(REG_FIFO_B);
                }
            }
        }
    }

    /// Returns whether the APU "cares about" a given timer
    /// and should thus receive events when it overflows.
    pub(crate) fn apu_needs_timer(&self, index: usize) -> bool {
        let enable_0 = self.apu.dma[0].channel[0] || self.apu.dma[0].channel[1];
        let enable_1 = self.apu.dma[1].channel[0] || self.apu.dma[1].channel[1];
        let timer_0 = enable_0 && (self.apu.dma[0].timer == index as u8);
        let timer_1 = enable_1 && (self.apu.dma[1].timer == index as u8);
        self.apu.master_enable && (timer_0 || timer_1)
    }

    /// Clear the APU buffer (at the beginning of a frame).
    pub(crate) fn apu_buffer_clear(&mut self) {
        self.apu.buffer.clear();
    }

    /// Get the current APU buffer.
    pub(crate) fn apu_buffer(&self) -> &[i16] {
        &self.apu.buffer
    }

    /// Emit a sample (left and right channels).
    #[inline(always)]
    fn emit_sample(&mut self) -> (i16, i16) {
        let time = self.apu.sample * CYCLES_PER_SAMPLE;
        self.apu.sample += 1;

        let psg_volume = [1, 2, 4, 0][self.apu.psg_mixer_volume as usize];

        self.apu.tone1.advance();
        self.apu.tone2.advance();

        // Cache channel samples that are used by both L and R
        let t1 = self.apu.tone1.sample(time);
        let t2 = self.apu.tone2.sample(time);
        let w = self.apu.wave.sample(time);
        let n = self.apu.noise.sample(time);

        let vol0 = self.apu.psg_channel_volume[0] as i16;
        let vol1 = self.apu.psg_channel_volume[1] as i16;
        let dma0_ch = &self.apu.dma[0].channel;
        let dma0_sample = self.apu.dma[0].sample;
        let dma0_vol = self.apu.dma[0].volume;
        let dma1_ch = &self.apu.dma[1].channel;
        let dma1_sample = self.apu.dma[1].sample;
        let dma1_vol = self.apu.dma[1].volume;

        let mut psg0 = 0i16;
        if self.apu.psg_channel_enable[0][0] { psg0 += t1; }
        if self.apu.psg_channel_enable[0][1] { psg0 += t2; }
        if self.apu.psg_channel_enable[0][2] { psg0 += w; }
        if self.apu.psg_channel_enable[0][3] { psg0 += n; }
        psg0 = (psg0 * psg_volume * vol0) / 28;
        let mut sample0 = psg0;
        if dma0_ch[0] {
            sample0 += (dma0_sample as i16) * (2 << dma0_vol);
        }
        if dma1_ch[0] {
            sample0 += (dma1_sample as i16) * (2 << dma1_vol);
        }

        let mut psg1 = 0i16;
        if self.apu.psg_channel_enable[1][0] { psg1 += t1; }
        if self.apu.psg_channel_enable[1][1] { psg1 += t2; }
        if self.apu.psg_channel_enable[1][2] { psg1 += w; }
        if self.apu.psg_channel_enable[1][3] { psg1 += n; }
        psg1 = (psg1 * psg_volume * vol1) / 28;
        let mut sample1 = psg1;
        if dma0_ch[1] {
            sample1 += (dma0_sample as i16) * (2 << dma0_vol);
        }
        if dma1_ch[1] {
            sample1 += (dma1_sample as i16) * (2 << dma1_vol);
        }

        let b = self.apu.bias_level as i16;
        let o0 = ((sample0 + b).clamp(0, 0x3FF) - 0x200) * 32;
        let o1 = ((sample1 + b).clamp(0, 0x3FF) - 0x200) * 32;
        (o0, o1)
    }
}

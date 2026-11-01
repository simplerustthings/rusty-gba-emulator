use crate::BitIndex;

use crate::io::*;
use crate::Gba;

use super::{channel::ToneRegister, CHANNEL_LEFT, CHANNEL_RIGHT};

impl Gba {
    #[inline(always)]
    pub(crate) fn apu_io_write(&mut self, addr: u32, value: u8) {
        match addr {
            REG_SOUND1CNT_L_L => self.apu.tone1.write_register(ToneRegister::SweepL, value),
            REG_SOUND1CNT_H_L => self.apu.tone1.write_register(ToneRegister::DutyL, value),
            REG_SOUND1CNT_H_H => self.apu.tone1.write_register(ToneRegister::DutyH, value),
            REG_SOUND1CNT_X_L => self.apu.tone1.write_register(ToneRegister::FreqL, value),
            REG_SOUND1CNT_X_H => self.apu.tone1.write_register(ToneRegister::FreqH, value),
            REG_SOUND2CNT_L_L => self.apu.tone2.write_register(ToneRegister::DutyL, value),
            REG_SOUND2CNT_L_H => self.apu.tone2.write_register(ToneRegister::DutyH, value),
            REG_SOUND2CNT_H_L => self.apu.tone2.write_register(ToneRegister::FreqL, value),
            REG_SOUND2CNT_H_H => self.apu.tone2.write_register(ToneRegister::FreqH, value),
            REG_SOUND3CNT_START..=REG_SOUND3CNT_END => self
                .apu
                .wave
                .write_register(addr - REG_SOUND3CNT_START, value),
            REG_WAVE_RAM_START..=REG_WAVE_RAM_END => self
                .apu
                .wave
                .write_wave_ram(addr - REG_WAVE_RAM_START, value),
            REG_SOUND4CNT_START..=REG_SOUND4CNT_END => {
                self.apu
                    .noise
                    .write_register(addr - REG_SOUND4CNT_START, value);
            }
            REG_SOUNDCNT_L_L => {
                self.apu.psg_channel_volume[1] = value.bit_range(0..3);
                self.apu.psg_channel_volume[0] = value.bit_range(4..7);
            }
            REG_SOUNDCNT_L_H => {
                self.apu.psg_channel_enable[1][0] = value.bit(0); self.apu.psg_channel_enable[0][0] = value.bit(4);
                self.apu.psg_channel_enable[1][1] = value.bit(1); self.apu.psg_channel_enable[0][1] = value.bit(5);
                self.apu.psg_channel_enable[1][2] = value.bit(2); self.apu.psg_channel_enable[0][2] = value.bit(6);
                self.apu.psg_channel_enable[1][3] = value.bit(3); self.apu.psg_channel_enable[0][3] = value.bit(7);
            }
            REG_SOUNDCNT_H_L => {
                self.apu.psg_mixer_volume = value.bit_range(0..2);
                self.apu.dma[0].volume = value.bit(2) as u8;
                self.apu.dma[1].volume = value.bit(3) as u8;
            }
            REG_SOUNDCNT_H_H => {
                self.apu.dma[0].channel[CHANNEL_RIGHT] = value.bit(0);
                self.apu.dma[0].channel[CHANNEL_LEFT] = value.bit(1);
                self.apu.dma[0].timer = value.bit(2) as u8;
                if value.bit(3) { self.apu.dma[0].fifo.reset(); }

                self.apu.dma[1].channel[CHANNEL_RIGHT] = value.bit(4);
                self.apu.dma[1].channel[CHANNEL_LEFT] = value.bit(5);
                self.apu.dma[1].timer = value.bit(6) as u8;
                if value.bit(7) { self.apu.dma[1].fifo.reset(); }

                self.timer_update();
            }
            REG_SOUNDCNT_X_L => {
                self.apu.master_enable = value.bit(7);
                self.timer_update();
                // TODO zero psg registers 4000060h..4000081h when disabled.
            }
            REG_SOUNDBIAS_L => {
                self.apu.bias_level.set_bit_range(0..8, value as u16);
            }
            REG_SOUNDBIAS_H => {
                self.apu
                    .bias_level
                    .set_bit_range(8..10, value.bit_range(0..2) as u16);
                self.apu.resolution = value.bit_range(6..8);
            }
            _ => {}
        }
    }

    #[inline(always)]
    pub(crate) fn apu_io_read(&mut self, addr: u32) -> u8 {
        match addr {
            REG_SOUND1CNT_L_L => self.apu.tone1.read_register(ToneRegister::SweepL),
            REG_SOUND1CNT_H_L => self.apu.tone1.read_register(ToneRegister::DutyL),
            REG_SOUND1CNT_H_H => self.apu.tone1.read_register(ToneRegister::DutyH),
            REG_SOUND1CNT_X_L => self.apu.tone1.read_register(ToneRegister::FreqL),
            REG_SOUND1CNT_X_H => self.apu.tone1.read_register(ToneRegister::FreqH),
            REG_SOUND2CNT_L_L => self.apu.tone2.read_register(ToneRegister::DutyL),
            REG_SOUND2CNT_L_H => self.apu.tone2.read_register(ToneRegister::DutyH),
            REG_SOUND2CNT_H_L => self.apu.tone2.read_register(ToneRegister::FreqL),
            REG_SOUND2CNT_H_H => self.apu.tone2.read_register(ToneRegister::FreqH),
            REG_SOUND3CNT_START..=REG_SOUND3CNT_END => {
                self.apu.wave.read_register(addr - REG_SOUND3CNT_START)
            }
            REG_WAVE_RAM_START..=REG_WAVE_RAM_END => {
                self.apu.wave.read_wave_ram(addr - REG_WAVE_RAM_START)
            }
            REG_SOUND4CNT_START..=REG_SOUND4CNT_END => {
                self.apu.noise.read_register(addr - REG_SOUND4CNT_START)
            }
            REG_SOUNDCNT_L_L => {
                self.apu.psg_channel_volume[1] | (self.apu.psg_channel_volume[0] << 4)
            }
            REG_SOUNDCNT_L_H => {
                (self.apu.psg_channel_enable[1][0] as u8)
                    | ((self.apu.psg_channel_enable[1][1] as u8) << 1)
                    | ((self.apu.psg_channel_enable[1][2] as u8) << 2)
                    | ((self.apu.psg_channel_enable[1][3] as u8) << 3)
                    | ((self.apu.psg_channel_enable[0][0] as u8) << 4)
                    | ((self.apu.psg_channel_enable[0][1] as u8) << 5)
                    | ((self.apu.psg_channel_enable[0][2] as u8) << 6)
                    | ((self.apu.psg_channel_enable[0][3] as u8) << 7)
            }
            REG_SOUNDCNT_H_L => {
                self.apu.psg_mixer_volume
                    | (self.apu.dma[0].volume << 2)
                    | (self.apu.dma[1].volume << 3)
            }
            REG_SOUNDCNT_H_H => {
                (self.apu.dma[0].channel[CHANNEL_RIGHT] as u8)
                    | ((self.apu.dma[0].channel[CHANNEL_LEFT] as u8) << 1)
                    | (self.apu.dma[0].timer << 2)
                    | ((self.apu.dma[1].channel[CHANNEL_RIGHT] as u8) << 4)
                    | ((self.apu.dma[1].channel[CHANNEL_LEFT] as u8) << 5)
                    | (self.apu.dma[1].timer << 6)
            }
            REG_SOUNDCNT_X_L => {
                (self.apu.tone1.enabled() as u8)
                    | ((self.apu.tone2.enabled() as u8) << 1)
                    | ((self.apu.wave.enabled() as u8) << 2)
                    | ((self.apu.noise.enabled() as u8) << 3)
                    | ((self.apu.master_enable as u8) << 7)
            }
            REG_SOUNDBIAS_L => (self.apu.bias_level & 0xFF) as u8,
            REG_SOUNDBIAS_H => {
                (((self.apu.bias_level & 0x300) >> 8) as u8) | (self.apu.resolution << 6)
            }
            _ => 0,
        }
    }

    #[inline(always)]
    pub(crate) fn apu_io_fifo_write(&mut self, index: usize, value: u32) {
        let fifo = &mut self.apu.dma[index].fifo;
        let bytes = value.to_le_bytes();
        fifo.enqueue(bytes[0] as i8);
        fifo.enqueue(bytes[1] as i8);
        fifo.enqueue(bytes[2] as i8);
        fifo.enqueue(bytes[3] as i8);
    }
}

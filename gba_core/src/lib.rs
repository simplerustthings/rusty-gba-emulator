mod apu;
mod bus;
mod cartridge;
mod cpu;
mod dma;
mod gba;
mod interrupt;
mod io;
mod keypad;
mod mem;
mod ppu;
mod save_state;
mod scheduler;
mod timer;
pub mod util;

#[cfg(test)]
mod tests;

use apu::Apu;
use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;
use dma::Dma;
use interrupt::InterruptKind;
use io::Io;
use mem::{Addr, Memory};
use ppu::Ppu;
use scheduler::{Event, Scheduler};
use timer::TimerManager;

pub use apu::{AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};
pub use cartridge::{BackupFile, BackupType, Rom};
pub use gba::{Gba, HEIGHT, WIDTH};
pub use keypad::KeypadState;

use std::ops::Range;

pub trait BitIndex {
    fn bit(self, index: usize) -> bool;
    fn bit_range(self, range: Range<usize>) -> Self;
    fn set_bit(&mut self, index: usize, value: bool);
    fn set_bit_range(&mut self, range: Range<usize>, value: Self);
}

macro_rules! impl_bit_index {
    ($ty:ty) => {
        impl BitIndex for $ty {
            #[inline(always)]
            fn bit(self, index: usize) -> bool {
                ((self >> index) & 1) != 0
            }
            #[inline(always)]
            fn bit_range(self, range: Range<usize>) -> $ty {
                let bits = range.end - range.start;
                let type_bits = std::mem::size_of::<$ty>() * 8;
                if bits >= type_bits {
                    return self;
                }
                let mask = ((1u64 << bits) - 1) as $ty;
                (self >> range.start) & mask
            }
            #[inline(always)]
            fn set_bit(&mut self, index: usize, value: bool) {
                let bits = std::mem::size_of::<$ty>() * 8;
                if index < bits {
                    let bit = (1 as $ty).wrapping_shl(index as u32);
                    if value { *self |= bit } else { *self &= !bit }
                }
            }
            #[inline(always)]
            fn set_bit_range(&mut self, range: Range<usize>, value: $ty) {
                let bits = range.end - range.start;
                let type_bits = std::mem::size_of::<$ty>() * 8;
                if bits >= type_bits {
                    *self = value;
                    return;
                }
                let mask = ((1u64 << bits) - 1) as $ty;
                let shifted = (value & mask).wrapping_shl(range.start as u32);
                let cleared = *self & !(mask.wrapping_shl(range.start as u32));
                *self = cleared | shifted;
            }
        }
    };
}

impl_bit_index!(u8);
impl_bit_index!(u16);
impl_bit_index!(u32);
impl_bit_index!(u64);
impl_bit_index!(usize);

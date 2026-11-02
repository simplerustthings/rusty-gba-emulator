use crate::impl_save_state_fields;

/// A DMA audio channel
pub struct DmaChannel {
    /// Channel volume (0=50%, 1=100%)
    pub volume: u8,
    /// Enabled on the left/right channels.
    pub channel: [bool; 2],
    /// Which timer (0 or 1) triggers the next sample.
    pub timer: u8,

    /// The FIFO backing this channel.
    pub fifo: Fifo,
    /// The current sample value.
    pub sample: i8,
}

impl_save_state_fields!(DmaChannel { volume, channel, timer, fifo, sample });

impl DmaChannel {
    pub fn new() -> DmaChannel {
        DmaChannel {
            volume: 0,
            channel: [false; 2],
            timer: 0,

            fifo: Fifo::new(),
            sample: 0,
        }
    }
}

/// FIFO has a buffer of 32 bytes.
pub const FIFO_SIZE: usize = 32;

/// Circular array backed FIFO queue of samples.
pub struct Fifo {
    buffer: [i8; FIFO_SIZE],
    count: usize,
    front: usize,
}

impl_save_state_fields!(Fifo { buffer, count, front });

impl Fifo {
    pub fn new() -> Fifo {
        Fifo {
            buffer: [0; FIFO_SIZE],
            count: 0,
            front: 0,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline(always)]
    pub fn enqueue(&mut self, data: i8) {
        if self.count < FIFO_SIZE {
            self.buffer[(self.front + self.count) & (FIFO_SIZE - 1)] = data;
            self.count += 1;
        }
    }

    #[inline(always)]
    pub fn dequeue(&mut self) -> i8 {
        let mut value = 0;
        if self.count > 0 {
            value = self.buffer[self.front];
            self.front = (self.front + 1) & (FIFO_SIZE - 1);
            self.count -= 1;
        }
        value
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.count = 0;
    }
}

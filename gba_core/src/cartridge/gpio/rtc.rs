use crate::save_state::SaveState;
use crate::impl_save_state_fields;
use crate::BitIndex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Clock Pin
const PIN_SCK: usize = 0;
/// Data Pin
const PIN_SIO: usize = 1;
/// Chip-Select Pin
const PIN_CS: usize = 2;

#[derive(Copy, Clone, Debug)]
enum Register {
    /// Forces time to reset.
    ForceReset = 0,
    /// Control register.
    Control = 4,
    /// DateTime (7 bytes, YMDWHMS)
    DateTime = 2,
    /// Time (3 bytes, HMS)
    Time = 6,
    /// Unused (alarm1), always 0xFF
    Alarm1 = 1,
    /// Unused (alarm2), always 0xFF
    Alarm2 = 5,
    /// Force IRQ
    ForceIrq = 3,
    /// Unused (free), always 0xFF
    Free = 7,
}

impl SaveState for Register {
    fn save(&self, buf: &mut Vec<u8>) {
        (*self as u8).save(buf);
    }
    fn load(buf: &mut &[u8]) -> Self {
        match u8::load(buf) {
            0 => Register::ForceReset,
            1 => Register::Alarm1,
            2 => Register::DateTime,
            3 => Register::ForceIrq,
            4 => Register::Control,
            5 => Register::Alarm2,
            6 => Register::Time,
            7 => Register::Free,
            _ => panic!("Unknown Register discriminant"),
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum State {
    /// Waiting for a command opcode.
    Waiting,
    /// Handling a read register command.
    Read(Register),
    /// Handling a write register command.
    Write(Register),
}

impl SaveState for State {
    fn save(&self, buf: &mut Vec<u8>) {
        match self {
            State::Waiting => buf.push(0),
            State::Read(r) => { buf.push(1); r.save(buf); }
            State::Write(r) => { buf.push(2); r.save(buf); }
        }
    }
    fn load(buf: &mut &[u8]) -> Self {
        match buf[0] {
            0 => { *buf = &buf[1..]; State::Waiting }
            1 => { *buf = &buf[1..]; State::Read(Register::load(buf)) }
            2 => { *buf = &buf[1..]; State::Write(Register::load(buf)) }
            _ => panic!("Unknown State discriminant"),
        }
    }
}

impl Register {
    /// Returns the parameter length in bytes.
    fn param_len(self) -> usize {
        match self {
            Self::ForceReset => 0,
            Self::Control => 1,
            Self::DateTime => 7,
            Self::Time => 3,
            Self::Alarm1 => 1,
            Self::Alarm2 => 1,
            Self::ForceIrq => 0,
            Self::Free => 1,
        }
    }

    fn from_u8(data: u8) -> Self {
        match data {
            0 => Self::ForceReset,
            1 => Self::Alarm1,
            2 => Self::DateTime,
            3 => Self::ForceIrq,
            4 => Self::Control,
            5 => Self::Alarm2,
            6 => Self::Time,
            7 => Self::Free,
            _ => unreachable!(),
        }
    }
}

/// State for the real-time clock.
pub struct Rtc {
    /// Whether chip-select is enabled.
    selected: bool,
    /// Whether the clock pin is high.
    clock: bool,
    /// State of the data pin.
    data: bool,

    /// Current command state.
    state: State,

    /// Serial buffer.
    serial_buffer: [u8; 8],
    /// Number of bits in the serial buffer.
    serial_buffer_len: usize,

    /// Control register: unknown (bit 1) -- "IRQ duty/hold related?"
    control_unknown1: bool,
    /// Control register: per-minute IRQ (bit 3)
    control_irq: bool,
    /// Control register: unknown (bit 5)
    control_unknown2: bool,
    /// Control register: 24-hour mode (bit 6). True for 24H, false for 12H.
    control_24h: bool,
}

impl_save_state_fields!(Rtc {
    selected,
    clock,
    data,
    state,
    serial_buffer,
    serial_buffer_len,
    control_unknown1,
    control_irq,
    control_unknown2,
    control_24h,
});

impl Rtc {
    pub fn new() -> Rtc {
        Rtc {
            selected: false,
            clock: false,
            data: false,

            state: State::Waiting,

            serial_buffer: [0; 8],
            serial_buffer_len: 0,

            control_unknown1: false,
            control_irq: false,
            control_unknown2: false,
            control_24h: false,
        }
    }

    /// Called when GPIO pins are set.
    pub fn pin_write(&mut self, pins: u8) {
        let pin_clock = pins.bit(PIN_SCK);
        let pin_data = pins.bit(PIN_SIO);
        let pin_chip_select = pins.bit(PIN_CS);

        // Handle chip-select.
        if !self.selected {
            if pin_chip_select {
                self.selected = true;
                // println!("rtc: chip selected!");
            }
            return;
        }
        if !pin_chip_select {
            self.selected = false;
            self.reset_serial();
            // println!("rtc: chip unselected.");
            return;
        }

        // Only do something on rising clock edge.
        let rising_clock = pin_clock && !self.clock;
        self.clock = pin_clock;
        if !rising_clock {
            return;
        }

        // Handle the clock.
        // println!("Clock: state={:?}", self.state);
        match self.state {
            State::Waiting => {
                self.data = pin_data;
                if self.serial_read(1) {
                    // We got the command code.
                    let byte = self.serial_buffer[0];
                    self.reset_serial();

                    // Bit swap if necessary. We expect the bottom bits to be 0110.
                    let byte = if byte & 0x0F == 0b0110 {
                        byte
                    } else {
                        byte.reverse_bits()
                    };
                    let register = Register::from_u8(byte.bit_range(4..7));

                    match byte.bit(7) {
                        true => {
                            // Reading a register.
                            self.register_read(register);
                            if register.param_len() > 0 {
                                self.serial_buffer_len = register.param_len() * 8;
                                self.state = State::Read(register);
                            } else {
                                self.state = State::Waiting;
                            }
                        }
                        false => {
                            // Writing a register.
                            if register.param_len() > 0 {
                                self.state = State::Write(register);
                            } else {
                                self.register_write(register);
                                self.state = State::Waiting;
                            }
                        }
                    }
                }
            }
            State::Read(register) => {
                // Output bits, LSB first.
                let bit = (register.param_len() * 8) - self.serial_buffer_len;
                self.data = self.serial_buffer[bit / 8].bit(bit % 8);

                self.serial_buffer_len -= 1;
                if self.serial_buffer_len == 0 {
                    self.state = State::Waiting;
                }
            }
            State::Write(register) => {
                // Wait until we have enough bits...
                self.data = pin_data;
                if self.serial_read(register.param_len()) {
                    self.register_write(register);
                    self.reset_serial();
                    self.state = State::Waiting;
                }
            }
        }
    }

    /// Read a register, filling up the serial buffer.
    fn register_read(&mut self, register: Register) {
        // println!("rtc: read from {:?}", register);
        match register {
            Register::Control => {
                let mut data = 0u8;
                data.set_bit(1, self.control_unknown1);
                data.set_bit(3, self.control_irq);
                data.set_bit(5, self.control_unknown2);
                data.set_bit(6, self.control_24h);
                self.serial_buffer[0] = data;
            }
            Register::DateTime => {
                let t = LocalTime::now();
                self.serial_buffer[0] = encode_bcd(t.year);
                self.serial_buffer[1] = encode_bcd(t.month);
                self.serial_buffer[2] = encode_bcd(t.day);
                self.serial_buffer[3] = encode_bcd(t.weekday);
                let hour = if self.control_24h { t.hour } else { t.hour % 12 };
                let pm = t.hour >= 12;
                self.serial_buffer[4] = encode_bcd(hour) | ((pm as u8) << 7);
                self.serial_buffer[5] = encode_bcd(t.minute);
                self.serial_buffer[6] = encode_bcd(t.second);
            }
            Register::Time => {
                let t = LocalTime::now();
                let hour = if self.control_24h { t.hour } else { t.hour % 12 };
                let pm = t.hour >= 12;
                self.serial_buffer[0] = encode_bcd(hour) | ((pm as u8) << 7);
                self.serial_buffer[1] = encode_bcd(t.minute);
                self.serial_buffer[2] = encode_bcd(t.second);
            }
            _ => {}
        }
    }

    /// Write to a register, using the serial buffer.
    fn register_write(&mut self, register: Register) {
        // println!("rtc: write to {:?}, {:?}", register, self.serial_buffer);
        match register {
            Register::Control => {
                let data = self.serial_buffer[0];
                self.control_unknown1 = data.bit(1);
                self.control_irq = data.bit(3);
                self.control_unknown2 = data.bit(5);
                self.control_24h = data.bit(6);
            }
            Register::ForceReset => {
                // Reset the date and time to 2000-01-01 00:00:00?
                println!("RTC: unimplemented force reset");
                self.control_unknown1 = false;
                self.control_irq = false;
                self.control_unknown2 = false;
                self.control_24h = false;
            }
            Register::ForceIrq => {
                // TODO: support cartridge IRQ
                println!("RTC: unimplemented force IRQ");
            }
            // XXX: support changing the time?
            _ => {}
        }
    }

    /// Called when GPIO pins are read.
    pub fn pin_read(&mut self) -> u8 {
        (self.data as u8) << PIN_SIO
    }

    /// Adds the current data bit to the serial buffer.
    /// Then, returns whether the number of buffered bytes is the requested number.
    fn serial_read(&mut self, requested: usize) -> bool {
        let byte_index = self.serial_buffer_len / 8;
        let bit_index = self.serial_buffer_len % 8;
        self.serial_buffer[byte_index].set_bit(bit_index, self.data);
        self.serial_buffer_len += 1;

        self.serial_buffer_len == (requested * 8)
    }

    /// Resets the serial buffer.
    fn reset_serial(&mut self) {
        self.serial_buffer_len = 0;
    }
}

struct LocalTime {
    year: u8,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl LocalTime {
    fn now() -> Self {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        unix_to_local(duration.as_secs())
    }
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || (y.is_multiple_of(400))
}

fn days_in_year(y: u64) -> u64 {
    if is_leap_year(y) { 366 } else { 365 }
}

fn unix_to_local(secs: u64) -> LocalTime {
    let days = secs / 86400;
    let time = secs % 86400;

    let hour = (time / 3600) as u8;
    let minute = ((time % 3600) / 60) as u8;
    let second = (time % 60) as u8;

    let mut remaining_days = days;
    let mut year = 1970u64;
    while remaining_days >= days_in_year(year) {
        remaining_days -= days_in_year(year);
        year += 1;
    }

    let month_days: &[u64] = if is_leap_year(year) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0u64;
    while remaining_days >= month_days[month as usize] {
        remaining_days -= month_days[month as usize];
        month += 1;
    }

    let day = remaining_days as u8 + 1;
    let month = month as u8 + 1;

    let weekday = ((days + 3) % 7) as u8 + 1;

    LocalTime {
        year: (year % 100) as u8,
        month,
        day,
        weekday,
        hour,
        minute,
        second,
    }
}

/// Converts a regular number (range 0..=99) to binary coded decimal.
fn encode_bcd(input: u8) -> u8 {
    assert!(input < 100);
    let ones = input % 10;
    let tens = input / 10;
    ones | (tens * 16)
}

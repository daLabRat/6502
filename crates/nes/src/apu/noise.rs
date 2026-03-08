use super::LENGTH_TABLE;

/// Noise channel timer period lookup table (NTSC).
static NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

pub struct Noise {
    // Timer
    timer_period: u16,
    timer_counter: u16,

    // Shift register
    shift: u16,
    mode: bool, // false = 1-bit, true = 6-bit

    // Length counter
    pub length_counter: u8,
    length_halt: bool,

    // Envelope
    envelope_start: bool,
    envelope_loop: bool,
    constant_volume: bool,
    envelope_period: u8,
    envelope_counter: u8,
    envelope_decay: u8,
}

impl Noise {
    pub fn new() -> Self {
        Self {
            timer_period: 0,
            timer_counter: 0,
            shift: 1,
            mode: false,
            length_counter: 0,
            length_halt: false,
            envelope_start: false,
            envelope_loop: false,
            constant_volume: false,
            envelope_period: 0,
            envelope_counter: 0,
            envelope_decay: 0,
        }
    }

    pub fn write_register(&mut self, reg: u16, val: u8) {
        match reg {
            0 => {
                self.length_halt = val & 0x20 != 0;
                self.envelope_loop = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.envelope_period = val & 0x0F;
            }
            2 => {
                self.mode = val & 0x80 != 0;
                self.timer_period = NOISE_PERIOD_TABLE[(val & 0x0F) as usize];
            }
            3 => {
                self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            let feedback_bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift & 1) ^ ((self.shift >> feedback_bit) & 1);
            self.shift >>= 1;
            self.shift |= feedback << 14;
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_counter = self.envelope_period;
        } else if self.envelope_counter > 0 {
            self.envelope_counter -= 1;
        } else {
            self.envelope_counter = self.envelope_period;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.envelope_loop {
                self.envelope_decay = 15;
            }
        }
    }

    pub fn clock_length_counter(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub fn snapshot(&self) -> crate::snapshot::NoiseSnapshot {
        crate::snapshot::NoiseSnapshot {
            timer_period: self.timer_period,
            timer_counter: self.timer_counter,
            shift: self.shift,
            mode: self.mode,
            length_counter: self.length_counter,
            length_halt: self.length_halt,
            envelope_start: self.envelope_start,
            envelope_loop: self.envelope_loop,
            constant_volume: self.constant_volume,
            envelope_period: self.envelope_period,
            envelope_counter: self.envelope_counter,
            envelope_decay: self.envelope_decay,
        }
    }

    pub fn restore(&mut self, s: &crate::snapshot::NoiseSnapshot) {
        self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter;
        self.shift = s.shift;
        self.mode = s.mode;
        self.length_counter = s.length_counter;
        self.length_halt = s.length_halt;
        self.envelope_start = s.envelope_start;
        self.envelope_loop = s.envelope_loop;
        self.constant_volume = s.constant_volume;
        self.envelope_period = s.envelope_period;
        self.envelope_counter = s.envelope_counter;
        self.envelope_decay = s.envelope_decay;
    }

    pub fn output(&self) -> u8 {
        if self.length_counter == 0 || self.shift & 1 != 0 {
            return 0;
        }
        if self.constant_volume {
            self.envelope_period
        } else {
            self.envelope_decay
        }
    }
}

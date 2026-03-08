use super::LENGTH_TABLE;

/// Triangle wave sequence.
static TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

pub struct Triangle {
    // Timer
    timer_period: u16,
    timer_counter: u16,

    // Sequencer
    sequence_pos: u8,

    // Length counter
    pub length_counter: u8,
    length_halt: bool,

    // Linear counter
    linear_counter: u8,
    linear_reload_value: u8,
    linear_reload_flag: bool,
    control_flag: bool,
}

impl Triangle {
    pub fn new() -> Self {
        Self {
            timer_period: 0,
            timer_counter: 0,
            sequence_pos: 0,
            length_counter: 0,
            length_halt: false,
            linear_counter: 0,
            linear_reload_value: 0,
            linear_reload_flag: false,
            control_flag: false,
        }
    }

    pub fn write_register(&mut self, reg: u16, val: u8) {
        match reg {
            0 => {
                self.control_flag = val & 0x80 != 0;
                self.length_halt = val & 0x80 != 0;
                self.linear_reload_value = val & 0x7F;
            }
            2 => {
                self.timer_period = (self.timer_period & 0xFF00) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x00FF) | ((val as u16 & 0x07) << 8);
                self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                self.linear_reload_flag = true;
            }
            _ => {}
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.sequence_pos = (self.sequence_pos + 1) % 32;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn clock_linear_counter(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.control_flag {
            self.linear_reload_flag = false;
        }
    }

    pub fn clock_length_counter(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub fn snapshot(&self) -> crate::snapshot::TriangleSnapshot {
        crate::snapshot::TriangleSnapshot {
            timer_period: self.timer_period,
            timer_counter: self.timer_counter,
            sequence_pos: self.sequence_pos,
            length_counter: self.length_counter,
            length_halt: self.length_halt,
            linear_counter: self.linear_counter,
            linear_reload_value: self.linear_reload_value,
            linear_reload_flag: self.linear_reload_flag,
            control_flag: self.control_flag,
        }
    }

    pub fn restore(&mut self, s: &crate::snapshot::TriangleSnapshot) {
        self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter;
        self.sequence_pos = s.sequence_pos;
        self.length_counter = s.length_counter;
        self.length_halt = s.length_halt;
        self.linear_counter = s.linear_counter;
        self.linear_reload_value = s.linear_reload_value;
        self.linear_reload_flag = s.linear_reload_flag;
        self.control_flag = s.control_flag;
    }

    pub fn output(&self) -> u8 {
        if self.length_counter == 0 || self.linear_counter == 0 {
            return 0;
        }
        // Silence on ultrasonic frequencies to prevent popping
        if self.timer_period < 2 {
            return 0;
        }
        TRIANGLE_TABLE[self.sequence_pos as usize]
    }
}

use super::LENGTH_TABLE;

/// Duty cycle waveforms for the pulse channels.
static DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% (negated 25%)
];

pub struct Pulse {
    // Duty
    duty: u8,
    duty_pos: u8,

    // Timer
    timer_period: u16,
    timer_counter: u16,

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

    // Sweep
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_counter: u8,
    sweep_reload: bool,
    is_pulse1: bool,
}

impl Pulse {
    pub fn new(is_pulse1: bool) -> Self {
        Self {
            duty: 0,
            duty_pos: 0,
            timer_period: 0,
            timer_counter: 0,
            length_counter: 0,
            length_halt: false,
            envelope_start: false,
            envelope_loop: false,
            constant_volume: false,
            envelope_period: 0,
            envelope_counter: 0,
            envelope_decay: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_counter: 0,
            sweep_reload: false,
            is_pulse1,
        }
    }

    pub fn write_register(&mut self, reg: u16, val: u8) {
        match reg {
            0 => {
                self.duty = (val >> 6) & 0x03;
                self.length_halt = val & 0x20 != 0;
                self.envelope_loop = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.envelope_period = val & 0x0F;
            }
            1 => {
                self.sweep_enabled = val & 0x80 != 0;
                self.sweep_period = (val >> 4) & 0x07;
                self.sweep_negate = val & 0x08 != 0;
                self.sweep_shift = val & 0x07;
                self.sweep_reload = true;
            }
            2 => {
                self.timer_period = (self.timer_period & 0xFF00) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x00FF) | ((val as u16 & 0x07) << 8);
                self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                self.envelope_start = true;
                self.duty_pos = 0;
            }
            _ => {}
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.duty_pos = (self.duty_pos + 1) % 8;
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

    pub fn clock_sweep(&mut self) {
        let change = self.timer_period >> self.sweep_shift;
        let target = if self.sweep_negate {
            self.timer_period.wrapping_sub(change).wrapping_sub(if self.is_pulse1 { 1 } else { 0 })
        } else {
            self.timer_period.wrapping_add(change)
        };

        if self.sweep_counter == 0 && self.sweep_enabled && self.sweep_shift > 0 {
            if self.timer_period >= 8 && target <= 0x7FF {
                self.timer_period = target;
            }
        }

        if self.sweep_counter == 0 || self.sweep_reload {
            self.sweep_counter = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_counter -= 1;
        }
    }

    pub fn snapshot(&self) -> crate::snapshot::PulseSnapshot {
        crate::snapshot::PulseSnapshot {
            duty: self.duty,
            duty_pos: self.duty_pos,
            timer_period: self.timer_period,
            timer_counter: self.timer_counter,
            length_counter: self.length_counter,
            length_halt: self.length_halt,
            envelope_start: self.envelope_start,
            envelope_loop: self.envelope_loop,
            constant_volume: self.constant_volume,
            envelope_period: self.envelope_period,
            envelope_counter: self.envelope_counter,
            envelope_decay: self.envelope_decay,
            sweep_enabled: self.sweep_enabled,
            sweep_period: self.sweep_period,
            sweep_negate: self.sweep_negate,
            sweep_shift: self.sweep_shift,
            sweep_counter: self.sweep_counter,
            sweep_reload: self.sweep_reload,
            is_pulse1: self.is_pulse1,
        }
    }

    pub fn restore(&mut self, s: &crate::snapshot::PulseSnapshot) {
        self.duty = s.duty;
        self.duty_pos = s.duty_pos;
        self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter;
        self.length_counter = s.length_counter;
        self.length_halt = s.length_halt;
        self.envelope_start = s.envelope_start;
        self.envelope_loop = s.envelope_loop;
        self.constant_volume = s.constant_volume;
        self.envelope_period = s.envelope_period;
        self.envelope_counter = s.envelope_counter;
        self.envelope_decay = s.envelope_decay;
        self.sweep_enabled = s.sweep_enabled;
        self.sweep_period = s.sweep_period;
        self.sweep_negate = s.sweep_negate;
        self.sweep_shift = s.sweep_shift;
        self.sweep_counter = s.sweep_counter;
        self.sweep_reload = s.sweep_reload;
        self.is_pulse1 = s.is_pulse1;
    }

    pub fn output(&self) -> u8 {
        if self.length_counter == 0 { return 0; }
        if self.timer_period < 8 { return 0; }
        // Mute if sweep target would overflow $7FF (not current period)
        let change = self.timer_period >> self.sweep_shift;
        let target = if self.sweep_negate {
            self.timer_period.saturating_sub(change)
                .saturating_sub(if self.is_pulse1 { 1 } else { 0 })
        } else {
            self.timer_period.saturating_add(change)
        };
        if target > 0x7FF { return 0; }
        if DUTY_TABLE[self.duty as usize][self.duty_pos as usize] == 0 { return 0; }
        if self.constant_volume { self.envelope_period } else { self.envelope_decay }
    }
}

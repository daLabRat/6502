use crate::snapshot::RiotSnapshot;

/// RIOT (RAM-I/O-Timer) chip - MOS 6532.
/// 128 bytes RAM, timer, I/O ports for switches/joysticks.
pub struct Riot {
    pub ram: [u8; 128],

    // I/O
    pub swcha: u8,     // Port A: external pin state (joystick directions)
    pub swcha_out: u8, // Port A: output latch (written by game)
    pub swacnt: u8,    // Port A direction (0=input, 1=output)
    pub swchb: u8,     // Port B: console switches
    pub swbcnt: u8,    // Port B direction

    // Timer
    timer_value: u8,
    timer_interval: u32,
    timer_subcycles: u32,
    timer_expired: bool, // After underflow, counts at 1-cycle rate
    timer_flag: bool,
}

impl Riot {
    pub fn new() -> Self {
        Self {
            ram: [0; 128],
            swcha: 0xFF,     // All joystick directions released (active low)
            swcha_out: 0xFF,
            swacnt: 0,       // All inputs
            swchb: 0x0B,     // Default switch state (color TV, P0 difficulty A)
            swbcnt: 0,
            timer_value: 0,
            timer_interval: 1024,
            timer_subcycles: 0,
            timer_expired: false,
            timer_flag: false,
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x2FF;

        // RAM: $80-$FF (mirrored)
        if addr & 0x80 != 0 && addr & 0x200 == 0 {
            return self.ram[(addr & 0x7F) as usize];
        }

        match addr & 0x07 {
            0x00 => {
                // SWCHA: read input pins for input-direction bits,
                // output latch for output-direction bits
                (self.swcha & !self.swacnt) | (self.swcha_out & self.swacnt)
            }
            0x01 => self.swacnt,
            0x02 => (self.swchb & !self.swbcnt) | (self.swchb & self.swbcnt),
            0x03 => self.swbcnt,
            0x04 => {
                // INTIM - read timer value
                self.timer_flag = false;
                self.timer_value
            }
            0x05 => {
                // INSTAT - timer interrupt flags (bit 7 = timer underflow)
                let val = if self.timer_flag { 0xC0 } else { 0x00 };
                val
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        let addr = addr & 0x2FF;

        // RAM: $80-$FF
        if addr & 0x80 != 0 && addr & 0x200 == 0 {
            self.ram[(addr & 0x7F) as usize] = val;
            return;
        }

        match addr & 0x1F {
            0x00 => self.swcha_out = val, // Don't overwrite external pin state
            0x01 => self.swacnt = val,
            0x02 => self.swchb = val,
            0x03 => self.swbcnt = val,
            // Timer writes ($294, $295, $296, $297)
            0x14 => {
                self.timer_value = val;
                self.timer_interval = 1;
                self.timer_subcycles = 0;
                self.timer_expired = false;
                self.timer_flag = false;
            }
            0x15 => {
                self.timer_value = val;
                self.timer_interval = 8;
                self.timer_subcycles = 0;
                self.timer_expired = false;
                self.timer_flag = false;
            }
            0x16 => {
                self.timer_value = val;
                self.timer_interval = 64;
                self.timer_subcycles = 0;
                self.timer_expired = false;
                self.timer_flag = false;
            }
            0x17 => {
                self.timer_value = val;
                self.timer_interval = 1024;
                self.timer_subcycles = 0;
                self.timer_expired = false;
                self.timer_flag = false;
            }
            _ => {}
        }
    }

    pub fn snapshot(&self) -> RiotSnapshot {
        RiotSnapshot {
            ram: self.ram,
            swcha: self.swcha, swcha_out: self.swcha_out, swacnt: self.swacnt,
            swchb: self.swchb, swbcnt: self.swbcnt,
            timer_value: self.timer_value,
            timer_interval: self.timer_interval,
            timer_subcycles: self.timer_subcycles,
            timer_expired: self.timer_expired,
            timer_flag: self.timer_flag,
        }
    }

    pub fn restore(&mut self, s: &RiotSnapshot) {
        self.ram = s.ram;
        self.swcha = s.swcha; self.swcha_out = s.swcha_out; self.swacnt = s.swacnt;
        self.swchb = s.swchb; self.swbcnt = s.swbcnt;
        self.timer_value = s.timer_value;
        self.timer_interval = s.timer_interval;
        self.timer_subcycles = s.timer_subcycles;
        self.timer_expired = s.timer_expired;
        self.timer_flag = s.timer_flag;
    }

    /// Step one CPU cycle.
    pub fn step(&mut self) {
        if self.timer_expired {
            // After underflow, count down at 1-cycle rate
            if self.timer_value == 0 {
                self.timer_value = 0xFF;
            } else {
                self.timer_value = self.timer_value.wrapping_sub(1);
            }
        } else {
            self.timer_subcycles += 1;
            if self.timer_subcycles >= self.timer_interval {
                self.timer_subcycles = 0;
                if self.timer_value == 0 {
                    // Underflow
                    self.timer_expired = true;
                    self.timer_flag = true;
                    self.timer_value = 0xFF;
                } else {
                    self.timer_value -= 1;
                }
            }
        }
    }
}

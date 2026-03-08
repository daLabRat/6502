use crate::snapshot::CiaSnapshot;

/// CIA (Complex Interface Adapter) chip.
/// CIA1: Keyboard matrix scanning, joystick, timer A/B → IRQ.
/// CIA2: VIC-II bank selection, serial bus, timer A/B → NMI.
pub struct Cia {
    /// Port A data register
    pub pra: u8,
    /// Port B data register
    pub prb: u8,
    /// Port A data direction
    pub ddra: u8,
    /// Port B data direction
    pub ddrb: u8,

    // Timer A
    timer_a_latch: u16,
    timer_a_counter: u16,
    timer_a_running: bool,
    timer_a_oneshot: bool,

    // Timer B
    timer_b_latch: u16,
    timer_b_counter: u16,
    timer_b_running: bool,
    timer_b_oneshot: bool,

    // Interrupt
    pub icr_data: u8,   // Interrupt data register
    pub icr_mask: u8,   // Interrupt mask register
    pub irq_pending: bool,

    /// Is this CIA1 (IRQ) or CIA2 (NMI)?
    is_cia1: bool,

    /// Keyboard matrix state (CIA1 only).
    /// 8x8 matrix: rows selected by PRA output, columns read from PRB.
    pub keyboard_matrix: [u8; 8],

    /// Joystick port 2 state (CIA1 only), active-low bits 0-4.
    /// Bit 0=Up, 1=Down, 2=Left, 3=Right, 4=Fire. 0=pressed.
    pub joy2: u8,
}

impl Cia {
    pub fn new(is_cia1: bool) -> Self {
        Self {
            pra: 0xFF,
            prb: 0xFF,
            ddra: 0,
            ddrb: 0,
            timer_a_latch: 0xFFFF,
            timer_a_counter: 0xFFFF,
            timer_a_running: false,
            timer_a_oneshot: false,
            timer_b_latch: 0xFFFF,
            timer_b_counter: 0xFFFF,
            timer_b_running: false,
            timer_b_oneshot: false,
            icr_data: 0,
            icr_mask: 0,
            irq_pending: false,
            is_cia1,
            keyboard_matrix: [0xFF; 8],
            joy2: 0xFF,
        }
    }

    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => {
                if self.is_cia1 {
                    // Port A: output bits from DDRA, input bits high
                    (self.pra & self.ddra) | (!self.ddra)
                } else {
                    (self.pra & self.ddra) | (!self.ddra)
                }
            }
            0x01 => {
                if self.is_cia1 {
                    // Keyboard scanning: for each active row in PRA, OR the column data
                    let mut result = 0xFF;
                    let rows = !(self.pra | !self.ddra);
                    for i in 0..8 {
                        if rows & (1 << i) != 0 {
                            result &= self.keyboard_matrix[i];
                        }
                    }
                    // Joystick port 2 drives bits 0-4 of Port B (active low)
                    result & self.joy2
                } else {
                    (self.prb & self.ddrb) | (!self.ddrb)
                }
            }
            0x02 => self.ddra,
            0x03 => self.ddrb,
            0x04 => (self.timer_a_counter & 0xFF) as u8,
            0x05 => (self.timer_a_counter >> 8) as u8,
            0x06 => (self.timer_b_counter & 0xFF) as u8,
            0x07 => (self.timer_b_counter >> 8) as u8,
            0x0D => {
                // ICR read - returns data and clears it
                let val = self.icr_data;
                self.icr_data = 0;
                self.irq_pending = false;
                val
            }
            _ => 0,
        }
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.pra = val,
            0x01 => self.prb = val,
            0x02 => self.ddra = val,
            0x03 => self.ddrb = val,
            0x04 => self.timer_a_latch = (self.timer_a_latch & 0xFF00) | val as u16,
            0x05 => {
                self.timer_a_latch = (self.timer_a_latch & 0x00FF) | ((val as u16) << 8);
                if !self.timer_a_running {
                    self.timer_a_counter = self.timer_a_latch;
                }
            }
            0x06 => self.timer_b_latch = (self.timer_b_latch & 0xFF00) | val as u16,
            0x07 => {
                self.timer_b_latch = (self.timer_b_latch & 0x00FF) | ((val as u16) << 8);
                if !self.timer_b_running {
                    self.timer_b_counter = self.timer_b_latch;
                }
            }
            0x0D => {
                // ICR mask write
                if val & 0x80 != 0 {
                    self.icr_mask |= val & 0x1F;
                } else {
                    self.icr_mask &= !(val & 0x1F);
                }
            }
            0x0E => {
                self.timer_a_running = val & 0x01 != 0;
                self.timer_a_oneshot = val & 0x08 != 0;
                if val & 0x10 != 0 {
                    self.timer_a_counter = self.timer_a_latch;
                }
            }
            0x0F => {
                self.timer_b_running = val & 0x01 != 0;
                self.timer_b_oneshot = val & 0x08 != 0;
                if val & 0x10 != 0 {
                    self.timer_b_counter = self.timer_b_latch;
                }
            }
            _ => {}
        }
    }

    /// Step one CPU cycle.
    pub fn step(&mut self) {
        // Timer A
        if self.timer_a_running {
            if self.timer_a_counter == 0 {
                self.timer_a_counter = self.timer_a_latch;
                self.icr_data |= 0x01;
                if self.icr_mask & 0x01 != 0 {
                    self.icr_data |= 0x80;
                    self.irq_pending = true;
                }
                if self.timer_a_oneshot {
                    self.timer_a_running = false;
                }
            } else {
                self.timer_a_counter -= 1;
            }
        }

        // Timer B
        if self.timer_b_running {
            if self.timer_b_counter == 0 {
                self.timer_b_counter = self.timer_b_latch;
                self.icr_data |= 0x02;
                if self.icr_mask & 0x02 != 0 {
                    self.icr_data |= 0x80;
                    self.irq_pending = true;
                }
                if self.timer_b_oneshot {
                    self.timer_b_running = false;
                }
            } else {
                self.timer_b_counter -= 1;
            }
        }
    }

    /// Set a key in the keyboard matrix (CIA1 only).
    /// row/col are the matrix position (0-7 each).
    pub fn key_down(&mut self, row: u8, col: u8) {
        if row < 8 && col < 8 {
            self.keyboard_matrix[row as usize] &= !(1 << col);
        }
    }

    /// Release a key in the keyboard matrix.
    pub fn key_up(&mut self, row: u8, col: u8) {
        if row < 8 && col < 8 {
            self.keyboard_matrix[row as usize] |= 1 << col;
        }
    }

    /// Press a joystick port 2 direction/fire (CIA1 only).
    /// bit: 0=Up, 1=Down, 2=Left, 3=Right, 4=Fire
    pub fn joy2_down(&mut self, bit: u8) {
        self.joy2 &= !(1 << bit);
    }

    /// Release a joystick port 2 direction/fire.
    pub fn joy2_up(&mut self, bit: u8) {
        self.joy2 |= 1 << bit;
    }

    pub fn snapshot(&self) -> CiaSnapshot {
        CiaSnapshot {
            pra: self.pra,
            prb: self.prb,
            ddra: self.ddra,
            ddrb: self.ddrb,
            timer_a_latch: self.timer_a_latch,
            timer_a_counter: self.timer_a_counter,
            timer_a_running: self.timer_a_running,
            timer_a_oneshot: self.timer_a_oneshot,
            timer_b_latch: self.timer_b_latch,
            timer_b_counter: self.timer_b_counter,
            timer_b_running: self.timer_b_running,
            timer_b_oneshot: self.timer_b_oneshot,
            icr_data: self.icr_data,
            icr_mask: self.icr_mask,
            irq_pending: self.irq_pending,
            is_cia1: self.is_cia1,
            keyboard_matrix: self.keyboard_matrix,
            joy2: self.joy2,
        }
    }

    pub fn restore(&mut self, s: &CiaSnapshot) {
        self.pra = s.pra;
        self.prb = s.prb;
        self.ddra = s.ddra;
        self.ddrb = s.ddrb;
        self.timer_a_latch = s.timer_a_latch;
        self.timer_a_counter = s.timer_a_counter;
        self.timer_a_running = s.timer_a_running;
        self.timer_a_oneshot = s.timer_a_oneshot;
        self.timer_b_latch = s.timer_b_latch;
        self.timer_b_counter = s.timer_b_counter;
        self.timer_b_running = s.timer_b_running;
        self.timer_b_oneshot = s.timer_b_oneshot;
        self.icr_data = s.icr_data;
        self.icr_mask = s.icr_mask;
        self.irq_pending = s.irq_pending;
        self.keyboard_matrix = s.keyboard_matrix;
        self.joy2 = s.joy2;
    }

    // ── Debugger accessors ────────────────────────────────────────────────

    pub fn timer_a_counter(&self) -> u16  { self.timer_a_counter }
    pub fn timer_a_latch(&self)   -> u16  { self.timer_a_latch }
    pub fn timer_a_running(&self) -> bool { self.timer_a_running }
    pub fn timer_b_counter(&self) -> u16  { self.timer_b_counter }
    pub fn timer_b_latch(&self)   -> u16  { self.timer_b_latch }
    pub fn timer_b_running(&self) -> bool { self.timer_b_running }
}

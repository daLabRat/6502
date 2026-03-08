/// VIA 6522 (Versatile Interface Adapter).
///
/// Used in the 1541 disk drive:
/// - VIA1: IEC serial bus interface
/// - VIA2: Drive mechanics (head stepping, motor, read/write)
///
/// Register layout (offset from base):
///   $0: ORB/IRB (Port B)  $1: ORA/IRA (Port A)
///   $2: DDRB              $3: DDRA
///   $4: T1C-L             $5: T1C-H
///   $6: T1L-L             $7: T1L-H
///   $8: T2C-L             $9: T2C-H
///   $A: SR                $B: ACR
///   $C: PCR               $D: IFR
///   $E: IER               $F: ORA (no handshake)
pub struct Via {
    // Port registers
    pub(crate) ora: u8,   // Output Register A
    pub(crate) orb: u8,   // Output Register B
    pub(crate) ddra: u8,  // Data Direction Register A
    pub(crate) ddrb: u8,  // Data Direction Register B

    // Port A input latch (directly driven by external hardware)
    pub(crate) ira: u8,
    // Port B input latch
    pub(crate) irb: u8,

    // Timer 1
    pub(crate) t1_counter: u16,
    t1_latch: u16,
    t1_running: bool,
    t1_triggered: bool,   // Has T1 fired at least once?

    // Timer 2
    t2_counter: u16,
    t2_latch_lo: u8,
    t2_running: bool,

    // Shift register
    sr: u8,

    // Control
    pub(crate) acr: u8,   // Auxiliary Control Register
    pub(crate) pcr: u8,   // Peripheral Control Register

    // Interrupts
    pub(crate) ifr: u8,   // Interrupt Flag Register
    pub(crate) ier: u8,   // Interrupt Enable Register

    // CA1 edge detection
    pub(crate) ca1_last: bool,

    /// External CA1 input (e.g., byte-ready signal from drive)
    pub(crate) ca1_input: bool,
}

impl Via {
    pub fn new() -> Self {
        Self {
            ora: 0, orb: 0,
            ddra: 0, ddrb: 0,
            ira: 0xFF, irb: 0xFF,
            t1_counter: 0xFFFF, t1_latch: 0xFFFF,
            t1_running: false, t1_triggered: false,
            t2_counter: 0xFFFF, t2_latch_lo: 0xFF,
            t2_running: false,
            sr: 0,
            acr: 0, pcr: 0,
            ifr: 0, ier: 0,
            ca1_last: false,
            ca1_input: false,
        }
    }

    /// Read a VIA register.
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => {
                // Port B: output bits from ORB through DDRB, input bits from IRB
                self.ifr &= !0x18; // Clear CB1/CB2 interrupt flags on Port B read
                self.update_irq();
                (self.orb & self.ddrb) | (self.irb & !self.ddrb)
            }
            0x01 | 0x0F => {
                // Port A (0x01 with handshake, 0x0F without)
                if addr & 0x0F == 0x01 {
                    self.ifr &= !0x03; // Clear CA1/CA2 interrupt flags
                    self.update_irq();
                }
                (self.ora & self.ddra) | (self.ira & !self.ddra)
            }
            0x02 => self.ddrb,
            0x03 => self.ddra,
            0x04 => {
                // T1 counter low — clears T1 interrupt flag
                self.ifr &= !0x40;
                self.update_irq();
                (self.t1_counter & 0xFF) as u8
            }
            0x05 => (self.t1_counter >> 8) as u8,
            0x06 => (self.t1_latch & 0xFF) as u8,
            0x07 => (self.t1_latch >> 8) as u8,
            0x08 => {
                // T2 counter low — clears T2 interrupt flag
                self.ifr &= !0x20;
                self.update_irq();
                (self.t2_counter & 0xFF) as u8
            }
            0x09 => (self.t2_counter >> 8) as u8,
            0x0A => self.sr,
            0x0B => self.acr,
            0x0C => self.pcr,
            0x0D => {
                // IFR: bit 7 = any enabled interrupt active
                let irq = if self.ifr & self.ier & 0x7F != 0 { 0x80 } else { 0x00 };
                self.ifr | irq
            }
            0x0E => self.ier | 0x80, // IER read always has bit 7 set
            _ => 0,
        }
    }

    /// Write a VIA register.
    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => {
                self.orb = val;
                self.ifr &= !0x18; // Clear CB1/CB2 flags
                self.update_irq();
            }
            0x01 | 0x0F => {
                self.ora = val;
                if addr & 0x0F == 0x01 {
                    self.ifr &= !0x03; // Clear CA1/CA2 flags
                    self.update_irq();
                }
            }
            0x02 => self.ddrb = val,
            0x03 => self.ddra = val,
            0x04 => self.t1_latch = (self.t1_latch & 0xFF00) | val as u16,
            0x05 => {
                self.t1_latch = (self.t1_latch & 0x00FF) | ((val as u16) << 8);
                self.t1_counter = self.t1_latch;
                self.t1_running = true;
                self.t1_triggered = false;
                self.ifr &= !0x40; // Clear T1 interrupt
                self.update_irq();
            }
            0x06 => self.t1_latch = (self.t1_latch & 0xFF00) | val as u16,
            0x07 => {
                self.t1_latch = (self.t1_latch & 0x00FF) | ((val as u16) << 8);
                self.ifr &= !0x40;
                self.update_irq();
            }
            0x08 => {
                self.t2_latch_lo = val;
            }
            0x09 => {
                self.t2_counter = (val as u16) << 8 | self.t2_latch_lo as u16;
                self.t2_running = true;
                self.ifr &= !0x20;
                self.update_irq();
            }
            0x0A => self.sr = val,
            0x0B => self.acr = val,
            0x0C => self.pcr = val,
            0x0D => {
                // IFR write: writing 1s clears those flags
                self.ifr &= !val;
                self.update_irq();
            }
            0x0E => {
                // IER: bit 7 determines set/clear
                if val & 0x80 != 0 {
                    self.ier |= val & 0x7F;
                } else {
                    self.ier &= !(val & 0x7F);
                }
                self.update_irq();
            }
            _ => {}
        }
    }

    /// Step one clock cycle: advance timers, detect edges.
    pub fn step(&mut self) {
        // Timer 1
        if self.t1_running {
            if self.t1_counter == 0 {
                self.ifr |= 0x40; // T1 interrupt flag
                if self.acr & 0x40 != 0 {
                    // Free-running mode: reload from latch
                    self.t1_counter = self.t1_latch;
                } else {
                    // One-shot mode
                    if !self.t1_triggered {
                        self.t1_triggered = true;
                    }
                    self.t1_running = false;
                }
                self.update_irq();
            } else {
                self.t1_counter -= 1;
            }
        }

        // Timer 2 (one-shot only in pulse counting mode)
        if self.t2_running && self.acr & 0x20 == 0 {
            if self.t2_counter == 0 {
                self.ifr |= 0x20;
                self.t2_running = false;
                self.update_irq();
            } else {
                self.t2_counter -= 1;
            }
        }

        // CA1 edge detection
        let ca1 = self.ca1_input;
        let ca1_rising = ca1 && !self.ca1_last;
        let ca1_falling = !ca1 && self.ca1_last;
        let ca1_edge = if self.pcr & 0x01 != 0 { ca1_rising } else { ca1_falling };
        if ca1_edge {
            self.ifr |= 0x02; // CA1 interrupt flag
            self.update_irq();
        }
        self.ca1_last = ca1;
    }

    /// Update the IRQ output based on IFR and IER.
    fn update_irq(&mut self) {
        if self.ifr & self.ier & 0x7F != 0 {
            self.ifr |= 0x80;
        } else {
            self.ifr &= !0x80;
        }
    }

    /// Is an IRQ pending?
    pub fn irq_pending(&self) -> bool {
        self.ifr & 0x80 != 0
    }

    /// Get the effective output of Port A (output bits from ORA, others from IRA).
    pub fn port_a_output(&self) -> u8 {
        (self.ora & self.ddra) | (!self.ddra & 0xFF)
    }

    /// Get the effective output of Port B.
    pub fn port_b_output(&self) -> u8 {
        (self.orb & self.ddrb) | (!self.ddrb & 0xFF)
    }
}

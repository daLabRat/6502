/// NES joypad - shift register emulation.
/// Buttons are latched on write to $4016, then read out one bit at a time.
pub struct Controller {
    /// Current button state (bitfield):
    /// bit 0: A, bit 1: B, bit 2: Select, bit 3: Start,
    /// bit 4: Up, bit 5: Down, bit 6: Left, bit 7: Right
    pub buttons: u8,
    /// Shift register for reads
    shift: u8,
    /// Strobe mode: when true, continuously reloads shift register
    strobe: bool,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift: 0,
            strobe: false,
        }
    }

    /// Write to $4016 (strobe)
    pub fn write(&mut self, val: u8) {
        let new_strobe = val & 1 != 0;
        if self.strobe && !new_strobe {
            // Strobe falling edge: latch button state
            self.shift = self.buttons;
        }
        self.strobe = new_strobe;
        if self.strobe {
            self.shift = self.buttons;
        }
    }

    /// Read from $4016/$4017 - returns one button bit
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }
        let val = self.shift & 1;
        self.shift >>= 1;
        // After all 8 bits read, returns 1 (open bus behavior)
        val
    }
}

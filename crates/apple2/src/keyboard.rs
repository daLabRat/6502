/// Apple II keyboard latch.
/// The keyboard data is latched at $C000 (bit 7 = key strobe).
/// Reading $C010 clears the strobe.
pub struct Keyboard {
    pub latch: u8,
    pub strobe: bool,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            latch: 0,
            strobe: false,
        }
    }

    /// Press a key (ASCII value).
    pub fn key_press(&mut self, key: u8) {
        log::info!("Keyboard: key_press ${:02X} ('{}')", key,
            if key >= 0x20 && key < 0x7F { key as char } else { '.' });
        self.latch = key | 0x80; // Set high bit (strobe)
        self.strobe = true;
    }

    /// Read $C000 - returns key with strobe bit.
    pub fn read_key(&self) -> u8 {
        self.latch
    }

    /// Read $C010 - clears strobe, returns strobe state.
    pub fn clear_strobe(&mut self) -> u8 {
        let val = if self.strobe { 0x80 } else { 0x00 };
        self.latch &= 0x7F;
        self.strobe = false;
        val
    }
}

/// Apple II soft switch flags.
/// Soft switches control video modes, speaker, and other I/O.
/// Includes IIe-style extended switches for 80-column card support.
pub struct SoftSwitches {
    pub text_mode: bool,
    pub mixed_mode: bool,
    pub page2: bool,
    pub hires: bool,
    pub an0: bool,
    pub an1: bool,
    pub an2: bool,
    pub an3: bool,

    // IIe extended switches
    pub store80: bool,     // $C000/$C001: 80STORE off/on
    pub ramrd: bool,       // $C002/$C003: RAMRD off/on
    pub ramwrt: bool,      // $C004/$C005: RAMWRT off/on
    pub altzp: bool,       // $C008/$C009: ALTZP off/on
    pub col80: bool,       // $C00C/$C00D: 80COL off/on
    pub altcharset: bool,  // $C00E/$C00F: ALTCHARSET off/on
}

impl SoftSwitches {
    pub fn new() -> Self {
        Self {
            text_mode: true,
            mixed_mode: false,
            page2: false,
            hires: false,
            an0: false,
            an1: false,
            an2: false,
            an3: false,
            store80: false,
            ramrd: false,
            ramwrt: false,
            altzp: false,
            col80: false,
            altcharset: false,
        }
    }

    /// Handle read/write to soft switch addresses ($C050-$C05F).
    pub fn handle(&mut self, addr: u16) {
        match addr {
            0xC050 => self.text_mode = false,   // GR
            0xC051 => self.text_mode = true,    // TEXT
            0xC052 => self.mixed_mode = false,  // FULL
            0xC053 => self.mixed_mode = true,   // MIXED
            0xC054 => self.page2 = false,       // PAGE1
            0xC055 => self.page2 = true,        // PAGE2
            0xC056 => self.hires = false,       // LORES
            0xC057 => self.hires = true,        // HIRES
            0xC058 => self.an0 = false,
            0xC059 => self.an0 = true,
            0xC05A => self.an1 = false,
            0xC05B => self.an1 = true,
            0xC05C => self.an2 = false,
            0xC05D => self.an2 = true,
            0xC05E => self.an3 = false,
            0xC05F => self.an3 = true,
            _ => {}
        }
    }

    /// Handle IIe soft switch writes ($C000-$C00F).
    /// Even addresses = off, odd addresses = on.
    pub fn handle_iie(&mut self, addr: u16) {
        match addr {
            0xC000 => self.store80 = false,
            0xC001 => self.store80 = true,
            0xC002 => self.ramrd = false,
            0xC003 => self.ramrd = true,
            0xC004 => self.ramwrt = false,
            0xC005 => self.ramwrt = true,
            0xC006 | 0xC007 => {} // INTCXROM — not implemented
            0xC008 => self.altzp = false,
            0xC009 => self.altzp = true,
            0xC00A | 0xC00B => {} // SLOTC3ROM — not implemented
            0xC00C => self.col80 = false,
            0xC00D => self.col80 = true,
            0xC00E => self.altcharset = false,
            0xC00F => self.altcharset = true,
            _ => {}
        }
    }

    /// Read IIe status switches ($C011-$C01F).
    /// Returns bit 7 set if the flag is active, 0 otherwise.
    pub fn read_status(&self, addr: u16) -> u8 {
        let active = match addr {
            0xC013 => self.ramrd,
            0xC014 => self.ramwrt,
            0xC016 => self.altzp,
            0xC018 => self.store80,
            0xC01A => self.text_mode,
            0xC01B => self.mixed_mode,
            0xC01C => self.page2,
            0xC01D => self.hires,
            0xC01E => self.altcharset,
            0xC01F => self.col80,
            _ => false,
        };
        if active { 0x80 } else { 0x00 }
    }
}

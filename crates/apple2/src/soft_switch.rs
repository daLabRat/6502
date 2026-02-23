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
    pub intcxrom: bool,    // $C006/$C007: INTCXROM off/on (use internal ROM for $C100-$CFFF)
    pub slotc3rom: bool,   // $C00A/$C00B: SLOTC3ROM off/on (slot vs internal for $C300)
    pub col80: bool,       // $C00C/$C00D: 80COL off/on
    pub altcharset: bool,  // $C00E/$C00F: ALTCHARSET off/on

    /// Language card status (mirrors memory state, set by bus)
    pub lc_bank2: bool,
    pub lc_read_enable: bool,

    /// Vertical blank flag — toggled by the bus based on cycle count.
    pub vbl: bool,

    /// IIe mode: enables lowercase in standard character set ($60-$7F).
    pub is_iie: bool,

    /// IIe $C800-$CFFF expansion ROM active (set by accessing slot 3 ROM,
    /// cleared by accessing $CFFF).
    pub intc8rom: bool,
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
            intcxrom: false,
            slotc3rom: false,
            col80: false,
            altcharset: false,
            lc_bank2: false,
            lc_read_enable: false,
            vbl: false,
            is_iie: false,
            intc8rom: false,
        }
    }

    /// Handle read/write to soft switch addresses ($C050-$C05F).
    pub fn handle(&mut self, addr: u16) {
        match addr {
            0xC050 => { if self.text_mode { log::info!("Switch: TEXT → GR ($C050)"); } self.text_mode = false; }   // GR
            0xC051 => { if !self.text_mode { log::info!("Switch: GR → TEXT ($C051)"); } self.text_mode = true; }    // TEXT
            0xC052 => self.mixed_mode = false,  // FULL
            0xC053 => self.mixed_mode = true,   // MIXED
            0xC054 => { if self.page2 { log::info!("Switch: PAGE2 → PAGE1"); } self.page2 = false; }
            0xC055 => { if !self.page2 { log::info!("Switch: PAGE1 → PAGE2"); } self.page2 = true; }
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
            0xC000 => { log::info!("IIe: 80STORE off"); self.store80 = false; }
            0xC001 => { log::info!("IIe: 80STORE on"); self.store80 = true; }
            0xC002 => { log::info!("IIe: RAMRD off"); self.ramrd = false; }
            0xC003 => { log::info!("IIe: RAMRD on"); self.ramrd = true; }
            0xC004 => { log::info!("IIe: RAMWRT off"); self.ramwrt = false; }
            0xC005 => { log::info!("IIe: RAMWRT on"); self.ramwrt = true; }
            0xC006 => { log::info!("IIe: INTCXROM off"); self.intcxrom = false; }
            0xC007 => { log::debug!("IIe: INTCXROM on"); self.intcxrom = true; }
            0xC008 => { log::info!("IIe: ALTZP off"); self.altzp = false; }
            0xC009 => { log::info!("IIe: ALTZP on"); self.altzp = true; }
            0xC00A => { log::info!("IIe: SLOTC3ROM off (internal)"); self.slotc3rom = false; }
            0xC00B => { log::info!("IIe: SLOTC3ROM on (slot)"); self.slotc3rom = true; }
            0xC00C => { log::info!("IIe: 80COL off"); self.col80 = false; }
            0xC00D => { log::info!("IIe: 80COL on"); self.col80 = true; }
            0xC00E => { log::info!("IIe: ALTCHARSET off"); self.altcharset = false; }
            0xC00F => { log::info!("IIe: ALTCHARSET on"); self.altcharset = true; }
            _ => {}
        }
    }

    /// Read IIe status switches ($C011-$C01F).
    /// Returns bit 7 set if the flag is active, 0 otherwise.
    pub fn read_status(&self, addr: u16) -> u8 {
        let active = match addr {
            0xC011 => self.lc_bank2,
            0xC012 => self.lc_read_enable,
            0xC013 => self.ramrd,
            0xC014 => self.ramwrt,
            0xC015 => self.intcxrom,
            0xC016 => self.altzp,
            0xC017 => self.slotc3rom,
            0xC018 => self.store80,
            // $C019: VBL — bit 7 is 0 during VBL, 1 during active display
            0xC019 => !self.vbl,
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

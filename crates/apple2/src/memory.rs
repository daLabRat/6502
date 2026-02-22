/// Apple II memory system.
/// 48KB main RAM + 16KB language card RAM + ROM.
/// Includes 64KB auxiliary RAM for IIe 80-column card support.
pub struct Memory {
    /// Main RAM (48KB: $0000-$BFFF)
    pub ram: [u8; 49152],
    /// Language card RAM (two 4KB banks + 8KB, $D000-$FFFF)
    pub lc_ram: [u8; 16384],
    pub lc_bank2: [u8; 4096],
    /// ROM ($D000-$FFFF)
    pub rom: Vec<u8>,

    // Language card state
    pub lc_read_enable: bool,
    pub lc_write_enable: bool,
    pub lc_prewrite: bool,
    pub lc_bank1: bool, // true = bank 1 at $D000-$DFFF

    // IIe auxiliary memory (80-column card)
    /// Auxiliary RAM (48KB: $0000-$BFFF)
    pub aux_ram: Vec<u8>,
    /// Auxiliary language card RAM ($D000-$FFFF)
    pub aux_lc_ram: Vec<u8>,
    /// Auxiliary language card bank 2 ($D000-$DFFF)
    pub aux_lc_bank2: Vec<u8>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            ram: [0; 49152],
            lc_ram: [0; 16384],
            lc_bank2: [0; 4096],
            rom: vec![0xFF; 16384],
            lc_read_enable: false,
            lc_write_enable: false,
            lc_prewrite: false,
            lc_bank1: true,
            aux_ram: vec![0; 49152],
            aux_lc_ram: vec![0; 16384],
            aux_lc_bank2: vec![0; 4096],
        }
    }

    /// Read from main text page ($0400-$07FF), always from main RAM.
    pub fn read_main_text(&self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    /// Read from auxiliary text page ($0400-$07FF), always from aux RAM.
    pub fn read_aux_text(&self, addr: u16) -> u8 {
        self.aux_ram[addr as usize]
    }

    /// Read from main hi-res page ($2000-$3FFF), always from main RAM.
    pub fn read_main_hires(&self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    /// Read from auxiliary hi-res page ($2000-$3FFF), always from aux RAM.
    pub fn read_aux_hires(&self, addr: u16) -> u8 {
        self.aux_ram[addr as usize]
    }

    pub fn load_rom(&mut self, data: &[u8]) {
        // Apple II ROM sizes and formats:
        //   12KB: Apple II+ firmware ($D000-$FFFF)
        //   16KB: Full $C000-$FFFF
        //   20KB: 8KB padding/chargen + 12KB firmware (use last 12KB)
        //   32KB: Apple IIe (use first 16KB — main firmware)
        if data.len() == 20480 {
            // 20KB ROM: skip first 8KB (padding/chargen), load 12KB at $D000
            let firmware = &data[8192..];
            let offset = 4096;
            let len = firmware.len().min(self.rom.len() - offset);
            self.rom[offset..offset + len].copy_from_slice(&firmware[..len]);
        } else if data.len() > 16384 {
            // IIe-style 32KB ROM: first 16KB is main firmware ($C000-$FFFF)
            self.rom.copy_from_slice(&data[..16384]);
        } else if data.len() <= 12288 {
            // 12KB ROM: load at $D000 (offset 4096 into 16KB ROM space)
            let offset = 4096;
            let len = data.len().min(self.rom.len() - offset);
            self.rom[offset..offset + len].copy_from_slice(&data[..len]);
        } else {
            // 16KB ROM: fills $C000-$FFFF
            let len = data.len().min(self.rom.len());
            self.rom[..len].copy_from_slice(&data[..len]);
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => self.ram[addr as usize],
            0xC000..=0xCFFF => {
                // I/O space - handled by bus
                0
            }
            0xD000..=0xDFFF => {
                if self.lc_read_enable {
                    if self.lc_bank1 {
                        self.lc_ram[(addr - 0xD000) as usize]
                    } else {
                        self.lc_bank2[(addr - 0xD000) as usize]
                    }
                } else {
                    self.rom[(addr - 0xC000) as usize]
                }
            }
            0xE000..=0xFFFF => {
                if self.lc_read_enable {
                    self.lc_ram[(addr - 0xD000) as usize]
                } else {
                    self.rom[(addr - 0xC000) as usize]
                }
            }
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0xBFFF => self.ram[addr as usize] = val,
            0xD000..=0xDFFF => {
                if self.lc_write_enable {
                    if self.lc_bank1 {
                        self.lc_ram[(addr - 0xD000) as usize] = val;
                    } else {
                        self.lc_bank2[(addr - 0xD000) as usize] = val;
                    }
                }
            }
            0xE000..=0xFFFF => {
                if self.lc_write_enable {
                    self.lc_ram[(addr - 0xD000) as usize] = val;
                }
            }
            _ => {}
        }
    }

    /// Handle language card soft switches ($C080-$C08F).
    pub fn handle_lc_switch(&mut self, addr: u16) {
        let switch = addr & 0x0F;
        self.lc_bank1 = switch & 0x08 == 0;

        match switch & 0x03 {
            0 => {
                self.lc_read_enable = true;
                self.lc_write_enable = false;
                self.lc_prewrite = false;
            }
            1 => {
                self.lc_read_enable = false;
                if self.lc_prewrite {
                    self.lc_write_enable = true;
                }
                self.lc_prewrite = true;
            }
            2 => {
                self.lc_read_enable = false;
                self.lc_write_enable = false;
                self.lc_prewrite = false;
            }
            3 => {
                self.lc_read_enable = true;
                if self.lc_prewrite {
                    self.lc_write_enable = true;
                }
                self.lc_prewrite = true;
            }
            _ => unreachable!(),
        }
    }
}

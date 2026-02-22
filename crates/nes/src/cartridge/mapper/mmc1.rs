use super::{Mapper, Mirroring};

/// MMC1 (Mapper 1) - Nintendo's most common mapper.
/// Serial shift register for bank switching.
/// Games: Zelda, Metroid, Mega Man 2, Final Fantasy.
pub struct Mmc1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    mirroring: Mirroring,

    // Shift register
    shift: u8,
    shift_count: u8,

    // Registers
    control: u8,    // $8000-$9FFF
    chr_bank0: u8,  // $A000-$BFFF
    chr_bank1: u8,  // $C000-$DFFF
    prg_bank: u8,   // $E000-$FFFF
}

impl Mmc1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: vec![0; 8192],
            mirroring,
            shift: 0,
            shift_count: 0,
            control: 0x0C, // PRG fixed last bank mode
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / 16384
    }

    fn chr_bank_count_4k(&self) -> usize {
        (self.chr.len() / 4096).max(1)
    }

    fn update_mirroring(&mut self) {
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::SingleScreenLow,
            1 => Mirroring::SingleScreenHigh,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };
    }
}

impl Mapper for Mmc1 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr as usize - 0x6000) & 0x1FFF],
            0x8000..=0xFFFF => {
                let prg_mode = (self.control >> 2) & 0x03;
                let bank = self.prg_bank as usize & 0x0F;
                let bank_count = self.prg_bank_count();

                let (lo_bank, hi_bank) = match prg_mode {
                    0 | 1 => {
                        // 32KB mode: ignore low bit
                        let b = bank & 0xFE;
                        (b % bank_count, (b + 1) % bank_count)
                    }
                    2 => {
                        // Fix first bank, switch second
                        (0, bank % bank_count)
                    }
                    3 => {
                        // Switch first, fix last bank
                        (bank % bank_count, bank_count - 1)
                    }
                    _ => unreachable!(),
                };

                if addr < 0xC000 {
                    self.prg_rom[lo_bank * 16384 + (addr as usize - 0x8000)]
                } else {
                    self.prg_rom[hi_bank * 16384 + (addr as usize - 0xC000)]
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr as usize - 0x6000) & 0x1FFF] = val;
            }
            0x8000..=0xFFFF => {
                if val & 0x80 != 0 {
                    // Reset shift register
                    self.shift = 0;
                    self.shift_count = 0;
                    self.control |= 0x0C;
                    return;
                }

                self.shift |= (val & 1) << self.shift_count;
                self.shift_count += 1;

                if self.shift_count == 5 {
                    let value = self.shift;
                    match addr {
                        0x8000..=0x9FFF => {
                            self.control = value;
                            self.update_mirroring();
                        }
                        0xA000..=0xBFFF => self.chr_bank0 = value,
                        0xC000..=0xDFFF => self.chr_bank1 = value,
                        0xE000..=0xFFFF => self.prg_bank = value,
                        _ => {}
                    }
                    self.shift = 0;
                    self.shift_count = 0;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }

        let chr_mode = (self.control >> 4) & 1;
        let chr_count = self.chr_bank_count_4k();

        let offset = if chr_mode == 0 {
            // 8KB mode
            let bank = (self.chr_bank0 as usize & 0x1E) % chr_count;
            bank * 4096 + addr as usize
        } else {
            // 4KB mode
            if addr < 0x1000 {
                let bank = self.chr_bank0 as usize % chr_count;
                bank * 4096 + addr as usize
            } else {
                let bank = self.chr_bank1 as usize % chr_count;
                bank * 4096 + (addr as usize - 0x1000)
            }
        };

        self.chr.get(offset).copied().unwrap_or(0)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            let chr_mode = (self.control >> 4) & 1;
            let chr_count = self.chr_bank_count_4k();
            let offset = if chr_mode == 0 {
                let bank = (self.chr_bank0 as usize & 0x1E) % chr_count;
                bank * 4096 + addr as usize
            } else if addr < 0x1000 {
                let bank = self.chr_bank0 as usize % chr_count;
                bank * 4096 + addr as usize
            } else {
                let bank = self.chr_bank1 as usize % chr_count;
                bank * 4096 + (addr as usize - 0x1000)
            };
            if let Some(byte) = self.chr.get_mut(offset) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

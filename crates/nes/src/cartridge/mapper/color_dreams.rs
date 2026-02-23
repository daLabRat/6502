use super::{Mapper, Mirroring};

/// Color Dreams (Mapper 11) - 32KB PRG and 8KB CHR, inverted bit layout from GxROM.
/// Games: Crystal Mines, Bible Adventures, Wisdom Tree titles.
pub struct ColorDreams {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: usize,
    chr_bank: usize,
    prg_bank_count: usize,
    chr_bank_count: usize,
}

impl ColorDreams {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let prg_bank_count = (prg_rom.len() / 32768).max(1);
        let chr_bank_count = (chr_rom.len() / 8192).max(1);
        Self {
            prg_rom,
            chr_rom,
            mirroring,
            prg_bank: 0,
            chr_bank: 0,
            prg_bank_count,
            chr_bank_count,
        }
    }
}

impl Mapper for ColorDreams {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = self.prg_bank * 32768 + (addr as usize - 0x8000);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = (val as usize & 0x03) % self.prg_bank_count;
            self.chr_bank = ((val as usize >> 4) & 0x0F) % self.chr_bank_count;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x2000 {
            let offset = self.chr_bank * 8192 + addr as usize;
            self.chr_rom.get(offset).copied().unwrap_or(0)
        } else {
            0
        }
    }

    fn ppu_write(&mut self, _addr: u16, _val: u8) {
        // CHR ROM only, no writes
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

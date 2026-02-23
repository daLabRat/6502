use super::{Mapper, Mirroring};

/// Camerica (Mapper 71) - 16KB PRG switching (UxROM-like), optional single-screen mirroring.
/// Games: Fire Hawk, Micro Machines, Linus Spacehead, Quattro series.
pub struct Camerica {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: usize,
    last_bank: usize,
}

impl Camerica {
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let last_bank = (prg_rom.len() / 16384).saturating_sub(1);
        Self {
            prg_rom,
            chr_ram: vec![0; 8192],
            mirroring,
            prg_bank: 0,
            last_bank,
        }
    }
}

impl Mapper for Camerica {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let offset = self.prg_bank * 16384 + (addr as usize - 0x8000);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let offset = self.last_bank * 16384 + (addr as usize - 0xC000);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x9000..=0x9FFF => {
                // Mirroring control (bit 4)
                self.mirroring = if val & 0x10 != 0 {
                    Mirroring::SingleScreenHigh
                } else {
                    Mirroring::SingleScreenLow
                };
            }
            0xC000..=0xFFFF => {
                // PRG bank select
                self.prg_bank = (val as usize) % (self.last_bank + 1);
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.chr_ram.get(addr as usize).copied().unwrap_or(0)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            if let Some(byte) = self.chr_ram.get_mut(addr as usize) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

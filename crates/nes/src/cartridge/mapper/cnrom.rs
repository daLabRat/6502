use super::{Mapper, Mirroring};

/// CNROM (Mapper 3) - Simple CHR bank switching, no PRG banking.
/// Games: Arkanoid, Paperboy, Gradius.
pub struct Cnrom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    chr_bank: usize,
    prg_mask: usize,
}

impl Cnrom {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        let prg_mask = if prg_rom.len() <= 16384 { 0x3FFF } else { 0x7FFF };
        Self { prg_rom, chr, mirroring, chr_bank: 0, prg_mask }
    }
}

impl Mapper for Cnrom {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_rom[(addr as usize - 0x8000) & self.prg_mask]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            let bank_count = (self.chr.len() / 8192).max(1);
            self.chr_bank = (val as usize) % bank_count;
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        if addr < 0x2000 {
            let offset = self.chr_bank * 8192 + addr as usize;
            self.chr.get(offset).copied().unwrap_or(0)
        } else {
            0
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            let offset = self.chr_bank * 8192 + addr as usize;
            if let Some(byte) = self.chr.get_mut(offset) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

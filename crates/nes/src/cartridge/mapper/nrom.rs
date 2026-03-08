use super::{Mapper, Mirroring};

/// NROM (Mapper 0) - No bank switching.
/// PRG: 16KB or 32KB, CHR: 8KB.
/// Games: Donkey Kong, Ice Climber, Excitebike, Super Mario Bros.
pub struct Nrom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_mask: usize,
}

impl Nrom {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        // Support both 16KB (mirrored) and 32KB PRG
        let prg_mask = if prg_rom.len() <= 16384 { 0x3FFF } else { 0x7FFF };
        Self { prg_rom, chr, mirroring, prg_mask }
    }
}

impl Mapper for Nrom {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_rom[(addr as usize - 0x8000) & self.prg_mask]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, _addr: u16, _val: u8) {
        // NROM has no writable registers or PRG RAM
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.chr.get(addr as usize).copied().unwrap_or(0)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        // Only works if CHR RAM (no CHR ROM banks)
        if addr < 0x2000 {
            if let Some(byte) = self.chr.get_mut(addr as usize) {
                *byte = val;
            }
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        self.chr.get(addr as usize).copied().unwrap_or(0)
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

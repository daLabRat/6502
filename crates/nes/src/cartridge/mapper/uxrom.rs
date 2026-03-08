use super::{Mapper, Mirroring};

/// UxROM (Mapper 2) - Simple PRG bank switching, no CHR banking.
/// Games: Mega Man, Castlevania, Contra, Duck Tales.
pub struct UxRom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: usize,
    last_bank: usize,
}

impl UxRom {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        let last_bank = (prg_rom.len() / 16384).saturating_sub(1);
        Self { prg_rom, chr, mirroring, prg_bank: 0, last_bank }
    }
}

impl Mapper for UxRom {
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
        if addr >= 0x8000 {
            self.prg_bank = (val as usize) % (self.last_bank + 1);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.chr.get(addr as usize).copied().unwrap_or(0)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            if let Some(byte) = self.chr.get_mut(addr as usize) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_state(&self) -> Vec<u8> {
        vec![self.prg_bank as u8]
    }

    fn restore_mapper_state(&mut self, data: &[u8]) {
        if data.len() >= 1 {
            self.prg_bank = data[0] as usize;
        }
    }
}

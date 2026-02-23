use super::{Mapper, Mirroring};

/// AxROM (Mapper 7) - 32KB PRG bank switching, single-screen mirroring.
/// Games: Battletoads, Marble Madness, Wizards & Warriors.
pub struct AxRom {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: usize,
    prg_bank_count: usize,
}

impl AxRom {
    pub fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let prg_bank_count = (prg_rom.len() / 32768).max(1);
        Self {
            prg_rom,
            chr_ram: vec![0; 8192],
            mirroring: Mirroring::SingleScreenLow,
            prg_bank: 0,
            prg_bank_count,
        }
    }
}

impl Mapper for AxRom {
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
            self.prg_bank = (val as usize & 0x07) % self.prg_bank_count;
            self.mirroring = if val & 0x10 != 0 {
                Mirroring::SingleScreenHigh
            } else {
                Mirroring::SingleScreenLow
            };
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

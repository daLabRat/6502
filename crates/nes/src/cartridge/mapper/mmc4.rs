use super::{Mapper, Mirroring};

/// MMC4 (Mapper 10) - Similar to MMC2 but with 16KB PRG banking.
/// Games: Fire Emblem, Fire Emblem Gaiden.
/// 16KB switchable PRG at $8000, last 16KB fixed at $C000.
/// Same CHR latch mechanism as MMC2.
pub struct Mmc4 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,

    prg_bank: usize,
    last_bank: usize,

    // CHR latches (same as MMC2)
    chr_fd_left: usize,
    chr_fe_left: usize,
    chr_fd_right: usize,
    chr_fe_right: usize,
    latch_left: bool,   // false = FD, true = FE
    latch_right: bool,

    chr_bank_count: usize,
    prg_bank_count: usize,
}

impl Mmc4 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let prg_bank_count = (prg_rom.len() / 16384).max(1);
        let chr_bank_count = (chr_rom.len() / 4096).max(1);
        let last_bank = prg_bank_count.saturating_sub(1);
        Self {
            prg_rom,
            chr_rom,
            mirroring,
            prg_bank: 0,
            last_bank,
            chr_fd_left: 0,
            chr_fe_left: 0,
            chr_fd_right: 0,
            chr_fe_right: 0,
            latch_left: true,
            latch_right: true,
            chr_bank_count,
            prg_bank_count,
        }
    }

    fn resolve_chr(&self, addr: u16) -> usize {
        let bank = if addr < 0x1000 {
            if self.latch_left { self.chr_fe_left } else { self.chr_fd_left }
        } else {
            if self.latch_right { self.chr_fe_right } else { self.chr_fd_right }
        };
        let bank = bank % self.chr_bank_count;
        bank * 4096 + (addr as usize & 0x0FFF)
    }
}

impl Mapper for Mmc4 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank % self.prg_bank_count;
                let offset = bank * 16384 + (addr as usize - 0x8000);
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
            0xA000..=0xAFFF => self.prg_bank = val as usize & 0x0F,
            0xB000..=0xBFFF => self.chr_fd_left = val as usize & 0x1F,
            0xC000..=0xCFFF => self.chr_fe_left = val as usize & 0x1F,
            0xD000..=0xDFFF => self.chr_fd_right = val as usize & 0x1F,
            0xE000..=0xEFFF => self.chr_fe_right = val as usize & 0x1F,
            0xF000..=0xFFFF => {
                self.mirroring = if val & 0x01 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }

        let offset = self.resolve_chr(addr);
        let val = self.chr_rom.get(offset).copied().unwrap_or(0);

        // Latch updates happen AFTER the data is read
        match addr {
            0x0FD8 => self.latch_left = false,
            0x0FE8 => self.latch_left = true,
            0x1FD8..=0x1FDF => self.latch_right = false,
            0x1FE8..=0x1FEF => self.latch_right = true,
            _ => {}
        }

        val
    }

    fn ppu_write(&mut self, _addr: u16, _val: u8) {
        // CHR ROM only, no writes
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_state(&self) -> Vec<u8> {
        let mir = match self.mirroring {
            Mirroring::Horizontal => 1u8,
            _ => 0u8,
        };
        vec![
            self.prg_bank as u8,
            self.chr_fd_left as u8,
            self.chr_fe_left as u8,
            self.chr_fd_right as u8,
            self.chr_fe_right as u8,
            self.latch_left as u8,
            self.latch_right as u8,
            mir,
        ]
    }

    fn restore_mapper_state(&mut self, data: &[u8]) {
        if data.len() >= 8 {
            self.prg_bank = data[0] as usize;
            self.chr_fd_left = data[1] as usize;
            self.chr_fe_left = data[2] as usize;
            self.chr_fd_right = data[3] as usize;
            self.chr_fe_right = data[4] as usize;
            self.latch_left = data[5] != 0;
            self.latch_right = data[6] != 0;
            self.mirroring = if data[7] != 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        }
    }
}

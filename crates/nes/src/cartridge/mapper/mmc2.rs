use super::{Mapper, Mirroring};

/// MMC2 (Mapper 9) - Used exclusively by Punch-Out!!
/// 8KB switchable PRG at $8000, three fixed banks at $A000-$FFFF.
/// Two CHR latches per 4KB half, triggered by specific PPU reads.
pub struct Mmc2 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,

    prg_bank: usize,
    last_three_start: usize, // byte offset of 3rd-to-last 8KB bank

    // CHR latches: each 4KB half has an FD and FE bank, selected by latch state
    chr_fd_left: usize,   // bank for $0000-$0FFF when latch_left == FD
    chr_fe_left: usize,   // bank for $0000-$0FFF when latch_left == FE
    chr_fd_right: usize,  // bank for $1000-$1FFF when latch_right == FD
    chr_fe_right: usize,  // bank for $1000-$1FFF when latch_right == FE
    latch_left: bool,     // false = FD, true = FE
    latch_right: bool,    // false = FD, true = FE

    chr_bank_count: usize, // number of 4KB banks
    prg_bank_count: usize, // number of 8KB banks
}

impl Mmc2 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let prg_bank_count = (prg_rom.len() / 8192).max(1);
        let chr_bank_count = (chr_rom.len() / 4096).max(1);
        let last_three_start = prg_rom.len().saturating_sub(3 * 8192);
        Self {
            prg_rom,
            chr_rom,
            mirroring,
            prg_bank: 0,
            last_three_start,
            chr_fd_left: 0,
            chr_fe_left: 0,
            chr_fd_right: 0,
            chr_fe_right: 0,
            latch_left: true,   // power-on state: FE
            latch_right: true,  // power-on state: FE
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

impl Mapper for Mmc2 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                let offset = (self.prg_bank % self.prg_bank_count) * 8192 + (addr as usize - 0x8000);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            0xA000..=0xFFFF => {
                let offset = self.last_three_start + (addr as usize - 0xA000);
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
            0x0FD8 => self.latch_left = false,         // FD
            0x0FE8 => self.latch_left = true,          // FE
            0x1FD8..=0x1FDF => self.latch_right = false, // FD
            0x1FE8..=0x1FEF => self.latch_right = true,  // FE
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

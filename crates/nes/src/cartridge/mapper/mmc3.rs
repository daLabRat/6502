use super::{Mapper, Mirroring};

/// MMC3 (Mapper 4) - Nintendo's most advanced common mapper.
/// Scanline counter for IRQ, 8KB PRG/CHR banking.
/// Games: Super Mario Bros 2/3, Kirby's Adventure, Mega Man 3-6.
pub struct Mmc3 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    mirroring: Mirroring,

    // Bank registers
    bank_select: u8,
    bank_regs: [u8; 8],

    // IRQ
    irq_counter: u8,
    irq_latch: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Mmc3 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: vec![0; 8192],
            mirroring,
            bank_select: 0,
            bank_regs: [0; 8],
            irq_counter: 0,
            irq_latch: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 8192).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 1024).max(1)
    }

    fn read_prg_bank(&self, bank: usize, offset: usize) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        self.prg_rom.get(bank * 8192 + offset).copied().unwrap_or(0)
    }

    fn read_chr_bank(&self, bank: usize, offset: usize) -> u8 {
        let bank = bank % self.chr_bank_count_1k();
        self.chr.get(bank * 1024 + offset).copied().unwrap_or(0)
    }
}

impl Mapper for Mmc3 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr as usize - 0x6000) & 0x1FFF],
            0x8000..=0xFFFF => {
                let prg_mode = (self.bank_select >> 6) & 1;
                let last_bank = self.prg_bank_count_8k() - 1;
                let second_last = self.prg_bank_count_8k() - 2;
                let r6 = self.bank_regs[6] as usize;
                let r7 = self.bank_regs[7] as usize;

                match addr {
                    0x8000..=0x9FFF => {
                        let bank = if prg_mode == 0 { r6 } else { second_last };
                        self.read_prg_bank(bank, addr as usize - 0x8000)
                    }
                    0xA000..=0xBFFF => {
                        self.read_prg_bank(r7, addr as usize - 0xA000)
                    }
                    0xC000..=0xDFFF => {
                        let bank = if prg_mode == 0 { second_last } else { r6 };
                        self.read_prg_bank(bank, addr as usize - 0xC000)
                    }
                    0xE000..=0xFFFF => {
                        self.read_prg_bank(last_bank, addr as usize - 0xE000)
                    }
                    _ => unreachable!(),
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
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    // Bank select
                    self.bank_select = val;
                } else {
                    // Bank data
                    let reg = (self.bank_select & 0x07) as usize;
                    self.bank_regs[reg] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    // Mirroring
                    self.mirroring = if val & 1 == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
                // Odd: PRG RAM protect (ignored)
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }
        let chr_mode = (self.bank_select >> 7) & 1;

        let (bank, offset) = if chr_mode == 0 {
            // R0,R1 = 2KB banks at $0000/$0800; R2-R5 = 1KB at $1000-$1C00
            match addr {
                0x0000..=0x07FF => ((self.bank_regs[0] & 0xFE) as usize, addr as usize & 0x7FF),
                0x0800..=0x0FFF => ((self.bank_regs[1] & 0xFE) as usize, addr as usize & 0x7FF),
                0x1000..=0x13FF => (self.bank_regs[2] as usize, addr as usize & 0x3FF),
                0x1400..=0x17FF => (self.bank_regs[3] as usize, addr as usize & 0x3FF),
                0x1800..=0x1BFF => (self.bank_regs[4] as usize, addr as usize & 0x3FF),
                0x1C00..=0x1FFF => (self.bank_regs[5] as usize, addr as usize & 0x3FF),
                _ => return 0,
            }
        } else {
            // Inverted: R2-R5 at $0000; R0,R1 at $1000
            match addr {
                0x0000..=0x03FF => (self.bank_regs[2] as usize, addr as usize & 0x3FF),
                0x0400..=0x07FF => (self.bank_regs[3] as usize, addr as usize & 0x3FF),
                0x0800..=0x0BFF => (self.bank_regs[4] as usize, addr as usize & 0x3FF),
                0x0C00..=0x0FFF => (self.bank_regs[5] as usize, addr as usize & 0x3FF),
                0x1000..=0x17FF => ((self.bank_regs[0] & 0xFE) as usize, addr as usize & 0x7FF),
                0x1800..=0x1FFF => ((self.bank_regs[1] & 0xFE) as usize, addr as usize & 0x7FF),
                _ => return 0,
            }
        };

        self.read_chr_bank(bank, offset)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            // CHR RAM write (same banking logic as read)
            let chr_mode = (self.bank_select >> 7) & 1;
            let (bank, offset) = if chr_mode == 0 {
                match addr {
                    0x0000..=0x07FF => ((self.bank_regs[0] & 0xFE) as usize, addr as usize & 0x7FF),
                    0x0800..=0x0FFF => ((self.bank_regs[1] & 0xFE) as usize, addr as usize & 0x7FF),
                    0x1000..=0x13FF => (self.bank_regs[2] as usize, addr as usize & 0x3FF),
                    0x1400..=0x17FF => (self.bank_regs[3] as usize, addr as usize & 0x3FF),
                    0x1800..=0x1BFF => (self.bank_regs[4] as usize, addr as usize & 0x3FF),
                    0x1C00..=0x1FFF => (self.bank_regs[5] as usize, addr as usize & 0x3FF),
                    _ => return,
                }
            } else {
                match addr {
                    0x0000..=0x03FF => (self.bank_regs[2] as usize, addr as usize & 0x3FF),
                    0x0400..=0x07FF => (self.bank_regs[3] as usize, addr as usize & 0x3FF),
                    0x0800..=0x0BFF => (self.bank_regs[4] as usize, addr as usize & 0x3FF),
                    0x0C00..=0x0FFF => (self.bank_regs[5] as usize, addr as usize & 0x3FF),
                    0x1000..=0x17FF => ((self.bank_regs[0] & 0xFE) as usize, addr as usize & 0x7FF),
                    0x1800..=0x1FFF => ((self.bank_regs[1] & 0xFE) as usize, addr as usize & 0x7FF),
                    _ => return,
                }
            };
            let bank = bank % self.chr_bank_count_1k();
            let idx = bank * 1024 + offset;
            if let Some(byte) = self.chr.get_mut(idx) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn scanline_tick(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_clear(&mut self) {
        self.irq_pending = false;
    }

    fn mapper_state(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(15 + self.prg_ram.len());
        data.push(self.bank_select);
        data.extend_from_slice(&self.bank_regs);
        data.push(self.irq_counter);
        data.push(self.irq_latch);
        data.push(self.irq_reload as u8);
        data.push(self.irq_enabled as u8);
        data.push(self.irq_pending as u8);
        // Mirroring: 0=Vertical, 1=Horizontal
        data.push(match self.mirroring {
            Mirroring::Horizontal => 1,
            _ => 0,
        });
        data.extend_from_slice(&self.prg_ram);
        data
    }

    fn restore_mapper_state(&mut self, data: &[u8]) {
        if data.len() >= 15 {
            self.bank_select = data[0];
            self.bank_regs.copy_from_slice(&data[1..9]);
            self.irq_counter = data[9];
            self.irq_latch = data[10];
            self.irq_reload = data[11] != 0;
            self.irq_enabled = data[12] != 0;
            self.irq_pending = data[13] != 0;
            self.mirroring = if data[14] != 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
            // Restore prg_ram if enough bytes remain
            let prg_ram_offset = 15;
            if data.len() >= prg_ram_offset + 8192 {
                self.prg_ram.copy_from_slice(&data[prg_ram_offset..prg_ram_offset + 8192]);
            }
        }
    }
}

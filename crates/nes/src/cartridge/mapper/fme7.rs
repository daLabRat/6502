use super::{Mapper, Mirroring};

/// FME-7 / Sunsoft 5B (Mapper 69) - Advanced mapper with cycle-counting IRQ.
/// Command register at $8000 selects register, data register at $A000 writes value.
/// Games: Batman: Return of the Joker, Gimmick!, Hebereke.
pub struct Fme7 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    mirroring: Mirroring,

    command: u8,

    // CHR banks (regs 0-7): 1KB each
    chr_banks: [usize; 8],

    // PRG banking
    prg_bank_6000: u8,    // reg 8: $6000-$7FFF (bit 6 = RAM select, bit 7 = RAM enable)
    prg_banks: [usize; 3], // regs 9-11: 8KB banks at $8000/$A000/$C000
    last_bank: usize,

    prg_bank_count: usize,
    chr_bank_count: usize,

    // IRQ
    irq_counter: u16,
    irq_enabled: bool,
    irq_counter_enabled: bool,
    irq_pending: bool,
}

impl Fme7 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let prg_bank_count = (prg_rom.len() / 8192).max(1);
        let chr_bank_count = (chr_rom.len() / 1024).max(1);
        let last_bank = prg_bank_count.saturating_sub(1);
        Self {
            prg_rom,
            chr_rom,
            prg_ram: vec![0; 8192],
            mirroring,
            command: 0,
            chr_banks: [0; 8],
            prg_bank_6000: 0,
            prg_banks: [0; 3],
            last_bank,
            prg_bank_count,
            chr_bank_count,
            irq_counter: 0,
            irq_enabled: false,
            irq_counter_enabled: false,
            irq_pending: false,
        }
    }

    fn read_prg_bank(&self, bank: usize, offset: usize) -> u8 {
        let bank = bank % self.prg_bank_count;
        self.prg_rom.get(bank * 8192 + offset).copied().unwrap_or(0)
    }
}

impl Mapper for Fme7 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                let ram_select = self.prg_bank_6000 & 0x40 != 0;
                let ram_enable = self.prg_bank_6000 & 0x80 != 0;
                if ram_select && ram_enable {
                    self.prg_ram[(addr as usize - 0x6000) & 0x1FFF]
                } else if !ram_select {
                    let bank = (self.prg_bank_6000 & 0x3F) as usize;
                    self.read_prg_bank(bank, addr as usize - 0x6000)
                } else {
                    0 // RAM selected but not enabled → open bus
                }
            }
            0x8000..=0x9FFF => self.read_prg_bank(self.prg_banks[0], addr as usize - 0x8000),
            0xA000..=0xBFFF => self.read_prg_bank(self.prg_banks[1], addr as usize - 0xA000),
            0xC000..=0xDFFF => self.read_prg_bank(self.prg_banks[2], addr as usize - 0xC000),
            0xE000..=0xFFFF => self.read_prg_bank(self.last_bank, addr as usize - 0xE000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                let ram_select = self.prg_bank_6000 & 0x40 != 0;
                let ram_enable = self.prg_bank_6000 & 0x80 != 0;
                if ram_select && ram_enable {
                    self.prg_ram[(addr as usize - 0x6000) & 0x1FFF] = val;
                }
            }
            0x8000..=0x9FFF => {
                self.command = val & 0x0F;
            }
            0xA000..=0xBFFF => {
                match self.command {
                    0..=7 => {
                        self.chr_banks[self.command as usize] = val as usize;
                    }
                    8 => {
                        self.prg_bank_6000 = val;
                    }
                    9 => self.prg_banks[0] = (val & 0x3F) as usize,
                    10 => self.prg_banks[1] = (val & 0x3F) as usize,
                    11 => self.prg_banks[2] = (val & 0x3F) as usize,
                    12 => {
                        self.mirroring = match val & 0x03 {
                            0 => Mirroring::Vertical,
                            1 => Mirroring::Horizontal,
                            2 => Mirroring::SingleScreenLow,
                            3 => Mirroring::SingleScreenHigh,
                            _ => unreachable!(),
                        };
                    }
                    13 => {
                        self.irq_counter_enabled = val & 0x80 != 0;
                        self.irq_enabled = val & 0x01 != 0;
                        self.irq_pending = false;
                    }
                    14 => {
                        self.irq_counter = (self.irq_counter & 0xFF00) | val as u16;
                    }
                    15 => {
                        self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }
        let bank_idx = addr as usize / 1024;
        let bank = self.chr_banks[bank_idx] % self.chr_bank_count;
        let offset = addr as usize & 0x3FF;
        self.chr_rom.get(bank * 1024 + offset).copied().unwrap_or(0)
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            let bank_idx = addr as usize / 1024;
            let bank = self.chr_banks[bank_idx] % self.chr_bank_count;
            let offset = addr as usize & 0x3FF;
            let idx = bank * 1024 + offset;
            if let Some(byte) = self.chr_rom.get_mut(idx) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn cpu_tick(&mut self) {
        if self.irq_counter_enabled {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0xFFFF && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_clear(&mut self) {
        self.irq_pending = false;
    }

    fn mapper_state(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(19 + self.prg_ram.len());
        data.push(self.command);
        // chr_banks: 8 bytes
        for &b in &self.chr_banks {
            data.push(b as u8);
        }
        data.push(self.prg_bank_6000);
        // prg_banks: 3 bytes
        for &b in &self.prg_banks {
            data.push(b as u8);
        }
        // mirroring: 0=Vertical, 1=Horizontal, 2=SingleScreenLow, 3=SingleScreenHigh
        let mir = match self.mirroring {
            Mirroring::Vertical => 0u8,
            Mirroring::Horizontal => 1u8,
            Mirroring::SingleScreenLow => 2u8,
            Mirroring::SingleScreenHigh => 3u8,
            _ => 0u8,
        };
        data.push(mir);
        // IRQ state
        data.push((self.irq_counter & 0xFF) as u8);
        data.push((self.irq_counter >> 8) as u8);
        data.push(self.irq_enabled as u8);
        data.push(self.irq_counter_enabled as u8);
        data.push(self.irq_pending as u8);
        data.extend_from_slice(&self.prg_ram);
        data
    }

    fn restore_mapper_state(&mut self, data: &[u8]) {
        if data.len() >= 19 {
            self.command = data[0];
            for i in 0..8 {
                self.chr_banks[i] = data[1 + i] as usize;
            }
            self.prg_bank_6000 = data[9];
            for i in 0..3 {
                self.prg_banks[i] = data[10 + i] as usize;
            }
            self.mirroring = match data[13] {
                0 => Mirroring::Vertical,
                1 => Mirroring::Horizontal,
                2 => Mirroring::SingleScreenLow,
                3 => Mirroring::SingleScreenHigh,
                _ => Mirroring::Vertical,
            };
            self.irq_counter = data[14] as u16 | ((data[15] as u16) << 8);
            self.irq_enabled = data[16] != 0;
            self.irq_counter_enabled = data[17] != 0;
            self.irq_pending = data[18] != 0;
            // Restore prg_ram if enough bytes remain
            let prg_ram_offset = 19;
            if data.len() >= prg_ram_offset + 8192 {
                self.prg_ram.copy_from_slice(&data[prg_ram_offset..prg_ram_offset + 8192]);
            }
        }
    }
}

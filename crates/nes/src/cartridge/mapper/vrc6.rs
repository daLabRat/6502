/// Mapper 24 (VRC6a) and Mapper 26 (VRC6b) — Konami VRC6
///
/// Notable games: Akumajou Densetsu (Castlevania III JP), Madara, Esper Dream 2
///
/// VRC6a and VRC6b are identical except for A0/A1 line swap on the cartridge.
pub struct Vrc6 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: super::super::Mirroring,

    // PRG banking (8KB banks)
    prg_bank_8k_a: u8, // $8000-$9FFF
    prg_bank_8k_b: u8, // $A000-$BFFF
    // $C000-$FFFF: fixed to last 16KB

    // CHR banking (8x 1KB banks)
    chr_banks: [u8; 8],

    // IRQ
    irq_enabled: bool,
    irq_pending: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: u16,
    irq_mode: bool, // false = scanline mode, true = CPU cycle mode

    // VRC6b swaps A0/A1 address lines
    swap_ab: bool,
}

impl Vrc6 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: super::super::Mirroring,
        swap_ab: bool,
    ) -> Self {
        let chr_ram = if chr_rom.is_empty() { vec![0u8; 8192] } else { vec![] };
        Self {
            prg_rom,
            chr_rom,
            chr_ram,
            mirroring,
            prg_bank_8k_a: 0,
            prg_bank_8k_b: 0,
            chr_banks: [0; 8],
            irq_enabled: false,
            irq_pending: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: 341,
            irq_mode: false,
            swap_ab,
        }
    }

    /// VRC6b swaps A0 and A1 to normalize the register address.
    fn normalize_addr(&self, addr: u16) -> u16 {
        if self.swap_ab {
            let a0 = (addr >> 0) & 1;
            let a1 = (addr >> 1) & 1;
            (addr & !0x03) | (a0 << 1) | (a1 << 0)
        } else {
            addr
        }
    }

    fn prg_addr(bank_8k: u8, addr: u16, prg_len: usize) -> usize {
        let offset = (addr & 0x1FFF) as usize;
        (bank_8k as usize * 0x2000 % prg_len) + offset
    }
}

impl super::Mapper for Vrc6 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => 0xFF, // No WRAM on VRC6
            0x8000..=0x9FFF => {
                let i = Self::prg_addr(self.prg_bank_8k_a, addr, self.prg_rom.len());
                self.prg_rom[i]
            }
            0xA000..=0xBFFF => {
                let i = Self::prg_addr(self.prg_bank_8k_b, addr, self.prg_rom.len());
                self.prg_rom[i]
            }
            0xC000..=0xFFFF => {
                let len = self.prg_rom.len();
                let offset = (addr - 0xC000) as usize;
                self.prg_rom[(len - 0x4000 + offset) % len]
            }
            _ => 0xFF,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        let addr = self.normalize_addr(addr);
        match addr {
            // PRG bank selects
            0x8000 => self.prg_bank_8k_a = val & 0x0F,
            0xC000 => self.prg_bank_8k_b = val & 0x1F,
            // CHR banks ($D000-$E003)
            0xD000 => self.chr_banks[0] = val,
            0xD001 => self.chr_banks[1] = val,
            0xD002 => self.chr_banks[2] = val,
            0xD003 => self.chr_banks[3] = val,
            0xE000 => self.chr_banks[4] = val,
            0xE001 => self.chr_banks[5] = val,
            0xE002 => self.chr_banks[6] = val,
            0xE003 => self.chr_banks[7] = val,
            // Mirroring ($B003)
            0xB003 => {
                self.mirroring = match (val >> 2) & 0x03 {
                    0 => super::super::Mirroring::Vertical,
                    1 => super::super::Mirroring::Horizontal,
                    2 => super::super::Mirroring::SingleScreenLow,
                    _ => super::super::Mirroring::SingleScreenHigh,
                };
            }
            // IRQ ($F000-$F002)
            0xF000 => self.irq_latch = val,
            0xF001 => {
                self.irq_mode = val & 0x04 != 0;
                self.irq_enabled = val & 0x02 != 0;
                if self.irq_enabled {
                    self.irq_counter = self.irq_latch;
                    self.irq_prescaler = 341;
                }
                self.irq_pending = false;
            }
            0xF002 => {
                self.irq_enabled = val & 0x01 != 0;
                self.irq_pending = false;
            }
            // Expansion audio ($9000-$9002, $A000-$A002, $B000-$B002) — stubbed
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }
        let bank = (addr / 0x0400) as usize;
        let offset = (addr & 0x03FF) as usize;
        let chr_bank = self.chr_banks[bank] as usize;
        if !self.chr_rom.is_empty() {
            self.chr_rom[(chr_bank * 0x400 + offset) % self.chr_rom.len()]
        } else {
            self.chr_ram[(chr_bank * 0x400 + offset) % self.chr_ram.len()]
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_rom.is_empty() {
            let bank = (addr / 0x0400) as usize;
            let offset = (addr & 0x03FF) as usize;
            let chr_bank = self.chr_banks[bank] as usize;
            let i = (chr_bank * 0x400 + offset) % self.chr_ram.len();
            self.chr_ram[i] = val;
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        if addr >= 0x2000 { return 0; }
        let bank = (addr / 0x0400) as usize;
        let offset = (addr & 0x03FF) as usize;
        let chr_bank = self.chr_banks[bank] as usize;
        if !self.chr_rom.is_empty() {
            self.chr_rom[(chr_bank * 0x400 + offset) % self.chr_rom.len()]
        } else {
            self.chr_ram[(chr_bank * 0x400 + offset) % self.chr_ram.len()]
        }
    }

    fn mirroring(&self) -> super::super::Mirroring { self.mirroring }

    fn cpu_tick(&mut self) {
        if !self.irq_enabled { return; }
        if self.irq_mode {
            // CPU cycle mode: counter ticks every CPU cycle
            if self.irq_counter == 0xFF {
                self.irq_counter = self.irq_latch;
                self.irq_pending = true;
            } else {
                self.irq_counter += 1;
            }
        } else {
            // Scanline mode: prescaler counts 341 PPU cycles (≈ 1 scanline)
            self.irq_prescaler = self.irq_prescaler.saturating_sub(3);
            if self.irq_prescaler == 0 {
                self.irq_prescaler = 341;
                if self.irq_counter == 0xFF {
                    self.irq_counter = self.irq_latch;
                    self.irq_pending = true;
                } else {
                    self.irq_counter += 1;
                }
            }
        }
    }

    fn irq_pending(&self) -> bool { self.irq_pending }
    fn irq_clear(&mut self) { self.irq_pending = false; }
    fn mapper_state(&self) -> Vec<u8> { vec![] }
    fn restore_mapper_state(&mut self, _data: &[u8]) {}
}

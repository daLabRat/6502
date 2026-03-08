/// Mapper 19 — Namco 163 (also known as Namco 129/163)
///
/// Notable games: Pac-Man (NES), Dig Dug, Galaxian, many Namco NES titles
///
/// Features: 4x 8KB PRG banks, 8x 1KB CHR banks, CPU-cycle IRQ, internal VRAM.
/// Expansion audio (wavetable synthesis) is stubbed.
pub struct Namco163 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: super::super::Mirroring,

    // PRG banks (8KB each); $E000 is fixed to last 8KB
    prg_banks: [u8; 3], // $8000, $A000, $C000

    // CHR banks (1KB each)
    chr_banks: [u16; 8],

    // IRQ: counts up to $7FFF, fires when it reaches that value
    irq_enabled: bool,
    irq_pending: bool,
    irq_counter: u16,

    // Nametable bank selects (4 NT slots → CHR ROM pages or internal VRAM)
    nt_banks: [u8; 4],
    // Threshold for CHR ROM vs internal VRAM in NT: pages ≥ this value use VRAM
    nt_ram_disable: u8,

    // Audio address register (stubbed)
    audio_addr: u8,
}

impl Namco163 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: super::super::Mirroring,
    ) -> Self {
        let chr_ram = if chr_rom.is_empty() { vec![0u8; 8192] } else { vec![] };
        Self {
            prg_rom,
            chr_rom,
            chr_ram,
            mirroring,
            prg_banks: [0; 3],
            chr_banks: [0; 8],
            irq_enabled: false,
            irq_pending: false,
            irq_counter: 0,
            nt_banks: [0; 4],
            nt_ram_disable: 0xE0,
            audio_addr: 0,
        }
    }

    fn read_chr(&self, bank: u16, offset: usize) -> u8 {
        if !self.chr_rom.is_empty() {
            let addr = (bank as usize * 0x400 + offset) % self.chr_rom.len();
            self.chr_rom[addr]
        } else {
            let len = self.chr_ram.len().max(1);
            self.chr_ram[(bank as usize * 0x400 + offset) % len]
        }
    }

    fn prg_addr(&self, bank: u8, addr: u16) -> usize {
        (bank as usize * 0x2000 + (addr & 0x1FFF) as usize) % self.prg_rom.len()
    }
}

impl super::Mapper for Namco163 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x4800 => 0, // Audio data read (stubbed)
            0x5000 => (self.irq_counter & 0xFF) as u8,
            0x5800 => ((self.irq_counter >> 8) & 0x7F) as u8
                | if self.irq_enabled { 0x80 } else { 0 },
            0x8000..=0x9FFF => {
                let i = self.prg_addr(self.prg_banks[0], addr);
                self.prg_rom[i]
            }
            0xA000..=0xBFFF => {
                let i = self.prg_addr(self.prg_banks[1], addr);
                self.prg_rom[i]
            }
            0xC000..=0xDFFF => {
                let i = self.prg_addr(self.prg_banks[2], addr);
                self.prg_rom[i]
            }
            0xE000..=0xFFFF => {
                // Fixed: last 8KB
                let len = self.prg_rom.len();
                let offset = (addr - 0xE000) as usize;
                self.prg_rom[(len.saturating_sub(0x2000) + offset) % len]
            }
            _ => 0xFF,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            // Audio data write ($4800) — stubbed
            0x4800 => {}
            // IRQ counter low ($5000)
            0x5000 => {
                self.irq_counter = (self.irq_counter & 0x7F00) | val as u16;
                self.irq_pending = false;
            }
            // IRQ counter high + enable ($5800)
            0x5800 => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16 & 0x7F) << 8);
                self.irq_enabled = val & 0x80 != 0;
                if !self.irq_enabled { self.irq_pending = false; }
            }
            // CHR banks 0-7 ($8000-$BFFF, one 1KB bank per $800 window)
            0x8000..=0xBFFF => {
                let idx = ((addr - 0x8000) / 0x800) as usize;
                if idx < 8 {
                    self.chr_banks[idx] = val as u16;
                }
            }
            // NT bank selects ($C000-$DFFF, one per $800 window)
            0xC000..=0xDFFF => {
                let idx = ((addr - 0xC000) / 0x800) as usize;
                if idx < 4 {
                    self.nt_banks[idx] = val;
                }
            }
            // PRG bank 0 + audio enable ($E000)
            0xE000 => {
                self.prg_banks[0] = val & 0x3F;
                self.nt_ram_disable = val & 0xC0;
            }
            // PRG bank 1 ($E800)
            0xE800 => self.prg_banks[1] = val & 0x3F,
            // PRG bank 2 ($F000)
            0xF000 => self.prg_banks[2] = val & 0x3F,
            // Audio address ($F800)
            0xF800 => self.audio_addr = val & 0x7F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x2000 {
            let bank_idx = (addr / 0x400) as usize;
            let offset = (addr & 0x03FF) as usize;
            self.read_chr(self.chr_banks[bank_idx], offset)
        } else {
            0
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_rom.is_empty() {
            let bank_idx = (addr / 0x400) as usize;
            let offset = (addr & 0x03FF) as usize;
            let len = self.chr_ram.len().max(1);
            let i = (self.chr_banks[bank_idx] as usize * 0x400 + offset) % len;
            self.chr_ram[i] = val;
        }
    }

    fn mirroring(&self) -> super::super::Mirroring { self.mirroring }

    fn cpu_tick(&mut self) {
        if !self.irq_enabled { return; }
        let next = (self.irq_counter + 1) & 0x7FFF;
        if next == 0x7FFF {
            self.irq_pending = true;
        }
        self.irq_counter = next;
    }

    fn irq_pending(&self) -> bool { self.irq_pending }
    fn irq_clear(&mut self) { self.irq_pending = false; }
    fn mapper_state(&self) -> Vec<u8> { vec![] }
    fn restore_mapper_state(&mut self, _data: &[u8]) {}
}

/// C64 memory system with PLA bank switching.
/// 64KB RAM + ROM overlays (BASIC, KERNAL, CHAR, I/O).
pub struct Memory {
    pub ram: [u8; 65536],
    pub basic_rom: Vec<u8>,   // 8KB at $A000-$BFFF
    pub kernal_rom: Vec<u8>,  // 8KB at $E000-$FFFF
    pub char_rom: Vec<u8>,    // 4KB at $D000-$DFFF

    /// CPU port at $0001 controls bank switching.
    /// Bits: x x CHAREN HIRAM LORAM
    pub cpu_port: u8,
    pub cpu_port_dir: u8,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            ram: [0; 65536],
            basic_rom: vec![0xFF; 8192],
            kernal_rom: vec![0xFF; 8192],
            char_rom: vec![0xFF; 4096],
            cpu_port: 0x37,     // Default: all ROMs enabled
            cpu_port_dir: 0x2F, // Default direction
        }
    }

    /// Load the three C64 ROMs.
    pub fn load_roms(&mut self, basic: &[u8], kernal: &[u8], char_rom: &[u8]) {
        if basic.len() >= 8192 {
            self.basic_rom[..8192].copy_from_slice(&basic[..8192]);
        }
        if kernal.len() >= 8192 {
            self.kernal_rom[..8192].copy_from_slice(&kernal[..8192]);
        }
        if char_rom.len() >= 4096 {
            self.char_rom[..4096].copy_from_slice(&char_rom[..4096]);
        }
    }

    /// Whether BASIC ROM is visible at $A000-$BFFF.
    pub fn basic_visible(&self) -> bool {
        let port = self.effective_port();
        port & 0x03 == 0x03 // LORAM=1 and HIRAM=1
    }

    /// Whether KERNAL ROM is visible at $E000-$FFFF.
    pub fn kernal_visible(&self) -> bool {
        let port = self.effective_port();
        port & 0x02 != 0 // HIRAM=1
    }

    /// Whether I/O is visible at $D000-$DFFF (vs CHAR ROM vs RAM).
    pub fn io_visible(&self) -> bool {
        let port = self.effective_port();
        port & 0x04 != 0 && (port & 0x03) != 0 // CHAREN=1 and not all RAM mode
    }

    /// Whether CHAR ROM is visible at $D000-$DFFF.
    pub fn char_rom_visible(&self) -> bool {
        let port = self.effective_port();
        port & 0x04 == 0 && (port & 0x03) != 0 // CHAREN=0 and not all RAM mode
    }

    fn effective_port(&self) -> u8 {
        (self.cpu_port & self.cpu_port_dir) | (!self.cpu_port_dir & 0x37)
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000 => self.cpu_port_dir,
            0x0001 => self.cpu_port,
            0xA000..=0xBFFF if self.basic_visible() => {
                self.basic_rom[(addr - 0xA000) as usize]
            }
            0xD000..=0xDFFF if self.char_rom_visible() => {
                self.char_rom[(addr - 0xD000) as usize]
            }
            0xD000..=0xDFFF if self.io_visible() => {
                // I/O space - handled by bus
                0
            }
            0xE000..=0xFFFF if self.kernal_visible() => {
                self.kernal_rom[(addr - 0xE000) as usize]
            }
            _ => self.ram[addr as usize],
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000 => self.cpu_port_dir = val,
            0x0001 => self.cpu_port = val,
            _ => {
                // All writes go to RAM (ROMs are read-only overlays)
                self.ram[addr as usize] = val;
            }
        }
    }
}

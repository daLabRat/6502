/// Atari 2600 cartridge.
/// Supports 2KB, 4KB (standard), and 8KB (F8 bank switching).
pub struct Cartridge {
    rom: Vec<u8>,
    bank: usize,
    bank_count: usize,
}

impl Cartridge {
    pub fn new(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Err("ROM data is empty".into());
        }

        let size = data.len();
        let bank_count = match size {
            ..=2048 => 1,    // 2KB
            ..=4096 => 1,    // 4KB (standard)
            ..=8192 => 2,    // 8KB (F8)
            ..=16384 => 4,   // 16KB (F6)
            _ => return Err(format!("Unsupported ROM size: {} bytes", size)),
        };

        let mut rom = data.to_vec();
        // Pad to at least 4KB if needed
        while rom.len() < 4096 {
            let len = rom.len();
            rom.extend_from_slice(&rom[..len].to_vec());
        }

        Ok(Self {
            rom,
            bank: 0,
            bank_count,
        })
    }

    pub fn read(&self, addr: u16) -> u8 {
        let offset = if self.bank_count == 1 {
            // No bank switching
            (addr as usize) & (self.rom.len() - 1)
        } else {
            // F8: 2 x 4KB banks
            self.bank * 4096 + (addr as usize & 0x0FFF)
        };
        self.rom.get(offset).copied().unwrap_or(0)
    }

    pub fn check_bank_switch(&mut self, addr: u16) {
        match self.bank_count {
            2 => {
                // F8 bank switching
                match addr {
                    0x1FF8 => self.bank = 0,
                    0x1FF9 => self.bank = 1,
                    _ => {}
                }
            }
            4 => {
                // F6 bank switching
                match addr {
                    0x1FF6 => self.bank = 0,
                    0x1FF7 => self.bank = 1,
                    0x1FF8 => self.bank = 2,
                    0x1FF9 => self.bank = 3,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

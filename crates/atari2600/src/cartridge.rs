/// Atari 2600 cartridge with support for multiple bank-switching schemes.
pub struct Cartridge {
    rom: Vec<u8>,
    scheme: BankScheme,
}

#[derive(Clone)]
#[allow(dead_code)]
enum BankScheme {
    /// 2KB or 4KB — no bank switching
    Fixed,
    /// F8: two 4KB banks; switch at $1FF8/$1FF9
    F8 { bank: usize },
    /// F6: four 4KB banks; switch at $1FF6-$1FF9
    F6 { bank: usize },
    /// E0 (Parker Bros): four 1KB windows into eight 1KB banks
    E0 { windows: [usize; 4] },
    /// FE (Activision): two 4KB banks; switch triggered by write to $01FE
    Fe { bank: usize, last_byte: u8 },
    /// 3F (Tigervision): lower 2KB bank switched by write to $3F; upper 2KB fixed to last bank
    TF { bank: usize },
}

impl Cartridge {
    pub fn new(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Err("ROM data is empty".into());
        }

        let size = data.len();
        let scheme = match size {
            ..=4096  => BankScheme::Fixed,
            8192     => BankScheme::F8 { bank: 0 },
            ..=16384 => BankScheme::F6 { bank: 0 },
            _        => return Err(format!("Unsupported ROM size: {} bytes", size)),
        };

        let mut cart = Self { rom: data.to_vec(), scheme };

        // Pad Fixed ROMs to 4KB by mirroring
        if matches!(cart.scheme, BankScheme::Fixed) {
            while cart.rom.len() < 4096 {
                let len = cart.rom.len();
                cart.rom.extend_from_slice(&cart.rom[..len].to_vec());
            }
        }

        // Detect E0: 8KB ROMs whose reset vector high byte points into fixed window ($1C00-$1FFF).
        // E0 games always start in the fixed window (bank 7). F8 ROMs reset into bank 1's $1000 range.
        if size == 8192 {
            let reset_hi = data[8191]; // last byte of ROM = high byte of IRQ/reset vector
            if reset_hi == 0x1E || reset_hi == 0x1F {
                cart.scheme = BankScheme::E0 { windows: [0, 1, 2, 7] };
            }
        }

        Ok(cart)
    }

    pub fn read(&self, addr: u16) -> u8 {
        let offset = match &self.scheme {
            BankScheme::Fixed => (addr as usize) & (self.rom.len() - 1),
            BankScheme::F8 { bank } => bank * 4096 + (addr as usize & 0x0FFF),
            BankScheme::F6 { bank } => bank * 4096 + (addr as usize & 0x0FFF),
            BankScheme::E0 { windows } => {
                let window = (addr as usize & 0x0FFF) >> 10; // bits 11-10 → window 0-3
                let bank_offset = windows[window] * 1024;
                let within = addr as usize & 0x03FF;
                bank_offset + within
            }
            BankScheme::Fe { bank, .. } => bank * 4096 + (addr as usize & 0x0FFF),
            BankScheme::TF { bank } => {
                let within = addr as usize & 0x0FFF;
                if within >= 0x0800 {
                    // Upper 2KB: fixed to last 2KB of ROM
                    self.rom.len() - 2048 + (within & 0x07FF)
                } else {
                    bank * 2048 + within
                }
            }
        };
        self.rom.get(offset).copied().unwrap_or(0)
    }

    /// Called on every bus read and write to check for bank-switch hotspots.
    pub fn check_bank_switch(&mut self, addr: u16) {
        let cart_addr = addr & 0x0FFF; // offset within cartridge window
        match &mut self.scheme {
            BankScheme::F8 { bank } => match addr {
                0x1FF8 => *bank = 0,
                0x1FF9 => *bank = 1,
                _ => {}
            },
            BankScheme::F6 { bank } => match addr {
                0x1FF6 => *bank = 0,
                0x1FF7 => *bank = 1,
                0x1FF8 => *bank = 2,
                0x1FF9 => *bank = 3,
                _ => {}
            },
            BankScheme::E0 { windows } => {
                // Hotspots: $1FE0-$1FE7 → window 0; $1FE8-$1FEF → window 1; $1FF0-$1FF7 → window 2
                match cart_addr {
                    0xFE0..=0xFE7 => windows[0] = (cart_addr & 0x7) as usize,
                    0xFE8..=0xFEF => windows[1] = (cart_addr & 0x7) as usize,
                    0xFF0..=0xFF7 => windows[2] = (cart_addr & 0x7) as usize,
                    _ => {}
                }
            }
            BankScheme::Fe { .. } => {} // FE switching handled in check_bank_switch_write
            BankScheme::TF { .. } => {} // 3F switching handled in check_bank_switch_write
            BankScheme::Fixed => {}
        }
    }

    /// Called on every bus write to handle write-triggered bank switching (FE, 3F).
    pub fn check_bank_switch_write(&mut self, addr: u16, val: u8) {
        match &mut self.scheme {
            BankScheme::TF { bank } => {
                if addr == 0x003F {
                    *bank = (val & 0x03) as usize;
                }
            }
            BankScheme::Fe { bank, last_byte } => {
                // FE switching: monitor writes to $01FE where bit 5 selects bank.
                if addr == 0x01FE {
                    *bank = if val & 0x20 != 0 { 0 } else { 1 };
                }
                let _ = last_byte;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f8_bank_switch() {
        let mut rom = vec![0x00u8; 8192];
        for b in &mut rom[4096..] { *b = 0x55; }
        let mut cart = Cartridge::new(&rom).unwrap();
        // Default bank 0
        assert_eq!(cart.read(0x1000), 0x00);
        // Switch to bank 1
        cart.check_bank_switch(0x1FF9);
        assert_eq!(cart.read(0x1000), 0x55);
        // Switch back to bank 0
        cart.check_bank_switch(0x1FF8);
        assert_eq!(cart.read(0x1000), 0x00);
    }

    #[test]
    fn test_f6_bank_switch() {
        let mut rom = vec![0u8; 16384];
        for i in 0..4 { for b in &mut rom[i * 4096..(i + 1) * 4096] { *b = i as u8; } }
        let mut cart = Cartridge::new(&rom).unwrap();
        for (hotspot, expected) in [(0x1FF6u16, 0u8), (0x1FF7, 1), (0x1FF8, 2), (0x1FF9, 3)] {
            cart.check_bank_switch(hotspot);
            assert_eq!(cart.read(0x1000), expected, "F6 bank {}", expected);
        }
    }

    #[test]
    fn test_3f_bank_switch() {
        // 8KB ROM with 4 × 2KB banks, each filled with bank index
        let mut rom = vec![0u8; 8192];
        for i in 0..4 { for b in &mut rom[i * 2048..(i + 1) * 2048] { *b = i as u8; } }
        let mut cart = Cartridge { rom, scheme: BankScheme::TF { bank: 0 } };
        // Lower 2KB = bank 0
        assert_eq!(cart.read(0x1000), 0x00);
        // Write $01 to $003F → switch lower to bank 1
        cart.check_bank_switch_write(0x003F, 1);
        assert_eq!(cart.read(0x1000), 0x01);
        // Upper 2KB = always last 2KB (bank 3)
        assert_eq!(cart.read(0x1800), 0x03);
    }

    #[test]
    fn test_e0_window_select() {
        // 8KB ROM, each 1KB bank filled with its bank index (0-7)
        let mut rom = vec![0u8; 8192];
        for i in 0..8 { for b in &mut rom[i * 1024..(i + 1) * 1024] { *b = i as u8; } }
        let mut cart = Cartridge { rom, scheme: BankScheme::E0 { windows: [0, 1, 2, 7] } };

        // Default: window 0 = bank 0, window 1 = bank 1, window 2 = bank 2, window 3 = bank 7
        assert_eq!(cart.read(0x1000), 0); // window 0, bank 0
        assert_eq!(cart.read(0x1400), 1); // window 1, bank 1
        assert_eq!(cart.read(0x1800), 2); // window 2, bank 2
        assert_eq!(cart.read(0x1C00), 7); // window 3, bank 7 (fixed)

        // Switch window 0 to bank 5 via hotspot $1FE5
        cart.check_bank_switch(0x1FE5);
        assert_eq!(cart.read(0x1000), 5);

        // Switch window 1 to bank 3 via hotspot $1FEB
        cart.check_bank_switch(0x1FEB);
        assert_eq!(cart.read(0x1400), 3);
    }
}

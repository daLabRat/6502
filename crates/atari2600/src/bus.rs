use emu_common::Bus;
use crate::tia::Tia;
use crate::riot::Riot;
use crate::cartridge::Cartridge;

/// Atari 2600 bus.
/// The 6507 CPU only has 13 address lines (8KB address space).
/// Memory map (mirrored throughout):
///   $00-$7F:   TIA registers
///   $80-$FF:   RIOT RAM (128 bytes)
///   $280-$2FF: RIOT I/O + timer
///   $F000-$FFFF: Cartridge ROM (4KB, may be bank-switched)
pub struct Atari2600Bus {
    pub tia: Tia,
    pub riot: Riot,
    pub cartridge: Cartridge,
}

impl Atari2600Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            tia: Tia::new(),
            riot: Riot::new(),
            cartridge,
        }
    }
}

impl Bus for Atari2600Bus {
    fn read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x1FFF; // 13-bit address space

        // Check bank switching hotspots
        self.cartridge.check_bank_switch(addr);

        if addr & 0x1000 != 0 {
            // Cartridge ROM
            self.cartridge.read(addr)
        } else if addr & 0x0280 == 0x0280 {
            // RIOT registers
            self.riot.read(addr)
        } else if addr & 0x0080 != 0 {
            // RIOT RAM
            self.riot.ram[(addr & 0x7F) as usize]
        } else {
            // TIA
            self.tia.read(addr)
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        let addr = addr & 0x1FFF;

        self.cartridge.check_bank_switch(addr);
        self.cartridge.check_bank_switch_write(addr, val);

        if addr & 0x1000 != 0 {
            // Cartridge (some mappers have writable areas)
        } else if addr & 0x0280 == 0x0280 {
            self.riot.write(addr, val);
        } else if addr & 0x0080 != 0 {
            self.riot.ram[(addr & 0x7F) as usize] = val;
        } else {
            self.tia.write(addr, val);
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x1FFF;
        if addr & 0x1000 != 0 {
            self.cartridge.read(addr)
        } else if addr & 0x0080 != 0 {
            self.riot.ram[(addr & 0x7F) as usize]
        } else {
            0
        }
    }

    fn tick(&mut self, cycles: u8) {
        // Each CPU cycle = 3 TIA color clocks
        for _ in 0..cycles {
            self.riot.step();
            self.tia.step_clock();
            self.tia.step_clock();
            self.tia.step_clock();
        }
    }

    fn poll_nmi(&mut self) -> bool {
        false // 6507 has no NMI
    }

    fn poll_irq(&mut self) -> bool {
        false // 6507 has no IRQ pin
    }
}

use emu_common::Bus;
use crate::memory::Memory;
use crate::vic_ii::VicII;
use crate::sid::Sid;
use crate::cia::Cia;

/// C64 memory bus.
pub struct C64Bus {
    pub memory: Memory,
    pub vic: VicII,
    pub sid: Sid,
    pub cia1: Cia,
    pub cia2: Cia,
    /// IEC bus input bits for CIA2 Port A (bit 6=CLK in, bit 7=DATA in).
    /// Updated by the system emulator from the shared IEC bus each cycle.
    pub(crate) iec_input: u8,
}

impl C64Bus {
    pub fn new() -> Self {
        Self {
            memory: Memory::new(),
            vic: VicII::new(),
            sid: Sid::new(),
            cia1: Cia::new(true),
            cia2: Cia::new(false),
            iec_input: 0xC0, // Both CLK and DATA released (high) by default
        }
    }

    /// Get the VIC-II bank base address from CIA2 Port A bits 0-1 (inverted).
    fn vic_bank_base(&self) -> u16 {
        let bits = !self.cia2.pra & 0x03;
        (bits as u16) * 0x4000
    }
}

impl Bus for C64Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000 => self.memory.cpu_port_dir,
            0x0001 => self.memory.cpu_port,
            0xD000..=0xD3FF if self.memory.io_visible() => {
                self.vic.read_register(addr)
            }
            0xD400..=0xD7FF if self.memory.io_visible() => {
                self.sid.read_register(addr)
            }
            0xD800..=0xDBFF if self.memory.io_visible() => {
                self.vic.color_ram[(addr - 0xD800) as usize]
            }
            0xDC00..=0xDCFF if self.memory.io_visible() => {
                self.cia1.read_register(addr)
            }
            0xDD00..=0xDDFF if self.memory.io_visible() => {
                let val = self.cia2.read_register(addr);
                if addr & 0x0F == 0x00 {
                    // Port A: merge IEC bus input on bits 6-7 (active low)
                    // Output bits (DDRA=1) from CIA, input bits (DDRA=0) from IEC bus
                    let ddra = self.cia2.ddra;
                    (val & ddra) | (self.iec_input & !ddra)
                } else {
                    val
                }
            }
            _ => self.memory.read(addr),
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000 | 0x0001 => self.memory.write(addr, val),
            0xD000..=0xD3FF if self.memory.io_visible() => {
                self.vic.write_register(addr, val);
            }
            0xD400..=0xD7FF if self.memory.io_visible() => {
                self.sid.write_register(addr, val);
            }
            0xD800..=0xDBFF if self.memory.io_visible() => {
                self.vic.color_ram[(addr - 0xD800) as usize] = val & 0x0F;
            }
            0xDC00..=0xDCFF if self.memory.io_visible() => {
                self.cia1.write_register(addr, val);
            }
            0xDD00..=0xDDFF if self.memory.io_visible() => {
                self.cia2.write_register(addr, val);
            }
            _ => self.memory.write(addr, val),
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        self.memory.read(addr)
    }

    fn tick(&mut self, cycles: u8) {
        // Update VIC bank from CIA2 before stepping
        self.vic.vic_bank_base = self.vic_bank_base();

        for _ in 0..cycles {
            self.vic.step(&self.memory.ram, &self.memory.char_rom);
            self.sid.step();
            self.cia1.step();
            self.cia2.step();
        }

        // Consume badline stall cycles — caller (CPU) should skip these
        if self.vic.stall_cycles > 0 {
            let stall = self.vic.stall_cycles;
            self.vic.stall_cycles = 0;
            // Step hardware for the stall period (VIC steals these cycles)
            for _ in 0..stall {
                self.vic.step(&self.memory.ram, &self.memory.char_rom);
                self.sid.step();
                self.cia1.step();
                self.cia2.step();
            }
        }
    }

    fn poll_nmi(&mut self) -> bool {
        let pending = self.cia2.irq_pending;
        self.cia2.irq_pending = false;
        pending
    }

    fn poll_irq(&mut self) -> bool {
        self.cia1.irq_pending || self.vic.irq_pending
    }
}

/// KERNAL trap handler for virtual disk drive.
///
/// Intercepts KERNAL ROM calls when PC hits specific addresses,
/// emulating a 1541 drive (device 8) without full hardware emulation.
/// Works for standard LOAD, SAVE, OPEN, CLOSE, CHKIN, BASIN operations.

use crate::d64_image::D64Image;
use emu_cpu::Cpu6502;
use crate::bus::C64Bus;

/// Virtual disk drive state.
pub struct KernalDrive {
    pub d64: Option<D64Image>,
    // SETLFS parameters
    logical_file: u8,
    device_number: u8,
    secondary_addr: u8,
    // SETNAM parameters
    filename: Vec<u8>,
    // Open file buffer for BASIN/CHRIN
    file_buffer: Vec<u8>,
    file_position: usize,
    file_open: bool,
}

impl KernalDrive {
    pub fn new(d64: Option<D64Image>) -> Self {
        Self {
            d64,
            logical_file: 0,
            device_number: 0,
            secondary_addr: 0,
            filename: Vec::new(),
            file_buffer: Vec::new(),
            file_position: 0,
            file_open: false,
        }
    }

    /// Check if the current PC is at a KERNAL trap address.
    /// If so, handle it and return true (caller should skip normal execution).
    pub fn check_trap(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        // Only intercept if we have a D64 mounted
        if self.d64.is_none() {
            return false;
        }

        match cpu.pc {
            0xFFBA => { self.trap_setlfs(cpu); true }
            0xFFBD => { self.trap_setnam(cpu); true }
            0xFFD5 => { self.trap_load(cpu); true }
            0xFFC0 => { self.trap_open(cpu); true }
            0xFFC3 => { self.trap_close(cpu); true }
            0xFFC6 => { self.trap_chkin(cpu); true }
            0xFFCF => { self.trap_basin(cpu); true }
            _ => false,
        }
    }

    /// SETLFS ($FFBA): A=logical file, X=device, Y=secondary address.
    fn trap_setlfs(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        self.logical_file = cpu.a;
        self.device_number = cpu.x;
        self.secondary_addr = cpu.y;
        self.simulate_rts(cpu);
    }

    /// SETNAM ($FFBD): A=name length, X/Y=pointer to name string.
    fn trap_setnam(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        let len = cpu.a as usize;
        let addr = cpu.x as u16 | ((cpu.y as u16) << 8);

        self.filename.clear();
        for i in 0..len {
            self.filename.push(cpu.bus.memory.ram[(addr + i as u16) as usize]);
        }

        self.simulate_rts(cpu);
    }

    /// LOAD ($FFD5): A=0 for LOAD, A=1 for VERIFY.
    /// Only intercepts device 8 operations.
    fn trap_load(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        if self.device_number != 8 {
            // Let the real KERNAL handle non-disk devices
            return;
        }

        let load_flag = cpu.a; // 0=LOAD, 1=VERIFY
        if load_flag != 0 {
            // VERIFY not supported, just succeed
            cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
            self.simulate_rts(cpu);
            return;
        }

        let d64 = self.d64.as_ref().unwrap();

        // Find and load the file
        let file_data = if self.filename == b"*" || self.filename == b"$" || self.filename.is_empty() {
            d64.load_first_prg()
        } else {
            d64.find_and_read_file(&self.filename)
        };

        match file_data {
            Ok(data) if data.len() >= 2 => {
                let load_addr = if self.secondary_addr == 0 {
                    // Secondary addr 0: load to address from X/Y registers
                    cpu.x as u16 | ((cpu.y as u16) << 8)
                } else {
                    // Secondary addr 1+: load to address in file header
                    u16::from_le_bytes([data[0], data[1]])
                };

                let payload = &data[2..];
                let end_addr = load_addr as usize + payload.len();

                if end_addr <= 65536 {
                    cpu.bus.memory.ram[load_addr as usize..end_addr]
                        .copy_from_slice(payload);

                    // Update end-of-load pointer in X/Y
                    cpu.x = (end_addr & 0xFF) as u8;
                    cpu.y = ((end_addr >> 8) & 0xFF) as u8;

                    // Update BASIC pointers if loaded at $0801
                    if load_addr == 0x0801 {
                        let end = end_addr as u16;
                        cpu.bus.memory.ram[0x2D] = end as u8;
                        cpu.bus.memory.ram[0x2E] = (end >> 8) as u8;
                        cpu.bus.memory.ram[0x2F] = end as u8;
                        cpu.bus.memory.ram[0x30] = (end >> 8) as u8;
                        cpu.bus.memory.ram[0x31] = end as u8;
                        cpu.bus.memory.ram[0x32] = (end >> 8) as u8;
                    }

                    log::info!(
                        "KERNAL LOAD: '{}' at ${:04X}-${:04X} ({} bytes)",
                        String::from_utf8_lossy(&self.filename),
                        load_addr,
                        end_addr - 1,
                        payload.len()
                    );

                    // Clear carry = success
                    cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
                    // Set status byte ($90) to 0 (no error)
                    cpu.bus.memory.ram[0x90] = 0x00;
                } else {
                    log::error!("KERNAL LOAD: file too large for memory");
                    cpu.p.insert(emu_cpu::flags::StatusFlags::CARRY);
                    cpu.a = 0x04; // File not found error
                }
            }
            Ok(_) => {
                log::error!("KERNAL LOAD: file too small");
                cpu.p.insert(emu_cpu::flags::StatusFlags::CARRY);
                cpu.a = 0x04;
            }
            Err(e) => {
                log::error!("KERNAL LOAD: {}", e);
                cpu.p.insert(emu_cpu::flags::StatusFlags::CARRY);
                cpu.a = 0x04; // File not found
            }
        }

        self.simulate_rts(cpu);
    }

    /// OPEN ($FFC0): Open a file for reading.
    fn trap_open(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        if self.device_number != 8 {
            return;
        }

        let d64 = self.d64.as_ref().unwrap();

        let file_data = if self.filename.is_empty() || self.filename == b"*" {
            d64.load_first_prg()
        } else {
            d64.find_and_read_file(&self.filename)
        };

        match file_data {
            Ok(data) => {
                self.file_buffer = data;
                self.file_position = 0;
                self.file_open = true;
                cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
            }
            Err(_) => {
                cpu.p.insert(emu_cpu::flags::StatusFlags::CARRY);
                cpu.a = 0x04;
            }
        }

        self.simulate_rts(cpu);
    }

    /// CLOSE ($FFC3): Close a file.
    fn trap_close(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        self.file_buffer.clear();
        self.file_position = 0;
        self.file_open = false;
        cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        self.simulate_rts(cpu);
    }

    /// CHKIN ($FFC6): Set input channel. Just acknowledge for device 8.
    fn trap_chkin(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        if self.device_number != 8 {
            return;
        }
        cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        self.simulate_rts(cpu);
    }

    /// BASIN/CHRIN ($FFCF): Read next byte from open file.
    fn trap_basin(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        if !self.file_open || self.device_number != 8 {
            return;
        }

        if self.file_position < self.file_buffer.len() {
            cpu.a = self.file_buffer[self.file_position];
            self.file_position += 1;

            // Set EOI on last byte
            if self.file_position >= self.file_buffer.len() {
                cpu.bus.memory.ram[0x90] = 0x40; // EOI
            } else {
                cpu.bus.memory.ram[0x90] = 0x00;
            }
            cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        } else {
            // Past end of file
            cpu.a = 0x00;
            cpu.bus.memory.ram[0x90] = 0x42; // EOI + timeout
            cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        }

        self.simulate_rts(cpu);
    }

    /// Simulate an RTS: pull return address from stack, set PC = addr + 1.
    fn simulate_rts(&self, cpu: &mut Cpu6502<C64Bus>) {
        let lo = cpu.pull() as u16;
        let hi = cpu.pull() as u16;
        cpu.pc = ((hi << 8) | lo).wrapping_add(1);
    }
}

/// KERNAL trap handler for virtual disk drive.
///
/// Intercepts KERNAL ROM calls when PC hits specific addresses,
/// emulating a 1541 drive (device 8) without full hardware emulation.
/// Works for standard LOAD, OPEN, CLOSE, CHKIN, BASIN operations.
///
/// We do NOT trap SETLFS ($FFBA) or SETNAM ($FFBD) — those run during
/// KERNAL boot and trapping them corrupts the boot sequence. Instead,
/// we read the parameters from KERNAL zero-page at the point of I/O:
///   $B7 = FNLEN (filename length)
///   $B8 = LA (logical file number)
///   $B9 = SA (secondary address)
///   $BA = FA (device number)
///   $BB/$BC = FNADR (filename pointer, lo/hi)

use crate::d64_image::D64Image;
use emu_cpu::Cpu6502;
use crate::bus::C64Bus;

/// KERNAL zero-page addresses for file I/O parameters.
const ZP_FNLEN: usize = 0xB7;
const ZP_SA: usize = 0xB9;
const ZP_FA: usize = 0xBA;
const ZP_FNADR_LO: usize = 0xBB;
const ZP_FNADR_HI: usize = 0xBC;

/// Virtual disk drive state.
pub struct KernalDrive {
    pub d64: Option<D64Image>,
    // Open file buffer for BASIN/CHRIN
    file_buffer: Vec<u8>,
    file_position: usize,
    file_open: bool,
}

impl KernalDrive {
    pub fn new(d64: Option<D64Image>) -> Self {
        Self {
            d64,
            file_buffer: Vec::new(),
            file_position: 0,
            file_open: false,
        }
    }

    /// Check if the current PC is at a KERNAL trap address.
    /// If so, handle it and return true (caller should skip normal execution).
    /// Returns false if the trap wasn't handled (e.g., non-disk device),
    /// allowing the real KERNAL code to execute.
    pub fn check_trap(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        // Only intercept if we have a D64 mounted
        if self.d64.is_none() {
            return false;
        }

        match cpu.pc {
            0xFFD5 => self.trap_load(cpu),
            0xFFC0 => self.trap_open(cpu),
            0xFFC3 => { self.trap_close(cpu); true }
            0xFFC6 => self.trap_chkin(cpu),
            0xFFCF => self.trap_basin(cpu),
            _ => false,
        }
    }

    /// Read KERNAL's device number from zero page ($BA).
    fn read_device(cpu: &Cpu6502<C64Bus>) -> u8 {
        cpu.bus.memory.ram[ZP_FA]
    }

    /// Read KERNAL's secondary address from zero page ($B9).
    fn read_secondary_addr(cpu: &Cpu6502<C64Bus>) -> u8 {
        cpu.bus.memory.ram[ZP_SA]
    }

    /// Read KERNAL's filename from zero page ($B7=len, $BB/$BC=pointer).
    fn read_filename(cpu: &Cpu6502<C64Bus>) -> Vec<u8> {
        let len = cpu.bus.memory.ram[ZP_FNLEN] as usize;
        let addr = cpu.bus.memory.ram[ZP_FNADR_LO] as u16
            | ((cpu.bus.memory.ram[ZP_FNADR_HI] as u16) << 8);
        let mut name = Vec::with_capacity(len);
        for i in 0..len {
            name.push(cpu.bus.memory.ram[(addr.wrapping_add(i as u16)) as usize]);
        }
        name
    }

    /// LOAD ($FFD5): A=0 for LOAD, A=1 for VERIFY.
    /// Only intercepts device 8 operations. Returns false for other devices.
    fn trap_load(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        let device = Self::read_device(cpu);
        if device != 8 {
            // Let the real KERNAL handle non-disk devices
            return false;
        }

        let load_flag = cpu.a; // 0=LOAD, 1=VERIFY
        if load_flag != 0 {
            // VERIFY not supported, just succeed
            cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
            self.simulate_rts(cpu);
            return true;
        }

        let secondary_addr = Self::read_secondary_addr(cpu);
        let filename = Self::read_filename(cpu);
        let d64 = self.d64.as_ref().unwrap();

        // Find and load the file
        let file_data = if filename == b"$" {
            Ok(d64.generate_directory_listing())
        } else if filename == b"*" || filename.is_empty() {
            d64.load_first_prg()
        } else {
            d64.find_and_read_file(&filename)
        };

        match file_data {
            Ok(data) if data.len() >= 2 => {
                let load_addr = if secondary_addr == 0 {
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
                        String::from_utf8_lossy(&filename),
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
        true
    }

    /// OPEN ($FFC0): Open a file for reading. Returns false for non-device-8.
    fn trap_open(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        let device = Self::read_device(cpu);
        if device != 8 {
            return false;
        }

        let filename = Self::read_filename(cpu);
        let d64 = self.d64.as_ref().unwrap();

        let file_data = if filename == b"$" {
            Ok(d64.generate_directory_listing())
        } else if filename.is_empty() || filename == b"*" {
            d64.load_first_prg()
        } else {
            d64.find_and_read_file(&filename)
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
        true
    }

    /// CLOSE ($FFC3): Close a file.
    fn trap_close(&mut self, cpu: &mut Cpu6502<C64Bus>) {
        self.file_buffer.clear();
        self.file_position = 0;
        self.file_open = false;
        cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        self.simulate_rts(cpu);
    }

    /// CHKIN ($FFC6): Set input channel. Returns false for non-device-8.
    fn trap_chkin(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        let device = Self::read_device(cpu);
        if device != 8 {
            return false;
        }
        cpu.p.remove(emu_cpu::flags::StatusFlags::CARRY);
        self.simulate_rts(cpu);
        true
    }

    /// BASIN/CHRIN ($FFCF): Read next byte from open file. Returns false for non-device-8.
    fn trap_basin(&mut self, cpu: &mut Cpu6502<C64Bus>) -> bool {
        let device = Self::read_device(cpu);
        if !self.file_open || device != 8 {
            return false;
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
        true
    }

    /// Simulate an RTS: pull return address from stack, set PC = addr + 1.
    fn simulate_rts(&self, cpu: &mut Cpu6502<C64Bus>) {
        let lo = cpu.pull() as u16;
        let hi = cpu.pull() as u16;
        cpu.pc = ((hi << 8) | lo).wrapping_add(1);
    }
}

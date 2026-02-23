use crate::cpu::Cpu6502;
use crate::addressing::AddrMode;
use crate::opcodes::Mnemonic;
use emu_common::Bus;

/// Result of address resolution for an instruction.
#[derive(Debug)]
pub(crate) enum Operand {
    /// The operand is the accumulator (for ASL A, etc.)
    Accumulator,
    /// The operand is at a memory address.
    Address(u16),
    /// An immediate byte value.
    Immediate(u8),
    /// A relative branch offset.
    Relative(i8),
    /// No operand (implied).
    Implied,
}

impl<B: Bus> Cpu6502<B> {
    /// Resolve the operand address/value for the current instruction.
    /// Returns (operand, extra_cycle) where extra_cycle is true if
    /// a page boundary was crossed.
    pub(crate) fn resolve_operand(&mut self, mode: AddrMode) -> (Operand, bool) {
        match mode {
            AddrMode::Implied => (Operand::Implied, false),
            AddrMode::Accumulator => (Operand::Accumulator, false),
            AddrMode::Immediate => {
                let val = self.fetch_byte();
                (Operand::Immediate(val), false)
            }
            AddrMode::ZeroPage => {
                let addr = self.fetch_byte() as u16;
                (Operand::Address(addr), false)
            }
            AddrMode::ZeroPageX => {
                let base = self.fetch_byte();
                let addr = base.wrapping_add(self.x) as u16;
                (Operand::Address(addr), false)
            }
            AddrMode::ZeroPageY => {
                let base = self.fetch_byte();
                let addr = base.wrapping_add(self.y) as u16;
                (Operand::Address(addr), false)
            }
            AddrMode::Absolute => {
                let addr = self.fetch_word();
                (Operand::Address(addr), false)
            }
            AddrMode::AbsoluteX => {
                let base = self.fetch_word();
                let addr = base.wrapping_add(self.x as u16);
                let page_cross = (base & 0xFF00) != (addr & 0xFF00);
                (Operand::Address(addr), page_cross)
            }
            AddrMode::AbsoluteY => {
                let base = self.fetch_word();
                let addr = base.wrapping_add(self.y as u16);
                let page_cross = (base & 0xFF00) != (addr & 0xFF00);
                (Operand::Address(addr), page_cross)
            }
            AddrMode::Indirect => {
                let ptr = self.fetch_word();
                let lo = self.bus.read(ptr) as u16;
                // NMOS 6502 bug: high byte wraps within page at $xxFF
                // CMOS 65C02 fixes this — reads from next address correctly
                let hi_addr = if self.cmos_mode {
                    ptr.wrapping_add(1)
                } else {
                    (ptr & 0xFF00) | ((ptr + 1) & 0x00FF)
                };
                let hi = self.bus.read(hi_addr) as u16;
                let addr = (hi << 8) | lo;
                (Operand::Address(addr), false)
            }
            AddrMode::IndexedIndirect => {
                // (zp,X): pointer at zp+X in zero page
                let base = self.fetch_byte();
                let ptr = base.wrapping_add(self.x);
                let lo = self.bus.read(ptr as u16) as u16;
                let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                (Operand::Address(addr), false)
            }
            AddrMode::IndirectIndexed => {
                // (zp),Y: pointer at zp, then add Y
                let ptr = self.fetch_byte();
                let lo = self.bus.read(ptr as u16) as u16;
                let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16;
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let page_cross = (base & 0xFF00) != (addr & 0xFF00);
                (Operand::Address(addr), page_cross)
            }
            AddrMode::Relative => {
                let offset = self.fetch_byte() as i8;
                (Operand::Relative(offset), false)
            }
            AddrMode::ZeroPageIndirect => {
                // 65C02: (zp) — pointer at zp, no index
                let ptr = self.fetch_byte();
                let lo = self.bus.read(ptr as u16) as u16;
                let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                (Operand::Address(addr), false)
            }
        }
    }

    /// Read the value for the resolved operand.
    pub(crate) fn read_operand(&mut self, operand: &Operand) -> u8 {
        match operand {
            Operand::Accumulator => self.a,
            Operand::Address(addr) => self.bus.read(*addr),
            Operand::Immediate(val) => *val,
            Operand::Implied | Operand::Relative(_) => 0,
        }
    }

    /// Write a value to the resolved operand target.
    pub(crate) fn write_operand(&mut self, operand: &Operand, val: u8) {
        match operand {
            Operand::Accumulator => self.a = val,
            Operand::Address(addr) => self.bus.write(*addr, val),
            Operand::Implied | Operand::Immediate(_) | Operand::Relative(_) => {}
        }
    }

    /// Execute an instruction. Returns extra cycles consumed.
    pub(crate) fn execute(&mut self, mnemonic: Mnemonic, mode: AddrMode, page_penalty: bool) -> u8 {
        let (operand, page_crossed) = self.resolve_operand(mode);
        let extra = if page_penalty && page_crossed { 1 } else { 0 };

        match mnemonic {
            // --- Load/Store ---
            Mnemonic::LDA => {
                let val = self.read_operand(&operand);
                self.a = self.p.set_nz(val);
                extra
            }
            Mnemonic::LDX => {
                let val = self.read_operand(&operand);
                self.x = self.p.set_nz(val);
                extra
            }
            Mnemonic::LDY => {
                let val = self.read_operand(&operand);
                self.y = self.p.set_nz(val);
                extra
            }
            Mnemonic::STA => {
                self.write_operand(&operand, self.a);
                0
            }
            Mnemonic::STX => {
                self.write_operand(&operand, self.x);
                0
            }
            Mnemonic::STY => {
                self.write_operand(&operand, self.y);
                0
            }

            // --- Transfer ---
            Mnemonic::TAX => { self.x = self.p.set_nz(self.a); 0 }
            Mnemonic::TAY => { self.y = self.p.set_nz(self.a); 0 }
            Mnemonic::TXA => { self.a = self.p.set_nz(self.x); 0 }
            Mnemonic::TYA => { self.a = self.p.set_nz(self.y); 0 }
            Mnemonic::TSX => { self.x = self.p.set_nz(self.sp); 0 }
            Mnemonic::TXS => { self.sp = self.x; 0 }

            // --- Stack ---
            Mnemonic::PHA => { self.push(self.a); 0 }
            Mnemonic::PHP => {
                let val = self.p.to_stack(true); // B flag set on PHP
                self.push(val);
                0
            }
            Mnemonic::PLA => {
                let val = self.pull();
                self.a = self.p.set_nz(val);
                0
            }
            Mnemonic::PLP => {
                let val = self.pull();
                self.p = crate::flags::StatusFlags::from_stack(val);
                0
            }

            // --- Arithmetic ---
            Mnemonic::ADC => {
                let val = self.read_operand(&operand);
                self.adc(val);
                extra
            }
            Mnemonic::SBC => {
                let val = self.read_operand(&operand);
                self.sbc(val);
                extra
            }

            // --- Logical ---
            Mnemonic::AND => {
                let val = self.read_operand(&operand);
                self.a &= val;
                self.p.set_nz(self.a);
                extra
            }
            Mnemonic::EOR => {
                let val = self.read_operand(&operand);
                self.a ^= val;
                self.p.set_nz(self.a);
                extra
            }
            Mnemonic::ORA => {
                let val = self.read_operand(&operand);
                self.a |= val;
                self.p.set_nz(self.a);
                extra
            }

            // --- Shift/Rotate ---
            Mnemonic::ASL => {
                let val = self.read_operand(&operand);
                let result = self.asl(val);
                self.write_operand(&operand, result);
                0
            }
            Mnemonic::LSR => {
                let val = self.read_operand(&operand);
                let result = self.lsr(val);
                self.write_operand(&operand, result);
                0
            }
            Mnemonic::ROL => {
                let val = self.read_operand(&operand);
                let result = self.rol(val);
                self.write_operand(&operand, result);
                0
            }
            Mnemonic::ROR => {
                let val = self.read_operand(&operand);
                let result = self.ror(val);
                self.write_operand(&operand, result);
                0
            }

            // --- Increment/Decrement ---
            Mnemonic::INC => {
                let val = self.read_operand(&operand).wrapping_add(1);
                self.p.set_nz(val);
                self.write_operand(&operand, val);
                0
            }
            Mnemonic::DEC => {
                let val = self.read_operand(&operand).wrapping_sub(1);
                self.p.set_nz(val);
                self.write_operand(&operand, val);
                0
            }
            Mnemonic::INX => { self.x = self.x.wrapping_add(1); self.p.set_nz(self.x); 0 }
            Mnemonic::INY => { self.y = self.y.wrapping_add(1); self.p.set_nz(self.y); 0 }
            Mnemonic::DEX => { self.x = self.x.wrapping_sub(1); self.p.set_nz(self.x); 0 }
            Mnemonic::DEY => { self.y = self.y.wrapping_sub(1); self.p.set_nz(self.y); 0 }

            // --- Compare ---
            Mnemonic::CMP => {
                let val = self.read_operand(&operand);
                self.compare(self.a, val);
                extra
            }
            Mnemonic::CPX => {
                let val = self.read_operand(&operand);
                self.compare(self.x, val);
                0
            }
            Mnemonic::CPY => {
                let val = self.read_operand(&operand);
                self.compare(self.y, val);
                0
            }
            Mnemonic::BIT => {
                let val = self.read_operand(&operand);
                self.p.set(crate::flags::StatusFlags::ZERO, (self.a & val) == 0);
                // 65C02: BIT #imm only sets Z, not N/V
                if !matches!(operand, Operand::Immediate(_)) {
                    self.p.set(crate::flags::StatusFlags::NEGATIVE, val & 0x80 != 0);
                    self.p.set(crate::flags::StatusFlags::OVERFLOW, val & 0x40 != 0);
                }
                0
            }

            // --- Branch ---
            Mnemonic::BCC => self.branch(!self.p.contains(crate::flags::StatusFlags::CARRY), &operand),
            Mnemonic::BCS => self.branch(self.p.contains(crate::flags::StatusFlags::CARRY), &operand),
            Mnemonic::BEQ => self.branch(self.p.contains(crate::flags::StatusFlags::ZERO), &operand),
            Mnemonic::BMI => self.branch(self.p.contains(crate::flags::StatusFlags::NEGATIVE), &operand),
            Mnemonic::BNE => self.branch(!self.p.contains(crate::flags::StatusFlags::ZERO), &operand),
            Mnemonic::BPL => self.branch(!self.p.contains(crate::flags::StatusFlags::NEGATIVE), &operand),
            Mnemonic::BVC => self.branch(!self.p.contains(crate::flags::StatusFlags::OVERFLOW), &operand),
            Mnemonic::BVS => self.branch(self.p.contains(crate::flags::StatusFlags::OVERFLOW), &operand),

            // --- Jump ---
            Mnemonic::JMP => {
                if let Operand::Address(addr) = operand {
                    if mode == AddrMode::AbsoluteX {
                        // 65C02 JMP (abs,X): addr is the pointer, read target from it
                        let lo = self.bus.read(addr) as u16;
                        let hi = self.bus.read(addr.wrapping_add(1)) as u16;
                        self.pc = (hi << 8) | lo;
                    } else {
                        self.pc = addr;
                    }
                }
                0
            }
            Mnemonic::JSR => {
                if let Operand::Address(addr) = operand {
                    let ret = self.pc.wrapping_sub(1);
                    self.push((ret >> 8) as u8);
                    self.push(ret as u8);
                    self.pc = addr;
                }
                0
            }
            Mnemonic::RTS => {
                let lo = self.pull() as u16;
                let hi = self.pull() as u16;
                self.pc = ((hi << 8) | lo).wrapping_add(1);
                0
            }
            Mnemonic::RTI => {
                let flags = self.pull();
                self.p = crate::flags::StatusFlags::from_stack(flags);
                let lo = self.pull() as u16;
                let hi = self.pull() as u16;
                self.pc = (hi << 8) | lo;
                0
            }

            // --- Flags ---
            Mnemonic::CLC => { self.p.remove(crate::flags::StatusFlags::CARRY); 0 }
            Mnemonic::CLD => { self.p.remove(crate::flags::StatusFlags::DECIMAL); 0 }
            Mnemonic::CLI => { self.p.remove(crate::flags::StatusFlags::IRQ_DISABLE); 0 }
            Mnemonic::CLV => { self.p.remove(crate::flags::StatusFlags::OVERFLOW); 0 }
            Mnemonic::SEC => { self.p.insert(crate::flags::StatusFlags::CARRY); 0 }
            Mnemonic::SED => { self.p.insert(crate::flags::StatusFlags::DECIMAL); 0 }
            Mnemonic::SEI => { self.p.insert(crate::flags::StatusFlags::IRQ_DISABLE); 0 }

            // --- System ---
            Mnemonic::BRK => {
                self.pc = self.pc.wrapping_add(1); // BRK has a padding byte
                self.push((self.pc >> 8) as u8);
                self.push(self.pc as u8);
                self.push(self.p.to_stack(true));
                self.p.insert(crate::flags::StatusFlags::IRQ_DISABLE);
                // 65C02: clear D flag on BRK
                if self.cmos_mode {
                    self.p.remove(crate::flags::StatusFlags::DECIMAL);
                }
                let lo = self.bus.read(0xFFFE) as u16;
                let hi = self.bus.read(0xFFFF) as u16;
                self.pc = (hi << 8) | lo;
                0
            }
            Mnemonic::NOP => extra,

            // --- 65C02 extensions ---
            Mnemonic::BRA => self.branch(true, &operand),
            Mnemonic::PHX => { self.push(self.x); 0 }
            Mnemonic::PLX => { let val = self.pull(); self.x = self.p.set_nz(val); 0 }
            Mnemonic::PHY => { self.push(self.y); 0 }
            Mnemonic::PLY => { let val = self.pull(); self.y = self.p.set_nz(val); 0 }
            Mnemonic::STZ => { self.write_operand(&operand, 0); 0 }
            Mnemonic::INA => { self.a = self.a.wrapping_add(1); self.p.set_nz(self.a); 0 }
            Mnemonic::DEA => { self.a = self.a.wrapping_sub(1); self.p.set_nz(self.a); 0 }
            Mnemonic::TRB => {
                let val = self.read_operand(&operand);
                self.p.set(crate::flags::StatusFlags::ZERO, (self.a & val) == 0);
                self.write_operand(&operand, val & !self.a);
                0
            }
            Mnemonic::TSB => {
                let val = self.read_operand(&operand);
                self.p.set(crate::flags::StatusFlags::ZERO, (self.a & val) == 0);
                self.write_operand(&operand, val | self.a);
                0
            }

            Mnemonic::JAM => {
                let jam_pc = self.pc.wrapping_sub(1);
                let opcode = self.bus.peek(jam_pc);
                log::warn!("JAM (illegal opcode ${:02X}) hit at PC=${:04X}", opcode, jam_pc);
                // Dump surrounding bytes for debugging
                let start = jam_pc.saturating_sub(8);
                let mut dump = String::new();
                for addr in start..start.wrapping_add(24) {
                    if addr == jam_pc { dump.push_str("["); }
                    dump.push_str(&format!("{:02X}", self.bus.peek(addr)));
                    if addr == jam_pc { dump.push_str("]"); }
                    dump.push(' ');
                }
                log::warn!("  Memory around PC: ${:04X}: {}", start, dump.trim());
                self.jammed = true;
                0
            }
        }
    }

    // --- Helper methods ---

    fn adc(&mut self, val: u8) {
        use crate::flags::StatusFlags;
        let carry_in = if self.p.contains(StatusFlags::CARRY) { 1u16 } else { 0 };

        if self.bcd_enabled && self.p.contains(StatusFlags::DECIMAL) {
            // BCD mode
            let mut lo = (self.a & 0x0F) as u16 + (val & 0x0F) as u16 + carry_in;
            if lo > 9 { lo += 6; }
            let mut hi = (self.a >> 4) as u16 + (val >> 4) as u16 + if lo > 0x0F { 1 } else { 0 };

            let sum_binary = (self.a as u16).wrapping_add(val as u16).wrapping_add(carry_in);
            self.p.set(StatusFlags::ZERO, (sum_binary & 0xFF) == 0);
            self.p.set(StatusFlags::NEGATIVE, hi & 0x08 != 0);
            self.p.set(StatusFlags::OVERFLOW,
                ((self.a ^ val) & 0x80 == 0) && ((self.a as u16 ^ (hi << 4 | (lo & 0x0F))) & 0x80 != 0));

            if hi > 9 { hi += 6; }
            self.p.set(StatusFlags::CARRY, hi > 0x0F);
            self.a = ((hi << 4) | (lo & 0x0F)) as u8;
        } else {
            // Binary mode
            let sum = self.a as u16 + val as u16 + carry_in;
            let result = sum as u8;
            self.p.set(StatusFlags::CARRY, sum > 0xFF);
            self.p.set(StatusFlags::OVERFLOW,
                (!(self.a ^ val) & (self.a ^ result) & 0x80) != 0);
            self.a = self.p.set_nz(result);
        }
    }

    fn sbc(&mut self, val: u8) {
        use crate::flags::StatusFlags;

        if self.bcd_enabled && self.p.contains(StatusFlags::DECIMAL) {
            let carry_in = if self.p.contains(StatusFlags::CARRY) { 0u16 } else { 1 };
            let mut lo = (self.a & 0x0F) as i16 - (val & 0x0F) as i16 - carry_in as i16;
            let mut hi = (self.a >> 4) as i16 - (val >> 4) as i16;

            if lo < 0 {
                lo += 10;
                hi -= 1;
            }
            if hi < 0 {
                hi += 10;
            }

            let sum_binary = (self.a as u16).wrapping_sub(val as u16).wrapping_sub(carry_in);
            let result_binary = sum_binary as u8;
            self.p.set(StatusFlags::CARRY, sum_binary < 0x100);
            self.p.set(StatusFlags::OVERFLOW,
                ((self.a ^ val) & (self.a ^ result_binary) & 0x80) != 0);
            self.p.set_nz(result_binary);
            self.a = ((hi as u8) << 4) | (lo as u8 & 0x0F);
        } else {
            // SBC is equivalent to ADC with the value complemented
            self.adc(!val);
        }
    }

    fn asl(&mut self, val: u8) -> u8 {
        self.p.set(crate::flags::StatusFlags::CARRY, val & 0x80 != 0);
        self.p.set_nz(val << 1)
    }

    fn lsr(&mut self, val: u8) -> u8 {
        self.p.set(crate::flags::StatusFlags::CARRY, val & 0x01 != 0);
        self.p.set_nz(val >> 1)
    }

    fn rol(&mut self, val: u8) -> u8 {
        let old_carry = if self.p.contains(crate::flags::StatusFlags::CARRY) { 1 } else { 0 };
        self.p.set(crate::flags::StatusFlags::CARRY, val & 0x80 != 0);
        self.p.set_nz((val << 1) | old_carry)
    }

    fn ror(&mut self, val: u8) -> u8 {
        let old_carry = if self.p.contains(crate::flags::StatusFlags::CARRY) { 0x80 } else { 0 };
        self.p.set(crate::flags::StatusFlags::CARRY, val & 0x01 != 0);
        self.p.set_nz((val >> 1) | old_carry)
    }

    fn compare(&mut self, reg: u8, val: u8) {
        let result = reg.wrapping_sub(val);
        self.p.set(crate::flags::StatusFlags::CARRY, reg >= val);
        self.p.set_nz(result);
    }

    fn branch(&mut self, condition: bool, operand: &Operand) -> u8 {
        if let Operand::Relative(offset) = operand {
            if condition {
                let old_pc = self.pc;
                self.pc = self.pc.wrapping_add(*offset as u16);
                // +1 for taken branch, +1 more if page crossed
                if (old_pc & 0xFF00) != (self.pc & 0xFF00) {
                    2
                } else {
                    1
                }
            } else {
                0
            }
        } else {
            0
        }
    }
}

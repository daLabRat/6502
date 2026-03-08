use emu_common::Bus;
use crate::flags::StatusFlags;
use crate::opcodes::OPCODE_TABLE;

/// The 6502 CPU, generic over the Bus implementation.
/// Monomorphized per system for zero-cost memory access.
pub struct Cpu6502<B: Bus> {
    /// Program counter
    pub pc: u16,
    /// Stack pointer (offsets from $0100)
    pub sp: u8,
    /// Accumulator
    pub a: u8,
    /// X index register
    pub x: u8,
    /// Y index register
    pub y: u8,
    /// Processor status flags
    pub p: StatusFlags,
    /// The system bus
    pub bus: B,
    /// Whether BCD mode is functional (false for NES, true for others)
    pub bcd_enabled: bool,
    /// 65C02 (CMOS) mode: clears D flag on interrupts/BRK
    pub cmos_mode: bool,
    /// Total cycles executed (wraps)
    pub total_cycles: u64,
    /// CPU is halted (hit JAM instruction)
    pub jammed: bool,
}

impl<B: Bus> Cpu6502<B> {
    /// Create a new CPU with the given bus. Call reset() before executing.
    pub fn new(bus: B) -> Self {
        Self {
            pc: 0,
            sp: 0xFD,
            a: 0,
            x: 0,
            y: 0,
            p: StatusFlags::default(),
            bus,
            bcd_enabled: true,
            cmos_mode: false,
            total_cycles: 0,
            jammed: false,
        }
    }

    /// Reset the CPU: read the reset vector and initialize registers.
    /// On 65C02 (bcd_enabled=true for Apple IIe), the D flag is cleared on reset.
    pub fn reset(&mut self) {
        let lo = self.bus.read(0xFFFC) as u16;
        let hi = self.bus.read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;
        self.sp = 0xFD;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        // 65C02 clears D on reset; NMOS 6502 leaves it undefined.
        // We clear it unconditionally since NMOS programs should CLD anyway.
        self.p = StatusFlags::IRQ_DISABLE | StatusFlags::UNUSED;
        self.jammed = false;
    }

    /// Execute one instruction. Returns the number of cycles consumed.
    pub fn step(&mut self) -> u8 {
        if self.jammed {
            return 1;
        }

        // Check for NMI
        if self.bus.poll_nmi() {
            self.nmi();
            return 7;
        }

        // Check for IRQ (only if interrupts enabled)
        if !self.p.contains(StatusFlags::IRQ_DISABLE) && self.bus.poll_irq() {
            self.irq();
            return 7;
        }

        // Check SO (Set Overflow) pin — falling edge sets V flag (used by 1541 BYTE READY)
        if self.bus.poll_so() {
            self.p.insert(StatusFlags::OVERFLOW);
        }

        let opcode_byte = self.fetch_byte();
        let opcode = &OPCODE_TABLE[opcode_byte as usize];

        let extra_cycles = self.execute(opcode.mnemonic, opcode.mode, opcode.page_penalty);
        let total = opcode.cycles + extra_cycles;

        self.total_cycles += total as u64;
        self.bus.tick(total);

        total
    }

    /// Trigger a non-maskable interrupt.
    pub fn nmi(&mut self) {
        self.push((self.pc >> 8) as u8);
        self.push(self.pc as u8);
        self.push(self.p.to_stack(false));
        self.p.insert(StatusFlags::IRQ_DISABLE);
        // 65C02: clear D flag on interrupt
        if self.cmos_mode {
            self.p.remove(StatusFlags::DECIMAL);
        }
        let lo = self.bus.read(0xFFFA) as u16;
        let hi = self.bus.read(0xFFFB) as u16;
        self.pc = (hi << 8) | lo;
    }

    /// Trigger a maskable interrupt (only if I flag is clear).
    pub fn irq(&mut self) {
        if !self.p.contains(StatusFlags::IRQ_DISABLE) {
            self.push((self.pc >> 8) as u8);
            self.push(self.pc as u8);
            self.push(self.p.to_stack(false));
            self.p.insert(StatusFlags::IRQ_DISABLE);
            // 65C02: clear D flag on interrupt
            if self.cmos_mode {
                self.p.remove(StatusFlags::DECIMAL);
            }
            let lo = self.bus.read(0xFFFE) as u16;
            let hi = self.bus.read(0xFFFF) as u16;
            self.pc = (hi << 8) | lo;
        }
    }

    // --- Internal helpers ---

    /// Fetch the next byte at PC and advance PC.
    #[inline]
    pub(crate) fn fetch_byte(&mut self) -> u8 {
        let val = self.bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        val
    }

    /// Fetch the next 16-bit word at PC (little-endian) and advance PC by 2.
    #[inline]
    pub(crate) fn fetch_word(&mut self) -> u16 {
        let lo = self.bus.read(self.pc) as u16;
        let hi = self.bus.read(self.pc.wrapping_add(1)) as u16;
        self.pc = self.pc.wrapping_add(2);
        (hi << 8) | lo
    }

    /// Push a byte onto the stack.
    #[inline]
    pub fn push(&mut self, val: u8) {
        self.bus.write(0x0100 | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pull a byte from the stack.
    #[inline]
    pub fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.bus.read(0x0100 | self.sp as u16)
    }

    pub fn snapshot(&self) -> crate::snapshot::Cpu6502Snapshot {
        crate::snapshot::Cpu6502Snapshot {
            pc: self.pc,
            sp: self.sp,
            a: self.a,
            x: self.x,
            y: self.y,
            p: self.p.bits(),
            bcd_enabled: self.bcd_enabled,
            cmos_mode: self.cmos_mode,
            total_cycles: self.total_cycles,
            jammed: self.jammed,
        }
    }

    pub fn restore(&mut self, s: &crate::snapshot::Cpu6502Snapshot) {
        self.pc = s.pc;
        self.sp = s.sp;
        self.a = s.a;
        self.x = s.x;
        self.y = s.y;
        self.p = crate::flags::StatusFlags::from_bits_truncate(s.p);
        self.bcd_enabled = s.bcd_enabled;
        self.cmos_mode = s.cmos_mode;
        self.total_cycles = s.total_cycles;
        self.jammed = s.jammed;
    }
}

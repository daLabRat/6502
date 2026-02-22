use crate::addressing::AddrMode;

/// Instruction mnemonic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mnemonic {
    // Load/Store
    LDA, LDX, LDY, STA, STX, STY,
    // Transfer
    TAX, TAY, TXA, TYA, TSX, TXS,
    // Stack
    PHA, PHP, PLA, PLP,
    // Arithmetic
    ADC, SBC,
    // Logical
    AND, EOR, ORA,
    // Shift/Rotate
    ASL, LSR, ROL, ROR,
    // Increment/Decrement
    INC, INX, INY, DEC, DEX, DEY,
    // Compare
    CMP, CPX, CPY, BIT,
    // Branch
    BCC, BCS, BEQ, BMI, BNE, BPL, BVC, BVS,
    // Jump
    JMP, JSR, RTS, RTI,
    // Flag
    CLC, CLD, CLI, CLV, SEC, SED, SEI,
    // System
    BRK, NOP,
    // Illegal / undocumented - treated as JAM (halt)
    JAM,
}

/// A decoded opcode entry.
#[derive(Debug, Clone, Copy)]
pub struct Opcode {
    pub mnemonic: Mnemonic,
    pub mode: AddrMode,
    /// Base cycle count (before page-crossing or branch penalties).
    pub cycles: u8,
    /// Whether this instruction adds a cycle on page boundary crossing.
    pub page_penalty: bool,
}

impl Opcode {
    const fn new(mnemonic: Mnemonic, mode: AddrMode, cycles: u8, page_penalty: bool) -> Self {
        Self { mnemonic, mode, cycles, page_penalty }
    }
}

use Mnemonic::*;
use AddrMode::*;

/// Full 256-entry opcode table for the 6502.
/// Illegal opcodes are mapped to JAM.
pub static OPCODE_TABLE: [Opcode; 256] = {
    // Helper to reduce verbosity
    const fn op(m: Mnemonic, am: AddrMode, c: u8, pp: bool) -> Opcode {
        Opcode::new(m, am, c, pp)
    }
    const fn jam() -> Opcode {
        Opcode::new(JAM, Implied, 2, false)
    }

    [
        // 0x00-0x0F
        op(BRK, Implied, 7, false),      // 00
        op(ORA, IndexedIndirect, 6, false), // 01
        jam(),                             // 02
        jam(),                             // 03
        jam(),                             // 04 - NOP zp (illegal)
        op(ORA, ZeroPage, 3, false),       // 05
        op(ASL, ZeroPage, 5, false),       // 06
        jam(),                             // 07
        op(PHP, Implied, 3, false),        // 08
        op(ORA, Immediate, 2, false),      // 09
        op(ASL, Accumulator, 2, false),    // 0A
        jam(),                             // 0B
        jam(),                             // 0C - NOP abs (illegal)
        op(ORA, Absolute, 4, false),       // 0D
        op(ASL, Absolute, 6, false),       // 0E
        jam(),                             // 0F
        // 0x10-0x1F
        op(BPL, Relative, 2, false),       // 10
        op(ORA, IndirectIndexed, 5, true), // 11
        jam(),                             // 12
        jam(),                             // 13
        jam(),                             // 14
        op(ORA, ZeroPageX, 4, false),      // 15
        op(ASL, ZeroPageX, 6, false),      // 16
        jam(),                             // 17
        op(CLC, Implied, 2, false),        // 18
        op(ORA, AbsoluteY, 4, true),      // 19
        jam(),                             // 1A
        jam(),                             // 1B
        jam(),                             // 1C
        op(ORA, AbsoluteX, 4, true),      // 1D
        op(ASL, AbsoluteX, 7, false),     // 1E
        jam(),                             // 1F
        // 0x20-0x2F
        op(JSR, Absolute, 6, false),       // 20
        op(AND, IndexedIndirect, 6, false),// 21
        jam(),                             // 22
        jam(),                             // 23
        op(BIT, ZeroPage, 3, false),       // 24
        op(AND, ZeroPage, 3, false),       // 25
        op(ROL, ZeroPage, 5, false),       // 26
        jam(),                             // 27
        op(PLP, Implied, 4, false),        // 28
        op(AND, Immediate, 2, false),      // 29
        op(ROL, Accumulator, 2, false),    // 2A
        jam(),                             // 2B
        op(BIT, Absolute, 4, false),       // 2C
        op(AND, Absolute, 4, false),       // 2D
        op(ROL, Absolute, 6, false),       // 2E
        jam(),                             // 2F
        // 0x30-0x3F
        op(BMI, Relative, 2, false),       // 30
        op(AND, IndirectIndexed, 5, true), // 31
        jam(),                             // 32
        jam(),                             // 33
        jam(),                             // 34
        op(AND, ZeroPageX, 4, false),      // 35
        op(ROL, ZeroPageX, 6, false),      // 36
        jam(),                             // 37
        op(SEC, Implied, 2, false),        // 38
        op(AND, AbsoluteY, 4, true),      // 39
        jam(),                             // 3A
        jam(),                             // 3B
        jam(),                             // 3C
        op(AND, AbsoluteX, 4, true),      // 3D
        op(ROL, AbsoluteX, 7, false),     // 3E
        jam(),                             // 3F
        // 0x40-0x4F
        op(RTI, Implied, 6, false),        // 40
        op(EOR, IndexedIndirect, 6, false),// 41
        jam(),                             // 42
        jam(),                             // 43
        jam(),                             // 44
        op(EOR, ZeroPage, 3, false),       // 45
        op(LSR, ZeroPage, 5, false),       // 46
        jam(),                             // 47
        op(PHA, Implied, 3, false),        // 48
        op(EOR, Immediate, 2, false),      // 49
        op(LSR, Accumulator, 2, false),    // 4A
        jam(),                             // 4B
        op(JMP, Absolute, 3, false),       // 4C
        op(EOR, Absolute, 4, false),       // 4D
        op(LSR, Absolute, 6, false),       // 4E
        jam(),                             // 4F
        // 0x50-0x5F
        op(BVC, Relative, 2, false),       // 50
        op(EOR, IndirectIndexed, 5, true), // 51
        jam(),                             // 52
        jam(),                             // 53
        jam(),                             // 54
        op(EOR, ZeroPageX, 4, false),      // 55
        op(LSR, ZeroPageX, 6, false),      // 56
        jam(),                             // 57
        op(CLI, Implied, 2, false),        // 58
        op(EOR, AbsoluteY, 4, true),      // 59
        jam(),                             // 5A
        jam(),                             // 5B
        jam(),                             // 5C
        op(EOR, AbsoluteX, 4, true),      // 5D
        op(LSR, AbsoluteX, 7, false),     // 5E
        jam(),                             // 5F
        // 0x60-0x6F
        op(RTS, Implied, 6, false),        // 60
        op(ADC, IndexedIndirect, 6, false),// 61
        jam(),                             // 62
        jam(),                             // 63
        jam(),                             // 64
        op(ADC, ZeroPage, 3, false),       // 65
        op(ROR, ZeroPage, 5, false),       // 66
        jam(),                             // 67
        op(PLA, Implied, 4, false),        // 68
        op(ADC, Immediate, 2, false),      // 69
        op(ROR, Accumulator, 2, false),    // 6A
        jam(),                             // 6B
        op(JMP, Indirect, 5, false),       // 6C
        op(ADC, Absolute, 4, false),       // 6D
        op(ROR, Absolute, 6, false),       // 6E
        jam(),                             // 6F
        // 0x70-0x7F
        op(BVS, Relative, 2, false),       // 70
        op(ADC, IndirectIndexed, 5, true), // 71
        jam(),                             // 72
        jam(),                             // 73
        jam(),                             // 74
        op(ADC, ZeroPageX, 4, false),      // 75
        op(ROR, ZeroPageX, 6, false),      // 76
        jam(),                             // 77
        op(SEI, Implied, 2, false),        // 78
        op(ADC, AbsoluteY, 4, true),      // 79
        jam(),                             // 7A
        jam(),                             // 7B
        jam(),                             // 7C
        op(ADC, AbsoluteX, 4, true),      // 7D
        op(ROR, AbsoluteX, 7, false),     // 7E
        jam(),                             // 7F
        // 0x80-0x8F
        jam(),                             // 80
        op(STA, IndexedIndirect, 6, false),// 81
        jam(),                             // 82
        jam(),                             // 83
        op(STY, ZeroPage, 3, false),       // 84
        op(STA, ZeroPage, 3, false),       // 85
        op(STX, ZeroPage, 3, false),       // 86
        jam(),                             // 87
        op(DEY, Implied, 2, false),        // 88
        jam(),                             // 89
        op(TXA, Implied, 2, false),        // 8A
        jam(),                             // 8B
        op(STY, Absolute, 4, false),       // 8C
        op(STA, Absolute, 4, false),       // 8D
        op(STX, Absolute, 4, false),       // 8E
        jam(),                             // 8F
        // 0x90-0x9F
        op(BCC, Relative, 2, false),       // 90
        op(STA, IndirectIndexed, 6, false),// 91
        jam(),                             // 92
        jam(),                             // 93
        op(STY, ZeroPageX, 4, false),      // 94
        op(STA, ZeroPageX, 4, false),      // 95
        op(STX, ZeroPageY, 4, false),      // 96
        jam(),                             // 97
        op(TYA, Implied, 2, false),        // 98
        op(STA, AbsoluteY, 5, false),     // 99
        op(TXS, Implied, 2, false),        // 9A
        jam(),                             // 9B
        jam(),                             // 9C
        op(STA, AbsoluteX, 5, false),     // 9D
        jam(),                             // 9E
        jam(),                             // 9F
        // 0xA0-0xAF
        op(LDY, Immediate, 2, false),      // A0
        op(LDA, IndexedIndirect, 6, false),// A1
        op(LDX, Immediate, 2, false),      // A2
        jam(),                             // A3
        op(LDY, ZeroPage, 3, false),       // A4
        op(LDA, ZeroPage, 3, false),       // A5
        op(LDX, ZeroPage, 3, false),       // A6
        jam(),                             // A7
        op(TAY, Implied, 2, false),        // A8
        op(LDA, Immediate, 2, false),      // A9
        op(TAX, Implied, 2, false),        // AA
        jam(),                             // AB
        op(LDY, Absolute, 4, false),       // AC
        op(LDA, Absolute, 4, false),       // AD
        op(LDX, Absolute, 4, false),       // AE
        jam(),                             // AF
        // 0xB0-0xBF
        op(BCS, Relative, 2, false),       // B0
        op(LDA, IndirectIndexed, 5, true), // B1
        jam(),                             // B2
        jam(),                             // B3
        op(LDY, ZeroPageX, 4, false),      // B4
        op(LDA, ZeroPageX, 4, false),      // B5
        op(LDX, ZeroPageY, 4, false),      // B6
        jam(),                             // B7
        op(CLV, Implied, 2, false),        // B8
        op(LDA, AbsoluteY, 4, true),      // B9
        op(TSX, Implied, 2, false),        // BA
        jam(),                             // BB
        op(LDY, AbsoluteX, 4, true),      // BC
        op(LDA, AbsoluteX, 4, true),      // BD
        op(LDX, AbsoluteY, 4, true),      // BE
        jam(),                             // BF
        // 0xC0-0xCF
        op(CPY, Immediate, 2, false),      // C0
        op(CMP, IndexedIndirect, 6, false),// C1
        jam(),                             // C2
        jam(),                             // C3
        op(CPY, ZeroPage, 3, false),       // C4
        op(CMP, ZeroPage, 3, false),       // C5
        op(DEC, ZeroPage, 5, false),       // C6
        jam(),                             // C7
        op(INY, Implied, 2, false),        // C8
        op(CMP, Immediate, 2, false),      // C9
        op(DEX, Implied, 2, false),        // CA
        jam(),                             // CB
        op(CPY, Absolute, 4, false),       // CC
        op(CMP, Absolute, 4, false),       // CD
        op(DEC, Absolute, 6, false),       // CE
        jam(),                             // CF
        // 0xD0-0xDF
        op(BNE, Relative, 2, false),       // D0
        op(CMP, IndirectIndexed, 5, true), // D1
        jam(),                             // D2
        jam(),                             // D3
        jam(),                             // D4
        op(CMP, ZeroPageX, 4, false),      // D5
        op(DEC, ZeroPageX, 6, false),      // D6
        jam(),                             // D7
        op(CLD, Implied, 2, false),        // D8
        op(CMP, AbsoluteY, 4, true),      // D9
        jam(),                             // DA
        jam(),                             // DB
        jam(),                             // DC
        op(CMP, AbsoluteX, 4, true),      // DD
        op(DEC, AbsoluteX, 7, false),     // DE
        jam(),                             // DF
        // 0xE0-0xEF
        op(CPX, Immediate, 2, false),      // E0
        op(SBC, IndexedIndirect, 6, false),// E1
        jam(),                             // E2
        jam(),                             // E3
        op(CPX, ZeroPage, 3, false),       // E4
        op(SBC, ZeroPage, 3, false),       // E5
        op(INC, ZeroPage, 5, false),       // E6
        jam(),                             // E7
        op(INX, Implied, 2, false),        // E8
        op(SBC, Immediate, 2, false),      // E9
        op(NOP, Implied, 2, false),        // EA
        jam(),                             // EB
        op(CPX, Absolute, 4, false),       // EC
        op(SBC, Absolute, 4, false),       // ED
        op(INC, Absolute, 6, false),       // EE
        jam(),                             // EF
        // 0xF0-0xFF
        op(BEQ, Relative, 2, false),       // F0
        op(SBC, IndirectIndexed, 5, true), // F1
        jam(),                             // F2
        jam(),                             // F3
        jam(),                             // F4
        op(SBC, ZeroPageX, 4, false),      // F5
        op(INC, ZeroPageX, 6, false),      // F6
        jam(),                             // F7
        op(SED, Implied, 2, false),        // F8
        op(SBC, AbsoluteY, 4, true),      // F9
        jam(),                             // FA
        jam(),                             // FB
        jam(),                             // FC
        op(SBC, AbsoluteX, 4, true),      // FD
        op(INC, AbsoluteX, 7, false),     // FE
        jam(),                             // FF
    ]
};

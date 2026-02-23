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
    // 65C02 extensions
    BRA,         // Branch always
    PHX, PLX,    // Push/pull X
    PHY, PLY,    // Push/pull Y
    STZ,         // Store zero
    TRB, TSB,    // Test and reset/set bits
    INA, DEA,    // Increment/decrement accumulator
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

/// Full 256-entry opcode table for the 6502/65C02.
/// NMOS 6502 illegal opcodes are mapped to JAM.
/// 65C02 extensions are included (BRA, PHX, PLX, PHY, PLY, STZ, TSB, TRB, INA, DEA, (zp) modes).
pub static OPCODE_TABLE: [Opcode; 256] = {
    // Helper to reduce verbosity
    const fn op(m: Mnemonic, am: AddrMode, c: u8, pp: bool) -> Opcode {
        Opcode::new(m, am, c, pp)
    }
    // 65C02 multi-byte NOPs (skip operand bytes without side effects)
    const fn nop1() -> Opcode { Opcode::new(NOP, Immediate, 2, false) }      // 2-byte NOP
    const fn nop2() -> Opcode { Opcode::new(NOP, Absolute, 4, false) }       // 3-byte NOP

    [
        // 0x00-0x0F
        op(BRK, Implied, 7, false),        // 00
        op(ORA, IndexedIndirect, 6, false),// 01
        nop1(),                            // 02 - 65C02: 2-byte NOP
        nop1(),                            // 03 - 65C02: 1-byte NOP
        op(TSB, ZeroPage, 5, false),       // 04 - 65C02: TSB zp
        op(ORA, ZeroPage, 3, false),       // 05
        op(ASL, ZeroPage, 5, false),       // 06
        nop1(),                            // 07
        op(PHP, Implied, 3, false),        // 08
        op(ORA, Immediate, 2, false),      // 09
        op(ASL, Accumulator, 2, false),    // 0A
        nop1(),                            // 0B
        op(TSB, Absolute, 6, false),       // 0C - 65C02: TSB abs
        op(ORA, Absolute, 4, false),       // 0D
        op(ASL, Absolute, 6, false),       // 0E
        nop1(),                            // 0F
        // 0x10-0x1F
        op(BPL, Relative, 2, false),       // 10
        op(ORA, IndirectIndexed, 5, true), // 11
        op(ORA, ZeroPageIndirect, 5, false),// 12 - 65C02: ORA (zp)
        nop1(),                            // 13
        op(TRB, ZeroPage, 5, false),       // 14 - 65C02: TRB zp
        op(ORA, ZeroPageX, 4, false),      // 15
        op(ASL, ZeroPageX, 6, false),      // 16
        nop1(),                            // 17
        op(CLC, Implied, 2, false),        // 18
        op(ORA, AbsoluteY, 4, true),      // 19
        op(INA, Implied, 2, false),        // 1A - 65C02: INC A
        nop1(),                            // 1B
        op(TRB, Absolute, 6, false),       // 1C - 65C02: TRB abs
        op(ORA, AbsoluteX, 4, true),      // 1D
        op(ASL, AbsoluteX, 7, false),     // 1E
        nop1(),                            // 1F
        // 0x20-0x2F
        op(JSR, Absolute, 6, false),       // 20
        op(AND, IndexedIndirect, 6, false),// 21
        nop1(),                            // 22
        nop1(),                            // 23
        op(BIT, ZeroPage, 3, false),       // 24
        op(AND, ZeroPage, 3, false),       // 25
        op(ROL, ZeroPage, 5, false),       // 26
        nop1(),                            // 27
        op(PLP, Implied, 4, false),        // 28
        op(AND, Immediate, 2, false),      // 29
        op(ROL, Accumulator, 2, false),    // 2A
        nop1(),                            // 2B
        op(BIT, Absolute, 4, false),       // 2C
        op(AND, Absolute, 4, false),       // 2D
        op(ROL, Absolute, 6, false),       // 2E
        nop1(),                            // 2F
        // 0x30-0x3F
        op(BMI, Relative, 2, false),       // 30
        op(AND, IndirectIndexed, 5, true), // 31
        op(AND, ZeroPageIndirect, 5, false),// 32 - 65C02: AND (zp)
        nop1(),                            // 33
        op(BIT, ZeroPageX, 4, false),      // 34 - 65C02: BIT zp,X
        op(AND, ZeroPageX, 4, false),      // 35
        op(ROL, ZeroPageX, 6, false),      // 36
        nop1(),                            // 37
        op(SEC, Implied, 2, false),        // 38
        op(AND, AbsoluteY, 4, true),      // 39
        op(DEA, Implied, 2, false),        // 3A - 65C02: DEC A
        nop1(),                            // 3B
        op(BIT, AbsoluteX, 4, true),      // 3C - 65C02: BIT abs,X
        op(AND, AbsoluteX, 4, true),      // 3D
        op(ROL, AbsoluteX, 7, false),     // 3E
        nop1(),                            // 3F
        // 0x40-0x4F
        op(RTI, Implied, 6, false),        // 40
        op(EOR, IndexedIndirect, 6, false),// 41
        nop1(),                            // 42
        nop1(),                            // 43
        nop1(),                            // 44
        op(EOR, ZeroPage, 3, false),       // 45
        op(LSR, ZeroPage, 5, false),       // 46
        nop1(),                            // 47
        op(PHA, Implied, 3, false),        // 48
        op(EOR, Immediate, 2, false),      // 49
        op(LSR, Accumulator, 2, false),    // 4A
        nop1(),                            // 4B
        op(JMP, Absolute, 3, false),       // 4C
        op(EOR, Absolute, 4, false),       // 4D
        op(LSR, Absolute, 6, false),       // 4E
        nop1(),                            // 4F
        // 0x50-0x5F
        op(BVC, Relative, 2, false),       // 50
        op(EOR, IndirectIndexed, 5, true), // 51
        op(EOR, ZeroPageIndirect, 5, false),// 52 - 65C02: EOR (zp)
        nop1(),                            // 53
        nop1(),                            // 54
        op(EOR, ZeroPageX, 4, false),      // 55
        op(LSR, ZeroPageX, 6, false),      // 56
        nop1(),                            // 57
        op(CLI, Implied, 2, false),        // 58
        op(EOR, AbsoluteY, 4, true),      // 59
        op(PHY, Implied, 3, false),        // 5A - 65C02: PHY
        nop1(),                            // 5B
        nop2(),                            // 5C - 65C02: 3-byte NOP
        op(EOR, AbsoluteX, 4, true),      // 5D
        op(LSR, AbsoluteX, 7, false),     // 5E
        nop1(),                            // 5F
        // 0x60-0x6F
        op(RTS, Implied, 6, false),        // 60
        op(ADC, IndexedIndirect, 6, false),// 61
        nop1(),                            // 62
        nop1(),                            // 63
        op(STZ, ZeroPage, 3, false),       // 64 - 65C02: STZ zp
        op(ADC, ZeroPage, 3, false),       // 65
        op(ROR, ZeroPage, 5, false),       // 66
        nop1(),                            // 67
        op(PLA, Implied, 4, false),        // 68
        op(ADC, Immediate, 2, false),      // 69
        op(ROR, Accumulator, 2, false),    // 6A
        nop1(),                            // 6B
        op(JMP, Indirect, 5, false),       // 6C
        op(ADC, Absolute, 4, false),       // 6D
        op(ROR, Absolute, 6, false),       // 6E
        nop1(),                            // 6F
        // 0x70-0x7F
        op(BVS, Relative, 2, false),       // 70
        op(ADC, IndirectIndexed, 5, true), // 71
        op(ADC, ZeroPageIndirect, 5, false),// 72 - 65C02: ADC (zp)
        nop1(),                            // 73
        op(STZ, ZeroPageX, 4, false),      // 74 - 65C02: STZ zp,X
        op(ADC, ZeroPageX, 4, false),      // 75
        op(ROR, ZeroPageX, 6, false),      // 76
        nop1(),                            // 77
        op(SEI, Implied, 2, false),        // 78
        op(ADC, AbsoluteY, 4, true),      // 79
        op(PLY, Implied, 4, false),        // 7A - 65C02: PLY
        nop1(),                            // 7B
        op(JMP, AbsoluteX, 6, false),     // 7C - 65C02: JMP (abs,X)
        op(ADC, AbsoluteX, 4, true),      // 7D
        op(ROR, AbsoluteX, 7, false),     // 7E
        nop1(),                            // 7F
        // 0x80-0x8F
        op(BRA, Relative, 3, false),       // 80 - 65C02: BRA
        op(STA, IndexedIndirect, 6, false),// 81
        nop1(),                            // 82
        nop1(),                            // 83
        op(STY, ZeroPage, 3, false),       // 84
        op(STA, ZeroPage, 3, false),       // 85
        op(STX, ZeroPage, 3, false),       // 86
        nop1(),                            // 87
        op(DEY, Implied, 2, false),        // 88
        op(BIT, Immediate, 2, false),      // 89 - 65C02: BIT #imm
        op(TXA, Implied, 2, false),        // 8A
        nop1(),                            // 8B
        op(STY, Absolute, 4, false),       // 8C
        op(STA, Absolute, 4, false),       // 8D
        op(STX, Absolute, 4, false),       // 8E
        nop1(),                            // 8F
        // 0x90-0x9F
        op(BCC, Relative, 2, false),       // 90
        op(STA, IndirectIndexed, 6, false),// 91
        op(STA, ZeroPageIndirect, 5, false),// 92 - 65C02: STA (zp)
        nop1(),                            // 93
        op(STY, ZeroPageX, 4, false),      // 94
        op(STA, ZeroPageX, 4, false),      // 95
        op(STX, ZeroPageY, 4, false),      // 96
        nop1(),                            // 97
        op(TYA, Implied, 2, false),        // 98
        op(STA, AbsoluteY, 5, false),     // 99
        op(TXS, Implied, 2, false),        // 9A
        nop1(),                            // 9B
        op(STZ, Absolute, 4, false),       // 9C - 65C02: STZ abs
        op(STA, AbsoluteX, 5, false),     // 9D
        op(STZ, AbsoluteX, 5, false),     // 9E - 65C02: STZ abs,X
        nop1(),                            // 9F
        // 0xA0-0xAF
        op(LDY, Immediate, 2, false),      // A0
        op(LDA, IndexedIndirect, 6, false),// A1
        op(LDX, Immediate, 2, false),      // A2
        nop1(),                            // A3
        op(LDY, ZeroPage, 3, false),       // A4
        op(LDA, ZeroPage, 3, false),       // A5
        op(LDX, ZeroPage, 3, false),       // A6
        nop1(),                            // A7
        op(TAY, Implied, 2, false),        // A8
        op(LDA, Immediate, 2, false),      // A9
        op(TAX, Implied, 2, false),        // AA
        nop1(),                            // AB
        op(LDY, Absolute, 4, false),       // AC
        op(LDA, Absolute, 4, false),       // AD
        op(LDX, Absolute, 4, false),       // AE
        nop1(),                            // AF
        // 0xB0-0xBF
        op(BCS, Relative, 2, false),       // B0
        op(LDA, IndirectIndexed, 5, true), // B1
        op(LDA, ZeroPageIndirect, 5, false),// B2 - 65C02: LDA (zp)
        nop1(),                            // B3
        op(LDY, ZeroPageX, 4, false),      // B4
        op(LDA, ZeroPageX, 4, false),      // B5
        op(LDX, ZeroPageY, 4, false),      // B6
        nop1(),                            // B7
        op(CLV, Implied, 2, false),        // B8
        op(LDA, AbsoluteY, 4, true),      // B9
        op(TSX, Implied, 2, false),        // BA
        nop1(),                            // BB
        op(LDY, AbsoluteX, 4, true),      // BC
        op(LDA, AbsoluteX, 4, true),      // BD
        op(LDX, AbsoluteY, 4, true),      // BE
        nop1(),                            // BF
        // 0xC0-0xCF
        op(CPY, Immediate, 2, false),      // C0
        op(CMP, IndexedIndirect, 6, false),// C1
        nop1(),                            // C2
        nop1(),                            // C3
        op(CPY, ZeroPage, 3, false),       // C4
        op(CMP, ZeroPage, 3, false),       // C5
        op(DEC, ZeroPage, 5, false),       // C6
        nop1(),                            // C7
        op(INY, Implied, 2, false),        // C8
        op(CMP, Immediate, 2, false),      // C9
        op(DEX, Implied, 2, false),        // CA
        nop1(),                            // CB
        op(CPY, Absolute, 4, false),       // CC
        op(CMP, Absolute, 4, false),       // CD
        op(DEC, Absolute, 6, false),       // CE
        nop1(),                            // CF
        // 0xD0-0xDF
        op(BNE, Relative, 2, false),       // D0
        op(CMP, IndirectIndexed, 5, true), // D1
        op(CMP, ZeroPageIndirect, 5, false),// D2 - 65C02: CMP (zp)
        nop1(),                            // D3
        nop1(),                            // D4
        op(CMP, ZeroPageX, 4, false),      // D5
        op(DEC, ZeroPageX, 6, false),      // D6
        nop1(),                            // D7
        op(CLD, Implied, 2, false),        // D8
        op(CMP, AbsoluteY, 4, true),      // D9
        op(PHX, Implied, 3, false),        // DA - 65C02: PHX
        nop1(),                            // DB
        nop2(),                            // DC - 65C02: 3-byte NOP
        op(CMP, AbsoluteX, 4, true),      // DD
        op(DEC, AbsoluteX, 7, false),     // DE
        nop1(),                            // DF
        // 0xE0-0xEF
        op(CPX, Immediate, 2, false),      // E0
        op(SBC, IndexedIndirect, 6, false),// E1
        nop1(),                            // E2
        nop1(),                            // E3
        op(CPX, ZeroPage, 3, false),       // E4
        op(SBC, ZeroPage, 3, false),       // E5
        op(INC, ZeroPage, 5, false),       // E6
        nop1(),                            // E7
        op(INX, Implied, 2, false),        // E8
        op(SBC, Immediate, 2, false),      // E9
        op(NOP, Implied, 2, false),        // EA
        nop1(),                            // EB
        op(CPX, Absolute, 4, false),       // EC
        op(SBC, Absolute, 4, false),       // ED
        op(INC, Absolute, 6, false),       // EE
        nop1(),                            // EF
        // 0xF0-0xFF
        op(BEQ, Relative, 2, false),       // F0
        op(SBC, IndirectIndexed, 5, true), // F1
        op(SBC, ZeroPageIndirect, 5, false),// F2 - 65C02: SBC (zp)
        nop1(),                            // F3
        nop1(),                            // F4
        op(SBC, ZeroPageX, 4, false),      // F5
        op(INC, ZeroPageX, 6, false),      // F6
        nop1(),                            // F7
        op(SED, Implied, 2, false),        // F8
        op(SBC, AbsoluteY, 4, true),      // F9
        op(PLX, Implied, 4, false),        // FA - 65C02: PLX
        nop1(),                            // FB
        nop2(),                            // FC - 65C02: 3-byte NOP
        op(SBC, AbsoluteX, 4, true),      // FD
        op(INC, AbsoluteX, 7, false),     // FE
        nop1(),                            // FF
    ]
};

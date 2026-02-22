/// 6502 addressing modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrMode {
    /// No operand (e.g., CLC, RTS)
    Implied,
    /// Operates on accumulator (e.g., ASL A)
    Accumulator,
    /// 8-bit constant operand (e.g., LDA #$FF)
    Immediate,
    /// 8-bit zero-page address (e.g., LDA $44)
    ZeroPage,
    /// Zero-page + X (wraps within page) (e.g., LDA $44,X)
    ZeroPageX,
    /// Zero-page + Y (wraps within page) (e.g., LDX $44,Y)
    ZeroPageY,
    /// 16-bit absolute address (e.g., LDA $4400)
    Absolute,
    /// Absolute + X (e.g., LDA $4400,X)
    AbsoluteX,
    /// Absolute + Y (e.g., LDA $4400,Y)
    AbsoluteY,
    /// Indirect - only used by JMP (e.g., JMP ($FFFC))
    Indirect,
    /// Indexed indirect: (zp,X) - pointer is at (zp+X) in zero page
    IndexedIndirect,
    /// Indirect indexed: (zp),Y - pointer is at zp, then add Y
    IndirectIndexed,
    /// 8-bit signed offset for branch instructions
    Relative,
}

impl AddrMode {
    /// Number of operand bytes for this addressing mode.
    pub fn operand_size(self) -> u16 {
        match self {
            AddrMode::Implied | AddrMode::Accumulator => 0,
            AddrMode::Immediate
            | AddrMode::ZeroPage
            | AddrMode::ZeroPageX
            | AddrMode::ZeroPageY
            | AddrMode::IndexedIndirect
            | AddrMode::IndirectIndexed
            | AddrMode::Relative => 1,
            AddrMode::Absolute
            | AddrMode::AbsoluteX
            | AddrMode::AbsoluteY
            | AddrMode::Indirect => 2,
        }
    }
}

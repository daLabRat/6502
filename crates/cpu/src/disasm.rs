//! 6502 disassembler.

use crate::addressing::AddrMode;
use crate::opcodes::OPCODE_TABLE;

/// Disassemble one 6502 instruction at `addr`.
///
/// `peek` is a closure that reads a byte from the address space without
/// side effects. Returns `(text, next_addr)`.
pub fn disassemble_6502(peek: impl Fn(u16) -> u8, addr: u16) -> (String, u16) {
    let byte   = peek(addr);
    let op     = &OPCODE_TABLE[byte as usize];
    let size   = 1 + op.mode.operand_size();
    let next   = addr.wrapping_add(size);

    let b1 = || peek(addr.wrapping_add(1));
    let b2 = || peek(addr.wrapping_add(2));
    let abs = || (b1() as u16) | ((b2() as u16) << 8);

    let operand = match op.mode {
        AddrMode::Implied      => String::new(),
        AddrMode::Accumulator  => "A".into(),
        AddrMode::Immediate    => format!("#${:02X}", b1()),
        AddrMode::ZeroPage     => format!("${:02X}", b1()),
        AddrMode::ZeroPageX    => format!("${:02X},X", b1()),
        AddrMode::ZeroPageY    => format!("${:02X},Y", b1()),
        AddrMode::Absolute     => format!("${:04X}", abs()),
        AddrMode::AbsoluteX    => format!("${:04X},X", abs()),
        AddrMode::AbsoluteY    => format!("${:04X},Y", abs()),
        AddrMode::Indirect     => format!("(${:04X})", abs()),
        AddrMode::IndexedIndirect  => format!("(${:02X},X)", b1()),
        AddrMode::IndirectIndexed  => format!("(${:02X}),Y", b1()),
        AddrMode::ZeroPageIndirect => format!("(${:02X})", b1()),
        AddrMode::Relative => {
            let offset = b1() as i8;
            let target = (addr.wrapping_add(2) as i32 + offset as i32) as u16;
            format!("${:04X}", target)
        }
    };

    let mnemonic = format!("{:?}", op.mnemonic);
    let text = if operand.is_empty() {
        mnemonic
    } else {
        format!("{} {}", mnemonic, operand)
    };

    // Raw bytes prefix: "A9 42   " style
    let bytes: String = (0..size)
        .map(|i| format!("{:02X} ", peek(addr.wrapping_add(i))))
        .collect();

    (format!("{:<9}{}", bytes, text), next)
}

/// Walk forward from `start`, collecting disassembled lines until we have
/// at least `before` instructions prior to `pc` plus `after` after it.
/// Returns a Vec of `(address, text, is_pc)`.
pub fn disassemble_around(
    peek: impl Fn(u16) -> u8,
    pc: u16,
    before: usize,
    after: usize,
) -> Vec<(u16, String, bool)> {
    // Collect forward from start, scanning back by up to 4*before bytes
    // to find a sequence that lands on PC.
    let scan_back = (before * 3 + 3) as u16;
    let start = pc.saturating_sub(scan_back);

    // Walk forward from start, collect all (addr, text) until well past PC.
    let mut lines: Vec<(u16, String)> = Vec::new();
    let mut addr = start;
    let limit = pc.saturating_add(after as u16 * 3 + 16);
    while addr <= limit {
        let (text, next) = disassemble_6502(&peek, addr);
        lines.push((addr, text));
        if next <= addr { break; } // safety: should never happen on valid code
        addr = next;
    }

    // Find the index where addr == pc
    let pc_idx = match lines.iter().position(|(a, _)| *a == pc) {
        Some(i) => i,
        None => {
            // PC not hit exactly — fall back to showing just forward from PC
            lines.clear();
            addr = pc;
            for _ in 0..(before + 1 + after) {
                let (text, next) = disassemble_6502(&peek, addr);
                lines.push((addr, text));
                addr = next;
            }
            before.min(lines.len() - 1)
        }
    };

    // Take `before` lines before pc_idx, PC line, and `after` lines after.
    let start_idx = pc_idx.saturating_sub(before);
    let end_idx   = (pc_idx + 1 + after).min(lines.len());

    lines[start_idx..end_idx]
        .iter()
        .map(|(a, t)| (*a, t.clone(), *a == pc))
        .collect()
}

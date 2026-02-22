pub mod addressing;
pub mod cpu;
pub mod flags;
pub mod instructions;
pub mod opcodes;

#[cfg(test)]
mod tests;

pub use cpu::Cpu6502;
pub use flags::StatusFlags;

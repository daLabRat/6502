pub mod ines;
pub mod mapper;

use mapper::Mapper;

/// Represents a loaded NES cartridge.
pub struct Cartridge {
    pub mapper: Box<dyn Mapper>,
    pub mirroring: Mirroring,
}

/// Name table mirroring arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLow,
    SingleScreenHigh,
    FourScreen,
}

impl Mirroring {
    /// Convert a VRAM address ($2000-$2FFF) to a nametable RAM index (0-$7FF).
    pub fn mirror_vram_addr(self, addr: u16) -> u16 {
        let addr = addr & 0x0FFF; // $2000-$2FFF → $000-$FFF
        let table = addr / 0x400;
        let offset = addr % 0x400;
        let mapped_table = match self {
            Mirroring::Horizontal => match table {
                0 | 1 => 0,
                2 | 3 => 1,
                _ => unreachable!(),
            },
            Mirroring::Vertical => match table {
                0 | 2 => 0,
                1 | 3 => 1,
                _ => unreachable!(),
            },
            Mirroring::SingleScreenLow => 0,
            Mirroring::SingleScreenHigh => 1,
            Mirroring::FourScreen => table,
        };
        mapped_table * 0x400 + offset
    }
}

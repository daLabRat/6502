pub mod nrom;
pub mod mmc1;
pub mod uxrom;
pub mod cnrom;
pub mod mmc3;

use super::Mirroring;

/// Cartridge mapper trait - handles bank switching for both CPU and PPU address spaces.
pub trait Mapper: Send {
    /// Read from CPU address space ($4020-$FFFF typically).
    fn cpu_read(&self, addr: u16) -> u8;
    /// Write to CPU address space (may trigger bank switching).
    fn cpu_write(&mut self, addr: u16, val: u8);
    /// Read from PPU address space ($0000-$1FFF pattern tables).
    fn ppu_read(&self, addr: u16) -> u8;
    /// Write to PPU address space (CHR RAM only).
    fn ppu_write(&mut self, addr: u16, val: u8);
    /// Some mappers override the nametable mirroring.
    fn mirroring(&self) -> Mirroring;
    /// Notify mapper of PPU scanline (for MMC3 IRQ counter).
    fn scanline_tick(&mut self) {}
    /// Check if mapper is asserting IRQ.
    fn irq_pending(&self) -> bool { false }
    /// Acknowledge/clear the IRQ.
    fn irq_clear(&mut self) {}
}

/// Create a mapper by ID.
pub fn create(
    id: u8,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
) -> Result<Box<dyn Mapper>, String> {
    match id {
        0 => Ok(Box::new(nrom::Nrom::new(prg_rom, chr_rom, mirroring))),
        1 => Ok(Box::new(mmc1::Mmc1::new(prg_rom, chr_rom, mirroring))),
        2 => Ok(Box::new(uxrom::UxRom::new(prg_rom, chr_rom, mirroring))),
        3 => Ok(Box::new(cnrom::Cnrom::new(prg_rom, chr_rom, mirroring))),
        4 => Ok(Box::new(mmc3::Mmc3::new(prg_rom, chr_rom, mirroring))),
        _ => Err(format!("Unsupported mapper: {}", id)),
    }
}

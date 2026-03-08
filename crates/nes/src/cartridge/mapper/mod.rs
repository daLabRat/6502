pub mod nrom;
pub mod mmc1;
pub mod uxrom;
pub mod cnrom;
pub mod mmc3;
pub mod axrom;
pub mod bnrom;
pub mod gxrom;
pub mod color_dreams;
pub mod camerica;
pub mod mmc2;
pub mod mmc4;
pub mod fme7;

use super::Mirroring;

/// Cartridge mapper trait - handles bank switching for both CPU and PPU address spaces.
pub trait Mapper: Send {
    /// Read from CPU address space ($4020-$FFFF typically).
    fn cpu_read(&self, addr: u16) -> u8;
    /// Write to CPU address space (may trigger bank switching).
    fn cpu_write(&mut self, addr: u16, val: u8);
    /// Read from PPU address space ($0000-$1FFF pattern tables).
    /// Mutable because some mappers (MMC2/MMC4) have latch side effects on PPU reads.
    fn ppu_read(&mut self, addr: u16) -> u8;
    /// Write to PPU address space (CHR RAM only).
    fn ppu_write(&mut self, addr: u16, val: u8);
    /// Some mappers override the nametable mirroring.
    fn mirroring(&self) -> Mirroring;
    /// Called once per CPU cycle for mappers with cycle-counting IRQs (FME-7).
    fn cpu_tick(&mut self) {}
    /// Notify mapper of PPU scanline (for MMC3 IRQ counter).
    fn scanline_tick(&mut self) {}
    /// Check if mapper is asserting IRQ.
    fn irq_pending(&self) -> bool { false }
    /// Acknowledge/clear the IRQ.
    fn irq_clear(&mut self) {}
    /// Serialize mapper-specific bank register state (for save states).
    /// Default: return empty vec (ROM-only mappers like NROM have no state).
    fn mapper_state(&self) -> Vec<u8> { vec![] }
    /// Restore mapper state from bytes previously returned by `mapper_state`.
    fn restore_mapper_state(&mut self, data: &[u8]) { let _ = data; }
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
        7 => Ok(Box::new(axrom::AxRom::new(prg_rom, chr_rom, mirroring))),
        9 => Ok(Box::new(mmc2::Mmc2::new(prg_rom, chr_rom, mirroring))),
        10 => Ok(Box::new(mmc4::Mmc4::new(prg_rom, chr_rom, mirroring))),
        11 => Ok(Box::new(color_dreams::ColorDreams::new(prg_rom, chr_rom, mirroring))),
        34 => Ok(Box::new(bnrom::BnRom::new(prg_rom, chr_rom, mirroring))),
        66 => Ok(Box::new(gxrom::GxRom::new(prg_rom, chr_rom, mirroring))),
        69 => Ok(Box::new(fme7::Fme7::new(prg_rom, chr_rom, mirroring))),
        71 => Ok(Box::new(camerica::Camerica::new(prg_rom, chr_rom, mirroring))),
        _ => Err(format!("Unsupported mapper: {}", id)),
    }
}

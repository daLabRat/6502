use super::Cartridge;
use super::Mirroring;
use super::mapper::{self, Mapper};

/// Parse an iNES format ROM file.
pub fn parse(data: &[u8]) -> Result<Cartridge, String> {
    if data.len() < 16 {
        return Err("File too small for iNES header".into());
    }

    // Check magic number: "NES\x1A"
    if &data[0..4] != b"NES\x1a" {
        return Err("Not a valid iNES file (bad magic)".into());
    }

    let prg_rom_banks = data[4] as usize; // 16KB units
    let chr_rom_banks = data[5] as usize; // 8KB units
    let flags6 = data[6];
    let flags7 = data[7];

    let mapper_id = (flags7 & 0xF0) | (flags6 >> 4);

    let mirroring = if flags6 & 0x08 != 0 {
        Mirroring::FourScreen
    } else if flags6 & 0x01 != 0 {
        Mirroring::Vertical
    } else {
        Mirroring::Horizontal
    };

    let has_trainer = flags6 & 0x04 != 0;
    let header_size = 16;
    let trainer_size = if has_trainer { 512 } else { 0 };
    let prg_rom_size = prg_rom_banks * 16384;
    let chr_rom_size = chr_rom_banks * 8192;

    let prg_start = header_size + trainer_size;
    let chr_start = prg_start + prg_rom_size;

    if data.len() < chr_start + chr_rom_size {
        return Err(format!(
            "File too small: expected {} bytes, got {}",
            chr_start + chr_rom_size,
            data.len()
        ));
    }

    let prg_rom = data[prg_start..prg_start + prg_rom_size].to_vec();
    let chr_rom = if chr_rom_size > 0 {
        data[chr_start..chr_start + chr_rom_size].to_vec()
    } else {
        // CHR RAM: 8KB
        vec![0u8; 8192]
    };

    let mapper: Box<dyn Mapper> = mapper::create(mapper_id, prg_rom, chr_rom, mirroring)?;

    log::info!(
        "Loaded iNES: mapper={}, PRG={}KB, CHR={}KB, mirroring={:?}",
        mapper_id,
        prg_rom_banks * 16,
        if chr_rom_banks > 0 { chr_rom_banks * 8 } else { 8 },
        mirroring
    );

    Ok(Cartridge { mapper, mirroring })
}

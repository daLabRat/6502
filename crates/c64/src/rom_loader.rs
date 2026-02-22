/// Load a .PRG file into memory.
/// PRG format: 2-byte little-endian load address, then data.
/// Updates BASIC pointers if the PRG loads at the standard BASIC start ($0801).
pub fn load_prg(data: &[u8], ram: &mut [u8; 65536]) -> Result<u16, String> {
    if data.len() < 3 {
        return Err("PRG file too small (need at least 3 bytes)".into());
    }

    let load_addr = data[0] as u16 | ((data[1] as u16) << 8);
    let payload = &data[2..];

    let end = load_addr as usize + payload.len();
    if end > 65536 {
        return Err(format!("PRG data exceeds memory (load=${:04X}, size={})", load_addr, payload.len()));
    }

    ram[load_addr as usize..end].copy_from_slice(payload);
    log::info!("Loaded PRG: ${:04X}-${:04X} ({} bytes)", load_addr, end - 1, payload.len());

    // If loaded at the standard BASIC start address, update BASIC pointers
    // so the user can type RUN to execute the program.
    // $2D/$2E = start of variables (end of BASIC program + 1)
    // $2F/$30 = start of arrays
    // $31/$32 = end of arrays
    if load_addr == 0x0801 {
        let end_addr = end as u16;
        ram[0x2D] = end_addr as u8;
        ram[0x2E] = (end_addr >> 8) as u8;
        ram[0x2F] = end_addr as u8;
        ram[0x30] = (end_addr >> 8) as u8;
        ram[0x31] = end_addr as u8;
        ram[0x32] = (end_addr >> 8) as u8;
        log::info!("Updated BASIC pointers (end=${:04X})", end_addr);
    }

    Ok(load_addr)
}

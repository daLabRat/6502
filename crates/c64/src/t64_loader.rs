/// T64 tape image parser.
///
/// T64 is a container format: 64-byte header + 32-byte directory entries + raw data.
/// We extract the first usable PRG file and return it in standard PRG format
/// (2-byte load address + payload).

/// Extract the first usable PRG from a T64 image.
/// Returns PRG data (2-byte load address + payload) suitable for `load_prg()`.
pub fn extract_first_prg(data: &[u8]) -> Result<Vec<u8>, String> {
    // Minimum: 64-byte header + at least one 32-byte directory entry
    if data.len() < 96 {
        return Err("T64 file too small".into());
    }

    // Validate magic: starts with "C64"
    if &data[0..3] != b"C64" {
        return Err("Not a valid T64 file (bad magic)".into());
    }

    // Header fields (little-endian)
    let total_entries = u16::from_le_bytes([data[0x22], data[0x23]]) as usize;
    let used_entries = u16::from_le_bytes([data[0x24], data[0x25]]) as usize;

    if total_entries == 0 && used_entries == 0 {
        return Err("T64 has no directory entries".into());
    }

    let max_entries = total_entries.max(used_entries).min(256);

    // Scan directory entries starting at offset 0x40
    for i in 0..max_entries {
        let entry_offset = 0x40 + i * 32;
        if entry_offset + 32 > data.len() {
            break;
        }

        let entry = &data[entry_offset..entry_offset + 32];
        let entry_type = entry[0];
        let file_type = entry[1];

        // entry_type: 0=free, 1=normal tape file, 3=memory snapshot
        // file_type: 0x82 = PRG (C64 file type), but also accept 1 (common in many T64s)
        if entry_type == 0 {
            continue;
        }
        if entry_type != 1 && entry_type != 3 {
            continue;
        }

        let start_addr = u16::from_le_bytes([entry[2], entry[3]]);
        let end_addr = u16::from_le_bytes([entry[4], entry[5]]);
        let tape_offset = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;

        if tape_offset == 0 || tape_offset >= data.len() {
            continue;
        }

        // Calculate payload size: prefer offset difference, fall back to end_addr
        let payload_size = if i + 1 < max_entries {
            let next_entry_offset = 0x40 + (i + 1) * 32;
            if next_entry_offset + 12 <= data.len() {
                let next_tape_offset = u32::from_le_bytes([
                    data[next_entry_offset + 8],
                    data[next_entry_offset + 9],
                    data[next_entry_offset + 10],
                    data[next_entry_offset + 11],
                ]) as usize;
                if next_tape_offset > tape_offset && next_tape_offset <= data.len() {
                    next_tape_offset - tape_offset
                } else {
                    // Fall back to end_addr or remaining file
                    calc_size_from_addrs(start_addr, end_addr, tape_offset, data.len())
                }
            } else {
                calc_size_from_addrs(start_addr, end_addr, tape_offset, data.len())
            }
        } else {
            // Last entry: use remaining file data or end_addr
            calc_size_from_addrs(start_addr, end_addr, tape_offset, data.len())
        };

        if payload_size == 0 {
            continue;
        }

        let end = (tape_offset + payload_size).min(data.len());
        let payload = &data[tape_offset..end];

        // Build PRG: 2-byte load address (LE) + payload
        let mut prg = Vec::with_capacity(2 + payload.len());
        prg.push(start_addr as u8);
        prg.push((start_addr >> 8) as u8);
        prg.extend_from_slice(payload);

        let _ = file_type; // Acknowledge unused binding
        log::info!(
            "T64: extracted entry {} (type={}, load=${:04X}, {} bytes)",
            i, entry_type, start_addr, payload.len()
        );
        return Ok(prg);
    }

    Err("No usable PRG entries found in T64 file".into())
}

/// Calculate payload size from start/end addresses, with file-size fallback.
fn calc_size_from_addrs(start: u16, end: u16, tape_offset: usize, file_len: usize) -> usize {
    if end > start {
        (end - start) as usize
    } else {
        // end_addr is unreliable — use rest of file
        file_len.saturating_sub(tape_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_prg_from_t64() {
        // Build a minimal T64 with one PRG entry
        let mut t64 = vec![0u8; 256];

        // Magic
        t64[0..16].copy_from_slice(b"C64 tape image \0");

        // Header: 1 entry total, 1 used
        t64[0x22] = 1;
        t64[0x24] = 1;

        // Directory entry at 0x40
        t64[0x40] = 1; // entry type: normal
        t64[0x41] = 0x82; // file type: PRG
        t64[0x42] = 0x01; // start addr low: $0801
        t64[0x43] = 0x08; // start addr high
        t64[0x44] = 0x05; // end addr low: $0805
        t64[0x45] = 0x08; // end addr high
        // tape offset = 0x60 (after header+dir)
        t64[0x48] = 0x60;

        // Payload at offset 0x60
        t64[0x60] = 0xAA;
        t64[0x61] = 0xBB;
        t64[0x62] = 0xCC;
        t64[0x63] = 0xDD;

        let prg = extract_first_prg(&t64).unwrap();
        assert_eq!(prg[0], 0x01); // load addr low
        assert_eq!(prg[1], 0x08); // load addr high
        assert_eq!(&prg[2..6], &[0xAA, 0xBB, 0xCC, 0xDD]);
    }
}

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

        // entry_type: 0=free, 1=normal tape file, 3=memory snapshot
        if entry_type == 0 {
            continue;
        }
        if entry_type != 1 && entry_type != 3 {
            continue;
        }

        let start_addr = u16::from_le_bytes([entry[2], entry[3]]);
        let tape_offset = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;

        if tape_offset == 0 || tape_offset >= data.len() {
            continue;
        }

        // Calculate data size: prefer offset to next entry, fall back to rest of file.
        // We do NOT trust end_addr — it's unreliable in many T64 files.
        let data_size = if i + 1 < max_entries {
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
                    data.len() - tape_offset
                }
            } else {
                data.len() - tape_offset
            }
        } else {
            // Last (or only) entry: use all remaining file data
            data.len() - tape_offset
        };

        if data_size == 0 {
            continue;
        }

        let end = (tape_offset + data_size).min(data.len());
        let file_data = &data[tape_offset..end];

        // Most T64 files store the data as a complete PRG (2-byte load address + payload).
        // Check if the first 2 bytes match the directory's start_addr — if so, the data
        // already includes the load address and is a valid PRG as-is.
        if file_data.len() >= 2 {
            let embedded_addr = u16::from_le_bytes([file_data[0], file_data[1]]);
            if embedded_addr == start_addr {
                // Data is already a complete PRG
                log::info!(
                    "T64: extracted entry {} (load=${:04X}, {} bytes, PRG with header)",
                    i, start_addr, file_data.len() - 2
                );
                return Ok(file_data.to_vec());
            }
        }

        // Data does not include load address — prepend it
        let mut prg = Vec::with_capacity(2 + file_data.len());
        prg.push(start_addr as u8);
        prg.push((start_addr >> 8) as u8);
        prg.extend_from_slice(file_data);

        log::info!(
            "T64: extracted entry {} (load=${:04X}, {} bytes, raw payload)",
            i, start_addr, file_data.len()
        );
        return Ok(prg);
    }

    Err("No usable PRG entries found in T64 file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_prg_with_embedded_header() {
        // T64 where the data already includes the 2-byte load address
        let mut t64 = vec![0u8; 256];
        t64[0..16].copy_from_slice(b"C64 tape image \0");
        t64[0x22] = 1; // total entries
        t64[0x24] = 1; // used entries

        // Directory entry
        t64[0x40] = 1;    // entry type: normal
        t64[0x41] = 0x82; // file type: PRG
        t64[0x42] = 0x01; // start addr low: $0801
        t64[0x43] = 0x08; // start addr high
        t64[0x44] = 0x05; // end addr low (unreliable)
        t64[0x45] = 0x08;
        t64[0x48] = 0x60; // tape offset

        // Data at 0x60: PRG with embedded load address
        t64[0x60] = 0x01; // load addr low (matches start_addr)
        t64[0x61] = 0x08; // load addr high
        t64[0x62] = 0xAA; // payload
        t64[0x63] = 0xBB;

        let prg = extract_first_prg(&t64).unwrap();
        // Should return data as-is (already a valid PRG)
        assert_eq!(prg[0], 0x01);
        assert_eq!(prg[1], 0x08);
        assert_eq!(prg[2], 0xAA);
        assert_eq!(prg[3], 0xBB);
        assert_eq!(prg.len(), t64.len() - 0x60); // uses rest of file
    }

    #[test]
    fn test_extract_prg_without_header() {
        // T64 where the data is raw payload (no embedded load address)
        let mut t64 = vec![0u8; 256];
        t64[0..16].copy_from_slice(b"C64 tape image \0");
        t64[0x22] = 1;
        t64[0x24] = 1;

        t64[0x40] = 1;
        t64[0x41] = 0x82;
        t64[0x42] = 0x01; // start addr $0801
        t64[0x43] = 0x08;
        t64[0x48] = 0x60;

        // Data at 0x60: raw payload (first 2 bytes DON'T match start_addr)
        t64[0x60] = 0x0B; // BASIC next-line pointer
        t64[0x61] = 0x08;
        t64[0x62] = 0x0A;
        t64[0x63] = 0x00;

        let prg = extract_first_prg(&t64).unwrap();
        // Should prepend load address
        assert_eq!(prg[0], 0x01); // prepended load addr low
        assert_eq!(prg[1], 0x08); // prepended load addr high
        assert_eq!(prg[2], 0x0B); // original data
        assert_eq!(prg[3], 0x08);
    }
}

/// D64 disk image parser.
///
/// A standard D64 image is 174,848 bytes: 683 sectors across 35 tracks.
/// Tracks 1-17: 21 sectors, 18-24: 19 sectors, 25-30: 18 sectors, 31-35: 17 sectors.
/// Track 18 holds the BAM (sector 0) and directory (sectors 1+).
/// File data is stored as linked sector chains.

/// Standard D64 image size (35 tracks, no error bytes).
const D64_SIZE: usize = 174848;

/// Sectors per track for each track zone.
fn sectors_per_track(track: u8) -> u8 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        31..=35 => 17,
        _ => 0,
    }
}

/// Calculate the byte offset of a given track/sector in the D64 image.
fn sector_offset(track: u8, sector: u8) -> Option<usize> {
    if track < 1 || track > 35 || sector >= sectors_per_track(track) {
        return None;
    }

    let mut offset = 0usize;
    for t in 1..track {
        offset += sectors_per_track(t) as usize * 256;
    }
    offset += sector as usize * 256;
    Some(offset)
}

/// A parsed D64 disk image.
pub struct D64Image {
    data: Vec<u8>,
}

/// A directory entry from the D64 image.
pub struct DirEntry {
    pub file_type: u8,
    pub name: [u8; 16],
    pub start_track: u8,
    pub start_sector: u8,
    pub file_size_sectors: u16,
}

impl D64Image {
    /// Parse a D64 image from raw bytes.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < D64_SIZE {
            return Err(format!(
                "D64 image too small: {} bytes (expected {})",
                data.len(),
                D64_SIZE
            ));
        }

        Ok(Self {
            data: data[..D64_SIZE].to_vec(),
        })
    }

    /// Read a 256-byte sector from the image.
    fn read_sector(&self, track: u8, sector: u8) -> Option<&[u8]> {
        let offset = sector_offset(track, sector)?;
        if offset + 256 <= self.data.len() {
            Some(&self.data[offset..offset + 256])
        } else {
            None
        }
    }

    /// Read the directory entries from track 18.
    pub fn read_directory(&self) -> Vec<DirEntry> {
        let mut entries = Vec::new();

        // Directory starts at track 18, sector 1
        let mut track = 18u8;
        let mut sector = 1u8;
        let mut visited = 0;

        while track != 0 && visited < 20 {
            visited += 1;
            let data = match self.read_sector(track, sector) {
                Some(d) => d,
                None => break,
            };

            // Next directory sector
            let next_track = data[0];
            let next_sector = data[1];

            // 8 directory entries per sector, each 32 bytes
            for i in 0..8 {
                let offset = i * 32;
                // First entry in first sector starts at byte 0 but the
                // track/sector link occupies bytes 0-1 of the sector.
                // Each entry's file type is at offset+2 from the entry start.
                let entry_base = if i == 0 { 0 } else { offset };
                let ft = data[entry_base + 2];

                if ft == 0 {
                    continue; // Unused entry
                }

                let mut name = [0u8; 16];
                name.copy_from_slice(&data[entry_base + 5..entry_base + 21]);

                entries.push(DirEntry {
                    file_type: ft & 0x07,
                    name,
                    start_track: data[entry_base + 3],
                    start_sector: data[entry_base + 4],
                    file_size_sectors: u16::from_le_bytes([
                        data[entry_base + 30],
                        data[entry_base + 31],
                    ]),
                });
            }

            track = next_track;
            sector = next_sector;
        }

        entries
    }

    /// Read a file's data by following its sector chain.
    /// Returns the raw data (without the 2-byte sector links).
    pub fn read_file(&self, start_track: u8, start_sector: u8) -> Result<Vec<u8>, String> {
        let mut result = Vec::new();
        let mut track = start_track;
        let mut sector = start_sector;
        let mut visited = 0;

        while track != 0 && visited < 800 {
            visited += 1;
            let data = self.read_sector(track, sector)
                .ok_or_else(|| format!("Bad sector reference: T{}/S{}", track, sector))?;

            let next_track = data[0];
            let next_sector = data[1];

            if next_track == 0 {
                // Last sector: next_sector is the index of the last valid byte.
                // Data bytes occupy positions 2 through `used`, inclusive.
                let used = next_sector as usize;
                if used >= 2 {
                    result.extend_from_slice(&data[2..=used]);
                }
            } else {
                result.extend_from_slice(&data[2..256]);
            }

            track = next_track;
            sector = next_sector;
        }

        Ok(result)
    }

    /// Find a file by name and read its contents.
    /// The name is padded with $A0 (shifted space) in the directory.
    pub fn find_and_read_file(&self, name: &[u8]) -> Result<Vec<u8>, String> {
        let dir = self.read_directory();

        for entry in &dir {
            // Compare name (directory names are padded with $A0)
            let entry_name: Vec<u8> = entry.name.iter()
                .copied()
                .take_while(|&b| b != 0xA0)
                .collect();

            if entry_name == name {
                return self.read_file(entry.start_track, entry.start_sector);
            }
        }

        Err(format!("File not found: {:?}", String::from_utf8_lossy(name)))
    }

    /// Generate a BASIC-formatted directory listing (as loaded by LOAD"$",8).
    /// Returns data in PRG format: 2-byte load address + BASIC program.
    pub fn generate_directory_listing(&self) -> Vec<u8> {
        let load_addr: u16 = 0x0401;
        let mut result = Vec::new();
        // PRG load address header
        result.push(load_addr as u8);
        result.push((load_addr >> 8) as u8);

        let mut addr = load_addr;

        // Read BAM (track 18, sector 0) for disk name and ID
        let bam = self.read_sector(18, 0).unwrap_or(&[0; 256]);
        let disk_name = &bam[0x90..0xA0]; // 16 bytes, $A0 padded
        let disk_id = &bam[0xA2..0xA4];   // 2 bytes
        let dos_type = &bam[0xA5..0xA7];  // 2 bytes ("2A")

        // Header line: 0 "DISK NAME       " ID 2A
        let line_start = result.len();
        result.extend_from_slice(&[0x00, 0x00]); // placeholder for next-line pointer
        result.extend_from_slice(&[0x00, 0x00]); // line number = 0
        result.push(0x12); // reverse-on
        result.push(0x22); // opening quote
        for &b in disk_name {
            result.push(if b == 0xA0 { 0x20 } else { b });
        }
        result.push(0x22); // closing quote
        result.push(0x20); // space
        for &b in disk_id {
            result.push(if b == 0xA0 { 0x20 } else { b });
        }
        result.push(0x20); // space
        for &b in dos_type {
            result.push(if b == 0xA0 { 0x20 } else { b });
        }
        result.push(0x00); // end of line
        addr += (result.len() - line_start) as u16;
        // Fix up next-line pointer
        result[line_start] = addr as u8;
        result[line_start + 1] = (addr >> 8) as u8;

        // File entries
        let dir = self.read_directory();
        let type_names: [&[u8]; 8] = [
            b"DEL", b"SEQ", b"PRG", b"USR", b"REL",
            b"???", b"???", b"???",
        ];
        for entry in &dir {
            let line_start = result.len();
            result.extend_from_slice(&[0x00, 0x00]); // placeholder for next-line pointer
            let blocks = entry.file_size_sectors;
            result.push(blocks as u8);
            result.push((blocks >> 8) as u8);

            // Padding spaces before filename (right-justify block count)
            if blocks < 10 {
                result.extend_from_slice(b"   ");
            } else if blocks < 100 {
                result.extend_from_slice(b"  ");
            } else if blocks < 1000 {
                result.push(0x20);
            }

            result.push(0x22); // opening quote
            let name_len = entry.name.iter()
                .position(|&b| b == 0xA0)
                .unwrap_or(16);
            for &b in &entry.name[..name_len] {
                result.push(b);
            }
            result.push(0x22); // closing quote

            // Padding after filename
            for _ in name_len..16 {
                result.push(0x20);
            }
            result.push(0x20); // space
            let ft = (entry.file_type & 0x07) as usize;
            result.extend_from_slice(type_names[ft.min(7)]);
            result.push(0x00); // end of line

            addr += (result.len() - line_start) as u16;
            result[line_start] = addr as u8;
            result[line_start + 1] = (addr >> 8) as u8;
        }

        // "BLOCKS FREE." line
        let blocks_free = self.count_free_blocks(bam);
        let line_start = result.len();
        result.extend_from_slice(&[0x00, 0x00]); // placeholder for next-line pointer
        result.push(blocks_free as u8);
        result.push((blocks_free >> 8) as u8);
        result.extend_from_slice(b"BLOCKS FREE.");
        result.push(0x00); // end of line
        addr += (result.len() - line_start) as u16;
        result[line_start] = addr as u8;
        result[line_start + 1] = (addr >> 8) as u8;

        // End of BASIC program
        result.extend_from_slice(&[0x00, 0x00]);

        result
    }

    /// Count free blocks from BAM data.
    fn count_free_blocks(&self, bam: &[u8]) -> u16 {
        let mut free = 0u16;
        // BAM entries at bytes $04-$8F: 4 bytes per track, 35 tracks
        // First byte of each 4-byte entry = number of free sectors on that track
        for track in 0..35u8 {
            if track == 17 { continue; } // Skip track 18 (directory track)
            let offset = 0x04 + track as usize * 4;
            free += bam[offset] as u16;
        }
        free
    }

    /// Load the first PRG file from the directory.
    /// Returns PRG data (2-byte load address + payload).
    pub fn load_first_prg(&self) -> Result<Vec<u8>, String> {
        let dir = self.read_directory();

        for entry in &dir {
            // File type 2 = PRG
            if entry.file_type == 2 && entry.start_track != 0 {
                let data = self.read_file(entry.start_track, entry.start_sector)?;
                if data.len() >= 2 {
                    return Ok(data);
                }
            }
        }

        Err("No PRG files found in D64 directory".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sector_offset() {
        // Track 1, sector 0 should be at offset 0
        assert_eq!(sector_offset(1, 0), Some(0));
        // Track 1, sector 1 should be at offset 256
        assert_eq!(sector_offset(1, 1), Some(256));
        // Track 2, sector 0 should be at 21*256
        assert_eq!(sector_offset(2, 0), Some(21 * 256));
        // Invalid track
        assert_eq!(sector_offset(0, 0), None);
        assert_eq!(sector_offset(36, 0), None);
    }

    #[test]
    fn test_sectors_per_track() {
        assert_eq!(sectors_per_track(1), 21);
        assert_eq!(sectors_per_track(17), 21);
        assert_eq!(sectors_per_track(18), 19);
        assert_eq!(sectors_per_track(24), 19);
        assert_eq!(sectors_per_track(25), 18);
        assert_eq!(sectors_per_track(30), 18);
        assert_eq!(sectors_per_track(31), 17);
        assert_eq!(sectors_per_track(35), 17);
    }
}

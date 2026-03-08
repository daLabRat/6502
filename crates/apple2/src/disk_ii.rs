/// Apple II Disk II controller.
///
/// Emulates the Disk II interface card in slot 6.
/// I/O at $C0E0-$C0EF, boot ROM at $C600-$C6FF.
/// Supports reading .dsk (DOS-order) and .po (ProDOS-order) disk images.

/// Number of bytes in a nibblized track.
const TRACK_NIBBLE_SIZE: usize = 6656;

/// Number of tracks on a standard 5.25" disk.
const NUM_TRACKS: usize = 35;

/// DOS 3.3 sector interleave: physical sector → logical sector in .dsk file.
/// Derived by inverting the logical→physical table [0,13,11,9,7,5,3,1,14,12,10,8,6,4,2,15].
static DOS33_PHYSICAL_TO_LOGICAL: [usize; 16] = [
    0, 7, 14, 6, 13, 5, 12, 4, 11, 3, 10, 2, 9, 1, 8, 15,
];

/// ProDOS sector interleave: physical sector → logical sector in .po file.
/// ProDOS logical→physical: [0,2,4,6,8,10,12,14,1,3,5,7,9,11,13,15].
/// Inverted: physical P → ProDOS logical sector.
static PRODOS_PHYSICAL_TO_LOGICAL: [usize; 16] = [
    0, 8, 1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15,
];

/// 6-and-2 write translate table: maps 6-bit values to valid disk nibbles.
static WRITE_TABLE: [u8; 64] = [
    0x96, 0x97, 0x9A, 0x9B, 0x9D, 0x9E, 0x9F, 0xA6,
    0xA7, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB2, 0xB3,
    0xB4, 0xB5, 0xB6, 0xB7, 0xB9, 0xBA, 0xBB, 0xBC,
    0xBD, 0xBE, 0xBF, 0xCB, 0xCD, 0xCE, 0xCF, 0xD3,
    0xD6, 0xD7, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE,
    0xDF, 0xE5, 0xE6, 0xE7, 0xE9, 0xEA, 0xEB, 0xEC,
    0xED, 0xEE, 0xEF, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6,
    0xF7, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

/// Disk II controller state.
pub struct DiskII {
    /// Pre-nibblized track data (35 tracks).
    nibble_data: Vec<[u8; TRACK_NIBBLE_SIZE]>,
    /// Mutable nibble track data (written over nibble_data on write).
    /// Initialised as a clone of nibble_data when a disk is loaded.
    write_data: Vec<[u8; TRACK_NIBBLE_SIZE]>,
    /// Which tracks have been written since last save.
    dirty_tracks: Vec<bool>,
    /// Write latch: byte staged by STA $C0ED, written on next step tick.
    write_latch: u8,
    write_latch_ready: bool,
    /// Current track (0-34).
    pub(crate) current_track: u8,
    /// Current byte position within the track's nibble stream.
    pub(crate) byte_position: usize,
    /// Motor on/off.
    pub(crate) motor_on: bool,
    /// Phase magnet states (4 phases for head stepping).
    phase_states: [bool; 4],
    /// Phase position (0-68 half-tracks, current_track = phase_position / 2).
    phase_position: u8,
    /// Data latch (last byte read).
    data_latch: u8,
    /// Write mode flag.
    pub(crate) write_mode: bool,
    /// Whether a disk is loaded.
    pub(crate) disk_loaded: bool,
    /// Boot ROM (256 bytes, P5 PROM at $C600-$C6FF).
    boot_rom: [u8; 256],
    /// Cycle accumulator for nibble timing (~32 CPU cycles per nibble byte).
    cycle_accumulator: u32,
}

impl DiskII {
    pub fn new() -> Self {
        Self {
            nibble_data: Vec::new(),
            write_data: Vec::new(),
            dirty_tracks: Vec::new(),
            write_latch: 0,
            write_latch_ready: false,
            current_track: 0,
            byte_position: 0,
            motor_on: false,
            phase_states: [false; 4],
            phase_position: 0,
            data_latch: 0,
            write_mode: false,
            disk_loaded: false,
            boot_rom: [0; 256],
            cycle_accumulator: 0,
        }
    }

    /// Load the P5 boot ROM (256 bytes).
    pub fn load_boot_rom(&mut self, data: &[u8]) {
        let len = data.len().min(256);
        self.boot_rom[..len].copy_from_slice(&data[..len]);
    }

    /// Load a .dsk/.po image (143360 bytes = 35 tracks × 16 sectors × 256 bytes).
    /// Auto-detects DOS 3.3 vs ProDOS sector ordering.
    pub fn load_dsk(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() != 143360 {
            return Err(format!(
                "Invalid .dsk image size: {} (expected 143360)",
                data.len()
            ));
        }

        let interleave = detect_sector_order(data);
        let interleave_name = match interleave {
            SectorOrder::Dos33 => "DOS 3.3",
            SectorOrder::ProDos => "ProDOS",
        };
        let table = match interleave {
            SectorOrder::Dos33 => &DOS33_PHYSICAL_TO_LOGICAL,
            SectorOrder::ProDos => &PRODOS_PHYSICAL_TO_LOGICAL,
        };

        self.nibble_data.clear();
        for track in 0..NUM_TRACKS {
            let track_data = &data[track * 4096..(track + 1) * 4096];
            self.nibble_data.push(nibblize_track_with_interleave(
                track as u8, track_data, table,
            ));
        }

        self.write_data = self.nibble_data.clone();
        self.dirty_tracks = vec![false; NUM_TRACKS];
        self.disk_loaded = true;
        self.current_track = 0;
        self.byte_position = 0;
        self.phase_position = 0;
        log::info!("Disk II: loaded {} order image ({} tracks)", interleave_name, NUM_TRACKS);
        Ok(())
    }

    /// Read from boot ROM space ($C600-$C6FF).
    pub fn read_rom(&self, addr: u16) -> u8 {
        self.boot_rom[(addr & 0xFF) as usize]
    }

    /// Handle I/O read ($C0E0-$C0EF).
    pub fn io_read(&mut self, addr: u16) -> u8 {
        let switch = (addr & 0x0F) as u8;
        match switch {
            0x0..=0x7 => {
                self.handle_phase(switch);
                0
            }
            0x8 => { self.motor_on = false; 0 }
            0x9 => { self.motor_on = true; 0 }
            0xA | 0xB => 0, // Drive select (only drive 1 supported)
            0xC => {
                // Q6L: Read data latch, then clear bit 7 to signal "consumed".
                // The next byte arriving from step() will restore bit 7.
                let val = self.data_latch;
                self.data_latch &= 0x7F;
                val
            }
            0xD => {
                // Q6H in write mode: latch the current data_latch as write byte
                if self.write_mode {
                    self.write_latch = self.data_latch;
                    self.write_latch_ready = true;
                }
                0
            }
            0xE => {
                // Q7L: Set read mode
                self.write_mode = false;
                // Return data latch when switching to read mode
                self.data_latch
            }
            0xF => {
                // Q7H: Set write mode
                self.write_mode = true;
                0
            }
            _ => 0,
        }
    }

    /// Handle I/O write ($C0E0-$C0EF).
    pub fn io_write(&mut self, addr: u16, val: u8) {
        let switch = (addr & 0x0F) as u8;
        match switch {
            0x0..=0x7 => self.handle_phase(switch),
            0x8 => self.motor_on = false,
            0x9 => self.motor_on = true,
            0xA | 0xB => {} // Drive select
            0xC => { /* Q6L */ }
            0xD => {
                // Q6H in write mode: load write latch with byte from CPU
                if self.write_mode {
                    self.write_latch = val;
                    self.write_latch_ready = true;
                }
            }
            0xE => self.write_mode = false,
            0xF => self.write_mode = true,
            _ => {}
        }
    }

    /// Step the disk controller: advance the byte position based on CPU cycles.
    /// Loads new nibble bytes into the data latch as they arrive.
    ///
    /// The real Disk II produces a byte every ~32 CPU cycles (4 microseconds
    /// at 1.023 MHz). The RWTS polling loop (LDA $C0EC / BPL) takes ~7 cycles
    /// per poll, so the CPU polls 4-5 times per byte.
    pub fn step(&mut self, cycles: u8) {
        if !self.motor_on || !self.disk_loaded {
            return;
        }

        self.cycle_accumulator += cycles as u32;
        while self.cycle_accumulator >= 32 {
            self.cycle_accumulator -= 32;
            self.byte_position = (self.byte_position + 1) % TRACK_NIBBLE_SIZE;

            if self.write_mode {
                // Flush the staged write latch byte into the track buffer
                if self.write_latch_ready {
                    let track = self.current_track as usize;
                    if track < self.write_data.len() {
                        self.write_data[track][self.byte_position] = self.write_latch;
                        self.dirty_tracks[track] = true;
                    }
                    self.write_latch_ready = false;
                }
            } else {
                // Read: load nibble from write_data (reflects any writes done this session)
                let track = self.current_track as usize;
                if let Some(track_data) = self.write_data.get(track) {
                    self.data_latch = track_data[self.byte_position];
                }
            }
        }
    }

    /// Handle phase magnet activation/deactivation for head stepping.
    ///
    /// The Disk II stepper motor has 4 phases (0-3) in a 4-half-track cycle:
    ///   Phase 0 → half-tracks 0, 4, 8, 12, ...
    ///   Phase 1 → half-tracks 1, 5, 9, 13, ...
    ///   Phase 2 → half-tracks 2, 6, 10, 14, ...
    ///   Phase 3 → half-tracks 3, 7, 11, 15, ...
    ///
    /// Each adjacent phase is 1 half-track apart. The RWTS seek code steps
    /// through phases sequentially (0→1→2→3→0 outward, 0→3→2→1→0 inward),
    /// moving 1 half-track per step. Two full-track seeks = 4 phase steps.
    fn handle_phase(&mut self, switch: u8) {
        let phase = (switch >> 1) as usize;
        let on = switch & 1 != 0;
        self.phase_states[phase] = on;

        // Determine target half-track based on all active phase magnets
        let mut active_phases = [0usize; 4];
        let mut active_count = 0;
        for p in 0..4 {
            if self.phase_states[p] {
                active_phases[active_count] = p;
                active_count += 1;
            }
        }

        if active_count == 0 || active_count > 2 {
            return;
        }

        let cur = self.phase_position as i32;

        // Find the nearest half-track position matching the active phase(s)
        // In a 4-ht cycle, phase P is at half-tracks P, P+4, P+8, ...
        let target = if active_count == 1 {
            let p = active_phases[0] as i32;
            // Find nearest ht where ht % 4 == p
            let cur_mod = ((cur % 4) + 4) % 4;
            let diff = ((p - cur_mod) + 4) % 4;
            // diff is 0,1,2,3 — pick shortest path (0,1,2 forward; 3 = -1 backward)
            if diff <= 2 { cur + diff } else { cur + diff - 4 }
        } else {
            // Two phases active — find midpoint
            let p0 = active_phases[0] as i32;
            let p1 = active_phases[1] as i32;
            let phase_diff = ((p1 - p0) + 4) % 4;
            if phase_diff == 2 {
                return; // Opposite phases — unstable, no movement
            }
            // Adjacent phases: midpoint is at a half-half-track position.
            // In practice the head moves toward the closer of the two phases.
            // Find nearest position for each phase and pick the closer one.
            let t0 = {
                let cur_mod = ((cur % 4) + 4) % 4;
                let diff = ((p0 - cur_mod) + 4) % 4;
                if diff <= 2 { cur + diff } else { cur + diff - 4 }
            };
            let t1 = {
                let cur_mod = ((cur % 4) + 4) % 4;
                let diff = ((p1 - cur_mod) + 4) % 4;
                if diff <= 2 { cur + diff } else { cur + diff - 4 }
            };
            // Move toward the closer target
            if (t0 - cur).unsigned_abs() <= (t1 - cur).unsigned_abs() { t0 } else { t1 }
        };

        let delta = target - cur;

        // Limit movement to ±1 half-track per phase event
        let movement = delta.clamp(-1, 1);
        if movement == 0 {
            return;
        }

        let new_pos = (cur + movement).clamp(0, 68) as u8;
        self.phase_position = new_pos;

        let new_track = self.phase_position / 2;
        if new_track != self.current_track {
            if new_track > 1 || self.current_track > 1 {
                log::info!("Disk II: seek track {} → {} (ht {})",
                    self.current_track, new_track, self.phase_position);
            }
            self.current_track = new_track;
            self.byte_position = 0;
        }
    }

    /// Return true if any track has been written since the last `clear_dirty()`.
    pub fn is_dirty(&self) -> bool {
        self.dirty_tracks.iter().any(|&d| d)
    }

    /// Clear all dirty flags (call after saving the disk image).
    pub fn clear_dirty(&mut self) {
        for d in &mut self.dirty_tracks {
            *d = false;
        }
    }

    /// Recover the modified disk image by denibblizing all dirty tracks.
    /// Returns `None` if no tracks have been written since load or last save.
    pub fn get_modified_dsk(&self) -> Option<Vec<u8>> {
        if !self.is_dirty() {
            return None;
        }

        let mut dsk = vec![0u8; 143360];
        for track in 0..NUM_TRACKS {
            if track >= self.write_data.len() { break; }
            let nibbles = &self.write_data[track];
            let sectors = denibblize_track(nibbles);
            for physical in 0..16 {
                let logical = DOS33_PHYSICAL_TO_LOGICAL[physical];
                let offset = track * 4096 + logical * 256;
                dsk[offset..offset + 256].copy_from_slice(&sectors[physical]);
            }
        }

        Some(dsk)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_read_table() -> [u8; 256] {
        let mut table = [0xFF_u8; 256];
        for (i, &w) in WRITE_TABLE.iter().enumerate() {
            table[w as usize] = i as u8;
        }
        table
    }

    /// Decode 343 nibble bytes back to 256 data bytes (reverse of encode_6and2).
    fn decode_6and2(nibble_bytes: &[u8]) -> [u8; 256] {
        let read_table = build_read_table();

        // Translate nibble bytes to 6-bit values
        let mut raw = [0u8; 343];
        for i in 0..343 {
            let val = read_table[nibble_bytes[i] as usize];
            assert_ne!(val, 0xFF, "Invalid nibble byte ${:02X} at pos {}", nibble_bytes[i], i);
            raw[i] = val;
        }

        // XOR unchain: first 86 values are aux (written in order 85,84,...,0)
        let mut prev = 0u8;
        let mut aux = [0u8; 86];
        for i in 0..86 {
            aux[85 - i] = raw[i] ^ prev;
            prev = aux[85 - i];
        }

        let mut primary = [0u8; 256];
        for i in 0..256 {
            primary[i] = raw[86 + i] ^ prev;
            prev = primary[i];
        }

        // Checksum should be 0
        let checksum = raw[342] ^ prev;
        assert_eq!(checksum, 0, "Checksum mismatch");

        // Recombine: reverse the bit swap on aux low-2 bits
        let mut data = [0u8; 256];
        for i in 0..256 {
            let aux_idx = 85 - (i % 86);
            let shift = (i / 86) * 2;
            let low2_swapped = (aux[aux_idx] >> shift) & 3;
            // Reverse bit 0 ↔ bit 1 (undo the encoding swap)
            let low2 = ((low2_swapped & 1) << 1) | ((low2_swapped >> 1) & 1);
            data[i] = (primary[i] << 2) | low2;
        }

        data
    }

    /// Find the Nth data field prologue (D5 AA AD) in the nibble stream.
    fn find_data_prologue(nibbles: &[u8], n: usize) -> usize {
        let mut count = 0;
        let mut pos = 0;
        while pos + 2 < nibbles.len() {
            if nibbles[pos] == 0xD5 && nibbles[pos + 1] == 0xAA && nibbles[pos + 2] == 0xAD {
                if count == n {
                    return pos + 3;
                }
                count += 1;
            }
            pos += 1;
        }
        panic!("Data prologue #{} not found", n);
    }

    #[test]
    fn test_6and2_round_trip_sequential() {
        let mut sector_data = [0u8; 256];
        for i in 0..256 { sector_data[i] = i as u8; }

        let mut track_data = [0u8; 4096];
        track_data[..256].copy_from_slice(&sector_data);

        let nibbles = nibblize_track(0, &track_data);

        // Physical sector 0 data starts after the first D5 AA AD
        let data_start = find_data_prologue(&nibbles, 0);
        let decoded = decode_6and2(&nibbles[data_start..data_start + 343]);

        assert_eq!(&decoded[..], &sector_data[..], "6-and-2 round trip failed for sequential pattern");
    }

    #[test]
    fn test_6and2_round_trip_all_ff() {
        let sector_data = [0xFF_u8; 256];

        let track_data = {
            let mut t = [0u8; 4096];
            t[..256].copy_from_slice(&sector_data);
            t
        };

        let nibbles = nibblize_track(0, &track_data);

        let data_start = find_data_prologue(&nibbles, 0);
        let decoded = decode_6and2(&nibbles[data_start..data_start + 343]);

        assert_eq!(&decoded[..], &sector_data[..], "6-and-2 round trip failed for 0xFF pattern");
    }

    #[test]
    fn test_6and2_round_trip_all_zeros() {
        let sector_data = [0x00_u8; 256];

        let track_data = [0u8; 4096];

        let nibbles = nibblize_track(0, &track_data);

        let data_start = find_data_prologue(&nibbles, 0);
        let decoded = decode_6and2(&nibbles[data_start..data_start + 343]);

        assert_eq!(&decoded[..], &sector_data[..], "6-and-2 round trip failed for zero pattern");
    }

    #[test]
    fn test_4and4_round_trip() {
        // Verify 4-and-4 encoding for address field values
        for val in 0..=255u8 {
            let mut nibbles = [0u8; TRACK_NIBBLE_SIZE];
            let mut pos = 0;
            encode_4and4(&mut nibbles, &mut pos, val);
            // Decode: first byte has odd bits, second has even bits
            let decoded = (nibbles[0] << 1) | 1;
            let decoded = decoded & nibbles[1];
            assert_eq!(decoded, val, "4-and-4 round trip failed for {}", val);
        }
    }

    /// Simulate the boot ROM's read sequence: poll data latch, find sector 0, decode data.
    #[test]
    fn test_boot_rom_read_simulation() {
        // Create a disk with known sector 0 data
        let mut dsk = vec![0u8; 143360];
        // Fill sector 0 with a known pattern (first 256 bytes of .dsk file)
        for i in 0..256 {
            dsk[i] = i as u8;
        }

        let mut disk = DiskII::new();
        disk.load_dsk(&dsk).unwrap();
        disk.motor_on = true;
        disk.write_mode = false;

        // Simulate polling the data latch like the boot ROM does
        fn read_byte(disk: &mut DiskII) -> u8 {
            for _ in 0..1000 {
                let val = disk.io_read(0xC0EC);
                disk.step(4); // LDA cycles
                if val & 0x80 != 0 {
                    return val;
                }
                disk.step(3); // BPL cycles
            }
            panic!("Timeout waiting for byte");
        }

        // Find address field: D5 AA 96
        for _ in 0..10000 {
            let b = read_byte(&mut disk);
            if b != 0xD5 { continue; }
            let b = read_byte(&mut disk);
            if b != 0xAA { continue; }
            let b = read_byte(&mut disk);
            if b != 0x96 { continue; }

            // Read address field (4-and-4 encoded: volume, track, sector, checksum)
            let vol_odd = read_byte(&mut disk);
            let vol_even = read_byte(&mut disk);
            let volume = (vol_odd << 1 | 1) & vol_even;

            let trk_odd = read_byte(&mut disk);
            let trk_even = read_byte(&mut disk);
            let track_num = (trk_odd << 1 | 1) & trk_even;

            let sec_odd = read_byte(&mut disk);
            let sec_even = read_byte(&mut disk);
            let sector_num = (sec_odd << 1 | 1) & sec_even;

            let _chk_odd = read_byte(&mut disk);
            let _chk_even = read_byte(&mut disk);

            if sector_num == 0 && track_num == 0 {
                // Now find data field: D5 AA AD
                for _ in 0..200 {
                    let b = read_byte(&mut disk);
                    if b != 0xD5 { continue; }
                    let b = read_byte(&mut disk);
                    if b != 0xAA { continue; }
                    let b = read_byte(&mut disk);
                    if b != 0xAD { continue; }

                    // Read 343 data nibbles
                    let read_table = build_read_table();
                    let mut raw = [0u8; 343];
                    for j in 0..343 {
                        let nibble = read_byte(&mut disk);
                        raw[j] = read_table[nibble as usize];
                        assert_ne!(raw[j], 0xFF,
                            "Invalid nibble ${:02X} at data byte {} (volume={}, track={}, sector={})",
                            nibble, j, volume, track_num, sector_num);
                    }

                    // XOR unchain
                    let mut prev = 0u8;
                    let mut aux = [0u8; 86];
                    for i in 0..86 {
                        aux[85 - i] = raw[i] ^ prev;
                        prev = aux[85 - i];
                    }
                    let mut primary = [0u8; 256];
                    for i in 0..256 {
                        primary[i] = raw[86 + i] ^ prev;
                        prev = primary[i];
                    }
                    let checksum = raw[342] ^ prev;
                    assert_eq!(checksum, 0, "Data field checksum mismatch");

                    // Recombine
                    let mut decoded = [0u8; 256];
                    for i in 0..256 {
                        let aux_idx = 85 - (i % 86);
                        let shift = (i / 86) * 2;
                        let low2_swapped = (aux[aux_idx] >> shift) & 3;
                        let low2 = ((low2_swapped & 1) << 1) | ((low2_swapped >> 1) & 1);
                        decoded[i] = (primary[i] << 2) | low2;
                    }

                    // Verify: should match our input (0, 1, 2, ..., 255)
                    for i in 0..256 {
                        assert_eq!(decoded[i], i as u8,
                            "Data mismatch at byte {}: got ${:02X}, expected ${:02X}",
                            i, decoded[i], i as u8);
                    }
                    return; // Success!
                }
                panic!("Data field prologue not found after address field");
            }
        }
        panic!("Sector 0 address field not found");
    }

    #[test]
    fn test_write_round_trip() {
        // Build a 143360-byte DSK with known sector 0 data
        let mut dsk = vec![0u8; 143360];
        for i in 0..256 {
            dsk[i] = i as u8; // Track 0, logical sector 0 = 0x00..0xFF
        }

        let mut disk = DiskII::new();
        disk.load_dsk(&dsk).unwrap();

        // Directly inject a modified nibblized track 0 with sector 0 = all 0xAB
        let new_sector = [0xABu8; 256];
        let mut new_track_data = [0u8; 4096];
        new_track_data[..256].copy_from_slice(&new_sector);
        let nibblized = nibblize_track_with_interleave(0, &new_track_data, &DOS33_PHYSICAL_TO_LOGICAL);
        disk.write_data[0] = nibblized;
        disk.dirty_tracks[0] = true;

        let modified = disk.get_modified_dsk().unwrap();
        assert_eq!(&modified[..256], &[0xABu8; 256],
            "Round-trip write should recover the modified sector 0 data");
    }

    #[test]
    fn test_interleave_is_invertible() {
        // Verify the interleave table maps 16 unique values
        let mut seen = [false; 16];
        for &v in DOS33_PHYSICAL_TO_LOGICAL.iter() {
            assert!(v < 16, "Interleave value out of range: {}", v);
            assert!(!seen[v], "Duplicate interleave value: {}", v);
            seen[v] = true;
        }
    }
}

/// Inverse of WRITE_TABLE: maps disk nibble byte → 6-bit value (0xFF = invalid).
const fn make_read_table() -> [u8; 256] {
    let mut t = [0xFFu8; 256];
    let mut i = 0usize;
    while i < 64 {
        t[WRITE_TABLE[i] as usize] = i as u8;
        i += 1;
    }
    t
}
static READ_TABLE: [u8; 256] = make_read_table();

/// Decode a 4-and-4 encoded pair of bytes back to a single byte.
fn decode_4and4(a: u8, b: u8) -> u8 {
    ((a & 0x55) << 1) | (b & 0x55)
}

/// Find a byte sequence in a circular buffer starting at `start`.
/// Returns the index of the first byte of the sequence, or `None`.
fn find_sequence(data: &[u8], start: usize, seq: &[u8]) -> Option<usize> {
    let len = data.len();
    for i in 0..len {
        let pos = (start + i) % len;
        if (0..seq.len()).all(|j| data[(pos + j) % len] == seq[j]) {
            return Some(pos);
        }
    }
    None
}

/// Decode 343 nibble bytes back to 256 data bytes (reverse of encode_6and2).
fn decode_6and2_nibbles(nibble_bytes: &[u8]) -> [u8; 256] {
    // Translate nibble bytes to 6-bit values
    let mut raw = [0u8; 343];
    for i in 0..343 {
        let val = READ_TABLE[nibble_bytes[i] as usize];
        raw[i] = if val == 0xFF { 0 } else { val };
    }

    // XOR unchain: first 86 values are aux (stored in reverse order 85..0)
    let mut prev = 0u8;
    let mut aux = [0u8; 86];
    for i in 0..86 {
        aux[85 - i] = raw[i] ^ prev;
        prev = aux[85 - i];
    }

    let mut primary = [0u8; 256];
    for i in 0..256 {
        primary[i] = raw[86 + i] ^ prev;
        prev = primary[i];
    }

    // Recombine: reverse the bit swap on aux low-2 bits
    let mut data = [0u8; 256];
    for i in 0..256 {
        let aux_idx = 85 - (i % 86);
        let shift = (i / 86) * 2;
        let low2_swapped = (aux[aux_idx] >> shift) & 3;
        let low2 = ((low2_swapped & 1) << 1) | ((low2_swapped >> 1) & 1);
        data[i] = (primary[i] << 2) | low2;
    }

    data
}

/// Scan a nibble track for up to 16 sectors and decode each via 6-and-2.
/// Returns an array of 16 raw 256-byte sectors (indexed by physical sector #).
fn denibblize_track(nibbles: &[u8; TRACK_NIBBLE_SIZE]) -> [[u8; 256]; 16] {
    let mut sectors = [[0u8; 256]; 16];
    let len = nibbles.len();
    let mut pos = 0;
    let mut found = 0;

    while found < 16 {
        // Find address field prologue: D5 AA 96
        let start = match find_sequence(nibbles, pos, &[0xD5, 0xAA, 0x96]) {
            Some(s) => s,
            None => break,
        };
        pos = (start + 3) % len;

        // Read 8 address field bytes (4 pairs of 4-and-4 encoded: vol, track, sector, checksum)
        let mut af = [0u8; 8];
        for b in &mut af {
            *b = nibbles[pos % len];
            pos = (pos + 1) % len;
        }
        let sector = decode_4and4(af[4], af[5]);
        if sector >= 16 {
            continue;
        }
        // Skip epilogue DE AA EB
        pos = (pos + 3) % len;

        // Find data field prologue: D5 AA AD
        let dstart = match find_sequence(nibbles, pos, &[0xD5, 0xAA, 0xAD]) {
            Some(s) => s,
            None => break,
        };
        pos = (dstart + 3) % len;

        // Read 343 data nibble bytes
        let mut data_nibbles = [0u8; 343];
        for b in &mut data_nibbles {
            *b = nibbles[pos % len];
            pos = (pos + 1) % len;
        }
        // Skip checksum + epilogue
        pos = (pos + 3) % len;

        sectors[sector as usize] = decode_6and2_nibbles(&data_nibbles);
        found += 1;
    }

    sectors
}

/// Sector ordering for .dsk/.po disk images.
enum SectorOrder {
    Dos33,
    ProDos,
}

/// Check if data at an offset looks like a ProDOS volume directory header.
/// Storage type $F, name length 1-15, name is uppercase letters/digits/period.
fn is_prodos_volume_header(data: &[u8]) -> bool {
    let storage_type = data[0] >> 4;
    let name_len = data[0] & 0x0F;
    if storage_type != 0x0F || name_len == 0 || name_len > 15 {
        return false;
    }
    (1..=name_len as usize).all(|i| {
        let c = data[i];
        (c >= b'A' && c <= b'Z') || (c >= b'0' && c <= b'9') || c == b'.'
    })
}

/// Detect whether a 143360-byte disk image is in DOS 3.3 or ProDOS sector order.
///
/// For ProDOS disks, the volume directory is at block 2. Its location in the
/// file differs depending on sector order:
///   - ProDOS order (.po): block 2 = file sectors 4,5 → byte offset 1024
///   - DOS 3.3 order (.dsk): block 2 = physical sectors 8,10 → DOS logical 11,10
///     → byte offset 2816
fn detect_sector_order(data: &[u8]) -> SectorOrder {
    // Check for ProDOS volume directory at block 2 in BOTH possible locations.
    let po_dir = 4 * 256;   // ProDOS order: sector 4
    let do_dir = 11 * 256;  // DOS order: physical 8 → DOS logical 11

    let po_valid = is_prodos_volume_header(&data[po_dir..]);
    let do_valid = is_prodos_volume_header(&data[do_dir..]);

    if po_valid && !do_valid {
        return SectorOrder::ProDos;
    }
    if do_valid && !po_valid {
        return SectorOrder::Dos33;
    }

    // Neither or both matched — fall back to DOS 3.3 VTOC check.
    let vtoc_offset = 17 * 16 * 256;
    let vtoc_track = data[vtoc_offset + 1];
    let vtoc_sector = data[vtoc_offset + 2];
    let vtoc_version = data[vtoc_offset + 3];
    if vtoc_track == 0x11 && vtoc_sector == 0x0F && vtoc_version == 3 {
        return SectorOrder::Dos33;
    }

    // Default to DOS 3.3 order
    SectorOrder::Dos33
}

/// Nibblize a single track using the DOS 3.3 interleave (used by tests).
#[cfg(test)]
fn nibblize_track(track: u8, track_data: &[u8]) -> [u8; TRACK_NIBBLE_SIZE] {
    nibblize_track_with_interleave(track, track_data, &DOS33_PHYSICAL_TO_LOGICAL)
}

/// Nibblize a single track of 16 × 256-byte sectors into GCR-encoded nibble stream.
fn nibblize_track_with_interleave(
    track: u8,
    track_data: &[u8],
    interleave: &[usize; 16],
) -> [u8; TRACK_NIBBLE_SIZE] {
    let mut nibbles = [0u8; TRACK_NIBBLE_SIZE];
    let mut pos = 0;

    let volume = 254u8;

    for physical_sector in 0..16 {
        let logical_sector = interleave[physical_sector];
        let sector_data = &track_data[logical_sector * 256..logical_sector * 256 + 256];

        // Gap 1: sync bytes
        let gap = if physical_sector == 0 { 40 } else { 14 };
        for _ in 0..gap {
            if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xFF; pos += 1; }
        }

        // Address field
        // Prologue: $D5 $AA $96
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xD5; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xAA; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0x96; pos += 1; }

        // 4-and-4 encoded fields
        let sector = physical_sector as u8;
        let checksum = volume ^ track ^ sector;
        encode_4and4(&mut nibbles, &mut pos, volume);
        encode_4and4(&mut nibbles, &mut pos, track);
        encode_4and4(&mut nibbles, &mut pos, sector);
        encode_4and4(&mut nibbles, &mut pos, checksum);

        // Epilogue: $DE $AA $EB
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xDE; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xAA; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xEB; pos += 1; }

        // Gap 2: sync bytes between address and data
        for _ in 0..7 {
            if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xFF; pos += 1; }
        }

        // Data field
        // Prologue: $D5 $AA $AD
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xD5; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xAA; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xAD; pos += 1; }

        // 6-and-2 encode the 256-byte sector
        encode_6and2(&mut nibbles, &mut pos, sector_data);

        // Epilogue: $DE $AA $EB
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xDE; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xAA; pos += 1; }
        if pos < TRACK_NIBBLE_SIZE { nibbles[pos] = 0xEB; pos += 1; }
    }

    // Fill remaining with sync bytes
    while pos < TRACK_NIBBLE_SIZE {
        nibbles[pos] = 0xFF;
        pos += 1;
    }

    nibbles
}

/// Encode a byte as two 4-and-4 nibbles (odd bits, even bits).
fn encode_4and4(nibbles: &mut [u8; TRACK_NIBBLE_SIZE], pos: &mut usize, val: u8) {
    if *pos < TRACK_NIBBLE_SIZE {
        nibbles[*pos] = (val >> 1) | 0xAA;
        *pos += 1;
    }
    if *pos < TRACK_NIBBLE_SIZE {
        nibbles[*pos] = val | 0xAA;
        *pos += 1;
    }
}

/// 6-and-2 encode 256 bytes of sector data into 342 + 1 nibbles.
fn encode_6and2(nibbles: &mut [u8; TRACK_NIBBLE_SIZE], pos: &mut usize, data: &[u8]) {
    // Step 1: Build auxiliary buffer (86 bytes) from low 2 bits of each byte.
    // The low 2 bits are reversed (bit 0 ↔ bit 1) per Apple II RWTS convention.
    let mut aux = [0u8; 86];
    for i in 0..256 {
        let low2 = ((data[i] & 0x01) << 1) | ((data[i] & 0x02) >> 1);
        let aux_idx = 85 - (i % 86);
        let shift = (i / 86) * 2;
        aux[aux_idx] |= low2 << shift;
    }

    // Step 2: Build primary buffer (256 bytes) from high 6 bits
    let mut primary = [0u8; 256];
    for i in 0..256 {
        primary[i] = data[i] >> 2;
    }

    // Step 3: XOR-chain encode (aux then primary)
    let mut prev = 0u8;

    // Aux bytes (86 values, reversed order)
    for i in (0..86).rev() {
        let val = aux[i] ^ prev;
        prev = aux[i];
        if *pos < TRACK_NIBBLE_SIZE {
            nibbles[*pos] = WRITE_TABLE[(val & 0x3F) as usize];
            *pos += 1;
        }
    }

    // Primary bytes (256 values)
    for i in 0..256 {
        let val = primary[i] ^ prev;
        prev = primary[i];
        if *pos < TRACK_NIBBLE_SIZE {
            nibbles[*pos] = WRITE_TABLE[(val & 0x3F) as usize];
            *pos += 1;
        }
    }

    // Checksum byte
    if *pos < TRACK_NIBBLE_SIZE {
        nibbles[*pos] = WRITE_TABLE[(prev & 0x3F) as usize];
        *pos += 1;
    }
}

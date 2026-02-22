/// Apple II Disk II controller.
///
/// Emulates the Disk II interface card in slot 6.
/// I/O at $C0E0-$C0EF, boot ROM at $C600-$C6FF.
/// Supports reading .dsk (DOS-order 16-sector) disk images.

/// Number of bytes in a nibblized track.
const TRACK_NIBBLE_SIZE: usize = 6656;

/// Number of tracks on a standard 5.25" disk.
const NUM_TRACKS: usize = 35;

/// DOS 3.3 sector interleave table (logical → physical).
static DOS33_INTERLEAVE: [usize; 16] = [
    0, 13, 11, 9, 7, 5, 3, 1, 14, 12, 10, 8, 6, 4, 2, 15,
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
    /// Current track (0-34).
    current_track: u8,
    /// Current byte position within the track's nibble stream.
    byte_position: usize,
    /// Motor on/off.
    motor_on: bool,
    /// Phase magnet states (4 phases for head stepping).
    phase_states: [bool; 4],
    /// Phase position (0-68 half-tracks, current_track = phase_position / 2).
    phase_position: u8,
    /// Data latch (last byte read).
    data_latch: u8,
    /// Write mode flag.
    write_mode: bool,
    /// Whether a disk is loaded.
    disk_loaded: bool,
    /// Boot ROM (256 bytes, P5 PROM at $C600-$C6FF).
    boot_rom: [u8; 256],
    /// Cycle accumulator for nibble timing (~32 CPU cycles per nibble byte).
    cycle_accumulator: u32,
}

impl DiskII {
    pub fn new() -> Self {
        Self {
            nibble_data: Vec::new(),
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

    /// Load a .dsk image (143360 bytes = 35 tracks × 16 sectors × 256 bytes).
    pub fn load_dsk(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() != 143360 {
            return Err(format!(
                "Invalid .dsk image size: {} (expected 143360)",
                data.len()
            ));
        }

        self.nibble_data.clear();
        for track in 0..NUM_TRACKS {
            let track_data = &data[track * 4096..(track + 1) * 4096];
            self.nibble_data.push(nibblize_track(track as u8, track_data));
        }

        self.disk_loaded = true;
        self.current_track = 0;
        self.byte_position = 0;
        self.phase_position = 0;
        log::info!("Disk II: loaded .dsk image ({} tracks)", NUM_TRACKS);
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
                // Q6L: Read data latch
                if !self.write_mode {
                    self.advance_byte();
                }
                self.data_latch
            }
            0xD => {
                // Q6H: Write load (not implemented for read-only)
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
    pub fn io_write(&mut self, addr: u16, _val: u8) {
        let switch = (addr & 0x0F) as u8;
        match switch {
            0x0..=0x7 => self.handle_phase(switch),
            0x8 => self.motor_on = false,
            0x9 => self.motor_on = true,
            0xA | 0xB => {} // Drive select
            0xC => { /* Q6L */ }
            0xD => { /* Q6H: write load */ }
            0xE => self.write_mode = false,
            0xF => self.write_mode = true,
            _ => {}
        }
    }

    /// Step the disk controller: advance the byte position based on CPU cycles.
    /// ~32 CPU cycles per nibble byte at 1.023 MHz (gives ~300 RPM).
    pub fn step(&mut self, cycles: u8) {
        if !self.motor_on || !self.disk_loaded {
            return;
        }

        self.cycle_accumulator += cycles as u32;
        // Each nibble byte takes ~32 CPU cycles
        while self.cycle_accumulator >= 32 {
            self.cycle_accumulator -= 32;
            // Track rotation continues even when not reading
            self.byte_position = (self.byte_position + 1) % TRACK_NIBBLE_SIZE;
        }
    }

    /// Handle phase magnet activation/deactivation for head stepping.
    fn handle_phase(&mut self, switch: u8) {
        let phase = (switch >> 1) as usize;
        let on = switch & 1 != 0;
        self.phase_states[phase] = on;

        if !on {
            return;
        }

        // Determine which direction to step based on the phase that's activated
        // relative to the current phase position
        let current_phase = (self.phase_position / 2) as usize % 4;

        // Check if activated phase is adjacent (next or previous)
        let next_phase = (current_phase + 1) % 4;
        let prev_phase = (current_phase + 3) % 4;

        if phase == next_phase && self.phase_position < 68 {
            self.phase_position += 1;
        } else if phase == prev_phase && self.phase_position > 0 {
            self.phase_position -= 1;
        }

        let new_track = self.phase_position / 2;
        if new_track != self.current_track {
            self.current_track = new_track;
            self.byte_position = 0; // Reset position on track change
        }
    }

    /// Read the next nibble byte from the current track.
    fn advance_byte(&mut self) {
        if !self.disk_loaded || (self.current_track as usize) >= self.nibble_data.len() {
            self.data_latch = 0xFF;
            return;
        }

        self.data_latch = self.nibble_data[self.current_track as usize][self.byte_position];
        self.byte_position = (self.byte_position + 1) % TRACK_NIBBLE_SIZE;
    }
}

/// Nibblize a single track of 16 × 256-byte sectors into GCR-encoded nibble stream.
fn nibblize_track(track: u8, track_data: &[u8]) -> [u8; TRACK_NIBBLE_SIZE] {
    let mut nibbles = [0u8; TRACK_NIBBLE_SIZE];
    let mut pos = 0;

    let volume = 254u8;

    for physical_sector in 0..16 {
        // DOS 3.3 interleave: map physical sector to logical sector
        let logical_sector = DOS33_INTERLEAVE[physical_sector];
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
    // Step 1: Build auxiliary buffer (86 bytes) from low 2 bits of each byte
    let mut aux = [0u8; 86];
    for i in 0..256 {
        let low2 = data[i] & 0x03;
        let aux_idx = i % 86;
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

/// 1541 Disk Drive Emulation.
///
/// The 1541 is a standalone computer with its own 6502 CPU, 2KB RAM,
/// 16KB ROM, and two VIA 6522 chips. It communicates with the C64
/// over the IEC serial bus.
///
/// This module handles GCR encoding and disk mechanics.

pub mod bus;

/// GCR (Group Code Recording) encoding table: 4-bit nybble → 5-bit GCR.
static GCR_ENCODE: [u8; 16] = [
    0x0A, 0x0B, 0x12, 0x13, 0x0E, 0x0F, 0x16, 0x17,
    0x09, 0x19, 0x1A, 0x1B, 0x0D, 0x1D, 0x1E, 0x15,
];

/// GCR decode table: 5-bit GCR → 4-bit nybble (0xFF = invalid).
static GCR_DECODE: [u8; 32] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // 00-07
    0xFF, 0x08, 0x00, 0x01, 0xFF, 0x0C, 0x04, 0x05, // 08-0F
    0xFF, 0xFF, 0x02, 0x03, 0xFF, 0x0F, 0x06, 0x07, // 10-17
    0xFF, 0x09, 0x0A, 0x0B, 0xFF, 0x0D, 0x0E, 0xFF, // 18-1F
];

/// Bytes per track by zone (GCR-encoded track lengths).
pub fn gcr_track_size(track: u8) -> usize {
    match track {
        1..=17 => 7692,
        18..=24 => 7142,
        25..=30 => 6666,
        31..=35 => 6250,
        _ => 6250,
    }
}

/// Sectors per track by zone.
pub fn sectors_per_track(track: u8) -> u8 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        31..=35 => 17,
        _ => 17,
    }
}

/// Speed zone index (0-3) for timer-based reading speed.
pub fn speed_zone(track: u8) -> u8 {
    match track {
        1..=17 => 3,
        18..=24 => 2,
        25..=30 => 1,
        31..=35 => 0,
        _ => 0,
    }
}

/// Cycles between byte-ready signals per speed zone.
/// The drive's read circuitry delivers bytes at different rates per zone.
pub fn cycles_per_byte(zone: u8) -> u16 {
    match zone {
        0 => 32, // slowest (outer tracks)
        1 => 30,
        2 => 28,
        3 => 26, // fastest (inner tracks)
        _ => 32,
    }
}

/// Encode a 4-byte group into 5 GCR bytes.
fn gcr_encode_group(input: &[u8; 4]) -> [u8; 5] {
    let g0 = GCR_ENCODE[(input[0] >> 4) as usize];
    let g1 = GCR_ENCODE[(input[0] & 0x0F) as usize];
    let g2 = GCR_ENCODE[(input[1] >> 4) as usize];
    let g3 = GCR_ENCODE[(input[1] & 0x0F) as usize];
    let g4 = GCR_ENCODE[(input[2] >> 4) as usize];
    let g5 = GCR_ENCODE[(input[2] & 0x0F) as usize];
    let g6 = GCR_ENCODE[(input[3] >> 4) as usize];
    let g7 = GCR_ENCODE[(input[3] & 0x0F) as usize];

    // Pack 8 × 5-bit groups into 5 bytes (40 bits)
    let bits: u64 = (g0 as u64) << 35 | (g1 as u64) << 30
        | (g2 as u64) << 25 | (g3 as u64) << 20
        | (g4 as u64) << 15 | (g5 as u64) << 10
        | (g6 as u64) << 5 | g7 as u64;

    [
        (bits >> 32) as u8,
        (bits >> 24) as u8,
        (bits >> 16) as u8,
        (bits >> 8) as u8,
        bits as u8,
    ]
}

/// Holds the GCR-encoded track data for all 35 tracks.
pub struct GcrDisk {
    /// GCR-encoded track data. Index 0 = track 1.
    pub tracks: Vec<Vec<u8>>,
    /// Current head position (track index, 0-based).
    pub current_track: u8,
    /// Current byte position within the track.
    pub byte_position: usize,
    /// Half-track position for stepper motor (0-69).
    pub half_track: u8,
    /// Motor on/off.
    pub motor_on: bool,
    /// Cycles until next byte is ready.
    pub byte_counter: u16,
    /// Last byte read from the track.
    pub current_byte: u8,
    /// True when a new byte is available to read.
    pub byte_ready: bool,
    /// Write protect flag.
    pub write_protect: bool,
}

impl GcrDisk {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            current_track: 0,
            byte_position: 0,
            half_track: 0,
            motor_on: false,
            byte_counter: 0,
            current_byte: 0,
            byte_ready: false,
            write_protect: true,
        }
    }

    /// Convert a D64 image into GCR-encoded track data.
    pub fn load_d64(&mut self, d64_data: &[u8]) {
        self.tracks.clear();

        // Read disk ID from BAM (track 18 sector 0, bytes 162-163).
        // Every sector header must carry these IDs; the 1541 ROM reads the
        // ID from the BAM during init and then verifies it on every header read.
        let bam_offset: usize = (1u8..18)
            .map(|t| sectors_per_track(t) as usize)
            .sum::<usize>() * 256; // = 91 392 bytes for a standard D64
        let disk_id1 = d64_data.get(bam_offset + 162).copied().unwrap_or(0x20);
        let disk_id2 = d64_data.get(bam_offset + 163).copied().unwrap_or(0x20);

        let mut offset = 0usize;
        for track_num in 1..=35u8 {
            let num_sectors = sectors_per_track(track_num);
            let track_size = gcr_track_size(track_num);
            let mut track_data = Vec::with_capacity(track_size);

            for sector in 0..num_sectors {
                // --- Sector header ---
                // Sync bytes (5 × $FF)
                for _ in 0..5 {
                    track_data.push(0xFF);
                }

                // Header block (8 bytes → 2 GCR groups of 4):
                //   [0x08, checksum, sector, track]  [ID1, ID2, 0x0F, 0x0F]
                // checksum = sector XOR track XOR ID1 XOR ID2
                let hdr_checksum = sector ^ track_num ^ disk_id1 ^ disk_id2;
                track_data.extend_from_slice(&gcr_encode_group(&[0x08, hdr_checksum, sector, track_num]));
                track_data.extend_from_slice(&gcr_encode_group(&[disk_id1, disk_id2, 0x0F, 0x0F]));

                // Header gap (9 × $55)
                for _ in 0..9 {
                    track_data.push(0x55);
                }

                // --- Data block ---
                // Sync bytes (5 × $FF)
                for _ in 0..5 {
                    track_data.push(0xFF);
                }

                // Data block layout (260 bytes → 65 GCR groups of 4 → 325 GCR bytes):
                //   [0x07, d[0], d[1], d[2]]          group 0  (marker + first 3 bytes)
                //   [d[3], d[4], d[5], d[6]]           group 1
                //   ...
                //   [d[251], d[252], d[253], d[254]]   group 63
                //   [d[255], checksum, 0x00, 0x00]     group 64
                //
                // Pre-compute data checksum = XOR of all 256 data bytes.
                let sector_data_offset = offset + sector as usize * 256;
                let mut data_checksum = 0u8;
                for i in 0..256 {
                    data_checksum ^= d64_data.get(sector_data_offset + i).copied().unwrap_or(0);
                }

                // Group 0: marker + d[0..2]
                let d0 = d64_data.get(sector_data_offset).copied().unwrap_or(0);
                let d1 = d64_data.get(sector_data_offset + 1).copied().unwrap_or(0);
                let d2 = d64_data.get(sector_data_offset + 2).copied().unwrap_or(0);
                track_data.extend_from_slice(&gcr_encode_group(&[0x07, d0, d1, d2]));

                // Groups 1-63: d[3..254] (63 full groups of 4)
                let mut di = 3usize;
                while di + 3 < 256 {
                    let b0 = d64_data.get(sector_data_offset + di).copied().unwrap_or(0);
                    let b1 = d64_data.get(sector_data_offset + di + 1).copied().unwrap_or(0);
                    let b2 = d64_data.get(sector_data_offset + di + 2).copied().unwrap_or(0);
                    let b3 = d64_data.get(sector_data_offset + di + 3).copied().unwrap_or(0);
                    track_data.extend_from_slice(&gcr_encode_group(&[b0, b1, b2, b3]));
                    di += 4;
                }
                // Group 64: d[255] + checksum + two off-bytes
                let d255 = d64_data.get(sector_data_offset + 255).copied().unwrap_or(0);
                track_data.extend_from_slice(&gcr_encode_group(&[d255, data_checksum, 0x00, 0x00]));

                // Inter-sector gap (variable, track padded to exact size at end)
                for _ in 0..8 {
                    track_data.push(0x55);
                }
            }

            // Pad or truncate to exact GCR track size
            track_data.resize(track_size, 0x55);
            self.tracks.push(track_data);

            offset += num_sectors as usize * 256;
        }

        self.current_track = 0;
        self.half_track = 0;
        self.byte_position = 0;
    }

    /// Step the disk rotation by one drive CPU cycle.
    /// Updates byte_ready when a new byte is available.
    pub fn step(&mut self) {
        if !self.motor_on || self.tracks.is_empty() {
            return;
        }

        self.byte_counter = self.byte_counter.saturating_sub(1);
        if self.byte_counter == 0 {
            let track_idx = (self.half_track / 2) as usize;
            if track_idx < self.tracks.len() {
                let track = &self.tracks[track_idx];
                if !track.is_empty() {
                    self.current_byte = track[self.byte_position % track.len()];
                    self.byte_position = (self.byte_position + 1) % track.len();
                }
            }
            self.byte_ready = true;
            let zone = speed_zone((self.half_track / 2) + 1);
            self.byte_counter = cycles_per_byte(zone);
        }
    }

    /// Move the stepper motor. `step_bits` are the 2-bit stepper phase from VIA2 PB0-PB1.
    /// The stepper moves when the phase changes by +1 or -1.
    pub fn step_head(&mut self, new_phase: u8) {
        let current_phase = self.half_track & 0x03;
        let diff = (new_phase.wrapping_sub(current_phase)) & 0x03;

        match diff {
            1 => {
                // Step inward (toward track 35)
                if self.half_track < 69 {
                    self.half_track += 1;
                }
            }
            3 => {
                // Step outward (toward track 1)
                if self.half_track > 0 {
                    self.half_track -= 1;
                }
            }
            _ => {} // No step (same phase or 180° — invalid)
        }

        self.current_track = self.half_track / 2;
    }
}

#[allow(dead_code)]
/// Decode a 5-bit GCR value back to a 4-bit nybble.
pub fn gcr_decode_nybble(gcr: u8) -> u8 {
    GCR_DECODE[(gcr & 0x1F) as usize]
}

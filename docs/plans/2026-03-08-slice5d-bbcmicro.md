# Slice 5D: BBC Micro — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot the BBC Micro to BASIC, load `.ssd`/`.dsd` disk images, and play an Acornsoft game with correct video (6845 CRTC + ULA 8 modes) and audio (SN76489 PSG).

**Architecture:** New `emu-bbcmicro` crate. Two 6522 VIAs (copied from `emu-c64/via.rs`), a 6845 CRTC for display address generation, an Acorn ULA for mode-specific pixel output, and an SN76489 PSG. The 8271 FDC is emulated at the command level to support `.ssd` (single-sided) and `.dsd` (double-sided) disk images. ROMs: MOS (16KB at `$C000-$FFFF`) + BASIC (16KB, paged at `$8000-$BFFF`, bank 0).

**Tech Stack:** Rust, `emu-common`, `emu-cpu` (Cpu6502), 6845 CRTC, ULA video, SN76489 audio, 8271 FDC command emulation.

---

## Context: What Exists

- No BBC Micro crate exists.
- The BBC Micro uses a 6502A CPU at 2 MHz.
- 6845 CRTC: generates MA (memory address) and RA (row address) for each character position.
- Acorn ULA: handles screen mode decoding, colour generation, and video RAM addressing.
- SN76489: 3 tone channels + 1 noise. Write-only, 4-bit volume per channel, 10-bit tone period.
- 6522 VIA × 2: System VIA (keyboard/sound) and User VIA (user port/RS-423).
- 8271 FDC: floppy disk controller, register-based. `.ssd` = 80-track × 10-sector × 256-byte.

---

## Phase A: Crate Skeleton

### Task A1: Create `emu-bbcmicro` crate

**Files:**
- Create: `crates/bbcmicro/Cargo.toml`
- Create all module stubs
- Modify: workspace `Cargo.toml`

**Step 1: `crates/bbcmicro/Cargo.toml`:**
```toml
[package]
name = "emu-bbcmicro"
version = "0.1.0"
edition = "2021"

[dependencies]
emu-common = { path = "../common" }
emu-cpu    = { path = "../cpu" }
log        = { workspace = true }
```

**Step 2: Add to workspace, create stub files:**
```
crates/bbcmicro/src/lib.rs
crates/bbcmicro/src/bus.rs
crates/bbcmicro/src/crtc.rs
crates/bbcmicro/src/ula.rs
crates/bbcmicro/src/sn76489.rs
crates/bbcmicro/src/via.rs
crates/bbcmicro/src/fdc.rs
```

**Step 3: Build and commit:**
```sh
cargo build --workspace
git add crates/bbcmicro/ Cargo.toml
git commit -m "feat(bbcmicro): add emu-bbcmicro crate skeleton"
```

---

## Phase B: VIA 6522

### Task B1: Copy VIA from C64 crate

**Files:**
- Write: `crates/bbcmicro/src/via.rs`

**Step 1:** Copy `crates/c64/src/via.rs` verbatim to `crates/bbcmicro/src/via.rs`. Change `pub(crate)` to `pub` throughout (different crate).

**Step 2: Commit:**
```sh
git add crates/bbcmicro/src/via.rs
git commit -m "feat(bbcmicro): copy VIA 6522 from C64 crate"
```

---

## Phase C: SN76489 PSG

### Task C1: Implement SN76489

**Files:**
- Write: `crates/bbcmicro/src/sn76489.rs`

**Background:** The SN76489 has 4 channels: 3 square-wave tone generators and 1 noise generator. It is **write-only** — no read registers. One byte is written at a time via a single data port; the high bit indicates whether it's a latch byte (select channel+type) or a data byte (set high bits of period/volume).

**Register format:**
- Latch byte: `1 CC T DDDD` — CC=channel (0-3), T=type (0=tone period, 1=volume), DDDD=low 4 bits of data
- Data byte:  `0 XXXXDD DD` — upper bits of period (XX = ignored, DDDDDD = 6-bit upper period)

**Tone period**: 10-bit value. Frequency = clock / (32 × period). BBC clock = 4 MHz (SN76489 clock pin), so 4000000 / (32 × period).

**Noise control (channel 3 tone register):**
- Bits 0-1: noise rate (00=clock/512, 01=clock/1024, 10=clock/2048, 11=tone2 frequency)
- Bit 2: noise type (0=periodic, 1=white)

**Step 1: Write `crates/bbcmicro/src/sn76489.rs`:**
```rust
const CLOCK: f64 = 4_000_000.0; // BBC Micro SN76489 clock

pub struct Sn76489 {
    // Tone channels 0-2: 10-bit period, 4-bit volume (0=max, 15=off)
    period: [u16; 3],
    volume: [u8; 4],   // channels 0-2 = tone, channel 3 = noise
    // Noise channel
    noise_ctrl: u8,    // bits 0-1 rate, bit 2 type
    noise_lfsr: u16,   // 15-bit LFSR

    // Internal state
    latched_channel: usize,
    latched_type:    bool,  // false=period, true=volume

    counter: [u16; 3],
    output:  [bool; 3],
    noise_counter: u16,
    noise_output:  bool,

    // Audio
    sample_rate:  u32,
    sample_accum: f64,
    cpu_cycle:    u64,
    pub sample_buffer: Vec<f32>,
}

impl Sn76489 {
    pub fn new() -> Self {
        Self {
            period:  [0x3FF; 3],
            volume:  [0x0F; 4], // max attenuation (silent)
            noise_ctrl: 0,
            noise_lfsr: 0x8000,
            latched_channel: 0,
            latched_type: false,
            counter: [0; 3],
            output:  [false; 3],
            noise_counter: 0,
            noise_output:  false,
            sample_rate: 44100,
            sample_accum: 0.0,
            cpu_cycle: 0,
            sample_buffer: Vec::with_capacity(1024),
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) { self.sample_rate = rate; }

    /// Write one byte to the SN76489.
    pub fn write(&mut self, val: u8) {
        if val & 0x80 != 0 {
            // Latch byte: select channel + type + low 4 data bits
            self.latched_channel = ((val >> 5) & 0x03) as usize;
            self.latched_type = val & 0x10 != 0;
            let data = (val & 0x0F) as u16;
            if !self.latched_type {
                if self.latched_channel < 3 {
                    // Period: replace low 4 bits
                    self.period[self.latched_channel] =
                        (self.period[self.latched_channel] & 0x3F0) | data;
                } else {
                    // Noise ctrl
                    self.noise_ctrl = (val & 0x07) as u8;
                    self.noise_lfsr = 0x8000;
                }
            } else {
                // Volume
                self.volume[self.latched_channel] = (val & 0x0F) as u8;
            }
        } else {
            // Data byte: upper bits
            let data = (val & 0x3F) as u16;
            if !self.latched_type && self.latched_channel < 3 {
                self.period[self.latched_channel] =
                    (self.period[self.latched_channel] & 0x000F) | (data << 4);
            }
            // Volume data byte: should not happen but ignore
        }
    }

    /// Step one CPU cycle (BBC Micro CPU = 2 MHz, SN76489 clock = 4 MHz).
    pub fn step(&mut self) {
        self.cpu_cycle += 1;

        // Step at 4 MHz rate = 2× per CPU cycle (simplified: step 2 SN clocks per CPU cycle)
        for _ in 0..2 {
            for ch in 0..3 {
                if self.period[ch] == 0 {
                    self.output[ch] = false;
                    continue;
                }
                if self.counter[ch] == 0 {
                    self.counter[ch] = self.period[ch];
                    self.output[ch] = !self.output[ch];
                } else {
                    self.counter[ch] -= 1;
                }
            }

            // Noise channel
            let noise_period = match self.noise_ctrl & 0x03 {
                0 => 0x10u16,
                1 => 0x20,
                2 => 0x40,
                _ => self.period[2],
            };
            if noise_period > 0 {
                if self.noise_counter == 0 {
                    self.noise_counter = noise_period;
                    if self.noise_ctrl & 0x04 != 0 {
                        // White noise: LFSR tap bits 0 and 3
                        let bit = (self.noise_lfsr ^ (self.noise_lfsr >> 3)) & 1;
                        self.noise_lfsr = (self.noise_lfsr >> 1) | (bit << 14);
                    } else {
                        // Periodic: simple divide
                        self.noise_lfsr = self.noise_lfsr.rotate_right(1);
                    }
                    self.noise_output = self.noise_lfsr & 1 != 0;
                } else {
                    self.noise_counter -= 1;
                }
            }
        }

        // Generate sample
        let cpu_freq: f64 = 2_000_000.0;
        self.sample_accum += self.sample_rate as f64;
        if self.sample_accum >= cpu_freq {
            self.sample_accum -= cpu_freq;
            self.sample_buffer.push(self.mix());
        }
    }

    fn vol_to_linear(vol: u8) -> f32 {
        // 0 = max, 15 = silent. 2dB steps.
        if vol >= 15 { return 0.0; }
        // 10^(-vol*2/20) = 10^(-vol/10)
        10.0f32.powf(-(vol as f32) * 0.1)
    }

    fn mix(&self) -> f32 {
        let mut out = 0.0f32;
        for ch in 0..3 {
            if self.output[ch] {
                out += Self::vol_to_linear(self.volume[ch]) / 4.0;
            }
        }
        if self.noise_output {
            out += Self::vol_to_linear(self.volume[3]) / 4.0;
        }
        out.min(1.0)
    }

    pub fn drain_samples(&mut self, out: &mut [f32]) -> usize {
        let n = out.len().min(self.sample_buffer.len());
        out[..n].copy_from_slice(&self.sample_buffer[..n]);
        self.sample_buffer.drain(..n);
        n
    }
}
```

**Step 2: Commit:**
```sh
git add crates/bbcmicro/src/sn76489.rs
git commit -m "feat(bbcmicro): SN76489 PSG — 3 tone + 1 noise channel"
```

---

## Phase D: 6845 CRTC + ULA Video

### Task D1: Implement 6845 CRTC

**Files:**
- Write: `crates/bbcmicro/src/crtc.rs`

**Background:** The 6845 CRTC generates display control signals: horizontal/vertical sync, memory address (MA[13:0]), and row address (RA[4:0]). The ULA uses MA + RA to compute screen RAM addresses and fetch pixel bytes.

**6845 registers (accessed via address register $FE00, data $FE01):**
```
R0:  Horizontal Total
R1:  Horizontal Displayed (columns)
R4:  Vertical Total
R6:  Vertical Displayed (rows)
R7:  Vertical Sync Position
R9:  Max Scan Line Address (character height - 1)
R12: Start Address High (bits 13:8)
R13: Start Address Low (bits 7:0)
```

**Step 1: Write `crates/bbcmicro/src/crtc.rs`:**
```rust
pub struct Crtc {
    pub regs: [u8; 32],
    pub address_reg: u8,

    // Internal counters
    pub h_count:  u8,   // Horizontal character counter
    pub v_count:  u8,   // Vertical line counter
    pub ra:       u8,   // Row address (scan line within character)
    pub ma:       u16,  // Memory address (character position)
    pub ma_row:   u16,  // MA at start of current row
    pub vsync:    bool,
    pub hsync:    bool,
    pub display:  bool, // In visible display area
    pub frame_end: bool,
}

impl Crtc {
    pub fn new() -> Self {
        let mut regs = [0u8; 32];
        // Sensible Mode 7 defaults
        regs[0] = 63;  // H total
        regs[1] = 40;  // H displayed
        regs[4] = 30;  // V total
        regs[6] = 25;  // V displayed
        regs[7] = 27;  // V sync pos
        regs[9] = 9;   // Max scan line (10 scan lines per char row)
        regs[12] = 0x74; // Start addr high (0x7400 = Mode 7 RAM)
        regs[13] = 0x00;
        Self {
            regs, address_reg: 0,
            h_count: 0, v_count: 0, ra: 0,
            ma: 0x7400, ma_row: 0x7400,
            vsync: false, hsync: false, display: false, frame_end: false,
        }
    }

    pub fn write_addr(&mut self, val: u8) { self.address_reg = val & 0x1F; }

    pub fn write_data(&mut self, val: u8) {
        if (self.address_reg as usize) < self.regs.len() {
            self.regs[self.address_reg as usize] = val;
        }
    }

    pub fn read_data(&self) -> u8 {
        match self.address_reg {
            14 => (self.ma >> 8) as u8,
            15 => (self.ma & 0xFF) as u8,
            _ if (self.address_reg as usize) < self.regs.len() => self.regs[self.address_reg as usize],
            _ => 0,
        }
    }

    /// Advance one character clock (1 MHz pixel clock for character rendering).
    pub fn step(&mut self) {
        let h_total    = self.regs[0] as u8;
        let h_display  = self.regs[1] as u8;
        let v_total    = self.regs[4] as u8;
        let v_display  = self.regs[6] as u8;
        let v_sync_pos = self.regs[7] as u8;
        let max_ra     = self.regs[9] as u8;
        let start_addr = ((self.regs[12] as u16) << 8) | self.regs[13] as u16;

        self.display = self.h_count < h_display && self.v_count < v_display;
        self.hsync   = self.h_count == h_total.saturating_sub(4);
        self.vsync   = self.v_count == v_sync_pos;

        if self.display {
            self.ma += 1;
        }

        self.h_count += 1;
        if self.h_count > h_total {
            self.h_count = 0;

            if self.ra < max_ra {
                self.ra += 1;
                self.ma = self.ma_row; // Reload MA from start of row
            } else {
                self.ra = 0;
                self.ma_row = self.ma; // MA is already past end of row — save for next row
                self.v_count += 1;

                if self.v_count > v_total {
                    self.v_count = 0;
                    self.ma = start_addr;
                    self.ma_row = start_addr;
                    self.frame_end = true;
                }
            }
        }
    }

    pub fn start_address(&self) -> u16 {
        ((self.regs[12] as u16) << 8) | self.regs[13] as u16
    }
}
```

**Step 2: Commit:**
```sh
git add crates/bbcmicro/src/crtc.rs
git commit -m "feat(bbcmicro): 6845 CRTC — MA/RA generation for all 8 video modes"
```

---

### Task D2: Implement ULA video

**Files:**
- Write: `crates/bbcmicro/src/ula.rs`

**Background:** The Acorn ULA decodes the video mode and generates pixel data. It reads bytes from screen RAM using the CRTC's MA/RA outputs.

**BBC Micro video modes:**
```
Mode 0: 640×256, 2 colours, 80 cols text  (20KB)
Mode 1: 320×256, 4 colours, 40 cols text  (20KB)
Mode 2: 160×256, 16 colours, 20 cols text (20KB)
Mode 3: 640×200, 2 colours, 80 cols text  (16KB, teletext font)
Mode 4: 320×256, 2 colours, 40 cols text  (10KB)
Mode 5: 160×256, 4 colours, 20 cols text  (10KB)
Mode 6: 320×200, 2 colours, 40 cols text  (8KB, teletext font)
Mode 7: 40-col teletext (SAA5050 chip)    (1KB)
```

For our implementation: Modes 0-6 are bitmapped, Mode 7 is text-mode approximation.

**ULA register at $FE20 (write-only):**
- Bits 0-2: flash rate / cursor
- Bits 3-6: video mode (ULA screen mode ≠ CRTC mode; set by OS together)

Typical: MODE 0 → ULA $28, MODE 7 → ULA $9C.

**Step 1: Write `crates/bbcmicro/src/ula.rs`:**
```rust
use emu_common::FrameBuffer;

pub const SCREEN_WIDTH: u32  = 640;
pub const SCREEN_HEIGHT: u32 = 256;

/// BBC Micro logical colour palette (colours 0-7, normal BBC palette).
static BBC_PALETTE: [u32; 16] = [
    0x000000, // 0 Black
    0xFF0000, // 1 Red
    0x00FF00, // 2 Green
    0xFFFF00, // 3 Yellow
    0x0000FF, // 4 Blue
    0xFF00FF, // 5 Magenta
    0x00FFFF, // 6 Cyan
    0xFFFFFF, // 7 White
    // Flashing variants (same as above for simplicity)
    0x000000, 0xFF0000, 0x00FF00, 0xFFFF00,
    0x0000FF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
];

pub struct Ula {
    pub ctrl_reg: u8,     // $FE20 write — screen mode + misc
    pub palette:  [u8; 16], // Logical to physical colour mapping
    pub frame_ready: bool,
    pub framebuffer: FrameBuffer,

    // Current rendering position
    render_x: u32,
    render_y: u32,
}

impl Ula {
    pub fn new() -> Self {
        let mut palette = [0u8; 16];
        for i in 0..8 { palette[i] = i as u8; } // Identity mapping
        Self {
            ctrl_reg: 0x28, // Mode 0 default
            palette,
            frame_ready: false,
            framebuffer: FrameBuffer::new(SCREEN_WIDTH, SCREEN_HEIGHT),
            render_x: 0,
            render_y: 0,
        }
    }

    pub fn write_ctrl(&mut self, val: u8) { self.ctrl_reg = val; }

    /// Returns the screen mode (0-7) derived from the ULA control register.
    pub fn screen_mode(&self) -> u8 {
        (self.ctrl_reg >> 2) & 0x07
    }

    /// Pixels per byte for current mode.
    fn pixels_per_byte(&self) -> u32 {
        match self.screen_mode() {
            0 | 3 => 8,
            1 | 4 | 6 => 4,
            2 | 5 => 2,
            _ => 2, // Mode 7: approximate
        }
    }

    /// Called each character clock with the byte fetched from screen RAM by the CRTC.
    /// `display`: whether CRTC says this is a visible cell.
    pub fn render_char(&mut self, byte: u8, display: bool, frame_end: bool) {
        if frame_end {
            self.render_x = 0;
            self.render_y = 0;
            self.frame_ready = true;
            return;
        }

        let ppb = self.pixels_per_byte();
        if display && self.render_y < SCREEN_HEIGHT {
            for bit in 0..ppb {
                let px = self.render_x + bit;
                if px >= SCREEN_WIDTH { break; }
                let color_idx = self.decode_pixel(byte, bit, ppb);
                let phys = self.palette[color_idx & 0xF] as usize;
                self.framebuffer.set_pixel_rgb(px, self.render_y, BBC_PALETTE[phys & 0xF]);
            }
        }

        if display {
            self.render_x += ppb;
        }

        // Advance on H-sync
        // (In a real implementation, we'd track HSYNC/VSYNC from CRTC;
        //  for simplicity, advance when render_x exceeds screen width)
        if self.render_x >= SCREEN_WIDTH {
            self.render_x = 0;
            self.render_y += 1;
            if self.render_y >= SCREEN_HEIGHT {
                self.render_y = 0;
            }
        }
    }

    fn decode_pixel(&self, byte: u8, bit: u32, ppb: u32) -> usize {
        match ppb {
            8 => ((byte >> (7 - bit)) & 1) as usize,
            4 => ((byte >> (7 - bit * 2)) & 1) as usize |
                 (((byte >> (6 - bit * 2)) & 1) as usize) << 1,
            2 => {
                // 4 colours per byte: bits 7,6,5,4 → pixel 0,1; bits 3,2,1,0 → pixel 2,3
                let hi = (byte >> (7 - bit)) & 1;
                let lo = (byte >> (3 - bit)) & 1;
                (hi as usize) << 1 | lo as usize
            }
            _ => 0,
        }
    }
}
```

**Step 2: Commit:**
```sh
git add crates/bbcmicro/src/ula.rs
git commit -m "feat(bbcmicro): ULA video — 8 BBC Micro screen modes"
```

---

## Phase E: 8271 FDC (Disk Support)

### Task E1: Implement 8271 FDC

**Files:**
- Write: `crates/bbcmicro/src/fdc.rs`

**Background:** The 8271 FDC is register-based. The BBC OS communicates via commands (Read Sector, Write Sector, Seek, etc.) issued to the FDC's command register at `$FE80`. The FDC uses DMA or interrupt-driven byte transfer.

For our purposes: implement command-level emulation that serves sector data from an in-memory `.ssd` image without cycle-accurate timing.

**SSD format:** 80 tracks × 10 sectors × 256 bytes = 204,800 bytes. Track/sector addressing starts from 0.

**Step 1: Write `crates/bbcmicro/src/fdc.rs`:**
```rust
/// 8271 Floppy Disk Controller (command-level emulation).
pub struct Fdc {
    pub disk: Option<SsdDisk>,
    // Registers
    pub status:  u8,  // $FE80 read
    pub result:  u8,  // $FE84 read
    param_buf:   [u8; 5],
    param_count: u8,
    param_idx:   u8,
    current_cmd: u8,
    // Data transfer
    pub data_ready: bool,
    pub data_reg:   u8,
    sector_buf:     Vec<u8>,
    sector_pos:     usize,
    // IRQ
    pub irq_pending: bool,
}

pub struct SsdDisk {
    data: Vec<u8>,
    tracks: u8,       // 80 for standard
    sectors: u8,      // 10 for standard
    sector_size: u16, // 256
}

impl SsdDisk {
    pub fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        if raw.is_empty() { return Err("Empty disk image".into()); }
        // Standard SSD: 80 × 10 × 256 = 204800 bytes
        let sectors = 10u8;
        let sector_size = 256u16;
        let tracks = (raw.len() / (sectors as usize * sector_size as usize)) as u8;
        Ok(Self { data: raw.to_vec(), tracks, sectors, sector_size })
    }

    pub fn read_sector(&self, track: u8, sector: u8) -> &[u8] {
        let offset = (track as usize * self.sectors as usize + sector as usize) * self.sector_size as usize;
        let end = (offset + self.sector_size as usize).min(self.data.len());
        if offset >= self.data.len() { return &[]; }
        &self.data[offset..end]
    }
}

impl Fdc {
    pub fn new() -> Self {
        Self {
            disk: None,
            status: 0, result: 0,
            param_buf: [0; 5], param_count: 0, param_idx: 0,
            current_cmd: 0,
            data_ready: false, data_reg: 0,
            sector_buf: Vec::new(), sector_pos: 0,
            irq_pending: false,
        }
    }

    pub fn load_ssd(&mut self, data: &[u8]) -> Result<(), String> {
        self.disk = Some(SsdDisk::from_bytes(data)?);
        Ok(())
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x07 {
            0x00 => self.status,
            0x01 => { // Result register
                // Real: after command, read result here
                self.result
            }
            0x04 => {
                // Data register
                self.data_reg
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x07 {
            0x00 => { // Command register
                self.current_cmd = val;
                self.param_idx = 0;
                match val {
                    0x35 => self.param_count = 3, // Read sector
                    0x7B => self.param_count = 3, // Write sector
                    0x69 => self.param_count = 1, // Seek
                    _    => self.param_count = 0,
                }
                if self.param_count == 0 {
                    self.execute_command();
                }
            }
            0x01 => { // Parameter register
                if (self.param_idx as usize) < self.param_buf.len() {
                    self.param_buf[self.param_idx as usize] = val;
                    self.param_idx += 1;
                    if self.param_idx >= self.param_count {
                        self.execute_command();
                    }
                }
            }
            0x04 => { // Data write (for write commands)
                if !self.sector_buf.is_empty() && self.sector_pos < self.sector_buf.len() {
                    self.sector_buf[self.sector_pos] = val;
                    self.sector_pos += 1;
                    if self.sector_pos >= self.sector_buf.len() {
                        self.data_ready = false;
                        self.status = 0x00; // Idle
                        self.irq_pending = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// Provide next data byte to CPU (read transfer).
    pub fn poll_data(&mut self) -> Option<u8> {
        if self.data_ready && self.sector_pos < self.sector_buf.len() {
            let b = self.sector_buf[self.sector_pos];
            self.sector_pos += 1;
            if self.sector_pos >= self.sector_buf.len() {
                self.data_ready = false;
                self.status = 0x00;
                self.result = 0x00; // OK
                self.irq_pending = true;
            }
            Some(b)
        } else {
            None
        }
    }

    fn execute_command(&mut self) {
        match self.current_cmd {
            0x35 => { // Read sector: [drive/track, sector, count]
                let track  = self.param_buf[0] & 0x7F;
                let sector = self.param_buf[1] & 0x0F;
                if let Some(disk) = &self.disk {
                    let data = disk.read_sector(track, sector);
                    self.sector_buf = data.to_vec();
                    self.sector_pos = 0;
                    self.data_ready = true;
                    self.status = 0x80; // Busy
                } else {
                    self.result = 0x18; // Drive not ready
                    self.irq_pending = true;
                }
            }
            0x69 => { // Seek: [track]
                // No-op for now (no physical head movement needed)
                self.result = 0x00;
                self.irq_pending = true;
            }
            _ => {
                self.result = 0x18;
                self.irq_pending = true;
            }
        }
    }
}
```

**Step 2: Commit:**
```sh
git add crates/bbcmicro/src/fdc.rs
git commit -m "feat(bbcmicro): 8271 FDC — command-level SSD disk emulation"
```

---

## Phase F: Bus + SystemEmulator + Frontend

### Task F1: Bus + lib.rs

**Files:**
- Write: `crates/bbcmicro/src/bus.rs`
- Write: `crates/bbcmicro/src/lib.rs`

**BBC Micro memory map:**
```
$0000-$7FFF  RAM (32KB)
$8000-$BFFF  Paged ROM (bank 0-15, default = BASIC)
$C000-$FBFF  MOS ROM
$FC00-$FEFF  I/O space
  $FE00/$FE01  6845 CRTC (address/data)
  $FE20        ULA video control (write)
  $FE40-$FE4F  System VIA (6522)
  $FE60-$FE6F  User VIA (6522)
  $FE80-$FE87  8271 FDC
$FF00-$FFFF  MOS ROM (high page)
```

**Step 1: Write `crates/bbcmicro/src/bus.rs`:**
```rust
use emu_common::Bus;
use crate::crtc::Crtc;
use crate::ula::Ula;
use crate::sn76489::Sn76489;
use crate::via::Via;
use crate::fdc::Fdc;

pub struct BbcBus {
    pub ram:       [u8; 0x8000], // 32KB RAM
    pub basic_rom: Vec<u8>,      // 16KB BASIC ROM ($8000-$BFFF)
    pub mos_rom:   Vec<u8>,      // 16KB MOS ROM ($C000-$FFFF)
    pub crtc:      Crtc,
    pub ula:       Ula,
    pub sn76489:   Sn76489,
    pub sys_via:   Via,          // System VIA ($FE40-$FE4F)
    pub user_via:  Via,          // User VIA ($FE60-$FE6F)
    pub fdc:       Fdc,

    // CRTC/ULA rendering state
    cycles:     u32,
    crtc_divider: u32,  // CRTC steps at 1 MHz (every 2 CPU cycles)
}

impl BbcBus {
    pub fn new(mos_rom: Vec<u8>, basic_rom: Vec<u8>) -> Self {
        Self {
            ram: [0; 0x8000],
            basic_rom,
            mos_rom,
            crtc: Crtc::new(),
            ula: Ula::new(),
            sn76489: Sn76489::new(),
            sys_via: Via::new(),
            user_via: Via::new(),
            fdc: Fdc::new(),
            cycles: 0,
            crtc_divider: 0,
        }
    }
}

impl Bus for BbcBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.ram[addr as usize],
            0x8000..=0xBFFF => self.basic_rom.get((addr - 0x8000) as usize).copied().unwrap_or(0xFF),
            0xC000..=0xFBFF | 0xFF00..=0xFFFF => {
                self.mos_rom.get((addr - 0xC000) as usize).copied().unwrap_or(0xFF)
            }
            // I/O
            0xFE00 => 0, // CRTC address register (write-only)
            0xFE01 => self.crtc.read_data(),
            0xFE40..=0xFE4F => self.sys_via.read(addr - 0xFE40),
            0xFE60..=0xFE6F => self.user_via.read(addr - 0xFE60),
            0xFE80..=0xFE87 => self.fdc.read(addr - 0xFE80),
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => self.ram[addr as usize] = val,
            0xFE00 => self.crtc.write_addr(val),
            0xFE01 => self.crtc.write_data(val),
            0xFE20 => self.ula.write_ctrl(val),
            0xFE40..=0xFE4F => {
                self.sys_via.write(addr - 0xFE40, val);
                // System VIA Port B bit 0-2 = SN76489 data/write enable
                if addr == 0xFE43 { // ORB — Port B output
                    // BBC writes to SN76489 when bit 3=0 (write enable) and bits 0-2 vary
                    // Actual SN76489 data comes from VIA Port A ($FE41)
                    let porta = self.sys_via.ora;
                    let portb = val;
                    if portb & 0x08 == 0 { // WE low = write to SN76489
                        self.sn76489.write(porta);
                    }
                }
            }
            0xFE60..=0xFE6F => self.user_via.write(addr - 0xFE60, val),
            0xFE80..=0xFE87 => self.fdc.write(addr - 0xFE80, val),
            _ => {}
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.ram[addr as usize],
            0x8000..=0xBFFF => self.basic_rom.get((addr - 0x8000) as usize).copied().unwrap_or(0xFF),
            0xC000..=0xFFFF => self.mos_rom.get((addr - 0xC000) as usize).copied().unwrap_or(0xFF),
            _ => 0xFF,
        }
    }

    fn tick(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.sn76489.step();
            self.sys_via.step();
            self.user_via.step();

            // CRTC runs at 1 MHz = every 2 CPU cycles (BBC runs at 2 MHz)
            self.crtc_divider += 1;
            if self.crtc_divider >= 2 {
                self.crtc_divider = 0;
                self.crtc.step();
                // Fetch screen byte for ULA
                let ma = self.crtc.ma;
                let screen_byte = self.ram.get(ma as usize).copied().unwrap_or(0);
                self.ula.render_char(screen_byte, self.crtc.display, self.crtc.frame_end);
                self.crtc.frame_end = false;
            }
        }
    }

    fn poll_nmi(&mut self) -> bool { false }
    fn poll_irq(&mut self) -> bool {
        self.sys_via.irq_pending() || self.user_via.irq_pending() || self.fdc.irq_pending
    }
}
```

**Step 2: Write `crates/bbcmicro/src/lib.rs`:**
```rust
pub mod bus;
pub mod crtc;
pub mod fdc;
pub mod sn76489;
pub mod ula;
pub mod via;

use emu_common::{AudioSample, Button, CpuDebugState, DebugSection, FrameBuffer, InputEvent, SystemEmulator};
use emu_cpu::Cpu6502;
use bus::BbcBus;

pub struct BbcMicro {
    cpu: Cpu6502<BbcBus>,
}

impl BbcMicro {
    pub fn with_roms(mos: Vec<u8>, basic: Vec<u8>) -> Self {
        let bus = BbcBus::new(mos, basic);
        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.reset();
        Self { cpu }
    }

    pub fn load_ssd(&mut self, data: &[u8]) -> Result<(), String> {
        self.cpu.bus.fdc.load_ssd(data)
    }
}

impl SystemEmulator for BbcMicro {
    fn step_frame(&mut self) -> usize {
        self.cpu.bus.ula.frame_ready = false;
        while !self.cpu.bus.ula.frame_ready {
            self.cpu.step();
        }
        0
    }

    fn framebuffer(&self) -> &FrameBuffer { &self.cpu.bus.ula.framebuffer }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.sn76489.drain_samples(out)
    }

    fn handle_input(&mut self, _event: InputEvent) {
        // BBC keyboard scanning is via System VIA — complex matrix
        // Minimal: no keyboard input for now
    }

    fn reset(&mut self) { self.cpu.reset(); }
    fn set_sample_rate(&mut self, rate: u32) { self.cpu.bus.sn76489.set_sample_rate(rate); }

    fn display_width(&self)  -> u32 { ula::SCREEN_WIDTH }
    fn display_height(&self) -> u32 { ula::SCREEN_HEIGHT }
    fn target_fps(&self)     -> f64 { 50.0 } // PAL BBC Micro
    fn system_name(&self)    -> &str { "BBC Micro" }
    fn save_state_system_id(&self) -> &str { "BBCMicro" }

    fn cpu_state(&self) -> CpuDebugState {
        CpuDebugState { pc: self.cpu.pc, sp: self.cpu.sp, a: self.cpu.a,
            x: self.cpu.x, y: self.cpu.y, flags: self.cpu.p.bits(), cycles: self.cpu.total_cycles }
    }
    fn peek_memory(&self, addr: u16) -> u8 { self.cpu.bus.peek(addr) }
    fn disassemble(&self, addr: u16) -> (String, u16) {
        emu_cpu::disassemble_6502(|a| self.cpu.bus.peek(a), addr)
    }
    fn step_instruction(&mut self) { self.cpu.step(); }

    fn system_debug_panels(&self) -> Vec<DebugSection> {
        let crtc = &self.cpu.bus.crtc;
        let ula  = &self.cpu.bus.ula;
        let sn   = &self.cpu.bus.sn76489;
        let via  = &self.cpu.bus.sys_via;

        let crtc_sec = DebugSection::new("6845 CRTC")
            .row("Mode",    format!("{}", ula.screen_mode()))
            .row("MA/RA",   format!("${:04X} / {}", crtc.ma, crtc.ra))
            .row("H/V cnt", format!("{} / {}", crtc.h_count, crtc.v_count))
            .row("Display", format!("{}", crtc.display));

        let sn_sec = DebugSection::new("SN76489")
            .row("Ch0", format!("period={} vol={}", sn.period[0], sn.volume[0]))
            .row("Ch1", format!("period={} vol={}", sn.period[1], sn.volume[1]))
            .row("Ch2", format!("period={} vol={}", sn.period[2], sn.volume[2]))
            .row("Noise", format!("ctrl={:02X} vol={}", sn.noise_ctrl, sn.volume[3]));

        let via_sec = DebugSection::new("System VIA")
            .row("ORA/IRA", format!("${:02X}/${:02X}", via.ora, via.ira))
            .row("ORB/IRB", format!("${:02X}/${:02X}", via.orb, via.irb));

        vec![crtc_sec, sn_sec, via_sec]
    }
}
```

**Step 3: Build:**
```sh
cargo build -p emu-bbcmicro
```

**Step 4: Commit:**
```sh
git add crates/bbcmicro/src/bus.rs crates/bbcmicro/src/lib.rs
git commit -m "feat(bbcmicro): Bus + SystemEmulator — 6502A + CRTC + ULA + SN76489 + 8271"
```

---

### Task F2: Frontend registration

**Files:**
- Modify: `crates/frontend/Cargo.toml`, `system_select.rs`, `app.rs`, `system_roms.rs`

**Step 1:** Add `emu-bbcmicro = { path = "../bbcmicro" }` to frontend Cargo.toml.

**Step 2:** Add `BbcMicro` to `SystemChoice`:
```rust
pub enum SystemChoice { Nes, Apple2, C64, Atari2600, Vic20, Atari800, Atari7800, BbcMicro }
```

Button:
```rust
if ui.add_sized(button_size, egui::Button::new("BBC Micro")).clicked() {
    action = Some(SystemAction::LoadRom(SystemChoice::BbcMicro));
}
if ui.add_sized(small_button, egui::Button::new("Boot BBC Micro")).clicked() {
    action = Some(SystemAction::BootSystem(SystemChoice::BbcMicro));
}
```

**Step 3:** ROM loader in `system_roms.rs`:
```rust
const BBC_MOS_NAMES:   &[&str] = &["os1.2.rom", "OS-1.2.ROM", "bbcos.rom"];
const BBC_BASIC_NAMES: &[&str] = &["basic2.rom", "BASIC2.ROM", "bbcbasic.rom"];

pub fn load_bbc_roms(dir: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let subdir = dir.join("bbcmicro");
    let find = |names: &[&str]| -> Option<Vec<u8>> {
        for n in names {
            if let Some(d) = try_load(&subdir.join(n)) { return Some(d); }
            if let Some(d) = try_load(&dir.join(n))    { return Some(d); }
        }
        None
    };
    (find(BBC_MOS_NAMES), find(BBC_BASIC_NAMES))
}
```

**Step 4:** Wire in `app.rs` (boot + load SSD), following C64/Apple II pattern.

**Step 5:**
```sh
cargo build --workspace
git add crates/bbcmicro/ crates/frontend/
git commit -m "feat(frontend): register BBC Micro system"
git push
```

---

## Final Verification

```sh
cargo test --workspace && cargo build --workspace
```

### Manual Test Checklist
- [ ] Boot BBC Micro → `BBC Computer` boot screen → `>` prompt
- [ ] Load `.ssd` disk image → catalog reads
- [ ] `PRINT "HELLO"` executes in BASIC
- [ ] SN76489 audio: `SOUND 1,-15,100,10` plays a tone
- [ ] Debugger → CRTC, SN76489, VIA panels visible

# Slice 5B: Atari 400/800/5200 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot the Atari 800 to BASIC, load cartridges and ATR disk images, play a 5200 game — all with POKEY audio, GTIA/CTIA video, and ANTIC DMA.

**Architecture:** New `emu-atari8bit` crate. Three modes share most hardware: Atari 400/800 (48KB RAM, BASIC cart, OS ROM, full keyboard), Atari 5200 (analog controllers, no BASIC ROM, no keyboard). ANTIC generates display list scan addresses for GTIA. POKEY handles audio, keyboard scanning, and serial I/O. All in one crate, mode selected at construction.

**Tech Stack:** Rust, `emu-common`, `emu-cpu` (Cpu6502), ATR disk format, standard Atari 8-bit ROM files.

---

## Context: What Exists

- No Atari 8-bit crate exists. This is a new crate.
- The Atari 8-bit uses a standard 6502 (not 6507 like the 2600).
- POKEY is a unique Atari chip: 4 audio channels with poly4/5/9/17 LFSRs + keyboard matrix + serial port.
- GTIA/CTIA is the graphics chip (replaces TIA from 2600 era). ANTIC handles all DMA for the display list.
- ATR format: 16-byte header + raw sector data. Standard 720-sector, 128-byte/sector = 92,176 bytes total.

---

## Phase A: Crate Skeleton

### Task A1: Create `emu-atari8bit` crate

**Files:**
- Create: `crates/atari8bit/Cargo.toml`
- Create: `crates/atari8bit/src/lib.rs`
- Modify: workspace `Cargo.toml`

**Step 1: `crates/atari8bit/Cargo.toml`:**
```toml
[package]
name = "emu-atari8bit"
version = "0.1.0"
edition = "2021"

[dependencies]
emu-common = { path = "../common" }
emu-cpu    = { path = "../cpu" }
log        = { workspace = true }
```

**Step 2: Add to workspace members:**
```toml
"crates/atari8bit",
```

**Step 3: Create module stubs:**
```
crates/atari8bit/src/lib.rs
crates/atari8bit/src/bus.rs
crates/atari8bit/src/pokey.rs
crates/atari8bit/src/gtia.rs
crates/atari8bit/src/antic.rs
crates/atari8bit/src/pia.rs
crates/atari8bit/src/atr.rs
```

**Step 4: Build:**
```sh
cargo build --workspace
```

**Step 5: Commit:**
```sh
git add crates/atari8bit/ Cargo.toml
git commit -m "feat(atari8bit): add emu-atari8bit crate skeleton"
```

---

## Phase B: POKEY Audio + Keyboard

### Task B1: Implement POKEY

**Files:**
- Write: `crates/atari8bit/src/pokey.rs`

**Background:** POKEY (POtentiometer and KEYboard) handles:
- 4 audio channels (channels 0-3), each with 8-bit frequency counter and control byte
- Polynomial counters: 4-bit (period 15), 5-bit (period 31), 9-bit (period 511), 17-bit (period 131071)
- Channels can be linked: ch0+ch1 become a 16-bit counter, ch2+ch3 similarly
- Audio control register (AUDCTL) at $D208 selects clock dividers and channel linking
- SKCTL register controls serial, clock, and key debounce

**POKEY register map ($D200–$D20F, read/write mixed):**
```
Write:
  $D200 AUDF1  — channel 0 frequency
  $D201 AUDC1  — channel 0 control (waveform + volume)
  $D202 AUDF2  — channel 1 frequency
  $D203 AUDC2  — channel 1 control
  $D204 AUDF3  — channel 2 frequency
  $D205 AUDC3  — channel 2 control
  $D206 AUDF4  — channel 3 frequency
  $D207 AUDC4  — channel 3 control
  $D208 AUDCTL — audio control (clock select, channel linking, high-pass)
  $D20E IRQEN  — interrupt enable
  $D20F SKCTL  — serial/keyboard control

Read:
  $D200 POT0..POT7 ($D200-$D207) — potentiometer values
  $D208 ALLPOT  — which pots are done
  $D209 KBCODE  — last key pressed
  $D20A RANDOM  — random number (from noise LFSR)
  $D20E IRQST   — interrupt status
  $D20F SKSTAT  — serial/keyboard status
```

**AUDC bits:**
- Bits 6-4: volume (0-15 in bits 3:0 when bit 4=0; envelope when bit 4=1)
- Bit 5: use poly5 to clock tone
- Bit 4: use poly4/pure tone select
- Bit 7: high-pass filter enable

**Step 1: Write `crates/atari8bit/src/pokey.rs`:**
```rust
use emu_common::AudioSample;

pub struct Pokey {
    pub audf:   [u8; 4],   // Frequency registers
    pub audc:   [u8; 4],   // Control registers
    pub audctl: u8,

    // Internal counters
    freq_counter: [u16; 4],
    tone_output:  [bool; 4],

    // Polynomial counters
    poly4:  u8,    // 4-bit, period 15
    poly5:  u8,    // 5-bit, period 31
    poly9:  u16,   // 9-bit, period 511
    poly17: u32,   // 17-bit, period 131071

    // Divider from system clock to POKEY clock
    // POKEY base clock = 64kHz (system 1.79MHz / 28) or 15kHz (system / 114)
    // AUDCTL bit 0 selects 1.79MHz for ch3/4, bits 4/5 for ch1/2
    clock_div: u8,

    // Keyboard
    pub kbcode: u8,
    pub irqst:  u8,
    pub irqen:  u8,
    pub skctl:  u8,

    // RANDOM from poly17
    pub random: u8,

    // Audio output
    sample_rate:  u32,
    cpu_cycles:   u64,
    sample_accum: f64,
    pub sample_buffer: Vec<f32>,
}

impl Pokey {
    pub fn new() -> Self {
        Self {
            audf: [0; 4], audc: [0; 4], audctl: 0,
            freq_counter: [0; 4],
            tone_output: [false; 4],
            poly4: 0xF, poly5: 0x1F, poly9: 0x1FF, poly17: 0x1FFFF,
            clock_div: 28, // default 64kHz base
            kbcode: 0xFF, irqst: 0xFF, irqen: 0, skctl: 0,
            random: 0xFF,
            sample_rate: 44100,
            cpu_cycles: 0,
            sample_accum: 0.0,
            sample_buffer: Vec::with_capacity(1024),
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) { self.sample_rate = rate; }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x08 => 0xFF,         // ALLPOT (no potentiometers)
            0x09 => self.kbcode,
            0x0A => self.random,
            0x0E => self.irqst,
            0x0F => 0xFF,         // SKSTAT
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.audf[0] = val,
            0x01 => self.audc[0] = val,
            0x02 => self.audf[1] = val,
            0x03 => self.audc[1] = val,
            0x04 => self.audf[2] = val,
            0x05 => self.audc[2] = val,
            0x06 => self.audf[3] = val,
            0x07 => self.audc[3] = val,
            0x08 => {
                self.audctl = val;
                // AUDCTL bit 0: 1.79MHz base for channels 3&4
                // AUDCTL bit 1: 1.79MHz base for channels 1&2
                // (simplified: update clock_div based on AUDCTL)
            }
            0x0A => {} // STIMER — reset timers (no-op for basic audio)
            0x0E => self.irqen = val,
            0x0F => self.skctl = val,
            _ => {}
        }
    }

    /// Step one CPU cycle. Updates poly counters, tone output, generates audio.
    pub fn step(&mut self) {
        self.cpu_cycles += 1;

        // Advance poly counters every cycle
        let p4 = ((self.poly4 >> 1) ^ self.poly4) & 1;
        self.poly4 = ((self.poly4 >> 1) | (p4 << 3)) & 0x0F;

        let p5 = ((self.poly5 >> 2) ^ self.poly5) & 1;
        self.poly5 = ((self.poly5 >> 1) | (p5 << 4)) & 0x1F;

        let p9 = (((self.poly9 >> 4) ^ self.poly9) & 1) as u16;
        self.poly9 = ((self.poly9 >> 1) | (p9 << 8)) & 0x1FF;

        let p17 = (((self.poly17 >> 12) ^ self.poly17) & 1) as u32;
        self.poly17 = ((self.poly17 >> 1) | (p17 << 16)) & 0x1FFFF;
        self.random = (self.poly17 & 0xFF) as u8;

        // POKEY base clock: CPU clock / 28 ≈ 63.9 kHz
        if self.cpu_cycles % 28 == 0 {
            self.clock_channels();
        }

        // Generate audio sample at target rate
        let cpu_freq: f64 = 1_789_773.0;
        self.sample_accum += self.sample_rate as f64;
        if self.sample_accum >= cpu_freq {
            self.sample_accum -= cpu_freq;
            self.sample_buffer.push(self.mix());
        }
    }

    fn clock_channels(&mut self) {
        // 16-bit linked mode: ch0+ch1, ch2+ch3 when AUDCTL bits 4/5 set
        let link_01 = self.audctl & 0x10 != 0;
        let link_23 = self.audctl & 0x08 != 0;

        for ch in 0..4 {
            // Determine period (AUDF+1 in base-clock units)
            let period = self.audf[ch] as u16 + 1;

            if self.freq_counter[ch] == 0 {
                self.freq_counter[ch] = period;
                // Select waveform source
                let audc = self.audc[ch];
                let use_poly5_gate = audc & 0x80 != 0;
                let pure = audc & 0x20 != 0;

                let output = if pure {
                    !self.tone_output[ch] // pure tone
                } else if use_poly5_gate {
                    if self.poly5 & 1 != 0 {
                        if audc & 0x40 != 0 { self.poly4 & 1 != 0 } else { self.poly17 & 1 != 0 }
                    } else {
                        self.tone_output[ch]
                    }
                } else {
                    if audc & 0x40 != 0 { self.poly4 & 1 != 0 } else { self.poly17 & 1 != 0 }
                };
                self.tone_output[ch] = output;
            } else {
                self.freq_counter[ch] -= 1;
            }
        }
        let _ = (link_01, link_23); // simplified: treat each channel independently
    }

    fn mix(&self) -> f32 {
        let mut out = 0.0f32;
        for ch in 0..4 {
            if self.tone_output[ch] {
                let vol = (self.audc[ch] & 0x0F) as f32 / 15.0 / 4.0;
                out += vol;
            }
        }
        out.min(1.0)
    }

    pub fn drain_samples(&mut self, out: &mut [f32]) -> usize {
        let n = out.len().min(self.sample_buffer.len());
        out[..n].copy_from_slice(&self.sample_buffer[..n]);
        self.sample_buffer.drain(..n);
        n
    }

    pub fn key_press(&mut self, kbcode: u8) {
        self.kbcode = kbcode;
        if self.irqen & 0x40 != 0 {
            self.irqst &= !0x40; // Set keyboard IRQ pending
        }
    }

    pub fn irq_pending(&self) -> bool {
        (!self.irqst) & self.irqen != 0
    }
}
```

**Step 2: Build:**
```sh
cargo build -p emu-atari8bit
```

**Step 3: Commit:**
```sh
git add crates/atari8bit/src/pokey.rs
git commit -m "feat(atari8bit): POKEY — 4-channel audio with poly LFSRs"
```

---

## Phase C: GTIA Video Chip

### Task C1: Implement GTIA

**Files:**
- Write: `crates/atari8bit/src/gtia.rs`

**Background:** GTIA (Graphics Television Interface Adaptor) processes pixel data from ANTIC's DMA and applies color and player-missile graphics. For our purposes, ANTIC provides the background pixel data and GTIA applies colors and PM graphics.

**GTIA register map ($D000–$D01F):**
```
$D000-$D003  HPOSP0-HPOSP3: player horizontal positions
$D004-$D005  HPOSM0-HPOSM3: missile positions
$D008-$D00B  SIZEP0-SIZEP3: player sizes
$D00C        SIZEM: missile sizes
$D00D-$D010  GRAFP0-GRAFP3: player graphics data
$D011        GRAFM: missile graphics data
$D012-$D01B  COLPMx/COLPFx: color registers
$D01C        COLBK: background color
$D01D        PRIOR: priority register
$D01E        VDELAY: vertical delay
$D01F        GRACTL: graphics control
```

**Step 1: Write `crates/atari8bit/src/gtia.rs`:**
```rust
use emu_common::FrameBuffer;

pub const SCREEN_WIDTH: u32  = 320;
pub const SCREEN_HEIGHT: u32 = 192;

/// GTIA (Graphics Television Interface Adaptor).
pub struct Gtia {
    pub regs: [u8; 32],

    // Player-missile graphics
    pub hpos: [u8; 4],   // player H positions
    pub hposm: [u8; 4],  // missile H positions
    pub sizep: [u8; 4],  // player sizes
    pub sizem: u8,
    pub grafp: [u8; 4],  // player graphics bytes
    pub grafm: u8,

    // Color registers
    pub colpm: [u8; 4],  // player/missile colors
    pub colpf: [u8; 4],  // playfield colors
    pub colbk: u8,       // background color

    // Control
    pub prior: u8,
    pub gractl: u8,

    // Collision registers (latched)
    pub collision_m2pf: u8,
    pub collision_p2pf: u8,
    pub collision_m2pl: u8,
    pub collision_p2pl: u8,
}

/// Convert Atari 8-bit color byte to RGB24.
/// High nibble = hue (0-15), low nibble = luminance (0-15, even only).
fn atari_color_to_rgb(c: u8) -> u32 {
    // Simplified NTSC hue → RGB mapping
    let hue = (c >> 4) as usize;
    let lum = ((c & 0x0E) as u32) * 17; // 0-238
    let (r, g, b): (u32, u32, u32) = match hue {
        0  => (lum, lum, lum),          // grey
        1  => (lum, lum/2, 0),          // gold
        2  => (lum, lum/3, 0),          // orange
        3  => (lum, 0, 0),              // red-orange
        4  => (lum, 0, lum/2),          // pink
        5  => (lum/2, 0, lum),          // purple
        6  => (0, 0, lum),              // blue-purple
        7  => (0, lum/4, lum),          // blue
        8  => (0, lum/2, lum),          // medium blue
        9  => (0, lum, lum),            // cyan
        10 => (0, lum, lum/2),          // blue-green
        11 => (0, lum, 0),              // green
        12 => (lum/4, lum, 0),          // yellow-green
        13 => (lum/2, lum, 0),          // yellow-green 2
        14 => (lum, lum, 0),            // yellow
        _  => (lum, lum/2, 0),          // yellow-orange
    };
    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

impl Gtia {
    pub fn new() -> Self {
        Self {
            regs: [0; 32],
            hpos: [0; 4], hposm: [0; 4],
            sizep: [0; 4], sizem: 0,
            grafp: [0; 4], grafm: 0,
            colpm: [0; 4], colpf: [0; 4], colbk: 0,
            prior: 0, gractl: 0,
            collision_m2pf: 0, collision_p2pf: 0,
            collision_m2pl: 0, collision_p2pl: 0,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x1F {
            0x00..=0x03 => self.collision_m2pf,  // simplified: same value
            0x04..=0x07 => self.collision_p2pf,
            0x08..=0x0B => self.collision_m2pl,
            0x0C..=0x0F => self.collision_p2pl,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        let r = (addr & 0x1F) as usize;
        self.regs[r] = val;
        match r {
            0x00..=0x03 => self.hpos[r] = val,
            0x04..=0x07 => self.hposm[r - 4] = val,
            0x08..=0x0B => self.sizep[r - 8] = val,
            0x0C        => self.sizem = val,
            0x0D..=0x10 => self.grafp[r - 0x0D] = val,
            0x11        => self.grafm = val,
            0x12..=0x15 => self.colpm[r - 0x12] = val,
            0x16..=0x19 => self.colpf[r - 0x16] = val,
            0x1A        => self.colbk = val,
            0x1B        => self.prior = val,
            0x1C        => {} // VDELAY
            0x1D        => self.gractl = val,
            0x1E        => {  // HITCLR — clear collision registers
                self.collision_m2pf = 0; self.collision_p2pf = 0;
                self.collision_m2pl = 0; self.collision_p2pl = 0;
            }
            _ => {}
        }
    }

    /// Render one scanline of ANTIC DMA data.
    /// `line_data`: the 40-byte playfield data ANTIC fetched for this line.
    /// `scanline`: visible scanline number (0-191).
    /// `mode`: ANTIC display mode for this line (determines pixel width).
    pub fn render_line(&mut self, fb: &mut FrameBuffer, line_data: &[u8], scanline: u32, mode: u8) {
        if scanline >= SCREEN_HEIGHT { return; }

        let bg_rgb = atari_color_to_rgb(self.colbk);

        // Fill background
        for x in 0..SCREEN_WIDTH {
            fb.set_pixel_rgb(x, scanline, bg_rgb);
        }

        // Simplified: render playfield in mode 2 (standard text/graphics)
        // For each byte, render 8 pixels using playfield color selection
        for (byte_idx, &b) in line_data.iter().take(40).enumerate() {
            for bit in 0..8 {
                let x = (byte_idx * 8 + bit) as u32;
                if x >= SCREEN_WIDTH { break; }
                let pf_color = match mode {
                    0x02 => {
                        // Mode 2: character mode — 1 bit = foreground (PF2) vs background
                        if b & (0x80 >> bit) != 0 { self.colpf[2] } else { self.colbk }
                    }
                    0x0C..=0x0F => {
                        // Hi-res modes: 2 bits per pixel
                        let pair = (b >> (6 - (bit & !1))) & 0x03;
                        [self.colbk, self.colpf[0], self.colpf[1], self.colpf[2]][pair as usize]
                    }
                    _ => if b & (0x80 >> bit) != 0 { self.colpf[0] } else { self.colbk },
                };
                fb.set_pixel_rgb(x, scanline, atari_color_to_rgb(pf_color));
            }
        }

        // Overlay player-missile graphics
        for p in 0..4 {
            let pixel_size = match (self.sizep[p] >> 1) & 0x03 {
                0 => 1u32,
                1 | 2 => 2,
                _ => 4,
            };
            let player_x = self.hpos[p] as u32;
            let grafp = self.grafp[p];
            for bit in 0..8u32 {
                if grafp & (0x80 >> bit) != 0 {
                    for px in 0..pixel_size {
                        let x = player_x + bit * pixel_size + px;
                        if x < SCREEN_WIDTH {
                            fb.set_pixel_rgb(x, scanline, atari_color_to_rgb(self.colpm[p]));
                        }
                    }
                }
            }
        }
    }
}
```

**Step 2: Build:**
```sh
cargo build -p emu-atari8bit
```

**Step 3: Commit:**
```sh
git add crates/atari8bit/src/gtia.rs
git commit -m "feat(atari8bit): GTIA video — playfield rendering + player-missile graphics"
```

---

## Phase D: ANTIC Display List

### Task D1: Implement ANTIC

**Files:**
- Write: `crates/atari8bit/src/antic.rs`

**Background:** ANTIC (Alphanumeric Television Interface Controller) is a specialized DMA processor that:
1. Reads a **display list** from RAM — a sequence of mode bytes and address pointers
2. For each scanline, fetches the appropriate data from RAM and passes pixels to GTIA
3. Generates VSYNC (at line 248) and VBLANK NMI

**Display list commands:**
- `$00-$07`: blank lines (n+1 blank lines)
- `$02-$0F` with LMS flag: mode lines with optional load-memory-scan address
- `$41`: JMP — jump to address (lo, hi follow)
- `$42` with NMI flag: VBLANK (end of frame, triggers NMI)

**Step 1: Write `crates/atari8bit/src/antic.rs`:**
```rust
use emu_common::FrameBuffer;
use crate::gtia::Gtia;

pub struct Antic {
    pub dlist_addr: u16,   // Display list base address ($D402/$D403)
    pub dmactl:     u8,    // DMA control ($D400)
    pub chactl:     u8,    // Character control ($D401)
    pub hscrol:     u8,    // Horizontal scroll ($D404)
    pub vscrol:     u8,    // Vertical scroll ($D405)
    pub pmbase:     u8,    // Player-missile base ($D407)
    pub nmien:      u8,    // NMI enable ($D40E)
    pub nmist:      u8,    // NMI status ($D40F)

    // Internal
    dlist_pc:  u16,        // Current DL read pointer
    scan_addr: u16,        // Current scan data address
    scanline:  u16,        // Current video scanline (0-261 NTSC)
    mode_lines_left: u8,   // Lines remaining in current mode block
    current_mode: u8,

    pub framebuffer: FrameBuffer,
    pub frame_ready: bool,
}

impl Antic {
    pub fn new() -> Self {
        Self {
            dlist_addr: 0x0000,
            dmactl: 0, chactl: 0, hscrol: 0, vscrol: 0, pmbase: 0,
            nmien: 0, nmist: 0,
            dlist_pc: 0, scan_addr: 0,
            scanline: 0, mode_lines_left: 0, current_mode: 2,
            framebuffer: FrameBuffer::new(crate::gtia::SCREEN_WIDTH, crate::gtia::SCREEN_HEIGHT),
            frame_ready: false,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => self.dmactl,
            0x0B => (self.scanline >> 1) as u8,  // VCOUNT: scanline / 2
            0x0E => self.nmien,
            0x0F => {
                let st = self.nmist;
                st // NMIST (reading clears VBI bit)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.dmactl = val,
            0x01 => self.chactl = val,
            0x02 => self.dlist_addr = (self.dlist_addr & 0xFF00) | val as u16,
            0x03 => self.dlist_addr = (self.dlist_addr & 0x00FF) | ((val as u16) << 8),
            0x04 => self.hscrol = val,
            0x05 => self.vscrol = val,
            0x07 => self.pmbase = val,
            0x0E => self.nmien = val,
            0x0F => self.nmist = 0, // NMIRES — clear NMI status
            _ => {}
        }
    }

    /// Process one ANTIC scanline. Reads from `memory` and renders via GTIA.
    /// Returns true if a VBI NMI should be triggered.
    pub fn step_scanline(&mut self, memory: &[u8], gtia: &mut Gtia) -> bool {
        let mut nmi = false;
        let visible_start = 8u16;
        let visible_end = 248u16;

        if self.scanline == 0 {
            // New frame: reset display list pointer
            self.dlist_pc = self.dlist_addr;
            self.mode_lines_left = 0;
            self.frame_ready = false;
        }

        if self.scanline >= visible_start && self.scanline < visible_end {
            let vis_line = self.scanline - visible_start;

            if self.mode_lines_left == 0 {
                // Fetch next display list command
                let cmd = self.dl_read(memory);
                let mode = cmd & 0x0F;
                let lms = cmd & 0x40 != 0;
                let dli = cmd & 0x80 != 0;
                let vscrol = cmd & 0x20 != 0;
                let hscrol = cmd & 0x10 != 0;
                let _ = (vscrol, hscrol, dli);

                match mode {
                    0x00..=0x01 => {
                        // Blank lines: (mode+1) blank lines
                        self.mode_lines_left = mode + 1;
                        self.current_mode = 0;
                    }
                    0x01 => {
                        // JMP instruction ($41 with bit 0 set = JVB, $01 = JMP)
                        let lo = self.dl_read(memory);
                        let hi = self.dl_read(memory);
                        self.dlist_pc = u16::from_le_bytes([lo, hi]);
                        self.mode_lines_left = 1;
                        self.current_mode = 0;
                    }
                    _ => {
                        if lms {
                            let lo = self.dl_read(memory);
                            let hi = self.dl_read(memory);
                            self.scan_addr = u16::from_le_bytes([lo, hi]);
                        }
                        // Lines per mode: mode 2=8, mode 3=10, mode 4/5=8, mode 6/7=8, etc.
                        let lines = match mode {
                            0x02 => 8u8,
                            0x03 => 10,
                            0x04 | 0x05 => 8,
                            0x06 | 0x07 => 8,
                            0x08..=0x0F => 1,
                            _ => 1,
                        };
                        self.mode_lines_left = lines;
                        self.current_mode = mode;
                    }
                }
            }

            if self.mode_lines_left > 0 { self.mode_lines_left -= 1; }

            // Fetch line data and render
            if self.current_mode >= 2 && self.dmactl & 0x03 != 0 {
                let bytes_per_line = match self.current_mode {
                    0x02..=0x05 => 40usize,
                    0x06 | 0x07 => 20,
                    0x08 | 0x09 => 10,
                    0x0A | 0x0B => 20,
                    0x0C..=0x0F => 40,
                    _ => 40,
                };
                let line_start = self.scan_addr as usize;
                let line_data: Vec<u8> = (0..bytes_per_line)
                    .map(|i| memory.get(line_start + i).copied().unwrap_or(0))
                    .collect();

                if vis_line < crate::gtia::SCREEN_HEIGHT as u16 {
                    gtia.render_line(&mut self.framebuffer, &line_data, vis_line as u32, self.current_mode);
                }
                self.scan_addr = self.scan_addr.wrapping_add(bytes_per_line as u16);
            }
        }

        // VBI at scanline 248
        if self.scanline == visible_end {
            if self.nmien & 0x40 != 0 {
                self.nmist |= 0x40;
                nmi = true;
            }
            self.frame_ready = true;
        }

        self.scanline += 1;
        if self.scanline >= 262 { // NTSC: 262 lines per frame
            self.scanline = 0;
        }

        nmi
    }

    fn dl_read(&mut self, memory: &[u8]) -> u8 {
        let b = memory.get(self.dlist_pc as usize).copied().unwrap_or(0);
        self.dlist_pc = self.dlist_pc.wrapping_add(1);
        b
    }
}
```

**Step 2: Commit:**
```sh
git add crates/atari8bit/src/antic.rs
git commit -m "feat(atari8bit): ANTIC display list processor + scanline DMA"
```

---

## Phase E: PIA, Bus, ATR Disk, SystemEmulator

### Task E1: PIA + Bus + lib.rs

**Files:**
- Write: `crates/atari8bit/src/pia.rs`
- Write: `crates/atari8bit/src/bus.rs`
- Write: `crates/atari8bit/src/atr.rs`
- Write: `crates/atari8bit/src/lib.rs`

**Step 1: Write `crates/atari8bit/src/pia.rs`** (simplified 6520 PIA for Atari OS ROM select and BASIC enable):
```rust
/// Simplified PIA (6520) for Atari 8-bit.
/// Controls OS/BASIC ROM enable and RAM size bits.
pub struct Pia {
    pub porta: u8,  // $D300: joystick directions
    pub portb: u8,  // $D301: ROM enable bits
}

impl Pia {
    pub fn new() -> Self {
        // PB0-PB7: RAM size, BASIC enable, OS enable
        // Default: all ROMs enabled
        Self { porta: 0xFF, portb: 0xFF }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x03 {
            0x00 => self.porta,
            0x02 => self.portb,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x03 {
            0x00 => self.porta = val,
            0x02 => self.portb = val,
            _ => {}
        }
    }
}
```

**Step 2: Write `crates/atari8bit/src/atr.rs`** (ATR disk image support):
```rust
/// ATR disk image.
/// Header: 16 bytes. Sector size 128 bytes (DD = 256). Sectors numbered from 1.
pub struct AtrDisk {
    sector_size: u16,
    data: Vec<u8>,
}

impl AtrDisk {
    pub fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < 16 { return Err("ATR too short".into()); }
        // Magic: $96, $02
        if raw[0] != 0x96 || raw[1] != 0x02 {
            return Err("Not an ATR file".into());
        }
        let paragraphs = u16::from_le_bytes([raw[2], raw[3]]) as u32
                       | ((raw[6] as u32) << 16);
        let sector_size = u16::from_le_bytes([raw[4], raw[5]]);
        let _ = paragraphs;
        Ok(Self {
            sector_size,
            data: raw[16..].to_vec(),
        })
    }

    /// Read sector (1-based). Returns 128 or 256 bytes.
    pub fn read_sector(&self, sector: u16) -> &[u8] {
        if sector == 0 { return &[]; }
        let offset = (sector - 1) as usize * self.sector_size as usize;
        let end = (offset + self.sector_size as usize).min(self.data.len());
        &self.data[offset..end]
    }
}
```

**Step 3: Write `crates/atari8bit/src/bus.rs`:**
```rust
use emu_common::Bus;
use crate::antic::Antic;
use crate::gtia::Gtia;
use crate::pokey::Pokey;
use crate::pia::Pia;

pub struct Atari8BitBus {
    pub ram:    Box<[u8; 0xC000]>, // 48KB RAM ($0000-$BFFF)
    pub os_rom: Vec<u8>,           // 16KB OS ROM ($C000-$FFFF)
    pub basic_rom: Vec<u8>,        // 8KB BASIC ROM ($A000-$BFFF, gated by PIA PB1)
    pub antic:  Antic,
    pub gtia:   Gtia,
    pub pokey:  Pokey,
    pub pia:    Pia,
    pub nmi_pending: bool,
    cycles: u32,
    cycles_per_line: u32,
}

impl Atari8BitBus {
    pub fn new(os_rom: Vec<u8>, basic_rom: Vec<u8>) -> Self {
        Self {
            ram: Box::new([0; 0xC000]),
            os_rom,
            basic_rom,
            antic: Antic::new(),
            gtia: Gtia::new(),
            pokey: Pokey::new(),
            pia: Pia::new(),
            nmi_pending: false,
            cycles: 0,
            cycles_per_line: 114, // 1.79MHz / 15734Hz ≈ 114 cycles per scanline
        }
    }

    fn basic_enabled(&self) -> bool {
        self.pia.portb & 0x02 == 0 // PB1=0 means BASIC enabled
    }
}

impl Bus for Atari8BitBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => {
                // BASIC ROM at $A000-$BFFF when enabled
                if addr >= 0xA000 && self.basic_enabled() && !self.basic_rom.is_empty() {
                    return self.basic_rom.get((addr - 0xA000) as usize).copied().unwrap_or(0xFF);
                }
                self.ram[addr as usize]
            }
            0xC000..=0xCFFF => self.gtia.read(addr - 0xC000),   // GTIA ($C000 = $D000 mirrored in some configs)
            0xD000..=0xD01F => self.gtia.read(addr - 0xD000),
            0xD200..=0xD2FF => self.pokey.read(addr - 0xD200),
            0xD300..=0xD3FF => self.pia.read(addr - 0xD300),
            0xD400..=0xD4FF => self.antic.read(addr - 0xD400),
            0xD800..=0xFFFF => self.os_rom.get((addr - 0xD800) as usize).copied().unwrap_or(0xFF),
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x9FFF => self.ram[addr as usize] = val,
            0xD000..=0xD01F => self.gtia.write(addr - 0xD000, val),
            0xD200..=0xD2FF => self.pokey.write(addr - 0xD200, val),
            0xD300..=0xD3FF => self.pia.write(addr - 0xD300, val),
            0xD400..=0xD4FF => self.antic.write(addr - 0xD400, val),
            _ => {}
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x9FFF => self.ram[addr as usize],
            0xD800..=0xFFFF => self.os_rom.get((addr - 0xD800) as usize).copied().unwrap_or(0xFF),
            _ => 0xFF,
        }
    }

    fn tick(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.pokey.step();
            self.cycles += 1;

            if self.cycles >= self.cycles_per_line {
                self.cycles = 0;
                // Build RAM snapshot for ANTIC
                let ram_snapshot: Vec<u8> = self.ram[..].to_vec();
                let nmi = self.antic.step_scanline(&ram_snapshot, &mut self.gtia);
                if nmi { self.nmi_pending = true; }
            }
        }
    }

    fn poll_nmi(&mut self) -> bool {
        let n = self.nmi_pending;
        self.nmi_pending = false;
        n
    }

    fn poll_irq(&mut self) -> bool {
        self.pokey.irq_pending()
    }
}
```

**Step 4: Write `crates/atari8bit/src/lib.rs`:**
```rust
pub mod antic;
pub mod atr;
pub mod bus;
pub mod gtia;
pub mod pia;
pub mod pokey;

use emu_common::{AudioSample, Button, CpuDebugState, DebugSection, FrameBuffer, InputEvent, SystemEmulator};
use emu_cpu::Cpu6502;
use bus::Atari8BitBus;

pub struct Atari8Bit {
    cpu: Cpu6502<Atari8BitBus>,
}

impl Atari8Bit {
    pub fn with_roms(os_rom: Vec<u8>, basic_rom: Vec<u8>) -> Self {
        let bus = Atari8BitBus::new(os_rom, basic_rom);
        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.reset();
        Self { cpu }
    }

    pub fn load_cart(&mut self, data: &[u8], addr: u16) {
        for (i, &b) in data.iter().enumerate() {
            let a = addr.wrapping_add(i as u16) as usize;
            if a < self.cpu.bus.ram.len() {
                self.cpu.bus.ram[a] = b;
            }
        }
        self.cpu.reset();
    }
}

impl SystemEmulator for Atari8Bit {
    fn step_frame(&mut self) -> usize {
        self.cpu.bus.antic.frame_ready = false;
        while !self.cpu.bus.antic.frame_ready {
            self.cpu.step();
        }
        0
    }

    fn framebuffer(&self) -> &FrameBuffer { &self.cpu.bus.antic.framebuffer }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.pokey.drain_samples(out)
    }

    fn handle_input(&mut self, event: InputEvent) {
        // Joystick 0 on PIA Port A bits 0-3 (active low)
        let mask = match event.button {
            Button::Up    => Some(0x01u8),
            Button::Down  => Some(0x02),
            Button::Left  => Some(0x04),
            Button::Right => Some(0x08),
            Button::Fire | Button::A => {
                // Fire goes through GTIA (TRIG0 at $D010)
                None
            }
            _ => None,
        };
        if let Some(m) = mask {
            if event.pressed { self.cpu.bus.pia.porta &= !m; }
            else             { self.cpu.bus.pia.porta |=  m; }
        }
    }

    fn reset(&mut self) { self.cpu.reset(); }
    fn set_sample_rate(&mut self, rate: u32) { self.cpu.bus.pokey.set_sample_rate(rate); }

    fn display_width(&self)  -> u32 { gtia::SCREEN_WIDTH }
    fn display_height(&self) -> u32 { gtia::SCREEN_HEIGHT }
    fn target_fps(&self)     -> f64 { 59.94 }
    fn system_name(&self)    -> &str { "Atari 800" }
    fn save_state_system_id(&self) -> &str { "Atari800" }

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
        let pokey = &self.cpu.bus.pokey;
        let antic  = &self.cpu.bus.antic;
        let gtia   = &self.cpu.bus.gtia;

        let pokey_sec = DebugSection::new("POKEY")
            .row("Ch0", format!("AUDF=${:02X} AUDC=${:02X}", pokey.audf[0], pokey.audc[0]))
            .row("Ch1", format!("AUDF=${:02X} AUDC=${:02X}", pokey.audf[1], pokey.audc[1]))
            .row("Ch2", format!("AUDF=${:02X} AUDC=${:02X}", pokey.audf[2], pokey.audc[2]))
            .row("Ch3", format!("AUDF=${:02X} AUDC=${:02X}", pokey.audf[3], pokey.audc[3]))
            .row("AUDCTL", format!("${:02X}", pokey.audctl))
            .row("IRQ",   format!("en=${:02X} st=${:02X}", pokey.irqen, pokey.irqst));

        let antic_sec = DebugSection::new("ANTIC")
            .row("DList",   format!("${:04X}", antic.dlist_addr))
            .row("Scanline",format!("{}", antic.scanline))
            .row("DMACTL", format!("${:02X}", antic.dmactl));

        let gtia_sec = DebugSection::new("GTIA")
            .row("COLBK",  format!("${:02X}", gtia.colbk))
            .row("COLPF",  format!("${:02X} ${:02X} ${:02X} ${:02X}",
                                   gtia.colpf[0], gtia.colpf[1], gtia.colpf[2], gtia.colpf[3]))
            .row("PRIOR",  format!("${:02X}", gtia.prior));

        vec![pokey_sec, antic_sec, gtia_sec]
    }
}
```

**Step 5: Build:**
```sh
cargo build -p emu-atari8bit
```

**Step 6: Commit:**
```sh
git add crates/atari8bit/src/
git commit -m "feat(atari8bit): PIA, Bus, ANTIC/GTIA/POKEY integration, SystemEmulator"
```

---

### Task E2: Register in frontend

**Files:**
- Modify: `crates/frontend/Cargo.toml`
- Modify: `crates/frontend/src/screens/system_select.rs`
- Modify: `crates/frontend/src/app.rs`
- Modify: `crates/frontend/src/system_roms.rs`

**Step 1: Add dependency:**
```toml
emu-atari8bit = { path = "../atari8bit" }
```

**Step 2: Add `Atari800` to `SystemChoice`:**
```rust
pub enum SystemChoice { Nes, Apple2, C64, Atari2600, Vic20, Atari800 }
```

Button in render():
```rust
if ui.add_sized(button_size, egui::Button::new("Atari 800")).clicked() {
    action = Some(SystemAction::LoadRom(SystemChoice::Atari800));
}
if ui.add_sized(small_button, egui::Button::new("Boot Atari 800")).clicked() {
    action = Some(SystemAction::BootSystem(SystemChoice::Atari800));
}
```

**Step 3: ROM names in `system_roms.rs`:**
```rust
const ATARI800_OS_NAMES:    &[&str] = &["atariosb.rom", "atarixl.rom", "os.rom", "ATARIBAS.ROM"];
const ATARI800_BASIC_NAMES: &[&str] = &["ataribas.rom", "basic.rom", "ATARIBAS.ROM"];

pub fn load_atari800_roms(dir: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let subdir = dir.join("atari800");
    let find = |names: &[&str]| -> Option<Vec<u8>> {
        for n in names {
            if let Some(d) = try_load(&subdir.join(n)) { return Some(d); }
            if let Some(d) = try_load(&dir.join(n))    { return Some(d); }
        }
        None
    };
    (find(ATARI800_OS_NAMES), find(ATARI800_BASIC_NAMES))
}
```

**Step 4: Wire in `app.rs`** (boot + load ROM), following the C64 pattern exactly.

**Step 5: Build and commit:**
```sh
cargo build --workspace
git add crates/atari8bit/ crates/frontend/
git commit -m "feat(frontend): register Atari 800 system"
```

---

## Final Verification

```sh
cargo test --workspace
cargo build --workspace
git push
```

### Manual Test Checklist
- [ ] Boot Atari 800 → BASIC prompt ("READY" on blue screen)
- [ ] Load a .com/.car cartridge → starts correctly
- [ ] POKEY audio: `SOUND 0,100,10,10` plays a tone
- [ ] Debugger → POKEY and ANTIC panels display correctly

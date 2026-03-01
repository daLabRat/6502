# C64 1541 Drive Emulation, Critical Audit Fixes, Debug Cleanup

## Context

The NES mapper expansion plan (previous contents) is **COMPLETE** — all 13 mappers and DMC audio are implemented and committed. This plan covers three remaining work areas:

1. **Full 1541 disk drive emulation** — Replace the KERNAL trap-based virtual drive with a cycle-accurate 1541 running its own 6502 CPU and IEC serial bus
2. **Critical audit fixes** — Address hardware accuracy issues marked Critical across all systems
3. **Debug cleanup** — Remove diagnostic trace code from Apple II bus.rs

---

## Phase 1: Debug Cleanup (Apple II)

Remove all diagnostic/debug trace code added during Bitsy Bye debugging.

**`crates/apple2/src/lib.rs`**:
- Remove VTAB instruction trace block (static atomics, frame 508 tracing, ~25 lines)
- Keep `debug_pc`/`debug_sp`/`debug_x` assignments (needed for soft switch address tracking)

**`crates/apple2/src/bus.rs`**:
- Remove MLI call tracing at $BF00 in read() (~60 lines: call number decode, READ/OPEN param dumps)
- Remove ZP $64/$65 write trace in write() (~4 lines)
- Remove frame 550 diagnostics: framebuffer save, buffer dump, path dump, ROM dump, CSW/KSW dump (~40 lines)
- Remove frame 600 diagnostics: code dump, ZP dump, text page hex dump, aux RAM dump, framebuffer save (~50 lines)
- Remove disk seek logging (keep motor on/off if desired)
- Remove `debug_sp` and `debug_x` fields if no longer needed after cleanup
- Remove 80-col firmware logging in `$C300` slot 3 handler

---

## Phase 2: C64 Critical Audit Fixes

### 2A: VIC-II Graphics Modes

**`crates/c64/src/vic_ii/mod.rs`** — Rewrite `render_scanline()`:

Current: Only standard 40×25 character mode. Must add:

1. **Multicolor Character Mode** (MCM=1, BMM=0): 4×8 pixel cells, 2 bits/pixel, 4 colors from color RAM + $D022/$D023/$D021
2. **Bitmap Mode** (BMM=1, MCM=0): 320×200, each bit = pixel, colors from screen RAM (hi=fg, lo=bg per 8×8 cell)
3. **Multicolor Bitmap Mode** (BMM=1, MCM=1): 160×200, 2 bits/pixel, 4 colors per 4×8 cell
4. **Extended Color Mode** (ECM=1): Character mode but char code upper 2 bits select from 4 background colors ($D021-$D024)

Mode bits: `$D011` bit 5 (BMM), bit 6 (ECM); `$D016` bit 4 (MCM)

### 2B: VIC-II Bank Selection from CIA2

**`crates/c64/src/vic_ii/mod.rs`** + **`crates/c64/src/bus.rs`**:

- Pass CIA2 Port A bits 0-1 to VIC-II during rendering (inverted: `!bits & 3`)
- Bank 0=$0000, 1=$4000, 2=$8000, 3=$C000
- All VIC-II memory fetches (screen, char, bitmap, sprite) offset by bank × $4000
- Character ROM visible only in banks 0 and 2 (at $1000-$1FFF relative to bank base)

### 2C: VIC-II Sprite Rendering

**`crates/c64/src/vic_ii/mod.rs`**:

- Add sprite rendering pass after background, respecting priority
- 8 sprites, each 24×21 pixels (expandable 2× in X and Y)
- Sprite pointers at screen_base + $03F8-$03FF
- Sprite data: 3 bytes per row × 21 rows = 63 bytes per sprite
- Registers: $D000-$D00E (X/Y), $D010 (X bit 8), $D015 (enable), $D017 (Y-expand), $D01B (priority), $D01C (multicolor), $D01D (X-expand), $D025-$D026 (multicolor colors), $D027-$D02E (sprite colors)
- Collision detection: $D01E (sprite-sprite), $D01F (sprite-background), cleared on read

### 2D: VIC-II Scrolling

**`crates/c64/src/vic_ii/mod.rs`**:

- Apply X scroll ($D016 bits 0-2): shift pixel output right by 0-7 pixels
- Apply Y scroll ($D011 bits 0-2): offset character row start by 0-7 lines
- 38-column mode ($D016 bit 3): narrow display, border covers columns 0 and 39
- 24-row mode ($D011 bit 3): narrow display, border covers rows 0 and 24

### 2E: VIC-II Badline CPU Stalling

**`crates/c64/src/vic_ii/mod.rs`** + **`crates/c64/src/bus.rs`**:

- Detect badlines: display on AND (raster_line & 7) == ($D011 & 7)
- On badline: VIC-II steals ~40 cycles from CPU
- Add `stall_cycles: u8` field to VIC-II, checked in `bus.tick()` to skip CPU cycles
- Approach: `tick()` returns extra stall cycles; bus subtracts from CPU budget

### 2F: SID Improvements

**`crates/c64/src/sid/mod.rs`**:

1. **Noise LFSR**: Replace XOR hack with proper 23-bit Galois LFSR (polynomial x²³+x¹⁸+1). Add `noise_lfsr: u32` per voice, shift on frequency counter tick
2. **Combined waveforms**: When multiple waveform bits set (bits 4-7 of control reg), AND the individual waveform outputs together instead of returning 0
3. **Ring modulation** (bit 3): XOR voice's triangle MSB with previous voice's accumulator MSB
4. **Hard sync** (bit 2): Reset voice accumulator to 0 when previous voice accumulator overflows. Track overflow with `prev_msb` per voice
5. **Filter**: Implement 12 dB/octave state-variable filter (lowpass/bandpass/highpass/notch). Cutoff from $D415-$D416 (11-bit), resonance from $D417 bits 4-7, mode from $D418 bits 4-6, voice routing from $D417 bits 0-2

---

## Phase 3: NES Critical Audit Fix

### 3A: PPU Odd Frame Skip

**`crates/nes/src/ppu/mod.rs`**:

- On pre-render scanline (-1), if `frame_count` is odd AND rendering enabled: skip cycle 339 (go from 338 directly to 0 of scanline 0)
- Small change (~5 lines) in the cycle/scanline increment logic

Note: Batch PPU tile fetching (the other NES critical item) is a major architectural change to scanline-buffer rendering. Deferring to a separate plan since it touches the core rendering pipeline and risks regressions across all mappers.

---

## Phase 4: Atari 2600 Critical Audit Fixes

### 4A: HMOVE Blanks

**`crates/atari2600/src/tia/mod.rs`**:

- Add `hmove_pending: bool` and `hmove_blanking: u8` to TIA
- On HMOVE write ($2A): set `hmove_pending = true`
- At start of next scanline: if pending, set `hmove_blanking = 8`
- During first 8 visible pixels: force black output when `hmove_blanking > 0`, decrement counter

### 4B: Player/Missile Position Pipeline Delay

**`crates/atari2600/src/tia/mod.rs`**:

- RESP0/RESP1/RESM0/RESM1/RESBL writes: add 4-5 color clock delay before position takes effect
- Add `resp0_delay: u8` etc. fields, buffer pending position
- Decrement in `step_clock()`, apply when counter reaches 0

---

## Phase 5: C64 1541 Full Drive Emulation

### Architecture

The 1541 is essentially a standalone computer with its own 6502, 2KB RAM, 16KB ROM, and two VIA 6522 chips. It communicates with the C64 over the IEC serial bus (3 wires: ATN, CLK, DATA).

### 5A: VIA 6522 Module

**New file: `crates/c64/src/via.rs`** (~200 lines)

- Port A/B data and direction registers
- Timer 1 & 2 (16-bit, free-running and one-shot modes)
- Shift register (serial I/O)
- Interrupt flag register (IFR) and interrupt enable register (IER)
- Handshake control (CA1/CA2, CB1/CB2 active edge selection)
- Similar to existing CIA but with different register layout and features
- Reusable for both VIA1 and VIA2 in the 1541

### 5B: IEC Bus

**New file: `crates/c64/src/iec_bus.rs`** (~50 lines)

- Shared state: `atn: bool`, `clk: bool`, `data: bool` (active low, open collector)
- C64 side (CIA2 Port A): bits 3/4/5 drive ATN/CLK/DATA out; bits 6/7 read CLK/DATA in
- 1541 side (VIA1 Port A/B): reads ATN/CLK/DATA, drives CLK/DATA
- Open-collector: line is LOW if ANY device pulls it low (OR logic, inverted)
- The bus is the shared communication channel between C64 and 1541

### 5C: Drive 1541 Bus

**New file: `crates/c64/src/drive1541/bus.rs`** (~150 lines)

Implements `emu_common::Bus` for the 1541's internal address space:

```
$0000-$07FF: 2KB RAM
$1800-$180F: VIA1 (IEC bus interface)
$1C00-$1C0F: VIA2 (drive mechanics)
$C000-$FFFF: 16KB ROM
```

- `tick()`: steps VIA1 and VIA2 timers, updates IEC bus state, handles disk rotation timing
- `poll_irq()`: checks VIA1/VIA2 IRQ lines
- Disk rotation: track byte position advances based on cycle count and zone speed

### 5D: Drive 1541 GCR + Disk Mechanics

**New file: `crates/c64/src/drive1541/mod.rs`** (~300 lines)

- GCR encode/decode: 4-bit nybble → 5-bit GCR (lookup table, 16 entries)
- D64 to GCR conversion: convert raw 256-byte sectors to GCR-encoded track data
- Track data includes: sync bytes, header (track/sector/checksum), data block, gap bytes
- Bytes per track by zone: tracks 1-17=7692, 18-24=7142, 25-30=6666, 31-35=6250
- Head stepping: 4-phase stepper motor via VIA2 Port B bits 0-1, ~8ms per half-track
- Motor control: VIA2 Port B bit 2 (motor on), bit 4 (write protect sense)
- Read head: VIA2 Port A reads current GCR byte from track at current position
- Byte-ready signal: VIA2 CA1 interrupt when new byte available from disk

### 5E: Integration into C64

**Modified: `crates/c64/src/lib.rs`**

```rust
pub struct C64 {
    cpu: Cpu6502<C64Bus>,
    drive_cpu: Option<Cpu6502<Drive1541Bus>>,  // None if no disk loaded
    iec_bus: IecBus,                            // Shared IEC bus state
    // ...
}
```

- In `step_frame()`: run both CPUs in lockstep. For each C64 CPU cycle, run ~1 drive CPU cycle (both at ~1 MHz)
- After each C64 tick: sync CIA2 Port A → IEC bus → VIA1 Port A
- Keep `KernalDrive` as fallback when no 1541 ROM is available

**Modified: `crates/c64/src/bus.rs`**

- CIA2 Port A reads: merge IEC bus input lines (bits 6-7) from `iec_bus`
- CIA2 Port A writes: push output lines (bits 3-5) to `iec_bus`

**Modified: `crates/frontend/src/system_roms.rs`**

- Add 1541 ROM detection: look for `1541.rom`, `dos1541`, `325302-01.bin` in `roms/c64/`

### 5F: Construction Methods

**Modified: `crates/c64/src/lib.rs`**

- `from_d64()`: if 1541 ROM found, create `Drive1541Bus` + second `Cpu6502`, load D64 into drive's GCR track buffer. If no 1541 ROM, fall back to KERNAL traps (current behavior)
- Keep KERNAL trap path as default until 1541 ROM is provided

---

## New Files (4)
- `crates/c64/src/via.rs` — VIA 6522 implementation
- `crates/c64/src/iec_bus.rs` — IEC serial bus shared state
- `crates/c64/src/drive1541/mod.rs` — 1541 drive mechanics + GCR
- `crates/c64/src/drive1541/bus.rs` — 1541 internal address space (Bus trait)

## Modified Files
- `crates/apple2/src/bus.rs` — Remove debug traces
- `crates/apple2/src/lib.rs` — Remove VTAB trace
- `crates/c64/src/vic_ii/mod.rs` — Graphics modes, sprites, scrolling, bank selection, badlines
- `crates/c64/src/sid/mod.rs` — LFSR noise, combined waveforms, ring mod, sync, filter
- `crates/c64/src/bus.rs` — CIA2↔IEC bus, VIC bank passing, badline stall
- `crates/c64/src/lib.rs` — Drive CPU integration, dual-CPU step loop
- `crates/nes/src/ppu/mod.rs` — Odd frame skip
- `crates/atari2600/src/tia/mod.rs` — HMOVE blanks, RESP pipeline delay
- `crates/frontend/src/system_roms.rs` — 1541 ROM loading

## Execution Order

1. **Phase 1** (debug cleanup) — Quick, no risk
2. **Phase 2A-2D** (VIC-II modes/sprites/scroll/bank) — Biggest visual impact
3. **Phase 2E** (badline stalling) — Timing correctness
4. **Phase 2F** (SID improvements) — Audio quality
5. **Phase 3** (NES odd frame skip) — Small, isolated
6. **Phase 4** (Atari 2600 HMOVE/RESP) — Small, isolated
7. **Phase 5A-5C** (VIA, IEC bus, drive bus) — 1541 foundation
8. **Phase 5D-5F** (GCR/mechanics, integration) — 1541 completion

## Verification

After each phase:
1. `cargo build --workspace` — 0 errors, 0 warnings
2. `cargo test --workspace` — all tests pass
3. Phase 1: Apple II ProDOS/Bitsy Bye still works
4. Phase 2: C64 games display correctly (test multicolor, bitmap, sprites)
5. Phase 2F: SID music sounds better (filter, noise, ring mod)
6. Phase 5: `LOAD"$",8` and `LOAD"*",8,1` work via real 1541 emulation

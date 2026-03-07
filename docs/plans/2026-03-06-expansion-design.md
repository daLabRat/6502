# emu6502 Expansion Design
**Date:** 2026-03-06
**Status:** Approved

## Goals

- Personal hobby / learning — depth and accuracy over breadth
- Actually playable — games work correctly, good UX, save states, controller support
- Accuracy / preservation — passing test ROMs, hardware-documented behavior
- Portfolio is a side effect, not a primary goal

## Architecture

Single application, single Cargo workspace. The `Bus` and `SystemEmulator` traits in `common` are CPU-agnostic — new CPU architectures (`CpuZ80`, `Cpu68000`) slot in as new crates alongside `Cpu6502`. The frontend never touches the CPU directly, only `SystemEmulator`. No separate applications per CPU family.

```
crates/
  common/        Bus + SystemEmulator traits (CPU-agnostic)
  cpu-6502/      MOS 6502 (existing)
  cpu-z80/       Zilog Z80 (Slice 6)
  cpu-68000/     Motorola 68000 (Slice 7)
  nes/           (existing)
  apple2/        (existing)
  c64/           (existing)
  atari2600/     (existing)
  vic20/         (Slice 5A)
  atari8bit/     Atari 400/800/5200 (Slice 5B)
  atari7800/     (Slice 5C)
  bbcmicro/      (Slice 5D)
  pet/           (Slice 5E)
  zxspectrum/    (Slice 6)
  amiga/         (Slice 7)
  frontend/      egui/eframe UI (existing, extended)
```

## Shared Infrastructure (built in Slice 1, extended throughout)

### CRT Shader Pipeline

The RGBA8 `FrameBuffer` is uploaded to a wgpu texture each frame. A WGSL post-process pass runs before egui composites to screen. Three user-selectable presets (per-system default, user-overridable):

- **Sharp** — nearest-neighbor, no effects (current behavior)
- **Scanlines** — horizontal darkening every other line, phosphor glow
- **CRT** — barrel distortion, RGB aperture mask, scanlines, bloom, chromatic aberration

Shader uniforms include source resolution for correct scanline scaling at any window size. Reference: `crt-lottes` / `crt-royale` GLSL → WGSL port.

### Debugger Window

Separate OS window via `egui::Context::show_viewport_deferred()`. Independently movable to any monitor. Panels:

**Always present:**
- CPU registers, flags, stack pointer, cycle count
- Live disassembler centered on PC (±20 instructions), click to set breakpoint
- Memory hex viewer with address input and ASCII sidebar
- Breakpoints list (address, condition, enable/disable)
- Step / Step Over / Run / Pause controls
- Execution trace log (last N instructions, toggleable)

**System-specific tab** — injected via `SystemEmulator::debugger_ui(&mut self, ui: &mut egui::Ui)`:

| System | Panels |
|--------|--------|
| C64 | VIC-II registers + mode preview, SID envelope per voice, CIA timers, 1541 drive state (track/sector/motor) |
| NES | PPU tile viewer (pattern tables + palette), nametable viewer (4 quadrants + scroll overlay), OAM sprite viewer, APU channel oscilloscope |
| Apple II | Soft switch state grid, video mode indicator, Disk II track/phase/motor |
| Atari 2600 | TIA register grid with descriptions, scanline/pixel position, RIOT timer |
| VIC-20 | VIC register viewer, memory map |
| Atari 400/800/5200 | POKEY register state, GTIA/CTIA mode, ANTIC DMA state |
| Atari 7800 | MARIA DMA list, TIA audio state, 2600 compatibility mode indicator |
| BBC Micro | 6845 CRTC registers, ULA mode, VIA state, SN76489 channel state |
| PET | 6545 CRTC state, PIA port state, IEEE-488 bus state |
| ZX Spectrum | ULA border/attributes, tape state, AY registers (128K) |
| Amiga | Copper list viewer, Blitter state, Paula channels, Agnus DMA |

`SystemEmulator` trait addition:
```rust
fn debugger_ui(&mut self, ui: &mut egui::Ui) {} // default no-op
fn cpu_state(&self) -> CpuDebugState;            // registers, PC, SP, flags
fn peek_memory(&self, addr: u16) -> u8;          // side-effect-free read
fn disassemble(&self, addr: u16) -> (String, u16); // instruction + next addr
```

### Save States

Full system state serialized via `serde` + `rmp-serde` (MessagePack — fast, compact). Four slots per ROM, stored in `saves/<system>/<rom_hash>/slot[0-3].bin`. Quicksave F5 / Quickload F7. Slot picker in debugger window and main menu.

Each system struct derives `serde::Serialize/Deserialize`. External state (open file handles, audio ring buffer) is excluded and re-initialized on load.

### ROM Browser

Thumbnail grid replacing the flat file picker. Remembers last directory per system. Shows system icon, filename, and last-played date. Drag-and-drop ROM loading.

---

## Vertical Slices

### Slice 1 — C64 Plays a Game
**Goal:** Load a D64, run a game start-to-finish, look great doing it.

- Verify IEC/1541 end-to-end: byte receive, directory listing, PRG load, `RUN`
- CIA2 fast serial if needed for loader compatibility
- VIC-II sprite collision flags, raster interrupt accuracy
- CRT shader (all three presets) — **shared infrastructure built here**
- Debugger window — **shared infrastructure built here**
- Save states — **shared infrastructure built here**
- ROM browser polish

### Slice 2 — NES Sounds Great
**Goal:** APU complete, games with sampled audio (Contra, SMB3) sound correct.

- APU DMC: DMA sample fetch, interrupt, memory reader
- APU sweep unit negative-negate accuracy
- Mappers: 9 (MMC2/Punch-Out), 10 (MMC4), 19 (Namco 163), 24/26 (VRC6), FDS
- Debugger NES tab: tile viewer, nametable viewer, OAM viewer, APU oscilloscope

### Slice 3 — Apple II Boots a Disk
**Goal:** Read/write Disk II, Mockingboard audio, 80-column text.

- Disk II write support (encode sectors back to NIB/DSK)
- 80-column card (double hi-res, 80-col text mode)
- Mockingboard audio (two AY-3-8910 chips, slot 4)
- Debugger Apple II tab

### Slice 4 — Atari 2600 Sounds Right
**Goal:** TIA audio synthesis complete, all collision bits correct.

- TIA audio: AUDC/AUDF/AUDV → polynomial counter waveform synthesis
- TIA collision registers (all 15 collision bits)
- Bank switching: E0 (Parker Bros 8K), FE, 3F (Tigervision)
- Debugger Atari 2600 tab

### Slice 5A — VIC-20
**Goal:** Boot to BASIC prompt, load cartridges and tape images.

- New `vic20` crate: VIC 6560/6561 video (text + multicolor), 6522 VIA (reuse existing), IEC bus (reuse existing)
- Cartridge (`.prg`, `.crt`) and tape (`.tap`) loading
- Debugger VIC-20 tab

### Slice 5B — Atari 400/800/5200
**Goal:** Atari 800 boots to BASIC, loads cartridges and ATR disk images; 5200 plays a game.

- New `atari8bit` crate: POKEY audio/keyboard, GTIA/CTIA video, ANTIC DMA
- ATR disk image support, standard cartridge formats
- 5200 variant: analog controller input, no internal BASIC
- Debugger Atari 8-bit tab

### Slice 5C — Atari 7800
**Goal:** Play a 7800 game; load and play a 2600 game via backward compatibility.

- New `atari7800` crate: MARIA video chip (DMA-driven sprite engine)
- TIA audio reused from `atari2600` crate
- 2600 backward compatibility mode (detect cart type, switch to TIA video)
- Debugger Atari 7800 tab

### Slice 5D — BBC Micro
**Goal:** Boot to BASIC prompt, load `.ssd`/`.dsd` disk images, play an Acornsoft game.

- New `bbcmicro` crate: 6845 CRTC + ULA video (8 video modes), SN76489 PSG audio
- 6522 VIA reused from existing implementation (BBC has two VIAs)
- 8271 FDC for disk images
- Debugger BBC Micro tab

### Slice 5E — Commodore PET
**Goal:** Boot to BASIC, run BASIC programs.

- New `pet` crate: 6545 CRTC, 40×25 character-mapped display
- 6520 PIA × 2, 6522 VIA, IEEE-488 bus stub
- Cassette tape (`.tap`) loading
- Debugger PET tab

### Slice 6 — ZX Spectrum
**Goal:** Load a `.TAP`/`.TZX` game, play it.

- New `cpu-z80` crate: `CpuZ80<B: Bus>` — full Z80 instruction set, IX/IY, DJNZ, block ops
- New `zxspectrum` crate: ULA (border, attributes, flash), 48K RAM/ROM model
- TAP/TZX tape loading, AY-3-8910 audio (128K model)
- Debugger ZX Spectrum tab

### Slice 7 — Amiga 500
**Goal:** Boot Workbench, run an ADF disk demo.

- New `cpu-68000` crate: `Cpu68000<B: Bus>` — full 68000 instruction set
- New `amiga` crate: Paula (4-channel audio + disk DMA), Denise (sprites/playfields), Agnus (Copper/Blitter/DMA scheduler)
- ADF disk image support
- Debugger Amiga tab: Copper list viewer, Blitter state, Paula channels

---

## Open Questions / Future

- **Game Boy**: sits between Slices 6 and 7 — modified Z80 (LR35902) + PPU. Natural follow-on to Slice 6.
- **MSX**: Z80 + TMS9918 video + AY audio — shares Z80 crate from Slice 6.
- **Fast loaders**: Many C64 games use custom fast loaders over the IEC bus — may need per-loader compatibility shims.
- **Netplay**: Long-term; save state infrastructure makes rollback netcode feasible.
- **Mobile / WASM**: eframe supports WASM targets; worth exploring after Slice 2.

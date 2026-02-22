# CLAUDE.md — Development Guide

## Build & Test

```sh
# Default build (no system lib deps)
cargo build --workspace

# Full build with audio + gamepad
cargo build --workspace --features "emu-frontend/audio,emu-frontend/gamepad"

# Run all tests
cargo test --workspace

# Run including slow integration tests
cargo test --workspace -- --ignored
```

The workspace must build with **0 warnings**. Fix any warnings before committing.

## Project Layout

7-crate Cargo workspace. The CPU crate is generic over `Bus` — each system provides its own bus implementation that owns all hardware.

```
crates/common/src/       — Bus trait, SystemEmulator trait, FrameBuffer, input types
crates/cpu/src/          — Cpu6502<B: Bus>, opcode table, addressing modes
crates/cpu/src/tests/    — CPU unit tests (30 tests)
crates/nes/src/          — NesBus, PPU (ppu/), APU (apu/), mappers (cartridge/mapper/)
crates/apple2/src/       — Apple2Bus, video modes (video/), keyboard, speaker
crates/c64/src/          — C64Bus, VIC-II (vic_ii/), SID (sid/), CIA (cia/)
crates/atari2600/src/    — AtariBus, TIA (tia/), RIOT
crates/frontend/src/     — egui app, audio output, input handling, ROM loading
```

## Key Design Conventions

- **Bus owns hardware**: CPU never touches PPU/APU directly. CPU → `bus.tick(cycles)` → hardware components.
- **`peek()` is side-effect-free**: Use for debugger/inspector reads. `read()` may trigger hardware side effects.
- **`bcd_enabled` flag**: `false` for NES (2A03 lacks BCD), `true` for Apple II/C64/Atari 2600.
- **FrameBuffer is RGBA8**: Emulation crates stay GPU-agnostic; the frontend handles display.
- **Feature gates**: `audio` (cpal) and `gamepad` (gilrs) are optional to avoid system lib deps in default builds.

## PPU Rendering (NES)

Scanline-buffer architecture:
1. `fill_bg_scanline_buffer()` at cycle 0 — pre-fetches 33 tiles into pixel/palette buffers
2. `fill_sprite_scanline_buffer()` at cycle 257 — decodes sprites into sprite buffers
3. `render_pixel()` at cycles 1-256 — reads from buffers, applies priority multiplexer
4. `increment_x()` at cycles 8, 16, ..., 248 in `step()` — keeps `v` register correct
5. `increment_y()` at cycle 256, horizontal copy at cycle 257

## Adding a New NES Mapper

1. Create `crates/nes/src/cartridge/mapper/mapperN.rs` implementing the `Mapper` trait
2. Add the match arm in `crates/nes/src/cartridge/mod.rs` (`from_ines()`)
3. Implement `cpu_read`, `cpu_write`, `ppu_read`, `ppu_write`, `mirroring`
4. If scanline-counting: implement `scanline_tick()` and `irq_pending()`

## Style

- No `unsafe` unless absolutely necessary
- Prefer explicit bit manipulation over abstractions for hardware registers
- Use `pub(crate)` for internal fields exposed across modules within a crate
- Keep emulation crates free of frontend/platform dependencies

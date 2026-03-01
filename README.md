# emu6502 — Multi-System 6502 Emulator

A cross-platform emulator for four classic 6502-based systems, built in Rust with an egui frontend.

## Supported Systems

| System | CPU | Video | Audio | Storage |
|--------|-----|-------|-------|---------|
| **NES** | 2A03 (6502 sans BCD) | PPU scanline renderer, 256×240 | Pulse ×2, Triangle, Noise, DMC | iNES cartridge, mappers 0–4+ |
| **Apple II** | 6502 | Text 40×24, Lo-Res, Hi-Res 280×192 | 1-bit speaker | Disk II (NIB/DSK) |
| **Commodore 64** | 6510 | VIC-II text/bitmap/multicolor, sprites | SID 3-voice (ADSR, filter) | D64 via 1541 drive emulation |
| **Atari 2600** | 6507 (6502 variant) | TIA 160×192 NTSC | — | Cartridge, F8/F6 bank switching |

## Building

Default build (no system library dependencies):

```sh
cargo build --workspace
```

With audio and gamepad support (requires `libasound2-dev` and `libudev-dev` on Linux):

```sh
sudo apt install libasound2-dev libudev-dev
cargo build --workspace --features "emu-frontend/audio,emu-frontend/gamepad"
```

### WSL2 Audio

WSLg routes audio automatically, but ALSA needs PulseAudio. Create `~/.asoundrc`:

```
pcm.default pulse
ctl.default pulse
pcm.pulse { type pulse }
ctl.pulse  { type pulse }
```

And install the required packages:

```sh
sudo apt install pulseaudio libasound2-plugins
```

Verify with `pactl info` and `speaker-test -t sine -l 1`.

## Running

```sh
cargo run -p emu-frontend
# with audio and gamepad:
cargo run -p emu-frontend --features "audio,gamepad"
```

Select a system from the start screen, then load a ROM or disk image.

## System ROMs

Place firmware ROM files in the `roms/` directory (path configurable via `system_roms_dir` in `config.ron`):

```
roms/
  c64/
    basic.rom          (8 KB)  — C64 BASIC interpreter
    kernal.rom         (8 KB)  — C64 Kernal OS
    chargen.rom        (4 KB)  — C64 character generator
    1541-c000.bin      (8 KB)  — 1541 DOS ROM (lower half, optional)
    1541-e000.bin      (8 KB)  — 1541 DOS ROM (upper half, optional)
  apple2/
    apple2plus.rom    (12 KB)  — Apple II+ firmware
```

### C64

The three system ROMs (`basic`, `kernal`, `chargen`) are required to boot to the `READY.` prompt. Alternative filenames accepted: `*.bin` extensions or original Commodore part numbers (`901226-01.bin`, `901227-03.bin`, `901225-01.bin`).

**1541 drive emulation** is activated automatically when both `1541-c000.bin` and `1541-e000.bin` are present and a D64 disk image is loaded. Without the 1541 ROM, the emulator falls back to KERNAL traps for D64 loading.

### Apple II

Needs its firmware ROM for the system monitor and Applesoft BASIC. Alternative filenames: `apple2p.rom`, `apple2.rom`, `apple2e.rom`.

### NES / Atari 2600

No system ROMs needed — all code is on the cartridge.

System ROM files are copyrighted and not included. Dump them from original hardware or obtain them through the usual retro-computing channels.

## Controls

### NES
| Key | Button |
|-----|--------|
| Arrow keys | D-Pad |
| Z | B |
| X | A |
| Enter | Start |
| Backspace | Select |

### Apple II
Full keyboard input — characters map to the Apple II keyboard latch.

### Commodore 64
Full keyboard input via CIA keyboard matrix scanning. Type `LOAD"*",8,1` to load from a mounted D64.

### Atari 2600
| Key | Action |
|-----|--------|
| Arrow keys | Joystick |
| Space | Fire |
| Enter | Reset switch |
| Backspace | Select switch |

## ROM Formats

### NES — iNES 1.0 (`.nes`)
```
[4B magic "NES\x1A"] [PRG banks] [CHR banks] [flags...] [PRG ROM] [CHR ROM]
```
- PRG ROM: 16 KB banks, CHR ROM: 8 KB banks
- 512-byte trainer auto-detected from flags
- CHR banks = 0 → 8 KB CHR RAM allocated

### Apple II — Raw binary (`.rom`, `.bin`)
- Up to 12 KB: loaded at `$D000–$FFFF`
- Up to 16 KB: loaded at `$C000–$FFFF`

### Commodore 64

**PRG** (`.prg`): 2-byte load address header followed by program data.

**D64** (`.d64`): Standard 1541 disk image (35 tracks, 683 sectors). With 1541 ROM files present, a full drive CPU runs in lockstep with the C64 over the emulated IEC serial bus. Without drive ROMs, the first PRG on the disk is loaded directly via KERNAL traps.

### Atari 2600 — Raw binary (`.a26`, `.bin`)

| Size | Banking |
|------|---------|
| 2 KB or 4 KB | None (mirrored) |
| 8 KB | F8 (hotspots `$1FF8–$1FF9`) |
| 16 KB | F6 (hotspots `$1FF6–$1FF9`) |

## Project Structure

```
crates/
  common/      Bus + SystemEmulator traits, FrameBuffer, input types
  cpu/         MOS 6502 — generic over Bus, monomorphized per system
  nes/         NES: PPU, APU, iNES cartridge, mappers
  apple2/      Apple II: video modes, keyboard, speaker, Disk II
  c64/         C64: VIC-II, SID, CIA×2, 1541 drive, IEC bus, VIA 6522
  atari2600/   Atari 2600: TIA, RIOT, cartridge
  frontend/    egui/eframe UI, audio, input, ROM/disk loading
```

### Architecture

The CPU is generic over a `Bus` trait — each system implements its own bus that owns all hardware. The CPU calls `bus.tick(cycles)` after each instruction, advancing video and audio in lockstep.

```
Cpu6502<NesBus>        — NES
Cpu6502<Apple2Bus>     — Apple II
Cpu6502<C64Bus>        — Commodore 64 (host)
Cpu6502<Drive1541Bus>  — 1541 drive (runs in lockstep with C64)
Cpu6502<AtariBus>      — Atari 2600
```

The C64 and 1541 drive each run a full 6502 core. An emulated IEC serial bus (open-collector ATN/CLK/DATA) syncs them after every instruction, including the ATN auto-acknowledge XOR circuit present on real 1541 hardware.

## NES Mapper Support

| # | Name | Notable Games |
|---|------|---------------|
| 0 | NROM | Donkey Kong, Super Mario Bros, Excitebike |
| 1 | MMC1 | Zelda, Metroid, Mega Man 2 |
| 2 | UxROM | Contra, Castlevania |
| 3 | CNROM | Arkanoid, Pac-Man |
| 4 | MMC3 | Super Mario Bros 2/3, Kirby, Mega Man 3–6 |
| 7 | AxROM | Battletoads, Marble Madness |
| 11 | Color Dreams | Crystal Mines |
| 28 | Action 53 | Multi-game |
| 34 | BxROM/NINA-001 | Deadly Towers |
| 66 | GxROM | Super Mario Bros + Duck Hunt |
| 69 | FME-7 | Sunsoft Batman |
| 71 | Camerica | Micro Machines |

## Testing

```sh
cargo test --workspace
```

40 tests: 30 CPU unit tests (all legal opcodes, addressing modes, flags, interrupts), 6 Apple II Disk II encoding tests, 4 C64 disk image tests. The Klaus Dormann functional test suite is gated behind `#[ignore]` (requires the test ROM binary).

## Known Limitations

- **Atari 2600 audio**: TIA audio synthesis not yet implemented
- **Illegal 6502 opcodes**: Mapped to JAM (halt); not emulated
- **No save states** or debugger UI yet

## License

Licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

# emu6502 — Multi-System 6502 Emulator

A cross-platform emulator for four classic 6502-based systems, built in Rust with an egui frontend.

## Supported Systems

| System | CPU | Video | Audio | Input |
|--------|-----|-------|-------|-------|
| **NES** | 2A03 (6502 sans BCD) | PPU scanline renderer, 256x240 | Pulse x2, Triangle, Noise | Standard joypad |
| **Apple II** | 6502 | Text 40x24, Lo-Res, Hi-Res 280x192 | 1-bit speaker | ASCII keyboard |
| **Commodore 64** | 6510 | VIC-II 320x200 | SID 3-voice synth | Keyboard matrix |
| **Atari 2600** | 6507 (6502 variant) | TIA 160x192 | Stub | Joystick + switches |

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

### WSL2 Audio Setup

WSL2 with WSLg should route audio automatically, but ALSA needs to be told to use PulseAudio. If you hear no sound, create `~/.asoundrc`:

```
pcm.default pulse
ctl.default pulse

pcm.pulse {
    type pulse
}

ctl.pulse {
    type pulse
}
```

You may also need the PulseAudio client and ALSA plugin:

```sh
sudo apt install pulseaudio libasound2-plugins
```

Verify with: `pactl info` (should show the WSLg PulseServer) and `speaker-test -t sine -l 1` (should produce a tone).

## Running

```sh
cargo run -p emu-frontend
# or with features:
cargo run -p emu-frontend --features "audio,gamepad"
```

Select a system from the start screen, then load a ROM file.

## System ROMs

Some systems require firmware ROM files to boot. Place them in the `roms/` directory (configurable via `system_roms_dir` in `config.ron`):

```
roms/
  c64/
    basic.rom        (8 KB)   — C64 BASIC interpreter
    kernal.rom       (8 KB)   — C64 Kernal OS
    chargen.rom      (4 KB)   — C64 character generator
  apple2/
    apple2plus.rom   (12 KB)  — Apple II+ firmware
```

### C64

All three ROMs are required for the C64 to boot to the `READY.` prompt. Without them, only standalone machine-code PRG files will run (no BASIC). Alternative filenames accepted: `basic.bin`, `kernal.bin`, `chargen.bin`, or the original Commodore part numbers (`901226-01.bin`, `901227-03.bin`, `901225-01.bin`).

### Apple II

The Apple II needs its firmware ROM to provide the system monitor and Applesoft BASIC. Without it, the screen will be blank. Alternative filenames: `apple2p.rom`, `apple2.rom`, `apple2e.rom`.

### NES / Atari 2600

No system ROMs needed — all code is on the cartridge.

System ROM files are copyrighted and not included. Dump them from original hardware or obtain them through the usual retro computing channels.

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
Full keyboard input via CIA keyboard matrix scanning.

### Atari 2600
| Key | Action |
|-----|--------|
| Arrow keys | Joystick |
| Space | Fire |
| Enter | Reset switch |
| Backspace | Select switch |

## ROM Formats

### NES — iNES 1.0 (`.nes`)
Standard iNES format with 16-byte header:
```
[4B magic "NES\x1A"] [PRG banks] [CHR banks] [flags...] [PRG ROM] [CHR ROM]
```
- PRG ROM: 16KB banks, CHR ROM: 8KB banks
- Optional 512-byte trainer (auto-detected from flags)
- If CHR banks = 0, 8KB CHR RAM is allocated
- Supported mappers: 0 (NROM), 1 (MMC1), 2 (UxROM), 3 (CNROM), 4 (MMC3)

### Apple II — Raw binary (`.rom`, `.bin`)
Raw ROM image loaded into the upper memory region:
- Files up to 12KB: loaded at `$D000-$FFFF`
- Files up to 16KB: loaded at `$C000-$FFFF`
- Typical: 12KB Apple II+ ROM or 16KB Apple IIe ROM

### Commodore 64 — PRG format (`.prg`)
Standard C64 PRG with a 2-byte load address header:
```
[2B load address (little-endian)] [program data...]
```
- Minimum 3 bytes (2-byte address + 1 byte data)
- Data is loaded into RAM at the specified address
- Note: BASIC/KERNAL/CHAR ROMs are not included — the C64 requires separate system ROMs for full boot

### Atari 2600 — Raw binary (`.a26`, `.bin`)
Headerless ROM image, bank switching auto-detected by file size:

| Size | Banking |
|------|---------|
| 2KB or 4KB | No switching (mirrored to fill 4KB) |
| 8KB | F8 (2 banks, hotspots at `$1FF8-$1FF9`) |
| 16KB | F6 (4 banks, hotspots at `$1FF6-$1FF9`) |

ROMs smaller than 4KB are automatically padded by mirroring.

## Project Structure

```
crates/
  common/      Shared traits (Bus, SystemEmulator), FrameBuffer, input types
  cpu/         MOS 6502 CPU — generic over Bus trait, monomorphized per system
  nes/         NES: PPU, APU, mappers 0-4, controller
  apple2/      Apple II: video modes, keyboard, speaker, soft switches
  c64/         C64: VIC-II, SID, CIA x2, memory banking
  atari2600/   Atari 2600: TIA, RIOT, cartridge
  frontend/    egui/eframe UI, audio output, input mapping, ROM loading
```

### Architecture

The CPU is generic over a `Bus` trait. Each system implements its own bus that owns all hardware components. The CPU calls `bus.tick(cycles)` after each instruction, which advances the PPU/TIA/VIC and APU in lockstep. This keeps the CPU crate system-agnostic while giving each system full control over cycle-accurate synchronization.

```
Cpu6502<NesBus>    — NES
Cpu6502<Apple2Bus> — Apple II
Cpu6502<C64Bus>    — Commodore 64
Cpu6502<AtariBus>  — Atari 2600
```

## NES Mapper Support

| # | Name | Notable Games |
|---|------|---------------|
| 0 | NROM | Donkey Kong, Super Mario Bros, Excitebike |
| 1 | MMC1 | Zelda, Metroid, Mega Man 2 |
| 2 | UxROM | Contra, Castlevania |
| 3 | CNROM | Arkanoid, Pac-Man |
| 4 | MMC3 | Super Mario Bros 2/3, Kirby, Mega Man 3-6 |

## Testing

```sh
cargo test --workspace
```

30 CPU unit tests cover all legal opcodes, addressing modes, flags, and interrupts. The Klaus Dormann functional test suite is gated behind `#[ignore]` (requires the test ROM binary).

## Known Limitations

- **NES APU DMC**: Sample playback channel is stubbed
- **Atari 2600 audio**: TIA audio synthesis not yet implemented
- **Illegal 6502 opcodes**: Mapped to JAM (halt); not emulated
- **No save states** or debugger UI yet

## License

This project is for personal/educational use.

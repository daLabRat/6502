# System ROMs

Place system firmware/BIOS ROM files here. The emulator looks for them
automatically when loading a game for that system.

## C64 (`roms/c64/`)

The Commodore 64 requires three system ROMs to boot:

| File | Size | Description | Original chip |
|------|------|-------------|---------------|
| `basic.rom` | 8 KB | BASIC interpreter | 901226-01 |
| `kernal.rom` | 8 KB | Kernal OS | 901227-03 |
| `chargen.rom` | 4 KB | Character generator | 901225-01 |

Without these, the C64 won't boot to a READY prompt. Standalone
machine-code PRG files may still run without them.

## Apple II (`roms/apple2/`)

| File | Size | Description |
|------|------|-------------|
| `apple2plus.rom` | 12 KB | Apple II+ firmware ROM |

Alternative names accepted: `apple2p.rom`, `apple2.rom`, `apple2e.rom`

Without the system ROM, the Apple II has no monitor or BASIC — the
screen will be blank.

## NES / Atari 2600

These systems have no firmware ROM — all code is on the cartridge.
No files needed here.

## Where to get system ROMs

System ROM files are copyrighted and cannot be distributed with the
emulator. You can dump them from original hardware or find them through
the usual retro computing channels.

# Next Steps: Disk Drive Support (Apple II & C64) + T64 Tape Support

## Context

The multi-system 6502 emulator has all core systems working (NES, Apple II, C64, Atari 2600) with video, audio, and input. The next major feature gap is **storage**: no system can load software from disk images. Currently Apple II only boots from system ROM, and C64 only supports direct PRG injection. Adding disk support unlocks the vast library of .dsk (Apple II) and .d64 (C64) software. T64 tape images are a quick win since they're just PRG containers.

## Implementation Phases

### Phase B: C64 T64 Tape Loader (simplest — do first)

**New file**: `crates/c64/src/t64_loader.rs`

T64 is a container format: 64-byte header + 32-byte directory entries + raw data. Parse the header (magic starts with "C64"), iterate directory entries (entry_used==1, file_type==1 for PRG), extract the first usable file using `tape_offset` field, build standard PRG (2-byte load addr + payload). Feed result into existing `load_prg()`.

Key detail: `end_addr` field is often wrong in real T64 files — calculate size from offset differences between entries, or from file size for the last entry.

**Modify**: `crates/c64/src/lib.rs` — add `pub mod t64_loader;` and `from_t64()` constructor that calls `extract_first_prg()` then `from_rom()`.

### Phase A: Apple II Disk II Controller

**New file**: `crates/apple2/src/disk_ii.rs`

The Disk II controller sits in slot 6 with I/O at $C0E0-$C0EF and boot ROM at $C600-$C6FF.

**DiskII struct fields**:
- `nibble_data: Vec<[u8; 6656]>` — 35 pre-nibblized tracks
- `current_track: u8`, `byte_position: usize` — head position + rotation
- `motor_on: bool`, `phase_states: [bool; 4]`, `phase_position: u8` (0..68 half-tracks)
- `data_latch: u8`, `write_mode: bool`, `disk_loaded: bool`
- `boot_rom: [u8; 256]` — P5 PROM loaded from `diskII.c600.c6ff.bin`
- `cycle_accumulator: u32` — for nibble timing (~32 CPU cycles per nibble)

**I/O switches** ($C0E0-$C0EF, low 4 bits):
- 0x0-0x7: Phase motors (even=off, odd=on) — step head via phase sequencing
- 0x8/0x9: Motor off/on
- 0xA/0xB: Drive select 1/2
- 0xC: Data latch read (Q6L) — **critical**: returns next nibble from track
- 0xD: Write load (Q6H)
- 0xE/0xF: Shift/load mode (Q7)

**Head stepping**: Track phase_position (0-68 half-tracks), current_track = phase_position/2. When a phase magnet activates, check if it's adjacent to current phase and step toward it.

**GCR Nibblization** (`nibblize_track()`): Convert each 256-byte sector into GCR-encoded nibble stream:
1. Sync bytes (0xFF gap)
2. Address field: prologue $D5 $AA $96, 4-and-4 encoded volume/track/sector/checksum, epilogue $DE $AA $EB
3. Data field: prologue $D5 $AA $AD, 6-and-2 encoded 342 nibbles (86 aux + 256 primary, XOR-chained), checksum, epilogue $DE $AA $EB

DOS 3.3 sector interleave: `[0,13,11,9,7,5,3,1,14,12,10,8,6,4,2,15]`

6-and-2 encoding: Split each byte into high 6 bits (primary) and low 2 bits (packed 3-per-byte into 86 auxiliary bytes). XOR-chain encode, then translate through 64-entry WRITE_TABLE to valid disk nibbles (all have bit 7 set).

**`step(cycles)`**: Advance byte_position based on cycle count (~32 cycles per nibble byte).

**Modify**: `crates/apple2/src/bus.rs`
- Add `pub disk_ii: DiskII` field to `Apple2Bus`
- Route $C0E0-$C0EF reads/writes to `disk_ii.io_read()`/`io_write()`
- Route $C600-$C6FF reads to `disk_ii.read_rom()`
- Call `disk_ii.step(cycles)` in `tick()`

**Modify**: `crates/apple2/src/lib.rs`
- Add `pub mod disk_ii;`
- Add `with_disk(system_rom, disk_rom, dsk_data)` constructor

**Existing assets**: `roms/apple2/diskII.c600.c6ff.bin` (P5 PROM), `roms/apple2/DOS33_2.DSK`, `U4boot.dsk`

### Phase C: C64 D64 Virtual Drive (KERNAL Trap Approach)

Rather than emulating the full 1541 drive (separate 6502 CPU + VIA chips + GCR + IEC serial bus), intercept KERNAL ROM calls when the CPU's PC hits specific addresses. This works for all standard LOAD/SAVE operations.

**New file**: `crates/c64/src/d64_image.rs`

D64 parser: 174,848 bytes = 683 sectors across 35 tracks (variable: 21/19/18/17 sectors per track zone). Track 18 sector 0 = BAM, sectors 1+ = directory (8 entries × 32 bytes per sector). File data stored as linked sector chains (bytes 0-1 of each sector = next track/sector, $00 = last sector).

Key methods: `read_directory()`, `read_file(track, sector)` (follow chain), `find_and_read_file(name)`, `load_first_prg()`.

**New file**: `crates/c64/src/kernal_traps.rs`

KernalDrive struct holds: `d64: Option<D64Image>`, file parameters (logical_file, device_number, secondary_addr), filename buffer, file_buffer + position for open files.

Trap addresses intercepted (only when device_number == 8):
- `$FFBA` SETLFS — store A=logical, X=device, Y=secondary
- `$FFBD` SETNAM — store filename from pointer in X/Y, length in A
- `$FFD5` LOAD — find file in D64, load into RAM, update BASIC pointers if $0801
- `$FFC0` OPEN — find file, buffer its data
- `$FFC3` CLOSE — clear buffer
- `$FFC6` CHKIN — acknowledge
- `$FFCF` BASIN/CHRIN — return next byte from buffer, set ST ($90) EOI on last byte

After handling each trap, simulate RTS: pull return address from stack, set PC = addr+1.

**CPU change needed**: `crates/cpu/src/cpu.rs` — change `pub(crate) fn pull()` to `pub fn pull()` and same for `push()`. Needed for RTS simulation from the KERNAL trap handler.

**Modify**: `crates/c64/src/lib.rs`
- Add modules: `pub mod d64_image;`, `pub mod kernal_traps;`
- Add `kernal_drive: KernalDrive` field to `C64` struct (sibling of cpu, not inside bus)
- Add `from_d64()` constructor
- Modify `step_frame()`: before `cpu.step()`, check `kernal_drive.check_trap(cpu.pc, &mut cpu)` — if true, skip the cpu.step() and tick bus nominally instead

### Phase D: Frontend Integration

**Modify**: `crates/frontend/src/app.rs`
- In `load_rom()`, detect file extension before system dispatch:
  - `.dsk` for Apple II → `Apple2::with_disk(sys_rom, disk_rom, data)`
  - `.d64` for C64 → `C64::from_d64(data)` + load_system_roms
  - `.t64` for C64 → `t64_loader::extract_first_prg(data)` → `C64::from_rom(prg)`
- Update file dialog filters: add "dsk" to Apple II, add "d64"/"t64" to C64

**Modify**: `crates/frontend/src/system_roms.rs`
- Add `load_disk_ii_rom()` — search for diskII.c600.c6ff.bin in roms/apple2/

## Files Summary

| New Files | Purpose |
|-----------|---------|
| `crates/c64/src/t64_loader.rs` | T64 tape image parser |
| `crates/apple2/src/disk_ii.rs` | Disk II controller + GCR nibblization |
| `crates/c64/src/d64_image.rs` | D64 disk image parser |
| `crates/c64/src/kernal_traps.rs` | KERNAL trap handler for virtual drive |

| Modified Files | Changes |
|----------------|---------|
| `crates/cpu/src/cpu.rs` | Make `push()`/`pull()` pub |
| `crates/apple2/src/lib.rs` | Add disk_ii module, `with_disk()` constructor |
| `crates/apple2/src/bus.rs` | Add DiskII field, route $C0E0-$C0EF + $C600-$C6FF, call step() |
| `crates/c64/src/lib.rs` | Add d64/t64/kernal_traps modules, KernalDrive field, new constructors, trap check in step_frame |
| `crates/frontend/src/app.rs` | File extension routing for .dsk/.d64/.t64, updated dialog filters |
| `crates/frontend/src/system_roms.rs` | Add load_disk_ii_rom() |

## Implementation Order

1. **Phase B** (T64) — simplest, self-contained
2. **Phase A** (Disk II) — most complex (GCR nibblization) but architecturally straightforward
3. **Phase C** (D64 + KERNAL traps) — moderate complexity, requires CPU pub change
4. **Phase D** (Frontend) — done incrementally with each phase

## Verification

1. `cargo build --workspace` — 0 errors, 0 warnings after each phase
2. `cargo test --workspace` — all 30 CPU tests pass
3. **T64**: Load a .t64 file for C64 → extracts PRG → boots and runs after `RUN`
4. **Apple II Disk**: Load DOS33_2.DSK → P5 ROM boots from $C600 → DOS 3.3 loads → `]` prompt
5. **C64 D64**: Boot C64 with .d64 mounted → type `LOAD"*",8,1` → KERNAL trap loads PRG → type `RUN`

# Next Steps: Disk Drive Support, T64, & Apple II 80-Column

## Context

The multi-system 6502 emulator has all core systems working (NES, Apple II, C64, Atari 2600) with video, audio, and input. The next major feature gaps are **storage** (no disk image loading) and **display** (Apple II lacks 80-column mode). Adding disk support unlocks the .dsk (Apple II) and .d64 (C64) software libraries. T64 tape images are a quick win (PRG containers). 80-column support brings the Apple II up to IIe capabilities.

## Implementation Phases

### Phase 1: C64 T64 Tape Loader (simplest — do first)

**New file**: `crates/c64/src/t64_loader.rs`

T64 is a container format: 64-byte header + 32-byte directory entries + raw data. Parse the header (magic starts with "C64"), iterate directory entries (entry_used==1, file_type==1 for PRG), extract the first usable file using `tape_offset` field, build standard PRG (2-byte load addr + payload). Feed result into existing `load_prg()`.

Key detail: `end_addr` field is often wrong in real T64 files — calculate size from offset differences between entries, or from file size for the last entry.

**Modify**: `crates/c64/src/lib.rs` — add `pub mod t64_loader;`

**Modify**: `crates/frontend/src/app.rs` — add "t64"/"T64" to C64 file dialog filter, detect `.t64` extension and call `t64_loader::extract_first_prg()` → feed to `C64::from_rom()`

### Phase 2: Apple II 80-Column Support

Adds IIe-style extended 80-column card: 64KB auxiliary RAM, banking soft switches, and 80-column text rendering. Framebuffer widens from 280px to 560px (all modes render at double width for consistency).

#### 2a: Auxiliary RAM + Soft Switches

**Modify `crates/apple2/src/soft_switch.rs`**:
Add new flags with defaults:
- `store80: bool` (false) — $C000/$C001: 80STORE off/on
- `ramrd: bool` (false) — $C002/$C003: RAMRD off/on
- `ramwrt: bool` (false) — $C004/$C005: RAMWRT off/on
- `altzp: bool` (false) — $C008/$C009: ALTZP off/on
- `col80: bool` (false) — $C00C/$C00D: 80COL off/on
- `altcharset: bool` (false) — $C00E/$C00F: ALTCHARSET off/on

Add `handle_iie(&mut self, addr: u16)` method for $C000-$C00F (even=off, odd=on pattern).

Add `read_status(&self, addr: u16) -> u8` for read-only status at $C011-$C01F. Returns bit 7 set if flag active:
- $C013: RAMRD, $C014: RAMWRT, $C016: ALTZP
- $C018: 80STORE, $C01A: TEXT, $C01B: MIXED
- $C01C: PAGE2, $C01D: HIRES, $C01E: ALTCHARSET, $C01F: 80COL

**Modify `crates/apple2/src/memory.rs`**:
Add auxiliary memory fields to `Memory`:
- `aux_ram: [u8; 49152]` — 48KB auxiliary RAM ($0000-$BFFF)
- `aux_lc_ram: [u8; 16384]` — aux language card RAM
- `aux_lc_bank2: [u8; 4096]` — aux language card bank 2

Add direct-access methods for video rendering (bypass banking):
- `read_main_text(addr: u16) -> u8` — always reads from main `ram`
- `read_aux_text(addr: u16) -> u8` — always reads from `aux_ram`
- `read_main_hires(addr: u16) -> u8` / `read_aux_hires(addr: u16) -> u8`

#### 2b: Bus Banking Logic

**Modify `crates/apple2/src/bus.rs`**:

Rework `read()` and `write()` to check soft switches for memory banking:

```
$0000-$01FF: ALTZP → aux_ram, else main ram
$0200-$03FF: RAMRD/RAMWRT → aux_ram, else main ram
$0400-$07FF: if 80STORE → PAGE2 selects main/aux
             else RAMRD/RAMWRT → aux_ram
$0800-$1FFF: RAMRD/RAMWRT → aux_ram
$2000-$3FFF: if 80STORE && HIRES → PAGE2 selects main/aux
             else RAMRD/RAMWRT → aux_ram
$4000-$BFFF: RAMRD/RAMWRT → aux_ram
```

Add soft switch routing for $C000-$C00F:
- Writes to $C000-$C00F → `switches.handle_iie(addr)`
- Reads from $C011-$C01F → `switches.read_status(addr)`

#### 2c: 560px Framebuffer + 80-Column Text

**Modify `crates/apple2/src/video/mod.rs`**:
- Change `DISPLAY_WIDTH` from 280 to 560
- Pass `switches` to all render functions (they already receive it)

**Modify `crates/apple2/src/video/text.rs`**:
- **40-column mode** (`col80 == false`): Render each character 14px wide (double every pixel column). 40 × 14 = 560px.
- **80-column mode** (`col80 == true`): For each of the 80 columns:
  - Even columns (0,2,4...): read character from `memory.read_aux_text(addr + col/2)`
  - Odd columns (1,3,5...): read character from `memory.read_main_text(addr + col/2)`
  - Render each character 7px wide. 80 × 7 = 560px.
- Update `render_lores()`: double pixel width (14px per block cell) for 560px.

**Modify `crates/apple2/src/video/hires.rs`**:
- Double each bit horizontally: each pixel → 2px wide. 40 bytes × 7 bits × 2 = 560px.

**Modify `crates/apple2/src/bus.rs`**:
- Update `FrameBuffer::new()` call to use new 560px width.

**Modify `crates/apple2/src/lib.rs`**:
- Update `display_width()` to return 560.

### Phase 3: Apple II Disk II Controller

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
- 0xC: Data latch read (Q6L) — returns next nibble from track
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

**Modify**: `crates/frontend/src/app.rs` — add "dsk"/"DSK" to Apple II file filter, detect `.dsk` and load with Disk II ROM
**Modify**: `crates/frontend/src/system_roms.rs` — add `load_disk_ii_rom()` for `diskII.c600.c6ff.bin`

**Existing assets**: `roms/apple2/diskII.c600.c6ff.bin`, `roms/apple2/DOS33_2.DSK`, `U4boot.dsk`

### Phase 4: C64 D64 Virtual Drive (KERNAL Trap Approach)

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

**CPU change needed**: `crates/cpu/src/cpu.rs` — change `pub(crate) fn pull()` to `pub fn pull()` and same for `push()`.

**Modify**: `crates/c64/src/lib.rs`
- Add modules: `pub mod d64_image;`, `pub mod kernal_traps;`
- Add `kernal_drive: KernalDrive` field to `C64` struct
- Add `from_d64()` constructor
- Modify `step_frame()`: check `kernal_drive.check_trap()` before `cpu.step()`

**Modify**: `crates/frontend/src/app.rs` — add "d64"/"D64" to C64 file filter, detect `.d64` and use `C64::from_d64()`

## Files Summary

| New Files | Purpose |
|-----------|---------|
| `crates/c64/src/t64_loader.rs` | T64 tape image parser |
| `crates/apple2/src/disk_ii.rs` | Disk II controller + GCR nibblization |
| `crates/c64/src/d64_image.rs` | D64 disk image parser |
| `crates/c64/src/kernal_traps.rs` | KERNAL trap handler for virtual drive |

| Modified Files | Changes |
|----------------|---------|
| `crates/apple2/src/soft_switch.rs` | Add IIe flags (store80, ramrd, ramwrt, altzp, col80, altcharset), handle_iie(), read_status() |
| `crates/apple2/src/memory.rs` | Add 64KB aux RAM, direct-access methods for video |
| `crates/apple2/src/bus.rs` | Banking logic in read/write, $C000-$C00F switches, $C011-$C01F status reads, DiskII routing, 560px framebuffer |
| `crates/apple2/src/video/mod.rs` | DISPLAY_WIDTH → 560 |
| `crates/apple2/src/video/text.rs` | 80-col rendering (interleaved main/aux), 40-col doubled to 560px |
| `crates/apple2/src/video/hires.rs` | Double pixels horizontally for 560px |
| `crates/apple2/src/lib.rs` | display_width() → 560, disk_ii module, with_disk() constructor |
| `crates/cpu/src/cpu.rs` | Make push()/pull() pub |
| `crates/c64/src/lib.rs` | Add d64/t64/kernal_traps modules, KernalDrive field, new constructors |
| `crates/frontend/src/app.rs` | File extension routing for .dsk/.d64/.t64, updated dialog filters |
| `crates/frontend/src/system_roms.rs` | Add load_disk_ii_rom() |

## Implementation Order

1. **Phase 1** (T64) — simplest, self-contained, ~30 min
2. **Phase 2** (80-col) — aux RAM + soft switches + video widening
3. **Phase 3** (Disk II) — GCR nibblization, most complex single component
4. **Phase 4** (D64 + KERNAL traps) — requires CPU pub change
5. Frontend integration done incrementally with each phase

## Verification

1. `cargo build --workspace` — 0 errors, 0 warnings after each phase
2. `cargo test --workspace` — all 30 CPU tests pass
3. **T64**: Load a .t64 file for C64 → extracts PRG → boots and runs
4. **80-col**: Boot Apple II → should display 40-col text at 560px wide (doubled pixels). Software that activates 80-col (via $C00D) should display 80 columns.
5. **Apple II Disk**: Load DOS33_2.DSK → P5 ROM boots from $C600 → DOS 3.3 loads → `]` prompt
6. **C64 D64**: Boot C64 with .d64 mounted → type `LOAD"*",8,1` → KERNAL trap loads PRG → `RUN`

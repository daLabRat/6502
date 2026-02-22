# Multi-System 6502 Emulator in Rust

## Context

Build a multi-system 6502 emulator from scratch supporting NES, Apple II, C64, and Atari 2600. Uses egui for the GUI frontend. Starts instruction-accurate with architecture ready for cycle-accuracy upgrade. Gameplay-first focus - debug tools come later.

## Cargo Workspace Layout

```
/mnt/unraid/CLAUDE/6502/
├── Cargo.toml                  # Virtual manifest (no src/)
├── .gitignore
├── crates/
│   ├── common/src/             # Shared traits and types
│   │   ├── lib.rs, bus.rs      #   Bus trait (read/write/peek/tick/poll_nmi/poll_irq)
│   │   ├── system.rs           #   SystemEmulator trait (step_frame/framebuffer/audio/input)
│   │   ├── framebuffer.rs      #   RGBA8 framebuffer → egui ColorImage bridge
│   │   ├── input.rs            #   InputEvent + Button enum (superset of all systems)
│   │   └── audio.rs            #   Audio sample types
│   ├── cpu/src/                # 6502 CPU core, generic over Bus
│   │   ├── cpu.rs              #   Cpu6502<B: Bus> - registers, step(), reset(), nmi(), irq()
│   │   ├── opcodes.rs          #   256-entry opcode table (mnemonic, mode, cycles)
│   │   ├── addressing.rs       #   12 addressing modes + resolution logic
│   │   ├── instructions.rs     #   All legal instruction implementations
│   │   ├── flags.rs            #   StatusFlags bitfield (N/V/B/D/I/Z/C)
│   │   └── tests/              #   Klaus Dormann test, nestest log comparison
│   ├── nes/src/                # NES system
│   │   ├── nes.rs, bus.rs      #   NES struct + NesBus (memory map routing)
│   │   ├── ppu/                #   2C02 PPU (registers, scanline renderer, palette)
│   │   ├── apu/                #   APU (2 pulse, triangle, noise, DMC, frame counter)
│   │   ├── cartridge/          #   iNES parser + Mapper trait
│   │   │   └── mapper/         #   NROM(0), MMC1(1), UxROM(2), CNROM(3), MMC3(4)
│   │   └── controller.rs       #   Joypad shift register emulation
│   ├── apple2/src/             # Apple II system
│   │   ├── bus.rs, memory.rs   #   48KB RAM + language card banking
│   │   ├── soft_switch.rs      #   $C000-$C0FF I/O switches
│   │   ├── video/              #   Text (40x24), Lo-Res (40x48), Hi-Res (280x192)
│   │   ├── speaker.rs          #   1-bit toggle audio ($C030)
│   │   └── keyboard.rs         #   Keyboard latch
│   ├── c64/src/                # Commodore 64 system
│   │   ├── bus.rs, memory.rs   #   64KB RAM + PLA bank switching (BASIC/KERNAL/CHAR/IO)
│   │   ├── vic_ii/             #   VIC-II (text/bitmap/multicolor modes, sprites, raster IRQ)
│   │   ├── sid/                #   SID (3 voices, ADSR, resonant filter)
│   │   ├── cia/                #   CIA1 (keyboard, joystick, timers) + CIA2 (VIC bank, NMI)
│   │   └── rom_loader.rs       #   .PRG format loader
│   ├── atari2600/src/          # Atari 2600 system
│   │   ├── bus.rs              #   13-bit address masking (6507)
│   │   ├── tia/                #   TIA - racing the beam (playfield, players, missiles, ball)
│   │   ├── riot.rs             #   RIOT (128B RAM, timers, I/O)
│   │   └── cartridge.rs        #   4KB standard + F8 bank switching
│   └── frontend/src/           # egui GUI application
│       ├── main.rs             #   eframe::run_native() entry point
│       ├── app.rs              #   EmuApp: screen state, emulation loop, texture updates
│       ├── screens/            #   SystemSelect (4 system buttons) + Emulation (game display)
│       ├── audio.rs            #   cpal output stream + rtrb ring buffer
│       ├── input.rs            #   Keyboard (egui) + gamepad (gilrs) → InputEvent mapping
│       ├── menu.rs             #   Menu bar (File/System/Options)
│       └── config.rs           #   Settings persistence (RON format)
```

## Key Design Decisions

1. **CPU generic over Bus** (`Cpu6502<B: Bus>`) - monomorphized per system, no vtable overhead on billions of memory accesses
2. **Bus owns all hardware** (PPU, APU, etc.) - avoids `Rc<RefCell<>>` borrow-checker pain; CPU→Bus→components chain
3. **Bus.tick(cycles)** called after each CPU instruction - synchronization hook where each system advances its components (NES: 3 PPU cycles per CPU cycle, etc.)
4. **Bus.peek()** for side-effect-free reads - future debug tools won't corrupt emulator state
5. **FrameBuffer as RGBA8 Vec** - emulation crates stay GPU-agnostic; only frontend knows about egui/wgpu
6. **Scanline-accurate PPU first** - covers ~95% of NES games; architecture supports cycle-accuracy upgrade later
7. **rtrb ring buffer for audio** - wait-free (no mutexes) between emulation thread and cpal audio callback thread
8. **bcd_enabled flag on CPU** - same CPU code for all systems; NES sets false, others set true

## Implementation Phases

### Phase 1: CPU Core + Infrastructure
Build and fully test the 6502 before touching any system.

1. Create workspace root `Cargo.toml` + `.gitignore`
2. `crates/common` - All shared traits (Bus, SystemEmulator, FrameBuffer, InputEvent)
3. `crates/cpu/src/flags.rs` - StatusFlags bitfield
4. `crates/cpu/src/addressing.rs` - 12 addressing mode enum + address resolution
5. `crates/cpu/src/opcodes.rs` - 256-entry opcode table (all legal opcodes; illegal = JAM)
6. `crates/cpu/src/instructions.rs` - All instruction implementations grouped by category
7. `crates/cpu/src/cpu.rs` - Cpu6502 struct: new(), reset(), step(), nmi(), irq(), stack ops
8. **Test**: Klaus Dormann 6502 functional test (PC reaches $3469 = all pass)
9. **Test**: nestest.nes log comparison (official opcodes, ~5000 lines match)

### Phase 2: NES System (First Playable Target)
NES has the best test ROM ecosystem - ideal first system.

1. `crates/nes/src/cartridge/ines.rs` - iNES header parser
2. `crates/nes/src/cartridge/mapper/nrom.rs` - Mapper 0 (supports DK, Ice Climber, Excitebike)
3. `crates/nes/src/ppu/` - PPU registers, palette, scanline renderer (background + sprites + scroll)
4. `crates/nes/src/bus.rs` - NesBus memory map ($0000-$07FF RAM, $2000 PPU, $4000 APU/IO, $8000+ ROM)
5. `crates/nes/src/controller.rs` - Joypad shift register ($4016/$4017)
6. `crates/nes/src/nes.rs` - NES struct implementing SystemEmulator
7. **Test**: Donkey Kong title screen renders correctly (static background)
8. **Test**: Super Mario Bros is playable (scrolling + sprites + input)

### Phase 3: GUI Frontend
Connect emulation to a real window.

1. `crates/frontend/src/main.rs` - eframe entry point (800x600 window, wgpu backend)
2. `crates/frontend/src/app.rs` - EmuApp: TextureHandle created once, updated each frame via `.set()`
3. `crates/frontend/src/screens/system_select.rs` - 4-button system picker
4. `crates/frontend/src/screens/emulation.rs` - Game texture display (nearest-neighbor scaling)
5. `crates/frontend/src/input.rs` - Keyboard mapping (arrows=dpad, Z=B, X=A, Enter=Start) + gilrs gamepad
6. `crates/frontend/src/menu.rs` - File→Load ROM (rfd native dialog), Reset, Quit
7. **Test**: Load NES ROM via dialog, play SMB at 60fps with keyboard

### Phase 4: Audio
Add sound to NES.

1. `crates/frontend/src/audio.rs` - cpal output stream + rtrb ring buffer (4096 sample capacity)
2. `crates/nes/src/apu/` - Frame counter, 2 pulse channels, triangle, noise, DMC stub, mixer
3. **Test**: SMB has recognizable music and sound effects

### Phase 5: Apple II
Second system - very different architecture tests the abstraction.

1. `crates/apple2/src/bus.rs` + `memory.rs` - 48KB RAM + ROM + language card banking
2. `crates/apple2/src/soft_switch.rs` - $C000-$C0FF I/O handling
3. `crates/apple2/src/video/text.rs` - 40x24 text mode
4. `crates/apple2/src/video/hires.rs` - Hi-Res 280x192
5. `crates/apple2/src/keyboard.rs` + `speaker.rs`
6. **Test**: Boot to `]` BASIC prompt (requires user-provided Apple II+ ROM)

### Phase 6: Commodore 64
Most complex system - VIC-II + SID + CIA.

1. `crates/c64/src/bus.rs` + `memory.rs` - PLA bank switching (BASIC/KERNAL/CHAR/IO overlays)
2. `crates/c64/src/vic_ii/` - Text mode first, then bitmap, sprites, raster IRQ
3. `crates/c64/src/sid/` - 3 voices (oscillator + ADSR + filter)
4. `crates/c64/src/cia/` - CIA1 keyboard matrix + timers, CIA2 VIC bank select
5. `crates/c64/src/rom_loader.rs` - .PRG format (2-byte load address + data)
6. **Test**: Boot to blue `READY.` screen (requires user-provided KERNAL/BASIC/CHAR ROMs)

### Phase 7: Atari 2600
Most unique architecture - no framebuffer, racing the beam.

1. `crates/atari2600/src/bus.rs` - 13-bit address masking for 6507
2. `crates/atari2600/src/riot.rs` - 128B RAM, timers, I/O ports
3. `crates/atari2600/src/tia/` - Playfield, players, missiles, ball, collision detection, WSYNC
4. `crates/atari2600/src/cartridge.rs` - 4KB standard + F8 bank switching
5. **Test**: Combat or Pitfall displays correctly

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| eframe | 0.30 | egui application framework (wgpu backend) |
| egui | 0.30 | Immediate-mode GUI |
| rfd | 0.15 | Native file dialogs |
| gilrs | 0.11 | Gamepad input |
| cpal | 0.15 | Audio output |
| rtrb | 0.3 | Lock-free ring buffer for audio |
| bitflags | 2.6 | CPU status flags |
| serde | 1.0 | Config serialization |
| ron | 0.8 | Config file format |
| log + env_logger | 0.4 / 0.11 | Logging |

## Verification Plan

- `cargo test --workspace` runs all non-ROM-dependent tests
- ROM-dependent tests gated behind `#[ignore]` with instructions
- Klaus Dormann test: CPU passes all legal opcode tests (PC trapped at $3469)
- nestest.nes: Log matches reference for ~5000 lines of official opcodes
- Visual milestones: DK title → SMB playable → SMB with audio → Apple II prompt → C64 prompt → Atari 2600 game

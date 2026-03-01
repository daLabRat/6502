# NES Mapper Expansion + DMC Audio

## Context

The NES emulator currently supports 5 mappers (0, 1, 2, 3, 4) covering ~35-40% of the licensed NES library. Adding 8 more mappers and implementing DMC audio will significantly expand compatibility. User also has urgent D64/Apple II disk bugs to fix first.

## Phase 0: Prerequisite — `ppu_read(&mut self)`

MMC2/MMC4 need side effects during PPU reads (latch switching). Change trait signature:

**`crates/nes/src/cartridge/mapper/mod.rs`**: `fn ppu_read(&self,` → `fn ppu_read(&mut self,`; add `fn cpu_tick(&mut self) {}` default method

**5 existing mapper files**: change `ppu_read(&self,` → `ppu_read(&mut self,` (nrom, mmc1, uxrom, cnrom, mmc3)

**`crates/nes/src/ppu/mod.rs`**: Change `&dyn Mapper` → `&mut dyn Mapper` in: `ppu_read()`, `read_register()`, `fill_bg_scanline_buffer()`, `fill_sprite_scanline_buffer()`, `evaluate_sprites()`

**`crates/nes/src/bus.rs`**: `.as_ref()` → `.as_mut()` for mapper in PPU read calls; add `self.cartridge.mapper.cpu_tick()` in tick loop

## Phase 1: Simple Mappers (7, 34, 66, 11, 71)

Each ~35-50 lines. All follow existing patterns (modulo bank counts, `.get().copied().unwrap_or(0)`).

| Mapper | File | PRG | CHR | Mirroring |
|--------|------|-----|-----|-----------|
| 7 AxROM | `axrom.rs` | 32KB switch (bits 0-2) | CHR RAM | Single-screen (bit 4) |
| 34 BNROM | `bnrom.rs` | 32KB switch (full val) | CHR RAM | Fixed |
| 66 GxROM | `gxrom.rs` | 32KB (bits 4-5) | 8KB (bits 0-1) | Fixed |
| 11 Color Dreams | `color_dreams.rs` | 32KB (bits 0-1) | 8KB (bits 4-7) | Fixed |
| 71 Camerica | `camerica.rs` | 16KB switch at $C000+ (UxROM-style) | CHR RAM | $9000 bit 4 single-screen |

## Phase 2: CHR-Latching Mappers (9, 10)

**Mapper 9 MMC2** (`mmc2.rs`): 8KB PRG at $8000, 3 fixed banks. Two CHR latches (FD/FE) per 4KB half. Latch triggers: $0FD8, $0FE8 (left), $1FD8-$1FDF, $1FE8-$1FEF (right). Latch updates AFTER read.

**Mapper 10 MMC4** (`mmc4.rs`): Same as MMC2 but 16KB PRG at $8000, last bank fixed at $C000.

## Phase 3: Mapper 69 FME-7 (`fme7.rs`)

Command/parameter architecture ($8000=command, $A000=data). Regs 0-7: 1KB CHR. Reg 8: $6000 PRG/RAM. Regs 9-11: 8KB PRG. Reg 12: mirroring. Regs 13-15: cycle-counting IRQ. Uses `cpu_tick()` trait method.

## Phase 4: DMC Audio

**`crates/nes/src/apu/dmc.rs`** — Full rewrite: rate table, timer, shift register, sample buffer, DMA request pattern (`dma_request: Option<u16>`), loop/IRQ support.

**`crates/nes/src/apu/mod.rs`** — Add `dmc.tick()` in step(), include DMC in mix formula (`dmc/22638.0` in tnd_out), DMC status bits in read_status(), DMC IRQ in irq_pending().

**`crates/nes/src/bus.rs`** — After `apu.step()`, check `apu.dmc.dma_request.take()`, read byte from RAM/cartridge, call `apu.dmc.receive_dma_byte()`.

## New Files (8)
- `crates/nes/src/cartridge/mapper/{axrom,bnrom,gxrom,color_dreams,camerica,mmc2,mmc4,fme7}.rs`

## Modified Files
- `crates/nes/src/cartridge/mapper/mod.rs` — trait + modules + create() match arms
- `crates/nes/src/cartridge/mapper/{nrom,mmc1,uxrom,cnrom,mmc3}.rs` — ppu_read &mut self
- `crates/nes/src/ppu/mod.rs` — &dyn Mapper → &mut dyn Mapper (5 signatures)
- `crates/nes/src/apu/{mod.rs,dmc.rs}` — DMC implementation + mix/status/IRQ
- `crates/nes/src/bus.rs` — DMC DMA + cpu_tick()

## Verification
1. `cargo build --workspace` — 0 errors, 0 warnings after each phase
2. `cargo test --workspace` — all tests pass
3. Test mapper 7 ROM (Battletoads), mapper 9 ROM (Punch-Out), etc.

# Fix NES PPU Rendering, OAM DMA, Frame Pacing, and Apple II Font

## Context

The initial multi-system emulator build is complete and compiles with 0 warnings, but the NES PPU has critical rendering bugs that prevent games from displaying correctly. The background renderer does per-pixel random-access VRAM reads instead of walking tiles in a scanline buffer, fine_x scrolling is broken, coarse X increment is in the wrong place, OAM DMA doesn't stall the CPU, the frontend has no frame pacing (runs at unlimited FPS), and the Apple II text mode has empty font data.

## Bugs and Fixes

### 1. Rewrite PPU Background Rendering — Scanline Buffer Approach
**File**: `crates/nes/src/ppu/mod.rs`, `crates/nes/src/ppu/renderer.rs`

**Problem**: `get_bg_pixel()` (renderer.rs:75-105) does 3 VRAM reads per pixel (nametable, attribute, pattern) and uses `self.v` directly. This is wrong — `v` is the *internal* address register that gets modified during the scanline, not a per-pixel lookup. The fine_x calculation at line 101 (`(pixel_in_tile + fine_x) % 8`) doesn't implement fine_x correctly — fine_x is an offset into a tile-pair, not a per-tile offset.

**Fix**: Add scanline buffers to PPU struct and pre-fetch tiles at the right times:

- Add to `Ppu` struct:
  - `bg_pixel_buffer: [u8; 264]` — 33 tiles × 8 pixels = 264 pixels per scanline (need extra for fine_x offset)
  - `bg_palette_buffer: [u8; 264]` — palette index for each pixel
  - `sprite_pixel_buffer: [u8; 256]` — sprite pixel for each column
  - `sprite_palette_buffer: [u8; 256]` — sprite palette for each column
  - `sprite_priority_buffer: [bool; 256]` — behind-BG flag per column
  - `sprite_zero_buffer: [bool; 256]` — sprite-0 flag per column

- Add `fill_bg_scanline_buffer(&mut self, mapper)` called at **cycle 0** of visible scanlines (0-239):
  - Save current `self.v`, then walk 33 tiles:
    - Read nametable byte: `nt_addr = 0x2000 | (v & 0x0FFF)`
    - Read attribute byte: `attr_addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)`
    - Compute palette index from attribute quadrant
    - Read two pattern table planes at `pattern_base + tile * 16 + fine_y`
    - Decode 8 pixels, write into `bg_pixel_buffer[tile_offset..tile_offset+8]`
    - Call `increment_x()` to advance to next tile
  - Restore `self.v` after buffer fill (the real v advances are handled by the step pipeline)

- Add `fill_sprite_scanline_buffer(&mut self, mapper)` called at **cycle 257** after `evaluate_sprites()`:
  - Clear all sprite buffers to 0/false
  - For each sprite in `sprite_scanline` (reverse order for priority):
    - Compute row within sprite, handle flip_v
    - Fetch two pattern planes
    - For each of 8 pixels: if non-transparent, write to `sprite_pixel_buffer[sp_x + col]` etc.
    - First (i==0) sprite found sets `sprite_zero_buffer[x] = true`

- Rewrite `render_pixel()` to be a simple buffer read:
  - `bg_color = bg_pixel_buffer[(cycle - 1 + fine_x) as usize]` — fine_x shifts into the tile pair
  - `bg_palette = bg_palette_buffer[(cycle - 1 + fine_x) as usize]`
  - `sp_color = sprite_pixel_buffer[x]`, `sp_palette = sprite_palette_buffer[x]`, etc.
  - Apply priority multiplexer (same logic as current)
  - Look up palette RAM and write pixel to framebuffer

- **Remove** `get_bg_pixel()` and `get_sprite_pixel()` from renderer.rs — replaced by buffer approach

### 2. Fix Coarse X / Y Increment Timing
**File**: `crates/nes/src/ppu/mod.rs` (step function), `crates/nes/src/ppu/renderer.rs`

**Problem**: `increment_x()` is called inside `render_pixel()` at every 8th cycle (renderer.rs:66-71). This is wrong — it should be in the `step()` pipeline. Also, the bg buffer pre-fetch handles its own X walking separately.

**Fix**:
- Remove the `increment_x()` call from `render_pixel()` entirely (lines 63-71)
- In `step()`, for visible scanlines (0-239), add X/Y increment timing in the fetch pipeline:
  - Cycles 8, 16, 24, ... 248: `increment_x()` (every 8 cycles, 8 fetches per tile)
  - Cycle 256: `increment_y()` (already there)
  - Cycle 257: copy horizontal bits from t→v (already there)
  - Since we pre-fill the bg buffer at cycle 0, these increments keep `v` correct for the *next* scanline's fetch at cycle 0+1

### 3. Fix Sprite Evaluation — Overflow Flag
**File**: `crates/nes/src/ppu/mod.rs`

**Problem**: `evaluate_sprites()` (line 320-338) stops at 8 sprites but never sets the sprite overflow flag (bit 5 of PPUSTATUS).

**Fix**: After finding 8 sprites, continue scanning remaining OAM entries. If any additional sprite is in range, set `self.status |= 0x20` (sprite overflow). Note: the real NES has a hardware bug in overflow evaluation, but the simple version (just set the flag) is correct enough for 99% of games.

### 4. Fix Sprite 0 Hit Detection
**File**: `crates/nes/src/ppu/renderer.rs`

**Problem**: Sprite 0 hit check at line 39 doesn't verify rendering is enabled. Should only trigger when both BG and sprites are enabled.

**Fix**: Add `self.rendering_enabled()` check: `if sp_is_zero && x < 255 && self.rendering_enabled()`. Since this is inside the `mask & 0x10 != 0` / `mask & 0x08 != 0` checks it's actually already guarded — but the explicit `x != 255` check is correct. Additionally, sprite 0 hit should not trigger at `x == 255` (already handled) but there's also no left-clip check — when left-8-pixel clipping is active for either BG or sprites, sprite 0 hit shouldn't fire in x < 8. Add: `if sp_is_zero && x < 255 && !(x < 8 && (self.mask & 0x02 == 0 || self.mask & 0x04 == 0))`.

### 5. OAM DMA CPU Cycle Stalling
**File**: `crates/nes/src/bus.rs`

**Problem**: `tick()` (line 92-121) performs OAM DMA instantly without consuming CPU cycles. Real NES DMA takes 513 cycles (514 if starting on odd cycle) during which the CPU is halted.

**Fix**:
- Add `oam_dma_cycles_remaining: u16` field to `NesBus`
- In `write()` at $4014: set `oam_dma_pending = true` as now
- In `tick()`: when `oam_dma_pending` is true:
  - Perform the 256-byte copy (existing code)
  - Set `oam_dma_cycles_remaining = 513`
  - Set pending to false
- Modify `tick()` return or add a `dma_stall(&mut self) -> u16` method that the CPU calls
- **Simpler approach**: Add `pub fn dma_cycles(&mut self) -> u16` to Bus trait or NesBus. In `Cpu6502::step()`, after `bus.tick(total)`, check for DMA stall and add those cycles. Or: change `Bus::tick()` to return extra stall cycles as `u16`, add to total.
- **Simplest approach**: In `NesBus::tick()`, when DMA fires, run the extra 513×3 PPU cycles and 513 APU cycles right there, and return the stall as extra. Since `tick()` currently returns `()`, we keep it void and instead add the stall cycles to a `stall_cycles` field that `step()` in cpu.rs reads and adds.

**Chosen approach**: Add `pub stall_cycles: u16` to `NesBus`. In `tick()`, when OAM DMA fires, set `stall_cycles = 513`, run the PPU/APU for those extra cycles. In `Cpu6502::step()`, after `bus.tick(total)`, check `bus.stall_cycles`, add it to total, advance PPU/APU accordingly, reset to 0. Actually this is tricky with generics. Simpler: just advance PPU/APU for 513 extra cycles inside `tick()` when DMA fires. The CPU "sees" the stall because those PPU cycles already ran.

### 6. Frame Pacing
**File**: `crates/frontend/src/app.rs`

**Problem**: `ctx.request_repaint()` (line 186) requests immediate repaint, causing the emulator to run as fast as possible instead of at 60fps.

**Fix**: Replace `ctx.request_repaint()` with `ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / 60.0))`. This tells egui to repaint after ~16.67ms, giving approximately 60fps pacing. Use `sys.target_fps()` for the rate.

### 7. Apple II Character ROM Font Data
**File**: `crates/apple2/src/video/text.rs`

**Problem**: `CHAR_ROM` (line 8-17) only defines space — all other characters render as blank.

**Fix**: Populate with complete Signetics 2513 character generator data (or equivalent Apple II+ character ROM). This is 64 printable characters (uppercase A-Z, digits 0-9, symbols), each 5×7 pixels stored as 8 bytes (7 rows + 1 blank). The Apple II character set covers ASCII 0x20-0x5F. Replace the `CHAR_ROM` static with a complete font table. The `get_char_pattern()` function already indexes it correctly.

## Implementation Order

1. **PPU struct fields** — Add all scanline buffers to Ppu
2. **`fill_bg_scanline_buffer()`** — Pre-fetch 33 tiles into bg buffers
3. **`fill_sprite_scanline_buffer()`** — Pre-compute sprite pixels into buffers
4. **Rewrite `render_pixel()`** — Read from buffers, apply multiplexer
5. **Fix step() timing** — Move increment_x to step pipeline, call buffer fills at correct cycles
6. **Fix sprite evaluation** — Add overflow flag
7. **Fix sprite 0 hit** — Add left-clip check
8. **OAM DMA stalling** — Add 513 extra PPU/APU cycles in tick()
9. **Frame pacing** — `request_repaint_after()` in app.rs
10. **Apple II font** — Populate CHAR_ROM with full character data

## Files Modified

| File | Changes |
|------|---------|
| `crates/nes/src/ppu/mod.rs` | Add buffer fields, `fill_bg_scanline_buffer()`, `fill_sprite_scanline_buffer()`, fix `evaluate_sprites()` overflow, update `step()` timing |
| `crates/nes/src/ppu/renderer.rs` | Rewrite `render_pixel()` to use buffers, remove `get_bg_pixel()` and `get_sprite_pixel()`, fix sprite 0 hit |
| `crates/nes/src/bus.rs` | OAM DMA stall: run 513 extra PPU/APU cycles |
| `crates/frontend/src/app.rs` | Replace `request_repaint()` with `request_repaint_after()` |
| `crates/apple2/src/video/text.rs` | Populate `CHAR_ROM` with full Apple II character set |

## Verification

1. `cargo build --workspace` — no compile errors or warnings
2. `cargo test --workspace` — all existing CPU tests still pass
3. **Visual test**: Load Donkey Kong (mapper 0) — title screen background tiles render correctly, sprites visible
4. **Scroll test**: Load Super Mario Bros — horizontal scrolling works smoothly, no tearing or misaligned tiles
5. **Frame pacing**: Emulator runs at ~60fps, not uncapped
6. **Apple II**: Text mode shows readable characters when ROM is loaded

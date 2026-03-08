# CRT Shader Pipeline Design
**Date:** 2026-03-07
**Status:** Approved

## Goal

Add a post-process CRT shader to the emulator frontend using wgpu + egui's
PaintCallback mechanism. Three user-selectable presets per system, persisted
in config.

## Architecture

`CrtPipeline` is a self-contained module at `crates/frontend/src/crt.rs`.
It owns all wgpu resources and integrates via egui's `PaintCallback` — the
game screen is rendered by the shader into an off-screen texture, then
displayed as a normal egui image. No modification to egui's compositing
model required; can be replaced with a full post-process pass later without
changing the rest of the frontend.

## Components

### `crt.rs` — CrtPipeline struct
- Holds: WGSL render pipeline, source texture, render target texture,
  uniform buffer, bind groups, egui texture ID for render target
- `CrtPipeline::new(render_state)` — called once in `EmuApp::new()`
- `CrtPipeline::upload_frame(fb)` — uploads emulator RGBA8 framebuffer to
  source texture each frame
- `CrtPipeline::paint_callback(rect, uniforms)` → `egui::PaintCallback` —
  returned to egui for execution during render
- `CrtPipeline::output_texture_id()` → `egui::TextureId` — used by
  `ui.image()` to display result

### `crt.wgsl` — Shader
Single WGSL file with three modes selected by `uniforms.mode`:

| Mode | Name | Effects |
|------|------|---------|
| 0 | Sharp | Passthrough, nearest-neighbour sample |
| 1 | Scanlines | Darken alternate output rows by `scanline_strength` |
| 2 | CRT | Barrel distortion + scanlines + RGB aperture mask + bloom |

CRT mode detail:
- **Barrel distortion**: k coefficient bends UV coords outward, clamps to
  black outside unit square
- **Scanlines**: modulate brightness by sin²(uv.y * source_height * π)
- **RGB aperture mask**: 3-pixel repeating pattern, slight R/G/B attenuation
  per column to mimic shadow mask
- **Bloom**: 3x3 tap weighted average blended by bloom_amount

### Uniforms (bytemuck Pod, std140 layout)
```
source_size:       vec2<f32>   // emulator framebuffer pixel dimensions
output_size:       vec2<f32>   // render target pixel dimensions
mode:              u32         // 0=Sharp 1=Scanlines 2=CRT
scanline_strength: f32         // 0.0-1.0
barrel_k:          f32         // distortion coefficient (e.g. 0.1)
bloom_amount:      f32         // 0.0-1.0
_pad:              u32         // alignment
```

## Frontend Integration

- `EmuApp` gains `crt: Option<CrtPipeline>` and `crt_mode: CrtMode`
- `crt` is `None` when wgpu render state is unavailable (graceful fallback
  to existing nearest-neighbour path)
- Each frame in `Screen::Emulation`:
  1. `crt.upload_frame(fb)`
  2. `ui.painter().add(crt.paint_callback(rect, uniforms))`
  3. `ui.image(crt.output_texture_id(), size)` — display result
- Menu gains "Display" submenu: Sharp / Scanlines / CRT (cycles on click)
- `CrtMode` added to `Config`, serialised as string

## File Layout

```
crates/frontend/src/
  crt.rs          — CrtPipeline struct + wgpu setup
  crt.wgsl        — WGSL shader (embedded via include_str!)
  app.rs          — wire up CrtPipeline, CrtMode, menu item
  config.rs       — add crt_mode field
  menu.rs         — add Display submenu
```

## Non-Goals

- No per-game shader config (uniform values use global defaults per preset)
- No full swap-chain post-process (can be done later; not needed now)
- No shader hot-reload

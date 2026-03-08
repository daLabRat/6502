# CRT Shader Pipeline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a wgpu-based CRT post-process shader to the frontend with Sharp/Scanlines/CRT presets selectable from the menu.

**Architecture:** The emulator RGBA8 framebuffer is uploaded each frame to a wgpu source texture. A WGSL render pass writes the CRT-processed result to an off-screen render-target texture. That texture is registered with egui's renderer as a native texture ID and displayed via `ui.image()`. No `PaintCallback` needed — the pass runs before egui renders. `CrtPipeline` is created in `start_system()` using the system's framebuffer dimensions; recreated on each system load.

**Tech Stack:** Rust, wgpu 23, egui_wgpu 0.30, bytemuck, WGSL

---

### Task 1: Write the WGSL shader

**Files:**
- Create: `crates/frontend/src/crt.wgsl`

**Step 1: Create the shader file**

```wgsl
// crt.wgsl — CRT post-process shader.
// mode 0 = Sharp (passthrough), 1 = Scanlines, 2 = CRT

struct Uniforms {
    source_size:       vec2<f32>,
    output_size:       vec2<f32>,
    mode:              u32,
    scanline_strength: f32,
    barrel_k:          f32,
    bloom_amount:      f32,
}

@group(0) @binding(0) var src_tex:     texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(1) @binding(0) var<uniform> u:  Uniforms;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

// Full-screen triangle (3 vertices cover the clip quad without a vertex buffer)
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    var out: VertOut;
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    out.uv  = uv;
    // wgpu NDC: Y increases upward, texture UVs increase downward — flip Y
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return out;
}

fn barrel(uv: vec2<f32>, k: f32) -> vec2<f32> {
    let c  = uv * 2.0 - 1.0;
    let r2 = dot(c, c);
    return (c * (1.0 + k * r2)) * 0.5 + 0.5;
}

fn scanline(uv_y: f32, src_h: f32, strength: f32) -> f32 {
    let s = sin(uv_y * src_h * 3.14159265);
    return 1.0 - strength * (1.0 - s * s);
}

fn aperture(uv_x: f32, src_w: f32) -> vec3<f32> {
    let col = u32(uv_x * src_w) % 3u;
    if col == 0u { return vec3<f32>(1.00, 0.85, 0.85); }
    if col == 1u { return vec3<f32>(0.85, 1.00, 0.85); }
    return          vec3<f32>(0.85, 0.85, 1.00);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // Sharp: passthrough
    if u.mode == 0u {
        return textureSample(src_tex, src_sampler, uv);
    }

    // Scanlines only
    if u.mode == 1u {
        let c  = textureSample(src_tex, src_sampler, uv);
        let sl = scanline(uv.y, u.source_size.y, u.scanline_strength);
        return vec4<f32>(c.rgb * sl, c.a);
    }

    // CRT: barrel + bloom + scanlines + aperture mask
    let duv = barrel(uv, u.barrel_k);
    if duv.x < 0.0 || duv.x > 1.0 || duv.y < 0.0 || duv.y > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // 3x3 bloom
    let dx = 1.0 / u.source_size.x;
    let dy = 1.0 / u.source_size.y;
    var bloom = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            bloom += textureSample(src_tex, src_sampler,
                         duv + vec2<f32>(f32(x)*dx, f32(y)*dy)).rgb;
        }
    }
    bloom /= 9.0;

    var col = textureSample(src_tex, src_sampler, duv).rgb;
    col = mix(col, bloom, u.bloom_amount);
    col *= scanline(duv.y, u.source_size.y, u.scanline_strength);
    col *= aperture(duv.x, u.source_size.x);

    return vec4<f32>(col, 1.0);
}
```

**Step 2: No test needed** — shader is validated at compile time by wgpu when `CrtPipeline::new()` is called in Task 2.

**Step 3: Commit**

```bash
git add crates/frontend/src/crt.wgsl
git commit -m "feat: add CRT post-process WGSL shader (Sharp/Scanlines/CRT modes)"
```

---

### Task 2: CrtPipeline and CrtMode structs

**Files:**
- Create: `crates/frontend/src/crt.rs`

**Step 1: Create crt.rs**

```rust
//! CRT post-process pipeline.
//!
//! Uploads the emulator RGBA8 framebuffer to a wgpu source texture, runs
//! the `crt.wgsl` shader to an off-screen render target, and exposes the
//! result as an `egui::TextureId` for display with `ui.image()`.

use std::sync::Arc;
use eframe::egui_wgpu;
use wgpu::util::DeviceExt;

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum CrtMode {
    #[default]
    Sharp,
    Scanlines,
    Crt,
}

impl CrtMode {
    pub fn next(self) -> Self {
        match self {
            Self::Sharp     => Self::Scanlines,
            Self::Scanlines => Self::Crt,
            Self::Crt       => Self::Sharp,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Sharp     => "Sharp",
            Self::Scanlines => "Scanlines",
            Self::Crt       => "CRT",
        }
    }
    fn as_u32(self) -> u32 {
        match self { Self::Sharp => 0, Self::Scanlines => 1, Self::Crt => 2 }
    }
}

// ── Uniforms (std140 / bytemuck::Pod) ─────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    source_size:       [f32; 2],
    output_size:       [f32; 2],
    mode:              u32,
    scanline_strength: f32,
    barrel_k:          f32,
    bloom_amount:      f32,
}

impl Uniforms {
    fn new(w: u32, h: u32, mode: CrtMode) -> Self {
        Self {
            source_size:       [w as f32, h as f32],
            output_size:       [w as f32, h as f32],
            mode:              mode.as_u32(),
            scanline_strength: 0.4,
            barrel_k:          0.08,
            bloom_amount:      0.12,
        }
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

pub struct CrtPipeline {
    device:             Arc<wgpu::Device>,
    queue:              Arc<wgpu::Queue>,
    pipeline:           wgpu::RenderPipeline,
    src_texture:        wgpu::Texture,
    src_bind_group:     wgpu::BindGroup,
    uniform_buffer:     wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    render_target_view: wgpu::TextureView,
    pub texture_id:     egui::TextureId,
    width:              u32,
    height:             u32,
}

impl CrtPipeline {
    pub fn new(rs: &egui_wgpu::RenderState, width: u32, height: u32) -> Self {
        let device = &rs.device;
        let queue  = &rs.queue;

        // ── Source texture ────────────────────────────────────────────────────
        let src_texture = device.create_texture(&wgpu::TextureDescriptor {
            label:             Some("crt_src"),
            size:              wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count:   1,
            sample_count:      1,
            dimension:         wgpu::TextureDimension::D2,
            format:            wgpu::TextureFormat::Rgba8UnormSrgb,
            usage:             wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:      &[],
        });
        let src_view    = src_texture.create_view(&Default::default());
        let src_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // ── Render target ─────────────────────────────────────────────────────
        let render_target = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("crt_target"),
            size:            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8UnormSrgb,
            usage:           wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats:    &[],
        });
        let render_target_view = render_target.create_view(&Default::default());

        // Register render target with egui so we can display it with ui.image()
        let texture_id = rs.renderer.write().register_native_texture(
            device,
            &render_target_view,
            wgpu::FilterMode::Linear,
        );

        // ── Bind group layouts ────────────────────────────────────────────────
        let src_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("crt_src_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count:      None,
                },
            ],
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("crt_uniform_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty:                wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:  None,
                },
                count: None,
            }],
        });

        // ── Bind groups ───────────────────────────────────────────────────────
        let src_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("crt_src_bg"),
            layout:  &src_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&src_sampler) },
            ],
        });
        let uniform_data   = Uniforms::new(width, height, CrtMode::Sharp);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("crt_uniforms"),
            contents: bytemuck::bytes_of(&uniform_data),
            usage:    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("crt_uniform_bg"),
            layout:  &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // ── Render pipeline ───────────────────────────────────────────────────
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("crt_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("crt.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("crt_layout"),
            bind_group_layouts:   &[&src_layout, &uniform_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("crt_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module:              &shader,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:      &shader,
                entry_point: Some("fs_main"),
                targets:     &[Some(wgpu::ColorTargetState {
                    format:     wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend:      None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive:    wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        Self {
            device: device.clone(),
            queue:  queue.clone(),
            pipeline,
            src_texture,
            src_bind_group,
            uniform_buffer,
            uniform_bind_group,
            render_target_view,
            texture_id,
            width,
            height,
        }
    }

    /// Upload RGBA8 framebuffer pixels to the source texture.
    pub fn upload(&self, pixels: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture:   &self.src_texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset:          0,
                bytes_per_row:   Some(4 * self.width),
                rows_per_image:  Some(self.height),
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
    }

    /// Apply the CRT shader (source → render target). Submit immediately.
    pub fn apply(&self, mode: CrtMode) {
        self.queue.write_buffer(
            &self.uniform_buffer, 0,
            bytemuck::bytes_of(&Uniforms::new(self.width, self.height, mode)),
        );
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("crt_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &self.render_target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes:         None,
                occlusion_query_set:      None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.src_bind_group, &[]);
            pass.set_bind_group(1, &self.uniform_bind_group, &[]);
            pass.draw(0..3, 0..1); // full-screen triangle, no vertex buffer needed
        }
        self.queue.submit(std::iter::once(enc.finish()));
    }
}
```

**Step 2: Add module declaration to main.rs**

In `crates/frontend/src/main.rs`, add after the existing `mod` lines:
```rust
mod crt;
```

**Step 3: Build to validate shader and pipeline**

```bash
cargo build --release -p emu-frontend 2>&1
```

Expected: compiles with 0 errors. If the WGSL has errors they'll appear as wgpu validation panics at startup — fix them before continuing.

**Step 4: Commit**

```bash
git add crates/frontend/src/crt.rs crates/frontend/src/crt.wgsl crates/frontend/src/main.rs
git commit -m "feat: CrtPipeline wgpu module with Sharp/Scanlines/CRT shader"
```

---

### Task 3: Add CrtMode to Config

**Files:**
- Modify: `crates/frontend/src/config.rs`

**Step 1: Add crt_mode field**

Add `use crate::crt::CrtMode;` at the top, then add to the `Config` struct:
```rust
pub crt_mode: CrtMode,
```

Add to `Config::default()`:
```rust
crt_mode: CrtMode::Sharp,
```

**Step 2: Build**

```bash
cargo build --release -p emu-frontend 2>&1
```

Expected: 0 errors (serde derives on `CrtMode` handle serialization automatically).

**Step 3: Commit**

```bash
git add crates/frontend/src/config.rs
git commit -m "feat: add crt_mode to Config (persisted)"
```

---

### Task 4: Wire CrtPipeline into EmuApp

**Files:**
- Modify: `crates/frontend/src/app.rs`

**Step 1: Add imports and fields to EmuApp**

At the top of `app.rs`, add:
```rust
use crate::crt::{CrtMode, CrtPipeline};
```

Add fields to `EmuApp`:
```rust
render_state: Option<Arc<egui_wgpu::RenderState>>,
crt: Option<CrtPipeline>,
```

Note: `CrtMode` is read/written via `self.config.crt_mode`.

**Step 2: Initialise in EmuApp::new()**

```rust
// After existing Self { ... } construction, before closing brace:
let render_state = cc.wgpu_render_state.clone().map(Arc::new);
// ... in the Self { } literal:
render_state,
crt: None,
```

**Step 3: Recreate CrtPipeline in start_system()**

At the end of `start_system()`, after `self.system = Some(sys);`:
```rust
// Create/recreate CRT pipeline for this system's framebuffer dimensions
if let Some(ref rs) = self.render_state {
    if let Some(ref sys) = self.system {
        let w = sys.display_width();
        let h = sys.display_height();
        self.crt = Some(CrtPipeline::new(rs, w, h));
    }
}
```

**Step 4: Upload and apply each emulation frame**

In the `Screen::Emulation` branch of `update()`, replace the existing emulation step block with:

```rust
Screen::Emulation => {
    if let Some(ref mut sys) = self.system {
        let target = std::time::Duration::from_secs_f64(1.0 / sys.target_fps());
        let now = std::time::Instant::now();
        if now.duration_since(self.last_frame_time) >= target {
            self.last_frame_time = now;
            sys.step_frame();

            let count = sys.audio_samples(&mut self.audio_buffer);
            if let Some(ref mut audio) = self.audio {
                audio.push_samples(&self.audio_buffer[..count], self.config.volume);
            }

            // Upload framebuffer to CRT pipeline
            let fb = sys.framebuffer();
            if let Some(ref crt) = self.crt {
                crt.upload(&fb.pixels);
                crt.apply(self.config.crt_mode);
            }
        }

        // Render
        let fb = sys.framebuffer();
        let aspect = sys.display_aspect_ratio() as f32;
        if let Some(ref crt) = self.crt {
            // Display CRT-processed texture
            let available = ui.available_size();
            let (w, h) = if available.x / available.y > aspect {
                (available.y * aspect, available.y)
            } else {
                (available.x, available.x / aspect)
            };
            ui.centered_and_justified(|ui| {
                ui.image(egui::load::SizedTexture::new(
                    crt.texture_id,
                    egui::vec2(w, h),
                ));
            });
        } else {
            crate::screens::emulation::render(ui, &mut self.texture, fb, aspect);
        }
    }
}
```

**Step 5: Build**

```bash
cargo build --release -p emu-frontend 2>&1
```

Fix any borrow/lifetime errors before continuing.

**Step 6: Smoke test** — run the emulator, load any ROM. Screen should display (Sharp mode = identical to before). No crash.

**Step 7: Commit**

```bash
git add crates/frontend/src/app.rs
git commit -m "feat: wire CrtPipeline into EmuApp — upload+apply each frame"
```

---

### Task 5: Add Display menu item

**Files:**
- Modify: `crates/frontend/src/menu.rs`
- Modify: `crates/frontend/src/app.rs`

**Step 1: Add DisplayMode action to MenuAction enum in menu.rs**

```rust
pub enum MenuAction {
    None,
    LoadRom,
    Reset,
    Break,
    Quit,
    BackToSystemSelect,
    CycleCrtMode,   // add this
}
```

**Step 2: Add Display menu button in render_menu()**

Inside the `if has_system` block, after the "System" menu button, add:
```rust
ui.menu_button("Display", |ui| {
    // Label shows current mode; clicking cycles to next
    if ui.button(format!("Mode: {} →", current_mode_label)).clicked() {
        action = MenuAction::CycleCrtMode;
        ui.close_menu();
    }
});
```

`render_menu` needs to accept the current mode label. Change its signature to:
```rust
pub fn render_menu(ui: &mut Ui, has_system: bool, crt_label: &str) -> MenuAction
```

And use `crt_label` in the button text:
```rust
if ui.button(format!("Mode: {} →", crt_label)).clicked() {
```

**Step 3: Update call site in app.rs**

Change:
```rust
let action = menu::render_menu(ui, self.system.is_some());
```
To:
```rust
let crt_label = self.config.crt_mode.label();
let action = menu::render_menu(ui, self.system.is_some(), crt_label);
```

Handle the new action in the match:
```rust
MenuAction::CycleCrtMode => {
    self.config.crt_mode = self.config.crt_mode.next();
    self.config.save();
}
```

**Step 4: Build**

```bash
cargo build --release -p emu-frontend 2>&1
```

**Step 5: Test** — run emulator, load ROM, use Display menu to cycle Sharp → Scanlines → CRT. Each mode should look visually distinct.

**Step 6: Commit**

```bash
git add crates/frontend/src/menu.rs crates/frontend/src/app.rs
git commit -m "feat: Display menu to cycle CRT shader mode (Sharp/Scanlines/CRT)"
```

---

### Task 6: Final check

**Step 1: Run full workspace build**

```bash
cargo build --workspace 2>&1
```

Expected: 0 warnings, 0 errors.

**Step 2: Visual check**

Load each system (NES, C64, Atari) and verify:
- Sharp: pixel-perfect, no filtering
- Scanlines: visible horizontal darkening between rows
- CRT: barrel distortion visible at edges, scanlines, slight colour fringing

**Step 3: Final commit if any fixes were made**

```bash
git add -p
git commit -m "fix: CRT shader visual tweaks"
```

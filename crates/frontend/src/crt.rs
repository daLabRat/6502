//! CRT post-process pipeline.

use std::sync::Arc;
use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use wgpu::util::DeviceExt;

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum CrtMode {
    #[default]
    Off,
    Sharp,
    Scanlines,
    Crt,
}

impl CrtMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off       => "Off",
            Self::Sharp     => "Sharp",
            Self::Scanlines => "Scanlines",
            Self::Crt       => "CRT",
        }
    }
    fn as_u32(self) -> u32 {
        match self { Self::Off | Self::Sharp => 0, Self::Scanlines => 1, Self::Crt => 2 }
    }
}

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
            scanline_strength: 0.5,
            barrel_k:          0.04,
            bloom_amount:      0.12,
        }
    }
}

// All GPU-side resources shared via Arc so the paint callback can borrow them.
struct CrtResources {
    pipeline:           wgpu::RenderPipeline,
    src_texture:        wgpu::Texture,
    src_bind_group:     wgpu::BindGroup,
    uniform_buffer:     wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    render_target_view: wgpu::TextureView,
    width:              u32,
    height:             u32,
}

pub struct CrtPipeline {
    inner:          Arc<CrtResources>,
    pub texture_id: egui::TextureId,
}

// Per-frame callback that records the CRT render pass into eframe's encoder.
// No separate queue.submit() — eframe submits everything in one batch.
struct CrtCallback {
    inner:  Arc<CrtResources>,
    pixels: Vec<u8>,
    mode:   CrtMode,
}

impl egui_wgpu::CallbackTrait for CrtCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let r = &*self.inner;

        // Upload framebuffer pixels to source texture.
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture:   &r.src_texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::ImageDataLayout {
                offset:         0,
                bytes_per_row:  Some(4 * r.width),
                rows_per_image: Some(r.height),
            },
            wgpu::Extent3d { width: r.width, height: r.height, depth_or_array_layers: 1 },
        );

        // Write uniforms.
        queue.write_buffer(
            &r.uniform_buffer, 0,
            bytemuck::bytes_of(&Uniforms::new(r.width, r.height, self.mode)),
        );

        // Record CRT render pass into the shared eframe encoder (no extra submit).
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("crt_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &r.render_target_view,
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
            pass.set_pipeline(&r.pipeline);
            pass.set_bind_group(0, &r.src_bind_group, &[]);
            pass.set_bind_group(1, &r.uniform_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        vec![] // No separate command buffers needed.
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        // We render to our own texture; display happens via texture_id in ui.image().
    }
}

impl CrtPipeline {
    pub fn new(rs: &egui_wgpu::RenderState, width: u32, height: u32) -> Self {
        let device = &rs.device;

        let src_texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("crt_src"),
            size:            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8UnormSrgb,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        let src_view    = src_texture.create_view(&Default::default());
        let src_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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

        let texture_id = rs.renderer.write().register_native_texture(
            device,
            &render_target_view,
            wgpu::FilterMode::Linear,
        );

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
                    ty:                 wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:   None,
                },
                count: None,
            }],
        });

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
            primitive:     wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        let inner = Arc::new(CrtResources {
            pipeline,
            src_texture,
            src_bind_group,
            uniform_buffer,
            uniform_bind_group,
            render_target_view,
            width,
            height,
        });

        Self { inner, texture_id }
    }

    /// Create a paint callback that uploads `pixels` and runs the CRT shader.
    /// Add the returned value to `ui.painter()` before displaying `texture_id`.
    pub fn make_callback(&self, pixels: Vec<u8>, mode: CrtMode, rect: egui::Rect) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            CrtCallback { inner: self.inner.clone(), pixels, mode },
        )
    }
}

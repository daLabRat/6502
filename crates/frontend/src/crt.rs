//! CRT post-process pipeline.

use std::sync::Arc;
use eframe::egui_wgpu;
use eframe::wgpu;
use wgpu::util::DeviceExt;

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
            wgpu::ImageCopyTexture {
                texture:   &self.src_texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset:         0,
                bytes_per_row:  Some(4 * self.width),
                rows_per_image: Some(self.height),
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
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(enc.finish()));
    }
}

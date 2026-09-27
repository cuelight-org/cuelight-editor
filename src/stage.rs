//! The stage: the show as the players draw it, inside an iced widget.
//!
//! iced hands a shader widget its device and queue only in `prepare`, so
//! that is where the engine's presenter builds the frame and vello
//! renders it, into a texture of the widget's size. `render` then draws
//! that texture into the widget's rectangle of the window. Vello writes
//! through a storage binding, which a window's surface cannot be, hence
//! the texture in between; when the window is sRGB the texture is read
//! through an sRGB view, so the colours survive the round trip.

use std::fmt;
use std::sync::{Arc, Mutex};

use cuelight::Engine;
use cuelight::render::Presenter;
use iced::widget::shader::{self, Viewport};
use iced::{Rectangle, mouse};

/// The widget's program: what to draw is whatever the engine shows now.
pub struct Stage {
    pub engine: Arc<Mutex<Engine>>,
    /// Changes when the show moved, so iced prepares a new frame.
    pub revision: u64,
}

impl<Message> shader::Program<Message> for Stage {
    type State = ();
    type Primitive = Frame;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Frame {
        Frame {
            engine: self.engine.clone(),
            revision: self.revision,
        }
    }
}

/// One frame to draw: a handle on the engine at a revision.
pub struct Frame {
    engine: Arc<Mutex<Engine>>,
    revision: u64,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Frame(revision {})", self.revision)
    }
}

impl shader::Primitive for Frame {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        pipeline.drawn = false;
        let gpu = pipeline
            .gpu
            .get_mut()
            .expect("the pipeline is not poisoned");
        let Some(renderer) = gpu.renderer.as_mut() else {
            return;
        };
        let scale = viewport.scale_factor();
        let size = [
            ((bounds.width * scale).round() as u32).max(1),
            ((bounds.height * scale).round() as u32).max(1),
        ];
        if pipeline.target.as_ref().is_none_or(|t| t.size != size) {
            pipeline.target = Some(Target::new(
                device,
                &pipeline.layout,
                &pipeline.sampler,
                pipeline.srgb,
                size,
            ));
        }
        let target = pipeline.target.as_ref().expect("just made");

        let presented = {
            let engine = match self.engine.lock() {
                Ok(engine) => engine,
                Err(_) => return,
            };
            match gpu
                .presenter
                .present(&engine, device, queue, renderer, size)
            {
                Ok(presented) => presented,
                Err(error) => {
                    log::warn!("stage: cannot present the show: {error}");
                    return;
                }
            }
        };
        if let Err(error) = renderer.render_to_texture(
            device,
            queue,
            &presented.scene,
            &target.storage,
            &vello::RenderParams {
                base_color: presented.base_color,
                width: size[0],
                height: size[1],
                antialiasing_method: vello::AaConfig::Area,
            },
        ) {
            log::warn!("stage: vello did not render: {error}");
            return;
        }
        pipeline.drawn = true;
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        let Some(frame) = pipeline.target.as_ref().filter(|_| pipeline.drawn) else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cuelight-stage-blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            clip.x as f32,
            clip.y as f32,
            clip.width as f32,
            clip.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_pipeline(&pipeline.blit);
        pass.set_bind_group(0, &frame.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// What lives on the GPU across frames: the vello renderer, the
/// presenter with its own targets, the texture between the two, and the
/// pipeline that draws that texture into the window.
pub struct Pipeline {
    /// The presenter and vello's renderer, neither of which is `Sync`,
    /// behind a lock the pipeline (which must be) opens in `prepare`.
    gpu: Mutex<Gpu>,
    blit: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The window wants sRGB-encoded values decoded on the way in.
    srgb: bool,
    target: Option<Target>,
    /// The last `prepare` put a frame in `target`.
    drawn: bool,
}

struct Gpu {
    presenter: Presenter,
    /// `None` where vello cannot run: a device without compute, which is
    /// what a browser's WebGL2-level device is. The stage then stays
    /// blank and the log says why.
    renderer: Option<vello::Renderer>,
}

struct Target {
    size: [u32; 2],
    /// The view vello writes: `Rgba8Unorm`, as a storage binding.
    storage: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

const BLIT: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }

// One triangle over the whole viewport; the viewport is the widget.
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

@fragment fn fs(vertex: Vertex) -> @location(0) vec4<f32> {
    return textureSample(frame, frame_sampler, vertex.uv);
}
"#;

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let limits = device.limits();
        log::info!(
            "stage: window format {format:?}; device allows {} compute workgroups per dimension and {} storage buffers per stage",
            limits.max_compute_workgroups_per_dimension,
            limits.max_storage_buffers_per_shader_stage
        );
        let renderer = match vello::Renderer::new(device, vello::RendererOptions::default()) {
            Ok(renderer) => Some(renderer),
            Err(error) => {
                log::error!(
                    "stage: vello cannot run on this device, the stage stays blank: {error}"
                );
                None
            }
        };

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cuelight-stage-blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cuelight-stage-blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cuelight-stage-blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cuelight-stage-blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cuelight-stage-blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            gpu: Mutex::new(Gpu {
                presenter: Presenter::new(),
                renderer,
            }),
            blit,
            layout,
            sampler,
            srgb: format.is_srgb(),
            target: None,
            drawn: false,
        }
    }
}

impl Target {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        srgb: bool,
        size: [u32; 2],
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cuelight-stage"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        let storage = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            }),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cuelight-stage-blit"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sampled),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Self {
            size,
            storage,
            bind_group,
        }
    }
}

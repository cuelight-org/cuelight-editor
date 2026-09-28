//! The stage: the show as the players draw it, inside an iced widget.
//!
//! iced hands a shader widget its device and queue only in `prepare`, so
//! that is where the engine's presenter builds the frame and vello
//! renders it, into a texture of the widget's size. `render` then draws
//! that texture into the widget's rectangle of the window. Vello writes
//! through a storage binding, which a window's surface cannot be, hence
//! the texture in between; when the window is sRGB the texture is read
//! through an sRGB view, so the colours survive the round trip.
//!
//! A zoomed-in widget is larger than what its scrollable shows of it:
//! `render` gets only the visible clip, and draws the slice of the frame
//! that lies under it. Vello's compute renderer draws targets of at most
//! [`MAX_BINS`] bins of [`BIN`] pixels (linebender/vello#680, about
//! 4096 x 4096 in all), so the frame is not prepared past that and the
//! window keeps its zoom under it (cuelight#245 asks for a presented
//! view instead).

use std::fmt;
use std::sync::{Arc, Mutex};

use cuelight::Engine;
use cuelight::render::Presenter;
use iced::widget::shader::{self, Action, Viewport};
use iced::{Event, Rectangle, mouse};

/// The side of one of vello's coarse bins, in pixels.
pub const BIN: u32 = 256;
/// How many bins vello's compute renderer draws in one target.
pub const MAX_BINS: u32 = 256;

/// Whether vello draws a target of this size at all.
pub fn drawable(size: [u32; 2]) -> bool {
    size[0].div_ceil(BIN) * size[1].div_ceil(BIN) <= MAX_BINS
}

/// The widget's program: what to draw is whatever the engine shows now,
/// and a click on it is a press at a canvas point.
pub struct Stage<Message> {
    pub engine: Arc<Mutex<Engine>>,
    /// Changes when the show moved, so iced prepares a new frame.
    pub revision: u64,
    /// The message a press at a canvas point becomes.
    pub on_press: fn([f64; 2]) -> Message,
}

impl<Message> shader::Program<Message> for Stage<Message> {
    type State = ();
    type Primitive = Frame;

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let at = cursor.position_in(bounds)?;
        let engine = self.engine.lock().ok()?;
        let show = engine.show()?;
        // The stage fits the show into its box the way a player fits it
        // into a window, so the same arithmetic maps a point back.
        let target = [bounds.width.round() as u32, bounds.height.round() as u32];
        let point = cuelight::render::canvas_at(
            show.size,
            target,
            engine.scaling(),
            [f64::from(at.x), f64::from(at.y)],
        )?;
        Some(Action::publish((self.on_press)(point)).and_capture())
    }

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Frame {
        Frame {
            engine: self.engine.clone(),
            revision: self.revision,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // A pointer over something pressable says so.
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let Ok(engine) = self.engine.lock() else {
            return mouse::Interaction::default();
        };
        let Some(show) = engine.show() else {
            return mouse::Interaction::default();
        };
        let target = [bounds.width.round() as u32, bounds.height.round() as u32];
        let pressable = cuelight::render::canvas_at(
            show.size,
            target,
            engine.scaling(),
            [f64::from(at.x), f64::from(at.y)],
        )
        .is_some_and(|point| engine.pressed(point).is_some());
        if pressable {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
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
        pipeline.bounds = [
            bounds.x * scale,
            bounds.y * scale,
            size[0] as f32,
            size[1] as f32,
        ];
        if !drawable(size) {
            if pipeline.target.take().is_some() {
                log::warn!(
                    "stage: {} x {} is more than vello draws, the stage stays blank",
                    size[0],
                    size[1]
                );
            }
            return;
        }
        if pipeline.target.as_ref().is_none_or(|t| t.size != size) {
            pipeline.target = Some(Target::new(
                device,
                &pipeline.layout,
                &pipeline.sampler,
                &pipeline.uniform,
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
        // The slice of the frame under the clip, as texture coordinates.
        let [x, y, w, h] = pipeline.bounds;
        let u0 = (clip.x as f32 - x) / w;
        let v0 = (clip.y as f32 - y) / h;
        let rect = [
            u0,
            v0,
            u0 + clip.width as f32 / w,
            v0 + clip.height as f32 / h,
        ];
        let bytes: Vec<u8> = rect.iter().flat_map(|v| v.to_ne_bytes()).collect();
        pipeline.queue.write_buffer(&pipeline.uniform, 0, &bytes);
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
    /// The queue, for writing the blit's slice from `render`, which is
    /// handed an encoder only.
    queue: wgpu::Queue,
    /// Which slice of the frame the blit draws: `[u0, v0, u1, v1]`.
    uniform: wgpu::Buffer,
    /// The widget's whole box in physical pixels, `[x, y, width,
    /// height]`, as the last `prepare` saw it.
    bounds: [f32; 4],
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
// The slice of the frame under the viewport: u0, v0, u1, v1.
@group(0) @binding(2) var<uniform> slice: vec4<f32>;

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }

// One triangle over the whole viewport; the viewport is the visible
// part of the widget, and gets the frame's slice under it.
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let uv = mix(slice.xy, slice.zw, corner);
    return Vertex(vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0), uv);
}

@fragment fn fs(vertex: Vertex) -> @location(0) vec4<f32> {
    return textureSample(frame, frame_sampler, vertex.uv);
}
"#;

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cuelight-stage-slice"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
            queue: queue.clone(),
            uniform,
            bounds: [0.0, 0.0, 1.0, 1.0],
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
        uniform: &wgpu::Buffer,
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
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

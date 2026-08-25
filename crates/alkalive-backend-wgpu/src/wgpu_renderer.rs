//! wgpu-based GPU renderer using WGSL shaders — the ADR-001/ADR-006
//! production rendering backend.
//!
//! This renderer is the **primary** production path selected by the runtime
//! (`alkalive-runtime-wasm`). It renders through the `wgpu` crate (WebGPU
//! where the browser provides it). The raw-WebGL2/GLSL renderer
//! ([`crate::WgpuRenderer`]) remains available as an explicit,
//! independently-tested fallback for browsers/devices without WebGPU.
//!
//! # Frame pipeline
//!
//! 1. **Tessellate** — [`crate::tessellate::tessellate_scene`] shapes the
//!    title + input-field runs through the HarfRust stack, rasterizes the
//!    glyph atlas page, and produces the combined pixel-space vertex buffer
//!    (re-run only when atlas inputs change).
//! 2. **Graph** — [`alkalive_render::graph::build_render_graph`] produces
//!    the 5-pass render-graph IR (ADR-001): Clear → InputFieldBackground →
//!    InputFieldBorder → TitleText → InputText.
//! 3. **Plan** — [`collect_frame_plan`] walks the graph in `pass_order`
//!    once and produces every per-draw-call uniform record plus an encode
//!    plan (pure function, unit-tested off-GPU in `frame_plan`).
//! 4. **Encode** — one command encoder records every pass: the first pass
//!    clears to the plan's clear color, later passes load; each draw call
//!    selects its pipeline and binds its per-draw uniforms through dynamic
//!    offsets into a ring buffer.
//! 5. **Submit + present.**
//!
//! # Uniform delivery
//!
//! Per-draw-call data lives in two ring buffers (text / rect) laid out in
//! device-aligned slots (`dynamic_stride`, ≥ 256 B). One bind group per
//! pipeline is created at init with `has_dynamic_offset: true`; encoding
//! binds the group with the slot offset. Slot assignment is deterministic:
//! nth rect-kind call ↔ nth rect slot, nth text-kind call ↔ nth text slot.

#![cfg(feature = "wgpu-backend")]

use std::num::NonZeroU64;
use wgpu::util::DeviceExt;

use crate::frame_plan::{collect_frame_plan, PlannedDraw, RectUniformsData, TextUniformsData};
use crate::tessellate::{tessellate_scene, SceneTessellation};
use crate::wgsl_shaders;
use crate::{Vertex, ATLAS_PAGE_BYTES, ATLAS_SIZE};

/// Maximum number of rect-kind and text-kind draw calls schedulable per
/// frame (the canonical Hello-World graph uses two of each).
pub const MAX_DYNAMIC_SLOTS: usize = 16;

/// Minimum byte size of one dynamic-offset slot before device alignment.
const BASE_SLOT_SIZE: u64 = 256;

/// Unit-corner quad (triangle list) consumed by `RECT_VERTEX_WGSL`: corner
/// components are 0 or 1 and are mapped through the per-draw rect uniform
/// into pixel space.
const RECT_CORNERS: [[f32; 2]; 6] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
];

/// The wgpu/WGSL production renderer (ADR-006).
pub struct WgpuBackendRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,

    text_pipeline: wgpu::RenderPipeline,
    rect_pipeline: wgpu::RenderPipeline,

    text_bind_group: wgpu::BindGroup,
    rect_bind_group: wgpu::BindGroup,

    text_uniform_buffer: wgpu::Buffer,
    rect_uniform_buffer: wgpu::Buffer,
    dynamic_stride: u64,

    // These are kept alive for the lifetime of the renderer because the bind
    // groups reference their views/samplers; Rust has no way to express that
    // borrow beyond struct fields, so the "never read" lint is expected.
    #[allow(dead_code)]
    glyph_texture: wgpu::Texture,
    #[allow(dead_code)]
    glyph_texture_view: wgpu::TextureView,
    #[allow(dead_code)]
    glyph_sampler: wgpu::Sampler,

    rect_vertex_buffer: wgpu::Buffer,
    text_vertex_buffer: Option<wgpu::Buffer>,

    tess: Option<SceneTessellation>,
    last_input_display: String,
    /// Set at init and on resize; forces one re-tessellation.
    tess_dirty: bool,

    width: u32,
    height: u32,
}

impl WgpuBackendRenderer {
    /// Initialize the renderer from an HTML canvas element.
    ///
    /// Creates the wgpu instance/surface/adapter/device, compiles the WGSL
    /// programs, creates explicit-layout pipelines, ring buffers, bind
    /// groups, and the empty glyph-atlas texture. Any failure returns a
    /// descriptive `Err` so the runtime can select the fallback renderer.
    pub async fn init_from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("create_surface failed: {e:?}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no compatible GPU adapter (WebGPU unavailable?)")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("AlkALive GPU Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("request_device failed: {e:?}"))?;

        // Surface configuration: prefer an sRGB format when offered.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // Dynamic-offset alignment: slot stride must be a multiple of the
        // device's minimum uniform buffer offset alignment.
        let align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let dynamic_stride = ((BASE_SLOT_SIZE + align - 1) / align) * align;
        let ring_size = dynamic_stride * MAX_DYNAMIC_SLOTS as u64;

        // --- WGSL programs ---------------------------------------------------
        let make_module = |label: &'static str, src: &'static str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let text_vs = make_module("text vs (WGSL)", wgsl_shaders::TEXT_VERTEX_WGSL);
        let text_fs = make_module("text fs (WGSL)", wgsl_shaders::TEXT_FRAGMENT_WGSL);
        let rect_vs = make_module("rect vs (WGSL)", wgsl_shaders::RECT_VERTEX_WGSL);
        let rect_fs = make_module("rect fs (WGSL)", wgsl_shaders::RECT_FRAGMENT_WGSL);

        // --- Ring buffers -----------------------------------------------------
        let ring_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let text_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text uniform ring"),
            size: ring_size,
            usage: ring_usage,
            mapped_at_creation: false,
        });
        let rect_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect uniform ring"),
            size: ring_size,
            usage: ring_usage,
            mapped_at_creation: false,
        });

        // --- Glyph atlas resources ---------------------------------------------
        let glyph_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let glyph_texture_view = glyph_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let glyph_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // --- Explicit layouts + bind groups ------------------------------------
        //
        // Layouts are explicit (not shader-derived) so a WGSL binding change
        // fails loudly here rather than silently re-ordering the interface.
        let text_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(TextUniformsData::WGSL_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let text_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text BG"),
            layout: &text_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &text_uniform_buffer,
                        offset: 0,
                        size: NonZeroU64::new(TextUniformsData::WGSL_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&glyph_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&glyph_sampler),
                },
            ],
        });

        let rect_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(RectUniformsData::WGSL_SIZE),
                },
                count: None,
            }],
        });
        let rect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect BG"),
            layout: &rect_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &rect_uniform_buffer,
                    offset: 0,
                    size: NonZeroU64::new(RectUniformsData::WGSL_SIZE),
                }),
            }],
        });

        // --- Pipelines -----------------------------------------------------------
        let position_attr = wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        };
        let uv_attr = wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        };

        let text_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("text pipeline layout"),
                bind_group_layouts: &[&text_bgl],
                push_constant_ranges: &[],
            });
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text pipeline (WGSL)"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_vs,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[position_attr, uv_attr],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_fs,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let rect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rect pipeline layout"),
                bind_group_layouts: &[&rect_bgl],
                push_constant_ranges: &[],
            });
        let rect_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect corner quad"),
            contents: bytemuck::cast_slice(&RECT_CORNERS),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect pipeline (WGSL)"),
            layout: Some(&rect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &rect_vs,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[position_attr],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &rect_fs,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            config,
            surface,
            text_pipeline,
            rect_pipeline,
            text_bind_group,
            rect_bind_group,
            text_uniform_buffer,
            rect_uniform_buffer,
            dynamic_stride,
            glyph_texture,
            glyph_texture_view,
            glyph_sampler,
            rect_vertex_buffer,
            text_vertex_buffer: None,
            tess: None,
            last_input_display: String::new(),
            tess_dirty: true,
            width,
            height,
        })
    }

    /// Render one frame from scene data via the render-graph IR.
    ///
    /// Re-tessellates when the glyph-atlas inputs changed (first frame,
    /// input-text change, or resize), then plans and encodes the graph.
    /// Failures are logged to the browser console — a skipped frame is made
    /// visible there rather than silent.
    pub fn render_frame(
        &mut self,
        scene: &crate::TextSceneData,
        _schedule: &alkalive_compiler::ScheduleIR,
        time: f32,
    ) {
        let input_display = if scene.input_text.is_empty() {
            scene.input_placeholder.as_str()
        } else {
            scene.input_text.as_str()
        };

        if self.tess_dirty || self.last_input_display != input_display {
            match tessellate_scene(scene, self.width as f32, self.height as f32) {
                Ok(t) => {
                    if let Err(e) = self.upload_tessellation(&t) {
                        web_sys::console::error_1(
                            &format!("AlkALive(wgpu): tessellation upload failed: {e}").into(),
                        );
                        return;
                    }
                    self.last_input_display = input_display.to_string();
                    self.tess_dirty = false;
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("AlkALive(wgpu): tessellation failed: {e}").into(),
                    );
                    return;
                }
            }
        }

        let (input_vertex_start, title_vertex_count, input_vertex_count, field_bounds) =
            match &self.tess {
                Some(t) => (
                    t.input_vertex_start as u32,
                    t.title_vertex_count as u32,
                    t.input_vertex_count as u32,
                    t.input_field_bounds,
                ),
                None => return,
            };

        // Build the render graph for this frame (ADR-001 IR).
        let graph = alkalive_render::graph::build_render_graph(
            scene,
            (self.width, self.height),
            field_bounds,
        );
        if let Err(e) = graph.validate() {
            web_sys::console::error_1(
                &format!("AlkALive(wgpu): render graph invalid: {e:?}").into(),
            );
            return;
        }

        let plan = collect_frame_plan(&graph, self.width as f32, self.height as f32, time);
        if plan.text_slot_count() > MAX_DYNAMIC_SLOTS || plan.rect_slot_count() > MAX_DYNAMIC_SLOTS
        {
            web_sys::console::error_1(
                &format!(
                    "AlkALive(wgpu): frame needs {} rect / {} text slots (max {MAX_DYNAMIC_SLOTS})",
                    plan.rect_slot_count(),
                    plan.text_slot_count()
                )
                .into(),
            );
            return;
        }

        // Upload the uniform rings (one write per ring per frame).
        self.write_ring(&self.text_uniform_buffer, &plan.text_uniforms);
        self.write_ring(&self.rect_uniform_buffer, &plan.rect_uniforms);

        // Acquire the swapchain frame.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                web_sys::console::error_1(
                    &format!("AlkALive(wgpu): acquire frame failed: {e}").into(),
                );
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("AlkALive frame encoder"),
            },
        );

        let clear = wgpu::Color {
            r: plan.clear_color[0] as f64,
            g: plan.clear_color[1] as f64,
            b: plan.clear_color[2] as f64,
            a: plan.clear_color[3] as f64,
        };

        let mut planned = plan.draws.iter();
        let mut is_first_pass = true;

        for &pass_idx in &graph.pass_order {
            let pass = &graph.passes[pass_idx];
            let load_op = if is_first_pass {
                is_first_pass = false;
                wgpu::LoadOp::Clear(clear)
            } else {
                wgpu::LoadOp::Load
            };

            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&pass.name),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            for _dc in &pass.draw_calls {
                match planned.next() {
                    Some(PlannedDraw::Clear) => {}
                    Some(PlannedDraw::Rect(slot)) => {
                        let offset = (*slot as u64) * self.dynamic_stride;
                        rpass.set_pipeline(&self.rect_pipeline);
                        rpass.set_bind_group(0, &self.rect_bind_group, &[offset as u32]);
                        rpass.set_vertex_buffer(0, self.rect_vertex_buffer.slice(..));
                        rpass.draw(0..6, 0..1);
                    }
                    Some(PlannedDraw::Text(slot, is_input)) => {
                        let (start, count) = if *is_input {
                            (input_vertex_start, input_vertex_count)
                        } else {
                            (0, title_vertex_count)
                        };
                        if count == 0 {
                            continue;
                        }
                        let Some(vb) = &self.text_vertex_buffer else {
                            continue;
                        };
                        let offset = (*slot as u64) * self.dynamic_stride;
                        rpass.set_pipeline(&self.text_pipeline);
                        rpass.set_bind_group(0, &self.text_bind_group, &[offset as u32]);
                        rpass.set_vertex_buffer(0, vb.slice(..));
                        rpass.draw(start..(start + count), 0..1);
                    }
                    None => break,
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    /// Write per-draw-call records into a ring buffer, one aligned slot per
    /// record.
    fn write_ring<T: bytemuck::Pod>(&self, buffer: &wgpu::Buffer, records: &[T]) {
        let rec_bytes = std::mem::size_of::<T>();
        let mut ring = vec![0u8; (self.dynamic_stride * MAX_DYNAMIC_SLOTS as u64) as usize];
        for (slot, rec) in records.iter().enumerate() {
            let off = slot * self.dynamic_stride as usize;
            let bytes = bytemuck::bytes_of(rec);
            debug_assert_eq!(bytes.len(), rec_bytes);
            ring[off..off + rec_bytes].copy_from_slice(bytes);
        }
        self.queue.write_buffer(buffer, 0, &ring);
    }

    /// Upload tessellation output: rebuild the text vertex buffer, upload
    /// the glyph atlas page, cache bounds.
    fn upload_tessellation(&mut self, t: &SceneTessellation) -> Result<(), String> {
        if t.atlas_page.len() != ATLAS_PAGE_BYTES {
            return Err(format!(
                "atlas page has {} bytes, expected {ATLAS_PAGE_BYTES}",
                t.atlas_page.len()
            ));
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &t.atlas_page,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_SIZE),
                rows_per_image: Some(ATLAS_SIZE),
            },
            wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );

        if !t.vertices.is_empty() {
            let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("text vertices"),
                contents: bytemuck::cast_slice(&t.vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.text_vertex_buffer = Some(buf);
        }

        self.tess = Some(t.clone());
        Ok(())
    }

    /// Resize the surface. The next frame re-tessellates so geometry tracks
    /// the new dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        // Force re-tessellation: cached geometry reflects the old size.
        self.tess_dirty = true;
    }

    /// Check whether a point lies inside the input field rectangle.
    pub fn hit_test_input_field(&self, x: f32, y: f32) -> bool {
        let Some(t) = &self.tess else {
            return false;
        };
        let (fx, fy, fw, fh) = t.input_field_bounds;
        x >= fx && x <= fx + fw && y >= fy && y <= fy + fh
    }

    /// Canvas width in physical pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Canvas height in physical pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

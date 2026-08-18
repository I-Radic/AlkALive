//! wgpu-based renderer using WGSL shaders (ADR-006).
//!
//! This module implements the GPU rendering backend using the `wgpu` crate
//! with WGSL shaders, replacing the raw WebGL2/GLSL path. The wgpu crate
//! provides a unified API that works with both WebGPU (when available) and
//! WebGL2 (via the `webgl` feature) as a fallback.
//!
//! # Architecture
//!
//! The renderer:
//! 1. Creates a `wgpu::Surface` from the HTML canvas.
//! 2. Requests a `wgpu::Device` and `wgpu::Queue`.
//! 3. Compiles WGSL shaders via `wgpu::Device::create_shader_module`.
//! 4. Creates render pipelines for text and rect rendering.
//! 5. Creates vertex buffers and a glyph atlas texture.
//! 6. Per frame: builds a command encoder, executes render passes, submits.
//!
//! # WGSL Shaders
//!
//! The shaders are defined in [`crate::wgsl_shaders`] and are compiled
//! at runtime by the wgpu device. This replaces the GLSL ES 3.00 shaders
//! that were compiled manually via `WebGl2RenderingContext::compile_shader`.

#![cfg(feature = "wgpu-backend")]

use std::sync::Arc;
use wgpu::util::DeviceExt;

use alkalive_scene_data::TextSceneData;
use alkalive_render::graph::{DrawCallKind, RenderGraph};

use crate::wgsl_shaders;

/// A wgpu-based GPU renderer that uses WGSL shaders.
///
/// This is the ADR-006 compliant renderer: it uses WGSL (not GLSL) and
/// the `wgpu` API (not raw WebGL2). The `webgl` feature on the `wgpu`
/// crate ensures WebGL2 fallback when WebGPU is not available.
pub struct WgpuBackendRenderer {
    /// The wgpu device (owns the GPU context).
    device: wgpu::Device,
    /// The wgpu queue (for submitting command buffers).
    queue: wgpu::Queue,
    /// The surface configuration.
    config: wgpu::SurfaceConfiguration,
    /// The render surface.
    surface: wgpu::Surface<'static>,
    /// The text render pipeline (WGSL vertex + fragment).
    text_pipeline: wgpu::RenderPipeline,
    /// The rect render pipeline (WGSL vertex + fragment).
    rect_pipeline: wgpu::RenderPipeline,
    /// The glyph atlas texture.
    glyph_texture: wgpu::Texture,
    /// The glyph texture view.
    glyph_texture_view: wgpu::TextureView,
    /// The glyph texture sampler.
    glyph_sampler: wgpu::Sampler,
    /// The vertex buffer for text quads.
    vertex_buffer: wgpu::Buffer,
    /// The vertex count (6 per glyph quad).
    vertex_count: u32,
    /// Canvas width in physical pixels.
    width: u32,
    /// Canvas height in physical pixels.
    height: u32,
    /// Input field bounds for hit-testing.
    input_field_bounds: (f32, f32, f32, f32),
    /// Uniform buffer for text rendering (rotation, canvas_size, time, text_color).
    uniform_buffer: wgpu::Buffer,
    /// Bind group layout for the text pipeline (uniform + texture + sampler).
    text_bind_group_layout: wgpu::BindGroupLayout,
    /// Bind group for the text pipeline.
    text_bind_group: wgpu::BindGroup,
}

/// Uniform data for text rendering, matching the WGSL TextUniforms struct.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextUniformsData {
    rotation: f32,
    canvas_size: [f32; 2],
    time: f32,
    text_color: [f32; 4],
}

/// Vertex format: position (vec2) + uv (vec2) = 16 bytes.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x2,  // position
        1 => Float32x2,  // uv
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

impl WgpuBackendRenderer {
    /// Initialize the wgpu renderer from an HTML canvas element.
    ///
    /// This creates a wgpu surface, requests a device, compiles WGSL shaders,
    /// and creates render pipelines.
    pub async fn init_from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // 1. Create a wgpu instance.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // 2. Create a surface from the canvas.
        // The Canvas variant is available when wgpu's `webgl` feature is enabled.
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .expect("Failed to create surface");

        // 3. Request an adapter.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("Failed to request adapter")?;

        // 4. Request a device and queue.
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("AlkALive GPU Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None, // no trace path
            )
            .await
            .map_err(|e| format!("Failed to request device: {:?}", e))?;

        // 5. Configure the surface.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // 6. Compile WGSL shaders.
        let text_vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Vertex Shader (WGSL)"),
            source: wgpu::ShaderSource::Wgsl(wgsl_shaders::TEXT_VERTEX_WGSL.into()),
        });
        let text_fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Fragment Shader (WGSL)"),
            source: wgpu::ShaderSource::Wgsl(wgsl_shaders::TEXT_FRAGMENT_WGSL.into()),
        });
        let rect_vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rect Vertex Shader (WGSL)"),
            source: wgpu::ShaderSource::Wgsl(wgsl_shaders::RECT_VERTEX_WGSL.into()),
        });
        let rect_fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rect Fragment Shader (WGSL)"),
            source: wgpu::ShaderSource::Wgsl(wgsl_shaders::RECT_FRAGMENT_WGSL.into()),
        });

        // 7. Create render pipelines.
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Render Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &text_vs,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Vertex::desc()],
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

        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rect Render Pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &rect_vs,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
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
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // 8. Create glyph atlas texture (512×512 R8).
        let glyph_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let glyph_texture_view = glyph_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let glyph_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glyph Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // 9. Create an initial vertex buffer (will be updated per frame).
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: 1024 * 16, // initial size; will be recreated as needed
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 10. Create uniform buffer for text rendering uniforms.
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Uniforms Buffer"),
            size: std::mem::size_of::<TextUniformsData>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 11. Create bind group layout for text pipeline.
        let text_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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

        // 12. Create bind group binding the uniform buffer + glyph texture + sampler.
        let text_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Bind Group"),
            layout: &text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(uniform_buffer.as_entire_buffer_binding()),
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

        Ok(Self {
            device,
            queue,
            config,
            surface,
            text_pipeline,
            rect_pipeline,
            glyph_texture,
            glyph_texture_view,
            glyph_sampler,
            vertex_buffer,
            vertex_count: 0,
            width,
            height,
            input_field_bounds: (0.0, 0.0, 0.0, 0.0),
            uniform_buffer,
            text_bind_group_layout,
            text_bind_group,
        })
    }

    /// Render one frame from scene data, building and executing a render graph.
    /// This is the high-level entry point matching the `WgpuRenderer::render_frame` API.
    pub fn render_frame(
        &mut self,
        scene: &TextSceneData,
        _schedule: &alkalive_compiler::ScheduleIR,
        time: f32,
    ) {
        let graph = alkalive_render::graph::build_render_graph(
            scene,
            (self.width, self.height),
            self.input_field_bounds,
        );
        self.render_graph(&graph, time);
    }

    /// Render one frame using the render graph.
    ///
    /// This method consumes a `RenderGraph` and executes its passes using
    /// the wgpu API, compiling WGSL shaders (done at init) and submitting
    /// command buffers to the GPU.
    pub fn render_graph(&mut self, graph: &RenderGraph, time: f32) {
        // Get the next frame texture.
        let output = self.surface.get_current_texture();
        let Ok(frame) = output else {
            return;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Update uniform buffer with current frame data.
        let uniforms = TextUniformsData {
            rotation: 0.5 * time, // rotation_speed * time
            canvas_size: [self.width as f32, self.height as f32],
            time,
            text_color: [1.0, 0.843, 0.0, 1.0], // gold
        };
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Create a command encoder.
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        // Find the clear color from the first Clear draw call.
        let mut clear_color = wgpu::Color::BLACK;
        for pass in &graph.passes {
            for dc in &pass.draw_calls {
                if let DrawCallKind::Clear { color } = &dc.kind {
                    clear_color = wgpu::Color {
                        r: color.0 as f64,
                        g: color.1 as f64,
                        b: color.2 as f64,
                        a: color.3 as f64,
                    };
                }
            }
        }

        // Execute each pass in the graph's pass_order.
        let mut is_first_pass = true;
        for &pass_idx in &graph.pass_order {
            let pass = &graph.passes[pass_idx];

            // Determine load operation: Clear on first pass, Load on subsequent.
            let load_op = if is_first_pass {
                wgpu::LoadOp::Clear(clear_color)
            } else {
                wgpu::LoadOp::Load
            };
            is_first_pass = false;

            // Begin a render pass.
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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

                // Execute draw calls in this pass.
                for dc in &pass.draw_calls {
                    match &dc.kind {
                        DrawCallKind::Clear { color: _ } => {
                            // Clear is handled by LoadOp::Clear above.
                        }
                        DrawCallKind::DrawRect { x, y, w, h, color: _ } => {
                            let _ = (x, y, w, h);
                            render_pass.set_pipeline(&self.rect_pipeline);
                            render_pass.draw(0..4, 0..1);
                        }
                        DrawCallKind::DrawRectOutline { x, y, w, h, color: _, line_width: _ } => {
                            let _ = (x, y, w, h);
                            render_pass.set_pipeline(&self.rect_pipeline);
                            render_pass.draw(0..4, 0..1);
                        }
                        DrawCallKind::DrawText { text_ptr: _, text_len: _, font_size: _, color: _, rotation: _, position: _ } => {
                            // Text rendering: set pipeline, bind group, vertex buffer, draw.
                            render_pass.set_pipeline(&self.text_pipeline);
                            render_pass.set_bind_group(0, &self.text_bind_group, &[]);
                            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                            if self.vertex_count > 0 {
                                render_pass.draw(0..self.vertex_count, 0..1);
                            }
                        }
                    }
                }
            }
        }

        // Submit the command buffer.
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    /// Resize the canvas + surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Update the vertex buffer with new text quad data.
    pub fn update_vertices(&mut self, vertices: &[Vertex]) {
        self.vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.vertex_count = vertices.len() as u32;
    }

    /// Update the glyph atlas texture.
    pub fn update_glyph_texture(&mut self, data: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(512),
            },
            wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
        );
    }

    /// Check if a point is inside the input field rectangle.
    pub fn hit_test_input_field(&self, x: f32, y: f32) -> bool {
        let (fx, fy, fw, fh) = self.input_field_bounds;
        x >= fx && x <= fx + fw && y >= fy && y <= fy + fh
    }

    /// Returns the canvas width.
    pub fn width(&self) -> u32 { self.width }

    /// Returns the canvas height.
    pub fn height(&self) -> u32 { self.height }
}

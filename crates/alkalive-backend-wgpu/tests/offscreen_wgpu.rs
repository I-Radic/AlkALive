//! Offscreen GPU integration test for the wgpu/WGSL production pipeline.
//!
//! This is the deterministic proof that the WGSL programs rasterize the
//! Hello-World frame correctly: it drives the SAME constructors and the SAME
//! [`record_frame`] encoder the browser surface path uses — WGSL compilation,
//! pipelines, dynamic-offset uniform rings, glyph-atlas sampling — but
//! renders into an offscreen texture whose bytes are read back and asserted.
//!
//! Skips (with a loud note) only when no GPU adapter exists at all; on any
//! machine with a working Vulkan/D3D12/Metal/GL driver it executes the real
//! GPU path.

#![cfg(feature = "wgpu-backend")]

use alkalive_backend_wgpu::frame_plan::collect_frame_plan;
use alkalive_backend_wgpu::tessellate::tessellate_scene;
use alkalive_backend_wgpu::wgpu_renderer::{
    upload_atlas_page, upload_ring,
    create_frame_pipelines, create_glyph_atlas_resources, create_uniform_rings,
    dynamic_slot_stride, record_frame, FrameGpuRefs, TextRanges, MAX_DYNAMIC_SLOTS,
};
use alkalive_scene_data::TextSceneData;
use wgpu::util::DeviceExt;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

/// Read the full RGBA bytes of `texture` back to the CPU.
async fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    let bytes_per_row = (WIDTH * 4).next_multiple_of(256);
    let buffer_size = (bytes_per_row * HEIGHT) as u64;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    // Drive the device until the map callback has fired.
    loop {
        device.poll(wgpu::Maintain::Wait);
        if rx.try_recv().is_ok() {
            break;
        }
    }

    let data = slice.get_mapped_range();
    // Un-pad rows into a tightly packed RGBA buffer.
    let mut out = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for row in 0..HEIGHT {
        let start = (row * bytes_per_row) as usize;
        out.extend_from_slice(&data[start..start + (WIDTH * 4) as usize]);
    }
    drop(data);
    buffer.unmap();
    out
}

#[test]
fn offscreen_gpu_frame_renders_golden_text() {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        else {
            eprintln!("SKIPPED: no GPU adapter available in this environment");
            return;
        };
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("AlkALive offscreen test"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .expect("device request");

        // --- Build the exact production resource set --------------------------
        let stride = dynamic_slot_stride(&device);
        let (text_ring, rect_ring) = create_uniform_rings(&device, stride);

        // Offscreen color target — NON-sRGB, matching the production surface
        // format preference (parity with the WebGL2/GLSL fallback).
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let atlas = create_glyph_atlas_resources(&device);
        let (text_pipeline, text_bind_group, rect_pipeline, rect_bind_group, rect_vb) =
            create_frame_pipelines(
                &device,
                format,
                &text_ring,
                &rect_ring,
                &atlas.view,
                &atlas.sampler,
            );

        // --- Scene → tessellation → uploads -----------------------------------
        let scene = TextSceneData::default(); // golden-on-black Hello World
        let tess = tessellate_scene(&scene, WIDTH as f32, HEIGHT as f32).expect("tessellate");

        upload_atlas_page(&queue, &atlas.texture, &tess.atlas_page).expect("atlas upload");
        assert!(
            !tess.vertices.is_empty(),
            "Hello World must tessellate to vertices"
        );
        let text_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text vertices"),
            contents: bytemuck::cast_slice(&tess.vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        // --- Graph → plan → rings ----------------------------------------------
        let graph = alkalive_render::graph::build_render_graph(
            &scene,
            (WIDTH, HEIGHT),
            tess.input_field_bounds,
        );
        graph.validate().expect("graph valid");
        let plan = collect_frame_plan(&graph, WIDTH as f32, HEIGHT as f32, 1.0);
        assert_eq!(plan.draws.len(), 5);

        // Write each ring via the shared production helper (slot i at byte
        // offset i * stride).
        upload_ring(&queue, &text_ring, stride, &plan.text_uniforms);
        upload_ring(&queue, &rect_ring, stride, &plan.rect_uniforms);

        // --- Encode + submit + readback -----------------------------------------
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        record_frame(
            &mut encoder,
            &view,
            plan.clear_color,
            &graph,
            &plan,
            FrameGpuRefs {
                text_pipeline: &text_pipeline,
                text_bind_group: &text_bind_group,
                rect_pipeline: &rect_pipeline,
                rect_bind_group: &rect_bind_group,
                rect_vertex_buffer: &rect_vb,
                text_vertex_buffer: Some(&text_vb),
                dynamic_stride: stride,
            },
            TextRanges {
                title_count: tess.title_vertex_count as u32,
                input_start: tess.input_vertex_start as u32,
                input_count: tess.input_vertex_count as u32,
            },
        );
        queue.submit(Some(encoder.finish()));
        let pixels = read_texture(&device, &queue, &target).await;

        // --- Assertions -----------------------------------------------------------
        let mut black = 0usize;
        let mut golden = 0usize;
        let mut input_field_bg = 0usize;
        let total = (WIDTH * HEIGHT) as usize;
        for i in (0..pixels.len()).step_by(4) {
            let r = pixels[i] as f32 / 255.0;
            let g = pixels[i + 1] as f32 / 255.0;
            let b = pixels[i + 2] as f32 / 255.0;
            if r < 0.02 && g < 0.02 && b < 0.02 {
                black += 1;
            } else if r > g && g > b && r > 0.25 {
                golden += 1;
            } else if (r - 0.05).abs() < 0.04 && (g - 0.05).abs() < 0.04 && (b - 0.08).abs() < 0.05
            {
                input_field_bg += 1;
            }
        }

        assert!(
            golden > total / 500,
            "golden title pixels must be visible (got {golden} of {total})"
        );
        assert!(
            black > total * 90 / 100,
            "background must be predominantly black (got {black}/{total})"
        );
        assert!(
            input_field_bg > 100,
            "input-field rectangle must be drawn (got {input_field_bg} px)"
        );
        eprintln!(
            "offscreen GPU frame OK: golden={golden} black={black} field={input_field_bg} total={total}"
        );
    });
}
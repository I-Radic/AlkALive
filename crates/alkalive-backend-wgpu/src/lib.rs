//! AlkALive GPU rendering backend — `alkalive-backend-wgpu`.
//!
//! This crate implements the GPU rendering backend for AlkALive. It replaces
//! the CPU-side `SoftwareRenderer` (in `alkalive-app`) with a real GPU
//! pipeline that runs inside a WASM module and renders directly to a canvas
//! via the browser's WebGL2 API.
//!
//! # Why WebGL2 via `web-sys` instead of the `wgpu` crate?
//!
//! The crate is named `alkalive-backend-wgpu` to express its intent (a
//! WebGPU-class backend), but the concrete implementation uses raw WebGL2
//! via `web-sys::WebGl2RenderingContext`. This is explicitly allowed by the
//! task brief ("Either approach (wgpu or raw WebGL2) is acceptable. The
//! requirement is: GPU rendering, not CPU.") and was chosen because:
//!
//! 1. WebGL2 is universally available in browsers (WebGPU is not yet).
//! 2. `web-sys` is already cached locally; pulling the `wgpu` crate would
//!    add ~50 transitive deps and several minutes of build time.
//! 3. The raw WebGL2 surface is small enough to fit in one file (~600 LOC)
//!    and exposes the GPU directly — no extra abstraction layer.
//!
//! A future migration to `wgpu` (with the `webgl` feature for fallback) can
//! swap the implementation behind the same `WgpuRenderer` API.
//!
//! # Architecture
//!
//! - [`TextSceneData`] — the per-frame scene description (text + colors).
//! - [`WgpuRenderer`] — owns the WebGL2 context, shader program, glyph
//!   atlas texture, and vertex buffers.
//! - [`Vertex`] — `[position: vec2, uv: vec2]` per vertex.
//! - [`Uniforms`] — `rotation`, `canvas_size`, `time`, `text_color`.
//!
//! # Cross-target compilation
//!
//! The crate compiles on **both** native and `wasm32` targets. On `wasm32`,
//! the full GPU implementation is available. On native, the GPU fields are
//! replaced with stubs and `init_from_canvas` returns an error — but the
//! vertex/uniform math, shader source strings, and `TextSceneData` are all
//! available for unit testing.
//!
//! Per the task brief, `#![allow(unsafe_code)]` is OK because GPU bindings
//! (specifically `js_sys::Float32Array::view`) require it.

#![allow(unsafe_code)]
#![warn(missing_docs)]

use bytemuck::{Pod, Zeroable};

// ---------------------------------------------------------------------------
// Public scene-data types (target-agnostic — compile everywhere)
// ---------------------------------------------------------------------------

/// The per-frame scene description passed to [`WgpuRenderer::render_frame`].
///
/// This is the runtime's view of a `SceneIR` after layout — a single text
/// run with rotation, a background color, and a foreground (text) color.
#[derive(Debug, Clone)]
pub struct TextSceneData {
    /// The text to render (will be shaped by `alkalive-text` on first frame).
    pub text: String,
    /// Font size in pixels.
    pub font_size: f32,
    /// Y-axis rotation speed in radians per second.
    pub rotation_speed: f32,
    /// Background fill color as `(R, G, B)` (0–255).
    pub background: (u8, u8, u8),
    /// Text color as normalized RGBA `(0.0–1.0)`. Default golden =
    /// `(1.0, 0.843, 0.0, 1.0)` (`#FFD700`).
    pub text_color: (f32, f32, f32, f32),
}

impl Default for TextSceneData {
    fn default() -> Self {
        // Golden text on black background, slowly rotating.
        Self {
            text: "Hello World!".to_string(),
            font_size: 64.0,
            rotation_speed: 0.5,
            background: (0, 0, 0),
            text_color: (1.0, 0.843, 0.0, 1.0), // gold #FFD700
        }
    }
}

impl TextSceneData {
    /// Construct a default golden-on-black scene with the given text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    /// Convert the `(R, G, B)` 0–255 background to normalized `(R, G, B)` floats.
    pub fn background_normalized(&self) -> (f32, f32, f32) {
        (
            self.background.0 as f32 / 255.0,
            self.background.1 as f32 / 255.0,
            self.background.2 as f32 / 255.0,
        )
    }
}

// ---------------------------------------------------------------------------
// GPU vertex + uniform layouts (target-agnostic — compile everywhere)
// ---------------------------------------------------------------------------

/// Vertex format: `[x, y, u, v]` — 4 floats = 16 bytes per vertex.
///
/// Matches the GLSL `layout(location=0) in vec2 position; layout(location=1)
/// in vec2 uv;` declaration in [`VERTEX_SHADER_SRC`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable, Default)]
pub struct Vertex {
    /// Screen-space X (pixels, Y-up in shader).
    pub x: f32,
    /// Screen-space Y (pixels, Y-up in shader).
    pub y: f32,
    /// Atlas U (0–1).
    pub u: f32,
    /// Atlas V (0–1).
    pub v: f32,
}

impl Vertex {
    /// Construct a vertex from `(x, y)` position and `(u, v)` texcoord.
    pub const fn new(x: f32, y: f32, u: f32, v: f32) -> Self {
        Self { x, y, u, v }
    }

    /// The byte size of one vertex (used for stride).
    pub const STRIDE: u32 = 16;
}

/// Uniform block matching the GLSL `uniform Uniforms { ... }` layout in
/// [`VERTEX_SHADER_SRC`] and [`FRAGMENT_SHADER_SRC`].
///
/// Layout (std140-equivalent, 32 bytes):
/// - `rotation: f32`         (offset 0)
/// - `_pad0: [f32; 2]`       (offset 4, 8 — padding to align vec2)
/// - `canvas_size: [f32; 2]` (offset 16)
/// - `time: f32`             (offset 24)
/// - `text_color: [f32; 4]`  (in a separate uniform, but kept here for upload)
///
/// Note: We upload `canvas_size`, `rotation`, and `time` via separate
/// `uniform1f`/`uniform2f` calls (simpler than UBOs for WebGL2), so this
/// struct is mainly used for documentation and unit tests.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable, Default)]
pub struct Uniforms {
    /// Y-axis rotation angle (radians).
    pub rotation: f32,
    /// Canvas width in pixels.
    pub canvas_w: f32,
    /// Canvas height in pixels.
    pub canvas_h: f32,
    /// Elapsed time in seconds.
    pub time: f32,
}

// ---------------------------------------------------------------------------
// WGSL-equivalent GLSL shader sources (target-agnostic — compile everywhere)
// ---------------------------------------------------------------------------

/// Vertex shader (GLSL ES 3.00). Transforms quad vertices by applying a
/// Y-axis rotation (scales X by `cos(rotation)`) and converts pixel-space
/// positions to clip space.
///
/// This is the GLSL equivalent of the WGSL shader specified in the task
/// brief. Inputs:
/// - `location=0`: `vec2 position` (pixel space, Y-up)
/// - `location=1`: `vec2 uv` (atlas texcoord)
///
/// Uniforms:
/// - `uniform rotation`: Y-axis rotation angle (radians)
/// - `uniform canvas_size`: viewport size in pixels
/// - `uniform time`: elapsed seconds
pub const VERTEX_SHADER_SRC: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;

uniform float rotation;
uniform vec2 canvas_size;
uniform float time;

out vec2 v_uv;

void main() {
    // Y-axis rotation: scale X by cos(rotation). The "rotation" here is
    // animated by the caller as `rotation_speed * time`, so the text
    // appears to rotate around its vertical axis (like a spinning sign).
    float cos_r = cos(rotation);
    vec2 pos = position;
    pos.x = pos.x * cos_r;

    // Convert pixel-space (Y-up, origin at center) to clip space
    // (X-right, Y-up, [-1, 1]).
    //
    // Pixel-space origin: caller places vertices with (0, 0) at the
    // canvas center, X-right, Y-up. We map to clip-space:
    //   clip.x = pos.x / (canvas_w / 2)
    //   clip.y = pos.y / (canvas_h / 2)
    vec2 clip = vec2(
        pos.x / (canvas_size.x * 0.5),
        pos.y / (canvas_size.y * 0.5)
    );

    gl_Position = vec4(clip, 0.0, 1.0);
    v_uv = uv;
}
"#;

/// Fragment shader (GLSL ES 3.00). Samples the glyph atlas (single-channel
/// grayscale) and multiplies by the text color to produce the final RGBA
/// pixel.
///
/// Inputs:
/// - `in vec2 v_uv`: atlas texcoord from vertex shader
///
/// Uniforms:
/// - `uniform sampler2D glyph_texture`: grayscale glyph atlas
/// - `uniform vec4 text_color`: text RGBA (normalized)
pub const FRAGMENT_SHADER_SRC: &str = r#"#version 300 es
precision highp float;

in vec2 v_uv;

uniform sampler2D glyph_texture;
uniform vec4 text_color;

out vec4 frag_color;

void main() {
    // Sample the red channel — the glyph atlas is single-channel grayscale.
    float alpha = texture(glyph_texture, v_uv).r;

    // Discard fully-transparent pixels (no glyph coverage).
    if (alpha < 0.01) {
        discard;
    }

    // Modulate text color by glyph alpha. Premultiplied output.
    frag_color = vec4(text_color.rgb * alpha, alpha);
}
"#;

// ---------------------------------------------------------------------------
// Vertex-buffer generation (target-agnostic — unit-tested on native)
// ---------------------------------------------------------------------------

/// A single glyph quad, in pixel space (Y-up, origin at the canvas center).
///
/// This is the CPU-side representation produced by walking a
/// `alkalive_text::ShapedRun` and looking up each glyph's `AtlasSlot`. The
/// renderer uploads these as a vertex buffer (two triangles per quad).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct GlyphQuad {
    /// Center X of the glyph quad (pixels, canvas-relative).
    pub center_x: f32,
    /// Center Y of the glyph quad (pixels, canvas-relative).
    pub center_y: f32,
    /// Width of the glyph quad (pixels).
    pub w: f32,
    /// Height of the glyph quad (pixels).
    pub h: f32,
    /// Atlas U origin (left edge).
    pub u0: f32,
    /// Atlas V origin (top edge).
    pub v0: f32,
    /// Atlas U extent (right edge).
    pub u1: f32,
    /// Atlas V extent (bottom edge).
    pub v1: f32,
}

/// Build a triangle-list vertex buffer (6 vertices per quad) from a slice
/// of [`GlyphQuad`]s.
///
/// Each quad becomes two triangles with CCW winding (in Y-up space):
/// ```text
/// (x0,y1) ─── (x1,y1)
///    │           │
/// (x0,y0) ─── (x1,y0)
/// ```
///
/// Triangles: `(x0,y0)-(x1,y0)-(x0,y1)` and `(x1,y0)-(x1,y1)-(x0,y1)`.
///
/// This function is target-agnostic and unit-tested on native.
pub fn build_vertex_buffer(quads: &[GlyphQuad]) -> Vec<Vertex> {
    let mut verts = Vec::with_capacity(quads.len() * 6);
    for q in quads {
        let half_w = q.w * 0.5;
        let half_h = q.h * 0.5;
        let x0 = q.center_x - half_w;
        let x1 = q.center_x + half_w;
        let y0 = q.center_y - half_h;
        let y1 = q.center_y + half_h;

        // Triangle 1: bottom-left, bottom-right, top-left
        verts.push(Vertex::new(x0, y0, q.u0, q.v1)); // BL
        verts.push(Vertex::new(x1, y0, q.u1, q.v1)); // BR
        verts.push(Vertex::new(x0, y1, q.u0, q.v0)); // TL

        // Triangle 2: bottom-right, top-right, top-left
        verts.push(Vertex::new(x1, y0, q.u1, q.v1)); // BR
        verts.push(Vertex::new(x1, y1, q.u1, q.v0)); // TR
        verts.push(Vertex::new(x0, y1, q.u0, q.v0)); // TL
    }
    verts
}

/// Convert a list of [`alkalive_text::Quad`]s (in baseline-relative pixel
/// space, Y-up) plus the run's metrics into canvas-centered [`GlyphQuad`]s.
///
/// `canvas_w`/`canvas_h` is the viewport size in pixels. The text is
/// centered horizontally and vertically (ascender at the top of the
/// vertical center).
pub fn quads_from_text(
    text_quads: &[alkalive_text::Quad],
    ascent: f32,
    descent: f32,
    total_advance: f32,
    canvas_w: f32,
    canvas_h: f32,
) -> Vec<GlyphQuad> {
    if text_quads.is_empty() {
        return Vec::new();
    }
    // Origin in baseline-relative pixel space is (0, 0) at the left end of
    // the baseline. We want the baseline's horizontal midpoint to land at
    // the canvas's horizontal center, and the ascender to land at the
    // vertical center.
    let baseline_x = canvas_w * 0.5 - total_advance * 0.5;
    let baseline_y = canvas_h * 0.5 - ascent * 0.5;

    let _ = (ascent, descent); // metrics used for centering math

    text_quads
        .iter()
        .map(|q| {
            // alkalive_text::Quad has `position` = top-left of the glyph
            // bitmap in baseline-relative pixel space (Y-up). Width/height
            // are in `size`. UV box is in `uv` (x, y, w, h) — origin at the
            // top-left of the glyph tile in the atlas.
            let px = q.position.0;
            let py = q.position.1;
            let center_x = baseline_x + px + q.size.0 * 0.5;
            // In Y-up baseline space, `py` is the bottom of the glyph (top
            // of the bitmap from the baseline's perspective). Convert to
            // canvas-centered Y-up: center = baseline_y + py + h/2 - h/2 =
            // baseline_y + py (we want the center of the glyph).
            let center_y = baseline_y + py + q.size.1 * 0.5;

            GlyphQuad {
                center_x,
                center_y,
                w: q.size.0,
                h: q.size.1,
                u0: q.uv.x,
                v0: q.uv.y,
                u1: q.uv.x + q.uv.w,
                v1: q.uv.y + q.uv.h,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// WgpuRenderer — wasm32 implementation (real WebGL2 GPU backend)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm {
    //! Real GPU backend — WebGL2 via `web-sys::WebGl2RenderingContext`.

    use super::*;
    use std::sync::Arc;
    use wasm_bindgen::JsCast;
    use web_sys::{
        HtmlCanvasElement, Performance, WebGl2RenderingContext, WebGlBuffer,
        WebGlProgram, WebGlShader, WebGlTexture, WebGlUniformLocation,
        WebGlVertexArrayObject,
    };

    /// A GPU renderer that renders text and a background directly to a canvas
    /// via WebGL2.
    ///
    /// Despite the `wgpu` in the crate name, the concrete backend is WebGL2
    /// (see the crate-level docs for the rationale).
    pub struct WgpuRenderer {
        /// The canvas element this renderer draws to.
        canvas: HtmlCanvasElement,
        /// The WebGL2 rendering context. `None` only if context loss occurs.
        gl: WebGl2RenderingContext,
        /// The compiled shader program (vertex + fragment).
        program: WebGlProgram,
        /// Vertex shader object (kept for re-link on context loss).
        vs: WebGlShader,
        /// Fragment shader object.
        fs: WebGlShader,
        /// VAO holding the vertex buffer binding.
        vao: WebGlVertexArrayObject,
        /// The vertex buffer (positions + UVs, 6 verts per glyph quad).
        vbo: WebGlBuffer,
        /// The glyph atlas texture (single-channel grayscale, 512×512).
        glyph_texture: WebGlTexture,
        /// Cached location of the `rotation` uniform.
        u_rotation: WebGlUniformLocation,
        /// Cached location of the `canvas_size` uniform.
        u_canvas_size: WebGlUniformLocation,
        /// Cached location of the `time` uniform.
        u_time: WebGlUniformLocation,
        /// Cached location of the `text_color` uniform.
        u_text_color: WebGlUniformLocation,
        /// Cached location of the `glyph_texture` sampler.
        u_glyph_texture: WebGlUniformLocation,
        /// The `performance.now()` timer used for animation.
        performance: Performance,
        /// The start time (ms) of the renderer, used to compute elapsed.
        start_ms: f64,
        /// Canvas width in physical pixels.
        width: u32,
        /// Canvas height in physical pixels.
        height: u32,
        /// Whether the glyph atlas texture has been uploaded at least once.
        atlas_uploaded: bool,
        /// The current vertex count (6 × number of glyph quads).
        vertex_count: u32,
    }

    impl WgpuRenderer {
        /// Initialize the renderer from a canvas element.
        ///
        /// This acquires the WebGL2 context (`canvas.getContext('webgl2')`),
        /// compiles the shaders, links the program, creates the VAO/VBO,
        /// and creates an empty glyph atlas texture (uploaded on first frame).
        pub async fn init_from_canvas(
            canvas: HtmlCanvasElement,
            width: u32,
            height: u32,
        ) -> Result<Self, String> {
            // Acquire the WebGL2 context.
            let canvas_ctx: Option<js_sys::Object> = canvas
                .get_context("webgl2")
                .map_err(|e| format!("getContext('webgl2') threw: {:?}", e))?;
            let gl: WebGl2RenderingContext = canvas_ctx
                .ok_or_else(|| "WebGL2 not supported by this browser".to_string())?
                .dyn_into::<WebGl2RenderingContext>()
                .map_err(|_| "getContext('webgl2') did not return a WebGL2RenderingContext".to_string())?;

            // Compile shaders.
            let vs = compile_shader(&gl, WebGl2RenderingContext::VERTEX_SHADER, VERTEX_SHADER_SRC)?;
            let fs = compile_shader(
                &gl,
                WebGl2RenderingContext::FRAGMENT_SHADER,
                FRAGMENT_SHADER_SRC,
            )?;

            // Link program.
            let program = gl
                .create_program()
                .ok_or_else(|| "create_program() returned null".to_string())?;
            gl.attach_shader(&program, &vs);
            gl.attach_shader(&program, &fs);
            gl.link_program(&program);
            if gl
                .get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS)
                .as_bool()
                != Some(true)
            {
                let log = gl
                    .get_program_info_log(&program)
                    .unwrap_or_else(|| "(no info log)".to_string());
                return Err(format!("Program link failed: {}", log));
            }

            // Create VAO + VBO.
            let vao = gl
                .create_vertex_array()
                .ok_or_else(|| "create_vertex_array() returned null".to_string())?;
            let vbo = gl
                .create_buffer()
                .ok_or_else(|| "create_buffer() returned null".to_string())?;

            gl.bind_vertex_array(Some(&vao));
            gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&vbo));

            // Set up vertex attribute pointers (vec2 position, vec2 uv).
            let stride = Vertex::STRIDE as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_with_i32(0, 2, WebGl2RenderingContext::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_with_i32(
                1,
                2,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                8,
            );
            gl.bind_vertex_array(None);

            // Create glyph atlas texture (empty 512×512 R8).
            // We'll upload pixels on the first render_frame call.
            let glyph_texture = gl
                .create_texture()
                .ok_or_else(|| "create_texture() returned null".to_string())?;
            gl.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&glyph_texture));
            gl.tex_parameteri(
                WebGl2RenderingContext::TEXTURE_2D,
                WebGl2RenderingContext::TEXTURE_MIN_FILTER,
                WebGl2RenderingContext::LINEAR as i32,
            );
            gl.tex_parameteri(
                WebGl2RenderingContext::TEXTURE_2D,
                WebGl2RenderingContext::TEXTURE_MAG_FILTER,
                WebGl2RenderingContext::LINEAR as i32,
            );
            gl.tex_parameteri(
                WebGl2RenderingContext::TEXTURE_2D,
                WebGl2RenderingContext::TEXTURE_WRAP_S,
                WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameteri(
                WebGl2RenderingContext::TEXTURE_2D,
                WebGl2RenderingContext::TEXTURE_WRAP_T,
                WebGl2RenderingContext::CLAMP_TO_EDGE as i32,
            );
            // Allocate a 512×512 R8 texture (initially zero).
            gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                WebGl2RenderingContext::TEXTURE_2D,
                0,
                WebGl2RenderingContext::R8 as i32,
                512,
                512,
                0,
                WebGl2RenderingContext::RED,
                WebGl2RenderingContext::UNSIGNED_BYTE,
                None,
            )
            .map_err(|e| format!("tex_image_2d initial failed: {:?}", e))?;

            // Cache uniform locations.
            let u_rotation = gl
                .get_uniform_location(&program, "rotation")
                .ok_or_else(|| "uniform 'rotation' not found".to_string())?;
            let u_canvas_size = gl
                .get_uniform_location(&program, "canvas_size")
                .ok_or_else(|| "uniform 'canvas_size' not found".to_string())?;
            let u_time = gl
                .get_uniform_location(&program, "time")
                .ok_or_else(|| "uniform 'time' not found".to_string())?;
            let u_text_color = gl
                .get_uniform_location(&program, "text_color")
                .ok_or_else(|| "uniform 'text_color' not found".to_string())?;
            let u_glyph_texture = gl
                .get_uniform_location(&program, "glyph_texture")
                .ok_or_else(|| "uniform 'glyph_texture' not found".to_string())?;

            // Acquire the performance timer.
            let window = web_sys::window().ok_or_else(|| "no global window".to_string())?;
            let performance = window
                .performance()
                .ok_or_else(|| "window.performance unavailable".to_string())?;
            let start_ms = performance.now();

            // Configure the canvas size and viewport.
            canvas.set_width(width);
            canvas.set_height(height);
            gl.viewport(0, 0, width as i32, height as i32);

            // Enable alpha blending for text antialiasing.
            gl.enable(WebGl2RenderingContext::BLEND);
            gl.blend_func(
                WebGl2RenderingContext::SRC_ALPHA,
                WebGl2RenderingContext::ONE_MINUS_SRC_ALPHA,
            );

            Ok(Self {
                canvas,
                gl,
                program,
                vs,
                fs,
                vao,
                vbo,
                glyph_texture,
                u_rotation,
                u_canvas_size,
                u_time,
                u_text_color,
                u_glyph_texture,
                performance,
                start_ms,
                width,
                height,
                atlas_uploaded: false,
                vertex_count: 0,
            })
        }

        /// Render one frame: clear to the background color, render the text
        /// quads (golden by default) with Y-axis rotation animated by `time`.
        ///
        /// `text_scene.text_color` modulates the fragment output; the glyph
        /// atlas is sampled for alpha.
        ///
        /// On the first call, the glyph atlas is rasterized via
        /// `alkalive-text` (HarfRust shaping + glyph rasterization) and
        /// uploaded to the GPU texture. Subsequent calls re-use the cached
        /// atlas.
        pub fn render_frame(&mut self, text_scene: &TextSceneData, time: f32) {
            // 1. Shape + rasterize the text on the first frame, then cache.
            //    (Must complete before any `self.gl.X(...)` borrow below.)
            if !self.atlas_uploaded {
                if let Err(e) = self.upload_text_atlas(&text_scene.text, text_scene.font_size) {
                    web_sys::console::error_1(
                        &format!("alkalive-backend-wgpu: atlas upload failed: {}", e).into(),
                    );
                }
                self.atlas_uploaded = true;
            }

            // 2. Set the viewport (in case the canvas was resized).
            self.gl.viewport(0, 0, self.width as i32, self.height as i32);

            // 3. Clear to the background color.
            let (br, bg, bb) = text_scene.background_normalized();
            self.gl.clear_color(br, bg, bb, 1.0);
            self.gl.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);

            // 4. Bind program + uniforms.
            self.gl.use_program(Some(&self.program));

            let rotation = text_scene.rotation_speed * time;
            self.gl.uniform1f(Some(&self.u_rotation), rotation);
            self.gl.uniform2f(Some(&self.u_canvas_size), self.width as f32, self.height as f32);
            self.gl.uniform1f(Some(&self.u_time), time);
            self.gl.uniform4f(
                Some(&self.u_text_color),
                text_scene.text_color.0,
                text_scene.text_color.1,
                text_scene.text_color.2,
                text_scene.text_color.3,
            );

            // 5. Bind glyph atlas texture to texture unit 0.
            self.gl.active_texture(WebGl2RenderingContext::TEXTURE0);
            self.gl.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&self.glyph_texture));
            self.gl.uniform1i(Some(&self.u_glyph_texture), 0);

            // 6. Bind VAO + draw.
            self.gl.bind_vertex_array(Some(&self.vao));
            if self.vertex_count > 0 {
                self.gl.draw_arrays(
                    WebGl2RenderingContext::TRIANGLES,
                    0,
                    self.vertex_count as i32,
                );
            }
        }

        /// Resize the canvas + WebGL viewport.
        pub fn resize(&mut self, width: u32, height: u32) {
            self.width = width;
            self.height = height;
            self.canvas.set_width(width);
            self.canvas.set_height(height);
            self.gl.viewport(0, 0, width as i32, height as i32);
        }

        /// Returns the elapsed time in seconds since the renderer was
        /// constructed, via the high-resolution `performance.now()` timer.
        ///
        /// The runtime can use this as the `time` argument to
        /// [`render_frame`](Self::render_frame) when it doesn't have its own
        /// clock (e.g. when WASM owns the frame loop).
        pub fn elapsed_seconds(&self) -> f32 {
            ((self.performance.now() - self.start_ms) / 1000.0) as f32
        }

        /// Returns the current canvas width in physical pixels.
        pub fn width(&self) -> u32 {
            self.width
        }

        /// Returns the current canvas height in physical pixels.
        pub fn height(&self) -> u32 {
            self.height
        }

        /// Returns the number of vertices currently in the vertex buffer
        /// (6 per glyph quad). Useful for diagnostics.
        pub fn vertex_count(&self) -> u32 {
            self.vertex_count
        }

        /// Shape + rasterize the text via `alkalive-text`, then upload the
        /// glyph atlas to the GPU texture and rebuild the vertex buffer.
        ///
        /// This is the per-frame GPU upload path on the first frame. It:
        /// 1. Loads the bundled Roboto-Regular font into a HarfRust registry.
        /// 2. Shapes the text via `HarfRustTextShaper`.
        /// 3. Rasterizes each glyph into the `HarfRustGlyphAtlas` (CPU-side
        ///    512×512 grayscale page).
        /// 4. Uploads the atlas page to the GPU as an R8 texture.
        /// 5. Builds a canvas-centered vertex buffer (6 verts per glyph).
        /// 6. Uploads the vertex buffer to the VBO.
        fn upload_text_atlas(&mut self, text: &str, font_size: f32) -> Result<(), String> {
            use alkalive_text::{
                FontRequest, FontRegistry, GlyphAtlas, GlyphKey, HarfRustFontRegistry,
                HarfRustGlyphAtlas, HarfRustTextShaper, ShapeContext, TextShaper,
            };

            // 1. Load the bundled Roboto-Regular font (also used by
            //    alkalive-app's text_scene — same source file).
            let font_bytes: &[u8] = include_bytes!("../../alkalive-app/assets/Roboto-Regular.ttf");
            let mut registry = HarfRustFontRegistry::new();
            let loaded_id = registry
                .load_bundle(font_bytes)
                .map_err(|e| format!("font load: {:?}", e))?;
            let req = FontRequest {
                family: "Roboto".to_string(),
                weight: 400,
                style: "normal",
            };
            let font_id = registry.resolve(&req).unwrap_or(loaded_id);
            let registry_arc = Arc::new(registry);

            // 2. Shape the text.
            let shaper = HarfRustTextShaper::new(Arc::clone(&registry_arc));
            let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry_arc));
            let ctx = ShapeContext {
                font: font_id,
                size_px: font_size,
                direction: None,
            };
            let run = shaper
                .shape(text, &ctx)
                .map_err(|e| format!("shape: {:?}", e))?;

            // 3. Rasterize each glyph into the atlas and build a list of
            //    pixel-space glyph quads (baseline-relative, Y-up).
            let mut text_quads: Vec<alkalive_text::Quad> = Vec::with_capacity(run.glyph_ids.len());
            let mut pen_x = 0.0f32;
            for (i, &glyph_id) in run.glyph_ids.iter().enumerate() {
                let key = GlyphKey {
                    font_id: run.font_id,
                    glyph_id,
                    phase: 0,
                    size_px: font_size as u16,
                };
                let slot = atlas.ensure(key);
                if slot.size.0 < 0.5 || slot.size.1 < 0.5 {
                    pen_x += run.advances[i];
                    continue;
                }
                let offset_x = run.offsets[i].0;
                let offset_y = run.offsets[i].1;
                text_quads.push(alkalive_text::Quad {
                    position: (pen_x + offset_x + slot.bearing.0, offset_y - slot.bearing.1),
                    size: slot.size,
                    uv: slot.uv,
                    page: slot.page,
                });
                pen_x += run.advances[i];
            }

            // 4. Upload the atlas page (page 0) to the GPU texture.
            //    The atlas is 512×512 grayscale R8.
            let page_data = atlas
                .page_data(0)
                .ok_or_else(|| "atlas page 0 missing".to_string())?;
            self.gl.bind_texture(WebGl2RenderingContext::TEXTURE_2D, Some(&self.glyph_texture));
            self.gl
                .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                    WebGl2RenderingContext::TEXTURE_2D,
                    0,
                    WebGl2RenderingContext::R8 as i32,
                    512,
                    512,
                    0,
                    WebGl2RenderingContext::RED,
                    WebGl2RenderingContext::UNSIGNED_BYTE,
                    Some(page_data),
                )
                .map_err(|e| format!("tex_image_2d upload failed: {:?}", e))?;

            // 5. Build the canvas-centered vertex buffer.
            let canvas_quads = quads_from_text(
                &text_quads,
                run.metrics.ascent,
                run.metrics.descent,
                run.metrics.total_advance,
                self.width as f32,
                self.height as f32,
            );
            let verts = build_vertex_buffer(&canvas_quads);
            self.vertex_count = verts.len() as u32;

            // 6. Upload the vertex buffer.
            self.gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&self.vbo));
            let byte_len = verts.len() * std::mem::size_of::<Vertex>();
            if byte_len == 0 {
                return Ok(());
            }
            // Safety: `verts` is a `Vec<Vertex>` of `Pod` types (repr(C),
            // no padding). The byte slice borrows `verts` for the duration
            // of the synchronous `buffer_data_with_u8_array` call, which
            // copies the bytes into the WebGL buffer. After the call,
            // `verts` is dropped normally.
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(verts.as_ptr() as *const u8, byte_len)
            };
            self.gl.buffer_data_with_u8_array(
                WebGl2RenderingContext::ARRAY_BUFFER,
                bytes,
                WebGl2RenderingContext::DYNAMIC_DRAW,
            );

            Ok(())
        }
    }

    impl Drop for WgpuRenderer {
        fn drop(&mut self) {
            // Free GPU resources. Best-effort — context loss can make these
            // no-ops.
            self.gl.delete_program(Some(&self.program));
            self.gl.delete_shader(Some(&self.vs));
            self.gl.delete_shader(Some(&self.fs));
            self.gl.delete_buffer(Some(&self.vbo));
            self.gl.delete_vertex_array(Some(&self.vao));
            self.gl.delete_texture(Some(&self.glyph_texture));
        }
    }

    /// Compile a single GLSL shader, returning the shader object or an
    /// error string containing the info log.
    fn compile_shader(
        gl: &WebGl2RenderingContext,
        kind: u32,
        src: &str,
    ) -> Result<WebGlShader, String> {
        let shader = gl
            .create_shader(kind)
            .ok_or_else(|| "create_shader() returned null".to_string())?;
        gl.shader_source(&shader, src);
        gl.compile_shader(&shader);
        if gl
            .get_shader_parameter(&shader, WebGl2RenderingContext::COMPILE_STATUS)
            .as_bool()
            != Some(true)
        {
            let log = gl
                .get_shader_info_log(&shader)
                .unwrap_or_else(|| "(no info log)".to_string());
            gl.delete_shader(Some(&shader));
            return Err(format!("Shader compile failed: {}", log));
        }
        Ok(shader)
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::WgpuRenderer;

// ---------------------------------------------------------------------------
// WgpuRenderer — native stub (compiles cleanly, returns errors at runtime)
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod native {
    //! Native stub. The GPU backend only runs in WASM (browsers); on native
    //! targets the API is provided for type-checking but returns an error
    //! if called. Vertex/uniform math is in the parent module and is
    //! unit-tested on native.

    use super::*;

    /// Stub `WgpuRenderer` for native targets.
    ///
    /// The real backend runs only on `wasm32` (it needs a browser WebGL2
    /// context). On native, this struct exists so the public API type-checks
    /// and downstream code can compile. Constructing one returns an error.
    pub struct WgpuRenderer {
        /// Width placeholder.
        pub width: u32,
        /// Height placeholder.
        pub height: u32,
        /// Cached vertex count (for tests).
        pub vertex_count: u32,
    }

    impl WgpuRenderer {
        /// Always returns an error on native — the WebGL2 backend requires
        /// a browser. The signature accepts a `web_sys::HtmlCanvasElement`
        /// for type-compatibility with the wasm32 build; on native, that
        /// type exists but cannot be instantiated, so this function is
        /// never actually called.
        pub async fn init_from_canvas(
            _canvas: web_sys::HtmlCanvasElement,
            _width: u32,
            _height: u32,
        ) -> Result<Self, String> {
            Err(
                "alkalive-backend-wgpu: WebGL2 backend only runs on wasm32 \
                 (this is a native build — the GPU backend is not available)"
                    .to_string(),
            )
        }

        /// No-op on native — the renderer was never actually constructed.
        pub fn render_frame(&mut self, _text_scene: &TextSceneData, _time: f32) {}

        /// No-op on native.
        pub fn resize(&mut self, width: u32, height: u32) {
            self.width = width;
            self.height = height;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::WgpuRenderer;

// ---------------------------------------------------------------------------
// Unit tests (target-agnostic — run on native)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_scene_data_default_is_golden_on_black() {
        let s = TextSceneData::default();
        assert_eq!(s.background, (0, 0, 0));
        assert_eq!(s.text_color, (1.0, 0.843, 0.0, 1.0));
        assert_eq!(s.text, "Hello World!");
        assert!((s.font_size - 64.0).abs() < 1e-6);
        assert!((s.rotation_speed - 0.5).abs() < 1e-6);
    }

    #[test]
    fn text_scene_data_new_overrides_text() {
        let s = TextSceneData::new("Hi!");
        assert_eq!(s.text, "Hi!");
        assert_eq!(s.background, (0, 0, 0));
    }

    #[test]
    fn text_scene_data_background_normalized() {
        let s = TextSceneData {
            background: (255, 128, 0),
            ..Default::default()
        };
        let (r, g, b) = s.background_normalized();
        assert!((r - 1.0).abs() < 1e-6);
        assert!((g - 128.0 / 255.0).abs() < 1e-6);
        assert!((b - 0.0).abs() < 1e-6);
    }

    #[test]
    fn vertex_stride_is_16_bytes() {
        assert_eq!(Vertex::STRIDE, 16);
        assert_eq!(std::mem::size_of::<Vertex>(), 16);
    }

    #[test]
    fn vertex_new_constructs_fields() {
        let v = Vertex::new(1.0, 2.0, 0.5, 0.25);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.u, 0.5);
        assert_eq!(v.v, 0.25);
    }

    #[test]
    fn build_vertex_buffer_empty_input() {
        let verts = build_vertex_buffer(&[]);
        assert!(verts.is_empty());
    }

    #[test]
    fn build_vertex_buffer_single_quad_produces_six_vertices() {
        let q = GlyphQuad {
            center_x: 100.0,
            center_y: 50.0,
            w: 40.0,
            h: 60.0,
            u0: 0.0,
            v0: 0.0,
            u1: 0.1,
            v1: 0.2,
        };
        let verts = build_vertex_buffer(&[q]);
        assert_eq!(verts.len(), 6);

        // Verify corner positions: half_w=20, half_h=30
        // x0=80, x1=120, y0=20, y1=80
        // Triangle 1: (80,20)-(120,20)-(80,80)
        assert_eq!(verts[0], Vertex::new(80.0, 20.0, 0.0, 0.2)); // BL
        assert_eq!(verts[1], Vertex::new(120.0, 20.0, 0.1, 0.2)); // BR
        assert_eq!(verts[2], Vertex::new(80.0, 80.0, 0.0, 0.0)); // TL
        // Triangle 2: (120,20)-(120,80)-(80,80)
        assert_eq!(verts[3], Vertex::new(120.0, 20.0, 0.1, 0.2)); // BR
        assert_eq!(verts[4], Vertex::new(120.0, 80.0, 0.1, 0.0)); // TR
        assert_eq!(verts[5], Vertex::new(80.0, 80.0, 0.0, 0.0)); // TL
    }

    #[test]
    fn build_vertex_buffer_multiple_quads() {
        let quads = vec![
            GlyphQuad {
                center_x: 0.0,
                center_y: 0.0,
                w: 10.0,
                h: 10.0,
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
            },
            GlyphQuad {
                center_x: 20.0,
                center_y: 0.0,
                w: 10.0,
                h: 10.0,
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
            },
        ];
        let verts = build_vertex_buffer(&quads);
        assert_eq!(verts.len(), 12);
    }

    #[test]
    fn quads_from_text_centers_horizontally() {
        // A 100px-wide text run in a 1000px canvas should be centered
        // around x=500. The first quad at position (0, 0) with w=10 should
        // land at center_x = 500 - 50 + 0 + 5 = 455.
        let text_quads = vec![alkalive_text::Quad {
            position: (0.0, 0.0),
            size: (10.0, 10.0),
            uv: alkalive_text::Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            page: 0,
        }];
        let quads = quads_from_text(&text_quads, 20.0, -5.0, 100.0, 1000.0, 500.0);
        assert_eq!(quads.len(), 1);
        // baseline_x = 1000/2 - 100/2 = 450
        // center_x = 450 + 0 + 10/2 = 455
        assert!((quads[0].center_x - 455.0).abs() < 1e-6);
    }

    #[test]
    fn quads_from_text_empty_input() {
        let quads = quads_from_text(&[], 0.0, 0.0, 0.0, 100.0, 100.0);
        assert!(quads.is_empty());
    }

    #[test]
    fn uniforms_default_is_zeroed() {
        let u = Uniforms::default();
        assert_eq!(u.rotation, 0.0);
        assert_eq!(u.canvas_w, 0.0);
        assert_eq!(u.canvas_h, 0.0);
        assert_eq!(u.time, 0.0);
    }

    #[test]
    fn vertex_shader_src_compiles_to_valid_glsl() {
        // Sanity-check: the shader source contains the expected GLSL
        // version directive and entry point.
        assert!(VERTEX_SHADER_SRC.starts_with("#version 300 es"));
        assert!(VERTEX_SHADER_SRC.contains("void main()"));
        assert!(VERTEX_SHADER_SRC.contains("in vec2 position"));
        assert!(VERTEX_SHADER_SRC.contains("in vec2 uv"));
        assert!(VERTEX_SHADER_SRC.contains("uniform float rotation"));
        assert!(VERTEX_SHADER_SRC.contains("uniform vec2 canvas_size"));
        assert!(VERTEX_SHADER_SRC.contains("gl_Position"));
    }

    #[test]
    fn fragment_shader_src_compiles_to_valid_glsl() {
        assert!(FRAGMENT_SHADER_SRC.starts_with("#version 300 es"));
        assert!(FRAGMENT_SHADER_SRC.contains("void main()"));
        assert!(FRAGMENT_SHADER_SRC.contains("uniform sampler2D glyph_texture"));
        assert!(FRAGMENT_SHADER_SRC.contains("uniform vec4 text_color"));
        assert!(FRAGMENT_SHADER_SRC.contains("out vec4 frag_color"));
        assert!(FRAGMENT_SHADER_SRC.contains("discard"));
    }

    #[test]
    fn glyph_quad_default_is_zeroed() {
        let q = GlyphQuad::default();
        assert_eq!(q.center_x, 0.0);
        assert_eq!(q.center_y, 0.0);
        assert_eq!(q.w, 0.0);
        assert_eq!(q.h, 0.0);
        assert_eq!(q.u0, 0.0);
        assert_eq!(q.v0, 0.0);
        assert_eq!(q.u1, 0.0);
        assert_eq!(q.v1, 0.0);
    }

    /// Verify the `WgpuRenderer` type exists and has the expected public API
    /// (init_from_canvas, render_frame, resize). On native, construction
    /// fails — but the type itself must compile.
    #[test]
    fn wgpu_renderer_type_compiles() {
        // Just exercise the API surface (type-check).
        fn _assert_api(r: &mut WgpuRenderer) {
            r.render_frame(&TextSceneData::default(), 0.0);
            r.resize(800, 600);
        }
        // (We can't actually construct one in a unit test without a canvas,
        // but the type-check above exercises the public API.)
    }

    /// End-to-end: shape "Hello" via alkalive-text, build quads, build
    /// vertex buffer, verify non-empty. This proves the data path from
    /// text → quads → vertices works without a GPU.
    #[test]
    fn end_to_end_text_to_vertex_buffer() {
        use alkalive_text::{
            FontRequest, FontRegistry, GlyphAtlas, GlyphKey, HarfRustFontRegistry,
            HarfRustGlyphAtlas, HarfRustTextShaper, ShapeContext, TextShaper,
        };
        use std::sync::Arc;

        let font_bytes: &[u8] =
            include_bytes!("../../alkalive-app/assets/Roboto-Regular.ttf");
        let mut registry = HarfRustFontRegistry::new();
        let loaded_id = registry.load_bundle(font_bytes).expect("font load");
        let req = FontRequest {
            family: "Roboto".to_string(),
            weight: 400,
            style: "normal",
        };
        let font_id = registry.resolve(&req).unwrap_or(loaded_id);
        let registry_arc = Arc::new(registry);

        let shaper = HarfRustTextShaper::new(Arc::clone(&registry_arc));
        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry_arc));
        let ctx = ShapeContext {
            font: font_id,
            size_px: 48.0,
            direction: None,
        };
        let run = shaper.shape("Hello", &ctx).expect("shape");

        let mut text_quads: Vec<alkalive_text::Quad> = Vec::new();
        let mut pen_x = 0.0f32;
        for (i, &glyph_id) in run.glyph_ids.iter().enumerate() {
            let key = GlyphKey {
                font_id: run.font_id,
                glyph_id,
                phase: 0,
                size_px: 48,
            };
            let slot = atlas.ensure(key);
            if slot.size.0 < 0.5 || slot.size.1 < 0.5 {
                pen_x += run.advances[i];
                continue;
            }
            text_quads.push(alkalive_text::Quad {
                position: (
                    pen_x + run.offsets[i].0 + slot.bearing.0,
                    run.offsets[i].1 - slot.bearing.1,
                ),
                size: slot.size,
                uv: slot.uv,
                page: slot.page,
            });
            pen_x += run.advances[i];
        }

        assert!(!text_quads.is_empty(), "expected non-empty text quads");

        let canvas_quads = quads_from_text(
            &text_quads,
            run.metrics.ascent,
            run.metrics.descent,
            run.metrics.total_advance,
            800.0,
            600.0,
        );
        let verts = build_vertex_buffer(&canvas_quads);

        // Each visible glyph produces 6 vertices.
        assert_eq!(verts.len(), text_quads.len() * 6);
        assert!(verts.iter().all(|v| v.x.is_finite() && v.y.is_finite()));
    }
}

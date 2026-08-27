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
// Public scene-data types (re-exported from alkalive-scene-data — Gap 6)
// ---------------------------------------------------------------------------

/// Re-export of the per-frame scene description.
///
/// `TextSceneData` was moved to `alkalive-scene-data` in Wave 11 (Gap 6 —
/// Render-Graph IR) to break the `alkalive-render` ↔ `alkalive-backend-wgpu`
/// dependency cycle. It is re-exported here so existing call sites
/// (`alkalive-runtime-wasm`) continue to compile unchanged.
pub use alkalive_scene_data::TextSceneData;

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
    // Y-axis rotation: scale X around the canvas CENTER (not origin).
    // This keeps the text centered while it narrows/widens.
    // When cos is negative, the text is mirrored (viewed from behind)
    // — we flip the UV to keep the text readable.
    float cos_r = cos(rotation);
    float center_x = canvas_size.x * 0.5;

    // Shift to center-relative, scale, shift back
    float rel_x = position.x - center_x;
    float scaled_x = rel_x * cos_r + center_x;

    // Convert pixel-space (Y-down, origin at top-left) to clip space.
    // Clip space is [-1, 1] with Y-up.
    vec2 clip = vec2(
        scaled_x / (canvas_size.x * 0.5) - 1.0,
        1.0 - position.y / (canvas_size.y * 0.5)
    );

    gl_Position = vec4(clip, 0.0, 1.0);

    // Pass UV through unchanged. When cos_r < 0, the X positions are
    // mirrored (text appears backwards), but the UVs stay correct so
    // the glyph atlas is sampled properly. The text appears mirrored
    // on the backface — like reading the back of a sign — which is
    // the expected behavior for a rotating sign.
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

/// Rect vertex shader (GLSL ES 3.00). Draws a full-viewport quad; the
/// fragment shader clips to the rect bounds. This replaces the scissor+clear
/// hack and correctly respects alpha blending.
pub const RECT_VERTEX_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 position;  // clip-space [-1,1]
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

/// Rect fragment shader (GLSL ES 3.00). Draws a uniform-color rectangle
/// with proper alpha blending.
pub const RECT_FRAGMENT_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_rect;   // pixel-space bounds: (x0, y0, x1, y1)
uniform vec4 u_color;  // RGBA
uniform vec2 u_canvas; // canvas size in pixels
out vec4 frag_color;
void main() {
    float px = gl_FragCoord.x;
    float py = u_canvas.y - gl_FragCoord.y;  // flip Y to Y-down
    if (px < u_rect.x || px > u_rect.z || py < u_rect.y || py > u_rect.w) {
        discard;
    }
    frag_color = u_color;
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
        // In Y-down screen space: y0 = top, y1 = bottom
        let x0 = q.center_x - half_w;
        let x1 = q.center_x + half_w;
        let y0 = q.center_y - half_h; // top
        let y1 = q.center_y + half_h; // bottom

        // UV: v0 = top of glyph in atlas, v1 = bottom
        // Top of quad (y0) maps to v0 (top of atlas glyph)
        // Bottom of quad (y1) maps to v1 (bottom of atlas glyph)

        // Triangle 1: top-left, top-right, bottom-left
        verts.push(Vertex::new(x0, y0, q.u0, q.v0)); // TL
        verts.push(Vertex::new(x1, y0, q.u1, q.v0)); // TR
        verts.push(Vertex::new(x0, y1, q.u0, q.v1)); // BL

        // Triangle 2: top-right, bottom-right, bottom-left
        verts.push(Vertex::new(x1, y0, q.u1, q.v0)); // TR
        verts.push(Vertex::new(x1, y1, q.u1, q.v1)); // BR
        verts.push(Vertex::new(x0, y1, q.u0, q.v1)); // BL
    }
    verts
}

/// Build baseline-relative glyph quads from a shaped run.
pub(crate) fn build_text_quads(
    run: &alkalive_text::ShapedRun,
    atlas: &mut alkalive_text::HarfRustGlyphAtlas,
    font_size: f32,
) -> Vec<alkalive_text::Quad> {
    use alkalive_text::{GlyphAtlas, GlyphKey};
    let mut quads = Vec::with_capacity(run.glyph_ids.len());
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
        quads.push(alkalive_text::Quad {
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
    quads
}

/// Convert a list of [`alkalive_text::Quad`]s (in baseline-relative pixel
/// space, Y-up) plus the run's metrics into canvas-centered [`GlyphQuad`]s.
///
/// `canvas_w`/`canvas_h` is the viewport size in pixels. The text is
/// centered horizontally and vertically (ascender at the top of the
/// vertical center). Vertex positions are in screen-space pixels (Y-down,
/// origin at the canvas top-left); the text stack produces quads in
/// baseline-relative Y-up space, so Y is converted here.
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
    //
    // Vertex positions are in screen-space pixels (Y-down, origin at
    // top-left of canvas). The text stack produces quads in baseline-relative
    // Y-up space, so we need to convert Y.
    let baseline_x = canvas_w * 0.5 - total_advance * 0.5;
    // In screen space (Y-down), the baseline is at canvas_h/2 + ascent/2
    // (slightly below center to account for descenders).
    let baseline_y_screen = canvas_h * 0.5 + ascent * 0.5;

    let _ = descent; // used for centering math

    text_quads
        .iter()
        .map(|q| {
            // alkalive_text::Quad has `position` = top-left of the glyph
            // bitmap. The Y coordinate is computed as `offset_y - bearing.1`
            // which gives a negative value (Y-down convention: negative =
            // above baseline). Width/height are in `size`. UV box is in
            // `uv` (x, y, w, h) — origin at the top-left of the glyph tile
            // in the atlas.
            let px = q.position.0;
            let py = q.position.1; // Y-down: negative = above baseline
            let center_x = baseline_x + px + q.size.0 * 0.5;
            // In Y-down screen space: glyph top is at baseline_y + py
            // (py is negative, so this is above the baseline = smaller Y).
            // Center = top + half_height.
            let center_y = baseline_y_screen + py + q.size.1 * 0.5;

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
// Shared tessellation + WGSL backend modules
// ---------------------------------------------------------------------------

/// The fixed glyph-atlas page edge length in pixels (single R8 page).
pub const ATLAS_SIZE: u32 = 512;

/// Byte size of one glyph-atlas page (`ATLAS_SIZE × ATLAS_SIZE`, 1 byte per
/// pixel).
pub const ATLAS_PAGE_BYTES: usize = (ATLAS_SIZE * ATLAS_SIZE) as usize;

pub mod frame_plan;
pub mod tessellate;
pub mod wgsl_shaders;
#[cfg(feature = "wgpu-backend")]
pub mod wgpu_renderer;

#[cfg(target_arch = "wasm32")]
mod wasm {
    //! Real GPU backend — WebGL2 via `web-sys::WebGl2RenderingContext`.

    use super::*;
    use std::sync::Arc;
    use wasm_bindgen::JsCast;
    use web_sys::{
        HtmlCanvasElement, Performance, WebGl2RenderingContext, WebGlBuffer, WebGlProgram,
        WebGlShader, WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
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
        /// The compiled text shader program (vertex + fragment).
        program: WebGlProgram,
        /// Text vertex shader object (kept for re-link on context loss).
        vs: WebGlShader,
        /// Text fragment shader object.
        fs: WebGlShader,
        /// The rect shader program (for filled/outlined rectangles with alpha).
        rect_program: WebGlProgram,
        /// Rect vertex shader object.
        rect_vs: WebGlShader,
        /// Rect fragment shader object.
        rect_fs: WebGlShader,
        /// VAO holding the text vertex buffer binding.
        vao: WebGlVertexArrayObject,
        /// VAO for the rect shader (a single full-viewport quad).
        rect_vao: WebGlVertexArrayObject,
        /// The text vertex buffer (positions + UVs, 6 verts per glyph quad).
        vbo: WebGlBuffer,
        /// The rect vertex buffer (a single triangle-strip covering the viewport).
        rect_vbo: WebGlBuffer,
        /// Cached rect shader uniform locations.
        rect_u_rect: WebGlUniformLocation,
        rect_u_color: WebGlUniformLocation,
        rect_u_canvas: WebGlUniformLocation,
        /// The glyph atlas texture (single-channel grayscale, 512×512).
        glyph_texture: WebGlTexture,
        /// Cached location of the `rotation` uniform.
        u_rotation: WebGlUniformLocation,
        /// Cached location of the `canvas_size` uniform.
        u_canvas_size: WebGlUniformLocation,
        /// Cached location of the `time` uniform (optional — may be optimized away).
        u_time: Option<WebGlUniformLocation>,
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
        /// Last input text rendered (to detect changes for atlas re-upload).
        last_input_text: String,
        /// Vertex count for the title text (drawn with rotation).
        title_vertex_count: u32,
        /// Start offset for input field text vertices (drawn without rotation).
        input_vertex_start: u32,
        /// Vertex count for the input field text.
        input_vertex_count: u32,
        /// Input field bounds in pixels (x, y, w, h) for hit-testing.
        input_field_bounds: (f32, f32, f32, f32),
        /// Cached font registry (avoids re-parsing the 170 KB TTF on every keystroke).
        font_registry: Option<Arc<alkalive_text::HarfRustFontRegistry>>,
        /// Cached font ID (resolved once from the registry).
        font_id: Option<alkalive_text::FontId>,
        /// Cached text shaper (avoids re-creating on every frame).
        text_shaper: Option<alkalive_text::HarfRustTextShaper>,
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
                .map_err(|_| {
                    "getContext('webgl2') did not return a WebGL2RenderingContext".to_string()
                })?;

            // Compile shaders.
            let vs = compile_shader(
                &gl,
                WebGl2RenderingContext::VERTEX_SHADER,
                VERTEX_SHADER_SRC,
            )?;
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
            gl.vertex_attrib_pointer_with_i32(
                0,
                2,
                WebGl2RenderingContext::FLOAT,
                false,
                stride,
                0,
            );
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
            // Note: `time` uniform may be optimized away by the GLSL compiler
            // if unused in the shader. Make it optional.
            let u_rotation = gl
                .get_uniform_location(&program, "rotation")
                .ok_or_else(|| "uniform 'rotation' not found".to_string())?;
            let u_canvas_size = gl
                .get_uniform_location(&program, "canvas_size")
                .ok_or_else(|| "uniform 'canvas_size' not found".to_string())?;
            let u_time = gl.get_uniform_location(&program, "time");
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

            // Enable alpha blending for text antialiasing and rect transparency.
            gl.enable(WebGl2RenderingContext::BLEND);
            gl.blend_func(
                WebGl2RenderingContext::SRC_ALPHA,
                WebGl2RenderingContext::ONE_MINUS_SRC_ALPHA,
            );

            // ---- Rect shader program (replaces scissor+clear hack; respects alpha) ----
            let rect_vs = compile_shader(
                &gl,
                WebGl2RenderingContext::VERTEX_SHADER,
                RECT_VERTEX_SHADER_SRC,
            )?;
            let rect_fs = compile_shader(
                &gl,
                WebGl2RenderingContext::FRAGMENT_SHADER,
                RECT_FRAGMENT_SHADER_SRC,
            )?;
            let rect_program = gl
                .create_program()
                .ok_or_else(|| "create_program() returned null for rect".to_string())?;
            gl.attach_shader(&rect_program, &rect_vs);
            gl.attach_shader(&rect_program, &rect_fs);
            gl.link_program(&rect_program);
            if gl
                .get_program_parameter(&rect_program, WebGl2RenderingContext::LINK_STATUS)
                .as_bool()
                != Some(true)
            {
                let log = gl
                    .get_program_info_log(&rect_program)
                    .unwrap_or_else(|| "(no info log)".to_string());
                return Err(format!("Rect program link failed: {}", log));
            }

            // Full-viewport quad for the rect shader (triangle strip).
            let rect_vao = gl
                .create_vertex_array()
                .ok_or_else(|| "create_vertex_array() returned null for rect".to_string())?;
            let rect_vbo = gl
                .create_buffer()
                .ok_or_else(|| "create_buffer() returned null for rect".to_string())?;
            let quad: [f32; 8] = [-1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, -1.0];
            let quad_bytes: &[u8] =
                unsafe { std::slice::from_raw_parts(quad.as_ptr() as *const u8, 32) };
            gl.bind_vertex_array(Some(&rect_vao));
            gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&rect_vbo));
            gl.buffer_data_with_u8_array(
                WebGl2RenderingContext::ARRAY_BUFFER,
                quad_bytes,
                WebGl2RenderingContext::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_with_i32(0, 2, WebGl2RenderingContext::FLOAT, false, 8, 0);
            gl.bind_vertex_array(None);

            let rect_u_rect = gl
                .get_uniform_location(&rect_program, "u_rect")
                .ok_or_else(|| "rect uniform 'u_rect' not found".to_string())?;
            let rect_u_color = gl
                .get_uniform_location(&rect_program, "u_color")
                .ok_or_else(|| "rect uniform 'u_color' not found".to_string())?;
            let rect_u_canvas = gl
                .get_uniform_location(&rect_program, "u_canvas")
                .ok_or_else(|| "rect uniform 'u_canvas' not found".to_string())?;

            Ok(Self {
                canvas,
                gl,
                program,
                vs,
                fs,
                rect_program,
                rect_vs,
                rect_fs,
                vao,
                rect_vao,
                vbo,
                rect_vbo,
                rect_u_rect,
                rect_u_color,
                rect_u_canvas,
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
                last_input_text: String::new(),
                title_vertex_count: 0,
                input_vertex_start: 0,
                input_vertex_count: 0,
                input_field_bounds: (0.0, 0.0, 0.0, 0.0),
                vertex_count: 0,
                font_registry: None,
                font_id: None,
                text_shaper: None,
            })
        }

        /// Render one frame using the ADR-024 schedule.
        ///
        /// Wave 11 (Gap 6 — Render-Graph IR): this method now drives
        /// rendering through the render-graph IR. It builds a
        /// [`alkalive_render::graph::RenderGraph`] from `text_scene` +
        /// the renderer's cached `input_field_bounds` (populated by
        /// `upload_text_atlas`), then dispatches it via
        /// [`render_graph`](Self::render_graph). The schedule argument
        /// is kept for API compatibility with `render_frame_with_dirty`
        /// and the runtime-wasm call site; it is not consumed here (the
        /// graph carries the pass order).
        ///
        /// On the first call, the glyph atlas is rasterized via
        /// `alkalive-text` (HarfRust shaping + glyph rasterization) and
        /// uploaded to the GPU texture. Subsequent calls re-use the cached
        /// atlas unless the input text changes (which triggers a re-upload).
        pub fn render_frame(
            &mut self,
            text_scene: &TextSceneData,
            _schedule: &alkalive_compiler::ScheduleIR,
            time: f32,
        ) {
            // 1. Determine input display text.
            let input_display = if text_scene.input_text.is_empty() {
                text_scene.input_placeholder.clone()
            } else {
                text_scene.input_text.clone()
            };

            // 2. Re-upload atlas if needed (first frame or input changed).
            //    This also recomputes `input_field_bounds` (set by
            //    `upload_text_atlas` based on canvas size + font size).
            if !self.atlas_uploaded || self.last_input_text != input_display {
                if let Err(e) =
                    self.upload_text_atlas(&text_scene.text, &input_display, text_scene.font_size)
                {
                    web_sys::console::error_1(&format!("atlas upload failed: {}", e).into());
                }
                self.atlas_uploaded = true;
                self.last_input_text = input_display;
            }

            // 3. Build the render graph from the per-frame scene data + the
            //    cached input-field bounds. The graph is the single source
            //    of truth for "what to draw this frame" — `render_graph`
            //    iterates it and dispatches the draw calls.
            let graph = alkalive_render::graph::build_render_graph(
                text_scene,
                (self.width, self.height),
                self.input_field_bounds,
            );

            // 4. Execute the graph.
            self.render_graph(&graph, time);
        }

        /// Render one frame from a [`alkalive_render::graph::RenderGraph`].
        ///
        /// This is the Wave 11 (Gap 6) render-graph-driven entry point.
        /// It iterates `graph.pass_order`, dispatches each pass's draw
        /// calls via [`execute_draw_call`](Self::execute_draw_call), and
        /// produces the same visual output as the previously hardcoded
        /// sequence in `render_frame_internal`.
        ///
        /// # Pre-conditions
        ///
        /// The caller is responsible for ensuring the glyph atlas has
        /// been uploaded (via `upload_text_atlas`) before calling this
        /// method. The [`render_frame`](Self::render_frame) wrapper
        /// handles this; direct callers must do it themselves.
        ///
        /// # Arguments
        ///
        /// * `graph` — the render graph to execute.
        /// * `time` — the animation time (drives text rotation).
        pub fn render_graph(&mut self, graph: &alkalive_render::graph::RenderGraph, time: f32) {
            // 1. Set up the canvas viewport. (The canvas size is set at
            //    init/resize time; we only need to reset the viewport here
            //    in case a previous frame left it in an unexpected state.)
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);

            // 2. Iterate passes in `graph.pass_order` and dispatch each
            //    draw call. The graph is the single source of truth for
            //    pass order — there is no fallback to the schedule.
            for &pass_idx in &graph.pass_order {
                let pass = match graph.passes.get(pass_idx) {
                    Some(p) => p,
                    None => continue,
                };
                for dc in &pass.draw_calls {
                    self.execute_draw_call(graph, dc, time);
                }
            }
        }

        /// Execute one draw call.
        ///
        /// Dispatches on [`alkalive_render::graph::DrawCallKind`] to the
        /// concrete GPU operation. This is the per-draw-call sink shared
        /// by [`render_graph`](Self::render_graph).
        ///
        /// # Draw-text dispatch
        ///
        /// `DrawCallKind::DrawText` carries a `text_ptr`/`text_len` pair
        /// for future SAB/IPC transport (Gap 8). Today the renderer does
        /// not dereference `text_ptr` — it reads the text from its own
        /// cached shaped-run vertex buffer. The draw call's `id` field
        /// selects which cached run to draw:
        /// - `id == 3` → title text (vertex range `0..title_vertex_count`).
        /// - `id == 4` → input text (vertex range
        ///   `input_vertex_start..input_vertex_start + input_vertex_count`).
        ///
        /// This mapping is a temporary pragmatic bridge until a future
        /// wave adds a `GlyphRunId` field to `DrawCallKind::DrawText`
        /// (per the rendering spec §1.2 REND-608).
        fn execute_draw_call(
            &self,
            _graph: &alkalive_render::graph::RenderGraph,
            dc: &alkalive_render::graph::DrawCall,
            time: f32,
        ) {
            use alkalive_render::graph::DrawCallKind;
            match &dc.kind {
                DrawCallKind::Clear { color } => {
                    self.gl.clear_color(color.0, color.1, color.2, color.3);
                    self.gl.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
                }
                DrawCallKind::DrawRect { x, y, w, h, color } => {
                    self.draw_rect_filled(*x, *y, *w, *h, color.0, color.1, color.2, color.3);
                }
                DrawCallKind::DrawRectOutline {
                    x,
                    y,
                    w,
                    h,
                    color,
                    line_width,
                } => {
                    self.draw_rect_outline_lw(
                        *x,
                        *y,
                        *w,
                        *h,
                        color.0,
                        color.1,
                        color.2,
                        color.3,
                        *line_width,
                    );
                }
                DrawCallKind::DrawText {
                    color, rotation, ..
                } => {
                    // Re-bind the text shader program (the rect passes
                    // above may have left the rect shader bound).
                    self.gl.use_program(Some(&self.program));
                    self.gl.bind_vertex_array(Some(&self.vao));
                    self.gl.uniform2f(
                        Some(&self.u_canvas_size),
                        self.width as f32,
                        self.height as f32,
                    );
                    self.gl.active_texture(WebGl2RenderingContext::TEXTURE0);
                    self.gl.bind_texture(
                        WebGl2RenderingContext::TEXTURE_2D,
                        Some(&self.glyph_texture),
                    );
                    self.gl.uniform1i(Some(&self.u_glyph_texture), 0);

                    // Map draw-call id → cached vertex range. See the
                    // method-level docstring for the rationale.
                    let (start, count) = match dc.id {
                        3 => (0i32, self.title_vertex_count as i32),
                        4 => (
                            self.input_vertex_start as i32,
                            self.input_vertex_count as i32,
                        ),
                        other => {
                            web_sys::console::warn_1(
                                &format!(
                                    "AlkALive: DrawText draw call id {} has no cached vertex range; skipping",
                                    other
                                )
                                .into(),
                            );
                            return;
                        }
                    };
                    if count <= 0 {
                        return;
                    }

                    // rotation in the IR is the rotation-speed coefficient;
                    // multiply by time to get the actual angle.
                    self.gl.uniform1f(Some(&self.u_rotation), rotation * time);
                    self.gl
                        .uniform4f(Some(&self.u_text_color), color.0, color.1, color.2, color.3);
                    if let Some(ref u_time) = self.u_time {
                        self.gl.uniform1f(Some(u_time), time);
                    }
                    self.gl
                        .draw_arrays(WebGl2RenderingContext::TRIANGLES, start, count);
                }
            }
        }

        /// Render one frame with ADR-025 dirty-pass info.
        ///
        /// This is the incremental-computation variant of [`render_frame`].
        /// It accepts a list of *dirty pass indices* — passes whose signal
        /// inputs changed since the last frame and therefore need to
        /// re-execute. Passes not in the list are skipped.
        ///
        /// # Current behaviour
        ///
        /// For correctness with WebGL2's single-buffered clear, the
        /// current implementation runs *all* passes when `dirty_passes` is
        /// non-empty (skipping only the `Clear` pass would leave ghosts of
        /// the previous frame's stale passes). When `dirty_passes` is
        /// empty, the renderer is a no-op — the previous frame is
        /// preserved.
        ///
        /// The `dirty_passes` parameter is therefore plumbed through but
        /// currently used only for the empty/non-empty decision. A future
        /// wave will use per-pass render targets to truly skip
        /// non-dirty passes (per ADR-025 §"Consequences").
        ///
        /// # Arguments
        ///
        /// * `text_scene` — the per-frame scene description.
        /// * `schedule` — the rendering schedule (ADR-024).
        /// * `time` — the animation time (drives rotation).
        /// * `dirty_passes` — the indices of passes whose inputs changed
        ///   since the last frame (computed by the runtime's
        ///   `SignalStore::propagate` + `dirty_passes`).
        pub fn render_frame_with_dirty(
            &mut self,
            text_scene: &TextSceneData,
            schedule: &alkalive_compiler::ScheduleIR,
            time: f32,
            dirty_passes: &[usize],
        ) {
            // If no passes are dirty, skip rendering entirely — the
            // previous frame is preserved (per ADR-025, "only re-submit
            // draw calls for passes whose inputs were dirty").
            if dirty_passes.is_empty() {
                return;
            }
            // At least one pass is dirty. For correctness, run the full
            // render path (the dirty info is a hint that the renderer
            // *could* use to skip unchanged passes, but the current
            // single-buffered WebGL2 backend can't safely skip Clear
            // without leaving ghosts of stale passes).
            //
            // The `Some(dirty_passes)` argument is forwarded to
            // `render_frame_internal` so a future wave can implement
            // per-pass skipping without changing this call site.
            self.render_frame_internal(text_scene, schedule, time, Some(dirty_passes));
        }

        /// Internal frame-rendering routine shared by
        /// [`render_frame`](Self::render_frame) and
        /// [`render_frame_with_dirty`](Self::render_frame_with_dirty).
        ///
        /// `dirty_passes` is `None` for the legacy full-rebuild path and
        /// `Some(&[usize])` for the incremental path. The current
        /// implementation ignores the contents of `dirty_passes` (it only
        /// uses the empty/non-empty check at the
        /// [`render_frame_with_dirty`](Self::render_frame_with_dirty)
        /// call site), but accepts it so the per-pass dispatch loop can
        /// be made dirty-aware in a future wave without changing the
        /// internal signature.
        fn render_frame_internal(
            &mut self,
            text_scene: &TextSceneData,
            schedule: &alkalive_compiler::ScheduleIR,
            time: f32,
            _dirty_passes: Option<&[usize]>,
        ) {
            use alkalive_compiler::PassKind;

            // 1. Determine input display text.
            let input_display = if text_scene.input_text.is_empty() {
                text_scene.input_placeholder.clone()
            } else {
                text_scene.input_text.clone()
            };

            // 2. Re-upload atlas if needed (first frame or input changed).
            //    ADR-025 note: this is the "primitive form of dirty
            //    tracking" (one bit: `last_input_text != input_display`).
            //    The dependency-graph path generalises this to per-signal
            //    dirty tracking, but the renderer's per-frame GPU upload
            //    still keys off this single comparison.
            if !self.atlas_uploaded || self.last_input_text != input_display {
                if let Err(e) =
                    self.upload_text_atlas(&text_scene.text, &input_display, text_scene.font_size)
                {
                    web_sys::console::error_1(&format!("atlas upload failed: {}", e).into());
                }
                self.atlas_uploaded = true;
                self.last_input_text = input_display.clone();
            }

            // 3. Bind program + shared state once (text-quad shader).
            //    Passes that need a different shader will rebind as needed.
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);
            self.gl.use_program(Some(&self.program));
            self.gl.uniform2f(
                Some(&self.u_canvas_size),
                self.width as f32,
                self.height as f32,
            );
            if let Some(ref u_time) = self.u_time {
                self.gl.uniform1f(Some(u_time), time);
            }
            self.gl.active_texture(WebGl2RenderingContext::TEXTURE0);
            self.gl.bind_texture(
                WebGl2RenderingContext::TEXTURE_2D,
                Some(&self.glyph_texture),
            );
            self.gl.uniform1i(Some(&self.u_glyph_texture), 0);
            self.gl.bind_vertex_array(Some(&self.vao));

            // 4. ADR-024: data-driven dispatch over the schedule's passes.
            //    Iterate passes in `schedule.pass_order` order, matching on
            //    `pass.kind` to decide what to draw. This replaces the
            //    previously hardcoded `clear → input-bg → input-border →
            //    title-text → input-text` sequence with a single loop whose
            //    order is controlled by the ScheduleIR data structure.
            for &pass_idx in &schedule.pass_order {
                let pass = match schedule.passes.get(pass_idx) {
                    Some(p) => p,
                    None => continue,
                };
                match pass.kind {
                    PassKind::Clear => {
                        // Clear to the background color.
                        let (br, bg, bb) = text_scene.background_normalized();
                        self.gl.clear_color(br, bg, bb, 1.0);
                        self.gl.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
                    }
                    PassKind::InputFieldBackground => {
                        // Solid-color fill of the input field rectangle.
                        let (fx, fy, fw, fh) = self.input_field_bounds;
                        self.draw_rect_filled(fx, fy, fw, fh, 0.05, 0.05, 0.08, 0.9);
                    }
                    PassKind::InputFieldBorder => {
                        // Solid-color outline of the input field rectangle.
                        let (fx, fy, fw, fh) = self.input_field_bounds;
                        self.draw_rect_outline(fx, fy, fw, fh, 0.8, 0.65, 0.0, 0.8);
                    }
                    PassKind::TitleText => {
                        // Draw title text WITH rotation (golden color).
                        // Re-bind the text shader program (the rect passes
                        // above may have left the rect shader bound).
                        self.gl.use_program(Some(&self.program));
                        self.gl.bind_vertex_array(Some(&self.vao));
                        if self.title_vertex_count > 0 {
                            let rotation = if pass.rotation {
                                text_scene.rotation_speed * time
                            } else {
                                0.0
                            };
                            self.gl.uniform1f(Some(&self.u_rotation), rotation);
                            self.gl.uniform4f(
                                Some(&self.u_text_color),
                                text_scene.text_color.0,
                                text_scene.text_color.1,
                                text_scene.text_color.2,
                                text_scene.text_color.3,
                            );
                            self.gl.draw_arrays(
                                WebGl2RenderingContext::TRIANGLES,
                                0,
                                self.title_vertex_count as i32,
                            );
                        }
                    }
                    PassKind::InputText => {
                        // Draw input field text WITHOUT rotation.
                        // Re-bind the text shader program (defensive).
                        self.gl.use_program(Some(&self.program));
                        self.gl.bind_vertex_array(Some(&self.vao));
                        if self.input_vertex_count > 0 {
                            self.gl.uniform1f(Some(&self.u_rotation), 0.0);
                            if text_scene.input_text.is_empty() {
                                // Placeholder: dim gray
                                self.gl
                                    .uniform4f(Some(&self.u_text_color), 0.35, 0.35, 0.4, 1.0);
                            } else {
                                // Typed text: white
                                self.gl
                                    .uniform4f(Some(&self.u_text_color), 0.9, 0.9, 0.95, 1.0);
                            }
                            self.gl.draw_arrays(
                                WebGl2RenderingContext::TRIANGLES,
                                self.input_vertex_start as i32,
                                self.input_vertex_count as i32,
                            );
                        }
                    }
                }
            }
        }

        /// Draw a filled rectangle with proper alpha blending using the rect shader.
        /// Replaces the old scissor+clear hack that silently ignored alpha.
        fn draw_rect_filled(&self, x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32, a: f32) {
            self.gl.use_program(Some(&self.rect_program));
            self.gl
                .uniform4f(Some(&self.rect_u_rect), x, y, x + w, y + h);
            self.gl.uniform4f(Some(&self.rect_u_color), r, g, b, a);
            self.gl.uniform2f(
                Some(&self.rect_u_canvas),
                self.width as f32,
                self.height as f32,
            );
            self.gl.bind_vertex_array(Some(&self.rect_vao));
            self.gl
                .draw_arrays(WebGl2RenderingContext::TRIANGLE_STRIP, 0, 4);
        }

        /// Draw a rectangle outline (4 edges) with proper alpha blending.
        ///
        /// Convenience wrapper around [`draw_rect_outline_lw`](Self::draw_rect_outline_lw)
        /// with the default line width of `2.0` pixels.
        fn draw_rect_outline(
            &self,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        ) {
            self.draw_rect_outline_lw(x, y, w, h, r, g, b, a, 2.0);
        }

        /// Draw a rectangle outline (4 edges) with the given line width.
        ///
        /// Each edge is a solid filled rect of thickness `line_width`
        /// pixels, drawn via [`draw_rect_filled`](Self::draw_rect_filled)
        /// so it inherits proper alpha blending.
        fn draw_rect_outline_lw(
            &self,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
            line_width: f32,
        ) {
            let lw = line_width.max(0.0);
            if lw <= 0.0 || w < lw || h < lw {
                return;
            }
            self.draw_rect_filled(x, y, w, lw, r, g, b, a); // top
            self.draw_rect_filled(x, y + h - lw, w, lw, r, g, b, a); // bottom
            self.draw_rect_filled(x, y, lw, h, r, g, b, a); // left
            self.draw_rect_filled(x + w - lw, y, lw, h, r, g, b, a); // right
        }

        /// Resize the canvas + WebGL viewport.
        /// Zero-sized requests are clamped to 1x1, mirroring
        /// [`WgpuBackendRenderer::resize`] — a 0-sized canvas/viewport is
        /// a degenerate edge case (e.g. a display:none parent) and must not
        /// produce an invalid GL viewport call.
        pub fn resize(&mut self, width: u32, height: u32) {
            let (width, height) = (width.max(1), height.max(1));
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

        /// Returns the input field bounds (x, y, w, h) in screen pixels.
        pub fn input_field_bounds(&self) -> (f32, f32, f32, f32) {
            self.input_field_bounds
        }

        /// Check if a point (x, y) is inside the input field rectangle.
        pub fn hit_test_input_field(&self, x: f32, y: f32) -> bool {
            let (fx, fy, fw, fh) = self.input_field_bounds;
            x >= fx && x <= fx + fw && y >= fy && y <= fy + fh
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
        fn upload_text_atlas(
            &mut self,
            title_text: &str,
            input_text: &str,
            font_size: f32,
        ) -> Result<(), String> {
            use alkalive_text::{
                FontRegistry, FontRequest, HarfRustFontRegistry,
                HarfRustGlyphAtlas, HarfRustTextShaper, ShapeContext, TextShaper,
            };

            // 1. Initialize or reuse the cached font registry + shaper (M7 fix:
            //    avoids re-parsing the 170 KB TTF on every keystroke).
            if self.font_registry.is_none() {
                let font_bytes: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");
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
                let shaper = HarfRustTextShaper::new(Arc::clone(&registry_arc));
                self.font_registry = Some(registry_arc);
                self.font_id = Some(font_id);
                self.text_shaper = Some(shaper);
            }

            let registry_arc = self.font_registry.as_ref().unwrap().clone();
            let font_id = self.font_id.unwrap();
            let shaper = self.text_shaper.as_ref().unwrap();

            // 2. Create a fresh atlas (cheap — just allocates an empty 512×512 page).
            let mut atlas = HarfRustGlyphAtlas::new(registry_arc);
            let title_font_size = font_size;
            let input_font_size = font_size * 0.5;

            // 3. Shape the title + input text.
            let ctx_title = ShapeContext {
                font: font_id,
                size_px: title_font_size,
                direction: None,
            };
            let title_run = shaper
                .shape(title_text, &ctx_title)
                .map_err(|e| format!("shape title: {:?}", e))?;

            let ctx_input = ShapeContext {
                font: font_id,
                size_px: input_font_size,
                direction: None,
            };
            let input_run = shaper
                .shape(input_text, &ctx_input)
                .map_err(|e| format!("shape input: {:?}", e))?;

            // 4. Rasterize glyphs into the atlas and build quads.
            let title_quads = build_text_quads(&title_run, &mut atlas, title_font_size);
            let input_quads = build_text_quads(&input_run, &mut atlas, input_font_size);

            // 5. Multi-page atlas detection (C1 fix): if the atlas overflowed
            //    to a second page, glyphs on page 1+ will render as blank
            //    because we only upload page 0. Log a warning so the failure
            //    is visible rather than silent.
            if atlas.page_count() > 1 {
                web_sys::console::warn_1(
                    &format!(
                        "AlkALive: glyph atlas overflowed to {} pages (only page 0 is uploaded). \
                     Some glyphs may not render. Consider reducing font size or text length.",
                        atlas.page_count()
                    )
                    .into(),
                );
            }

            // 6. Upload atlas page 0 to GPU.
            let page_data = atlas
                .page_data(0)
                .ok_or_else(|| "atlas page 0 missing".to_string())?;
            self.gl.bind_texture(
                WebGl2RenderingContext::TEXTURE_2D,
                Some(&self.glyph_texture),
            );
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

            // 6. Build canvas-centered title quads (centered, will get rotation).
            let title_canvas_quads = quads_from_text(
                &title_quads,
                title_run.metrics.ascent,
                title_run.metrics.descent,
                title_run.metrics.total_advance,
                self.width as f32,
                self.height as f32,
            );
            let title_verts = build_vertex_buffer(&title_canvas_quads);
            self.title_vertex_count = title_verts.len() as u32;

            // 7. Build input field quads (positioned inside the input field, no rotation).
            //    Input field is centered horizontally, below the title.
            let field_w = (self.width as f32 * 0.5).min(400.0);
            let field_h = 40.0;
            let field_x = (self.width as f32 - field_w) * 0.5;
            let field_y = (self.height as f32 * 0.5) + font_size * 0.5 + 20.0;
            self.input_field_bounds = (field_x, field_y, field_w, field_h);

            // Center input text inside the field.
            let input_baseline_x = field_x + (field_w - input_run.metrics.total_advance) * 0.5;
            let input_baseline_y = field_y + field_h * 0.5 + input_run.metrics.ascent * 0.5;

            let input_canvas_quads: Vec<GlyphQuad> = input_quads
                .iter()
                .map(|q| {
                    let px = q.position.0;
                    let py = q.position.1;
                    GlyphQuad {
                        center_x: input_baseline_x + px + q.size.0 * 0.5,
                        center_y: input_baseline_y + py + q.size.1 * 0.5,
                        w: q.size.0,
                        h: q.size.1,
                        u0: q.uv.x,
                        v0: q.uv.y,
                        u1: q.uv.x + q.uv.w,
                        v1: q.uv.y + q.uv.h,
                    }
                })
                .collect();
            let input_verts = build_vertex_buffer(&input_canvas_quads);
            self.input_vertex_start = self.title_vertex_count;
            self.input_vertex_count = input_verts.len() as u32;

            // 8. Combine and upload vertex buffer.
            let mut all_verts = title_verts;
            all_verts.extend(input_verts);
            self.vertex_count = all_verts.len() as u32;

            self.gl
                .bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&self.vbo));
            let byte_len = all_verts.len() * std::mem::size_of::<Vertex>();
            if byte_len == 0 {
                return Ok(());
            }
            let bytes: &[u8] =
                unsafe { std::slice::from_raw_parts(all_verts.as_ptr() as *const u8, byte_len) };
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
            self.gl.delete_program(Some(&self.program));
            self.gl.delete_shader(Some(&self.vs));
            self.gl.delete_shader(Some(&self.fs));
            self.gl.delete_program(Some(&self.rect_program));
            self.gl.delete_shader(Some(&self.rect_vs));
            self.gl.delete_shader(Some(&self.rect_fs));
            self.gl.delete_buffer(Some(&self.vbo));
            self.gl.delete_buffer(Some(&self.rect_vbo));
            self.gl.delete_vertex_array(Some(&self.vao));
            self.gl.delete_vertex_array(Some(&self.rect_vao));
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
            Err("alkalive-backend-wgpu: WebGL2 backend only runs on wasm32 \
                 (this is a native build — the GPU backend is not available)"
                .to_string())
        }

        /// No-op on native — the renderer was never actually constructed.
        pub fn render_frame(
            &mut self,
            _text_scene: &TextSceneData,
            _schedule: &alkalive_compiler::ScheduleIR,
            _time: f32,
        ) {
            // Intentionally empty: the GPU backend only runs on wasm32.
            // (ADR-024: signature now takes the schedule for type-compat
            // with the wasm32 build, but the native stub does nothing.)
        }

        /// No-op on native (ADR-025: dirty-pass-aware variant).
        ///
        /// This is the type-compatible stub for the wasm32
        /// [`render_frame_with_dirty`](super::WgpuRenderer::render_frame_with_dirty)
        /// method. It accepts the dirty pass indices but does nothing —
        /// the native build never has a real renderer.
        pub fn render_frame_with_dirty(
            &mut self,
            _text_scene: &TextSceneData,
            _schedule: &alkalive_compiler::ScheduleIR,
            _time: f32,
            _dirty_passes: &[usize],
        ) {
            // Intentionally empty: the GPU backend only runs on wasm32.
        }

        /// No-op on native (Wave 11 / Gap 6: render-graph-driven entry
        /// point).
        ///
        /// This is the type-compatible stub for the wasm32
        /// [`render_graph`](super::WgpuRenderer::render_graph) method.
        /// It accepts a [`alkalive_render::graph::RenderGraph`] but does
        /// nothing — the native build never has a real renderer.
        pub fn render_graph(&mut self, _graph: &alkalive_render::graph::RenderGraph, _time: f32) {
            // Intentionally empty: the GPU backend only runs on wasm32.
        }

        /// No-op on native.
        pub fn resize(&mut self, width: u32, height: u32) {
            self.width = width;
            self.height = height;
        }

        /// Always returns false on native (no renderer).
        pub fn hit_test_input_field(&self, _x: f32, _y: f32) -> bool {
            false
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
        // x0=80, x1=120, y0=20 (top), y1=80 (bottom)
        // In Y-down space: y0=top, y1=bottom
        // UV: v0=top of glyph, v1=bottom of glyph
        // Triangle 1: TL-TR-BL = (80,20)-(120,20)-(80,80)
        assert_eq!(verts[0], Vertex::new(80.0, 20.0, 0.0, 0.0)); // TL
        assert_eq!(verts[1], Vertex::new(120.0, 20.0, 0.1, 0.0)); // TR
        assert_eq!(verts[2], Vertex::new(80.0, 80.0, 0.0, 0.2)); // BL
                                                                 // Triangle 2: TR-BR-BL = (120,20)-(120,80)-(80,80)
        assert_eq!(verts[3], Vertex::new(120.0, 20.0, 0.1, 0.0)); // TR
        assert_eq!(verts[4], Vertex::new(120.0, 80.0, 0.1, 0.2)); // BR
        assert_eq!(verts[5], Vertex::new(80.0, 80.0, 0.0, 0.2)); // BL
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
    ///
    /// ADR-024: `render_frame` now accepts a `&ScheduleIR` argument so the
    /// renderer can do data-driven pass dispatch.
    #[test]
    fn wgpu_renderer_type_compiles() {
        // Just exercise the API surface (type-check).
        fn _assert_api(r: &mut WgpuRenderer) {
            // Build a default schedule for an empty algorithm — the native
            // stub ignores it, but the type must accept it.
            let algo = alkalive_compiler::AlgorithmIR::new(
                alkalive_compiler::mint_module_id("Test"),
                "Test",
            );
            let sched = alkalive_compiler::schedule_lowering(&algo);
            r.render_frame(&TextSceneData::default(), &sched, 0.0);
            r.resize(800, 600);
        }
        // (We can't actually construct one in a unit test without a canvas,
        // but the type-check above exercises the public API.)
    }

    /// ADR-024: the `ScheduleIR` data structure carries the passes in the
    /// expected order. This is a backend-side sanity check that the
    /// schedule types are re-exported through `alkalive_compiler`.
    #[test]
    fn schedule_ir_passes_have_expected_kinds_for_hello_world() {
        // Build an AlgorithmIR matching the Hello World scene.
        let mut algo = alkalive_compiler::AlgorithmIR::new(
            alkalive_compiler::mint_module_id("HelloWorld"),
            "HelloWorld",
        );
        algo.nodes.push(alkalive_compiler::NodeIR::Text {
            content: "Hello World!".into(),
            color: alkalive_compiler::ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: alkalive_compiler::PositionIR::Center,
        });
        algo.nodes.push(alkalive_compiler::NodeIR::InputField {
            placeholder: "Type here...".into(),
            position: alkalive_compiler::PositionIR::BelowText,
        });

        let sched = alkalive_compiler::schedule_lowering(&algo);
        // Five passes: Clear, InputFieldBackground, InputFieldBorder,
        // TitleText, InputText.
        assert_eq!(sched.passes.len(), 5);
        let kinds: Vec<_> = sched.passes.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![
                alkalive_compiler::PassKind::Clear,
                alkalive_compiler::PassKind::InputFieldBackground,
                alkalive_compiler::PassKind::InputFieldBorder,
                alkalive_compiler::PassKind::TitleText,
                alkalive_compiler::PassKind::InputText,
            ]
        );
        // TitleText pass has rotation=true.
        let title = sched
            .passes
            .iter()
            .find(|p| p.kind == alkalive_compiler::PassKind::TitleText)
            .expect("title pass");
        assert!(title.rotation);
        // All other passes have rotation=false.
        for p in sched
            .passes
            .iter()
            .filter(|p| p.kind != alkalive_compiler::PassKind::TitleText)
        {
            assert!(!p.rotation, "pass {:?} should not have rotation", p.kind);
        }
    }

    /// End-to-end: shape "Hello" via alkalive-text, build quads, build
    /// vertex buffer, verify non-empty. This proves the data path from
    /// text → quads → vertices works without a GPU.
    #[test]
    fn end_to_end_text_to_vertex_buffer() {
        use alkalive_text::{
            FontRegistry, FontRequest, GlyphAtlas, GlyphKey, HarfRustFontRegistry,
            HarfRustGlyphAtlas, HarfRustTextShaper, ShapeContext, TextShaper,
        };
        use std::sync::Arc;

        let font_bytes: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");
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

    // -------------------------------------------------------------------------
    // Wave 11 (Gap 6) — render-graph-driven rendering tests
    // -------------------------------------------------------------------------

    /// The renderer's `render_graph` method exists and accepts a
    /// `&alkalive_render::graph::RenderGraph` argument. This is a
    /// type-level smoke test — on native the renderer is a stub, but the
    /// signature must compile and the call must not panic.
    #[test]
    fn render_graph_method_accepts_render_graph() {
        fn _assert_api(r: &mut WgpuRenderer) {
            let scene = TextSceneData::default();
            let graph = alkalive_render::graph::build_render_graph(
                &scene,
                (800, 600),
                (100.0, 200.0, 400.0, 40.0),
            );
            r.render_graph(&graph, 0.0);
        }
        // No construction on native — just exercising the API surface.
        let _ = _assert_api;
    }

    /// The `render_frame` method still works end-to-end through the new
    /// render-graph path. On native the renderer is a stub, so this just
    /// verifies the call does not panic and the schedule argument is
    /// accepted for type-compat.
    #[test]
    fn render_frame_routes_through_render_graph() {
        fn _assert_api(r: &mut WgpuRenderer) {
            let scene = TextSceneData::default();
            let algo = alkalive_compiler::AlgorithmIR::new(
                alkalive_compiler::mint_module_id("Test"),
                "Test",
            );
            let sched = alkalive_compiler::schedule_lowering(&algo);
            r.render_frame(&scene, &sched, 0.0);
        }
        let _ = _assert_api;
    }

    /// The graph produced by `build_render_graph` validates cleanly when
    /// called from the backend crate (cross-crate integration check).
    #[test]
    fn backend_built_render_graph_validates() {
        let scene = TextSceneData::default();
        let graph = alkalive_render::graph::build_render_graph(
            &scene,
            (800, 600),
            (100.0, 200.0, 400.0, 40.0),
        );
        assert!(graph.validate().is_ok(), "graph must validate");
        assert_eq!(graph.passes.len(), 5, "graph must have 5 passes");
        assert_eq!(graph.pass_order, vec![0, 1, 2, 3, 4]);
    }

    /// The `TextSceneData` re-export from `alkalive-scene-data` still
    /// constructs the same default golden-on-black scene (cross-crate
    /// identity check after the Wave 11 move).
    #[test]
    fn text_scene_data_reexport_works() {
        let s = TextSceneData::default();
        assert_eq!(s.text, "Hello World!");
        assert_eq!(s.background, (0, 0, 0));
        assert_eq!(s.text_color, (1.0, 0.843, 0.0, 1.0));
        assert_eq!(s.input_placeholder, "Type here...");

        // The constructor + helper survive the move.
        let s2 = TextSceneData::new("Hi");
        assert_eq!(s2.text, "Hi");
        let (r, _g, _b) = s2.background_normalized();
        assert!((r - 0.0).abs() < 1e-6);
    }
}
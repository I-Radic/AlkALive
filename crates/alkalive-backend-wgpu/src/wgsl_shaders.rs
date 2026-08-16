//! WGSL shader sources for the AlkALive GPU backend (Gap 7 — ADR-006).
//!
//! These WGSL shaders are the WebGPU-native replacement for the existing
//! GLSL ES 3.00 shaders. They provide the same rendering functionality:
//! - Text quad rendering with Y-axis rotation and glyph atlas sampling
//! - Rectangle rendering with alpha blending
//!
//! When the `wgpu` backend is activated, these shaders are compiled via
//! `wgpu::Device::create_shader_module(wgpu::ShaderSource::Wgsl(...))`.
//! The existing GLSL shaders remain as the WebGL2 fallback.

/// WGSL vertex shader for text quad rendering.
/// Equivalent to VERTEX_SHADER_SRC (GLSL ES 3.00).
pub const TEXT_VERTEX_WGSL: &str = r#"
@group(0) @binding(0) var<uniform> uniforms: TextUniforms;

struct TextUniforms {
    rotation: f32,
    canvas_size: vec2<f32>,
    time: f32,
};

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let cos_r = cos(uniforms.rotation);
    let center_x = uniforms.canvas_size.x * 0.5;
    let rel_x = input.position.x - center_x;
    let scaled_x = rel_x * cos_r + center_x;
    let clip_x = scaled_x / (uniforms.canvas_size.x * 0.5) - 1.0;
    let clip_y = 1.0 - input.position.y / (uniforms.canvas_size.y * 0.5);
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.v_uv = input.uv;
    return output;
}
"#;

/// WGSL fragment shader for text quad rendering.
/// Equivalent to FRAGMENT_SHADER_SRC (GLSL ES 3.00).
pub const TEXT_FRAGMENT_WGSL: &str = r#"
@group(0) @binding(1) var glyph_texture: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;
@group(0) @binding(3) var<uniform> text_color: vec4<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
};

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_texture, glyph_sampler, input.v_uv).r;
    if (alpha < 0.01) {
        discard;
    }
    return vec4<f32>(text_color.rgb * alpha, alpha);
}
"#;

/// WGSL vertex shader for rectangle rendering.
/// Equivalent to RECT_VERTEX_SHADER_SRC (GLSL ES 3.00).
pub const RECT_VERTEX_WGSL: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> @builtin(position) vec4<f32> {
    return vec4<f32>(input.position, 0.0, 1.0);
}
"#;

/// WGSL fragment shader for rectangle rendering.
/// Equivalent to RECT_FRAGMENT_SHADER_SRC (GLSL ES 3.00).
pub const RECT_FRAGMENT_WGSL: &str = r#"
@group(0) @binding(0) var<uniform> u_rect: vec4<f32>;
@group(0) @binding(1) var<uniform> u_color: vec4<f32>;
@group(0) @binding(2) var<uniform> u_canvas: vec2<f32>;

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag_coord.x;
    let py = u_canvas.y - frag_coord.y;
    if (px < u_rect.x || px > u_rect.z || py < u_rect.y || py > u_rect.w) {
        discard;
    }
    return u_color;
}
"#;

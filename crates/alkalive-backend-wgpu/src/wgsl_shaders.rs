//! WGSL shader sources for the AlkALive GPU backend (ADR-006).
//!
//! These WGSL shaders are the WebGPU-native rendering programs used by
//! [`crate::wgpu_renderer::WgpuBackendRenderer`]. They provide the same
//! rendering functionality as the GLSL ES 3.00 fallback shaders in
//! [`crate::VERTEX_SHADER_SRC`] / [`crate::FRAGMENT_SHADER_SRC`]:
//!
//! - Text quad rendering with Y-axis rotation and glyph-atlas sampling
//! - Filled-rectangle and rectangle-outline rendering
//!
//! # Binding model (single source of truth)
//!
//! Both pipelines use **explicit** bind group layouts created by the
//! renderer, so the bindings below must match those layouts exactly:
//!
//! | Pipeline | Group 0 binding 0                          |
//! |----------|--------------------------------------------|
//! | Text     | `TextUniformsData` uniform buffer          |
//! |          | + binding 1: glyph atlas `texture_2d<f32>` |
//! |          | + binding 2: glyph `sampler`               |
//! | Rect     | `RectUniformsData` uniform buffer          |
//!
//! Per-draw-call data is delivered via **dynamic offsets** into one ring
//! buffer per pipeline a device-aligned slot stride of at least 256 bytes, so a
//! single bind group serves every draw call of a frame.
//!
//! # Uniform layout parity
//!
//! The WGSL structs below are laid out per the WGSL alignment rules; the
//! Rust mirrors ([`crate::frame_plan::TextUniformsData`] /
//! [`crate::frame_plan::RectUniformsData`]) must produce byte-identical
//! layouts. This parity is enforced by unit tests
//! (`uniform_layout_parity_*`) that assert the Rust offsets against the
//! constants documented here.
//!
//! `TextUniformsData` (WGSL offsets):
//! - `rotation: f32`         → 0
//! - `_pad0: f32`            → 4   *(Rust-only explicit padding)*
//! - `canvas_size: vec2f`    → 8   (vec2 alignment = 8)
//! - `time: f32`             → 16
//! - `_pad1: vec3f`          → 20  *(Rust-only explicit padding to 32)*
//! - `text_color: vec4f`     → 32  (vec4 alignment = 16)
//! - total size              → 48  (multiple of 16 — valid uniform stride)
//!
//! `RectUniformsData` (WGSL offsets):
//! - `rect: vec4f`           → 0   (x, y, w, h in pixels)
//! - `color: vec4f`          → 16
//! - `canvas_size: vec2f`    → 32
//! - `line_width: f32`       → 40  (0.0 = filled rect; > 0 = outline)
//! - `_pad1: f32`            → 44  *(Rust-only explicit padding)*
//! - total size              → 48

/// WGSL vertex shader for text quad rendering.
///
/// Vertex positions are canvas pixel coordinates (Y-down). The Y-axis
/// rotation scales X about the canvas center — the same transform as the
/// GLSL fallback vertex shader. Pixel space is converted to clip space
/// with the same flip as the GLSL path.
pub const TEXT_VERTEX_WGSL: &str = r#"
struct TextUniforms {
    rotation: f32,
    canvas_size: vec2<f32>,
    time: f32,
    text_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: TextUniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>
) -> VertexOutput {
    var output: VertexOutput;
    let cos_r = cos(uniforms.rotation);
    let half_w = uniforms.canvas_size.x * 0.5;
    let half_h = uniforms.canvas_size.y * 0.5;
    let center_x = half_w;
    let rel_x = position.x - center_x;
    let scaled_x = rel_x * cos_r + center_x;
    let clip_x = scaled_x / half_w - 1.0;
    let clip_y = 1.0 - position.y / half_h;
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.v_uv = uv;
    return output;
}
"#;

/// WGSL fragment shader for text quad rendering.
///
/// Samples the single-channel glyph atlas and outputs premultiplied-alpha
/// text colored by the per-draw `text_color` uniform — matching the GLSL
/// fallback fragment shader.
pub const TEXT_FRAGMENT_WGSL: &str = r#"
struct TextUniforms {
    rotation: f32,
    canvas_size: vec2<f32>,
    time: f32,
    text_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: TextUniforms;
@group(0) @binding(1) var glyph_texture: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;

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
    return vec4<f32>(uniforms.text_color.rgb * alpha, alpha);
}
"#;

/// WGSL vertex shader for rectangle rendering.
///
/// Consumes a unit-corner quad (corner components in {0, 1}) from the
/// shared rect vertex buffer and maps it through the per-draw
/// `RectUniforms.rect` (x, y, w, h in pixels) into canvas pixel space,
/// then to clip space with the same Y-flip as the text path.
pub const RECT_VERTEX_WGSL: &str = r#"
struct RectUniforms {
    rect: vec4<f32>,
    color: vec4<f32>,
    canvas_size: vec2<f32>,
    line_width: f32,
};

@group(0) @binding(0) var<uniform> u: RectUniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@location(0) corner: vec2<f32>) -> VertexOutput {
    let px = u.rect.x + corner.x * u.rect.z;
    let py = u.rect.y + corner.y * u.rect.w;
    let clip_x = px / (u.canvas_size.x * 0.5) - 1.0;
    let clip_y = 1.0 - py / (u.canvas_size.y * 0.5);
    return VertexOutput(vec4<f32>(clip_x, clip_y, 0.0, 1.0));
}
"#;

/// WGSL fragment shader for rectangle rendering.
///
/// `line_width == 0.0` renders a filled rectangle; `line_width > 0.0`
/// discards the interior and renders only the border ring (matching the
/// scissor-based border drawing of the GLSL fallback).
pub const RECT_FRAGMENT_WGSL: &str = r#"
struct RectUniforms {
    rect: vec4<f32>,
    color: vec4<f32>,
    canvas_size: vec2<f32>,
    line_width: f32,
};

@group(0) @binding(0) var<uniform> u: RectUniforms;

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    // Framebuffer coords are Y-down pixel coords — the same space as
    // RectUniforms.rect.
    let x = frag_coord.x;
    let y = frag_coord.y;
    let rx = u.rect.x;
    let ry = u.rect.y;
    let rw = u.rect.z;
    let rh = u.rect.w;

    let inside = x >= rx && x <= rx + rw && y >= ry && y <= ry + rh;
    if (!inside) {
        discard;
    }

    if (u.line_width > 0.0) {
        let lw = u.line_width;
        let on_border = x < rx + lw || x > rx + rw - lw || y < ry + lw || y > ry + rh - lw;
        if (!on_border) {
            discard;
        }
    }

    return u.color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every WGSL module must parse under naga's WGSL front-end. This is the
    /// static half of shader verification (the dynamic half is browser E2E):
    /// any syntax or type error in these sources fails this test without
    /// needing a GPU device.
    #[test]
    fn wgsl_modules_parse_with_naga() {
        for (name, src) in [
            ("text_vertex", TEXT_VERTEX_WGSL),
            ("text_fragment", TEXT_FRAGMENT_WGSL),
            ("rect_vertex", RECT_VERTEX_WGSL),
            ("rect_fragment", RECT_FRAGMENT_WGSL),
        ] {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("WGSL module `{}` failed to parse: {:?}", name, e));
            assert!(
                !module.types.is_empty(),
                "WGSL module `{}` parsed but declared no types",
                name
            );
        }
    }

    /// Full naga validation of each entry point. Catches binding-type
    /// mismatches, invalid type usage, and missing entry points before any
    /// GPU is involved.
    #[test]
    fn wgsl_entry_points_validate_with_naga() {
        let cases: [(&str, &str, &str); 4] = [
            ("text_vertex", TEXT_VERTEX_WGSL, "vs_main"),
            ("text_fragment", TEXT_FRAGMENT_WGSL, "fs_main"),
            ("rect_vertex", RECT_VERTEX_WGSL, "vs_main"),
            ("rect_fragment", RECT_FRAGMENT_WGSL, "fs_main"),
        ];
        for (name, src, entry) in cases {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("WGSL module `{}` failed to parse: {:?}", name, e));
            assert!(
                module.entry_points.iter().any(|ep| ep.name == entry),
                "WGSL module `{}` does not declare entry point `{}`",
                name,
                entry
            );
            let mut validator = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            );
            validator
                .validate(&module)
                .unwrap_or_else(|e| panic!("WGSL module `{}` failed validation: {:?}", name, e));
        }
    }

    /// Documented WGSL offset contract (see module docs): the struct field
    /// order must be stable because the Rust-side mirrors are tested
    /// byte-for-byte against these offsets.
    #[test]
    fn wgsl_uniform_struct_layout_contract() {
        // Parse the text vertex shader and inspect the TextUniforms struct
        // layout produced by naga.
        let module = naga::front::wgsl::parse_str(TEXT_VERTEX_WGSL).expect("text vertex parses");
        let ty_handle = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("TextUniforms"))
            .map(|(h, _)| h)
            .expect("TextUniforms struct exists");
        let ty = &module.types[ty_handle];
        let naga::TypeInner::Struct { members, .. } = &ty.inner else {
            panic!("TextUniforms is a struct");
        };
        let offsets: Vec<u32> = members.iter().map(|m| m.offset).collect();
        assert_eq!(
            offsets,
            vec![0, 8, 16, 32],
            "TextUniforms WGSL offsets changed — update TextUniformsData and its parity test"
        );

        let rmodule = naga::front::wgsl::parse_str(RECT_VERTEX_WGSL).expect("rect vertex parses");
        let rty_handle = rmodule
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("RectUniforms"))
            .map(|(h, _)| h)
            .expect("RectUniforms struct exists");
        let rty = &rmodule.types[rty_handle];
        let naga::TypeInner::Struct { members, .. } = &rty.inner else {
            panic!("RectUniforms is a struct");
        };
        let roffsets: Vec<u32> = members.iter().map(|m| m.offset).collect();
        assert_eq!(
            roffsets,
            vec![0, 16, 32, 40],
            "RectUniforms WGSL offsets changed — update RectUniformsData and its parity test"
        );
    }
}

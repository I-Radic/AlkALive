//! Target-agnostic scene tessellation â€” text shaping, glyph rasterization,
//! and vertex generation shared by the GPU backends.
//!
//! This module is the GPU-API-free core of the per-frame upload path: it
//! takes the per-frame [`TextSceneData`] plus canvas size and produces
//!
//! - the combined title + input-field **vertex buffer** (pixel-space quads,
//!   6 vertices per glyph), and
//! - the rasterized **glyph atlas page** (512Ã—512 single-channel R8),
//! - the computed **input-field bounds** used by hit-testing and by the
//!   render-graph's rect passes.
//!
//! The wgpu/WGSL backend consumes this directly. The GLSL/WebGL2 fallback
//! still contains its own copy of this logic inline (its implementation
//! predates this module); converging both onto [`tessellate_scene`] is the
//! documented follow-up so the two backends cannot drift.

use crate::{build_text_quads, build_vertex_buffer, quads_from_text, GlyphQuad, Vertex};
use alkalive_scene_data::TextSceneData;
use alkalive_text::{
    FontRegistry, FontRequest, HarfRustFontRegistry, HarfRustGlyphAtlas, HarfRustTextShaper,
    ShapeContext, TextShaper,
};

/// Re-export of the crate-level glyph-atlas page edge length: the
/// tessellation output must always match the renderer's texture allocation.
pub use crate::ATLAS_SIZE;

/// The complete CPU-side result of tessellating one scene frame.
#[derive(Debug, Clone)]
pub struct SceneTessellation {
    /// Combined vertex buffer: title vertices followed by input-field
    /// vertices. Upload as one vertex buffer; draw the two ranges with two
    /// `draw()` calls.
    pub vertices: Vec<Vertex>,
    /// Number of vertices belonging to the rotated title run (prefix of
    /// `vertices`).
    pub title_vertex_count: u32,
    /// Offset of the first input-field vertex within `vertices`.
    pub input_vertex_start: u32,
    /// Number of vertices belonging to the unrotated input-field run.
    pub input_vertex_count: u32,
    /// Rasterized atlas page 0 (`ATLAS_SIZE * ATLAS_SIZE` bytes, R8).
    pub atlas_page: Vec<u8>,
    /// Pixel-space `(x, y, w, h)` of the input field rectangle.
    pub input_field_bounds: (f32, f32, f32, f32),
}

impl SceneTessellation {
    /// Total vertex count in the combined buffer.
    pub fn total_vertex_count(&self) -> u32 {
        self.title_vertex_count + self.input_vertex_count
    }
}

/// Tessellate a scene into vertices + glyph atlas + layout bounds.
///
/// Mirrors the semantics of the GLSL backend's `upload_text_atlas`:
///
/// - Title text is shaped at `scene.font_size` and centered on the canvas;
///   it is drawn with Y-axis rotation (applied by the vertex shader from a
///   uniform, not baked here).
/// - The input-field display text is `scene.input_text` when non-empty,
///   otherwise `scene.input_placeholder`; it is shaped at half the title
///   font size and centered inside the input-field rectangle.
/// - The input field is centered horizontally, sized to
///   `min(width * 0.5, 400) Ã— 40` px, positioned below the vertical center.
///
/// A fresh [`alkalive_text::HarfRustGlyphAtlas`] is allocated per call
/// (matching current production behavior); the font registry/shaper pair is
/// cached process-wide behind an [`std::sync::OnceLock`] so the bundled TTF
/// is parsed exactly once per process instead of once per keystroke.
pub fn tessellate_scene(
    scene: &TextSceneData,
    width: f32,
    height: f32,
) -> Result<SceneTessellation, String> {
    let font = bundled_font();
    let font_id = font.font_id;

    // Fresh atlas per call â€” matches the GLSL backend's current behavior.
    let mut atlas = HarfRustGlyphAtlas::new(font.registry.clone());

    let title_font_size = scene.font_size;
    let input_font_size = scene.font_size * 0.5;

    // Shape the title run.
    let ctx_title = ShapeContext {
        font: font_id,
        size_px: title_font_size,
        direction: None,
    };
    let title_run = font
        .shaper
        .shape(&scene.text, &ctx_title)
        .map_err(|e| format!("shape title: {:?}", e))?;

    // Shape the input-field display run (typed text or placeholder).
    let input_display = if scene.input_text.is_empty() {
        scene.input_placeholder.as_str()
    } else {
        scene.input_text.as_str()
    };
    let ctx_input = ShapeContext {
        font: font_id,
        size_px: input_font_size,
        direction: None,
    };
    let input_run = font
        .shaper
        .shape(input_display, &ctx_input)
        .map_err(|e| format!("shape input: {:?}", e))?;

    // Rasterize glyphs into the atlas and build baseline-relative quads.
    let title_quads = build_text_quads(&title_run, &mut atlas, title_font_size);
    let input_quads = build_text_quads(&input_run, &mut atlas, input_font_size);

    if atlas.page_count() > 1 {
        // Only page 0 is uploaded by the renderers; overflow would silently
        // drop glyphs. Surface it loudly (same policy as the GLSL backend).
        return Err(format!(
            "glyph atlas overflowed to {} pages (capacity is one {}Ã—{} page)",
            atlas.page_count(),
            ATLAS_SIZE,
            ATLAS_SIZE
        ));
    }

    let atlas_page = atlas
        .page_data(0)
        .ok_or_else(|| "atlas page 0 missing".to_string())?
        .to_vec();

    // Canvas-centered title geometry.
    let title_canvas_quads: Vec<GlyphQuad> = quads_from_text(
        &title_quads,
        title_run.metrics.ascent,
        title_run.metrics.descent,
        title_run.metrics.total_advance,
        width,
        height,
    );
    let title_verts = build_vertex_buffer(&title_canvas_quads);

    // Input-field layout bounds (identical formula to the GLSL backend).
    let field_w = (width * 0.5).min(400.0);
    let field_h = 40.0f32;
    let field_x = (width - field_w) * 0.5;
    let field_y = (height * 0.5) + scene.font_size * 0.5 + 20.0;
    let input_field_bounds = (field_x, field_y, field_w, field_h);

    // Center input text inside the field.
    let input_baseline_x = field_x + (field_w - input_run.metrics.total_advance) * 0.5;
    let input_baseline_y = field_y + field_h * 0.5 + input_run.metrics.ascent * 0.5;

    let input_canvas_quads: Vec<GlyphQuad> = input_quads
        .iter()
        .map(|q| GlyphQuad {
            center_x: input_baseline_x + q.position.0 + q.size.0 * 0.5,
            center_y: input_baseline_y + q.position.1 + q.size.1 * 0.5,
            w: q.size.0,
            h: q.size.1,
            u0: q.uv.x,
            v0: q.uv.y,
            u1: q.uv.x + q.uv.w,
            v1: q.uv.y + q.uv.h,
        })
        .collect();
    let input_verts = build_vertex_buffer(&input_canvas_quads);

    let title_vertex_count = title_verts.len() as u32;
    let input_vertex_start = title_vertex_count;
    let input_vertex_count = input_verts.len() as u32;

    let mut vertices = title_verts;
    vertices.extend(input_verts);

    Ok(SceneTessellation {
        vertices,
        title_vertex_count,
        input_vertex_start,
        input_vertex_count,
        atlas_page,
        input_field_bounds,
    })
}

struct BundledFont {
    registry: std::sync::Arc<alkalive_text::HarfRustFontRegistry>,
    shaper: std::sync::Arc<alkalive_text::HarfRustTextShaper>,
    font_id: alkalive_text::FontId,
}

/// Process-wide cached font bundle: parse the bundled Roboto-Regular TTF
/// exactly once per process (the same M7 fix the GLSL backend applies, but
/// shared and lock-free after initialization).
fn bundled_font() -> &'static BundledFont {
    static FONT: std::sync::OnceLock<BundledFont> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let font_bytes: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");
        let mut registry = HarfRustFontRegistry::new();
        let loaded_id = registry
            .load_bundle(font_bytes)
            .expect("bundled Roboto-Regular.ttf must load");
        let req = FontRequest {
            family: "Roboto".to_string(),
            weight: 400,
            style: "normal",
        };
        let font_id = registry.resolve(&req).unwrap_or(loaded_id);
        let registry = std::sync::Arc::new(registry);
        let shaper = std::sync::Arc::new(HarfRustTextShaper::new(registry.clone()));
        BundledFont {
            registry,
            shaper,
            font_id,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_scene() -> TextSceneData {
        TextSceneData {
            text: "Hello World!".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn tessellates_nonempty_geometry_for_hello_world() {
        let t = tessellate_scene(&hello_scene(), 800.0, 600.0).expect("tessellate ok");
        assert!(t.title_vertex_count > 0, "title must produce vertices");
        assert!(t.input_vertex_count > 0, "placeholder must produce vertices");
        assert_eq!(
            t.total_vertex_count() as usize,
            t.vertices.len(),
            "combined buffer must hold exactly title+input vertices"
        );
        assert_eq!(t.input_vertex_start, t.title_vertex_count);
        assert_eq!(
            t.atlas_page.len(),
            (ATLAS_SIZE * ATLAS_SIZE) as usize,
            "atlas page is a full 512Ã—512 R8 page"
        );
        assert!(
            t.atlas_page.iter().any(|&b| b != 0),
            "rasterized glyphs must mark non-zero coverage in the atlas"
        );
    }

    #[test]
    fn input_field_bounds_match_layout_formula() {
        let t = tessellate_scene(&hello_scene(), 800.0, 600.0).expect("tessellate ok");
        let expected_w: f32 = (800.0f32 * 0.5).min(400.0);
        let expected_x: f32 = (800.0 - expected_w) * 0.5;
        let expected_y = 300.0 + hello_scene().font_size * 0.5 + 20.0;
        let (x, y, w, h) = t.input_field_bounds;
        assert_eq!(w, expected_w);
        assert_eq!(h, 40.0);
        assert!((x - expected_x).abs() < f32::EPSILON);
        assert!((y - expected_y).abs() < f32::EPSILON);
    }

    #[test]
    fn typed_input_and_placeholder_shape_differently() {
        let mut scene = hello_scene();
        scene.input_placeholder = "Type here...".to_string();
        let placeholder = tessellate_scene(&scene, 800.0, 600.0).unwrap();

        scene.input_text = "abc".to_string();
        let typed = tessellate_scene(&scene, 800.0, 600.0).unwrap();

        assert_ne!(
            placeholder.input_vertex_count, typed.input_vertex_count,
            "'abc' and 'Type here...' must shape to different vertex counts"
        );
    }

    #[test]
    fn empty_title_yields_zero_title_vertices_but_still_valid_output() {
        let mut scene = hello_scene();
        scene.text = String::new();
        let t = tessellate_scene(&scene, 800.0, 600.0).unwrap();
        assert_eq!(t.title_vertex_count, 0);
        assert!(t.input_vertex_count > 0);
    }

    #[test]
    fn vertices_are_pixel_space_inside_reasonable_bounds() {
        let t = tessellate_scene(&hello_scene(), 800.0, 600.0).unwrap();
        for v in &t.vertices {
            assert!(
                v.x > -100.0 && v.x < 900.0 && v.y > -100.0 && v.y < 700.0,
                "vertex outside canvas margin: {:?}",
                v
            );
            assert!(v.u >= 0.0 && v.u <= 1.0 && v.v >= 0.0 && v.v <= 1.0);
        }
    }
}
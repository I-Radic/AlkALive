//! Target-agnostic scene tessellation — text shaping, glyph rasterization,
//! and vertex generation shared by the GPU backends.
//!
//! This module is the GPU-API-free core of the per-frame upload path: it
//! takes the per-frame [`TextSceneData`] plus canvas size and produces
//!
//! - the combined title + input-field **vertex buffer** (pixel-space quads,
//!   6 vertices per glyph), and
//! - the rasterized **glyph atlas page** (512×512 single-channel R8),
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
    /// Security (T-D1): set when the combined vertex budget
    /// ([`MAX_TEXT_VERTICES`]) was exceeded and trailing glyph quads were
    /// dropped — a visible prefix still renders; renderers log this.
    pub truncated: bool,
}

impl SceneTessellation {
    /// Total vertex count in the combined buffer.
    pub fn total_vertex_count(&self) -> u32 {
        self.title_vertex_count + self.input_vertex_count
    }
}

/// Maximum number of vertices in a combined per-frame text vertex buffer
/// (security, T-D1: docs/security/06-mitigations.md).
///
/// A hostile 1 MiB input string can expand to ~1M glyphs ⇒ ~6M vertices ⇒
/// ~96 MB of CPU+GPU allocation before the shaper's length cap binds. This
/// budget bounds the worst case to `MAX_TEXT_VERTICES × 16` bytes of vertex
/// data (16 MiB) while leaving two orders of magnitude of headroom over any
/// realistic UI text (~174k glyphs). Truncation drops trailing glyph quads
/// (a visible prefix still renders) and is reported via
/// [`SceneTessellation::truncated`].
pub const MAX_TEXT_VERTICES: usize = 1_048_576;

/// Budget the two quad runs against [`MAX_TEXT_VERTICES`] (6 vertices per
/// glyph quad, title first). Shared by the wgpu tessellation path and the
/// GLSL backend's inline copy so the two cannot drift.
pub(crate) fn cap_quads_to_vertex_budget(
    mut title_quads: Vec<alkalive_text::Quad>,
    mut input_quads: Vec<alkalive_text::Quad>,
) -> (Vec<alkalive_text::Quad>, Vec<alkalive_text::Quad>, bool) {
    const VERTS_PER_QUAD: usize = 6;
    let budget = MAX_TEXT_VERTICES / VERTS_PER_QUAD;
    let mut truncated = false;
    if title_quads.len() > budget {
        title_quads.truncate(budget);
        truncated = true;
    }
    let remaining = budget - title_quads.len();
    if input_quads.len() > remaining {
        input_quads.truncate(remaining);
        truncated = true;
    }
    (title_quads, input_quads, truncated)
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
///   `min(width * 0.5, 400) × 40` px, positioned below the vertical center.
///
/// The glyph atlas is **persistent** (TD1 fix): this wrapper locks a
/// process-wide [`HarfRustGlyphAtlas`] created alongside the cached font
/// registry/shaper pair, so unchanged glyphs (notably the title) are cache
/// hits across re-tessellations and only newly typed glyphs rasterize. On
/// page overflow the atlas resets and the current text re-rasterizes (see
/// [`tessellate_scene_with_atlas`]). Use the `_with_atlas` variant to
/// supply an atlas with a different lifetime (e.g. a renderer-owned one).
pub fn tessellate_scene(
    scene: &TextSceneData,
    width: f32,
    height: f32,
) -> Result<SceneTessellation, String> {
    let mut guard = persistent_atlas()
        .lock()
        .map_err(|_| "persistent glyph atlas mutex poisoned".to_string())?;
    tessellate_scene_with_atlas(scene, width, height, &mut guard)
}

/// The process-wide persistent glyph atlas (TD1 fix): created lazily on
/// first use from the same bundled-font registry as the shaper, and
/// reused for the lifetime of the process.
fn persistent_atlas() -> &'static std::sync::Mutex<HarfRustGlyphAtlas> {
    static ATLAS: std::sync::OnceLock<std::sync::Mutex<HarfRustGlyphAtlas>> =
        std::sync::OnceLock::new();
    ATLAS.get_or_init(|| {
        std::sync::Mutex::new(HarfRustGlyphAtlas::new(bundled_font().registry.clone()))
    })
}

/// [`tessellate_scene`] with an explicit caller-owned atlas.
///
/// All glyph rasterization flows through `atlas`
/// ([`alkalive_text::GlyphAtlas::ensure`] semantics): glyphs already
/// cached from previous calls are cache hits, so a long-lived atlas avoids
/// re-rasterizing unchanged text. If the current text's glyphs overflow
/// the 512×512 page 0 (the only page the renderers upload), the atlas is
/// **reset and the current text re-rasterized** — a graceful recovery that
/// keeps every glyph on page 0 instead of erroring or silently dropping
/// glyphs.
pub fn tessellate_scene_with_atlas(
    scene: &TextSceneData,
    width: f32,
    height: f32,
    atlas: &mut HarfRustGlyphAtlas,
) -> Result<SceneTessellation, String> {
    let font = bundled_font();
    let font_id = font.font_id;

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
    // Unchanged glyphs (notably the title) are cache hits in a persistent
    // atlas; only new glyphs rasterize.
    let mut title_quads = build_text_quads(&title_run, atlas, title_font_size);
    let mut input_quads = build_text_quads(&input_run, atlas, input_font_size);

    if atlas.page_count() > 1 {
        // Only page 0 is uploaded by the renderers. A persistent atlas can
        // overflow after enough distinct input strings; reset and
        // re-rasterize just the current text so every glyph lands on
        // page 0 (graceful recovery — same policy as the GLSL backend).
        atlas.reset();
        title_quads = build_text_quads(&title_run, atlas, title_font_size);
        input_quads = build_text_quads(&input_run, atlas, input_font_size);
    }

    // Security (T-D1): bound the combined vertex budget before any
    // vertex-buffer allocation happens (16 B/vertex × 6 verts/quad).
    let (title_quads, input_quads, truncated) =
        cap_quads_to_vertex_budget(title_quads, input_quads);

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
        truncated,
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

    fn synthetic_quad(i: usize) -> alkalive_text::Quad {
        alkalive_text::Quad {
            position: (i as f32, 0.0),
            size: (1.0, 1.0),
            uv: alkalive_text::Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            page: 0,
        }
    }

    #[test]
    fn vertex_budget_cap_truncates_and_reports() {
        // Security (T-D1): a run exceeding MAX_TEXT_VERTICES/6 quads must be
        // truncated to exactly the budget and flagged; a small second run
        // must then only get the REMAINING budget.
        let budget_quads = MAX_TEXT_VERTICES / 6;
        let oversized_title: Vec<alkalive_text::Quad> =
            (0..budget_quads + 5).map(synthetic_quad).collect();
        let (title, input, truncated) =
            cap_quads_to_vertex_budget(oversized_title, vec![synthetic_quad(0); 3]);
        assert!(truncated, "oversized run must report truncation");
        assert_eq!(title.len(), budget_quads, "title truncated to exact budget");
        assert_eq!(input.len(), 0, "no budget left for the input run");
    }

    #[test]
    fn vertex_budget_cap_passes_small_runs_untouched() {
        // Well-behaved scenes must pass through unchanged and unflagged.
        let title: Vec<alkalive_text::Quad> = (0..12).map(synthetic_quad).collect();
        let input: Vec<alkalive_text::Quad> = (0..34).map(synthetic_quad).collect();
        let (t, i, truncated) = cap_quads_to_vertex_budget(title.clone(), input.clone());
        assert!(!truncated);
        assert_eq!(t.len(), 12);
        assert_eq!(i.len(), 34);
    }

    #[test]
    fn vertex_budget_cap_splits_remaining_budget_between_runs() {
        // Title within budget, input over the REMAINING budget: only the
        // input run is truncated (title keeps its quads).
        let title: Vec<alkalive_text::Quad> = (0..10).map(synthetic_quad).collect();
        let budget_quads = MAX_TEXT_VERTICES / 6;
        let oversized_input: Vec<alkalive_text::Quad> =
            (0..budget_quads).map(synthetic_quad).collect();
        let (t, i, truncated) = cap_quads_to_vertex_budget(title.clone(), oversized_input);
        assert!(truncated);
        assert_eq!(t.len(), 10, "title untouched");
        assert_eq!(
            i.len(),
            budget_quads - 10,
            "input gets exactly the remaining budget"
        );
    }

    #[test]
    fn persistent_atlas_reuses_cached_glyphs_across_tessellations() {
        // TD1: with an explicit long-lived atlas, the second tessellation
        // of the same scene must not rasterize any NEW glyph (cache hit
        // for every glyph) and must produce an identical atlas page.
        let mut atlas = HarfRustGlyphAtlas::new(bundled_font().registry.clone());
        let t1 = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut atlas)
            .expect("first tessellate ok");
        assert!(!atlas.is_empty());
        let cached = atlas.len();

        let t2 = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut atlas)
            .expect("second tessellate ok");
        assert_eq!(
            atlas.len(),
            cached,
            "same scene must be a full cache hit (no new glyphs)"
        );
        assert_eq!(
            t1.atlas_page, t2.atlas_page,
            "identical scenes must produce identical atlas pages"
        );
        assert_eq!(t1.vertices, t2.vertices);
    }

    #[test]
    fn persistent_atlas_overflows_then_recovers_via_reset() {
        // Force the caller-owned atlas past one page (many distinct glyph
        // sizes of a real outlined glyph), then tessellate: the
        // overflow-recovery path must reset and re-rasterize the current
        // text so every glyph lands on page 0 and the call still succeeds.
        use alkalive_text::{GlyphAtlas, GlyphKey};
        let font = bundled_font();
        let mut atlas = HarfRustGlyphAtlas::new(font.registry.clone());

        // Shape a real letter to obtain a glyph id with a non-empty
        // outline (Roboto's glyph 1 is `.null` — empty).
        let run = font
            .shaper
            .shape(
                "A",
                &ShapeContext {
                    font: font.font_id,
                    size_px: 64.0,
                    direction: None,
                },
            )
            .expect("shape 'A'");
        let outlined_gid = run.glyph_ids[0];

        let mut size: u16 = 200;
        let mut tried = 0;
        while atlas.page_count() == 1 && tried < 300 {
            atlas.ensure(GlyphKey {
                font_id: font.font_id,
                glyph_id: outlined_gid,
                phase: 0,
                size_px: size,
            });
            size = size.saturating_add(2);
            tried += 1;
        }
        assert!(
            atlas.page_count() > 1,
            "precondition: atlas must overflow before tessellating (tried {} sizes)",
            tried
        );

        let t = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut atlas)
            .expect("overflow recovery must succeed, not error");
        assert!(t.title_vertex_count > 0);
        assert!(t.input_vertex_count > 0);
        assert!(
            t.atlas_page.iter().any(|&b| b != 0),
            "re-rasterized glyphs must mark non-zero coverage"
        );
    }

    #[test]
    fn tessellation_output_is_cache_state_independent() {
        // The SceneTessellation produced for a given scene must not depend
        // on whether the atlas was cold or warm — cache hits return the
        // same slots, so vertices and pages are identical either way.
        let mut cold = HarfRustGlyphAtlas::new(bundled_font().registry.clone());
        let mut warm = HarfRustGlyphAtlas::new(bundled_font().registry.clone());
        // Warm the second atlas with the same scene first.
        let _ = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut warm).unwrap();

        let a = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut cold).unwrap();
        let b = tessellate_scene_with_atlas(&hello_scene(), 800.0, 600.0, &mut warm).unwrap();
        assert_eq!(a.vertices, b.vertices);
        assert_eq!(a.atlas_page, b.atlas_page);
        assert_eq!(a.input_field_bounds, b.input_field_bounds);
    }

    #[test]
    fn tessellates_nonempty_geometry_for_hello_world() {
        let t = tessellate_scene(&hello_scene(), 800.0, 600.0).expect("tessellate ok");
        assert!(t.title_vertex_count > 0, "title must produce vertices");
        assert!(
            t.input_vertex_count > 0,
            "placeholder must produce vertices"
        );
        assert_eq!(
            t.total_vertex_count() as usize,
            t.vertices.len(),
            "combined buffer must hold exactly title+input vertices"
        );
        assert_eq!(t.input_vertex_start, t.title_vertex_count);
        assert_eq!(
            t.atlas_page.len(),
            (ATLAS_SIZE * ATLAS_SIZE) as usize,
            "atlas page is a full 512×512 R8 page"
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

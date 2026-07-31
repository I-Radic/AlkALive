//! Text scene — orchestrates font loading, shaping, rasterization, and
//! positioning to produce a list of [`PositionedGlyph`]s ready for compositing.
//!
//! This module bridges the mature `alkalive-text` stack (HarfRust shaping +
//! glyph atlas rasterization) with the [`SoftwareRenderer`]. It implements
//! the missing `TextStack::rasterize` adapter (Gap G5) by walking a
//! [`ShapedRun`], calling `atlas.ensure()` per glyph, and accumulating pen
//! position to produce positioned glyph quads.

use std::sync::Arc;

use alkalive_text::{
    FontId, FontRegistry, FontRequest, GlyphAtlas, GlyphKey, HarfRustFontRegistry,
    HarfRustGlyphAtlas, HarfRustTextShaper, RunMetrics, ShapeContext, ShapedRun,
    TextShaper,
};

use crate::renderer::PositionedGlyph;

/// The embedded Roboto-Regular TTF font covering ASCII printable range.
/// Source: `vendor/harfrust/harfrust/benches/fonts/Roboto-Regular.ttf`
/// (Apache-2.0 licensed, 305 KB, covers U+0020–U+007E and much more).
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");

/// The text to render.
pub const HELLO_WORLD_TEXT: &str = "Hello World!";

/// The atlas page size (must match `alkalive_text::ATLAS_SIZE`).
const ATLAS_SIZE: usize = 512;

/// A complete text scene: font loaded, text shaped, glyphs rasterized into
/// the atlas, and positioned quads ready for rendering.
pub struct TextScene {
    /// The glyph atlas (owns rasterized glyph bitmaps in CPU-side pages).
    pub atlas: HarfRustGlyphAtlas,
    /// The shaped run (immutable output of HarfRust shaping).
    pub shaped_run: ShapedRun,
    /// Positioned glyphs ready for compositing.
    pub glyphs: Vec<PositionedGlyph>,
    /// The font ID used for shaping.
    pub font_id: FontId,
    /// The pixel size used for shaping.
    pub size_px: f32,
    /// The run metrics (ascent, descent, total advance).
    pub metrics: RunMetrics,
}

impl TextScene {
    /// Build a text scene for the given text at the given pixel size.
    ///
    /// This performs:
    /// 1. Font loading into `HarfRustFontRegistry`.
    /// 2. Font resolution via `FontRequest`.
    /// 3. Text shaping via `HarfRustTextShaper`.
    /// 4. Per-glyph rasterization via `HarfRustGlyphAtlas::ensure`.
    /// 5. Pen-position accumulation to produce `PositionedGlyph`s.
    pub fn new(text: &str, size_px: f32) -> Result<Self, String> {
        // 1. Load font into registry.
        let mut registry = HarfRustFontRegistry::new();
        let loaded_id = registry
            .load_bundle(FONT_BYTES)
            .map_err(|e| format!("Font load failed: {:?}", e))?;

        // 2. Resolve font (Roboto has weight 400, family "Roboto").
        let req = FontRequest {
            family: "Roboto".to_string(),
            weight: 400,
            style: "normal",
        };
        let font_id = registry.resolve(&req).unwrap_or(loaded_id);

        // Wrap in Arc for sharing between shaper and atlas.
        let registry_arc = Arc::new(registry);

        // 3. Create shaper and atlas.
        let shaper = HarfRustTextShaper::new(Arc::clone(&registry_arc));
        let mut atlas = HarfRustGlyphAtlas::new(registry_arc);

        // 4. Shape the text.
        let ctx = ShapeContext {
            font: font_id,
            size_px,
            direction: None, // Auto-detect (LTR for "Hello World!")
        };
        let shaped_run = shaper
            .shape(text, &ctx)
            .map_err(|e| format!("Shaping failed: {:?}", e))?;

        let metrics = shaped_run.metrics;

        // 5. Rasterize each glyph and accumulate pen position.
        let glyphs = Self::rasterize_run(&shaped_run, &mut atlas, size_px);

        Ok(Self {
            atlas,
            shaped_run,
            glyphs,
            font_id,
            size_px,
            metrics,
        })
    }

    /// Walk a [`ShapedRun`], rasterize each glyph into the atlas, and produce
    /// positioned glyph quads. This is the §6.5 `TextStack::rasterize`
    /// adapter that was missing (Gap G5).
    ///
    /// Pen position accumulates horizontally using `advances` and `offsets`.
    /// The baseline is at y=0; glyphs are positioned relative to the baseline
    /// using their `bearing` from the [`AtlasSlot`].
    pub fn rasterize_run(
        run: &ShapedRun,
        atlas: &mut HarfRustGlyphAtlas,
        size_px: f32,
    ) -> Vec<PositionedGlyph> {
        let mut glyphs = Vec::with_capacity(run.glyph_ids.len());
        let mut pen_x = 0.0f32;

        for (i, &glyph_id) in run.glyph_ids.iter().enumerate() {
            let key = GlyphKey {
                font_id: run.font_id,
                glyph_id,
                phase: 0,
                size_px: size_px as u16,
            };

            let slot = atlas.ensure(key);

            // Skip degenerate slots (space, .notdef with no outline).
            if slot.size.0 < 0.5 || slot.size.1 < 0.5 {
                pen_x += run.advances[i];
                continue;
            }

            // Glyph position: pen_x + offset_x + bearing_x (horizontal)
            //                 baseline + offset_y - bearing_y (vertical)
            //
            // The bearing is baseline-relative. In screen coordinates (y-down),
            // the glyph bitmap's top-left is at:
            let offset_x = run.offsets[i].0;
            let offset_y = run.offsets[i].1;

            let x = pen_x + offset_x + slot.bearing.0;
            // bearing.1 = y_max_ceil = vertical offset from baseline to bitmap
            // top (positive = up, FreeType convention). In screen coords
            // (y-down), the bitmap's top-left is at: baseline + offset_y - bearing.1
            let y = offset_y - slot.bearing.1;

            glyphs.push(PositionedGlyph {
                page: slot.page,
                uv_x: slot.uv.x,
                uv_y: slot.uv.y,
                uv_w: slot.uv.w,
                uv_h: slot.uv.h,
                x,
                y,
                w: slot.size.0,
                h: slot.size.1,
            });

            pen_x += run.advances[i];
        }

        glyphs
    }

    /// Get the atlas page data for compositing.
    pub fn page_data(&self, page: usize) -> Option<&[u8]> {
        self.atlas.page_data(page)
    }

    /// Get the total width of the shaped text (sum of advances).
    pub fn total_width(&self) -> f32 {
        self.metrics.total_advance
    }

    /// Get the ascent (distance from baseline to top of ascenders).
    pub fn ascent(&self) -> f32 {
        self.metrics.ascent
    }

    /// Get the descent (distance from baseline to bottom of descenders, typically negative).
    pub fn descent(&self) -> f32 {
        self.metrics.descent
    }

    /// The atlas size (dimension of each square page).
    pub fn atlas_size(&self) -> usize {
        ATLAS_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_scene_shapes_hello_world() {
        let scene = TextScene::new(HELLO_WORLD_TEXT, 48.0);
        assert!(scene.is_ok(), "Failed to create text scene");
        let scene = scene.unwrap();
        assert!(!scene.glyphs.is_empty(), "No glyphs were produced");
        assert!(scene.total_width() > 0.0, "Total width should be positive");
        assert!(scene.ascent() > 0.0, "Ascent should be positive");
    }

    #[test]
    fn text_scene_produces_visible_glyphs() {
        let scene = TextScene::new("Hello", 48.0).unwrap();
        // At least some glyphs should have non-zero size (not spaces).
        let visible = scene
            .glyphs
            .iter()
            .filter(|g| g.w > 0.5 && g.h > 0.5)
            .count();
        assert!(
            visible > 0,
            "Expected at least one visible glyph, got {} visible out of {} total",
            visible,
            scene.glyphs.len()
        );
    }

    #[test]
    fn text_scene_atlas_has_pixels() {
        let scene = TextScene::new("Hello", 48.0).unwrap();
        let page = scene.page_data(0).expect("Page 0 should exist");
        // The atlas should have some non-zero pixels (rasterized glyphs).
        let non_zero = page.iter().filter(|&&p| p > 0).count();
        assert!(
            non_zero > 0,
            "Atlas page should have non-zero pixels after rasterization"
        );
    }

    #[test]
    fn text_scene_supports_ascii_range() {
        // Test all ASCII printable characters.
        let text: String = (32u8..=126).map(|c| c as char).collect();
        let scene = TextScene::new(&text, 32.0);
        assert!(scene.is_ok(), "Failed to shape ASCII printable range");
        let scene = scene.unwrap();
        assert!(!scene.glyphs.is_empty());
    }
}

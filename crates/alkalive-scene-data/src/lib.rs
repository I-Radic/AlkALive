//! AlkALive per-frame scene data — `TextSceneData`.
//!
//! This crate is the canonical home of the [`TextSceneData`] struct. It was
//! extracted out of `alkalive-backend-wgpu` (Wave 11, Gap 6 — Render-Graph
//! IR) to break a structural dependency cycle: `alkalive-render` needs
//! `TextSceneData` to build a render graph, but `alkalive-backend-wgpu`
//! depends on `alkalive-render`. Per the spec
//! (`docs/alkalive-specification-rendering.md` §0.3 — CR-3 resolution), the
//! cycle is broken by moving `TextSceneData` into this tiny crate.
//!
//! `TextSceneData` is the runtime's view of a `SceneIR` after layout — a
//! single text run with rotation, a background color, and a foreground (text)
//! color, plus an input-field text + placeholder. The runtime mutates it
//! in place as the user types, and the render-graph builder consumes it to
//! produce draw calls.
//!
//! # Cross-target compilation
//!
//! This crate has no platform-specific dependencies and compiles cleanly on
//! both native and `wasm32` targets. It is intentionally minimal — only the
//! `TextSceneData` struct, its `Default` impl, its `new()` constructor, and
//! its `background_normalized()` helper live here.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// The per-frame scene description passed to the renderer.
///
/// This is the runtime's view of a `SceneIR` after layout — a single text
/// run with rotation, a background color, and a foreground (text) color.
/// Originally defined in `alkalive-backend-wgpu`; moved here in Wave 11
/// (Gap 6 — Render-Graph IR) to break the `alkalive-render` ↔
/// `alkalive-backend-wgpu` dependency cycle.
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
    /// Input field text (what the user has typed). Empty string = show placeholder.
    pub input_text: String,
    /// Input field placeholder text (shown when input_text is empty).
    pub input_placeholder: String,
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
            input_text: String::new(),
            input_placeholder: "Type here...".to_string(),
        }
    }
}

impl TextSceneData {
    /// Construct a default golden-on-black scene with the given text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            input_text: String::new(),
            input_placeholder: "Type here...".to_string(),
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
}

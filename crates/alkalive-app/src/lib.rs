//! AlkALive application crate — WASM entry points for the Hello World deployment.
//!
//! This crate is a `cdylib` that exports `#[wasm_bindgen]` functions callable
//! from JavaScript. It constructs the Hello World scene directly in Rust
//! (bypassing the not-yet-implemented `.alk` compiler), using the real
//! AlkaLive text stack (HarfRust shaping + glyph rasterization) and a
//! CPU software renderer.
//!
//! ## WASM Exports
//!
//! - [`init`] — Create the renderer and text scene.
//! - [`tick`] — Render one frame (clear + composite text with rotation).
//! - [`get_framebuffer_ptr`] — Raw pointer to the RGBA framebuffer.
//! - [`get_framebuffer_len`] — Framebuffer length in bytes.
//! - [`resize`] — Resize the framebuffer.
//! - [`get_width`] / [`get_height`] — Current dimensions.

#![allow(unsafe_code)]

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

pub mod renderer;
pub mod text_scene;

use renderer::{PositionedGlyph, SoftwareRenderer};
use text_scene::TextScene;

/// Rotation speed in radians per second.
const ROTATION_SPEED: f32 = 0.5;

/// Font size in pixels.
const FONT_SIZE_PX: f32 = 64.0;

/// The application state: renderer + text scene + animation time.
struct App {
    renderer: SoftwareRenderer,
    scene: TextScene,
    time: f32,
    /// Offset to center the text horizontally in the framebuffer.
    text_offset_x: f32,
    /// Offset to place the baseline vertically center.
    text_baseline_y: f32,
    /// The center X for rotation pivot.
    rotation_center_x: f32,
}

thread_local! {
    static APP: RefCell<Option<App>> = RefCell::new(None);
}

/// Initialize the application with the given canvas dimensions.
///
/// This loads the embedded Roboto font, shapes "Hello World!", rasterizes
/// all glyphs into the atlas, and prepares the framebuffer.
#[wasm_bindgen]
pub fn init(width: u32, height: u32) -> Result<(), JsValue> {
    // Set up panic hook for better error messages in the browser console.
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("AlkALive panic: {}", info).into());
    }));

    // Create the text scene.
    let scene = TextScene::new(text_scene::HELLO_WORLD_TEXT, FONT_SIZE_PX)
        .map_err(|e| JsValue::from_str(&format!("TextScene error: {}", e)))?;

    // Create the renderer.
    let mut renderer = SoftwareRenderer::new(width, height);

    // Calculate centering offsets.
    let text_width = scene.total_width();
    let text_ascent = scene.ascent();
    let text_descent = scene.descent();
    let text_height = text_ascent - text_descent;

    let text_offset_x = (width as f32 - text_width) / 2.0;
    let text_baseline_y = (height as f32 - text_height) / 2.0 + text_ascent;
    let rotation_center_x = width as f32 / 2.0;

    let _ = &mut renderer; // Suppress unused mut warning

    APP.with(|app| {
        *app.borrow_mut() = Some(App {
            renderer,
            scene,
            time: 0.0,
            text_offset_x,
            text_baseline_y,
            rotation_center_x,
        });
    });

    Ok(())
}

/// Render one frame.
///
/// Advances the animation time, clears the framebuffer to black, and
/// composites the golden "Hello World!" text with a Y-axis rotation.
#[wasm_bindgen]
pub fn tick() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let app = match app.as_mut() {
            Some(a) => a,
            None => return,
        };

        // Advance time.
        app.time += 1.0 / 60.0; // Assume 60 FPS.
        let angle = app.time * ROTATION_SPEED;

        // Clear framebuffer to black.
        app.renderer.clear();

        // Get the atlas page data (all glyphs are on page 0 for this simple scene).
        let atlas_size = app.scene.atlas_size();
        let page_data = match app.scene.page_data(0) {
            Some(d) => d,
            None => return,
        };

        // Transform glyphs to screen coordinates (apply centering offsets).
        let screen_glyphs: Vec<PositionedGlyph> = app
            .scene
            .glyphs
            .iter()
            .map(|g| {
                let mut sg = *g;
                // Apply horizontal offset (centering).
                sg.x += app.text_offset_x;
                // Apply vertical offset: glyphs are relative to baseline (y=0).
                // Screen y = baseline_y + glyph_relative_y.
                sg.y += app.text_baseline_y;
                sg
            })
            .collect();

        // Composite with rotation.
        app.renderer.composite_glyphs_rotated(
            page_data,
            atlas_size,
            &screen_glyphs,
            angle,
            app.rotation_center_x,
        );
    });
}

/// Get a raw pointer to the framebuffer data.
///
/// In JavaScript, use this with `new Uint8Array(wasm.memory.buffer, ptr, len)`
/// to create a view into the WASM memory without copying.
#[wasm_bindgen]
pub fn get_framebuffer_ptr() -> usize {
    APP.with(|app| {
        let app = app.borrow();
        match app.as_ref() {
            Some(a) => a.renderer.framebuffer_ptr() as usize,
            None => 0,
        }
    })
}

/// Get the framebuffer length in bytes.
#[wasm_bindgen]
pub fn get_framebuffer_len() -> usize {
    APP.with(|app| {
        let app = app.borrow();
        match app.as_ref() {
            Some(a) => a.renderer.framebuffer_len(),
            None => 0,
        }
    })
}

/// Resize the framebuffer.
#[wasm_bindgen]
pub fn resize(width: u32, height: u32) {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        if let Some(a) = app.as_mut() {
            a.renderer.resize(width, height);
            // Recalculate centering offsets.
            let text_width = a.scene.total_width();
            let text_ascent = a.scene.ascent();
            let text_descent = a.scene.descent();
            let text_height = text_ascent - text_descent;
            a.text_offset_x = (width as f32 - text_width) / 2.0;
            a.text_baseline_y = (height as f32 - text_height) / 2.0 + text_ascent;
            a.rotation_center_x = width as f32 / 2.0;
        }
    });
}

/// Get the framebuffer width.
#[wasm_bindgen]
pub fn get_width() -> u32 {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0, |a| a.renderer.width)
    })
}

/// Get the framebuffer height.
#[wasm_bindgen]
pub fn get_height() -> u32 {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0, |a| a.renderer.height)
    })
}

// ============================================================================
// Native (non-WASM) entry point for testing
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
pub fn render_test_frame(width: u32, height: u32) -> Vec<u8> {
    init(width, height).expect("init failed");
    tick();
    APP.with(|app| {
        let app = app.borrow();
        let a = app.as_ref().expect("app not initialized");
        a.renderer.framebuffer.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_tick_produces_non_blank_framebuffer() {
        let fb = render_test_frame(800, 600);
        assert_eq!(fb.len(), 800 * 600 * 4);

        // Count non-black pixels (should have golden text pixels).
        let non_black = fb
            .chunks_exact(4)
            .filter(|c| c[0] > 0 || c[1] > 0 || c[2] > 0)
            .count();
        assert!(
            non_black > 100,
            "Expected at least 100 non-black pixels (golden text), got {}",
            non_black
        );
    }

    #[test]
    fn framebuffer_has_golden_pixels() {
        let fb = render_test_frame(800, 600);

        // Check for golden-tinted pixels. Due to anti-aliasing, edge pixels
        // have partial alpha, so we check for R > G > B pattern (golden tint)
        // rather than exact color match.
        let golden = fb
            .chunks_exact(4)
            .filter(|c| c[0] > 50 && c[1] > 30 && c[0] > c[1] && c[2] < c[1])
            .count();
        assert!(
            golden > 0,
            "Expected at least some golden-tinted pixels in the framebuffer"
        );
    }

    #[test]
    fn resize_updates_dimensions() {
        init(400, 300).unwrap();
        assert_eq!(get_width(), 400);
        assert_eq!(get_height(), 300);
        resize(800, 600);
        assert_eq!(get_width(), 800);
        assert_eq!(get_height(), 600);
    }
}

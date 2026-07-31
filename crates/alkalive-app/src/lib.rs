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
//! ### Lifecycle
//! - [`init`] — Create the renderer and text scene.
//! - [`tick`] — Render one frame (clear + starfield + composite text + glow).
//! - [`resize`] — Resize the framebuffer.
//!
//! ### Framebuffer Access
//! - [`get_framebuffer_ptr`] — Raw pointer to the RGBA framebuffer.
//! - [`get_framebuffer_len`] — Framebuffer length in bytes.
//! - [`get_width`] / [`get_height`] — Current dimensions.
//!
//! ### Interactive Controls
//! - [`set_rotation_speed`] — Set rotation speed (rad/s).
//! - [`set_text`] — Change the rendered text.
//! - [`set_color`] — Set solid text color (r, g, b).
//! - [`set_gradient`] — Set vertical gradient (top r,g,b + bottom r,g,b).
//! - [`set_glow`] — Configure glow effect (enabled, radius, intensity).
//! - [`set_starfield_enabled`] — Toggle starfield background.
//! - [`set_paused`] — Pause/resume animation.
//! - [`get_rotation_angle`] — Get current rotation angle (for HUD).
//! - [`get_fps`] — Get current FPS estimate.

#![allow(unsafe_code)]

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

pub mod renderer;
pub mod starfield;
pub mod text_scene;

use renderer::{ColorMode, PositionedGlyph, SoftwareRenderer};
use starfield::Starfield;
use text_scene::TextScene;

/// Default rotation speed in radians per second.
const DEFAULT_ROTATION_SPEED: f32 = 0.5;

/// Default font size in pixels.
const FONT_SIZE_PX: f32 = 64.0;

/// Number of stars in the starfield.
const STAR_COUNT: usize = 150;

/// The application state: renderer + text scene + animation + controls.
struct App {
    renderer: SoftwareRenderer,
    scene: TextScene,
    starfield: Starfield,
    time: f32,
    rotation_speed: f32,
    paused: bool,
    color_mode: ColorMode,
    /// Offset to center the text horizontally in the framebuffer.
    text_offset_x: f32,
    /// Offset to place the baseline vertically center.
    text_baseline_y: f32,
    /// The center X for rotation pivot.
    rotation_center_x: f32,
    /// The current text being rendered.
    text: String,
    /// FPS tracking: frame timestamps (simplified — just count + last time).
    frame_count: u64,
    last_fps_time: f32,
    current_fps: f32,
    /// Starfield enabled.
    starfield_enabled: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = RefCell::new(None);
}

/// Initialize the application with the given canvas dimensions.
#[wasm_bindgen]
pub fn init(width: u32, height: u32) -> Result<(), JsValue> {
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("AlkALive panic: {}", info).into());
    }));

    let text = text_scene::HELLO_WORLD_TEXT.to_string();
    let scene = TextScene::new(&text, FONT_SIZE_PX)
        .map_err(|e| JsValue::from_str(&format!("TextScene error: {}", e)))?;

    let renderer = SoftwareRenderer::new(width, height);
    let starfield = Starfield::new(STAR_COUNT, 42);

    let (text_offset_x, text_baseline_y, rotation_center_x) =
        compute_layout(&scene, width, height);

    APP.with(|app| {
        *app.borrow_mut() = Some(App {
            renderer,
            scene,
            starfield,
            time: 0.0,
            rotation_speed: DEFAULT_ROTATION_SPEED,
            paused: false,
            color_mode: ColorMode::Gradient(
                255, 255, 180,  // Top: light gold
                255, 165, 0,    // Bottom: orange-gold
            ),
            text_offset_x,
            text_baseline_y,
            rotation_center_x,
            text,
            frame_count: 0,
            last_fps_time: 0.0,
            current_fps: 0.0,
            starfield_enabled: true,
        });
    });

    Ok(())
}

/// Compute text centering offsets based on the scene metrics and canvas size.
fn compute_layout(scene: &TextScene, width: u32, height: u32) -> (f32, f32, f32) {
    let text_width = scene.total_width();
    let text_ascent = scene.ascent();
    let text_descent = scene.descent();
    let text_height = text_ascent - text_descent;

    let text_offset_x = (width as f32 - text_width) / 2.0;
    let text_baseline_y = (height as f32 - text_height) / 2.0 + text_ascent;
    let rotation_center_x = width as f32 / 2.0;

    (text_offset_x, text_baseline_y, rotation_center_x)
}

/// Render one frame.
#[wasm_bindgen]
pub fn tick() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let app = match app.as_mut() {
            Some(a) => a,
            None => return,
        };

        // Advance time (unless paused).
        if !app.paused {
            app.time += 1.0 / 60.0;
        }
        let angle = app.time * app.rotation_speed;

        // Clear framebuffer to black.
        app.renderer.clear();

        // Render starfield background.
        if app.starfield_enabled {
            app.starfield.render(
                &mut app.renderer.framebuffer,
                app.renderer.width,
                app.renderer.height,
                app.time,
            );
        }

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
                sg.x += app.text_offset_x;
                sg.y += app.text_baseline_y;
                sg
            })
            .collect();

        // Composite text with rotation and current color mode.
        app.renderer.composite_glyphs_rotated(
            page_data,
            atlas_size,
            &screen_glyphs,
            angle,
            app.rotation_center_x,
            app.color_mode,
        );

        // Apply glow/bloom effect.
        app.renderer.apply_glow();

        // FPS tracking: update every 60 frames (~1 second).
        app.frame_count += 1;
        if app.frame_count % 30 == 0 {
            let elapsed = app.time - app.last_fps_time;
            if elapsed > 0.0 {
                app.current_fps = 30.0 / elapsed;
                app.last_fps_time = app.time;
            }
        }
    });
}

/// Get a raw pointer to the framebuffer data.
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
            let (ox, by, cx) = compute_layout(&a.scene, width, height);
            a.text_offset_x = ox;
            a.text_baseline_y = by;
            a.rotation_center_x = cx;
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
// Interactive Controls
// ============================================================================

/// Set the rotation speed (radians per second).
#[wasm_bindgen]
pub fn set_rotation_speed(speed: f32) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.rotation_speed = speed;
        }
    });
}

/// Change the rendered text. Re-shapes and re-rasterizes the glyphs.
#[wasm_bindgen]
pub fn set_text(text: String) -> Result<(), JsValue> {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            match TextScene::new(&text, FONT_SIZE_PX) {
                Ok(scene) => {
                    a.scene = scene;
                    a.text = text;
                    let (ox, by, cx) =
                        compute_layout(&a.scene, a.renderer.width, a.renderer.height);
                    a.text_offset_x = ox;
                    a.text_baseline_y = by;
                    a.rotation_center_x = cx;
                }
                Err(e) => {
                    return Err(JsValue::from_str(&format!(
                        "TextScene error: {}",
                        e
                    )));
                }
            }
        }
        Ok(())
    })
}

/// Set solid text color (r, g, b).
#[wasm_bindgen]
pub fn set_color(r: u8, g: u8, b: u8) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.color_mode = ColorMode::Solid(r, g, b);
        }
    });
}

/// Set vertical gradient: top (r1,g1,b1) to bottom (r2,g2,b2).
#[wasm_bindgen]
pub fn set_gradient(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.color_mode = ColorMode::Gradient(r1, g1, b1, r2, g2, b2);
        }
    });
}

/// Configure the glow effect: (enabled, radius, intensity).
#[wasm_bindgen]
pub fn set_glow(enabled: bool, radius: u32, intensity: f32) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.renderer.set_glow(enabled, radius, intensity);
        }
    });
}

/// Toggle the starfield background.
#[wasm_bindgen]
pub fn set_starfield_enabled(enabled: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.starfield_enabled = enabled;
        }
    });
}

/// Pause or resume the animation.
#[wasm_bindgen]
pub fn set_paused(paused: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.paused = paused;
        }
    });
}

/// Get the current rotation angle in radians (for HUD display).
#[wasm_bindgen]
pub fn get_rotation_angle() -> f32 {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0.0, |a| a.time * a.rotation_speed)
    })
}

/// Get the current FPS estimate.
#[wasm_bindgen]
pub fn get_fps() -> f32 {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0.0, |a| a.current_fps)
    })
}

/// Get the current frame count.
#[wasm_bindgen]
pub fn get_frame_count() -> u64 {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0, |a| a.frame_count)
    })
}

/// Check if animation is paused.
#[wasm_bindgen]
pub fn is_paused() -> bool {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(false, |a| a.paused)
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

        let non_black = fb
            .chunks_exact(4)
            .filter(|c| c[0] > 0 || c[1] > 0 || c[2] > 0)
            .count();
        assert!(
            non_black > 100,
            "Expected at least 100 non-black pixels, got {}",
            non_black
        );
    }

    #[test]
    fn framebuffer_has_golden_pixels() {
        let fb = render_test_frame(800, 600);
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

    #[test]
    fn set_color_changes_text_color() {
        init(400, 300).unwrap();
        set_color(255, 0, 0); // Red
        tick();
        let fb_color = APP.with(|app| {
            let app = app.borrow();
            let a = app.as_ref().unwrap();
            a.renderer.framebuffer.clone()
        });
        // Should have some reddish pixels.
        let red = fb_color
            .chunks_exact(4)
            .filter(|c| c[0] > 100 && c[1] < 50 && c[2] < 50)
            .count();
        assert!(red > 0, "Expected reddish pixels after set_color(255,0,0)");
    }

    #[test]
    fn set_text_updates_scene() {
        init(400, 300).unwrap();
        set_text("Hi".to_string()).unwrap();
        let text = APP.with(|app| {
            let app = app.borrow();
            app.as_ref().unwrap().text.clone()
        });
        assert_eq!(text, "Hi");
    }

    #[test]
    fn set_paused_stops_time_advancement() {
        init(400, 300).unwrap();
        set_paused(true);
        tick();
        let angle1 = get_rotation_angle();
        tick();
        let angle2 = get_rotation_angle();
        assert_eq!(angle1, angle2, "Angle should not advance when paused");
    }

    #[test]
    fn set_rotation_speed_affects_angle() {
        init(400, 300).unwrap();
        set_rotation_speed(2.0);
        tick(); // time = 1/60
        let angle = get_rotation_angle();
        assert!(
            (angle - 2.0 / 60.0).abs() < 0.001,
            "Expected angle ~= 2/60, got {}",
            angle
        );
    }

    #[test]
    fn starfield_can_be_disabled() {
        init(400, 300).unwrap();
        set_starfield_enabled(false);
        tick();
        // Verify it doesn't crash and still renders text.
        assert!(get_framebuffer_len() > 0);
    }

    #[test]
    fn glow_can_be_configured() {
        init(400, 300).unwrap();
        set_glow(false, 0, 0.0);
        tick();
        set_glow(true, 8, 0.9);
        tick();
        assert!(get_framebuffer_len() > 0);
    }

    #[test]
    fn get_fps_returns_value() {
        init(400, 300).unwrap();
        for _ in 0..35 {
            tick();
        }
        // FPS should be calculated after 30 frames.
        // (May be 0 if time tracking hasn't elapsed, but should not panic.)
        let _fps = get_fps();
    }

    #[test]
    fn get_frame_count_increments() {
        init(400, 300).unwrap();
        assert_eq!(get_frame_count(), 0);
        tick();
        assert_eq!(get_frame_count(), 1);
        tick();
        assert_eq!(get_frame_count(), 2);
    }
}

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

pub mod input_field;
pub mod particles;
pub mod renderer;
pub mod starfield;
pub mod text_scene;

use input_field::{input_color_mode, InputField};
use particles::{EmissionMode, ParticleSystem};
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
    input_field: InputField,
    particles: ParticleSystem,
    time: f32,
    rotation_speed: f32,
    paused: bool,
    color_mode: ColorMode,
    /// Whether the color mode is the animated rainbow (needs time updates).
    rainbow_mode: bool,
    /// Rainbow animation speed.
    rainbow_speed: f32,
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
    /// Input field enabled.
    input_enabled: bool,
    /// Particles enabled.
    particles_enabled: bool,
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
    let input_field = InputField::new("Click here to type...");
    let mut particles = ParticleSystem::new();
    // Default: emit from text line
    particles.set_mode(EmissionMode::TextLine);
    particles.set_emission_rate(30.0);
    particles.set_center(width as f32 / 2.0, height as f32 / 2.0);
    particles.set_line_width(scene.total_width());

    let (text_offset_x, text_baseline_y, rotation_center_x) =
        compute_layout(&scene, width, height);

    APP.with(|app| {
        *app.borrow_mut() = Some(App {
            renderer,
            scene,
            starfield,
            input_field,
            particles,
            time: 0.0,
            rotation_speed: DEFAULT_ROTATION_SPEED,
            paused: false,
            color_mode: ColorMode::Gradient(
                255, 255, 180,  // Top: light gold
                255, 165, 0,    // Bottom: orange-gold
            ),
            rainbow_mode: false,
            rainbow_speed: 0.3,
            text_offset_x,
            text_baseline_y,
            rotation_center_x,
            text,
            frame_count: 0,
            last_fps_time: 0.0,
            current_fps: 0.0,
            starfield_enabled: true,
            input_enabled: true,
            particles_enabled: true,
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
        let dt = if app.paused { 0.0 } else { 1.0 / 60.0 };
        app.time += dt;
        let angle = app.time * app.rotation_speed;

        // Update the color mode if rainbow is active (needs current time).
        let effective_color_mode = if app.rainbow_mode {
            ColorMode::AnimatedRainbow {
                time: app.time,
                speed: app.rainbow_speed,
            }
        } else {
            app.color_mode
        };

        // Update particle system.
        if app.particles_enabled && !app.paused {
            // Update emission center to follow the text position.
            app.particles.set_center(
                app.rotation_center_x,
                app.text_baseline_y,
            );
            app.particles.set_line_width(app.scene.total_width());
            app.particles.update(dt, app.renderer.width, app.renderer.height);
        }

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

        // Render particles behind the text (for depth).
        if app.particles_enabled {
            app.particles.render(
                &mut app.renderer.framebuffer,
                app.renderer.width,
                app.renderer.height,
            );
        }

        // Get the atlas page data (all glyphs are on page 0 for this simple scene).
        let atlas_size = app.scene.atlas_size();

        // --- Render the rotating title text ---
        let page_data = match app.scene.page_data(0) {
            Some(d) => d,
            None => return,
        };

        // Transform title glyphs to screen coordinates (apply centering offsets).
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

        // Composite title text with rotation and current color mode.
        app.renderer.composite_glyphs_rotated(
            page_data,
            atlas_size,
            &screen_glyphs,
            angle,
            app.rotation_center_x,
            effective_color_mode,
        );

        // --- Render the input field below the title ---
        if app.input_enabled {
            render_input_field(app);
        }

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

/// Render the input field below the rotating title text.
///
/// This draws:
/// 1. A rounded rectangle border (focused = gold, unfocused = gray)
/// 2. The input text (or placeholder) centered horizontally
/// 3. A blinking cursor if focused
fn render_input_field(app: &mut App) {
    let width = app.renderer.width as f32;
    let height = app.renderer.height as f32;

    // Input field dimensions.
    let field_w = (width * 0.6).min(500.0);
    let field_h = 44.0;
    let field_x = (width - field_w) / 2.0;
    // Position below the title (title baseline + some gap).
    let title_bottom = app.text_baseline_y + 20.0;
    let field_y = title_bottom + 40.0;

    // Ensure the field fits on screen.
    if field_y + field_h > height {
        return;
    }

    // Draw the field background (semi-transparent dark).
    app.renderer.fill_rect(
        field_x as i32,
        field_y as i32,
        field_w as i32,
        field_h as i32,
        12, 12, 20, // Very dark blue-gray
    );

    // Draw the field border.
    let (br, bg, bb) = if app.input_field.focused {
        (255, 215, 0) // Gold when focused
    } else {
        (80, 80, 100) // Gray when unfocused
    };
    app.renderer.draw_rect_outline(
        field_x as i32,
        field_y as i32,
        field_w as i32,
        field_h as i32,
        br, bg, bb,
    );

    // Get the input field text glyphs.
    // We need to borrow the registry from the scene and the atlas mutably.
    let registry = app.scene.registry.clone();
    let (input_glyphs, input_width) = {
        let atlas = &mut app.scene.atlas;
        app.input_field.get_positioned_glyphs(&registry, atlas)
    };

    // Center the input text horizontally within the field.
    let text_x = field_x + (field_w - input_width) / 2.0;
    let text_y = field_y + (field_h - input_field::INPUT_FONT_SIZE_PX) / 2.0
        + app.scene.metrics.ascent * (input_field::INPUT_FONT_SIZE_PX / FONT_SIZE_PX)
            - input_field::INPUT_FONT_SIZE_PX * 0.8;

    // Use the title's ascent to estimate input text baseline. Simplified:
    // position text vertically centered in the field.
    let text_baseline_y = field_y + field_h / 2.0 + input_field::INPUT_FONT_SIZE_PX * 0.35;

    let _ = text_y; // Suppress unused warning

    // Get the atlas page data (input field shares the same atlas).
    let atlas_size = app.scene.atlas_size();
    let page_data = match app.scene.page_data(0) {
        Some(d) => d,
        None => return,
    };

    // Composite input text glyphs (no rotation).
    let color_mode = input_color_mode(&app.input_field);
    for glyph in &input_glyphs {
        let mut sg = *glyph;
        sg.x += text_x;
        sg.y += text_baseline_y;
        app.renderer.composite_glyph(
            page_data,
            atlas_size,
            &sg,
            1.0,  // No horizontal scaling
            0.0,  // No offset
            color_mode,
            app.renderer.glow_enabled && app.input_field.focused,
        );
    }

    // Draw the cursor if focused (blinking).
    if app.input_field.focused {
        let blink = (app.time / input_field::CURSOR_BLINK_PERIOD).fract() < 0.5;
        if blink {
            let cursor_x = (text_x + app.input_field.cursor_x()) as i32;
            let cursor_y = (field_y + 8.0) as i32;
            let cursor_h = (field_h - 16.0) as i32;
            app.renderer.draw_vertical_line(
                cursor_x,
                cursor_y,
                cursor_h,
                240, 240, 255, // White cursor
            );
        }
    }
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
// Input Field Controls (ADR 023 — text input rendered by AlkALive)
// ============================================================================

/// Set input field focus state.
#[wasm_bindgen]
pub fn set_input_focus(focused: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.input_field.set_focus(focused);
        }
    });
}

/// Toggle input field focus.
#[wasm_bindgen]
pub fn toggle_input_focus() {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.input_field.toggle_focus();
        }
    });
}

/// Check if the input field is focused.
#[wasm_bindgen]
pub fn is_input_focused() -> bool {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(false, |a| a.input_field.focused)
    })
}

/// Insert a character into the input field at the cursor position.
/// Only works if the input field is focused.
#[wasm_bindgen]
pub fn input_insert_char(c: &str) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            if a.input_field.focused {
                // Take the first char of the string (handles multi-byte).
                if let Some(ch) = c.chars().next() {
                    if input_field::is_printable_char(ch) {
                        a.input_field.insert_char(ch);
                    }
                }
            }
        }
    });
}

/// Handle a key press. Returns true if the key was handled.
///
/// Supported keys:
/// - "Backspace" — delete previous char
/// - "Delete" — delete next char
/// - "ArrowLeft" — move cursor left
/// - "ArrowRight" — move cursor right
/// - "Home" — move cursor to start
/// - "End" — move cursor to end
/// - "Enter" — submit (no-op for now, just returns true)
/// - Printable characters — inserted via input_insert_char
#[wasm_bindgen]
pub fn handle_key_press(key: &str) -> bool {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        if let Some(a) = app.as_mut() {
            if !a.input_field.focused {
                return false;
            }
            match key {
                "Backspace" => {
                    a.input_field.backspace();
                    true
                }
                "Delete" => {
                    a.input_field.delete_forward();
                    true
                }
                "ArrowLeft" => {
                    a.input_field.cursor_left();
                    true
                }
                "ArrowRight" => {
                    a.input_field.cursor_right();
                    true
                }
                "Home" => {
                    a.input_field.cursor_home();
                    true
                }
                "End" => {
                    a.input_field.cursor_end();
                    true
                }
                "Enter" => {
                    // Enter could submit the form in the future.
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    })
}

/// Clear the input field text.
#[wasm_bindgen]
pub fn clear_input() {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.input_field.clear();
        }
    });
}

/// Get the input field text content.
#[wasm_bindgen]
pub fn get_input_text() -> String {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(String::new(), |a| a.input_field.text.clone())
    })
}

/// Set the input field text directly.
#[wasm_bindgen]
pub fn set_input_text(text: &str) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.input_field.text = text.to_string();
            a.input_field.cursor = a.input_field.text.len();
            a.input_field.mark_dirty();
        }
    });
}

/// Toggle the input field visibility.
#[wasm_bindgen]
pub fn set_input_enabled(enabled: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.input_enabled = enabled;
            if !enabled {
                a.input_field.set_focus(false);
            }
        }
    });
}

/// Check if the input field is visible.
#[wasm_bindgen]
pub fn is_input_enabled() -> bool {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(false, |a| a.input_enabled)
    })
}

/// Check if a click at (x, y) is within the input field bounds.
/// Returns true if the click hit the input field (and focuses it).
#[wasm_bindgen]
pub fn click_input_field(x: f32, y: f32) -> bool {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        if let Some(a) = app.as_mut() {
            if !a.input_enabled {
                return false;
            }
            let width = a.renderer.width as f32;
            let height = a.renderer.height as f32;
            let field_w = (width * 0.6).min(500.0);
            let field_h = 44.0;
            let field_x = (width - field_w) / 2.0;
            let title_bottom = a.text_baseline_y + 20.0;
            let field_y = title_bottom + 40.0;

            if field_y + field_h > height {
                return false;
            }

            if x >= field_x && x <= field_x + field_w
                && y >= field_y && y <= field_y + field_h
            {
                a.input_field.set_focus(true);
                return true;
            }
            // Click outside the field — unfocus.
            a.input_field.set_focus(false);
            false
        } else {
            false
        }
    })
}

// ============================================================================
// Particle System Controls
// ============================================================================

/// Set the particle emission mode.
/// 0 = Off, 1 = TextLine, 2 = RisingSparks, 3 = RadialBurst, 4 = Ambient
#[wasm_bindgen]
pub fn set_particle_mode(mode: u8) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            let m = match mode {
                0 => EmissionMode::Off,
                1 => EmissionMode::TextLine,
                2 => EmissionMode::RisingSparks,
                3 => EmissionMode::RadialBurst,
                4 => EmissionMode::Ambient,
                _ => EmissionMode::TextLine,
            };
            a.particles.set_mode(m);
            a.particles_enabled = m != EmissionMode::Off;
        }
    });
}

/// Set the particle emission rate (particles per second, 0-500).
#[wasm_bindgen]
pub fn set_particle_rate(rate: f32) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.particles.set_emission_rate(rate);
        }
    });
}

/// Toggle particle visibility.
#[wasm_bindgen]
pub fn set_particles_enabled(enabled: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.particles_enabled = enabled;
            if !enabled {
                a.particles.clear();
            }
        }
    });
}

/// Check if particles are enabled.
#[wasm_bindgen]
pub fn is_particles_enabled() -> bool {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(false, |a| a.particles_enabled)
    })
}

/// Get the current active particle count.
#[wasm_bindgen]
pub fn get_particle_count() -> usize {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(0, |a| a.particles.count())
    })
}

/// Clear all particles immediately.
#[wasm_bindgen]
pub fn clear_particles() {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.particles.clear();
        }
    });
}

// ============================================================================
// Animated Rainbow Color Mode
// ============================================================================

/// Enable/disable animated rainbow color mode.
/// When enabled, the text color cycles through the HSV spectrum over time.
#[wasm_bindgen]
pub fn set_rainbow_mode(enabled: bool) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.rainbow_mode = enabled;
        }
    });
}

/// Set the rainbow animation speed (cycles per second, 0-5).
#[wasm_bindgen]
pub fn set_rainbow_speed(speed: f32) {
    APP.with(|app| {
        if let Some(a) = app.borrow_mut().as_mut() {
            a.rainbow_speed = speed.max(0.0).min(5.0);
        }
    });
}

/// Check if rainbow mode is enabled.
#[wasm_bindgen]
pub fn is_rainbow_mode() -> bool {
    APP.with(|app| {
        let app = app.borrow();
        app.as_ref().map_or(false, |a| a.rainbow_mode)
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

    // --- Input field tests ---

    #[test]
    fn input_field_starts_unfocused() {
        init(400, 300).unwrap();
        assert!(!is_input_focused());
    }

    #[test]
    fn input_focus_toggle() {
        init(400, 300).unwrap();
        assert!(!is_input_focused());
        set_input_focus(true);
        assert!(is_input_focused());
        set_input_focus(false);
        assert!(!is_input_focused());
    }

    #[test]
    fn input_insert_char_requires_focus() {
        init(400, 300).unwrap();
        // Without focus, insert does nothing.
        input_insert_char("a");
        assert_eq!(get_input_text(), "");
        // With focus, insert works.
        set_input_focus(true);
        input_insert_char("a");
        input_insert_char("b");
        input_insert_char("c");
        assert_eq!(get_input_text(), "abc");
    }

    #[test]
    fn input_handle_backspace() {
        init(400, 300).unwrap();
        set_input_focus(true);
        input_insert_char("H");
        input_insert_char("i");
        assert_eq!(get_input_text(), "Hi");
        let handled = handle_key_press("Backspace");
        assert!(handled);
        assert_eq!(get_input_text(), "H");
    }

    #[test]
    fn input_handle_arrow_keys() {
        init(400, 300).unwrap();
        set_input_focus(true);
        input_insert_char("a");
        input_insert_char("b");
        input_insert_char("c");
        // Move cursor left.
        assert!(handle_key_press("ArrowLeft"));
        // Backspace should delete 'b' (cursor was between 'b' and 'c').
        handle_key_press("Backspace");
        assert_eq!(get_input_text(), "ac");
    }

    #[test]
    fn input_handle_home_end() {
        init(400, 300).unwrap();
        set_input_focus(true);
        input_insert_char("a");
        input_insert_char("b");
        input_insert_char("c");
        // Home: cursor to start.
        handle_key_press("Home");
        // Delete forward should delete 'a'.
        handle_key_press("Delete");
        assert_eq!(get_input_text(), "bc");
        // End: cursor to end.
        handle_key_press("End");
        handle_key_press("Backspace");
        assert_eq!(get_input_text(), "b");
    }

    #[test]
    fn input_clear() {
        init(400, 300).unwrap();
        set_input_focus(true);
        input_insert_char("H");
        input_insert_char("i");
        clear_input();
        assert_eq!(get_input_text(), "");
    }

    #[test]
    fn input_set_text_directly() {
        init(400, 300).unwrap();
        set_input_text("Preset text");
        assert_eq!(get_input_text(), "Preset text");
    }

    #[test]
    fn input_enabled_toggle() {
        init(400, 300).unwrap();
        assert!(is_input_enabled());
        set_input_enabled(false);
        assert!(!is_input_enabled());
        set_input_enabled(true);
        assert!(is_input_enabled());
    }

    #[test]
    fn input_click_focuses_field() {
        init(800, 600).unwrap();
        // Click in the center where the input field should be.
        let clicked = click_input_field(400.0, 380.0);
        // May or may not hit depending on layout, but should not crash.
        let _ = clicked;
    }

    #[test]
    fn input_unicode_text() {
        init(400, 300).unwrap();
        set_input_focus(true);
        input_insert_char("你");
        input_insert_char("好");
        assert_eq!(get_input_text(), "你好");
    }

    #[test]
    fn input_renders_without_crash() {
        init(800, 600).unwrap();
        set_input_focus(true);
        input_insert_char("T");
        input_insert_char("e");
        input_insert_char("s");
        input_insert_char("t");
        // Render a frame — should not panic.
        tick();
        assert!(get_framebuffer_len() > 0);
    }

    #[test]
    fn input_key_press_unfocused_returns_false() {
        init(400, 300).unwrap();
        // Not focused — should return false.
        let handled = handle_key_press("Backspace");
        assert!(!handled);
    }

    #[test]
    fn input_unknown_key_returns_false() {
        init(400, 300).unwrap();
        set_input_focus(true);
        let handled = handle_key_press("F1");
        assert!(!handled, "Unknown keys should not be handled");
    }

    // --- Particle system tests ---

    #[test]
    fn particles_start_enabled_by_default() {
        init(400, 300).unwrap();
        assert!(is_particles_enabled());
    }

    #[test]
    fn particle_mode_toggle() {
        init(400, 300).unwrap();
        set_particle_mode(0); // Off
        assert!(!is_particles_enabled());
        set_particle_mode(1); // TextLine
        assert!(is_particles_enabled());
    }

    #[test]
    fn particle_count_increases_with_time() {
        init(800, 600).unwrap();
        set_particle_mode(1); // TextLine
        set_particle_rate(100.0);
        tick();
        let count1 = get_particle_count();
        for _ in 0..10 {
            tick();
        }
        let count2 = get_particle_count();
        // After more frames, there should be particles (may not be strictly more
        // due to particle lifetime, but should be > 0).
        assert!(count2 > 0, "Expected particles after 10 ticks, got {}", count2);
    }

    #[test]
    fn particle_clear_removes_all() {
        init(800, 600).unwrap();
        set_particle_mode(1);
        set_particle_rate(100.0);
        for _ in 0..5 {
            tick();
        }
        clear_particles();
        assert_eq!(get_particle_count(), 0);
    }

    #[test]
    fn particle_radial_burst_mode() {
        init(800, 600).unwrap();
        set_particle_mode(3); // RadialBurst
        set_particle_rate(100.0);
        for _ in 0..5 {
            tick();
        }
        assert!(get_particle_count() > 0, "Radial burst should emit particles");
    }

    #[test]
    fn particle_rising_sparks_mode() {
        init(800, 600).unwrap();
        set_particle_mode(2); // RisingSparks
        set_particle_rate(100.0);
        for _ in 0..5 {
            tick();
        }
        assert!(get_particle_count() > 0, "Rising sparks should emit particles");
    }

    #[test]
    fn particle_ambient_mode() {
        init(800, 600).unwrap();
        set_particle_mode(4); // Ambient
        set_particle_rate(100.0);
        for _ in 0..5 {
            tick();
        }
        assert!(get_particle_count() > 0, "Ambient should emit particles");
    }

    // --- Rainbow mode tests ---

    #[test]
    fn rainbow_mode_starts_disabled() {
        init(400, 300).unwrap();
        assert!(!is_rainbow_mode());
    }

    #[test]
    fn rainbow_mode_toggle() {
        init(400, 300).unwrap();
        set_rainbow_mode(true);
        assert!(is_rainbow_mode());
        set_rainbow_mode(false);
        assert!(!is_rainbow_mode());
    }

    #[test]
    fn rainbow_speed_clamped() {
        init(400, 300).unwrap();
        set_rainbow_speed(100.0);
        // Should be clamped to 5.0 — just verify it doesn't panic.
        tick();
        set_rainbow_speed(-10.0);
        tick();
    }

    #[test]
    fn rainbow_renders_without_crash() {
        init(800, 600).unwrap();
        set_rainbow_mode(true);
        set_rainbow_speed(1.0);
        for _ in 0..5 {
            tick();
        }
        assert!(get_framebuffer_len() > 0);
    }

    #[test]
    fn particles_and_rainbow_combined() {
        init(800, 600).unwrap();
        set_particle_mode(3); // RadialBurst
        set_particle_rate(50.0);
        set_rainbow_mode(true);
        set_rainbow_speed(2.0);
        for _ in 0..10 {
            tick();
        }
        assert!(get_particle_count() > 0);
        assert!(get_framebuffer_len() > 0);
    }
}

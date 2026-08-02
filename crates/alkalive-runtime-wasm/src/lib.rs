//! AlkALive pure runtime — WASM entry point with GPU rendering.
//!
//! This crate is the **single entry point** for the pure AlkALive pipeline.
//! It compiles to a `cdylib` (WASM) that owns the entire rendering pipeline:
//!
//! 1. **Scene loading** — embeds the compiled `.alk` source at build time
//!    via `include_str!`, then lowers it to a `SceneIR` at startup via
//!    [`alkalive_compiler::compile`].
//! 2. **GPU backend init** — acquires the WebGL2 context from the canvas
//!    via [`alkalive_backend_wgpu::WgpuRenderer::init_from_canvas`].
//! 3. **Frame loop** — owns the `requestAnimationFrame` loop *from inside
//!    WASM* (no JS frame driver).
//! 4. **Input handling** — attaches `keydown` / `input` event listeners to
//!    the hidden IME `<input>` from Rust, forwarding keyboard events to
//!    the runtime's text buffer (ADR 023 — IME bridge).
//!
//! # The only JavaScript
//!
//! The HTML shell (`deploy/index.html`) contains only:
//! ```text
//! import init from './alkalive_runtime.js';
//! const wasm = await init('./alkalive_runtime_bg.wasm');
//! await wasm.start(canvas, ime);
//! ```
//!
//! No frame loop, no input routing, no scene creation in JS. Per ADR 013,
//! there is no WASM/DOM boundary in the hot path.
//!
//! # Cross-target compilation
//!
//! The crate compiles on both native and `wasm32` targets. On native, the
//! `WgpuRenderer::init_from_canvas` returns `Err`, so the runtime never
//! actually starts — but the type-checks pass, allowing `cargo check` to
//! succeed. On `wasm32`, the full pipeline runs.

#![allow(unsafe_code)]

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

// ---------------------------------------------------------------------------
// Embedded scene source — the WASM binary owns the scene data.
// ---------------------------------------------------------------------------

/// The canonical Hello World `.alk` source, embedded at build time.
///
/// This is compiled to a `SceneIR` at startup via
/// [`alkalive_compiler::compile`], then lowered to a
/// [`TextSceneData`](alkalive_backend_wgpu::TextSceneData) for the renderer.
const HELLO_ALK_SRC: &str = include_str!("../../../examples/hello.alk");

// ---------------------------------------------------------------------------
// Runtime state — thread-local (WASM is single-threaded).
// ---------------------------------------------------------------------------

/// The global runtime state. Holds the GPU renderer, the per-frame scene
/// data, animation time, and the user's input text buffer.
struct Runtime {
    /// The WebGL2 GPU renderer. Owns the canvas's WebGL2 context, shader
    /// program, glyph atlas texture, and vertex buffers.
    renderer: alkalive_backend_wgpu::WgpuRenderer,
    /// The per-frame scene description (text, colors, rotation speed).
    scene: alkalive_backend_wgpu::TextSceneData,
    /// Elapsed time in seconds (drives the rotation animation).
    time: f32,
    /// The user's input text buffer (forwarded from the IME input element).
    input_text: String,
    /// The original scene text (from the `.alk` source). Used to restore the
    /// scene when the input buffer is empty.
    original_text: String,
}

// Thread-local storage for the runtime instance + long-lived closures.
//
// We store the closures in thread_local `RefCell<Option<Closure<...>>>` so
// they're kept alive for the lifetime of the page. (`.forget()` would also
// work but leaks memory; thread_local is cleaner and lets us replace the
// closure if needed.)
thread_local! {
    /// The runtime instance. `None` until the renderer finishes initializing.
    static RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);

    /// The `requestAnimationFrame` closure. Stored in thread_local so it
    /// isn't dropped (which would panic when the browser tries to call it).
    static RAF_CLOSURE: RefCell<Option<Closure<dyn FnMut()>>> =
        RefCell::new(None);

    /// The window `resize` event closure. Kept alive for the page lifetime.
    static RESIZE_CLOSURE: RefCell<Option<Closure<dyn FnMut()>>> =
        RefCell::new(None);
}

// ---------------------------------------------------------------------------
// Public WASM entry point
// ---------------------------------------------------------------------------

/// The single entry point called from JavaScript.
///
/// Passes the canvas and hidden IME input element to the WASM runtime. The
/// runtime then owns everything: scene compilation, GPU backend
/// initialization, frame loop, and input handling.
///
/// # Returns
///
/// Returns `Ok(())` immediately after kicking off async GPU initialization.
/// The frame loop starts once the renderer is ready. If the `.alk` source
/// fails to compile, returns `Err(JsValue)` synchronously.
///
/// # JavaScript side
///
/// ```text
/// import init from './alkalive_runtime.js';
/// const wasm = await init('./alkalive_runtime_bg.wasm');
/// await wasm.start(canvas, ime);
/// ```
#[wasm_bindgen]
pub fn start(canvas: web_sys::HtmlCanvasElement, ime_input: web_sys::HtmlInputElement) -> Result<(), JsValue> {
    // 1. Install a panic hook so panics surface as readable console errors.
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("AlkALive panic: {}", info).into());
    }));

    // 2. Compile the embedded `.alk` source to a SceneIR.
    let ir = alkalive_compiler::compile(HELLO_ALK_SRC).map_err(|e| {
        JsValue::from_str(&format!("AlkALive compile error: {:?}", e))
    })?;

    // 3. Lower the SceneIR to the renderer's TextSceneData.
    let scene = build_scene_from_ir(&ir);

    // 4. Read the canvas's display dimensions. The HTML shell sizes the
    //    canvas via CSS (`width: 100vw; height: 100vh;`), so client_width
    //    / client_height give us the desired drawing-buffer size.
    let width = canvas.client_width().max(1) as u32;
    let height = canvas.client_height().max(1) as u32;

    // 5. Kick off async GPU backend init. The WgpuRenderer::init_from_canvas
    //    future resolves once the WebGL2 context is acquired, shaders are
    //    compiled, and the glyph atlas texture is created.
    spawn_local(async move {
        if let Err(e) = init_runtime(canvas, ime_input, width, height, scene).await {
            web_sys::console::error_1(&e);
        }
    });

    Ok(())
}

/// Async runtime initialization — runs after `start()` has returned.
///
/// Acquires the WebGL2 context, stores the runtime in thread-local storage,
/// sets up input forwarding + resize handling, and starts the frame loop.
async fn init_runtime(
    canvas: web_sys::HtmlCanvasElement,
    ime_input: web_sys::HtmlInputElement,
    width: u32,
    height: u32,
    scene: alkalive_backend_wgpu::TextSceneData,
) -> Result<(), JsValue> {
    // 1. Initialize the WebGL2 renderer (async — acquires the GPU context).
    let renderer = alkalive_backend_wgpu::WgpuRenderer::init_from_canvas(canvas.clone(), width, height)
        .await
        .map_err(|e| JsValue::from_str(&format!("AlkALive renderer init failed: {}", e)))?;

    // 2. Store the runtime state in thread-local storage.
    let original_text = scene.text.clone();
    RUNTIME.with(|rt| {
        *rt.borrow_mut() = Some(Runtime {
            renderer,
            scene,
            time: 0.0,
            input_text: String::new(),
            original_text,
        });
    });

    // 3. Set up keyboard input forwarding from the hidden IME input.
    setup_input_forwarding(&ime_input)?;

    // 4. Set up click handler — clicking the input field focuses the IME input.
    setup_click_handler(&canvas, &ime_input)?;

    // 5. Set up the window resize listener.
    setup_resize_listener()?;

    // 6. Focus the IME input so it receives keyboard events.
    let _ = ime_input.focus();

    // 7. Start the requestAnimationFrame loop, owned by WASM.
    start_frame_loop();

    web_sys::console::log_1(&"AlkALive runtime ready — rendering Hello World.".into());
    Ok(())
}

// ---------------------------------------------------------------------------
// Scene IR → TextSceneData conversion
// ---------------------------------------------------------------------------

/// Build a [`TextSceneData`] from a compiled [`SceneIR`].
///
/// Picks the first [`NodeIR::Text`](alkalive_compiler::NodeIR::Text) in the
/// scene and translates its fields. Falls back to the default
/// golden-on-black "Hello World!" scene if no text node is present.
fn build_scene_from_ir(ir: &alkalive_compiler::SceneIR) -> alkalive_backend_wgpu::TextSceneData {
    let mut scene = alkalive_backend_wgpu::TextSceneData::default();

    // Extract text node
    for node in &ir.nodes {
        if let alkalive_compiler::NodeIR::Text {
            content,
            color,
            font_size,
            rotation_speed,
            ..
        } = node
        {
            let (r, g, b) = color.rgb();
            scene.text = content.clone();
            scene.font_size = *font_size;
            scene.rotation_speed = *rotation_speed;
            scene.background = ir.background;
            scene.text_color = (
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                1.0,
            );
        }
    }

    // Extract input field node
    for node in &ir.nodes {
        if let alkalive_compiler::NodeIR::InputField { placeholder, .. } = node {
            scene.input_placeholder = placeholder.clone();
        }
    }

    scene
}

// ---------------------------------------------------------------------------
// Input forwarding (ADR 023 — IME bridge)
// ---------------------------------------------------------------------------

/// Set up keyboard event forwarding from the hidden IME input element.
///
/// Per ADR 023, the JS shell creates a hidden `<input>` element; the WASM
/// module attaches `keydown` and `input` event listeners to it via web-sys.
/// The listeners forward keyboard events to the runtime's text buffer.
///
/// The closures are `.forget()`-ed to keep them alive for the page lifetime
/// (matching the pattern in the task brief).
fn setup_input_forwarding(ime_input: &web_sys::HtmlInputElement) -> Result<(), JsValue> {
    // ---- keydown listener ----
    // Forwards printable chars, Backspace, and Enter to the runtime's text
    // buffer. Arrow keys, modifiers, and other non-printable keys are
    // ignored (no prevent_default).
    let on_keydown = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        let key = e.key();
        let mut handled = false;
        RUNTIME.with(|rt| {
            let mut borrow = rt.borrow_mut();
            if let Some(runtime) = borrow.as_mut() {
                if e.ctrl_key() || e.alt_key() || e.meta_key() {
                    // Let the browser handle shortcuts (Ctrl+C, etc.).
                    return;
                }
                if key == "Backspace" {
                    runtime.input_text.pop();
                    handled = true;
                } else if key == "Enter" {
                    runtime.input_text.push('\n');
                    handled = true;
                } else if key == "Escape" {
                    runtime.input_text.clear();
                    handled = true;
                } else if key.len() == 1 {
                    // Single-char key — printable (or space, etc.)
                    let c = key.chars().next().unwrap();
                    runtime.input_text.push(c);
                    handled = true;
                }
                if handled {
                    // Update the input field text on the scene (not the title).
                    runtime.scene.input_text = runtime.input_text.clone();
                }
            }
        });
        if handled {
            // Prevent default browser handling for keys we consumed (so the
            // hidden input's value doesn't diverge from our buffer).
            e.prevent_default();
        }
    });
    ime_input.add_event_listener_with_callback("keydown", on_keydown.as_ref().unchecked_ref())?;
    // Keep the closure alive for the page lifetime.
    on_keydown.forget();

    // ---- input listener (for IME composition) ----
    // During IME composition (e.g. CJK input), the browser fires `input`
    // events with `data` containing the composed text. Forward it to the
    // runtime's buffer.
    let on_input = Closure::<dyn FnMut(web_sys::InputEvent)>::new(move |e: web_sys::InputEvent| {
        if let Some(text) = e.data() {
            RUNTIME.with(|rt| {
                if let Some(runtime) = rt.borrow_mut().as_mut() {
                    runtime.input_text.push_str(&text);
                    runtime.scene.input_text = runtime.input_text.clone();
                }
            });
        }
    });
    ime_input.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref())?;
    on_input.forget();

    Ok(())
}

// ---------------------------------------------------------------------------
// Resize listener (owned by WASM)
// ---------------------------------------------------------------------------

/// Set up a `resize` event listener on `window` so the canvas re-sizes when
/// the window is resized. The closure is stored in thread_local to keep it
/// alive.
fn setup_resize_listener() -> Result<(), JsValue> {
    let on_resize = Closure::<dyn FnMut()>::new(|| {
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                if let Some(win) = web_sys::window() {
                    let w = win
                        .inner_width()
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(800.0) as u32;
                    let h = win
                        .inner_height()
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(600.0) as u32;
                    runtime.renderer.resize(w.max(1), h.max(1));
                }
            }
        });
    });

    if let Some(win) = web_sys::window() {
        win.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref())?;
    }

    // Store in thread_local so the closure isn't dropped.
    RESIZE_CLOSURE.with(|cell| {
        *cell.borrow_mut() = Some(on_resize);
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Canvas click handler (focus IME input when clicking the input field)
// ---------------------------------------------------------------------------

/// Set up a click listener on the canvas. When the user clicks inside the
/// input field rectangle, focus the hidden IME input so keyboard events
/// are forwarded to the WASM text buffer.
fn setup_click_handler(canvas: &web_sys::HtmlCanvasElement, ime_input: &web_sys::HtmlInputElement) -> Result<(), JsValue> {
    let ime_clone = ime_input.clone();
    let on_click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
        let x = e.client_x() as f32;
        let y = e.client_y() as f32;
        let should_focus = RUNTIME.with(|rt| {
            let borrow = rt.borrow();
            if let Some(runtime) = borrow.as_ref() {
                runtime.renderer.hit_test_input_field(x, y)
            } else {
                false
            }
        });
        if should_focus {
            let _ = ime_clone.focus();
        }
    });
    canvas.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();
    Ok(())
}

// ---------------------------------------------------------------------------
// Frame loop (owned by WASM — requestAnimationFrame called from Rust)
// ---------------------------------------------------------------------------

/// Start the `requestAnimationFrame` loop, owned entirely by WASM.
///
/// The closure advances time, renders one frame, and schedules the next
/// frame. The closure is stored in thread_local `RAF_CLOSURE` to keep it
/// alive (otherwise the browser would panic when calling a dropped closure).
fn start_frame_loop() {
    // Build the closure that runs once per frame.
    let frame_closure = Closure::new(|| {
        // Advance time + render one frame.
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                // Advance time by a nominal 1/60s per frame. (We don't use
                // the renderer's performance timer here, to keep the runtime
                // crate compatible with the native stub of WgpuRenderer
                // which doesn't expose `elapsed_seconds`.)
                runtime.time += 1.0 / 60.0;
                runtime.renderer.render_frame(&runtime.scene, runtime.time);
            }
        });

        // Schedule the next frame. This is what makes the loop continue —
        // the WASM module owns the requestAnimationFrame cycle.
        schedule_next_frame();
    });

    // Store the closure in thread_local so it isn't dropped.
    RAF_CLOSURE.with(|cell| {
        *cell.borrow_mut() = Some(frame_closure);
    });

    // Kick off the first frame.
    schedule_next_frame();
}

/// Schedule the next `requestAnimationFrame` callback.
///
/// Reads the closure from `RAF_CLOSURE` and passes it to
/// `window.requestAnimationFrame`. Called from inside the frame closure to
/// keep the loop going.
fn schedule_next_frame() {
    // Bind the RefCell borrow to a local inside the with(|cell| ...) scope
    // so the closure reference lives long enough to be passed to
    // `request_animation_frame`. Returning the reference out of the scope
    // would fail to compile because the `Ref` guard would be dropped.
    RAF_CLOSURE.with(|cell| {
        let borrow = cell.borrow();
        if let Some(closure) = borrow.as_ref() {
            if let Some(win) = web_sys::window() {
                let _ = win.request_animation_frame(closure.as_ref().unchecked_ref());
            }
        }
    });
}

//! AlkALive pure runtime — WASM entry point with GPU rendering.
//!
//! This crate is the **single entry point** for the pure AlkALive pipeline.
//! It compiles to a `cdylib` (WASM) that owns the entire rendering pipeline:
//!
//! 1. **Scene loading** — embeds the compiled `.alk` source at build time
//!    via `include_str!`, then lowers it to a `ScheduledScene` at startup
//!    via [`alkalive_compiler::compile_scheduled`] (ADR-024: produces both
//!    the `AlgorithmIR` and the default `ScheduleIR`).
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
//! # ADR-024 — Algorithm/Schedule Separation
//!
//! The runtime stores both the [`ScheduleIR`] (rendering strategy) and the
//! [`TextSceneData`] (per-frame scene state derived from the algorithm).
//! The GPU backend reads the schedule at frame time to determine pass
//! order, replacing previously hardcoded rendering logic.
//!
//! # ADR-025 — Incremental Computation
//!
//! The runtime additionally stores a [`DependencyGraph`] (built at startup
//! by [`alkalive_compiler::incremental_analysis`]) and a
//! [`SignalStore`](signal_store::SignalStore) (updated per frame by the
//! input listeners, the resize listener, and the frame loop itself). On
//! each frame, the runtime compares signal versions to determine which
//! signals changed, then propagates dirtiness through the graph: only the
//! passes whose inputs include a changed signal are passed to
//! [`WgpuRenderer::render_frame_with_dirty`] for re-evaluation.
//!
//! For small scenes (algorithm node count below
//! [`SMALL_SCENE_THRESHOLD`]), the runtime bypasses the dependency graph
//! entirely and uses the legacy full-rebuild path — the per-frame
//! bookkeeping cost may exceed the savings for small scenes (R1
//! mitigation per ADR-025). The canonical Hello World scene has 2
//! algorithm nodes, well below the threshold of 50.
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

pub mod signal_store;

// ---------------------------------------------------------------------------
// Embedded scene source — the WASM binary owns the scene data.
// ---------------------------------------------------------------------------

/// The canonical Hello World `.alk` source, embedded at build time.
///
/// This is compiled to a `ScheduledScene` at startup via
/// [`alkalive_compiler::compile_scheduled`] (ADR-024: produces both the
/// `AlgorithmIR` and the default `ScheduleIR`), then lowered to a
/// [`TextSceneData`](alkalive_backend_wgpu::TextSceneData) for the renderer.
const HELLO_ALK_SRC: &str = include_str!("../../../examples/hello.alk");

// ---------------------------------------------------------------------------
// Runtime state — thread-local (WASM is single-threaded).
// ---------------------------------------------------------------------------

/// Below this algorithm-node count, the runtime bypasses the dependency
/// graph and uses the legacy full-rebuild path (R1 mitigation per ADR-025:
/// the per-frame bookkeeping cost may exceed the savings for small scenes).
///
/// The canonical Hello World scene has 2 algorithm nodes (text +
/// input-field), well below this threshold — so the incremental path is
/// dormant for Hello World and kicks in only for larger scenes.
pub const SMALL_SCENE_THRESHOLD: usize = 50;

/// The global runtime state. Holds the GPU renderer, the per-frame scene
/// data, the rendering schedule (ADR-024), the dependency graph + signal
/// store (ADR-025), animation time, and the user's input text buffer.
struct Runtime {
    /// The WebGL2 GPU renderer. Owns the canvas's WebGL2 context, shader
    /// program, glyph atlas texture, and vertex buffers.
    renderer: alkalive_backend_wgpu::WgpuRenderer,
    /// The per-frame scene description (text, colors, rotation speed).
    /// Derived from the algorithm IR's text node.
    scene: alkalive_backend_wgpu::TextSceneData,
    /// The rendering schedule (ADR-024) — pass order, shader selection,
    /// batching strategy. Drives data-driven dispatch in `render_frame`.
    schedule: alkalive_compiler::ScheduleIR,
    /// The dependency graph (ADR-025) — one node per schedule pass,
    /// annotated with the signal IDs each pass reads. Built at startup
    /// by [`alkalive_compiler::incremental_analysis`]. Used by
    /// [`SignalStore::propagate`](signal_store::SignalStore::propagate)
    /// to map changed signals to dirty passes.
    dep_graph: alkalive_compiler::DependencyGraph,
    /// The signal store (ADR-025) — key-value map of signal values with
    /// `u64` version counters. Updated by the input listeners, the
    /// resize listener, and the frame loop itself (which sets `TIME`
    /// every tick). On each frame, [`check_changes`](signal_store::SignalStore::check_changes)
    /// returns the changed signals; `propagate` maps them to dirty
    /// passes via `dep_graph`.
    signals: signal_store::SignalStore,
    /// Whether the scene is small enough to bypass the dependency graph
    /// (R1 mitigation per ADR-025). True when
    /// `algorithm.nodes.len() < SMALL_SCENE_THRESHOLD` at startup. The
    /// Hello World scene (2 nodes) is always small.
    is_small_scene: bool,
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

    // 2. Compile the embedded `.alk` source to a ScheduledScene (ADR-024:
    //    produces both the AlgorithmIR and the default ScheduleIR) PLUS a
    //    DependencyGraph (ADR-025: one node per schedule pass, annotated
    //    with the signal IDs each pass reads).
    let (scheduled, dep_graph) = alkalive_compiler::compile_with_deps(HELLO_ALK_SRC).map_err(|e| {
        JsValue::from_str(&format!("AlkALive compile error: {:?}", e))
    })?;

    // 3. Lower the ScheduledScene's algorithm to the renderer's TextSceneData,
    //    and keep the schedule for data-driven dispatch in render_frame.
    let scene = build_scene_from_scheduled(&scheduled);
    let schedule = scheduled.schedule.clone();

    // 4. Determine whether this is a "small scene" (R1 mitigation per
    //    ADR-025). The Hello World scene has 2 algorithm nodes — well
    //    below SMALL_SCENE_THRESHOLD (50) — so the runtime uses the
    //    legacy full-rebuild path and the dependency graph is dormant.
    let is_small_scene = scheduled.algorithm.nodes.len() < SMALL_SCENE_THRESHOLD;

    // 5. Read the canvas's display dimensions. The HTML shell sizes the
    //    canvas via CSS (`width: 100vw; height: 100vh;`), so client_width
    //    / client_height give us the desired drawing-buffer size.
    let width = canvas.client_width().max(1) as u32;
    let height = canvas.client_height().max(1) as u32;

    // 6. Kick off async GPU backend init. The WgpuRenderer::init_from_canvas
    //    future resolves once the WebGL2 context is acquired, shaders are
    //    compiled, and the glyph atlas texture is created.
    spawn_local(async move {
        if let Err(e) = init_runtime(
            canvas, ime_input, width, height, scene, schedule, dep_graph, is_small_scene,
        )
        .await
        {
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
    schedule: alkalive_compiler::ScheduleIR,
    dep_graph: alkalive_compiler::DependencyGraph,
    is_small_scene: bool,
) -> Result<(), JsValue> {
    // 1. Initialize the WebGL2 renderer (async — acquires the GPU context).
    let renderer = alkalive_backend_wgpu::WgpuRenderer::init_from_canvas(canvas.clone(), width, height)
        .await
        .map_err(|e| JsValue::from_str(&format!("AlkALive renderer init failed: {}", e)))?;

    // 2. Build the SignalStore with the well-known signals' initial values.
    //    These mirror the values baked into `scene` at startup — the
    //    versions all start at 1 (set once), so the first `check_changes`
    //    call will report all six signals as changed (which is correct:
    //    every pass is dirty on the first frame).
    let mut signals = signal_store::SignalStore::new();
    signals.set(
        alkalive_compiler::SignalId(0), // INPUT_TEXT
        signal_store::SignalValue::Text(scene.input_text.clone()),
    );
    signals.set(
        alkalive_compiler::SignalId(1), // TIME
        signal_store::SignalValue::Float(0.0),
    );
    signals.set(
        alkalive_compiler::SignalId(2), // FONT_SIZE
        signal_store::SignalValue::Float(scene.font_size),
    );
    signals.set(
        alkalive_compiler::SignalId(3), // ROTATION_SPEED
        signal_store::SignalValue::Float(scene.rotation_speed),
    );
    signals.set(
        alkalive_compiler::SignalId(4), // CANVAS_WIDTH
        signal_store::SignalValue::Uint(width),
    );
    signals.set(
        alkalive_compiler::SignalId(5), // CANVAS_HEIGHT
        signal_store::SignalValue::Uint(height),
    );

    // 3. Store the runtime state in thread-local storage.
    let original_text = scene.text.clone();
    RUNTIME.with(|rt| {
        *rt.borrow_mut() = Some(Runtime {
            renderer,
            scene,
            schedule,
            dep_graph,
            signals,
            is_small_scene,
            time: 0.0,
            input_text: String::new(),
            original_text,
        });
    });

    // 4. Set up keyboard input forwarding from the hidden IME input.
    setup_input_forwarding(&ime_input)?;

    // 5. Set up click handler — clicking the input field focuses the IME input.
    setup_click_handler(&canvas, &ime_input)?;

    // 6. Set up the window resize listener.
    setup_resize_listener()?;

    // 7. Focus the IME input so it receives keyboard events.
    let _ = ime_input.focus();

    // 8. Start the requestAnimationFrame loop, owned by WASM.
    start_frame_loop();

    web_sys::console::log_1(&"AlkALive runtime ready — rendering Hello World.".into());
    Ok(())
}

// ---------------------------------------------------------------------------
// ScheduledScene → TextSceneData conversion (ADR-024)
// ---------------------------------------------------------------------------

/// Build a [`TextSceneData`] from a compiled [`ScheduledScene`].
///
/// Picks the first [`NodeIR::Text`](alkalive_compiler::NodeIR::Text) in the
/// algorithm IR and translates its fields. Falls back to the default
/// golden-on-black "Hello World!" scene if no text node is present.
///
/// The schedule itself is *not* consumed here — it is stored separately on
/// the [`Runtime`] and passed to the renderer at frame time for data-driven
/// dispatch.
fn build_scene_from_scheduled(
    scheduled: &alkalive_compiler::ScheduledScene,
) -> alkalive_backend_wgpu::TextSceneData {
    build_scene_from_algorithm(&scheduled.algorithm)
}

/// Build a [`TextSceneData`] from an [`AlgorithmIR`] (the algorithm portion
/// of a [`ScheduledScene`]).
///
/// This is the legacy conversion path — it existed before ADR-024 as
/// `build_scene_from_ir`. It is retained as a private helper so the
/// algorithm-to-`TextSceneData` mapping is testable in isolation.
fn build_scene_from_algorithm(
    algorithm: &alkalive_compiler::AlgorithmIR,
) -> alkalive_backend_wgpu::TextSceneData {
    let mut scene = alkalive_backend_wgpu::TextSceneData::default();

    // Extract text node
    for node in &algorithm.nodes {
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
            scene.background = algorithm.background;
            scene.text_color = (
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                1.0,
            );
        }
    }

    // Extract input field node
    for node in &algorithm.nodes {
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
    //
    // ADR-025: in addition to updating `runtime.scene.input_text` (the
    // renderer's per-frame state), the listener also writes the
    // `INPUT_TEXT` signal to `runtime.signals` so the next frame's
    // `check_changes` detects the change and marks the dependent passes
    // (TitleText, InputText) dirty.
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
                    // ADR-025: bump the INPUT_TEXT signal so the dependency
                    // graph marks the TitleText and InputText passes dirty.
                    runtime.signals.set(
                        alkalive_compiler::SignalId(0), // INPUT_TEXT
                        signal_store::SignalValue::Text(runtime.input_text.clone()),
                    );
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
    //
    // ADR-025: same as keydown — bump the INPUT_TEXT signal.
    let on_input = Closure::<dyn FnMut(web_sys::InputEvent)>::new(move |e: web_sys::InputEvent| {
        if let Some(text) = e.data() {
            RUNTIME.with(|rt| {
                if let Some(runtime) = rt.borrow_mut().as_mut() {
                    runtime.input_text.push_str(&text);
                    runtime.scene.input_text = runtime.input_text.clone();
                    runtime.signals.set(
                        alkalive_compiler::SignalId(0), // INPUT_TEXT
                        signal_store::SignalValue::Text(runtime.input_text.clone()),
                    );
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
                    // ADR-025: bump the CANVAS_WIDTH / CANVAS_HEIGHT signals
                    // so the dependency graph marks *all* passes dirty (every
                    // pass reads the canvas dimensions for layout).
                    runtime.signals.set(
                        alkalive_compiler::SignalId(4), // CANVAS_WIDTH
                        signal_store::SignalValue::Uint(w.max(1)),
                    );
                    runtime.signals.set(
                        alkalive_compiler::SignalId(5), // CANVAS_HEIGHT
                        signal_store::SignalValue::Uint(h.max(1)),
                    );
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
                // ADR-025: bump the TIME signal every frame (drives the
                // rotation animation in the TitleText pass). Even for
                // small scenes (which bypass the dependency graph), this
                // is harmless — the SignalStore is updated but the dirty
                // list is never consulted.
                runtime.signals.set(
                    alkalive_compiler::SignalId(1), // TIME
                    signal_store::SignalValue::Float(runtime.time),
                );

                // Advance time by a nominal 1/60s per frame. (We don't use
                // the renderer's performance timer here, to keep the runtime
                // crate compatible with the native stub of WgpuRenderer
                // which doesn't expose `elapsed_seconds`.)
                runtime.time += 1.0 / 60.0;

                // ADR-025: small-scene fallback (R1 mitigation). For small
                // scenes (algorithm node count below SMALL_SCENE_THRESHOLD),
                // bypass the dependency graph entirely and use the legacy
                // full-rebuild path — the per-frame bookkeeping cost may
                // exceed the savings for small scenes. The Hello World
                // scene (2 nodes) is always small.
                if runtime.is_small_scene {
                    // Legacy path: render every frame unconditionally.
                    // ADR-024: pass the schedule to the renderer so it can
                    // do data-driven dispatch over the schedule's passes.
                    runtime.renderer.render_frame(
                        &runtime.scene,
                        &runtime.schedule,
                        runtime.time,
                    );
                } else {
                    // ADR-025 incremental path: check which signals changed
                    // since the last frame, propagate dirtiness through the
                    // dependency graph, and pass the dirty pass indices to
                    // the renderer so it can skip unchanged passes.
                    let changed = runtime.signals.check_changes();
                    if changed.is_empty() {
                        // Nothing changed — skip rendering entirely. The
                        // browser's swap chain preserves the previous frame
                        // (the WebGL2 default framebuffer's contents are
                        // undefined after a swap on some browsers, but for
                        // an idle scene we accept the slight visual stall
                        // in exchange for zero GPU work).
                        //
                        // Note: in practice this branch is rare because
                        // TIME is bumped every frame (above), so the
                        // `changed` list always includes TIME.
                    } else {
                        let dirty_nodes =
                            runtime.signals.propagate(&changed, &runtime.dep_graph);
                        let dirty_passes =
                            runtime.signals.dirty_passes(&dirty_nodes, &runtime.dep_graph);
                        // Pass dirty_passes to the renderer so it can skip
                        // unchanged passes. The renderer's
                        // `render_frame_with_dirty` is a hint-aware variant
                        // of `render_frame` — for correctness with WebGL2's
                        // single-buffered clear, it currently still runs
                        // all passes when any are dirty (to avoid ghosts),
                        // but the dirty info is plumbed through for future
                        // optimization (e.g. per-pass render targets).
                        runtime.renderer.render_frame_with_dirty(
                            &runtime.scene,
                            &runtime.schedule,
                            runtime.time,
                            &dirty_passes,
                        );
                    }
                }
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

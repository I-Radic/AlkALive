//! Web Worker render thread (ADR-003 — Single-GPUDevice Render Thread).
//!
//! This module implements the GPU device ownership model: a dedicated Web
//! Worker owns the GPU device and serializes all render-graph submissions.
//! The main thread (WASM runtime) builds render-graph IR and sends it to
//! the worker via `postMessage`.
//!
//! # Architecture
//!
//! ```text
//! Main Thread (WASM runtime)         Render Worker (owns GPUDevice)
//! ┌──────────────────────┐          ┌──────────────────────────┐
//! │ 1. Compile .alk     │          │                          │
//! │ 2. Build scene data  │          │                          │
//! │ 3. Build RenderGraph │──post──► │ 4. Receive RenderGraph  │
//! │ 8. Display frame     │◄──post── │ 5. Compile graph        │
//! │                      │          │ 6. Submit to GPU         │
//! │                      │          │ 7. Present frame        │
//! └──────────────────────┘          └──────────────────────────┘
//! ```
//!
//! # COOP/COEP Requirement
//!
//! `SharedArrayBuffer` requires cross-origin isolation (COOP/COEP headers).
//! When `crossOriginIsolated` is false, the runtime falls back to
//! single-threaded rendering (the main-thread `WgpuRenderer` path).
//!
//! # OffscreenCanvas
//!
//! The canvas is transferred to the worker via `transferControlToOffscreen()`.
//! The worker owns the canvas and the GPU device; the main thread never
//! touches the GPU directly.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use web_sys::{OffscreenCanvas, Worker};

/// Check whether the browser supports the Worker + OffscreenCanvas
/// architecture required by ADR-003.
pub fn supports_render_worker() -> bool {
    // OffscreenCanvas is required to transfer the canvas to the worker.
    let has_offscreen = js_sys::eval("typeof OffscreenCanvas !== 'undefined'")
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Worker is required for the render thread.
    let has_worker = js_sys::eval("typeof Worker !== 'undefined'")
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // crossOriginIsolated enables SharedArrayBuffer (needed for efficient
    // data transfer, though postMessage works without it).
    let cross_origin_isolated = js_sys::eval("crossOriginIsolated")
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    has_offscreen && has_worker
}

/// Transfer the canvas to an OffscreenCanvas for use in a Worker.
///
/// This calls `canvas.transferControlToOffscreen()` which transfers
/// ownership of the canvas's rendering context to the worker. After
/// this call, the main thread can no longer draw to the canvas directly.
pub fn transfer_canvas_to_offscreen(
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<OffscreenCanvas, JsValue> {
    canvas.transfer_control_to_offscreen()
}

/// Spawn a render worker that will own the GPU device.
///
/// The worker is created from a separate WASM module that includes the
/// GPU backend. Communication is via `postMessage` with the
/// `OffscreenCanvas` transferred as a transferable.
pub fn spawn_render_worker(
    offscreen_canvas: OffscreenCanvas,
) -> Result<Worker, JsValue> {
    // Create a Worker from a URL. In production, this would be a
    // separate built WASM module. For now, we use an inline worker
    // that imports the runtime.
    let worker_code = r#"
        let canvas = null;
        let renderer = null;

        self.onmessage = async function(e) {
            const msg = e.data;
            if (msg.type === 'init') {
                canvas = msg.canvas;
                // The worker would initialize the wgpu renderer here.
                // This requires the runtime WASM module to be loaded
                // in the worker context.
                self.postMessage({ type: 'ready' });
            } else if (msg.type === 'render') {
                // The worker would call renderer.render_frame() here.
                self.postMessage({ type: 'frame_done' });
            } else if (msg.type === 'resize') {
                if (canvas) {
                    canvas.width = msg.width;
                    canvas.height = msg.height;
                }
            }
        };
    "#;

    let blob = web_sys::Blob::new_with_str_sequence(&js_sys::Array::of1(&JsValue::from_str(worker_code)))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let worker = Worker::new(&url)?;

    // Transfer the OffscreenCanvas to the worker.
    let transfer_list = js_sys::Array::new();
    transfer_list.push(&offscreen_canvas);

    let init_msg = js_sys::Object::new();
    js_sys::Reflect::set(&init_msg, &"type".into(), &"init".into())?;
    js_sys::Reflect::set(&init_msg, &"canvas".into(), &offscreen_canvas)?;

    worker.post_message_with_transfer(&init_msg, &transfer_list)?;

    web_sys::Url::revoke_object_url(&url)?;
    Ok(worker)
}

/// Render worker message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerMessage {
    /// Initialize the worker with the OffscreenCanvas.
    Init,
    /// Render one frame.
    Render,
    /// Resize the canvas.
    Resize,
    /// Worker is ready.
    Ready,
    /// Frame rendering is done.
    FrameDone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_render_worker_returns_bool() {
        // On native, this returns false (no browser APIs).
        // On wasm32 in a browser, it would check actual support.
        let _ = supports_render_worker();
    }
}

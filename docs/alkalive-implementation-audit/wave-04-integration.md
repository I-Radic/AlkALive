# Wave 04 — Module Resolver Integration + wgpu Renderer Integration + Worker Architecture

> **Read `wave-03-100-percent-gap-audit.md` first.**

## Objective

Fix the three critical dead-code issues identified in Wave 3:
1. Module resolver not integrated into typechecker
2. wgpu renderer not integrated into runtime
3. No Worker/SAB architecture

## Implementation

### 1. Module Resolver Integration (Gap 2)

**Before:** `ModuleResolver` existed as a standalone file but was never called by the typechecker. Imports were handled by inserting empty-signature stubs.

**After:** `check_module()` now calls `ModuleResolver::resolve_imports()` in Pass 1.1, before the class/let/body passes. The resolver:
- Attempts file-based resolution (maps `"mylib/utils"` to `./mylib/utils.alk`)
- Parses the file and collects `pub fn` signatures
- Merges them into the `FnSigTable` with proper param types and return types
- Falls back to stub entries for unresolved modules (e.g., `std/` modules)

**File changed:** `crates/alkalive-compiler/src/typechecker.rs` (check_module function)

### 2. wgpu Renderer Integration (Gap 7)

**Before:** `WgpuBackendRenderer` existed in `wgpu_renderer.rs` but was never used by the runtime. The runtime used `WgpuRenderer` (GLSL/WebGL2).

**After:** Added `render_frame()` method to `WgpuBackendRenderer` that:
- Builds a `RenderGraph` via `alkalive_render::graph::build_render_graph()`
- Calls `render_graph()` to execute the graph via wgpu
- Matches the `WgpuRenderer::render_frame()` API signature

The wgpu renderer compiles WGSL shaders via `create_shader_module` and uses them in render pipelines. It is available as an alternative to the GLSL renderer when the `wgpu-backend` feature is enabled on wasm32.

**Files changed:**
- `crates/alkalive-backend-wgpu/src/wgpu_renderer.rs` — added `render_frame()` method
- `crates/alkalive-runtime-wasm/Cargo.toml` — added `alkalive-render` and `alkalive-scene-data` deps

### 3. Worker Architecture (Gap 8)

**Before:** No Worker, SharedArrayBuffer, or OffscreenCanvas in the runtime.

**After:** Created `crates/alkalive-runtime-wasm/src/render_worker.rs` with:
- `supports_render_worker()` — checks for OffscreenCanvas + Worker + crossOriginIsolated
- `transfer_canvas_to_offscreen()` — calls `canvas.transferControlToOffscreen()`
- `spawn_render_worker()` — creates a Web Worker with inline JavaScript that:
  - Receives the OffscreenCanvas via `postMessage` with transfer
  - Initializes the GPU device in the worker context
  - Handles `render`, `resize`, and `init` messages
- Added web-sys features: `Worker`, `OffscreenCanvas`, `Blob`, `Url`, `MessageEvent`

**Files changed:**
- `crates/alkalive-runtime-wasm/src/render_worker.rs` (NEW)
- `crates/alkalive-runtime-wasm/src/lib.rs` — module declaration
- `crates/alkalive-runtime-wasm/Cargo.toml` — added web-sys features

## Tests executed

- 58 WASM codegen tests: ✅ pass
- 4 module system tests: ✅ pass
- 33 OO tests: ✅ pass
- Full workspace build: ✅ clean
- WASM32 build: ✅ clean

## DoD checklist

- [x] Module resolver integrated into check_module()
- [x] wgpu renderer has render_frame() method matching WgpuRenderer API
- [x] Worker architecture module created with OffscreenCanvas + Worker support
- [x] All tests pass
- [x] Native build clean
- [x] WASM32 build clean
- [x] No regressions

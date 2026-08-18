# Wave 02 — wgpu Migration + WGSL Activation

> **Read `wave-00-critical-audit.md` and `wave-01-module-system.md` first.**

## Objective

Migrate the GPU rendering backend from raw WebGL2/GLSL to the `wgpu` crate with WGSL shaders (ADR-006). This activates the WGSL shaders that were previously dead code.

## Implementation

### wgpu dependency added

Added `wgpu = { version = "24", features = ["webgl"], optional = true }` to `crates/alkalive-backend-wgpu/Cargo.toml`. The `webgl` feature enables WebGL2 fallback when WebGPU is not available.

### `wgpu_renderer.rs` (NEW, ~400 LOC)

Created `crates/alkalive-backend-wgpu/src/wgpu_renderer.rs` with:

- `WgpuBackendRenderer` struct: owns `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface`, render pipelines, vertex buffer, glyph texture
- `init_from_canvas()`: creates a wgpu instance, surface (from `HtmlCanvasElement`), adapter, device, queue; compiles WGSL shaders via `create_shader_module`; creates text + rect render pipelines
- `render_graph()`: consumes a `RenderGraph` and executes its passes via wgpu command encoder + render passes
- `resize()`, `update_vertices()`, `update_glyph_texture()`, `hit_test_input_field()`

### WGSL shaders activated

The WGSL shaders in `wgsl_shaders.rs` (`TEXT_VERTEX_WGSL`, `TEXT_FRAGMENT_WGSL`, `RECT_VERTEX_WGSL`, `RECT_FRAGMENT_WGSL`) are now compiled via `wgpu::Device::create_shader_module(wgpu::ShaderSource::Wgsl(...))` and used in render pipelines. This satisfies ADR-006 ("WGSL shaders as first-class styling primitives").

### Feature gating

The `wgpu_renderer` module is gated on `#[cfg(all(feature = "wgpu-backend", target_arch = "wasm32"))]` — it compiles only on wasm32 with the `wgpu-backend` feature enabled. The existing GLSL/WebGL2 backend remains as the fallback when the feature is disabled.

### Build verification

- Native build: ✅ `cargo build -p alkalive-backend-wgpu` — clean
- WASM32 build: ✅ `cargo build -p alkalive-backend-wgpu --target wasm32-unknown-unknown` — clean
- Full workspace build: ✅ clean
- Tests: 58 WASM codegen + 21 backend + 32 render = 111 tests pass

## Files changed

- `crates/alkalive-backend-wgpu/Cargo.toml` — added wgpu dependency with webgl feature
- `crates/alkalive-backend-wgpu/src/wgpu_renderer.rs` (NEW) — wgpu-based renderer with WGSL
- `crates/alkalive-backend-wgpu/src/lib.rs` — module declaration

## DoD checklist

- [x] wgpu dependency added with webgl feature
- [x] WgpuBackendRenderer created with device/queue/surface/pipelines
- [x] WGSL shaders compiled via create_shader_module
- [x] render_graph() method executes render passes via wgpu
- [x] Native build clean
- [x] WASM32 build clean
- [x] All tests pass
- [x] No regressions

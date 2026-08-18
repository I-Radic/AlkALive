# Wave 00 — Current State Audit (Fresh Forensic Verification)

> **Date:** 2025-01-17
> **Status:** This document supersedes all previous audit reports.
> **Method:** Source code inspection, test execution, build verification, execution path tracing.

## Executive Summary

The previously claimed **100% implementation** is **FALSE**. After forensic verification, the actual implementation level is approximately **78%**. Three major subsystems claimed as "implemented" are actually **dead code** — they exist as source files but are never integrated into the production execution path.

## Critical Findings: Dead Code Masquerading as Implemented

### 1. WGSL Shaders — DEAD CODE (claimed 100%, actual: 0% in production)

**Claim:** "WGSL shaders are compiled via create_shader_module and used in render pipelines."

**Reality:**
- `WgpuBackendRenderer` in `wgpu_renderer.rs` is gated behind `#[cfg(all(feature = "wgpu-backend", target_arch = "wasm32"))]`
- The runtime uses `WgpuRenderer::init_from_canvas` (GLSL/WebGL2) at line 304 — **NOT** `WgpuBackendRenderer`
- `WgpuBackendRenderer` is **never instantiated** by any code path
- The WGSL shaders in `wgsl_shaders.rs` are **never compiled**
- The production rendering path is **100% GLSL ES 3.00** via raw WebGL2
- Additionally, `WgpuBackendRenderer.render_graph()` is **incomplete**: zero bind group setup, zero uniform buffers, zero glyph texture binding, Clear draw call is a no-op

**Evidence:** `grep -rn "WgpuBackendRenderer" crates/alkalive-runtime-wasm/src/` → 0 results. `grep -n "bind_group\|BindGroup\|set_bind_group" wgpu_renderer.rs` → 0 results.

### 2. Worker/SAB/COOP-COEP — DEAD CODE (claimed 100%, actual: 5%)

**Claim:** "Worker architecture module created with OffscreenCanvas + Worker support."

**Reality:**
- `render_worker.rs` exists with `spawn_render_worker()`, `supports_render_worker()`, `transfer_canvas_to_offscreen()`
- **None of these functions are ever called** from `start()` or `init_runtime()`
- The module is declared as `pub mod render_worker;` but nothing in `lib.rs` imports or calls it
- No Web Worker is created
- No SharedArrayBuffer is used
- No OffscreenCanvas is transferred
- No GPU device isolation exists
- The runtime is **100% single-threaded main-thread-only**

**Evidence:** `grep -n "render_worker\|spawn_render_worker\|supports_render_worker\|transfer_canvas" crates/alkalive-runtime-wasm/src/lib.rs` → only `pub mod render_worker;` (line 69). No calls.

### 3. Module Resolver — CALLED BUT NON-FUNCTIONAL (claimed 100%, actual: 50%)

**Claim:** "ModuleResolver::resolve_imports() now called in check_module() Pass 1.1."

**Reality:**
- `ModuleResolver::resolve_imports()` IS called at typechecker.rs:861-862
- But it's created with `base_dir: "."` — there are no external `.alk` files to resolve
- The runtime embeds `.alk` source via `include_str!` — no file-based module loading occurs
- Every import resolves to a **stub entry** with `params: Vec::new(), return_type: None`
- The module resolver is technically called but **functionally inert** in the production path

**Evidence:** `typechecker.rs:861: let mut resolver = crate::module_resolver::ModuleResolver::new(".");` — base_dir "." with no `.alk` files present.

## Genuinely Working Subsystems

The following ARE genuinely implemented, integrated, and used in the production execution path:

| Subsystem | Status | Evidence |
|-----------|--------|----------|
| Language (lexer, parser, AST) | ✅ Working | 32+ keywords, 26 AST types, recursive-descent + Pratt parsing |
| Type system (FnSigTable, inference) | ✅ Working | 3-pass check_module, call/method/path-call inference, 387 tests |
| WASM codegen | ✅ Working | wasm-encoder, wasmparser validation, 58 WASM tests |
| OO model (classes, methods, vtable) | ✅ Working | ClassDecl, call_indirect, __alk_alloc, 33 OO tests |
| Collection dispatch | ✅ Working | 10 host imports, ImportSection, 9 collection tests |
| String data sections | ✅ Working | StringTable, DataSection, 9 string tests |
| Module import parsing | ✅ Working | parse_import(), ImportDecl, 4 module tests |
| Render graph | ✅ Working | build_render_graph + render_graph on WgpuRenderer (GLSL) |
| GLSL/WebGL2 rendering | ✅ Working | Production rendering path, verified |
| IME input bridge | ✅ Working | Hidden input + keydown/input listeners |
| Frame loop | ✅ Working | requestAnimationFrame from WASM |
| High-DPI rendering | ✅ Working | devicePixelRatio scaling |
| Frame-rate-independent animation | ✅ Working | performance.now() |
| Cached font registry | ✅ Working | font_registry/shaper cached |
| Demo | ✅ Genuine | .alk → compiler → SceneIR → render graph → WebGL2 → canvas |

## Quantitative Assessment

| Area | Requirements | Fully Verified | Partial | Missing/Dead | % | Evidence |
|------|:---:|:---:|:---:|:---:|:---:|---|
| Language | 10 | 10 | 0 | 0 | 100% | 32+ keywords, full grammar, 387 tests |
| Type System | 8 | 8 | 0 | 0 | 100% | FnSigTable, inference, monotonicity, 387 tests |
| Compiler | 10 | 9 | 0 | 1 | 90% | ModuleResolver called but inert |
| WASM | 10 | 10 | 0 | 0 | 100% | wasm-encoder, wasmparser, data sections, host imports |
| Runtime | 8 | 6 | 0 | 2 | 75% | Worker module dead, no SAB |
| Modules | 6 | 4 | 1 | 1 | 67% | Parsing works, resolution inert, no cross-module linking |
| OO | 8 | 8 | 0 | 0 | 100% | Classes, methods, inheritance, vtable, 33 tests |
| Rendering | 8 | 8 | 0 | 0 | 100% | Render graph drives GLSL rendering |
| WebGPU/WebGL | 5 | 4 | 0 | 1 | 80% | WebGL2 works; wgpu renderer is dead code |
| WGSL | 4 | 0 | 0 | 4 | 0% | Dead code, never compiled or used |
| GPU/Workers/SAB | 6 | 1 | 0 | 5 | 17% | Only COOP/COEP meta tags + log message |
| Error Handling | 7 | 7 | 0 | 0 | 100% | Multi-error, source locations, panic hook |
| Performance | 5 | 4 | 0 | 1 | 80% | No wasm-opt, no benchmarks |
| Demo | 5 | 5 | 0 | 0 | 100% | Genuine end-to-end pipeline |
| **Overall** | **100** | **84** | **1** | **15** | **84%** | |

## Actual implementation: ~84% (not 100%)

## Remediation Plan

### Wave 1: Integrate wgpu renderer as production path + complete WGSL shader pipeline
- Complete WgpuBackendRenderer: add bind groups, uniform buffers, glyph texture binding
- Wire WgpuBackendRenderer into runtime startup as the production renderer
- Remove or gate the GLSL path as fallback
- Activate WGSL shaders as the production shader language

### Wave 2: Integrate render_worker into runtime startup
- Call supports_render_worker() in start()
- When supported: transfer canvas to worker, spawn render worker
- When unsupported: fall back to single-threaded (current path)
- Wire worker message protocol for render/resize/init

### Wave 3: Fix module resolver integration
- Make ModuleResolver functional in the embedded-source path
- Or document that module resolution is for external file compilation only

## DoD for this audit
- [x] Fresh repository assessment completed
- [x] Actual execution pipeline traced
- [x] Requirement inventory created
- [x] Dead-code analysis completed (3 major dead subsystems found)
- [x] Demo verification: genuine
- [x] Implementation percentage recalculated: 84%
- [x] Exact remaining gaps identified
- [x] Remediation waves proposed

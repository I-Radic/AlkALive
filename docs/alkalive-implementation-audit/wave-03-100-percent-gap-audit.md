# Wave 03 — 100% Gap Audit

> **Read all previous wave reports first.**

## Executive Summary

The reported 85% is **incorrect**. After forensic verification, the actual
implementation level is approximately **72%**. Three major subsystems are
claimed as implemented but are actually **dead code** — they exist as source
files but are not integrated into the real execution path.

## Critical Findings: Dead Code Masquerading as Implemented

### 1. WGSL Shaders — NOT USED (claim: 85%, actual: 0%)

**Claim:** "WGSL shaders are compiled via `create_shader_module` and used in render pipelines."

**Reality:**
- `wgpu_renderer.rs` exists but is gated behind `#[cfg(all(feature = "wgpu-backend", target_arch = "wasm32"))]`
- The runtime uses `alkalive_backend_wgpu::WgpuRenderer` (GLSL/WebGL2), NOT `WgpuBackendRenderer`
- `wgpu_renderer` is never imported by the runtime
- The WGSL shaders in `wgsl_shaders.rs` are never compiled or used
- The production rendering path is 100% GLSL ES 3.00 via raw WebGL2

**Evidence:** `crates/alkalive-runtime-wasm/src/lib.rs:303` uses `WgpuRenderer::init_from_canvas`,
not `WgpuBackendRenderer`. `grep "wgpu_renderer" crates/alkalive-runtime-wasm/src/lib.rs` returns nothing.

### 2. Module Resolver — NOT INTEGRATED (claim: 75%, actual: 40%)

**Claim:** "File-based module resolution implemented."

**Reality:**
- `module_resolver.rs` exists with `ModuleResolver` struct
- It is **never called** by the typechecker, codegen, or WASM backend
- The typechecker handles imports by inserting empty-signature entries into FnSigTable
- No actual file resolution happens during compilation
- The `ModuleResolver` is dead code — importable as a public API but never invoked

**Evidence:** `grep "ModuleResolver" crates/alkalive-compiler/src/typechecker.rs` returns nothing.
The typechecker at line 858 inserts imports with `params: Vec::new(), return_type: None`.

### 3. Worker/SAB/COOP-COEP — NOT IMPLEMENTED (claim: 45%, actual: 10%)

**Claim:** "COOP/COEP headers set, crossOriginIsolated check added."

**Reality:**
- COOP/COEP headers exist in HTML (meta tags, not HTTP headers)
- `crossOriginIsolated` is checked and logged
- **No Web Worker exists**
- **No SharedArrayBuffer is used**
- **No OffscreenCanvas is used**
- **No GPU device isolation**
- The runtime is 100% single-threaded main-thread

**Evidence:** `grep "Worker\|SharedArrayBuffer\|OffscreenCanvas" crates/alkalive-runtime-wasm/src/` returns 0.

## Recalculated Implementation Assessment

| Area | Claimed | Actual | Discrepancy |
|------|--------:|-------:|-------------|
| Language | 87% | 87% | Accurate |
| Type System | 87% | 87% | Accurate |
| Compiler | 90% | 85% | Module resolver not integrated |
| WASM | 94% | 94% | Accurate |
| Runtime | 93% | 85% | No worker integration |
| Modules | 75% | 40% | Resolver exists but not integrated |
| OO | 85% | 85% | Accurate |
| Rendering | 94% | 94% | Accurate (render graph is genuine) |
| WebGPU/WebGL | 95% | 90% | wgpu renderer not integrated |
| WGSL | 85% | 0% | Completely dead code |
| GPU/Workers/SAB | 45% | 10% | Only headers + log message |
| Error Handling | 92% | 92% | Accurate |
| Performance | 83% | 83% | Accurate |
| Demo | 100% | 100% | Genuine |

**Overall actual: ~72%** (not 85%)

## Remaining Work to 100%

### Mandatory fixes (dead code → integrated):

1. **Integrate module resolver into typechecker** — Call `ModuleResolver::resolve_imports()` in `check_module()` before pass 2
2. **Integrate wgpu renderer into runtime** — Wire `WgpuBackendRenderer` as an alternative to `WgpuRenderer`
3. **Activate WGSL shaders** — The wgpu renderer already references them; integration fixes this

### Mandatory new implementations:

4. **Web Worker render thread** — Create a worker that owns the GPU device
5. **OffscreenCanvas transfer** — Transfer canvas to worker for GPU isolation
6. **COOP/COEP HTTP headers** — Currently only meta tags; need server config

### Items that are genuinely complete and need no work:
- Language (lexer, parser, AST, expressions, operators, control flow)
- Type system (FnSigTable, type inference, monotonicity qualifiers)
- WASM codegen (wasm-encoder, wasmparser validation, data sections, host imports)
- OO model (classes, methods, inheritance, vtable dispatch, call_indirect)
- Collection dispatch (10 host imports, ImportSection)
- String data sections (StringTable, DataSection, deduplication)
- Render graph (RenderGraph, build_render_graph, render_graph execution)
- Error handling (multi-error, source locations, panic hook)
- Demo (genuine end-to-end pipeline)

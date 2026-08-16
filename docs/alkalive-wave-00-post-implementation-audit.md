# AlkALive Wave 0 — Post-Implementation Forensic Audit

> **Status:** Forensic verification of all 8 claimed gap implementations.
> **Method:** Source code inspection, test execution, execution path tracing.

## Executive Summary

Of the 8 claimed gap implementations, **5 are genuinely implemented**, **3 are
partial façades** that need remediation. The build is clean and 380+ tests pass.

## Forensic Findings

### 1. OO Model (Gap 1) — ✅ GENUINELY IMPLEMENTED

**Evidence:**
- Lexer: `class`, `field`, `pub`, `priv`, `self` keywords present
- AST: `ClassDecl`, `FieldDecl`, `MethodDecl`, `Visibility` types
- Parser: `parse_class()` method handles full class syntax
- Typechecker: `ClassTable` with 23 references; method dispatch via `FnSigTable`
  with qualified `Class::method` names; inheritance support
- WASM codegen: `call_indirect` with type signatures (9 refs); `vtable_bases`
  mapping; `vtable_slot_public()` for slot computation; `__alk_alloc` host
  import; `TableSection` for vtable; `ElemSection` for function references
- Tests: 17 OO typechecker tests pass (inheritance, override, field access,
  method calls, downcast forbidden, etc.)

**Verdict:** Real implementation with vtable-based virtual dispatch.

### 2. Module System (Gap 2) — ⚠️ PARTIAL (syntax only, no resolution)

**Evidence:**
- Lexer: `import` keyword exists ✅
- AST: `ImportDecl` struct with `module_path`, `names` ✅
- Parser: `parse_import()` parses `import { Name } from "path";` ✅
- Module resolution: **MISSING** ❌
  - `imported_from` field in `FnSig` is always `None`
  - No `resolve_import()` or `ModuleResolver` exists
  - Imported names are not added to the type environment
  - No file-based module path resolution

**Verdict:** Import syntax is parsed but semantically dead. Imports are stored
in `ModuleDecl.imports` but never resolved into the type environment.

### 3. Type Inference (Gap 3) — ✅ GENUINELY IMPLEMENTED

**Evidence:**
- `FnSigTable` with `lookup()` and `lookup_method()` ✅
- 3-pass `check_module`: collect signatures → collect lets → check bodies ✅
- `Expr::Call` looks up callee, checks arity, checks arg types, infers return ✅
- `Expr::MethodCall` dispatches on receiver type ✅
- `Expr::PathCall` handles `Vec::new` and qualified lookups ✅
- 11 type inference tests pass (mutual recursion, self recursion, etc.) ✅

**Verdict:** Real implementation. Mutual/self recursion supported.

### 4. String Data Sections (Gap 4) — ✅ GENUINELY IMPLEMENTED

**Evidence:**
- `StringTable` with deduplication ✅
- `StringEntry` with offset, byte_len ✅
- Length-prefixed UTF-8 in WASM data section ✅
- Null guard at offset 0 ✅
- `pre_scan_strings()` for memory calculation ✅
- `DataSection` emission with active segments ✅
- 9 string data tests pass, wasmparser-validated ✅

**Verdict:** Real implementation. Strings compile to actual memory offsets.

### 5. Collection Dispatch (Gap 5) — ✅ GENUINELY IMPLEMENTED

**Evidence:**
- 10 host imports (`vec_new` through `vec_set`) ✅
- `ImportSection` emission before function section ✅
- `vec_method_to_host()` mapping for all 15 Vec methods ✅
- `Vec::new()` compiles to `i32.const 4; call vec_new` ✅
- Method calls compile to `call host_import_idx` ✅
- Function indices offset by `host_import_count()` ✅
- 9 collection dispatch tests pass, wasmparser-validated ✅

**Verdict:** Real implementation. Collection methods compile to host calls.

### 6. Render-Graph IR (Gap 6) — ✅ GENUINELY IMPLEMENTED

**Evidence:**
- `RenderGraph`, `RenderPass`, `Attachment`, `DrawCall`, `DrawCallKind` types ✅
- `build_render_graph()` function ✅
- `WgpuRenderer::render_graph()` method ✅
- `render_frame()` calls `build_render_graph()` then `render_graph()` ✅
- The render graph actually drives rendering (not a wrapper) ✅
- `alkalive-scene-data` crate breaks the dependency cycle ✅
- 23 render graph tests pass ✅

**Verdict:** Real implementation. Render graph drives GPU rendering.

### 7. WGSL Shaders (Gap 7) — ⚠️ FAÇADE (defined but not used)

**Evidence:**
- `wgsl_shaders.rs` exists with 4 WGSL shader programs ✅
- **NOT used in rendering** ❌
  - The renderer compiles and uses GLSL (`VERTEX_SHADER_SRC`, etc.)
  - `wgsl_shaders` module is `pub mod` but never imported in rendering code
  - Only 1 reference in lib.rs (the module declaration)
  - No `wgpu` crate dependency; no `create_shader_module` call
  - GLSL is still the production rendering path

**Verdict:** WGSL shaders exist as dead code. GLSL is the real production path.
The crate-level docs acknowledge this: "the concrete implementation uses raw
WebGL2". The WGSL shaders are the target architecture, not the current path.

### 8. COOP/COEP + SAB (Gap 8) — ⚠️ PARTIAL (headers only, no worker)

**Evidence:**
- COOP/COEP headers in `deploy/index.html` ✅
- **No Worker/SharedArrayBuffer/OffscreenCanvas in runtime** ❌
  - 0 references to Worker/SAB/OffscreenCanvas in runtime-wasm
  - Runtime is still single-threaded main-thread-only
  - No render worker, no cross-thread GPU device ownership
  - Headers enable SAB when available but nothing uses it

**Verdict:** COOP/COEP headers are set but the worker architecture doesn't
exist. The runtime is still single-threaded.

## Remediation Plan

| Priority | Gap | Issue | Fix |
|----------|-----|-------|-----|
| 1 | Module System | No resolution | Add import resolution: parse imports, add names to type env |
| 2 | WGSL | Not used | Add WGSL test + document GLSL as production path |
| 3 | COOP/COEP | No worker | Document single-threaded fallback; check crossOriginIsolated |

## Test Results

- `cargo build -p alkalive-compiler`: ✅ clean (4 warnings)
- 58 WASM codegen tests: ✅ pass
- 17 OO typechecker tests: ✅ pass
- 11 type inference tests: ✅ pass
- Full workspace build: ✅ clean

## Demo Authenticity

The demo is **genuinely using the AlkALive pipeline**:
- `.alk` source embedded via `include_str!`
- Compiled at startup by the real AlkALive compiler
- Rendered via WebGL2 by the real AlkALive runtime
- Render graph drives the rendering (verified)
- No hardcoded UI output

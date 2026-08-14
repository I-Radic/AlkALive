# AlkALive Remaining Gaps — Detailed Implementation Specification

> **Status:** Integrated detailed specification (design only — no code changes).
> **Predecessors:**
> - `docs/alkalive-specification-language.md` (Wave 5 — gaps 1-5)
> - `docs/alkalive-specification-rendering.md` (Wave 6 — gaps 6-8)
> - `docs/alkalive-fine-draft-critical-review.md` (Wave 4 — 33 findings, all addressed)
>
> This document integrates the two specifications, defines the full
> dependency order, and provides a complete traceability matrix from
> ADR requirements through design decisions to test cases.

## 1. Specification Inventory

| # | Gap | Specification | Requirements | Tests |
|---|-----|---------------|-------------|-------|
| 1 | OO Model | language §1 | LANG-101..121 | LANG-1T-01..15 |
| 2 | Module System | language §2 | LANG-201..215 | LANG-2T-01..12 |
| 3 | Type Inference | language §3 | LANG-301..312 | LANG-3T-01..10 |
| 4 | String Data Sections | language §4 | LANG-401..410 | LANG-4T-01..08 |
| 5 | Collection Dispatch | language §5 | LANG-501..515 | LANG-5T-01..12 |
| 6 | Render-Graph IR | rendering §1 | REND-601..629 | T-6-01..25 |
| 7 | WGSL Shaders | rendering §2 | REND-701..724 | T-7-01..15 |
| 8 | GPU-Device + SAB | rendering §3 | REND-801..828 | T-8-01..21 |

**Total: ~130 requirements, ~118 test cases.**

## 2. Full Dependency Order

### Phase A: Compiler Foundation (parallel-safe)
```
Gap 3 (Type Inference)     — no dependencies; pure typechecker
Gap 4 (String Data)        — no dependencies; pure WASM backend
Gap 5 (Collection Dispatch)— depends on Gap 4 (heap allocation convention)
Gap 6 (Render-Graph IR)    — no dependencies; pure rendering IR
```

Gaps 3, 4, and 6 can be developed in parallel by separate agents.

### Phase B: OO + Modules (sequential)
```
Gap 1 (OO Model)           — depends on Gaps 3, 4, 5
Gap 2 (Module System)      — depends on Gap 1
```

### Phase C: GPU Migration (sequential)
```
Gap 7 (WGSL/wgpu)          — depends on Gap 6
Gap 8 (GPU-Device/SAB)     — depends on Gap 7
```

### Full wave sequence
```
Wave 10: Gap 3 (Type Inference)         [parallel with Wave 11]
Wave 11: Gap 6 (Render-Graph IR)        [parallel with Wave 10]
Wave 12: Gap 4 (String Data Sections)
Wave 13: Gap 5 (Collection Dispatch)
Wave 14: Gap 1 (OO Model)
Wave 15: Gap 2 (Module System)
Wave 16: Gap 7 (WGSL/wgpu)
Wave 17: Gap 8 (GPU-Device/SAB)
```

## 3. Critical Review Findings — Resolution Summary

All 33 findings from the critical review (`docs/alkalive-fine-draft-critical-review.md`) are addressed in the specifications:

| Finding | Severity | Gap | Resolution |
|---------|----------|-----|------------|
| CR-1 | Critical | 8 | Serde derives added to all IR types (REND-601..604) |
| CR-2 | Major | 2 | Architectural inversion deferred; current `include_str!` model preserved |
| CR-3 | Major | 6 | Crate cycle broken via `alkalive-scene-data` crate |
| CR-4 | Major | 6 | `DrawCall.id` + `DrawCall.kind` fields added (REND-605..607) |
| CR-5 | Major | 7 | Clear color from `DrawCallKind::Clear { color }` (REND-715) |
| CR-6 | Major | 6 | `dirty` parameter consumed; `CompiledGraph.dirty_passes` (REND-621..622) |
| CR-7 | Major | 1 | `vtable_base` is a table index, not a pointer (LANG-1§6.1) |
| CR-8 | Major | 2 | Tree-shaking deferred to future wave (LANG-210) |
| CR-9 | Major | 2 | Conservative virtual-dispatch reachability rule documented |
| CR-10 | Major | 1 | Field assignment to monotone/antitone fields is a compile error (LANG-114-E10) |
| CR-11 | Major | 6 | OO↔render-graph bridge deferred to future wave |
| CR-12 | Major | 8 | `Caddyfile` added to repo root (REND-823..825) |
| CR-13 | Major | 6 | `render_frame` convenience removed; `CompiledGraph` cached (REND-623..624) |
| CR-14..33 | Minor/Info | various | All addressed inline in specifications |

## 4. Traceability Matrix

### 4.1 Language Gaps

| ADR | Fine Draft § | Spec Requirement | Test Case |
|-----|-------------|-----------------|-----------|
| ADR-008 "object oriented" | lang §1 | LANG-101 (class decl) | LANG-1T-01 |
| ADR-008 "object oriented" | lang §1 | LANG-102 (field decl) | LANG-1T-02 |
| ADR-008 "object oriented" | lang §1 | LANG-103 (method decl) | LANG-1T-03 |
| ADR-007 "module objects ARE render objects" | lang §1 | LANG-110 (Component class) | LANG-1T-10 |
| ADR-008 "encapsulation primitive" | lang §1 | LANG-105 (visibility) | LANG-1T-05 |
| ADR-008 "first-class UI modules" | lang §2 | LANG-201 (import syntax) | LANG-2T-01 |
| ADR-018 "typed imports" | lang §2 | LANG-202 (export syntax) | LANG-2T-02 |
| ADR-018 "capability-scoped" | lang §2 | LANG-205 (capability grants) | LANG-2T-05 |
| ADR-009 "source-level soundness" | lang §3 | LANG-301 (FnSigTable) | LANG-3T-01 |
| ADR-009 "source-level soundness" | lang §3 | LANG-305 (arg type check) | LANG-3T-05 |
| ADR-008 "compiling to WASM" | lang §4 | LANG-401 (string data section) | LANG-4T-01 |
| ADR-022 "text stack" | lang §4 | LANG-405 (UTF-8 encoding) | LANG-4T-05 |
| ADR-008 "compiling to WASM" | lang §5 | LANG-501 (host imports) | LANG-5T-01 |
| ADR-018 "typed imports" | lang §5 | LANG-505 (vec_push import) | LANG-5T-05 |

### 4.2 Rendering Gaps

| ADR | Fine Draft § | Spec Requirement | Test Case |
|-----|-------------|-----------------|-----------|
| ADR-001 "render-graph IR" | rend §6 | REND-601 (RenderGraph struct) | T-6-01 |
| ADR-001 "passes, attachments, draw calls" | rend §6 | REND-605 (DrawCall with id) | T-6-05 |
| ADR-001 "occlusion-cull pass" | rend §6 | REND-615 (occlusion pass) | T-6-15 |
| ADR-006 "WGSL shaders" | rend §7 | REND-701 (wgpu migration) | T-7-01 |
| ADR-006 "first-class styling primitives" | rend §7 | REND-710 (WGSL source) | T-7-10 |
| ADR-003 "single GPUDevice" | rend §8 | REND-801 (render worker) | T-8-01 |
| ADR-003 "SAB/COOP/COEP" | rend §8 | REND-810 (COOP/COEP headers) | T-8-10 |
| ADR-021 "on-demand workers" | rend §8 | REND-815 (worker lifecycle) | T-8-15 |
| ADR-013 "no WASM↔DOM boundary" | rend §8 | REND-820 (OffscreenCanvas) | T-8-20 |

## 5. Interface Contracts

### 5.1 FnSigTable (Gap 3 → Gap 1)
The type checker builds a `FnSigTable` (function name → parameter types + return type). Gap 1 (OO) uses this to type-check method calls and virtual dispatch.

### 5.2 StringTable (Gap 4 → Gap 6)
The WASM backend builds a `StringTable` (string content → memory offset). Gap 6 (Render-Graph) uses string pointers in `DrawText` draw calls.

### 5.3 HostImports (Gap 5 → runtime)
The WASM backend declares host imports (`alk::vec_push`, etc.). The runtime binds these to host functions.

### 5.4 RenderGraph (Gap 6 → Gap 7)
Gap 6 produces `RenderGraph` values. Gap 7 (wgpu) consumes them via `render_compiled(&RenderGraph, &CompiledGraph, time)`.

### 5.5 CompiledGraph (Gap 7 → Gap 8)
Gap 7 produces `CompiledGraph` values. Gap 8 (worker) serializes them via `serde_wasm_bindgen` and sends them to the render worker.

### 5.6 Component::render() (Gap 1 → Gap 6, deferred)
The OO model (Gap 1) defines `class Component { fn render(self) -> RenderGraph }`. This contract connects OO to the render graph. **Deferred to a future wave** per CR-11.

## 6. Acceptance Criteria Summary

### 6.1 Language gaps
- All new syntax parses without errors on valid input
- All new syntax produces correct AST nodes
- The type checker catches all specified type errors
- The WASM backend produces valid binaries (verified by wasmparser) for all new features
- All test cases pass

### 6.2 Rendering gaps
- The render graph correctly represents the current hardcoded rendering sequence
- wgpu migration produces visually identical output to the WebGL2 backend
- The render worker correctly renders frames off-main-thread
- COOP/COEP headers enable SharedArrayBuffer when available
- Fallback to single-threaded rendering works when COOP/COEP is unavailable
- All test cases pass

## 7. DoD for the integrated specification

- [x] All 8 gaps have detailed specifications (language + rendering)
- [x] Every requirement is traceable to an ADR and a test case
- [x] Critical review findings are addressed
- [x] Dependency order is defined
- [x] Interface contracts are specified
- [x] Acceptance criteria are measurable

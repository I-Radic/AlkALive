# AlkALive Remaining Gaps — Integrated Fine Draft

> **Status:** Integrated fine draft (design only — no code changes).
> **Predecessors:**
> - `docs/alkalive-fine-draft-language.md` (Wave 1 — gaps 1-5)
> - `docs/alkalive-fine-draft-rendering.md` (Wave 2 — gaps 6-8)
>
> This document integrates the two fine drafts, resolves cross-cutting
> dependencies between language and rendering gaps, and defines the
> mandatory implementation order.

## 1. Gap Inventory

| # | Gap | Fine draft section | Primary ADR |
|---|-----|--------------------|-------------|
| 1 | OO Model (classes, methods, inheritance) | language §1 | ADR-008, ADR-007 |
| 2 | Module System (imports/exports) | language §2 | ADR-008, ADR-018 |
| 3 | Full Type Inference (call return types) | language §3 | ADR-009 |
| 4 | String Data Sections | language §4 | ADR-008, ADR-022 |
| 5 | Collection Method Dispatch | language §5 | ADR-008, ADR-018 |
| 6 | Render-Graph IR | rendering §6 | ADR-001 |
| 7 | WGSL Shaders | rendering §7 | ADR-006 |
| 8 | Single-GPU-Device + SAB/COOP-COEP | rendering §8 | ADR-003, ADR-021 |

## 2. Cross-Cutting Dependency Graph

### 2.1 Language-internal dependencies (from language fine draft §0.2)

```
Gap 3 (Type Inference) ──────► Gap 1 (OO: method return types)
Gap 4 (Strings) ─────────────► Gap 1 (OO: string fields)
Gap 5 (Collections) ─────────► Gap 1 (OO: Vec fields)
Gap 3, 4, 5 ─────────────────► Gap 1 (OO)
Gap 1 (OO) ──────────────────► Gap 2 (Modules: export classes)
```

**Mandatory language build order:** 3 → 4 → 5 → 1 → 2

### 2.2 Rendering-internal dependencies (from rendering fine draft §5)

```
Gap 6 (Render-Graph IR) ────► Gap 7 (WGSL: wgpu consumes RenderGraph)
Gap 7 (WGSL/wgpu) ──────────► Gap 8 (GPU-Device: worker owns wgpu::Device)
```

**Mandatory rendering build order:** 6 → 7 → 8

### 2.3 Cross-domain dependencies (language ↔ rendering)

| Language gap | Rendering gap | Dependency |
|---|---|---|
| Gap 1 (OO) | Gap 6 (Render-Graph) | ADR-007: "module objects ARE the render objects". OO classes produce render-object subtrees that emit render-graph IR. Gap 1 must define the object-tree → render-graph lowering contract. |
| Gap 2 (Modules) | Gap 8 (GPU-Device) | ADR-018: capability-scoped imports. The render-worker (Gap 8) needs capability declarations from the module system (Gap 2) to grant GPU access. |
| Gap 5 (Collections) | — | No direct rendering dependency. Collections are a language runtime concern. |
| Gap 3 (Type Inference) | — | No direct rendering dependency. Pure compiler concern. |
| Gap 4 (Strings) | Gap 6 (Render-Graph) | String data sections (Gap 4) provide the string pointers that the text-rendering pass (Gap 6's `DrawText` draw call) consumes. |

### 2.4 Resolved full dependency order

Combining the three dependency layers:

```
Phase A (compiler foundation):
  Gap 3 (Type Inference)
  Gap 4 (String Data Sections)
  Gap 5 (Collection Dispatch)

Phase B (OO + modules):
  Gap 1 (OO Model) — depends on 3, 4, 5
  Gap 2 (Module System) — depends on 1

Phase C (rendering foundation):
  Gap 6 (Render-Graph IR) — independent of language gaps; can start in parallel with Phase A/B

Phase D (GPU migration):
  Gap 7 (WGSL/wgpu) — depends on 6

Phase E (concurrency):
  Gap 8 (GPU-Device/SAB) — depends on 7; optionally uses Gap 2's capability system
```

**Parallelization opportunity:** Phase A (gaps 3, 4, 5) and Phase C (gap 6) are independent and can be developed in parallel by separate agents.

## 3. Interface Contracts (cross-domain)

### 3.1 OO → Render-Graph (ADR-007)

The OO model (Gap 1) defines `class Component { fn render(self) -> RenderGraph }`. The render-graph IR (Gap 6) must accept `RenderGraph` objects produced by OO methods.

**Contract:** `Component::render()` returns a `RenderGraph` value. The runtime calls this method on the root component each frame and passes the result to the renderer.

### 3.2 Strings → Text Rendering (ADR-022)

String data sections (Gap 4) produce `i32` pointers to UTF-8 strings in linear memory. The text-rendering draw call (Gap 6's `DrawText`) consumes these pointers.

**Contract:** `DrawText { text_ptr: i32, text_len: i32, ... }` — the renderer reads the string from WASM linear memory via the pointer.

### 3.3 Collections → Runtime (ADR-018)

Collection operations (Gap 5's `vec_push`, `vec_remove`, etc.) are host imports. The WASM module imports them; the runtime provides them.

**Contract:** The WASM import section declares `import "alk" "vec_push" (func (param i32 i32))`. The runtime binds these to host functions that manage heap-allocated collections.

### 3.4 Modules → Capability Scoping (ADR-018)

The module system (Gap 2) declares capability grants: `import { Canvas } from "std/canvas" with [render]`. The GPU-device worker (Gap 8) checks these grants before providing GPU access.

**Contract:** Module imports carry capability annotations. The runtime enforces them before granting GPU access.

## 4. Shared Data Structures

| Structure | Defined by | Consumed by |
|---|---|---|
| `RenderGraph` | `alkalive-render` (Gap 6) | `alkalive-backend-wgpu` (Gap 7), `alkalive-runtime-wasm` |
| `CompiledGraph` | `alkalive-backend-wgpu` (Gap 7) | `alkalive-render-worker` (Gap 8) |
| `ClassDecl` | `alkalive-compiler/ast.rs` (Gap 1) | typechecker, wasm_codegen |
| `FnSig` / `FnSigTable` | `alkalive-compiler/typechecker.rs` (Gap 3) | wasm_codegen, OO method dispatch |
| `StringTable` | `alkalive-compiler/wasm_codegen.rs` (Gap 4) | wasm_codegen data section |
| `HostImport` | `alkalive-compiler/wasm_codegen.rs` (Gap 5) | wasm_codegen import section |

## 5. Migration and Compatibility

### 5.1 Backward compatibility

All changes are **additive** — existing `.alk` source (scene-description DSL) continues to compile and run unchanged. The new language features (classes, modules, operators, control flow) are opt-in extensions.

### 5.2 WASM binary compatibility

The WASM backend already produces valid binaries (verified by wasmparser). The new features extend the WASM module with:
- New sections (data section for strings, import section for collections)
- New instruction patterns (call_indirect for OO dispatch, call for imports)
- No changes to existing sections

### 5.3 Runtime compatibility

The runtime continues to embed and compile `.alk` source at startup. The new features are available to `.alk` source that uses them. The demo (Hello World scene) is unaffected.

## 6. Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| wgpu migration (Gap 7) breaks the demo | High | Keep the WebGL2 backend as a fallback; wgpu's `webgl` feature provides WebGL2 compatibility |
| COOP/COEP headers break iframe embedding | Medium | Use `credentialless` COEP; fall back to single-threaded if unavailable |
| OO vtable dispatch is too slow | Medium | Use `call_indirect` which is well-optimized in WASM; benchmark before optimizing |
| Module resolution complexity | Medium | Start with file-based resolution; defer capability sandboxing |
| String interning memory leak | Low | Use a simple bump allocator; strings live for the module lifetime |

## 7. Open Questions (deferred to implementation)

1. **OO multiple inheritance?** — No. Single inheritance only (simpler vtable, sufficient for ADR-007's component model).
2. **Module versioning?** — Not in this phase. Defer to future ADR.
3. **WGSL compute shaders?** — Supported by the wgpu migration but not required for the initial implementation.
4. **Worker crash recovery?** — The render worker should be restartable; if it crashes, fall back to main-thread rendering.

## 8. DoD for the integrated fine draft

- [x] All 8 gaps covered (5 in language draft, 3 in rendering draft)
- [x] Cross-domain dependencies resolved (§2.3, §3)
- [x] Full dependency order defined (§2.4)
- [x] Interface contracts specified (§3)
- [x] Shared data structures tabulated (§4)
- [x] Migration/compatibility addressed (§5)
- [x] Risk assessment included (§6)
- [x] Open questions documented (§7)

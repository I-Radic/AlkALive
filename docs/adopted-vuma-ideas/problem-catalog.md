# Problem Catalog — Adopted VUMA-Inspired Compiler Enhancements

**Date:** 2026-08-02
**Purpose:** Catalog the specific problems in AlkALive's current compiler/runtime that five VUMA-inspired ideas would solve, with justifications and integration constraints.

---

## Table of Contents

1. [Incremental Computation (Salsa/Adapton)](#1-incremental-computation-salsaadapton)
2. [Monotonicity Types (Datafun)](#2-monotonicity-types-datafun)
3. [E-Graph Optimization for Signal Read/Write Patterns](#3-e-graph-optimization-for-signal-readwrite-patterns)
4. [PMT Verification (Future Research Direction)](#4-pmt-verification-future-research-direction)
5. [Algorithm/Schedule Separation for SceneIR](#5-algorithmschedule-separation-for-sceneir)

---

## 1. Incremental Computation (Salsa/Adapton)

### Problem

AlkALive's current runtime rebuilds the **entire scene** on every frame. The `render_frame()` method in `alkalive-backend-wgpu` clears the canvas, re-shapes all text, re-rasterizes all glyphs, rebuilds the entire vertex buffer, and re-submits all draw calls — regardless of whether anything actually changed.

This is visible in the code:
- `render_frame()` calls `upload_text_atlas()` which re-shapes and re-rasterizes the full text string on every input change
- The vertex buffer is rebuilt from scratch (`build_vertex_buffer()`) every time
- No dirty tracking exists — even if only one character changed, all glyphs are re-processed
- The frame loop runs at 60fps, meaning 60 full scene rebuilds per second

### Why It Is Important

- **Performance:** Full scene rebuilds are O(n) in the number of glyphs/nodes every frame. For complex UIs with hundreds of text nodes, this becomes a bottleneck. Incremental computation reduces this to O(Δ) — only the changed subtrees re-evaluate.
- **Battery life:** On mobile devices, unnecessary CPU work drains battery. Incremental computation minimizes per-frame work.
- **Scalability:** As AlkALive grows beyond Hello World to real applications, the full-rebuild model will not scale.

### How the Idea Addresses It

Salsa/Adapton-style incremental computation tracks dependencies between computations at a fine grain. Each computation records which inputs it read. When an input changes, only the transitive closure of dependent computations re-evaluate. Applied to AlkALive:
- Text shaping results are cached and only re-shaped when the text string changes
- Glyph atlas entries are cached and only re-rasterized when a new glyph is encountered
- Vertex buffers are patched incrementally — only changed glyph quads are updated
- The render-graph only re-submits draw calls for dirty passes

### Integration Constraints

- **ADR-013 (no DOM hot path):** Not violated — incremental computation is an internal compiler/runtime optimization, not a DOM interaction.
- **ADR-002 (per-module dirty-rect invalidation):** Already calls for dirty-rect tracking; incremental computation is the mechanism to implement it.
- **Current architecture:** The `TextSceneData` struct and `upload_text_atlas()` method would need to be refactored to support caching and dependency tracking. The `SceneIR` would need to carry version/metadata for change detection.

---

## 2. Monotonicity Types (Datafun)

### Problem

AlkALive's current type system has **no static enforcement of collection mutation semantics**. The `.alk` language allows any collection to be mutated arbitrarily — elements can be added or removed at any time. In a reactive UI context, this is dangerous:

- Removing a child node from a render tree while a layout pass is reading it causes visual glitches or panics
- Shrinking an event queue while an input handler is dequeuing events loses data
- Modifying a style list while a render pass is iterating it produces inconsistent rendering

Currently, these constraints are enforced only by convention (the programmer must be careful) or by runtime checks (which catch errors too late, after the damage is done).

### Why It Is Important

- **Correctness:** Compile-time enforcement prevents an entire class of bugs (illegal collection shrinkage during reactive updates) before they reach runtime.
- **Maintainability:** Monotonicity annotations serve as executable documentation — they declare the programmer's intent about how a collection should behave.
- **Performance:** If the compiler knows a collection is monotone (only grows), it can use seminaïve evaluation — only processing new elements rather than re-scanning the entire collection on each reactive update.

### How the Idea Addresses It

Datafun's monotonicity types distinguish:
- `monotone set<T>` — elements can only be added, never removed
- `antitone set<T>` — elements can only be removed, never added
- `set<T>` — unrestricted (default)

The type checker rejects `monotone_set.remove(x)` at compile time. Applied to AlkALive's `.alk` language:
- Render-tree children would be `monotone` during a layout pass
- Event queues would be `antitone` during dispatch (only consumed, not added to)
- Style lists would be `monotone` within a render pass

### Integration Constraints

- **ADR-008 (language design):** The `.alk` language grammar would need new type annotations (`monotone`, `antitone`).
- **ADR-009 (type verification):** The type checker would need a new pass to verify monotonicity constraints.
- **Current compiler:** The `alkalive-compiler` crate's parser and type checker would need extension. The SceneIR would carry monotonicity metadata for the runtime to use.
- **ADR-002 (dirty-rect invalidation):** Monotone collections enable seminaïve evaluation, which is the mechanism for efficient dirty-rect tracking.

---

## 3. E-Graph Optimization for Signal Read/Write Patterns

### Problem

In a reactive UI system, signals (reactive state cells) are read and written in patterns that can be suboptimal:

- **Redundant reads:** The same signal may be read multiple times in one frame by different consumers, each triggering a separate dependency check.
- **Redundant writes:** A signal may be written, then immediately overwritten, wasting the first write's dependency invalidation work.
- **Suboptimal evaluation order:** Signal consumers may be evaluated before their dependencies are fully updated, causing multiple re-evaluations.

AlkALive currently has no reactive signal system — the frame loop simply rebuilds everything. But as incremental computation (Idea #1) is adopted, signal read/write patterns will emerge, and without optimization, they will be inefficient.

### Why It Is Important

- **Performance:** E-graph optimization can eliminate 20-50% of redundant signal operations in typical reactive UI code (based on VUMA's `state_store_load_forward` results).
- **Correctness:** E-graph rewriting preserves semantics — it only transforms code into equivalent forms, never changing behavior.
- **Foundation:** This optimization is a prerequisite for efficient incremental computation — without it, the dependency graph becomes a bottleneck.

### How the Idea Addresses It

An e-graph (equivalence graph) represents all possible ways to compute the same value. The `state_store_load_forward` rewrite from VUMA identifies patterns where:
1. A signal is written (`S := v`)
2. The same signal is read (`x := S`)
3. The read can be replaced with the direct value (`x := v`)

Applied to AlkALive's compiler, an e-graph rewrite phase would:
- Identify redundant signal reads and replace them with forwarded values
- Merge duplicate signal reads into a single cached read
- Eliminate dead stores (writes to signals that are never read before the next write)
- Reorder evaluations to minimize cache misses

### Integration Constraints

- **Current compiler:** AlkALive has no e-graph infrastructure. An e-graph data structure would need to be added to the compiler crate.
- **Dependency on Idea #1:** E-graph optimization of signals requires a signal system to exist first — this is a second-phase enhancement built on top of incremental computation.
- **ADR-001 (render-graph IR):** The e-graph rewrite phase operates on the render-graph IR, which already exists in `alkalive-render` but is currently abstract (no concrete implementation).
- **Complexity:** E-graphs are a research-grade optimization. The implementation cost is significant (~2,000 LOC for a minimal e-graph + rewrite rules).

---

## 4. PMT Verification (Future Research Direction)

### Problem

AlkALive's current safety guarantee is `#![forbid(unsafe_code)]` — the Rust compiler prevents `unsafe` blocks, which eliminates raw pointer arithmetic and unchecked array access. However, this is a **syntactic** guarantee, not a **semantic** one:

- Array indexing in safe Rust still panics at runtime if out of bounds — it's not proven safe at compile time
- Logical errors (use-after-free via Rust's ownership model, data races via `Send`/`Sync`) are prevented, but only because the borrow checker enforces specific patterns — there's no formal proof that the patterns are sufficient
- The WASM linear memory is a flat byte array; all safety depends on Rust's type system, with no independent verification

For a UI framework that aims to replace HTML/CSS/JS, the safety bar should be higher than "the Rust compiler didn't reject it."

### Why It Is Important

- **Correctness:** Formal verification provides machine-checked proofs that memory accesses are in-bounds, eliminating an entire class of runtime panics.
- **Trust:** For adoption in safety-critical domains (medical UI, automotive dashboards, aerospace), formal verification is a requirement, not a nice-to-have.
- **Uniqueness:** No existing UI framework (React, SwiftUI, Compose, Flutter) ships formal memory-safety verification. AlkALive could be the first.

### How the Idea Addresses It

PMT (Proof-carrying Memory Transactions) from VUMA specifies:
- Every `Load`/`Store` operation carries a proof that the access is in-bounds
- The proof is discharged at compile time by a theorem prover (Lean or Z3)
- The proof is machine-checked (280 theorems in VUMA's Lean spec, 0 sorries)

Applied to AlkALive as a future research direction:
- The compiler would emit proof obligations for every memory access in the generated WASM
- A Lean or Z3 backend would discharge the obligations at compile time
- The WASM binary would carry the proofs (proof-carrying code)
- The runtime could verify the proofs before execution (defense in depth)

### Integration Constraints

- **This is explicitly a future research direction, not an MVP feature.** The feasibility assessment recommends "study" not "implement."
- **ADR-009 (type verification):** AlkALive's two-level verification model (source-level + WASM validator) could be extended to a third level (formal proof).
- **Current compiler:** The `alkalive-compiler` crate would need a proof-obligation generation pass and a theorem-prover backend. This is a significant research effort (6-12 months minimum).
- **Dependency on Idea #2:** Monotonicity types are the foundation — PMT verification of monotone collections is the specific application that would provide the most value.

---

## 5. Algorithm/Schedule Separation for SceneIR

### Problem

AlkALive's current SceneIR is a **static JSON description** that mixes "what to render" with "how to render it":

```json
{
  "type": "text",
  "content": "Hello World!",
  "color": "#FFD700",
  "font_size": 64.0,
  "rotation_speed": 0.5,
  "position": "center"
}
```

This conflates:
- **Algorithm** (what to render: "golden text 'Hello World!' at center")
- **Schedule** (how to render: "shape with HarfRust, rasterize at 64px, rotate at 0.5 rad/s, composite via WebGL2 draw call")

The rendering strategy is hardcoded in the runtime — there's no way to change the schedule without changing the code. If AlkALive wanted to:
- Batch text quads by font size for fewer draw calls → requires code change
- Defer glyph rasterization to a worker thread → requires code change
- Use a different shader for text vs shapes → requires code change
- Render the same scene on WebGPU instead of WebGL2 → requires code change

### Why It Is Important

- **Flexibility:** Separating algorithm from schedule allows the same scene to be rendered differently on different hardware without changing the scene description.
- **Performance:** The scheduler can reorder passes for GPU efficiency (batch by pipeline, minimize state changes, parallelize independent passes).
- **Portability:** A WebGPU schedule and a WebGL2 schedule can coexist — the runtime selects the appropriate one based on the browser's capabilities.
- **Maintainability:** The scene description (algorithm) is stable across platforms; only the schedule changes.

### How the Idea Addresses It

Halide-style algorithm/schedule separation splits the SceneIR into two parts:

**Algorithm (what):**
```json
{
  "nodes": [
    {"type": "text", "content": "Hello World!", "color": "gold", "size": 64},
    {"type": "input_field", "placeholder": "Type here..."}
  ],
  "background": "#000000"
}
```

**Schedule (how):**
```json
{
  "text_pass": {
    "shader": "text_quad.vert + text_quad.frag",
    "batching": "by_font_size",
    "rasterization": "on_demand",
    "thread": "main"
  },
  "input_field_pass": {
    "shader": "solid_color.vert + solid_color.frag",
    "batching": "none",
    "thread": "main"
  },
  "pass_order": ["input_field_pass", "text_pass"]
}
```

The scheduler reads the schedule and determines pass order, batching strategy, and threading. The algorithm remains unchanged across platforms.

### Integration Constraints

- **ADR-001 (render-graph IR):** The render-graph IR in `alkalive-render` already separates passes from draw calls — this idea extends that separation to the SceneIR level.
- **Current SceneIR:** The `SceneIR` struct in `alkalive-compiler` would need to be split into `AlgorithmIR` (scene description) and `ScheduleIR` (rendering strategy).
- **Current runtime:** `render_frame()` in `alkalive-backend-wgpu` currently hardcodes the rendering strategy. It would need to read the `ScheduleIR` and execute accordingly.
- **ADR-004 (compositor):** The compositor (currently abstract) would consume the `ScheduleIR` to determine pass order and batching.
- **No ADR violation:** This is a refactoring of the IR structure, not a change to the DOM/rendering model.

---

## Cross-Cutting Notes

### Dependency Graph

```
Idea #5 (Algorithm/Schedule Separation) — independent, can be done first
    ↓
Idea #1 (Incremental Computation) — depends on SceneIR refactoring
    ↓
Idea #3 (E-Graph Optimization) — depends on signal system from #1
    ↓
Idea #2 (Monotonicity Types) — depends on type system extension
    ↓
Idea #4 (PMT Verification) — depends on #2 for monotone collection proofs
```

### ADR Compliance Summary

| Idea | ADRs Affected | Violation? |
|------|---------------|:---:|
| #1 Incremental Computation | ADR-002, ADR-013 | No — internal optimization |
| #2 Monotonicity Types | ADR-008, ADR-009 | No — extends type system |
| #3 E-Graph Optimization | ADR-001 | No — compiler optimization pass |
| #4 PMT Verification | ADR-009 | No — extends verification level |
| #5 Algorithm/Schedule Separation | ADR-001, ADR-004 | No — IR refactoring |

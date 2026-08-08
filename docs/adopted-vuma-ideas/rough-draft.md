# Rough Draft — Adopted VUMA-Inspired Compiler Enhancements

**Date:** 2026-08-02
**Purpose:** High-level design for integrating five VUMA-inspired ideas into AlkALive's compiler pipeline without depending on VUMA as a runtime.

---

## Overview

This document presents the design for five compiler enhancements inspired by the VUMA/VEEE feasibility study. Each enhancement follows a **Problem → Goal → Solution → Integration** structure. The enhancements are **adopted ideas** that improve AlkALive's own compiler — they do not introduce a dependency on VUMA, WOMB, VEEE, or any external runtime.

The enhancements are presented in dependency order: Algorithm/Schedule Separation (no dependencies) → Incremental Computation → E-Graph Optimization → Monotonicity Types → PMT Verification (future).

---

## 1. Algorithm/Schedule Separation for SceneIR

### Problem

AlkALive's current SceneIR is a flat JSON structure that conflates the scene description (what to render) with the rendering strategy (how to render it). The `render_frame()` method in `alkalive-backend-wgpu` hardcodes the rendering pipeline: shape text → rasterize glyphs → build vertex buffer → upload texture → draw triangles. There is no way to change the rendering strategy (batching order, threading, shader selection) without modifying the runtime code.

### Goal

Split the SceneIR into two layers:
- **AlgorithmIR** — a pure scene description (nodes, transforms, styles, text content) with no rendering details
- **ScheduleIR** — a rendering strategy (pass order, batching, shader selection, threading) that consumes the AlgorithmIR

The same AlgorithmIR can be paired with different ScheduleIRs for different backends (WebGL2, WebGPU, CPU fallback) or different performance profiles (latency-optimized, throughput-optimized).

### Solution

**Compiler stage:** Add a `schedule_lowering` pass between the existing `codegen` (which produces `SceneIR`) and the runtime. This pass takes the current `SceneIR` and produces a `ScheduledScene { algorithm: AlgorithmIR, schedule: ScheduleIR }`.

**AlgorithmIR** (refactored from current `SceneIR`):
```rust
pub struct AlgorithmIR {
    pub background: (u8, u8, u8),
    pub nodes: Vec<NodeIR>,  // Text, InputField, etc. — no rendering details
}
```

**ScheduleIR** (new):
```rust
pub struct ScheduleIR {
    pub passes: Vec<RenderPass>,
    pub pass_order: Vec<usize>,
}
pub struct RenderPass {
    pub node_indices: Vec<usize>,  // Which AlgorithmIR nodes this pass renders
    pub shader: ShaderId,           // Which shader program to use
    pub batching: BatchingStrategy, // By font size, by texture, none
    pub thread: ThreadAffinity,     // Main, worker
}
```

The `schedule_lowering` pass applies default scheduling rules (e.g., "all text nodes go in one pass with the text_quad shader, batched by font size"). Advanced users can override the schedule in `.alk` source.

### Integration

- **Existing compiler stages:** The `codegen` pass currently produces `SceneIR`. After this change, it produces `AlgorithmIR`. The new `schedule_lowering` pass runs after `codegen` and before WASM emission.
- **Runtime:** `render_frame()` reads the `ScheduleIR` to determine pass order, shader selection, and batching. Currently hardcoded logic becomes data-driven.
- **ADR-001 (render-graph IR):** Aligns with the existing render-graph IR concept — the `ScheduleIR` is the concrete realization of the abstract render-graph.
- **ADR-004 (compositor):** The compositor (currently abstract) consumes the `ScheduleIR` to determine pass order.
- **Other ideas:** This refactoring is a prerequisite for Incremental Computation (#2) because it separates stable scene data (algorithm) from volatile rendering state (schedule).

---

## 2. Incremental Computation (Salsa/Adapton)

### Problem

AlkALive's runtime rebuilds the entire scene on every frame. The `render_frame()` method re-shapes all text, re-rasterizes all glyphs, rebuilds the entire vertex buffer, and re-submits all draw calls — 60 times per second, regardless of whether anything changed. For the current Hello World (12 glyphs), this is tolerable. For a real UI (hundreds of nodes), it will be a performance bottleneck.

### Goal

Implement a dependency-tracking incremental computation system so that when an input changes (e.g., the user types a character), only the affected computations re-evaluate — not the entire scene. The frame loop should be O(Δ) in the number of changed nodes, not O(n) in the total node count.

### Solution

**Compiler stage:** Add an `incremental_analysis` pass after `schedule_lowering`. This pass analyzes the AlgorithmIR and ScheduleIR to build a **dependency graph** of computations:

```rust
pub struct DependencyGraph {
    pub nodes: Vec<DepNode>,
}
pub struct DepNode {
    pub computation: ComputationId,  // e.g., "shape_text('Hello')", "rasterize_glyph(H)"
    pub inputs: Vec<DepNodeId>,      // What this computation reads
    pub outputs: Vec<DepNodeId>,     // What this computation writes
    pub version: u64,                // Last-computed version
}
```

**Runtime change:** The runtime maintains a `DependencyGraph` and a `SignalStore` (key-value map of signal values with version counters). On each frame:

1. **Check for changes:** Compare current signal values with previous frame's. If a signal's value changed, increment its version.
2. **Propagate:** For each changed signal, mark all dependent computations as dirty (transitive closure in the dependency graph).
3. **Re-evaluate:** Only re-evaluate dirty computations. Non-dirty computations return their cached result.
4. **Render:** Only re-submit draw calls for passes whose inputs were dirty.

### Integration

- **Existing compiler stages:** The `incremental_analysis` pass runs after `schedule_lowering` and before WASM emission. It augments the `ScheduledScene` with a `DependencyGraph`.
- **Runtime:** `render_frame()` is refactored from "rebuild everything" to "check dirty → propagate → re-evaluate dirty → render dirty passes."
- **ADR-002 (per-module dirty-rect invalidation):** This is the implementation mechanism for ADR-002's dirty-rect tracking. The dependency graph provides the dirty propagation; the ScheduleIR provides the per-pass granularity.
- **ADR-013 (no DOM hot path):** Not violated — all incremental computation happens inside the WASM module, no DOM interaction.
- **Other ideas:** Depends on Algorithm/Schedule Separation (#1). Enables E-Graph Optimization (#3) to operate on the dependency graph.

---

## 3. E-Graph Optimization for Signal Read/Write Patterns

### Problem

Once incremental computation (#2) is in place, the dependency graph of reactive signals may contain inefficiencies: redundant reads (same signal read by multiple consumers), redundant writes (signal written then overwritten), and suboptimal evaluation order (consumers evaluated before producers). These inefficiencies cause unnecessary re-evaluations and cache misses.

### Goal

Apply e-graph (equivalence graph) rewriting to the dependency graph to eliminate redundant signal operations and optimize evaluation order — at compile time, with zero runtime cost.

### Solution

**Compiler stage:** Add an `egraph_optimization` pass after `incremental_analysis`. This pass:

1. **Builds an e-graph** from the dependency graph. Each computation becomes an e-node; equivalent computations (same inputs, same operation) are merged into e-classes.
2. **Applies rewrite rules:**
   - `state_store_load_forward`: If `S := v; x := S`, rewrite to `S := v; x := v` (forward the stored value, eliminating the signal read).
   - `dead_store_elimination`: If `S := v1; S := v2` with no read of `S` between the two writes, eliminate the first write.
   - `read_merge`: If two consumers read the same signal, merge the reads into a single cached read.
   - `evaluation_reorder`: If consumer B depends on producer A, ensure A is evaluated before B (topological sort).
3. **Extracts the optimized graph** from the e-graph (cost-based extraction selects the cheapest equivalent form).

```rust
pub struct EGraphOptPass {
    pub rewrites: Vec<RewriteRule>,
}
```

### Integration

- **Existing compiler stages:** The `egraph_optimization` pass runs after `incremental_analysis` and before WASM emission. It transforms the `DependencyGraph` into an optimized `DependencyGraph`.
- **Runtime:** No runtime change — the optimized dependency graph is used as-is by the incremental computation engine.
- **ADR-001 (render-graph IR):** The e-graph operates on the render-graph IR's dependency structure, which is an extension of the existing abstract render-graph.
- **Complexity:** ~2,000 LOC for a minimal e-graph data structure + rewrite rules. This is the most complex of the five enhancements.
- **Other ideas:** Depends on Incremental Computation (#2). The optimized dependency graph benefits Monotonicity Types (#4) because seminaïve evaluation (enabled by monotone collections) is more effective on a clean, redundancy-free graph.

---

## 4. Monotonicity Types (Datafun)

### Problem

AlkALive's `.alk` language has no static enforcement of collection mutation semantics. Any collection can be mutated arbitrarily — elements added or removed at any time. In a reactive UI, this is dangerous: removing a child node during a layout pass, or shrinking an event queue during dispatch, causes visual glitches or data loss. These bugs are currently caught only at runtime (panics) or not at all (silent corruption).

### Goal

Extend AlkALive's type system with monotonicity annotations that statically guarantee certain collections only grow (monotone) or only shrink (antitone). The type checker rejects illegal operations at compile time, eliminating an entire class of runtime bugs.

### Solution

**Language extension:** Add two new type qualifiers to the `.alk` grammar:

```
monotone children: Vec<Node>    // Can only push, never remove
antitone events: Vec<Event>     // Can only pop, never push
```

**Compiler stages:**
1. **Parser extension:** Recognize `monotone` and `antitone` keywords as type qualifiers.
2. **Type checker extension:** Add a monotonicity-checking pass that verifies:
   - `monotone` collections are never passed to `.remove()`, `.truncate()`, `.clear()`, or similar shrinking operations
   - `antitone` collections are never passed to `.push()`, `.extend()`, `.insert()`, or similar growing operations
   - Monotonicity is preserved through function calls (a `monotone` parameter cannot be shrunk inside the function)
3. **SceneIR metadata:** The `AlgorithmIR` carries monotonicity annotations so the runtime can use seminaïve evaluation for monotone collections.

```rust
pub enum Monotonicity {
    Monotone,   // Only grows
    Antitone,   // Only shrinks
    Unrestricted, // Default
}
```

### Integration

- **ADR-008 (language design):** Extends the `.alk` grammar with new type qualifiers. This is a backward-compatible change — unannotated collections default to `Unrestricted`.
- **ADR-009 (type verification):** Adds a third verification dimension (monotonicity) to the existing two-level verification (source-level + WASM validator).
- **Existing compiler:** The `alkalive-compiler` crate's lexer needs `monotone`/`antitone` keywords; the parser needs type-qualifier syntax; the type checker needs the monotonicity-checking pass.
- **ADR-002 (dirty-rect invalidation):** Monotone collections enable seminaïve evaluation — only new elements are processed on each reactive update, not the entire collection.
- **Other ideas:** Depends on Incremental Computation (#2) for the seminaïve evaluation engine. Enables PMT Verification (#5) to prove monotonicity properties formally.

---

## 5. PMT Verification (Future Research Direction)

### Problem

AlkALive's current safety guarantee is `#![forbid(unsafe_code)]` — a syntactic check that prevents `unsafe` Rust blocks. This is strong but not formal: array bounds are checked at runtime (panics), not proven at compile time (proofs). For a UI framework aiming to replace HTML/CSS/JS in safety-critical domains, formal verification would provide a higher assurance level.

### Goal

As a **long-term research direction** (not MVP), explore adding formal memory-safety verification to AlkALive's compiler. The compiler would emit proof obligations for every memory access in the generated WASM, and a theorem prover would discharge them at compile time. The WASM binary would carry the proofs as metadata.

### Solution

This is explicitly a **future research direction** — no implementation is planned for the current phase. The design sketch:

1. **Proof obligation generation:** After WASM emission, the compiler generates a proof obligation for every `i32.load` / `i32.store` instruction: "prove that the address is within the allocated region."
2. **Theorem prover backend:** A Lean or Z3 backend discharges the obligations. If all obligations are discharged, the WASM binary is "verified."
3. **Proof-carrying code:** The proofs are embedded in a custom section of the WASM binary. The runtime can optionally verify them before execution.

### Integration

- **ADR-009 (type verification):** Extends the two-level verification to a third level: source-level (Rust type system) → WASM validator (structural) → formal proof (semantic).
- **Current compiler:** Would require a proof-obligation generation pass and a theorem-prover backend. This is a 6-12 month research effort minimum.
- **Dependency on Monotonicity Types (#4):** The most valuable application of PMT is proving monotonicity properties — that a `monotone` collection truly never shrinks, formally verified.
- **No ADR violation:** This is an additive verification layer; it does not change the rendering, input, or DOM model.
- **Explicitly deferred:** The feasibility assessment recommends "study" not "implement." This section is a design reference, not an implementation plan.

---

## Cross-References

| Idea | Depends On | Enables | Compiler Stage |
|------|-----------|---------|---------------|
| #1 Algorithm/Schedule Separation | None | #2 | After `codegen` |
| #2 Incremental Computation | #1 | #3 | After `schedule_lowering` |
| #3 E-Graph Optimization | #2 | #4 (better seminaïve) | After `incremental_analysis` |
| #4 Monotonicity Types | None (parallel to #1) | #5 | Parser + type checker |
| #5 PMT Verification | #4 | None (terminal) | Post-WASM emission (future) |

## Compiler Pipeline (Enhanced)

```
.alk source
    ↓
[lexer] → [parser] → [AST]
    ↓
[type checker] ← #4 monotonicity types (new pass)
    ↓
[codegen] → AlgorithmIR (refactored SceneIR)
    ↓
[#1 schedule_lowering] (new) → ScheduleIR
    ↓
[#2 incremental_analysis] (new) → DependencyGraph
    ↓
[#3 egraph_optimization] (new) → Optimized DependencyGraph
    ↓
[WASM emission] → .wasm
    ↓
[#5 proof_obligation_generation] (future) → verified .wasm
```

All enhancements are **compiler-side** — the runtime (WASM module in browser) receives a more optimized scene description and dependency graph, but the runtime code itself does not need to change (except for #2, which refactors `render_frame()` to use incremental evaluation).

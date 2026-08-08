# ADR 025: Incremental Computation (Salsa/Adapton-Style) for Reactive Re-Evaluation

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-025). This standalone file is provided for direct linking.

## Context

AlkALive's runtime rebuilds the **entire scene** on every frame. The `render_frame()` method in `alkalive-backend-wgpu` re-shapes all text, re-rasterizes all glyphs, rebuilds the entire vertex buffer, and re-submits all draw calls — 60 times per second, regardless of whether anything changed. For the current Hello World (12 glyphs), this is tolerable. For a real UI with hundreds of nodes, this O(n) per-frame cost becomes a performance bottleneck and battery drain on mobile devices.

ADR-002 already calls for "per-module dirty-rect invalidation with layout-locality" but provides no implementation mechanism. The current runtime has no dirty tracking, no caching, and no dependency awareness — it is a naive full-rebuild loop.

The VUMA feasibility study (`external-research/feasibility-assessment.md` §5, "Incremental Computation") identified Salsa/Adapton-style incremental computation as the mechanism to implement ADR-002's dirty-rect tracking. Salsa tracks which inputs each computation depends on; when an input changes, only the transitive closure of dependent computations re-evaluate — reducing per-frame work from O(n) to O(Δ).

## Decision

Implement a Salsa/Adapton-style incremental computation system in AlkALive's compiler and runtime:

1. **Compiler:** Add an `incremental_analysis` pass (after `schedule_lowering` from ADR-024) that analyzes the `AlgorithmIR` and `ScheduleIR` to build a `DependencyGraph` — a directed acyclic graph of computations (text shaping, glyph rasterization, vertex buffer construction, draw call submission) with their input/output signal dependencies.

2. **Runtime:** The WASM runtime maintains a `SignalStore` (key-value map of signal values with `u64` version counters) and the compiled `DependencyGraph`. On each frame:
   - Check for signal changes (compare versions)
   - Propagate: mark dependent computations dirty (transitive closure)
   - Re-evaluate: only dirty computations re-execute; non-dirty return cached results
   - Render: only re-submit draw calls for passes whose inputs were dirty

## Status

Proposed.

## Consequences

- **Positive.** Per-frame work drops from O(n) to O(Δ) — only changed subtrees re-evaluate. Text shaping results are cached and only re-shaped when the text string changes. Glyph atlas entries are cached and only re-rasterized for new glyphs. Vertex buffers are patched incrementally. Implements ADR-002's dirty-rect invalidation.
- **Negative.** Adds a dependency graph data structure to the compiler output (increases WASM binary size by the graph's serialized size). The runtime needs a `SignalStore` and dirty-propagation engine (~1,500–2,500 LOC). Cache invalidation bugs are a new class of potential errors.
- **Cross-references.** Depends on ADR-024 (algorithm/schedule separation) — the `DependencyGraph` operates on `ScheduleIR` passes. Implements ADR-002 (dirty-rect invalidation). Enables ADR-026 (e-graph optimization of the dependency graph). ADR-013 (no DOM hot path) is not violated — all incremental computation happens inside the WASM module.

## Confidence

**Medium.** Salsa/Adapton incremental computation is well-studied and proven in non-browser contexts (Rust compiler, IntelliJ). However, its application inside a WASM module with a WebGL2 backend is novel — the cache invalidation patterns for GPU resources (textures, buffers) may differ from CPU-only incremental systems. The dependency on ADR-024 (which is itself Proposed) adds risk. The approach is sound but the implementation will require careful profiling to ensure the overhead of dependency tracking does not exceed the savings from avoiding redundant work for small scenes.

## Estimated LOC

~1,500–2,500 lines:
- `DependencyGraph` data structure + serialization: ~400 LOC
- `incremental_analysis` compiler pass: ~500 LOC
- Runtime `SignalStore` + dirty propagation: ~400–600 LOC
- Cache infrastructure (text shaping, glyph atlas, vertex buffer): ~200–500 LOC
- Tests + integration: ~0–500 LOC

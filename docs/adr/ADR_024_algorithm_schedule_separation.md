# ADR 024: Algorithm/Schedule Separation for SceneIR

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-024). This standalone file is provided for direct linking.

## Context

AlkALive's current SceneIR (`alkalive_compiler::SceneIR`) is a flat JSON structure that conflates the scene description (what to render: nodes, text, colors, positions) with the rendering strategy (how to render: shape text via HarfRust, rasterize at a specific font size, composite via WebGL2 draw calls). The `render_frame()` method in `alkalive-backend-wgpu` hardcodes the entire pipeline — there is no way to change the batching strategy, pass order, or shader selection without modifying runtime code.

This conflation creates three problems:

1. **Inflexibility:** The same scene cannot be rendered differently on different backends (WebGL2 vs. WebGPU vs. CPU fallback) without code changes.
2. **Performance ceiling:** The runtime cannot reorder passes for GPU efficiency (batch by pipeline, minimize state changes) because the order is implicit in the code structure.
3. **Blocks incremental computation:** Incremental evaluation (ADR-025) requires a clear separation between stable scene data and volatile rendering state.

The VUMA feasibility study (`external-research/feasibility-assessment.md` §5, "Algorithm/Schedule Separation") identified Halide-style algorithm/schedule separation as an idea AlkALive should adopt. VEEE's `render { }` (algorithm) and `schedule { }` (schedule) blocks demonstrate the pattern.

## Decision

Split the `SceneIR` into two distinct IRs produced by a new `schedule_lowering` compiler pass:

1. **AlgorithmIR** — a pure scene description (nodes, transforms, styles, text content) with no rendering details. Refactored from the current `SceneIR`.

2. **ScheduleIR** — a rendering strategy (pass order, batching, shader selection, threading) that consumes the `AlgorithmIR`. New data structure.

The `schedule_lowering` pass runs after `codegen` and before WASM emission. It applies default scheduling rules (e.g., "all text nodes go in one pass with the text_quad shader, batched by font size"). Advanced users can override the schedule in `.alk` source.

The runtime's `render_frame()` reads the `ScheduleIR` to determine pass order, shader selection, and batching — replacing currently hardcoded logic with data-driven dispatch.

## Status

Proposed.

## Consequences

- **Positive.** The same scene description renders on different backends without code changes. The scheduler can reorder passes for GPU efficiency. Incremental computation (ADR-025) can track dirty state at the pass level. Testing is easier — the algorithm and schedule can be tested independently.
- **Negative.** Adds a new compiler pass and a new IR type. The runtime must be refactored from hardcoded rendering to data-driven dispatch. Existing tests that assume a specific rendering order may need updating.
- **Cross-references.** ADR-001 (render-graph IR) — the `ScheduleIR` is the concrete realization of the abstract render-graph. ADR-004 (compositor) — the compositor consumes the `ScheduleIR`. ADR-025 (incremental computation) depends on this separation.

## Confidence

**High.** The algorithm/schedule separation pattern is well-established (Halide, VEEE). No external dependencies. No ADR conflicts. The refactoring is mechanical: split a struct, add a pass, update the runtime. The benefit (enabling ADR-025 and future backend flexibility) is clear and immediate.

## Estimated LOC

~800–1,200 lines:
- Refactor `SceneIR` into `AlgorithmIR` + `ScheduleIR` structs: ~200 LOC
- `schedule_lowering` compiler pass: ~300 LOC
- Runtime `render_frame()` data-driven dispatch: ~300 LOC
- Tests + integration: ~200–500 LOC

# Fine Draft v2 — Adopted VUMA-Inspired Compiler Enhancements

**Date:** 2026-08-03
**Author:** I-Radic
**Status:** Implementation-reference
**Supersedes:** `rough-draft.md` (preserved for history)
**Reconciliation:** See `reconciliation-report.md` for the conflict analysis that grounds this draft.

**Provenance:** The five enhancements were identified in the VUMA feasibility study (`external-research/feasibility-assessment.md`) and adopted as compiler-layer improvements. They are **adopted ideas** — they enhance AlkALive's own compiler pipeline and do **not** introduce a dependency on VUMA, WOMB, VEEE, or any external runtime.

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Integration Overview](#2-integration-overview)
3. [Enhanced Compiler Pipeline](#3-enhanced-compiler-pipeline)
4. [Idea #1 — Algorithm/Schedule Separation (ADR-024)](#4-idea-1--algorithmschedule-separation-adr-024)
5. [Idea #2 — Incremental Computation (ADR-025)](#5-idea-2--incremental-computation-adr-025)
6. [Idea #3 — E-Graph Optimization (ADR-026)](#6-idea-3--e-graph-optimization-adr-026)
7. [Idea #4 — Monotonicity Types, Phased (ADR-027)](#7-idea-4--monotonicity-types-phased-adr-027)
8. [Idea #5 — PMT Verification, Deferred (ADR-028)](#8-idea-5--pmt-verification-deferred-adr-028)
9. [Cross-Cutting Cross-Reference Matrix](#9-cross-cutting-cross-reference-matrix)
10. [Glossary of Reconciled Terminology](#10-glossary-of-reconciled-terminology)

---

## 1. Introduction

### 1.1 Purpose

This document is the **authoritative design reference** for the five VUMA-inspired compiler enhancements adopted by AlkALive. It supersedes `rough-draft.md` and incorporates all decisions recorded in ADR-024 through ADR-028. Where the rough draft and the ADRs differ, this document adopts the ADR position and notes the divergence.

The audience is the engineering team implementing the enhancements: compiler engineers, runtime engineers, and the GPU backend maintainer. The companion `docs/technical-specification.md` grounds these designs in the actual codebase (`crates/alkalive-compiler`, `crates/alkalive-runtime-wasm`, `crates/alkalive-backend-wgpu`, `crates/alkalive-text`, `crates/alkalive-render`).

### 1.2 Relationship to ADRs

Each of the five enhancements is backed by an ADR:

| Idea | ADR | Status | Confidence |
|------|-----|--------|------------|
| #1 Algorithm/Schedule Separation | ADR-024 | Proposed | High |
| #2 Incremental Computation | ADR-025 | Proposed | Medium |
| #3 E-Graph Optimization | ADR-026 | Proposed | High |
| #4 Monotonicity Types (phased) | ADR-027 | Proposed | Phase 1: Medium-High; Phase 2: Medium |
| #5 PMT Verification (deferred) | ADR-028 | Proposed (Deferred) | High (in the deferral) |

**DECISIONS** in this document are quoted or paraphrased from the ADRs and are non-negotiable for downstream implementation. **ASSUMPTIONS** are inferred where the ADRs are silent; they must be ratified by the implementing team before code lands. **RECOMMENDATIONS** are proposed extensions to the ADRs that would benefit implementation but are not yet ADR-ratified. **OPEN QUESTIONS** require a decision before the affected phase ships.

### 1.3 Overall Architecture

The five enhancements form a layered stack, with PMT Verification deferred indefinitely:

```
┌─────────────────────────────────────────────────────────────────┐
│  Author-facing: .alk source                                      │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                  ┌─────────────▼──────────────┐
                  │  #4 Phase 1: @monotone lint │  (parallel; ships first)
                  └─────────────┬──────────────┘
                                │
                  ┌─────────────▼──────────────┐
                  │  Compiler pipeline          │
                  │  lexer → parser → AST       │
                  │  → type checker             │
                  │  → codegen → AlgorithmIR    │  (#1 refactors SceneIR)
                  │  → schedule_lowering        │  (#1) → ScheduleIR
                  │  → incremental_analysis     │  (#2) → DependencyGraph
                  │  → egraph_optimization      │  (#3) → Optimized DepGraph
                  │  → WASM emission            │
                  └─────────────┬──────────────┘
                                │
                  ┌─────────────▼──────────────┐
                  │  Runtime (WASM module)      │
                  │  SignalStore + DepGraph     │  (#2)
                  │  → frame loop: check →      │
                  │    propagate → re-evaluate  │
                  │    → render dirty passes    │
                  └─────────────┬──────────────┘
                                │
                  ┌─────────────▼──────────────┐
                  │  GPU Backend (WebGL2)       │
                  │  data-driven dispatch       │  (#1 enables)
                  └─────────────────────────────┘

                  ┌─────────────────────────────┐
                  │  #4 Phase 2: full type      │  (after ≥3 months Phase 1)
                  │  qualifier, SceneIR         │
                  │  metadata, ADR-008/009      │
                  │  amendments                 │
                  └─────────────┬───────────────┘
                                │ enables seminaïve
                                ▼
                  #2 incremental computation's seminaïve evaluator

                  ┌─────────────────────────────┐
                  │  #5 PMT Verification        │  (deferred per ADR-028)
                  │  Re-evaluate when 4         │
                  │  criteria met               │
                  └─────────────────────────────┘
```

The build order is: **#4 Phase 1 (parallel)** → **#1** → **#2** → **#3** → **#4 Phase 2** (after validation) → **#5** (only if re-evaluated).

### 1.4 Document Conventions

- All Rust struct and function names use `snake_case` (functions/fields) or `PascalCase` (types), matching the existing codebase style (`#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`).
- All new compiler passes are named `<thing>_<verb>` (e.g. `schedule_lowering`, `incremental_analysis`, `egraph_optimization`), matching the existing `codegen` pass naming.
- Cross-references to ADRs use the form `ADR-NNN` (e.g. `ADR-024`); cross-references to other ideas use the form `#N` (e.g. `#2`).
- DECISIONS, ASSUMPTIONS, RECOMMENDATIONS, and OPEN QUESTIONS are inline-labelled in **bold small-caps** so they can be mechanically extracted.

---

## 2. Integration Overview

### 2.1 Layered Responsibilities

The five enhancements divide cleanly into three layers:

1. **Author-facing layer** (Phase 1 of #4): the `@monotone` / `@antitone` lint attributes are the only author-visible syntax change in the current phase. No grammar changes; no new keywords; no breaking changes to existing `.alk` source.

2. **Compiler layer** (#1, #2, #3, Phase 2 of #4): three new compiler passes (`schedule_lowering`, `incremental_analysis`, `egraph_optimization`) and one Phase-2 type-system extension. These produce a richer IR (`ScheduledScene` with `AlgorithmIR` + `ScheduleIR` + `DependencyGraph` + monotonicity metadata) that the runtime consumes.

3. **Runtime layer** (#1, #2): the WASM runtime gains a `SignalStore` and a dirty-propagation engine; `render_frame()` is refactored from "rebuild everything" to "check dirty → propagate → re-evaluate dirty → render dirty passes." The GPU backend's dispatch becomes data-driven (reads `ScheduleIR`).

### 2.2 Dependency Graph (Build Order)

```
[#4 Phase 1: Lint]    (parallel; ships any time)
        │
        │ (no dep)
        ▼
[#1 Algorithm/Schedule]   (no dep; foundational)
        │ enables
        ▼
[#2 Incremental]    (depends on #1)
        │ enables
        ▼
[#3 E-Graph]    (depends on #2; transitive on #1)
        │
        │ (no dep)
        ▼
[#4 Phase 2: Type Qualifier]    (prerequisites: ≥3mo Phase 1 + ADR-008/009 amendments)
        │ enables seminaïve in #2
        ▼
[#5 PMT Verification]    (DEFERRED per ADR-028; re-eval criteria must all hold)
```

### 2.3 What Each Enhancement Touches

| Crate | #1 | #2 | #3 | #4 P1 | #4 P2 | #5 |
|-------|----|----|----|-------|-------|----|
| `alkalive-compiler` (lib) | +`schedule.rs`, refactor `ir.rs` | +`incremental.rs` | +`egraph.rs` | +`lints/monotonicity.rs` | + type checker, parser keywords | (deferred) |
| `alkalive-compiler` (CLI) | JSON output schema extended | — | — | `--lint` flag | — | — |
| `alkalive-runtime-wasm` | `build_scene_from_scheduled()` | +`SignalStore`, dirty propagation | — | — | consumes monotone metadata | (deferred) |
| `alkalive-backend-wgpu` | `render_frame()` reads `ScheduleIR` | dirty-pass dispatch | — | — | — | (deferred) |
| `alkalive-render` | receives lowered render-graph IR | — | — | — | — | — |
| `alkalive-text` | — | cached shaping keyed by signal version | — | — | — | — |
| `alkalive-core` | — | possibly `SignalId`, `Version` types | possibly union-find | — | `Monotonicity` enum | — |

### 2.4 What Does NOT Change

To bound the blast radius, the following are explicitly preserved:

- The `.alk` grammar (no new keywords) — except Phase 2 of #4, which is gated by ADR-008 amendment.
- The `#![forbid(unsafe_code)]` guarantee in `alkalive-compiler`, `alkalive-render`, `alkalive-text`, `alkalive-core`. The existing `#![allow(unsafe_code)]` in `alkalive-runtime-wasm` and `alkalive-backend-wgpu` (required for `js_sys::Float32Array::view` and WebGL2 bindings) is unchanged.
- The current Hello World scene (`examples/hello.alk`) compiles and runs unchanged through every enhancement. This is **DECISION A9** in the reconciliation report (an ASSUMPTION carried forward as a contract).
- ADR-013 (no WASM↔DOM boundary in the hot path) is not violated. All incremental computation happens inside the WASM module.
- ADR-018 (5-crate external dependency policy) is not violated. No new external crates are added by #1, #2, #3, #4 Phase 1, or #4 Phase 2. #5 (if re-evaluated) would require an ADR-018 amendment for Z3.

---

## 3. Enhanced Compiler Pipeline

```
                         ┌──────────────────────────┐
                         │  .alk source              │
                         └────────────┬─────────────┘
                                      │
                                      ▼
                         ┌──────────────────────────┐
                         │  lexer                   │  (existing)
                         └────────────┬─────────────┘
                                      │ Vec<Token>
                                      ▼
                         ┌──────────────────────────┐
                         │  parser                  │  (existing)
                         └────────────┬─────────────┘
                                      │ ast::ModuleDecl
                                      ▼
                         ┌──────────────────────────┐
            ╔════════╗   │  type checker            │  (existing; #4 P2 extends)
            ║ #4 P1  ║──▶│  + @monotone lint pass   │  (NEW — Phase 1)
            ╚════════╝   └────────────┬─────────────┘
                                      │ ast::ModuleDecl (linted)
                                      ▼
                         ┌──────────────────────────┐
                         │  codegen (lower)         │  (existing)
                         └────────────┬─────────────┘
                                      │ AlgorithmIR  (refactored from SceneIR)
                                      ▼
            ╔════════╗   ┌──────────────────────────┐
            ║  #1    ║──▶│  schedule_lowering       │  (NEW)
            ╚════════╝   └────────────┬─────────────┘
                                      │ ScheduleIR  (consumes AlgorithmIR)
                                      ▼
                         ┌──────────────────────────┐
                         │  ScheduledScene {        │
                         │    algorithm, schedule   │
                         │  }                       │
                         └────────────┬─────────────┘
                                      │
                                      ▼
            ╔════════╗   ┌──────────────────────────┐
            ║  #2    ║──▶│  incremental_analysis    │  (NEW)
            ╚════════╝   └────────────┬─────────────┘
                                      │ DependencyGraph
                                      ▼
            ╔════════╗   ┌──────────────────────────┐
            ║  #3    ║──▶│  egraph_optimization     │  (NEW — custom, no `egg`)
            ╚════════╝   └────────────┬─────────────┘
                                      │ Optimized DependencyGraph
                                      ▼
                         ┌──────────────────────────┐
                         │  WASM emission           │  (existing)
                         └────────────┬─────────────┘
                                      │ .wasm  (bundles ScheduledScene + DepGraph)
                                      ▼
                         ┌──────────────────────────┐
                         │  Runtime (WASM module)   │
                         │  SignalStore + DepGraph  │  (#2 runtime half)
                         │  → frame loop:           │
                         │    check → propagate →   │
                         │    re-evaluate → render  │
                         └────────────┬─────────────┘
                                      │
                                      ▼
            ╔══════════════════════════════════════╗
            ║  #5 proof_obligation_generation      ║  (DEFERRED per ADR-028)
            ║  → verified .wasm                    ║  Re-evaluate when 4 criteria hold
            ╚══════════════════════════════════════╝
```

**Reading the diagram:** Boxes are compiler stages or runtime components. `╔══╗` annotations mark which idea introduces or modifies the adjacent stage. The bottommost box (#5) is **dashed** to indicate it is deferred per ADR-028; no implementation is planned for the current phase.

---

## 4. Idea #1 — Algorithm/Schedule Separation (ADR-024)

### 4.1 Problem

AlkALive's current `SceneIR` (`crates/alkalive-compiler/src/ir.rs`, lines 22–32) is a flat structure that conflates the scene description (what to render: `module_id`, `module_name`, `background`, `nodes`) with the rendering strategy (how to render it). The conflation is invisible in the IR itself — the IR has no rendering-strategy fields — but it is *enforced* by the runtime: `alkalive-backend-wgpu`'s `render_frame()` hardcodes the rendering pipeline (shape text via HarfRust → rasterize glyphs → build vertex buffer → upload texture → draw triangles). There is no way to change the batching strategy, pass order, or shader selection without modifying runtime code.

This produces three problems (per ADR-024 Context):

1. **Inflexibility:** the same scene cannot be rendered differently on different backends (WebGL2 vs. WebGPU vs. CPU fallback) without code changes.
2. **Performance ceiling:** the runtime cannot reorder passes for GPU efficiency (batch by pipeline, minimize state changes) because the order is implicit in the code structure.
3. **Blocks incremental computation:** ADR-025 requires a clear separation between stable scene data (algorithm) and volatile rendering state (schedule).

### 4.2 Goal

Split the `SceneIR` into two distinct IRs produced by a new `schedule_lowering` compiler pass:

1. **AlgorithmIR** — a pure scene description (nodes, transforms, styles, text content) with no rendering details. Refactored from the current `SceneIR`.
2. **ScheduleIR** — a rendering strategy (pass order, batching, shader selection, threading) that consumes the `AlgorithmIR`. New data structure.

The same `AlgorithmIR` can be paired with different `ScheduleIR`s for different backends or different performance profiles. The runtime's `render_frame()` reads the `ScheduleIR` to determine pass order, shader selection, and batching — replacing hardcoded logic with data-driven dispatch.

### 4.3 Solution

**DECISION (ADR-024):** Add a `schedule_lowering` compiler pass after `codegen` and before WASM emission. The pass takes the current `SceneIR` output and produces a `ScheduledScene { algorithm: AlgorithmIR, schedule: ScheduleIR }`.

**AlgorithmIR** (refactored from current `SceneIR`):
```rust
pub struct AlgorithmIR {
    pub module_id: ModuleId,
    pub module_name: String,
    pub background: (u8, u8, u8),
    pub nodes: Vec<NodeIR>,   // Text, InputField — no rendering details
}
```

**ScheduleIR** (new):
```rust
pub struct ScheduleIR {
    pub passes: Vec<RenderPass>,
    pub pass_order: Vec<usize>,
}

pub struct RenderPass {
    pub node_indices: Vec<usize>,   // Which AlgorithmIR nodes this pass renders
    pub shader: ShaderId,           // Which shader program to use
    pub batching: BatchingStrategy, // ByFontSize, ByTexture, None
    pub thread: ThreadAffinity,     // Main, Worker
}
```

The `schedule_lowering` pass applies default scheduling rules (e.g. "all text nodes go in one pass with the text_quad shader, batched by font size"). **DECISION (ADR-024):** advanced users can override the schedule in `.alk` source.

**ASSUMPTION A1:** the `schedule_lowering` pass lives in `crates/alkalive-compiler/src/schedule.rs` (new module).
**ASSUMPTION A2:** `AlgorithmIR` is a thin refactor of the existing `SceneIR` struct (same fields, no rendering details added). The `module_id`, `module_name`, `background`, `nodes` fields are preserved verbatim.
**ASSUMPTION A3:** `ScheduledScene { algorithm: AlgorithmIR, schedule: ScheduleIR }` is the new compiler output; the existing `compile()` function returns `ScheduledScene` instead of `SceneIR` after ADR-024 lands.
**ASSUMPTION A4:** the runtime's `build_scene_from_ir()` function (currently in `alkalive-runtime-wasm/src/lib.rs`) is renamed `build_scene_from_scheduled()` and consumes `ScheduledScene`.

**Relationship to `alkalive-render`:** `ScheduleIR` (in `alkalive-compiler`) is the *author-facing schedule representation*. The `schedule_lowering` pass additionally lowers it into the existing `alkalive-render` IR types (`PassId`, `AttachmentId`, `DrawCallId`, `PassType`, `AttachmentFormat`, etc., per SPECIFICATION §4.1–§4.7) which are the *runtime / GPU-layer render graph*. **RECOMMENDATION R3:** the lowering boundary is specified in a future rendering-ABI ADR.

### 4.4 Integration

- **Existing compiler stages:** the `codegen` pass currently produces `SceneIR`. After this change, it produces `AlgorithmIR`. The new `schedule_lowering` pass runs after `codegen` and before WASM emission.
- **Runtime:** `render_frame()` reads the `ScheduleIR` to determine pass order, shader selection, and batching. Currently hardcoded logic becomes data-driven.
- **ADR-001 (render-graph IR):** aligns with the existing render-graph IR concept — the `ScheduleIR` is the author-facing schedule representation that lowers into the concrete `alkalive-render` IR (see §4.3 above).
- **ADR-004 (compositor):** the compositor (currently abstract per `alkalive-render`'s `Compositor` trait) consumes the lowered render-graph IR.
- **Other ideas:** this refactoring is a prerequisite for #2 (Incremental Computation) because it separates stable scene data (algorithm) from volatile rendering state (schedule).

**DECISION (ADR-024 Confidence):** **High.** The algorithm/schedule separation pattern is well-established (Halide, VEEE). No external dependencies. No ADR conflicts. The refactoring is mechanical: split a struct, add a pass, update the runtime.
**DECISION (ADR-024 LOC):** ~800–1,200 lines (200 refactor + 300 pass + 300 runtime dispatch + 200–500 tests).

**ASSUMPTION A10:** the current `render_frame()` method signature changes from `render_frame(&scene: &TextSceneData, time: f32)` to `render_frame(&scheduled: &ScheduledScene, &signals: &SignalStore, time: f32)` (or similar) after ADR-024 + ADR-025 land.

---

## 5. Idea #2 — Incremental Computation (ADR-025)

### 5.1 Problem

AlkALive's runtime rebuilds the entire scene on every frame. The `render_frame()` method in `alkalive-backend-wgpu` re-shapes all text, re-rasterizes all glyphs, rebuilds the entire vertex buffer, and re-submits all draw calls — 60 times per second, regardless of whether anything changed. For the current Hello World (12 glyphs), this is tolerable. For a real UI (hundreds of nodes), it will be a performance bottleneck and a battery drain on mobile.

ADR-002 already calls for "per-module dirty-rect invalidation with layout-locality" but provides no implementation mechanism. The current runtime has no dirty tracking, no caching, and no dependency awareness — it is a naive full-rebuild loop.

### 5.2 Goal

**DECISION (ADR-025):** Implement a Salsa/Adapton-style incremental computation system. Reduce per-frame work from O(n) (full rebuild) to O(Δ) (only changed subtrees re-evaluate). Implements ADR-002's dirty-rect invalidation mechanism.

### 5.3 Solution

**Compiler stage:** add an `incremental_analysis` pass after `schedule_lowering`. This pass analyzes the `AlgorithmIR` and `ScheduleIR` to build a `DependencyGraph` — a directed acyclic graph of computations (text shaping, glyph rasterization, vertex buffer construction, draw call submission) with their input/output signal dependencies.

```rust
pub struct DependencyGraph {
    pub nodes: Vec<DepNode>,
}

pub struct DepNode {
    pub computation: ComputationId,  // e.g. "shape_text('Hello')", "rasterize_glyph(H)"
    pub inputs: Vec<DepNodeId>,      // What this computation reads
    pub outputs: Vec<DepNodeId>,     // What this computation writes
    pub version: u64,                // Last-computed version
}
```

**Runtime change:** the WASM runtime maintains a `DependencyGraph` (compiled into the WASM binary) and a `SignalStore` (key-value map of signal values with `u64` version counters). On each frame:

1. **Check for changes:** compare current signal values with the previous frame's. If a signal's value changed, increment its version.
2. **Propagate:** for each changed signal, mark all dependent computations as dirty (transitive closure in the dependency graph).
3. **Re-evaluate:** only dirty computations re-execute. Non-dirty computations return their cached result.
4. **Render:** only re-submit draw calls for passes whose inputs were dirty.

**ASSUMPTION A5:** the `SignalStore` lives in `crates/alkalive-runtime-wasm/src/lib.rs` (or a new `crates/alkalive-runtime/src/signal_store.rs` module), not in `alkalive-backend-wgpu`. Runtime owns signal state; backend remains stateless w.r.t. signals.
**ASSUMPTION A11:** the current `Runtime::time` field (incremented by `1.0 / 60.0` per frame in `start_frame_loop`) becomes a signal source (`signal::time`) in the `SignalStore` after ADR-025. All per-frame inputs should flow through the signal store for uniform dirty tracking.
**ASSUMPTION A12:** the existing `TextSceneData` (in `alkalive-backend-wgpu`) is retained as the renderer's per-frame input; the `SignalStore` produces a fresh `TextSceneData` each frame from dirty signals.
**RECOMMENDATION R2 / OPEN QUESTION Q1:** add an explicit small-scene fallback — when the scene has fewer than N nodes (suggested N = 50, tuned by profiling), the runtime bypasses the `DependencyGraph` and falls back to the existing full-rebuild path. This preserves Hello-World latency while unlocking Δ-scaling for larger scenes.

### 5.4 Integration

- **Existing compiler stages:** the `incremental_analysis` pass runs after `schedule_lowering` and before WASM emission. It augments `ScheduledScene` with a `DependencyGraph`.
- **Runtime:** `render_frame()` is refactored from "rebuild everything" to "check dirty → propagate → re-evaluate dirty → render dirty passes."
- **ADR-002 (per-module dirty-rect invalidation):** this is the implementation mechanism. The dependency graph provides dirty propagation; the `ScheduleIR` provides per-pass granularity.
- **ADR-013 (no DOM hot path):** not violated — all incremental computation happens inside the WASM module, no DOM interaction.
- **Other ideas:** depends on #1 (Algorithm/Schedule Separation). Enables #3 (E-Graph Optimization) to operate on the dependency graph. Phase 2 of #4 (Monotonicity Types) enables seminaïve evaluation in this engine.

**DECISION (ADR-025 Confidence):** **Medium.** Salsa/Adapton is well-studied and proven in non-browser contexts (Rust compiler, IntelliJ). However, its application inside a WASM module with a WebGL2 backend is novel — cache invalidation patterns for GPU resources (textures, buffers) may differ from CPU-only incremental systems. The dependency on ADR-024 (Proposed) adds risk. The implementation will require careful profiling to ensure dependency-tracking overhead does not exceed savings from avoiding redundant work for small scenes.
**DECISION (ADR-025 LOC):** ~1,500–2,500 lines (400 graph + 500 pass + 400–600 runtime + 200–500 cache + 0–500 tests).
**OPEN QUESTION Q2:** should `SignalStore` be a separate crate (`alkalive-signals`) or a module in `alkalive-runtime`?

---

## 6. Idea #3 — E-Graph Optimization (ADR-026)

### 6.1 Problem

Once #2 is in place, the dependency graph of reactive signals may contain inefficiencies:

- **Redundant reads:** the same signal is read by multiple consumers, each triggering a separate dependency check.
- **Redundant writes:** a signal is written, then immediately overwritten, wasting the first write's invalidation work.
- **Suboptimal evaluation order:** consumers may be evaluated before their producers, causing multiple re-evaluations.

Without optimization, these inefficiencies cause unnecessary re-evaluations and cache misses, partially negating the benefits of #2.

### 6.2 Goal

**DECISION (ADR-026):** Apply e-graph (equivalence graph) rewriting to the dependency graph to eliminate redundant signal operations and optimize evaluation order — at compile time, with zero runtime cost.

### 6.3 Solution

**Compiler stage:** add an `egraph_optimization` pass after `incremental_analysis`. The pass:

1. **Builds an e-graph** from the `DependencyGraph`. Each computation becomes an e-node; equivalent computations (same inputs, same operation) are merged into e-classes.
2. **Applies rewrite rules:**
   - `state_store_load_forward`: if `S := v; x := S`, rewrite to `S := v; x := v` (forward the stored value, eliminating the signal read).
   - `dead_store_elimination`: if `S := v1; S := v2` with no read of `S` between the two writes, eliminate the first write.
   - `read_merge`: if two consumers read the same signal, merge the reads into a single cached read.
   - `evaluation_reorder`: topologically sort consumers after producers to minimize re-evaluations.
3. **Extracts the optimized graph** via cost-based extraction (selects the cheapest equivalent form).

```rust
pub struct EGraphOptPass {
    pub rewrites: Vec<RewriteRule>,
}
```

**DECISION (ADR-026 Implementation Choice):** use a **custom lightweight e-graph implementation** (~2,000 LOC) rather than the `egg` crate. Rationale: AlkALive's 5-crate external dependency policy (per ADR-018) is strict; adding `egg` would require an ADR amendment. A minimal e-graph for 4 rewrite rules is tractable in ~2,000 LOC.

**ADR-018 Compliance:** this DECISION is the sole rationale for the custom implementation. The technical complexity (union-find, hash-consing, e-class merging) is not a barrier; the policy constraint is. If, during implementation, the custom e-graph exceeds ~3,000 LOC or fails to converge on the 4 rewrite rules, an ADR amendment must be opened before considering `egg` — **OPEN QUESTION Q3** for the implementation phase.

**ASSUMPTION A6:** the custom e-graph lives in `crates/alkalive-compiler/src/egraph.rs` (new module).
**OPEN QUESTION Q3:** should union-find and hash-consing be standalone reusable modules in `alkalive-core`, or inlined into `egraph.rs`?

### 6.4 Integration

- **Existing compiler stages:** the `egraph_optimization` pass runs after `incremental_analysis` and before WASM emission. It transforms the `DependencyGraph` into an optimized `DependencyGraph`.
- **Runtime:** no runtime change — the optimized dependency graph is used as-is by the incremental computation engine.
- **ADR-001 (render-graph IR):** the e-graph operates on the render-graph IR's dependency structure, which is an extension of the existing abstract render-graph.
- **ADR-018 (5-crate dependency policy):** the decision to use a custom e-graph rather than `egg` is to comply with this ADR.
- **Complexity:** ~2,000 LOC for a minimal e-graph data structure + rewrite rules. This is the most complex of the five enhancements.
- **Other ideas:** depends on #2 (Incremental Computation). The optimized dependency graph benefits #4 Phase 2 because seminaïve evaluation (enabled by monotone collections) is more effective on a clean, redundancy-free graph.

**DECISION (ADR-026 Confidence):** **High.** E-graph optimization is well-established (COW, Cranelift, VUMA). The `state_store_load_forward` rewrite is proven in VUMA. The 4 rewrite rules are clearly defined and their semantics are well-understood. The main risk is implementation complexity, but the ~2,000 LOC estimate is grounded in VUMA's actual implementation size.
**DECISION (ADR-026 LOC):** ~2,000 lines (800 e-graph data structure + 400 rewrite rules + 300 cost-based extraction + 200 pass integration + 300 tests).
**DECISION (ADR-026 Performance Claim):** eliminates 20–50% of redundant signal operations in typical reactive UI code, based on VUMA's `state_store_load_forward` benchmarks.

---

## 7. Idea #4 — Monotonicity Types, Phased (ADR-027)

> **Provenance:** the monotone/antitone distinction originates in Datafun (Arbitman et al.). "Monotonicity Types" is the canonical AlkALive name; Datafun is the academic attribution.

### 7.1 Problem

AlkALive's `.alk` language has no static enforcement of collection mutation semantics. Any collection can be mutated arbitrarily — elements added or removed at any time. In a reactive UI, this is dangerous: removing a child node during a layout pass, or shrinking an event queue during dispatch, causes visual glitches or data loss. These bugs are currently caught only at runtime (panics) or not at all (silent corruption).

### 7.2 Goal

Extend AlkALive's type system with monotonicity annotations that statically guarantee certain collections only grow (monotone) or only shrink (antitone). The type checker rejects illegal operations at compile time, eliminating an entire class of runtime bugs.

**DECISION (ADR-027):** adopt a **two-phase implementation**. Phase 1 ships first as a low-risk lint; Phase 2 upgrades to full type qualifiers after Phase 1 is validated on real code.

### 7.3 Solution — Phase 1: Lint-Based Enforcement

**DECISION (ADR-027 Phase 1):** Implement `@monotone` and `@antitone` as **attributes** on collection declarations. A standalone linter pass (not in the type checker) scans for illegal operations on annotated collections within the same function scope:

- `@monotone` collections reject `.remove()`, `.truncate()`, `.clear()`, `.swap_remove()`, `.drain()`.
- `@antitone` collections reject `.push()`, `.extend()`, `.insert()`, `.append()`.

**Scope (DECISION):** intra-function only. Cannot enforce through function boundaries.
**Output (DECISION):** lint warnings, configurable to errors via `#![deny(monotonicity)]`.
**LOC (DECISION):** ~500–1,000.
**Confidence (DECISION):** Medium-High — lint-based enforcement is well-understood and low-risk.

**ASSUMPTION A7:** the Phase 1 lint pass lives in `crates/alkalive-compiler/src/lints/monotonicity.rs` (new module).
**ASSUMPTION A8:** Phase 1 lint warnings are emitted via a new `LintReport` type alongside the existing `CompileError`. Lints are non-fatal by default; `#![deny(monotonicity)]` makes them fatal.
**OPEN QUESTION Q4:** does Phase 1 lint operate on the AST, the IR, or both? The rough draft implies AST (parser extension); ADR-027 does not specify.
**OPEN QUESTION Q5:** the `#![deny(monotonicity)]` attribute (A8) — is this the first use of file-level lint attributes in `.alk`? If so, ADR-008 (language design) needs an amendment for lint-attribute syntax even in Phase 1.

### 7.4 Solution — Phase 2: Full Type Qualifier System

**DECISION (ADR-027 Phase 2):** Upgrade `monotone` and `antitone` from attributes to first-class type qualifiers in the `.alk` grammar:

- Parser recognizes `monotone`/`antitone` as type qualifiers (not just attributes).
- Type checker verifies monotonicity flows through function signatures: a `monotone` parameter cannot be shrunk inside the function.
- SceneIR carries monotonicity metadata for runtime seminaïve evaluation.
- Enables #2's incremental computation to process only new elements (seminaïve evaluation).

**Scope (DECISION):** full type system integration, function-boundary enforcement, SceneIR metadata.
**LOC (DECISION):** additional ~2,500–4,000 on top of Phase 1.
**Confidence (DECISION):** Medium — requires type-checker extension design; depends on Phase 1 validation.

The Phase 2 SceneIR metadata type (referenced from the rough draft):

```rust
pub enum Monotonicity {
    Monotone,     // Only grows
    Antitone,     // Only shrinks
    Unrestricted, // Default
}
```

**DECISION (ADR-027 Phase 2 Prerequisites):** Phase 2 may begin only after:

1. Phase 1 lint rules are validated on real `.alk` code (≥3 months of usage).
2. The type-checker extension design is reviewed and approved.
3. ADR-008 (language design) is amended to formally include monotonicity qualifiers.
4. ADR-009 (type verification) is amended to add monotonicity as a third verification dimension.

**RECOMMENDATION R4 / OPEN QUESTION:** the Phase 1 → Phase 2 migration path is unspecified. A `migrate-monotonicity` compiler subcommand should rewrite `@monotone X` → `monotone X` and `@antitone X` → `antitone X` in `.alk` source. Both syntaxes accepted during a deprecation window (≥2 minor versions). This is an ASSUMPTION for Phase 2 planning, not a Phase 1 deliverable.

### 7.5 Integration

- **Phase 1:**
  - **ADR-008 (language design):** no grammar change. The `@monotone` / `@antitone` attributes are parsed as attribute syntax (which already exists conceptually for `#![deny(...)]`).
  - **ADR-009 (type verification):** no change. Lint is not type checking.
  - **Existing compiler:** the `alkalive-compiler` crate gains a `lints/` module tree.
  - **Other ideas:** none. Phase 1 is fully parallel to #1, #2, #3.

- **Phase 2:**
  - **ADR-008:** amended to include `monotone`/`antitone` keywords.
  - **ADR-009:** amended to add monotonicity as a third verification dimension.
  - **Existing compiler:** lexer needs `monotone`/`antitone` keywords; parser needs type-qualifier syntax; type checker needs the monotonicity-checking pass.
  - **ADR-002 (dirty-rect invalidation):** monotone collections enable seminaïve evaluation — only new elements are processed on each reactive update, not the entire collection. **DECISION (ADR-027 dependency direction):** Phase 2 metadata **enables** #2's seminaïve evaluation; #2 does not gate Phase 2.
  - **Other ideas:** enables #5 (PMT Verification, deferred) to prove monotonicity properties formally.

---

## 8. Idea #5 — PMT Verification, Deferred (ADR-028)

### 8.1 Problem

AlkALive's current safety guarantee is `#![forbid(unsafe_code)]` — a syntactic check that prevents `unsafe` Rust blocks. This is strong but not formal: array bounds are checked at runtime (panics), not proven at compile time (proofs). For a UI framework aiming to replace HTML/CSS/JS in safety-critical domains, formal verification would provide a higher assurance level.

### 8.2 Goal

**DECISION (ADR-028):** Defer all PMT (Proof-carrying Memory Transactions) verification work. AlkALive's current safety model (`#![forbid(unsafe_code)]` + WASM sandboxing + Rust borrow checker) provides adequate safety for browser-deployed UI. PMT verification is recorded as a **future research direction** with clear re-evaluation criteria.

### 8.3 Solution — Deferral Rationale

**DECISION (ADR-028 Rationale):** the deferral is grounded in five rationales:

1. **Adequate current safety:** WASM sandboxing prevents memory corruption across the sandbox boundary. Rust's `#![forbid(unsafe_code)]` prevents raw pointer arithmetic. The borrow checker prevents use-after-free and data races. For a browser UI framework, this is sufficient.
2. **GPU kernels remain unverified:** VUMA's PMT verification does not cover GPU kernels. AlkALive's WebGL2 shaders would remain unverified regardless of PMT adoption — limiting the value proposition.
3. **Dependency on Monotonicity Types:** the most valuable PMT application is proving monotonicity properties. ADR-027 is itself in a phased adoption — Phase 2 (full type qualifiers) must be stable before PMT can be layered on top.
4. **Cost vs. benefit:** 6–12 months of research effort (10,000+ LOC) is not justified for a browser-deployed UI framework where the runtime safety net is already strong.
5. **ADR-018 compliance:** Z3/Lean are not among AlkALive's 5 allowed external crates. Adding them would require an ADR amendment — a significant policy change for a future research direction.

### 8.4 Solution — Re-Evaluation Criteria

**DECISION (ADR-028 Re-Evaluation Criteria):** PMT verification should be re-evaluated when **all** of the following are true:

1. ADR-027 Phase 2 (full type qualifier system) is implemented and stable for ≥6 months.
2. AlkALive targets a safety-critical domain (medical UI, automotive, aerospace) where formal verification is a regulatory requirement.
3. VUMA's PMT proof layer has demonstrated composability with external compilers (not just VUMA's own pipeline).
4. A cost-benefit analysis shows the formal verification benefit exceeds the 10,000+ LOC implementation cost.

### 8.5 Solution — Conditional Design (Reference Only)

If re-evaluated and approved, **DECISION (ADR-028):** Approach B (Z3-only contracts) is the preferred starting point. It is lighter weight than full PMT and provides contract-level verification without requiring a Lean proof layer.

The conditional design sketch (reference only, not an implementation plan):

1. **Contract syntax (Approach B):** `requires` / `ensures` clauses in `.alk`, discharged by Z3.
2. **Proof obligation generation:** after WASM emission, the compiler generates a proof obligation for every `i32.load` / `i32.store` instruction: "prove that the address is within the allocated region."
3. **Theorem prover backend:** Z3 discharges the obligations. If all obligations are discharged, the WASM binary is "verified."
4. **Proof-carrying code:** the proofs are embedded in a custom section of the WASM binary. The runtime can optionally verify them before execution.

**OPEN QUESTION Q6:** when ADR-028 is re-evaluated, will the Z3 dependency require an ADR-018 amendment, or can Z3 be vendored in-tree (as HarfRust was per ADR-022)?

### 8.6 Integration

- **ADR-009 (type verification):** if pursued later, would add a third verification level (formal proof) on top of the existing two-level model (source-level + WASM validator).
- **Current compiler:** would require a proof-obligation generation pass and a theorem-prover backend. 6–12 month research effort minimum.
- **Dependency on #4 Phase 2:** the most valuable application of PMT is proving monotonicity properties — that a `monotone` collection truly never shrinks, formally verified.
- **No ADR violation:** additive verification layer; does not change the rendering, input, or DOM model.
- **Explicitly deferred:** no implementation planned for the current phase.

**DECISION (ADR-028 Confidence):** **High** in the deferral decision. The rationale is clear, the re-evaluation criteria are well-defined, and the current safety model is adequate for the browser deployment target.
**DECISION (ADR-028 LOC):** 0 (deferral). If re-evaluated: Approach A = 10,000+ LOC; Approach B = 2,000–4,000 LOC.

---

## 9. Cross-Cutting Cross-Reference Matrix

### 9.1 Dependency Matrix

| Idea | Depends On | Enables | Compiler Stage | Confidence | LOC |
|------|------------|---------|----------------|------------|-----|
| #1 Algorithm/Schedule Separation | None | #2 | `schedule_lowering` (after `codegen`) | High | 800–1,200 |
| #2 Incremental Computation | #1 | #3 | `incremental_analysis` (after `schedule_lowering`) | Medium | 1,500–2,500 |
| #3 E-Graph Optimization | #2 (transitive #1) | #4 P2 (better seminaïve) | `egraph_optimization` (after `incremental_analysis`) | High | 2,000 |
| #4 P1 Monotonicity Lint | None | #4 P2 | `lints/monotonicity` (after parser) | Medium-High | 500–1,000 |
| #4 P2 Monotonicity Type Qualifier | #4 P1 + ADR-008/009 amendments | #2 seminaïve, #5 (if re-eval) | Type checker extension | Medium | +2,500–4,000 |
| #5 PMT Verification | #4 P2 stable ≥6mo + 3 other criteria | None (terminal) | `proof_obligation_generation` (post-WASM, **deferred**) | High (in deferral) | 0 (deferred) |

### 9.2 ADR Compliance Matrix

| Idea | ADRs Cited | ADR-018 (5-crate policy) | ADR-013 (no DOM hot path) |
|------|-----------|--------------------------|---------------------------|
| #1 | ADR-001, ADR-004, ADR-024 | Compliant (no new deps) | Compliant |
| #2 | ADR-002, ADR-013, ADR-024, ADR-025 | Compliant (no new deps) | Compliant (WASM-internal) |
| #3 | ADR-001, ADR-018, ADR-024, ADR-025, ADR-026 | Compliant — `egg` excluded, custom impl | N/A (compiler-only) |
| #4 P1 | ADR-027 | Compliant | N/A (compiler-only) |
| #4 P2 | ADR-002, ADR-008, ADR-009, ADR-027 | Compliant | N/A (compiler-only) |
| #5 | ADR-009, ADR-018, ADR-027, ADR-028 | **Would require amendment** if Z3 added | N/A |

### 9.3 What Touches What

| Component | #1 | #2 | #3 | #4 P1 | #4 P2 | #5 |
|-----------|----|----|----|-------|-------|----|
| `.alk` grammar | — | — | — | — | + keywords | (+ contracts, deferred) |
| `lexer.rs` | — | — | — | — | + keywords | — |
| `parser.rs` | — | — | — | — | + qualifier syntax | — |
| `ast.rs` | — | — | — | + attr nodes | + qualifier nodes | — |
| `ir.rs` | refactor → `algorithm.rs` | + `Monotonicity` enum (P2 only) | — | — | + metadata | — |
| `codegen.rs` | output `AlgorithmIR` instead of `SceneIR` | — | — | — | — | — |
| new `schedule.rs` | + pass | — | — | — | — | — |
| new `incremental.rs` | — | + pass | — | — | — | — |
| new `egraph.rs` | — | — | + pass + data structure | — | — | — |
| new `lints/monotonicity.rs` | — | — | — | + lint pass | — | — |
| `main.rs` (CLI) | JSON schema extended | — | — | `--lint` flag | — | — |
| `alkalive-runtime-wasm/lib.rs` | `build_scene_from_scheduled()` | + `SignalStore` + dirty prop | — | — | consumes metadata | — |
| `alkalive-backend-wgpu/lib.rs` | `render_frame()` reads `ScheduleIR` | dirty-pass dispatch | — | — | — | — |
| `alkalive-render/lib.rs` | receives lowered IR | — | — | — | — | — |
| `alkalive-text/lib.rs` | — | cached shaping keyed by version | — | — | — | — |

---

## 10. Glossary of Reconciled Terminology

| Term | Definition | Source |
|------|------------|--------|
| **AlgorithmIR** | Pure scene description (nodes, transforms, styles, text). Refactored from current `SceneIR`. | ADR-024 |
| **ScheduleIR** | Author-facing rendering strategy (pass order, batching, shaders, threading). Lowers into `alkalive-render` IR. | ADR-024, RECOMMENDATION R3 |
| **ScheduledScene** | `{ algorithm: AlgorithmIR, schedule: ScheduleIR }`. New compiler output. | Rough draft §1 (ASSUMPTION A3) |
| **DependencyGraph** | DAG of computations with input/output signal dependencies. Produced by `incremental_analysis`. | ADR-025 |
| **SignalStore** | Runtime key-value map of signal values with `u64` version counters. Lives in `alkalive-runtime-wasm`. | ADR-025, ASSUMPTION A5 |
| **E-Graph** | Equivalence graph: nodes are computations, e-classes merge equivalent computations. Custom impl, no `egg`. | ADR-026 |
| **Rewrite rule** | Pattern → replacement transformation on the e-graph. 4 rules: `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`. | ADR-026 |
| **Monotonicity** | `Monotone` (only grows) / `Antitone` (only shrinks) / `Unrestricted` (default). Phase 2 SceneIR metadata. | ADR-027 |
| **Phase 1 (Monotonicity)** | Lint-based `@monotone` / `@antitone` attributes, intra-function scope, warnings. | ADR-027 |
| **Phase 2 (Monotonicity)** | Full type qualifiers `monotone` / `antitone` in grammar, function-boundary enforcement, SceneIR metadata. | ADR-027 |
| **PMT** | Proof-carrying Memory Transactions. Formal verification approach. **Deferred** per ADR-028. | ADR-028 |
| **Approach A (PMT)** | Full PMT with Lean. ~10,000+ LOC. Rejected. | ADR-028 |
| **Approach B (PMT)** | Z3-only contracts. ~2,000–4,000 LOC. Preferred if re-evaluated. | ADR-028 |
| **Approach C (PMT)** | Defer. 0 LOC. **Current decision.** | ADR-028 |
| **Seminaïve evaluation** | Incremental evaluation strategy that processes only new elements of a monotone collection. Enabled by Phase 2 metadata. | ADR-027 |
| **`schedule_lowering`** | New compiler pass: `SceneIR` (existing codegen output) → `ScheduledScene`. | ADR-024 |
| **`incremental_analysis`** | New compiler pass: `ScheduledScene` → `ScheduledScene` + `DependencyGraph`. | ADR-025 |
| **`egraph_optimization`** | New compiler pass: `DependencyGraph` → optimized `DependencyGraph`. | ADR-026 |
| **`proof_obligation_generation`** | Deferred compiler pass: WASM → verified WASM with embedded proofs. | ADR-028 |
| **Δ-scaling** | Per-frame cost proportional to the number of changed nodes (Δ), not the total node count (n). | ADR-025 |
| **Datafun** | Academic origin of the monotone/antitone distinction. Referenced once in this document as provenance. | RECOMMENDATION R6 |

---

*End of fine-draft-v2.md. For the codebase-grounded analysis of how these designs integrate with the actual `crates/` source, see `docs/technical-specification.md`.*

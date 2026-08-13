# Implementation Plan — Technical Specification Realization

**Date:** 2026-08-12
**Purpose:** Map every requirement from `docs/technical-specification.md` to concrete implementation tasks, define wave order, and set DoDs.

---

## Current State Analysis

**Already implemented (820 tests pass):**
- `alkalive-compiler`: lexer, parser, ast, ir, codegen, main (CLI) — 3-stage pipeline: lex → parse → lower → `SceneIR`
- `alkalive-runtime-wasm`: `start()` entry point, frame loop, input forwarding, click handler, resize listener
- `alkalive-backend-wgpu`: `WgpuRenderer` with WebGL2, shaders, glyph atlas, `render_frame()`, `upload_text_atlas()`, input field rendering (title + input text, separate vertex ranges)
- `alkalive-text`: HarfRust font registry, text shaper, glyph atlas
- `alkalive-render`: Abstract render-graph IR (Backend/RenderLoop/Compositor traits — no concrete impl)
- ADRs 023–028 written and resolved

**NOT yet implemented (from tech spec §4):**
- ADR-024: `schedule.rs`, `ScheduleIR`, `ScheduledScene`, `schedule_lowering` pass, data-driven `render_frame()`
- ADR-025: `incremental.rs`, `DependencyGraph`, `SignalStore`, dirty propagation, cache infrastructure
- ADR-026: `egraph.rs`, custom e-graph, 4 rewrite rules, cost-based extraction
- ADR-027 P1: `lints/` module, `@monotone`/`@antitone` attribute parsing, lint pass
- ADR-027 P2: `typechecker.rs`, `monotone`/`antitone` keywords, type qualifier enforcement (GATED — not implementable now per ADR-027 prerequisites)
- ADR-028: PMT verification (DEFERRED per ADR-028 — no implementation)

---

## Implementation Waves

### Wave 1: ADR-027 Phase 1 — Monotonicity Lint (No dependencies, ships first)

**DoD:** `@monotone`/`@antitone` attributes parsed, lint pass detects illegal operations, `#![deny(monotonicity)]` upgrades warnings to errors, all existing tests pass.

**Tasks:**
1. Add `TokenKind::At` to lexer, recognize `@ident` tokens
2. Add `Attribute` AST node to `ast.rs`
3. Parse `@ident` as attribute in `parser.rs`
4. Create `lints/mod.rs` with `LintReport`, `LintSeverity`, `LintSet`
5. Create `lints/monotonicity.rs` with the lint pass
6. Wire lint pass into `compile()` / `compile_with_lints()`
7. Add `--lint` CLI flag to `main.rs`
8. Write lint tests

### Wave 2: ADR-024 — Algorithm/Schedule Separation

**DoD:** `SceneIR` renamed to `AlgorithmIR`, `ScheduleIR` type created, `schedule_lowering` pass implemented, `compile()` returns `ScheduledScene`, runtime uses `build_scene_from_scheduled()`, `render_frame()` reads `ScheduleIR` for pass order, all tests pass.

**Tasks:**
1. Rename `SceneIR` → `AlgorithmIR` in `ir.rs` (alias for backward compat)
2. Create `schedule.rs` with `ScheduleIR`, `RenderPass`, `BatchingStrategy`, `ThreadAffinity`, `ShaderId`
3. Implement `schedule_lowering()` function
4. Create `ScheduledScene { algorithm, schedule }` type
5. Update `compile()` to return `ScheduledScene`
6. Update `codegen.rs` `lower()` to return `AlgorithmIR`
7. Update `main.rs` CLI JSON output
8. Update runtime `build_scene_from_ir()` → `build_scene_from_scheduled()`
9. Update `render_frame()` to accept `ScheduledScene` and use data-driven dispatch
10. Update all tests

### Wave 3: ADR-025 — Incremental Computation

**DoD:** `DependencyGraph` built by `incremental_analysis` pass, `SignalStore` in runtime with version counters, dirty propagation engine, frame loop uses check→propagate→reevaluate, text-stack instances lifted to long-lived state, small-scene fallback, all tests pass.

**Tasks:**
1. Create `incremental.rs` with `DependencyGraph`, `DepNode`, `ComputationId`, `DepNodeId`
2. Implement `incremental_analysis()` function
3. Create `SignalStore` type in runtime
4. Implement dirty propagation: `check_changes()`, `propagate()`, `reevaluate()`
5. Refactor `Runtime` struct: replace `input_text`/`original_text` with `SignalStore`
6. Update frame loop to use dirty propagation
7. Lift `HarfRustFontRegistry`/`HarfRustTextShaper`/`HarfRustGlyphAtlas` to long-lived `Runtime` state
8. Update `render_frame()` to accept `SignalStore` and use dirty-pass info
9. Implement small-scene fallback (N=50)
10. Update all tests

### Wave 4: ADR-026 — E-Graph Optimization

**DoD:** Custom e-graph data structure, 4 rewrite rules implemented, cost-based extraction, `egraph_optimization` pass wired into compiler, all tests pass.

**Tasks:**
1. Create `egraph.rs` with `ENode`, `EClass`, `EClassId`, `EGraph` (union-find, hash-consing)
2. Implement `EGraph::add()`, `merge()`, `find()`
3. Implement 4 rewrite rules: `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`
4. Implement cost-based extraction
5. Implement `egraph_optimization()` entry point
6. Wire into `compile()` pipeline
7. Write e-graph tests

### Wave 5: Global Integration & Final Verification

**DoD:** Full workspace builds (native + wasm32), all tests pass, traceability matrix complete, no regressions.

**Tasks:**
1. Full workspace build + test
2. WASM build verification
3. Traceability matrix
4. Final QA

---

## What Is NOT Implemented (Per Spec)

- **ADR-027 Phase 2** (type qualifier system): GATED by ADR-027 prerequisites (≥3 months Phase 1 validation, ADR-008/009 amendments). The spec explicitly says "Phase 2 may begin only after" these conditions. We implement Phase 1 only.
- **ADR-028** (PMT verification): DEFERRED per ADR-028. Zero implementation. The spec says "no implementation" (DD9).

These are not gaps — they are explicitly deferred by the ADRs themselves.

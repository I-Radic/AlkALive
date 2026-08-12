# ADRs 023–028 Summary and Impact Assessment

**Date:** 2026-08-12
**Purpose:** Structured summary of each ADR 023–028 with impact assessment for the Technical Specification.

---

## ADR 023: IME Composition via Hidden Input Exception (Approach B)

**File:** `docs/adr/ADR_023_IME_Composition.md`

**Decision:** Adopt a narrowly-scoped hidden `<input>` element as an exception to ADR-020's metadata-only DOM rule, solely for IME composition-event acquisition.

**Context:** Browser IME composition events only fire on focused, editable DOM elements. Without a DOM element, CJK text input is impossible. The EditContext API is experimental (Chrome-only, behind a flag).

**Consequences:**
- Positive: Reuses browser's contractual IME pipeline; shippable today; composition-state-only (not a render target)
- Negative: Formal exception to ADR-020; introduces a narrow DOM surface for input
- Amends: ADR-020 (grants scoped exception for IME composition events only)

**Constraints:** The hidden `<input>` carries composition state only — no text rendering, no UI state, no layout participation. It is the sole DOM input surface.

**Tech Spec Impact:** The tech spec already references ADR-023 in the context of ADR-013 (line 242). The IME bridge is part of the runtime (`alkalive-runtime-wasm/src/lib.rs`'s `setup_input_forwarding()`). No update needed beyond the file path fix.

---

## ADR 024: Algorithm/Schedule Separation for SceneIR

**File:** `docs/adr/ADR_024_algorithm_schedule_separation.md`

**Decision:** Split `SceneIR` into `AlgorithmIR` (pure scene description) and `ScheduleIR` (rendering strategy). Add a `schedule_lowering` compiler pass. Runtime becomes data-driven.

**Context:** Current SceneIR conflates scene description with rendering strategy. The runtime hardcodes the pipeline. No way to change strategy without code changes.

**Consequences:**
- Positive: Same scene renders on different backends; scheduler reorders passes; enables ADR-025
- Negative: New compiler pass; runtime refactor from hardcoded to data-driven

**Constraints:** Depends on nothing. Enables ADR-025. Aligns with ADR-001 (render-graph IR) and ADR-004 (compositor).

**LOC Estimate:** 800–1,200

**Tech Spec Impact:** Fully integrated in §4.1. The key insight (SceneIR is already an AlgorithmIR — it's a rename + new ScheduleIR) is documented. No content update needed beyond file path fix.

---

## ADR 025: Incremental Computation (Salsa/Adapton-Style)

**File:** `docs/adr/ADR_025_incremental_computation.md`

**Decision:** Implement Salsa/Adapton-style dependency tracking. Add `incremental_analysis` compiler pass. Runtime maintains `SignalStore` with version counters. Only dirty computations re-evaluate.

**Context:** Runtime rebuilds entire scene every frame (O(n) per frame, 60fps). No dirty tracking, no caching. ADR-002 calls for dirty-rect invalidation but provides no mechanism.

**Consequences:**
- Positive: O(n)→O(Δ); implements ADR-002; enables ADR-026
- Negative: Adds dependency graph to WASM binary; cache invalidation bugs; Medium confidence

**Constraints:** Depends on ADR-024. Implements ADR-002. Enables ADR-026. ADR-013 not violated (all inside WASM).

**LOC Estimate:** 1,500–2,500

**Tech Spec Impact:** Fully integrated in §4.2. The key observation (no caching of any kind in runtime; `upload_text_atlas()` has primitive 1-bit dirty tracking) is documented. No content update needed.

---

## ADR 026: E-Graph Optimization for Signal Read/Write Patterns

**File:** `docs/adr/ADR_026_egraph_optimization.md`

**Decision:** Add `egraph_optimization` compiler pass with 4 rewrite rules: `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`. Custom lightweight e-graph (~2,000 LOC), no `egg` crate.

**Context:** Once incremental computation exists, dependency graph may contain redundant reads, dead stores, suboptimal evaluation order.

**Consequences:**
- Positive: 20–50% reduction in redundant signal operations; zero runtime cost
- Negative: ~2,000 LOC e-graph infrastructure; adds compilation time

**Constraints:** Depends on ADR-025. ADR-018 compliance (no `egg` dependency — custom implementation). Operates on DependencyGraph, not RenderGraph.

**LOC Estimate:** 2,000

**Tech Spec Impact:** Fully integrated in §4.3. The distinction between DependencyGraph (incremental computation) and RenderGraph (GPU passes) is documented. No content update needed.

---

## ADR 027: Monotonicity Types — Phased Adoption

**File:** `docs/adr/027_monotonicity_types_phased.md`

**Decision:** Two-phase implementation:
- Phase 1: Lint-based enforcement (`@monotone`/`@antitone` attributes, ~500–1,000 LOC, Medium-High confidence)
- Phase 2: Full type qualifier system (`monotone`/`antitone` keywords, +2,500–4,000 LOC, Medium confidence, requires ADR-008/009 amendments)

**Context:** No static enforcement of collection mutation semantics. Removing a child during layout causes glitches. Currently caught only at runtime.

**Consequences:**
- Positive: Phase 1 is low-risk quick win; Phase 2 enables seminaïve evaluation for ADR-025
- Negative: Phase 1 is less powerful (no function-boundary enforcement); Phase 2 requires type-checker extension + ADR amendments

**Constraints:** Parallel to ADR-024. Phase 2 enables seminaïve evaluation in ADR-025. Phase 2 requires ADR-008 and ADR-009 amendments. Enables ADR-028 (PMT).

**LOC Estimate:** 3,000–5,000 (both phases)

**Tech Spec Impact:** Fully integrated in §4.4 (Phase 1) and §4.5 (Phase 2). The phased approach and ADR amendment requirements are documented. No content update needed.

---

## ADR 028: PMT Verification — Deferred (Approach C)

**File:** `docs/adr/ADR_028_pmt_verification_deferred.md`

**Decision:** Defer all PMT verification work. Rely on `#![forbid(unsafe_code)]` + WASM sandboxing. Re-evaluate when: ADR-027 Phase 2 stable ≥6 months, safety-critical domain targeting, VUMA PMT composability demonstrated, cost-benefit positive.

**Context:** Current safety is syntactic (forbid unsafe), not formal (proofs). PMT would add formal verification but requires Lean/Z3 (violates ADR-018), 10,000+ LOC, 6–12 months. GPU kernels remain unverified regardless.

**Consequences:**
- Positive: Zero cost; no new dependencies; does not block other work
- Negative: No formal verification; may miss first-mover advantage

**Constraints:** Depends on ADR-027 Phase 2. If pursued later: Approach B (Z3-only) preferred over full PMT. ADR-018 compliance issue (Z3/Lean not in 5-crate policy).

**LOC Estimate:** 0 (deferred); 10,000+ if pursued

**Tech Spec Impact:** Documented in §4.5 as deferred. Re-evaluation criteria are listed. No content update needed.

---

## Impact Assessment Summary

| ADR | Changes existing ADRs? | Extends existing spec? | New architectural req? | Tech Spec update needed? |
|-----|:---:|:---:|:---:|:---:|
| 023 | Amends ADR-020 | No | No (already integrated) | No (file path fix only) |
| 024 | No | Yes (new compiler pass) | Yes (AlgorithmIR/ScheduleIR) | No (already integrated) |
| 025 | Implements ADR-002 | Yes (new runtime data structures) | Yes (DependencyGraph, SignalStore) | No (already integrated) |
| 026 | No | Yes (new compiler pass) | Yes (e-graph data structure) | No (already integrated) |
| 027 | Phase 2 amends ADR-008, ADR-009 | Yes (new type system features) | Yes (monotonicity qualifiers) | No (already integrated) |
| 028 | No | No (deferred) | No (future research) | No (already integrated) |

**Conclusion:** All ADRs 023–028 are properly integrated into the technical specification's content. The only corrections needed are the 2 file path references identified in Wave 1.

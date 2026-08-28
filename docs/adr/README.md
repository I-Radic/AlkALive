# Architectural Decision Records (ADR)

This directory records the architectural decisions for the AlkALive system (a custom, module- and object-oriented language compiling to WebAssembly with direct WebGPU rendering), derived from the analysis in [`../ROUGH_DRAFT.md`](../ROUGH_DRAFT.md) and grounded in [`../PROBLEM_CATALOG.md`](../PROBLEM_CATALOG.md).

## Files

| File | Description |
|------|-------------|
| [`ADR.md`](ADR.md) | **Consolidated ADR** — all 22 architectural decisions (ADR 001–022) in a single document, each with Context, Decision, Status, Consequences, and Confidence. |
| [`ADR_019_accessibility_deferred.md`](ADR_019_accessibility_deferred.md) | Standalone copy of ADR 019 (Defer Accessibility Bridge — No DOM Mirror). |
| [`ADR_020_metadata_only_dom_layer_for_seo.md`](ADR_020_metadata_only_dom_layer_for_seo.md) | Standalone copy of ADR 020 (Metadata-Only DOM Layer for SEO). |
| [`ADR_021_main_thread_on_demand_wasm_threads_socket_ipc.md`](ADR_021_main_thread_on_demand_wasm_threads_socket_ipc.md) | Standalone copy of ADR 021 (Main Thread + On-Demand WASM Threads with Socket IPC). |
| [`ADR_022_forked_harfrust_text_stack.md`](ADR_022_forked_harfrust_text_stack.md) | Standalone copy of ADR 022 (Forked HarfRust as the In-WASM Text Stack). |
| [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) | ⚠ RESOLVED — superseded by ADR 022. Retained for historical context. |
| [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) | ⚠ RESOLVED — superseded by ADR 021. Retained for historical context. |
| [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) | ⚠ RESOLVED — superseded by ADR 019. Retained for historical context. |
| [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) | ⚠ RESOLVED — superseded by ADR 020. Retained for historical context. |
| [`ADR_023_IME_Composition.md`](ADR_023_IME_Composition.md) | **ADR 023** — IME Composition via Hidden Input Exception (Approach B). Grants a scoped exception to ADR 020 for IME composition-event acquisition. |
| [`ADR_024_algorithm_schedule_separation.md`](ADR_024_algorithm_schedule_separation.md) | **ADR 024** — Algorithm/Schedule Separation for SceneIR. Split SceneIR into AlgorithmIR (what) + ScheduleIR (how). Confidence: High. |
| [`ADR_025_incremental_computation.md`](ADR_025_incremental_computation.md) | **ADR 025** — Incremental Computation (Salsa/Adapton-style). Only dirty subtrees re-evaluate. Implements ADR-002. Confidence: Medium. |
| [`ADR_026_egraph_optimization.md`](ADR_026_egraph_optimization.md) | **ADR 026** — E-Graph Optimization for signal read/write patterns. Compile-time rewrite rules. Confidence: High. |
| [`ADR_027_monotonicity_types_phased.md`](ADR_027_monotonicity_types_phased.md) | **ADR 027** — Monotonicity Types, Phased Adoption. Phase 1: lint-based (Approach B). Phase 2: full type qualifier (Approach A). Confidence: Medium-High (P1), Medium (P2). |
| [`ADR_028_pmt_verification_deferred.md`](ADR_028_pmt_verification_deferred.md) | **ADR 028** — PMT Verification, Deferred (Approach C). Re-evaluate after ADR-027 Phase 2 is stable. Confidence: High (in deferral). |
| [`Decision_Alternatives_Monotonicity_Types.md`](Decision_Alternatives_Monotonicity_Types.md) | ⚠ RESOLVED — superseded by ADR 027. Retained for historical context. |
| [`Decision_Alternatives_PMT_Verification.md`](Decision_Alternatives_PMT_Verification.md) | ⚠ RESOLVED — superseded by ADR 028. Retained for historical context. |
| [`Spec_Tradeoff_Note_IME.md`](Spec_Tradeoff_Note_IME.md) | ⚠ RESOLVED — superseded by ADR 023. Retained for historical context. |

## Decision summary

All 28 ADRs carry the formal status **Proposed** (awaiting ratification) unless their own file says otherwise — ADR.md's status convention keeps ratification separate from implementation, which is tracked in the per-ADR audit subsections, `docs/technical-specification.md`, and `docs/master-backlog.md`. Exceptions as of 2026-08-27: ADR-008 and ADR-009 are **amended by ADR-027 Phase 2**; ADR-026 is **accepted and implemented** (Wave 5 audit); ADR-027 has **Phase 1 and Phase 2 implemented**; ADR-028 is **deferred**. ADRs 001–018 were the original set; ADRs 019–022 are the four project-owner resolutions; ADR 023 resolves the IME composition-event acquisition; ADRs 024–026 are VUMA-inspired compiler enhancements; ADR 027 resolves Monotonicity Types (phased adoption); ADR 028 defers PMT Verification. All Decision Alternative files are now resolved.

| ID | Decision | Confidence | Status |
|----|----------|------------|--------|
| ADR 001 | Render-Graph IR as the Atomic Rendering Unit | High | Proposed |
| ADR 002 | Per-Module Dirty-Rect Invalidation with Layout-Locality | High | Proposed |
| ADR 003 | Single-GPUDevice Render Thread + SAB/COOP-COEP Compositor | Medium | Proposed |
| ADR 004 | Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract | High | Proposed |
| ADR 005 | Object-Owned Per-Instance Styling | High | Proposed |
| ADR 006 | WGSL Shaders as First-Class Styling Primitives | High | Proposed |
| ADR 007 | Single Owned Render-Object Tree (Component = Subtree) | High | Proposed |
| ADR 008 | Statically-Typed Module+OO Language Compiling to WASM | High | Amended by ADR-027 Phase 2 |
| ADR 009 | Two-Level Type Verification | Medium | Amended by ADR-027 Phase 2 |
| ADR 010 | CPU Bounding-Volume Hit-Testing + First-Class Device-Event Input | High | Proposed |
| ADR 011 | Unified Virtual Focus/Accessibility Annotation Layer | High | Proposed |
| ADR 012 | Navigation/URL Contract and Explicit SEO Scope | High | Proposed |
| ADR 013 | No WASM↔DOM Boundary in the Hot Path | High | Proposed |
| ADR 014 | Design-Tool-as-Runtime + Typed Component Testing | Medium | Proposed |
| ADR 015 | HMR via Serialisable Scene-Graph State Rehydration | Medium | Proposed |
| ADR 016 | Unified Author-Owned Trace with Split Determinism | Medium | Proposed |
| ADR 017 | Compiled WASM Binary + WebGPU Pipeline Precompilation | Medium | Proposed |
| ADR 018 | Capability-Scoped Imports + Component-Model Tree-Shaking | Medium | Proposed |
| ADR 019 | Defer Accessibility Bridge — No DOM Mirror | High | Proposed |
| ADR 020 | Metadata-Only DOM Layer for SEO — No UI DOM Interop | High | Proposed |
| ADR 021 | Main Thread + On-Demand WASM Threads with Socket IPC | High | Proposed |
| ADR 022 | Forked HarfRust as the In-WASM Text Shaping/Rasterization Stack | High | Proposed |
| ADR 023 | IME Composition via Hidden Input Exception (Approach B) | High | Proposed |
| ADR 024 | Algorithm/Schedule Separation for SceneIR | High | Proposed |
| ADR 025 | Incremental Computation (Salsa/Adapton-Style) | Medium | Proposed |
| ADR 026 | E-Graph Optimization for Signal Read/Write Patterns | High | Accepted & implemented (Wave 5 audit) |
| ADR 027 | Monotonicity Types — Phased Adoption | Medium-High / Medium | Phase 1 & 2 implemented |
| ADR 028 | PMT Verification — Deferred (Approach C) | High | Proposed (Deferred) |

## Resolved Decision Alternatives

The four Decision Alternative files have been resolved by the project owner's non-negotiable choices and are now superseded by ADRs 019–022. They are retained for historical context only.

| File | Resolved By | Override Note |
|------|-------------|---------------|
| `Decision_Alternatives_text-rendering.md` | ADR 022 | Forked HarfRust (Approach A) overrides prior Approach B (hidden DOM) |
| `Decision_Alternatives_concurrency-scheduling.md` | ADR 021 | Main thread + on-demand WASM threads + socket IPC (new hybrid) overrides prior Approach A (cooperative coroutines) |
| `Decision_Alternatives_accessibility-bridge.md` | ADR 019 | Approach A (no DOM mirror, a11y deferred) overrides prior Approach C (hybrid DOM projection) |
| `Decision_Alternatives_adoption-interop.md` | ADR 020 | Approach C (DOM only for metadata/SEO) overrides prior Approach A (host-DOM interop bridges) |

## Resolved Decision Alternatives (VUMA-Inspired)

Both VUMA-inspired Decision Alternative files are now resolved:

| File | Resolved By | Decision |
|------|-------------|----------|
| `Decision_Alternatives_Monotonicity_Types.md` | ADR 027 | Phased adoption: lint (Phase 1) → type qualifier (Phase 2) |
| `Decision_Alternatives_PMT_Verification.md` | ADR 028 | Defer (Approach C) — re-evaluate after ADR-027 Phase 2 |

## Consistency review

After creating ADRs 019–022, a four-wave sub-agent consistency review identified 8 existing ADRs (001, 003, 004, 007, 011, 012, 013, 017) with stale integration notes. All 8 were **MINOR UPDATEs** (no CONFLICTs); their cross-references and integration notes were updated to reflect the four new decisions, and ADR 011's "DOM projection surface" clause was removed (per ADR 019), ADR 012's abstract SEO taxonomy was concretized (per ADR 020, Confidence raised Medium → High), and ADR 004's "hidden-DOM measurement shim" interim was replaced by the forked HarfRust backend (per ADR 022). No new Decision Alternative files were needed — the system is internally consistent.

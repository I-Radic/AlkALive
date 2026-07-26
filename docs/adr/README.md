# Architectural Decision Records (ADR)

This directory records the architectural decisions for the AlkALive system (a custom, module- and object-oriented language compiling to WebAssembly with direct WebGPU rendering), derived from the analysis in [`../ROUGH_DRAFT.md`](../ROUGH_DRAFT.md) and grounded in [`../PROBLEM_CATALOG.md`](../PROBLEM_CATALOG.md).

## Files

| File | Description |
|------|-------------|
| [`ADR.md`](ADR.md) | **Consolidated ADR** — all 18 architectural decisions (ADR 001–018) in a single document, each with Context, Decision, Status, Consequences, and Confidence. |
| [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) | Low-confidence decision: text rendering strategy (P3.5, decisive). |
| [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) | Low-confidence decision: concurrency/scheduling model (P4.3, multiple viable). |
| [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) | Low-confidence decision: accessibility bridge approach (P6.1, decisive). |
| [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) | Low-confidence decision: adoption/interop strategy (P9.5, strategic). |

## Decision summary

All 18 ADRs are **Proposed** (awaiting ratification). High/Medium-confidence decisions are recorded as standard ADRs in [`ADR.md`](ADR.md); Low-confidence decisions are recorded as Decision Alternatives files (precursors to future ADRs).

| ID | Decision | Confidence |
|----|----------|------------|
| ADR 001 | Render-Graph IR as the Atomic Rendering Unit | High |
| ADR 002 | Per-Module Dirty-Rect Invalidation with Layout-Locality | High |
| ADR 003 | Single-GPUDevice Render Thread + SAB/COOP-COEP Compositor | Medium |
| ADR 004 | Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract | High |
| ADR 005 | Object-Owned Per-Instance Styling | High |
| ADR 006 | WGSL Shaders as First-Class Styling Primitives | High |
| ADR 007 | Single Owned Render-Object Tree (Component = Subtree) | High |
| ADR 008 | Statically-Typed Module+OO Language Compiling to WASM | High |
| ADR 009 | Two-Level Type Verification | Medium |
| ADR 010 | CPU Bounding-Volume Hit-Testing + First-Class Device-Event Input | High |
| ADR 011 | Unified Virtual Focus/Accessibility Annotation Layer | High / Medium |
| ADR 012 | Navigation/URL Contract and Explicit SEO Scope | Medium |
| ADR 013 | No WASM↔DOM Boundary in the Hot Path | High |
| ADR 014 | Design-Tool-as-Runtime + Typed Component Testing | Medium |
| ADR 015 | HMR via Serialisable Scene-Graph State Rehydration | Medium |
| ADR 016 | Unified Author-Owned Trace with Split Determinism | Medium |
| ADR 017 | Compiled WASM Binary + WebGPU Pipeline Precompilation | Medium |
| ADR 018 | Capability-Scoped Imports + Component-Model Tree-Shaking | Medium |

## How these decisions were produced

Four-wave sub-agent orchestration:
1. **Wave 1** — 9 sub-agents extracted decision points (one per rough-draft cluster).
2. **Wave 2** — 22 sub-agents drafted ADRs (High/Medium) or Alternatives (Low), one per decision point.
3. **Wave 3** — 6 pairwise sub-agents checked cross-ADR consistency; corrections applied.
4. **Wave 4** — 5 sub-agents verified every Context against the rough draft; all SUPPORTED.

Every Context is traceable to a `P x.y` problem entry and `[n]` reference in the Problem Catalog. Circumstantial or catalog-acknowledged-gap evidence (G1, G2, G4, G5, G6) is flagged inline. The two decisive hard problems (text rendering, accessibility bridge) and the strategic adoption risk are deliberately kept as Decision Alternatives rather than committed ADRs, reflecting genuine uncertainty requiring further spikes before ratification.

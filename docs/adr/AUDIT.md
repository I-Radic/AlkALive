# ADR Structure Audit

**Date:** 2026-08-12
**Purpose:** Document the actual file structure, numbering, and locations of all ADRs in the AlkALive repository.

---

## Findings

### `docs/adr/ADR.md` (Consolidated File)

Contains **ADRs 001–022 only** (22 ADRs). These are the original architectural decisions plus ADRs 019–022 which were added later but also included in the consolidated file.

**ADRs in ADR.md:**
- ADR 001: Render-Graph IR as the Atomic Rendering Unit
- ADR 002: Per-Module Dirty-Rect Invalidation with Layout-Locality
- ADR 003: Single-GPUDevice Render Thread + SAB/COOP-COEP Compositor
- ADR 004: Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract
- ADR 005: Object-Owned Per-Instance Styling
- ADR 006: WGSL Shaders as First-Class Styling Primitives
- ADR 007: Single Owned Render-Object Tree (Component = Subtree)
- ADR 008: Statically-Typed Module+OO Language Compiling to WASM
- ADR 009: Two-Level Type Verification
- ADR 010: CPU Bounding-Volume Hit-Testing + First-Class Device-Event Input
- ADR 011: Unified Virtual Focus/Accessibility Annotation Layer
- ADR 012: Navigation/URL Contract and Explicit SEO Scope
- ADR 013: No WASm↔DOM Boundary in the Hot Path
- ADR 014: Design-Tool-as-Runtime + Typed Component Testing
- ADR 015: HMR via Serialisable Scene-Graph State Rehydration
- ADR 016: Unified Author-Owned Trace with Split Determinism
- ADR 017: Compiled WASM Binary + WebGPU Pipeline Precompilation
- ADR 018: Capability-Scoped Imports + Component-Model Tree-Shaking
- ADR 019: Defer Accessibility Bridge — No DOM Mirror
- ADR 020: Metadata-Only DOM Layer for SEO — No UI DOM Interop
- ADR 021: Main Thread + On-Demand WASM Threads with Socket IPC
- ADR 022: Forked HarfRust as the In-WASM Text Shaping/Rasterization Stack

### Separate ADR Files (ADRs 019–028)

ADRs 019–022 have **both** a consolidated entry in `ADR.md` **and** a standalone file. ADRs 023–028 exist **only** as standalone files — they are NOT in `ADR.md`.

| ADR # | Title | File Path | Also in ADR.md? |
|-------|-------|-----------|:---:|
| 019 | Defer Accessibility Bridge — No DOM Mirror | `docs/adr/ADR_019_accessibility_deferred.md` | ✅ |
| 020 | Metadata-Only DOM Layer for SEO | `docs/adr/ADR_020_metadata_only_dom_layer_for_seo.md` | ✅ |
| 021 | Main Thread + On-Demand WASM Threads with Socket IPC | `docs/adr/ADR_021_main_thread_on_demand_wasm_threads_socket_ipc.md` | ✅ |
| 022 | Forked HarfRust as the In-WASM Text Stack | `docs/adr/ADR_022_forked_harfrust_text_stack.md` | ✅ |
| 023 | IME Composition via Hidden Input Exception (Approach B) | `docs/adr/ADR_023_IME_Composition.md` | ❌ |
| 024 | Algorithm/Schedule Separation for SceneIR | `docs/adr/ADR_024_algorithm_schedule_separation.md` | ❌ |
| 025 | Incremental Computation (Salsa/Adapton-Style) | `docs/adr/ADR_025_incremental_computation.md` | ❌ |
| 026 | E-Graph Optimization for Signal Read/Write Patterns | `docs/adr/ADR_026_egraph_optimization.md` | ❌ |
| 027 | Monotonicity Types — Phased Adoption | `docs/adr/ADR_027_monotonicity_types_phased.md` | ❌ |
| 028 | PMT Verification — Deferred (Approach C) | `docs/adr/ADR_028_pmt_verification_deferred.md` | ❌ |

### Resolved Decision Alternative Files

| File | Resolved By | Status |
|------|-------------|--------|
| `Decision_Alternatives_text-rendering.md` | ADR 022 | RESOLVED |
| `Decision_Alternatives_concurrency-scheduling.md` | ADR 021 | RESOLVED |
| `Decision_Alternatives_accessibility-bridge.md` | ADR 019 | RESOLVED |
| `Decision_Alternatives_adoption-interop.md` | ADR 020 | RESOLVED |
| `Decision_Alternatives_Monotonicity_Types.md` | ADR 027 | RESOLVED |
| `Decision_Alternatives_PMT_Verification.md` | ADR 028 | RESOLVED |
| `Spec_Tradeoff_Note_IME.md` | ADR 023 | RESOLVED |

### Key Finding

**`docs/adr/ADR.md` contains ADRs 001–022 only.** ADRs 023–028 are in separate standalone files. Any reference in the technical specification or other documentation that claims `ADR.md` contains ADRs 001–028 is **incorrect**.

# LOC Estimation — VUMA-Inspired Compiler Enhancements

**Date:** 2026-08-02
**Purpose:** Summarize per-idea and total LOC estimates for the five adopted VUMA-inspired enhancements.

---

## Estimation Methodology

Estimates are based on:

1. **Existing codebase size:** The `alkalive-compiler` crate is ~1,200 LOC (lexer + parser + codegen + CLI). The `alkalive-backend-wgpu` crate is ~1,300 LOC. The `alkalive-runtime-wasm` crate is ~450 LOC. New features proportional to these sizes.

2. **VUMA reference implementation:** VUMA's e-graph is 3,235 LOC; VUMA's PMT Lean spec is 82 files / 280 theorems. These provide grounded reference points.

3. **Previous agent estimates:** The feasibility assessment and rough draft provide per-idea estimates that are cross-checked against the codebase.

4. **Complexity assessment:** Each estimate is broken into sub-components (data structures, compiler passes, runtime changes, tests) with individual LOC ranges.

5. **Exclusion of test code:** Estimates include test code where noted, but test LOC is typically 20–30% of implementation LOC and varies by testing depth.

---

## Per-Idea Estimates

### ADR-024: Algorithm/Schedule Separation (High confidence)

| Component | LOC (low) | LOC (high) |
|-----------|-----------|------------|
| Refactor SceneIR → AlgorithmIR + ScheduleIR | 200 | 300 |
| `schedule_lowering` compiler pass | 300 | 400 |
| Runtime data-driven dispatch | 300 | 400 |
| Tests + integration | 0 | 100 |
| **Subtotal** | **800** | **1,200** |

### ADR-025: Incremental Computation (Medium confidence)

| Component | LOC (low) | LOC (high) |
|-----------|-----------|------------|
| DependencyGraph data structure + serialization | 400 | 500 |
| `incremental_analysis` compiler pass | 500 | 600 |
| Runtime SignalStore + dirty propagation | 400 | 600 |
| Cache infrastructure (text, atlas, vertex buffer) | 200 | 500 |
| Tests + integration | 0 | 300 |
| **Subtotal** | **1,500** | **2,500** |

### ADR-026: E-Graph Optimization (High confidence)

| Component | LOC (low) | LOC (high) |
|-----------|-----------|------------|
| E-graph data structure (e-node, e-class, union-find) | 800 | 800 |
| Rewrite rules (4 rules + pattern matching) | 400 | 400 |
| Cost-based extraction | 300 | 300 |
| `egraph_optimization` pass integration | 200 | 200 |
| Tests + integration | 300 | 300 |
| **Subtotal** | **2,000** | **2,000** |

### Decision_Alternatives: Monotonicity Types (Unresolved — phased)

| Component | Phase 1 (lint) | Phase 2 (type qualifier) |
|-----------|-----------------|--------------------------|
| Lexer keywords + parser syntax | 0 | 500 |
| Linter pass | 500 | 0 |
| Type-checker monotonicity pass | 0 | 1,500 |
| SceneIR metadata | 0 | 300 |
| Runtime seminaïve evaluation | 0 | 700 |
| Tests + integration | 0 | 500 |
| **Subtotal** | **500** | **3,500** |
| **Total (both phases)** | **3,000** | **5,000** |

### Decision_Alternatives: PMT Verification (Deferred — research)

| Component | LOC |
|-----------|-----|
| Not implementable now — 6–12 month research effort | 10,000+ |
| **Subtotal** | **10,000+ (deferred)** |

---

## Total Summary

| Idea | Confidence | LOC (low) | LOC (high) | Implementable now? |
|------|-----------|-----------|------------|:---:|
| ADR-024 Algorithm/Schedule Separation | High | 800 | 1,200 | ✅ |
| ADR-025 Incremental Computation | Medium | 1,500 | 2,500 | ✅ (depends on 024) |
| ADR-026 E-Graph Optimization | High | 2,000 | 2,000 | ✅ (depends on 025) |
| Monotonicity Types | Unresolved | 3,000 | 5,000 | ✅ (phased) |
| PMT Verification | Deferred | — | 10,000+ | ❌ (research) |
| **Total (excl. PMT)** | | **7,300** | **10,700** | |
| **Total (incl. PMT)** | | **17,300+** | **20,700+** | |

---

## Implementation Order

The recommended implementation order (based on dependency chain):

1. **ADR-024** (Algorithm/Schedule Separation) — no dependencies, ~1 month
2. **Monotonicity Types Phase 1** (lint-based) — parallel to #1, ~2 weeks
3. **ADR-025** (Incremental Computation) — depends on #1, ~2 months
4. **ADR-026** (E-Graph Optimization) — depends on #3, ~1.5 months
5. **Monotonicity Types Phase 2** (type qualifier) — depends on lint validation, ~2 months
6. **PMT Verification** — deferred, re-evaluate after #5

**Total estimated timeline (excl. PMT):** ~7–9 months for a single engineer, ~4–5 months for a 2-person team.

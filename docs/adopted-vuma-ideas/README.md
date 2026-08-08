# Adopted VUMA-Inspired Compiler Enhancements

This directory contains the design documents for five compiler enhancements inspired by the VUMA feasibility study. These are **adopted ideas** — they enhance AlkALive's own compiler pipeline without introducing a dependency on VUMA, WOMB, VEEE, or any external runtime.

## Background

A feasibility study (`external-research/feasibility-assessment.md`) concluded that VUMA cannot serve as AlkALive's kernel due to missing browser GPU support, missing browser IME/a11y, and a non-existent UI engine. However, the study identified five programming-language and compiler ideas from VUMA/VEEE that are valuable as design references:

1. **Incremental computation** (Salsa/Adapton) — recompute only what changed
2. **Monotonicity types** (Datafun) — compile-time enforcement of collection growth
3. **E-graph optimization** — optimize signal read/write patterns
4. **PMT verification** — formal memory-safety proofs (future research)
5. **Algorithm/schedule separation** (Halide) — decouple scene description from rendering strategy

## Documents

| File | Purpose |
|------|---------|
| `source-summary.md` | Excerpts from the VUMA feasibility study relevant to the five ideas |
| `problem-catalog.md` | For each idea: the problem it solves, why it matters, how it works, integration constraints |
| `rough-draft.md` | For each idea: Problem → Goal → Solution → Integration design, with compiler pipeline diagram |

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

## Build Order

1. Algorithm/Schedule Separation (no dependencies)
2. Incremental Computation (depends on #1)
3. E-Graph Optimization (depends on #2)
4. Monotonicity Types (parallel to #1)
5. PMT Verification (depends on #4, future research)

## ADR Compliance

All five ideas are compliant with existing ADRs. See the problem catalog's ADR compliance summary table for details.

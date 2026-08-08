# Ideas Summary — VUMA-Inspired Compiler Enhancements

**Source:** `docs/adopted-vuma-ideas/rough-draft.md`
**Purpose:** Compact reference for ADR drafting.

---

## 1. Algorithm/Schedule Separation for SceneIR

**Problem:** The current SceneIR conflates scene description (what to render) with rendering strategy (how to render). The runtime hardcodes the pipeline: shape → rasterize → build vertices → upload → draw. No way to change strategy without code changes.

**Solution:** Split SceneIR into AlgorithmIR (pure scene description) and ScheduleIR (rendering strategy: pass order, batching, shader selection, threading). Add a `schedule_lowering` compiler pass between `codegen` and WASM emission. The same AlgorithmIR pairs with different ScheduleIRs for different backends.

**LOC estimate:** ~800–1,200 (refactor SceneIR struct, add ScheduleIR, add schedule_lowering pass, update runtime to be data-driven)

---

## 2. Incremental Computation (Salsa/Adapton)

**Problem:** The runtime rebuilds the entire scene on every frame — O(n) in total nodes, 60 times per second, regardless of changes. For complex UIs this becomes a bottleneck.

**Solution:** Add an `incremental_analysis` compiler pass that builds a dependency graph of computations. The runtime maintains a `SignalStore` with version counters; on each frame, only dirty computations (transitive closure of changed inputs) re-evaluate. Reduces per-frame work from O(n) to O(Δ).

**LOC estimate:** ~1,500–2,500 (dependency graph data structure, incremental_analysis pass, runtime SignalStore + dirty propagation, cache infrastructure)

---

## 3. E-Graph Optimization for Signal Read/Write Patterns

**Problem:** Once incremental computation exists, the dependency graph may contain redundancies: duplicate signal reads, dead stores, suboptimal evaluation order — causing unnecessary re-evaluations.

**Solution:** Add an `egraph_optimization` compiler pass that builds an e-graph from the dependency graph and applies rewrite rules: `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`. Extracts the optimized graph via cost-based extraction.

**LOC estimate:** ~2,000 (e-graph data structure, rewrite rules, extraction algorithm, integration with dependency graph)

---

## 4. Monotonicity Types (Datafun)

**Problem:** The `.alk` language has no static enforcement of collection mutation semantics. Any collection can shrink at any time — dangerous in reactive UI where removing a child during layout causes glitches.

**Solution:** Extend the `.alk` grammar with `monotone` and `antitone` type qualifiers. The type checker rejects illegal operations (`monotone_set.remove()`). The SceneIR carries monotonicity metadata enabling seminaïve evaluation (only process new elements).

**LOC estimate:** ~3,000–5,000 (lexer keywords, parser type-qualifier syntax, type-checker monotonicity pass, SceneIR metadata, runtime seminaïve evaluation)

---

## 5. PMT Verification (Future Research Direction)

**Problem:** AlkALive's safety guarantee is `#![forbid(unsafe_code)]` — syntactic, not formal. Array bounds are runtime-checked (panics), not compile-time-proven. For safety-critical domains, formal verification would provide higher assurance.

**Solution (future):** The compiler would emit proof obligations for every memory access in generated WASM. A Lean or Z3 backend discharges them at compile time. The WASM binary carries proofs (proof-carrying code). This is explicitly a 6–12 month research effort, not an MVP feature.

**LOC estimate:** Research phase — not implementable now. Estimated 10,000+ LOC if pursued (proof obligation generation, theorem-prover backend, proof serialization).

---

## Dependency Chain

```
#1 Algorithm/Schedule Separation (no deps) → enables #2
#2 Incremental Computation (depends on #1) → enables #3
#3 E-Graph Optimization (depends on #2)
#4 Monotonicity Types (parallel to #1) → enables #5
#5 PMT Verification (depends on #4, future research)
```

## Total LOC Estimate

| Idea | LOC (low) | LOC (high) |
|------|-----------|------------|
| #1 Algorithm/Schedule Separation | 800 | 1,200 |
| #2 Incremental Computation | 1,500 | 2,500 |
| #3 E-Graph Optimization | 2,000 | 2,000 |
| #4 Monotonicity Types | 3,000 | 5,000 |
| #5 PMT Verification | — | (future research) |
| **Total (excluding #5)** | **7,300** | **10,700** |

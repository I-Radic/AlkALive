# Source Summary — VUMA-Inspired Ideas for AlkALive

**Date:** 2026-08-02
**Purpose:** Reference excerpts from the VUMA feasibility study relevant to the five adopted ideas.

---

## Source Documents

| Document | Lines | Location |
|----------|-------|----------|
| Feasibility Assessment | 260 | `external-research/feasibility-assessment.md` |
| VUMA Recommendation | 55 | `VUMA_RECOMMENDATION.md` |
| Individual Analysis | 149 | `external-research/individual-analysis.md` |

---

## 1. Incremental Computation (Salsa/Adapton)

**From feasibility assessment (line 220):**
> Adopt incremental computation (Salsa-style) in `alkalive-compiler` — signal-change-driven reactivity without virtual-DOM diffing

**From individual analysis (line 32):**
> Incremental computation and monotonicity types (principled alternatives to virtual-DOM diffing)

**From VUMA recommendation (line 45):**
> Study incremental computation (Salsa-style) for AlkALive's reactivity model

**Key concept:** Salsa/Adapton-style incremental computation tracks which inputs each computation depends on. When an input changes, only the transitive closure of dependent computations re-evaluate — not the entire scene graph. This replaces whole-scene rebuilds with surgical updates.

---

## 2. Monotonicity Types (Datafun)

**From feasibility assessment (line 221):**
> Adopt monotonicity types for AlkALive's module system — compile-time enforcement of "this collection only grows"

**From individual analysis (line 14):**
> Reactive UI DSL built on four pillars: ... monotonicity types (Datafun)

**From VUMA recommendation (line 46):**
> Study monotonicity types for compile-time collection-growth enforcement

**Key concept:** Datafun's monotonicity types distinguish collections that only grow (`monotone set<T>`) from those that only shrink (`antitone set<T>`). The type checker rejects operations that violate the monotonicity constraint at compile time. In UI context: node children, event queues, and style lists should be monotone — accidental removals cause visual glitches.

---

## 3. E-Graph Optimization for Signal Read/Write Patterns

**From feasibility assessment (line 222):**
> Adopt e-graph optimization with `state_store_load_forward` — optimize signal read/write patterns

**From individual analysis (line 31):**
> E-graph optimization with `state_store_load_forward`

**From VUMA recommendation (line 47):**
> Study e-graph optimization for signal read/write pattern optimization

**Key concept:** An e-graph (equivalence graph) represents all possible ways to compute the same value. The `state_store_load_forward` rewrite eliminates redundant signal reads by forwarding stored values. Applied to AlkALive's reactive dependency graph, this could merge duplicate signal reads, eliminate dead stores, and reorder evaluations for fewer cache misses.

---

## 4. PMT (Proof-carrying Memory Transactions) Verification

**From feasibility assessment (line 223):**
> Study PMT verification as a future direction — if AlkALive ever needs formal memory-safety proofs, VUMA's Lean approach is the reference

**From individual analysis (line 29):**
> Formal PMT memory-safety verification (Lean, 280 theorems, 0 sorries)

**From VUMA recommendation (line 48):**
> Study PMT verification as a future direction if formal memory safety becomes a requirement

**Key concept:** PMT is a formal memory model where every Load/Store operation carries a proof that the access is in-bounds. The proof is discharged at compile time by a theorem prover (Lean/Z3). AlkALive currently uses `#![forbid(unsafe_code)]` as its safety guarantee; PMT would add formal, machine-checked proofs on top.

---

## 5. Algorithm/Schedule Separation for SceneIR

**From feasibility assessment (line 224):**
> Study VEEE's algorithm/schedule separation — AlkALive's SceneIR could benefit from separating "what to render" from "how to render it"

**From individual analysis (line 14):**
> Reactive UI DSL built on four pillars: ... algorithm/schedule separation (Halide)

**From VUMA recommendation (line 49):**
> Study algorithm/schedule separation for AlkALive's SceneIR

**Key concept:** Halide separates the algorithm (what to compute) from the schedule (when and how to compute it). Applied to SceneIR: the "algorithm" describes the scene tree (nodes, transforms, styles), while the "schedule" describes the execution strategy (pass order, batching, GPU vs CPU, parallelization). This allows the same scene to be rendered differently on different hardware without changing the scene description.

---

## AlkALive Architecture Context

- **Compiler pipeline:** `.alk` source → lexer → parser → AST → codegen → SceneIR (JSON)
- **Runtime:** SceneIR → WASM runtime → WebGL2 GPU rendering → canvas
- **Key ADRs:** ADR-003 (module model), ADR-013 (no DOM hot path), ADR-020 (metadata-only DOM), ADR-022 (HarfRust), ADR-023 (IME hidden input)
- **Current SceneIR:** Static JSON describing text nodes, input fields, colors, positions, rotation speeds — no reactivity, no scheduling, no incremental computation
- **Current rendering:** Full scene rebuild every frame (no dirty tracking, no incremental updates)

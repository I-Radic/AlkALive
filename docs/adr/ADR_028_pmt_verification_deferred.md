# ADR 028: PMT Verification — Deferred (Approach C)

> **Supersedes:** `Decision_Alternatives_PMT_Verification.md` (resolved)
> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-028). This standalone file is provided for direct linking.

## Context

AlkALive's current safety guarantee is `#![forbid(unsafe_code)]` — a syntactic check that prevents `unsafe` Rust blocks. This is strong but not formal: array bounds are checked at runtime (panics), not proven at compile time (proofs).

The `Decision_Alternatives_PMT_Verification.md` file explored three approaches:
- **Approach A (full PMT integration):** Compiler emits proof obligations for every WASM `i32.load`/`i32.store`; Lean/Z3 discharges them. ~10,000+ LOC, 6–12 months.
- **Approach B (Z3-only contracts):** `requires`/`ensures` clauses in `.alk`, discharged by Z3. ~2,000–4,000 LOC.
- **Approach C (defer):** Do not implement now. Rely on `#![forbid(unsafe_code)]` + WASM sandboxing. Re-evaluate later.

After analysis, the decision is to **defer** PMT verification (Approach C).

## Decision

**Defer all PMT verification work.** AlkALive's current safety model (`#![forbid(unsafe_code)]` + WASM sandboxing + Rust borrow checker) provides adequate safety for browser-deployed UI. PMT verification is recorded as a **future research direction** with clear re-evaluation criteria.

## Rationale

1. **Adequate current safety:** WASM sandboxing prevents memory corruption across the sandbox boundary. Rust's `#![forbid(unsafe_code)]` prevents raw pointer arithmetic. The borrow checker prevents use-after-free and data races. For a browser UI framework, this is sufficient.

2. **GPU kernels remain unverified:** VUMA's PMT verification does not cover GPU kernels. AlkALive's WebGL2 shaders would remain unverified regardless of PMT adoption — limiting the value proposition.

3. **Dependency on Monotonicity Types:** The most valuable PMT application is proving monotonicity properties. ADR-027 (Monotonicity Types) is itself in a phased adoption — Phase 2 (full type qualifiers) must be stable before PMT can be layered on top.

4. **Cost vs. benefit:** 6–12 months of research effort (10,000+ LOC) is not justified for a browser-deployed UI framework where the runtime safety net is already strong.

5. **ADR-018 compliance:** Z3/Lean are not among AlkALive's 5 allowed external crates. Adding them would require an ADR amendment — a significant policy change for a future research direction.

## Re-Evaluation Criteria

PMT verification should be re-evaluated when **all** of the following are true:

1. ADR-027 Phase 2 (full type qualifier system) is implemented and stable for ≥6 months
2. AlkALive targets a safety-critical domain (medical UI, automotive, aerospace) where formal verification is a regulatory requirement
3. VUMA's PMT proof layer has demonstrated composability with external compilers (not just VUMA's own pipeline)
4. A cost-benefit analysis shows the formal verification benefit exceeds the 10,000+ LOC implementation cost

If re-evaluated, **Approach B (Z3-only contracts)** is the recommended starting point — it is lighter weight than full PMT and provides contract-level verification without requiring a Lean proof layer.

## Alternatives (Brief)

- **Approach A (full PMT):** Rejected for now — 10,000+ LOC, 6–12 months, requires Lean/Z3 dependency, GPU kernels remain unverified.
- **Approach B (Z3-only contracts):** Considered but deferred — lighter weight but still requires Z3 dependency and is only as strong as user-authored contracts. Re-evaluate first if PMT is pursued.

## Status

Proposed (Deferred).

## Consequences

- **Positive.** Zero implementation cost. No new dependencies. Does not block other enhancements. Allows time for VUMA's PMT to mature.
- **Negative.** AlkALive remains at the same safety level as any Rust/WASM project. No formal verification of reactive UI correctness. May miss a window to be the first formally-verified UI framework.
- **Cross-references.** Depends on ADR-027 (Monotonicity Types) — PMT's most valuable application is proving monotonicity properties. ADR-009 (type verification) — if pursued later, would add a third verification level. ADR-018 (5-crate policy) — Z3/Lean are not among the allowed crates.

## Confidence

**High** (in the deferral decision). The rationale is clear, the re-evaluation criteria are well-defined, and the current safety model is adequate for the browser deployment target.

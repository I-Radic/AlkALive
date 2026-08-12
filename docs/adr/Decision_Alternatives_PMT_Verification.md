# Decision Alternatives: PMT Verification for AlkALive

> **⚠ RESOLVED — superseded by [ADR 028](ADR_028_pmt_verification_deferred.md).** The decision is to defer PMT verification (Approach C). This file is retained for historical context.

## Context

AlkALive's current safety guarantee is `#![forbid(unsafe_code)]` — a syntactic check that prevents `unsafe` Rust blocks. This eliminates raw pointer arithmetic and unchecked array access at the Rust level, but it is a **syntactic** guarantee, not a **semantic** one:

- Array indexing in safe Rust still panics at runtime if out of bounds — not proven safe at compile time
- Logical errors (use-after-free, data races) are prevented by the borrow checker, but there is no formal proof that the borrow checker's rules are sufficient for all cases
- The WASM linear memory is a flat byte array; all safety depends on Rust's type system with no independent verification

The VUMA feasibility study (`external-research/feasibility-assessment.md` §5, "PMT Verification") identified VUMA's PMT (Proof-carrying Memory Transactions) model as a future research direction. VUMA's PMT is formally specified in Lean (280 theorems, 0 sorries) and discharged by Z3 at compile time.

## The Uncertainty

The key question is: **should AlkALive invest in formal verification now, or defer it?**

There are three possible paths, but the uncertainty is high enough that none can be committed to as a standard ADR at this time.

## Approach A: Full PMT Integration (VUMA-Style)

**Description:** The compiler emits proof obligations for every `i32.load`/`i32.store` instruction in the generated WASM. A Lean or Z3 backend discharges the obligations at compile time. The WASM binary carries proofs as metadata (proof-carrying code). The runtime optionally verifies proofs before execution.

**Pros:**
- Machine-checked formal memory safety — highest assurance level
- No existing UI framework offers this — AlkALive would be unique
- Proofs are independent of the compiler (defense in depth)

**Cons:**
- 6–12 month research effort minimum (estimated 10,000+ LOC)
- Requires Lean or Z3 as a build dependency (violates ADR-018's 5-crate policy unless Z3 is already counted)
- The PMT arena model (single bump allocator) may conflict with AlkALive's existing memory model (WASM linear memory, `Vec<u8>` framebuffer)
- VUMA's PMT verification does not cover GPU kernels — AlkALive's WebGL2 shaders would remain unverified
- No proven benefit for a browser-deployed UI framework (the runtime safety net is already strong via WASM sandboxing + Rust's `forbid(unsafe_code)`)

**LOC estimate:** 10,000+ LOC (research phase, not implementable now)

## Approach B: Lightweight Contract Checking (Z3-Only)

**Description:** Instead of full PMT, add Z3-discharged `requires`/`ensures` contracts to AlkALive's `.alk` language (similar to VUMA transforms). The type checker generates proof obligations for user-authored contracts (e.g., `requires index < len`), and Z3 discharges them. No memory-model formalization; no Lean.

**Pros:**
- Lighter weight than full PMT (~2,000–4,000 LOC)
- Z3 is already a well-known dependency in the Rust ecosystem
- User-authored contracts are opt-in — no overhead for code without contracts
- Provides formal verification of monotonicity properties (depends on Decision_Alternatives_Monotonicity_Types.md)

**Cons:**
- Still requires Z3 as a build dependency
- Only as strong as the contracts users write — no verification of code without contracts
- Does not cover memory safety (only contract correctness)
- Z3 solver timeouts on complex proofs are a usability problem

**LOC estimate:** 2,000–4,000 LOC

## Approach C: Defer — Study and Re-evaluate

**Description:** Do not implement formal verification now. Monitor VUMA's PMT progress. If VUMA's Lean proof layer proves composable with AlkALive's compiler, re-evaluate at a future milestone. In the meantime, `#![forbid(unsafe_code)]` + WASM sandboxing provides adequate safety for browser-deployed UI.

**Pros:**
- Zero implementation cost
- No new dependencies
- Does not block other enhancements
- Allows time for VUMA's PMT to mature and prove its composability

**Cons:**
- AlkALive remains at the same safety level as any Rust/WASM project
- No formal verification of reactive UI correctness
- May miss a window to be the first formally-verified UI framework

**LOC estimate:** 0 LOC (deferred)

## Recommendation

**Approach C (Defer)** is recommended. The rationale:

1. The feasibility assessment explicitly recommends "study" not "implement" for PMT.
2. PMT depends on Monotonicity Types (Decision_Alternatives_Monotonicity_Types.md), which itself is unresolved.
3. The 6–12 month research effort is not justified for a browser-deployed UI framework where WASM sandboxing + `#![forbid(unsafe_code)]` already provides strong safety.
4. VUMA's PMT verification does not cover GPU kernels — AlkALive's WebGL2 shaders would remain unverified, limiting the value proposition.
5. If AlkALive later targets safety-critical domains (medical, automotive, aerospace), re-evaluate Approach B (Z3-only contract checking) as a lighter-weight alternative to full PMT.

## Dependency

- Depends on Decision_Alternatives_Monotonicity_Types.md being resolved first — the most valuable PMT application is proving monotonicity properties.
- If Approach B is pursued later, it would require an ADR amending ADR-009 (type verification) to add a third verification level (formal proof).

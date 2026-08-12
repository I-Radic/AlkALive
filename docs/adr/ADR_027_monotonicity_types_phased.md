# ADR 027: Monotonicity Types for AlkALive — Phased Adoption

> **Supersedes:** `Decision_Alternatives_Monotonicity_Types.md` (resolved)
> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-027). This standalone file is provided for direct linking.

## Context

AlkALive's `.alk` language has no static enforcement of collection mutation semantics. Any collection can be mutated arbitrarily — elements added or removed at any time. In a reactive UI, this is dangerous: removing a child node during a layout pass, or shrinking an event queue during dispatch, causes visual glitches or data loss.

The `Decision_Alternatives_Monotonicity_Types.md` file explored three approaches:
- **Approach A (full type qualifier):** `monotone`/`antitone` as first-class type qualifiers in the grammar, enforced by the type checker, flowing through function signatures. ~3,000–5,000 LOC.
- **Approach B (lint-based):** `@monotone`/`@antitone` as attributes checked by a linter pass. ~500–1,000 LOC.
- **Approach C (runtime assertions):** `monotone_vec!()` macro with runtime panics. ~200–400 LOC.

After analysis, the decision is to adopt a **phased approach**: start with Approach B (lint-based, quick win), then upgrade to Approach A (full type qualifier) after the lint rules are validated on real code.

## Decision

Adopt a **two-phase implementation**:

### Phase 1: Lint-Based Enforcement (Immediate)

Implement `@monotone` and `@antitone` as attributes on collection declarations. A linter pass (standalone, not in the type checker) scans for illegal operations on annotated collections within the same function scope:
- `@monotone` collections reject `.remove()`, `.truncate()`, `.clear()`, `.swap_remove()`, `.drain()`
- `@antitone` collections reject `.push()`, `.extend()`, `.insert()`, `.append()`

**Scope:** Intra-function only. Cannot enforce through function boundaries.
**Output:** Lint warnings (configurable to errors via `#![deny(monotonicity)]`).
**LOC:** ~500–1,000.
**Confidence:** Medium-High — lint-based enforcement is well-understood and low-risk.

### Phase 2: Full Type Qualifier System (After Phase 1 Validation)

Upgrade `monotone` and `antitone` from attributes to first-class type qualifiers in the `.alk` grammar:
- Parser recognizes `monotone`/`antitone` as type qualifiers (not just attributes)
- Type checker verifies monotonicity flows through function signatures: a `monotone` parameter cannot be shrunk inside the function
- SceneIR carries monotonicity metadata for runtime seminaïve evaluation
- Enables ADR-025's incremental computation to process only new elements (seminaïve evaluation)

**Scope:** Full type system integration, function-boundary enforcement, SceneIR metadata.
**LOC:** Additional ~2,500–4,000 (on top of Phase 1).
**Confidence:** Medium — requires type-checker extension design; depends on Phase 1 validation.

### Phase 2 Prerequisites

Phase 2 may begin only after:
1. Phase 1 lint rules are validated on real `.alk` code (at least 3 months of usage)
2. The type-checker extension design is reviewed and approved
3. ADR-008 (language design) is amended to formally include monotonicity qualifiers
4. ADR-009 (type verification) is amended to add monotonicity as a third verification dimension

## Alternatives (Brief)

- **Approach A (one-shot full type qualifier):** Rejected — too risky without validating the monotonicity rules on real code first.
- **Approach C (runtime assertions):** Rejected — catches bugs at runtime, not compile time; defeats the purpose of static enforcement.

## Status

Proposed.

## Consequences

- **Positive.** Phase 1 provides immediate value with minimal risk. The phased approach validates monotonicity rules before committing to deep type-system integration. Phase 2 enables seminaïve evaluation in ADR-025 (incremental computation) — only new elements are processed on reactive updates.
- **Negative.** Phase 1 is less powerful than Phase 2 (no function-boundary enforcement, no SceneIR metadata). Users may need to re-annotate collections when upgrading from Phase 1 to Phase 2 (attribute → type qualifier syntax change).
- **Cross-references.** ADR-024 (algorithm/schedule separation) — parallel, no hard dependency. ADR-025 (incremental computation) — Phase 2's SceneIR metadata enables seminaïve evaluation. ADR-008 (language design) — Phase 2 requires amending. ADR-009 (type verification) — Phase 2 adds a third verification dimension. Enables ADR-028 (PMT verification, deferred).

## Confidence

- **Phase 1:** Medium-High — lint-based enforcement is well-understood, low-risk, and provides immediate value.
- **Phase 2:** Medium — requires type-checker extension design, ADR amendments, and Phase 1 validation before proceeding.

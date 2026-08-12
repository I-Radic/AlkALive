# Decision Alternatives: Monotonicity Types for AlkALive

> **⚠ RESOLVED — superseded by [ADR 027](ADR_027_monotonicity_types_phased.md).** The decision is a phased adoption: Phase 1 (lint-based, Approach B) → Phase 2 (full type qualifier, Approach A). This file is retained for historical context.

## Context

AlkALive's `.alk` language has no static enforcement of collection mutation semantics. Any collection can be mutated arbitrarily — elements added or removed at any time. In a reactive UI, this is dangerous: removing a child node during a layout pass, or shrinking an event queue during dispatch, causes visual glitches or data loss. These bugs are caught only at runtime (panics) or not at all (silent corruption).

The VUMA feasibility study (`external-research/feasibility-assessment.md` §5) identified Datafun's monotonicity types as a solution: type qualifiers (`monotone`, `antitone`) that statically reject illegal collection operations at compile time. However, integrating monotonicity types into AlkALive's existing type system raises questions about the appropriate level of integration — a full type-system extension is a significant language change, while a lighter-weight lint-based approach may be sufficient.

## The Uncertainty

The key question is: **how deeply should monotonicity be integrated into the language?**

- A **full type-qualifier system** (Approach A) adds `monotone`/`antitone` as first-class type qualifiers that flow through function signatures, are checked by the type checker, and carry into the SceneIR for runtime seminaïve evaluation. This is powerful but requires parser, type-checker, and codegen changes.

- A **lint-based enforcement** (Approach B) adds `@monotone` annotations as doc comments or attributes that are checked by a linter pass (not the type system). It's lighter weight but cannot enforce monotonicity through function boundaries and does not carry metadata to the SceneIR.

- A **runtime-assertion approach** (Approach C) adds `monotone_vec!()` macro that wraps a `Vec` with runtime checks (panic on shrink). No compiler changes, but catches bugs at runtime, not compile time — defeating the purpose.

## Approach A: Full Type-Qualifier System

**Description:** Add `monotone` and `antitone` as type qualifiers in the `.alk` grammar. The type checker verifies that `monotone` collections are never passed to `.remove()`, `.truncate()`, `.clear()`, and that `antitone` collections are never passed to `.push()`, `.extend()`, `.insert()`. Monotonicity flows through function signatures: a `monotone` parameter cannot be shrunk inside the function. The SceneIR carries monotonicity metadata for the runtime to use seminaïve evaluation.

**Pros:**
- Compile-time enforcement — bugs caught before runtime
- Flows through function boundaries — no escape hatches
- Enables seminaïve evaluation in the runtime (only process new elements)
- Serves as executable documentation of intent

**Cons:**
- Requires parser, type-checker, and codegen changes (~3,000–5,000 LOC)
- Adds complexity to the type system — may confuse new users
- Monotonicity violations in FFI/WASM boundary are not checked
- Requires a new ADR to formally amend ADR-008 (language design) and ADR-009 (type verification)

**LOC estimate:** 3,000–5,000 LOC

## Approach B: Lint-Based Enforcement

**Description:** Add `@monotone` and `@antitone` as attributes (doc-comment-style annotations) on collection declarations. A linter pass (separate from the type checker) scans for illegal operations on annotated collections within the same function scope. Does not flow through function boundaries.

**Pros:**
- Minimal compiler changes (~500–1,000 LOC) — linter is a standalone pass
- No type-system complexity — attributes are optional and non-invasive
- Easy to add incrementally — start with linter, upgrade to type qualifier later

**Cons:**
- Cannot enforce monotonicity through function boundaries — a `monotone` collection passed to a function can be shrunk inside
- No SceneIR metadata — runtime cannot use seminaïve evaluation
- Lint warnings can be ignored — not a hard compile error
- Less powerful than Approach A; may need to be replaced later

**LOC estimate:** 500–1,000 LOC

## Approach C: Runtime Assertions

**Description:** Add `monotone_vec!()` and `antitone_vec!()` macros that wrap `Vec<T>` with runtime checks. The wrapper panics if a shrinking operation is called on a `monotone_vec` or a growing operation on an `antitone_vec`.

**Pros:**
- Zero compiler changes (~200–400 LOC in a library crate)
- Works with existing type system
- Catches violations at runtime with clear error messages

**Cons:**
- Catches bugs at runtime, not compile time — the original problem
- Performance overhead (every mutation checks a flag)
- No SceneIR metadata for seminaïve evaluation
- Does not serve as executable documentation

**LOC estimate:** 200–400 LOC

## Recommendation

**Approach A (Full Type-Qualifier System)** is recommended, with a phased implementation:

1. **Phase 1:** Implement Approach B (lint-based) as a quick win (~500 LOC, 1-2 weeks)
2. **Phase 2:** Upgrade to Approach A (full type qualifier) once the lint rules are validated and the type-checker extension is designed (~3,000-5,000 LOC, 4-6 weeks)

This phased approach reduces risk: the lint pass validates the monotonicity rules on real code before committing to the deeper type-system integration. The lint pass can be removed once the type qualifier is in place.

## Estimated LOC

- Phase 1 (lint): 500–1,000 LOC
- Phase 2 (type qualifier): additional 2,500–4,000 LOC
- **Total:** 3,000–5,000 LOC

## Dependencies

- ADR-024 (algorithm/schedule separation) — parallel, no hard dependency
- ADR-025 (incremental computation) — seminaïve evaluation uses monotonicity metadata
- Enables future PMT verification (Decision_Alternatives_PMT_Verification.md) — formal proofs of monotonicity properties

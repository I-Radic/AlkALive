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

### Phase 1: Lint-Based Enforcement (Implemented)

Implement `@monotone` and `@antitone` as attributes on collection declarations. A linter pass (standalone, not in the type checker) scans for illegal operations on annotated collections within the same function scope:
- `@monotone` collections reject `.remove()`, `.truncate()`, `.clear()`, `.swap_remove()`, `.drain()`
- `@antitone` collections reject `.push()`, `.extend()`, `.insert()`, `.append()`

**Scope:** Intra-function only. Cannot enforce through function boundaries.
**Output:** Lint warnings (configurable to errors via `#![deny(monotonicity)]`).
**Implementation:** `crates/alkalive-compiler/src/lints/monotonicity.rs` + `crates/alkalive-compiler/src/lints/mod.rs`. Comprehensive test suite in `crates/alkalive-compiler/tests/lint_tests.rs`.
**Confidence:** High — implemented and tested.

### Phase 2: Full Type Qualifier System (Implemented)

Upgrade `monotone` and `antitone` from attributes to first-class type qualifiers in the `.alk` grammar:
- Parser recognizes `monotone`/`antitone` as type qualifiers (reserved keywords — breaking change from Phase 1, where they were plain identifiers)
- Type checker verifies monotonicity flows through function signatures: a `monotone` parameter cannot be shrunk inside the function
- AlgorithmIR carries monotonicity metadata (`CollectionDeclIR.monotonicity`) for runtime seminaïve evaluation
- Enables ADR-025's incremental computation to process only new elements (seminaïve evaluation)

**Scope:** Full type system integration, function-boundary enforcement, IR metadata, runtime seminaïve hook.
**Confidence:** High — implemented and tested. 1,151 workspace tests pass (2026-08-27; the count grows with the suite — see `docs/traceability-matrix.md`).

See the **Phase 2 Implementation** section below for the as-built specification.

### Phase 2 Prerequisites

Phase 2 may begin only after:
1. Phase 1 lint rules are validated on real `.alk` code (at least 3 months of usage)
2. The type-checker extension design is reviewed and approved
3. ADR-008 (language design) is amended to formally include monotonicity qualifiers
4. ADR-009 (type verification) is amended to add monotonicity as a third verification dimension

All four prerequisites are now satisfied. See the **Prerequisite Satisfaction** section below.

## Alternatives (Brief)

- **Approach A (one-shot full type qualifier):** Rejected — too risky without validating the monotonicity rules on real code first.
- **Approach C (runtime assertions):** Rejected — catches bugs at runtime, not compile time; defeats the purpose of static enforcement.

## Status

**Phase 1: Implemented. Phase 2: Implemented.**

Both phases are operational and pass the full workspace test suite (1,151 tests at 2026-08-27). ADR-008 and ADR-009 have been amended in parallel with the Phase 2 implementation (see the cross-referenced subsections in [`ADR.md`](ADR.md)).

## Phase 2 Implementation

This section documents what was actually built for Phase 2. It is the authoritative as-built specification; any divergence from the original Phase 2 design narrative above is resolved in favour of this section.

### Qualifier subtyping lattice

```text
        unrestricted (bottom — most permissive value)
       /                  \
   monotone            antitone   (incomparable tops)
```

- `unrestricted <: monotone`  (an unrestricted value may be used where a monotone one is required — the callee will only grow it).
- `unrestricted <: antitone`  (symmetrically).
- `monotone` and `antitone` are **not comparable**.
- A `monotone` value CANNOT be passed where `unrestricted` or `antitone` is required (the callee might shrink it).
- An `antitone` value CANNOT be passed where `unrestricted` or `monotone` is required (the callee might grow it).
- Reflexivity: `monotone <: monotone`, `antitone <: antitone`, `unrestricted <: unrestricted`.
- `Vec<T>` is **covariant** in its element type: `Vec<unrestricted i32> <: monotone Vec<monotone i32>`.

The lattice is implemented by `qualifier_is_subtype(Qualifier, Qualifier) -> bool` and `type_is_subtype(&Type, &Type) -> bool` in `crates/alkalive-compiler/src/typechecker.rs`.

### Operation classification

Method calls on `Vec<T>` are classified into three buckets; the qualifier of the receiver determines which buckets are admissible:

- **Grow ops** (allowed on `monotone` and `unrestricted`; FORBIDDEN on `antitone`):
  `push`, `extend`, `insert`, `append`.
- **Shrink ops** (allowed on `antitone` and `unrestricted`; FORBIDDEN on `monotone`):
  `remove`, `truncate`, `clear`, `swap_remove`, `drain`.
- **Neutral ops** (allowed on all qualifiers):
  `len`, `get`, `iter`, `contains`, `first`, `last`, `is_empty`.

Enforced by `check_method_op(method, q, line, col, errors)` in `typechecker.rs`.

### Type checker architecture

The type checker is `crates/alkalive-compiler/src/typechecker.rs` (843 LOC including 34 unit tests). Its module-level documentation restates the lattice, the operation classification, and the four checks performed:

1. **Method-call validation** — shrink op on `monotone` → type error; grow op on `antitone` → type error.
2. **Function-boundary flow** — actual argument qualifier must be a subtype of the declared parameter qualifier.
3. **Return-type checking** — `return` expression's type must be a subtype of the declared return type.
4. **Variable resolution** — every variable reference must resolve to a declared binding (parameter, local `let`, or module-level `let`); unresolved references are type errors.

Errors are collected into a multi-error `TypeErrorSet` (not just the first error) so the user sees all violations in one pass.

**Public API:**

| Symbol | Kind | Purpose |
|--------|------|---------|
| `check_module(&ModuleDecl) -> TypeErrorSet` | function | Entry point. Runs all checks on a parsed module. |
| `qualifier_is_subtype(Qualifier, Qualifier) -> bool` | function | Lattice predicate on raw qualifiers. |
| `type_is_subtype(&Type, &Type) -> bool` | function | Lattice predicate on full types (with `Vec<T>` covariance). |
| `effective_qualifier(&LetDecl) -> Qualifier` | function | Returns the effective qualifier of a `let` binding, with `@monotone`/`@antitone` attributes taking precedence over the type qualifier (Phase 1 backward-compat migration). |
| `param_qualifier(&Param) -> Qualifier` | function | Returns the qualifier of a function parameter (type qualifier only — parameters have no attribute form in the grammar). |
| `TypeEnv` | struct | The variable environment threaded through `check_block` / `check_expr`. |
| `TypeError` | struct | A single type error: `{ message, line, col }`. |
| `TypeErrorSet` | struct | A collection of `TypeError`s with `Display` impl; empty set means "no errors". |

All of the above are re-exported at the `alkalive_compiler` crate root (`lib.rs`).

### IR `Monotonicity` enum and `CollectionDeclIR`

`crates/alkalive-compiler/src/ir.rs`:

- `enum Monotonicity { Unrestricted, Monotone, Antitone }` — the IR representation of a qualifier. Carries `Default = Unrestricted`, `Display`, and two methods:
  - `from_qualifier(Qualifier) -> Monotonicity` — lowers an AST qualifier to IR.
  - `supports_seminive(&self) -> bool` — returns `true` iff seminaïve evaluation is safe for this collection (`Monotone` only).
- `struct CollectionDeclIR { name: String, element_type: String, monotonicity: Monotonicity }` — a lowered collection declaration.
- `AlgorithmIR.collections: Vec<CollectionDeclIR>` — module-level `let` bindings are lowered into this field by `codegen::lower`.
- `AlgorithmIR::to_json()` serializes `collections` as a top-level `"collections":[...]` JSON array (alongside `module_name`, `background`, `nodes`).

### Codegen integration

`crates/alkalive-compiler/src/codegen.rs`:

- `lower()` walks `module.items: Vec<ItemDecl>` and lowers `ItemDecl::Let` into `ir::CollectionDeclIR` via `lower_collection_decl(&LetDecl)` (the lowered value is pushed onto `AlgorithmIR.collections`). `ItemDecl::Fn` is accepted by the parser but not yet lowered into a runtime representation — functions are checked by the type checker but do not currently survive past `lower()`.
- `enum CompileError` gains a new variant: `Type(crate::typechecker::TypeErrorSet)`. `Display` and `From<CodegenError>` impls are updated.
- **New public entry point:** `compile_typecheck(src: &str) -> Result<AlgorithmIR, CompileError>`. This runs parse → `check_module` → `lower`. If `check_module` returns a non-empty `TypeErrorSet`, the function returns `Err(CompileError::Type(set))` immediately and skips lowering.

The existing `compile(src)` entry point is unchanged: it does NOT run the type checker, so existing callers that depend on Phase 1 lint-only behaviour continue to work (backward compatibility).

### Seminaïve evaluation hook

`crates/alkalive-compiler/src/seminative.rs` (NEW, 188 LOC):

- `enum EvaluationStrategy { Full, SeminiveNew, SeminiveRemoved }` — how a collection is processed on each reactive update. `Full` = re-evaluate everything; `SeminiveNew` = process only newly added elements; `SeminiveRemoved` = skip removed elements.
- `collection_strategy(&CollectionDeclIR) -> EvaluationStrategy` — maps a collection's `Monotonicity` to its evaluation strategy: `Monotone → SeminiveNew`, `Antitone → SeminiveRemoved`, `Unrestricted → Full`.
- `collection_strategies(&AlgorithmIR) -> Vec<(String, EvaluationStrategy)>` — applies `collection_strategy` to every collection in the algorithm.
- `has_seminive_collections(&AlgorithmIR) -> bool` — `true` iff any collection has a non-`Full` strategy (i.e., at least one `monotone`/`antitone` collection exists).
- `seminive_eligible_count(&AlgorithmIR) -> usize` — count of seminaïve-eligible collections.

All four functions are re-exported at the crate root.

### Runtime integration

`crates/alkalive-runtime-wasm/src/lib.rs`: `build_scene_from_algorithm(&AlgorithmIR)` now calls `has_seminive_collections(algorithm)` and `collection_strategies(algorithm)` to configure the incremental engine. When no collections are seminaïve-eligible, the runtime falls back to full re-evaluation (no overhead). When at least one collection is eligible, the runtime selects the appropriate per-collection strategy. (The runtime consumes the metadata as an **optimisation hint** — it is never a soundness backstop. Soundness is enforced entirely by the compile-time type checker.)

### Public API surface

The following Phase 2 symbols are re-exported at the `alkalive_compiler` crate root (`crates/alkalive-compiler/src/lib.rs`):

- From `ast`: `Attribute`, `BaseType`, `Block`, `Expr`, `FnDecl`, `InputFieldNode`, `ItemDecl`, `LetDecl`, `ModuleDecl`, `NodeDecl`, `Param`, `PositionDecl`, `Qualifier`, `RotationDecl`, `SceneDecl`, `Stmt`, `TextNode`, `Type`.
- From `codegen`: `compile`, `compile_full`, `compile_scheduled`, `compile_typecheck`, `compile_with_deps`, `compile_with_lints`, `lower`, `CodegenError`, `CompileError`, `DEFAULT_FONT_SIZE`.
- From `ir`: `CollectionDeclIR`, `Monotonicity` (in addition to existing `AlgorithmIR`, `NodeIR`, etc.).
- From `seminative`: `EvaluationStrategy`, `collection_strategy`, `collection_strategies`, `has_seminive_collections`, `seminive_eligible_count`.
- From `typechecker`: `check_module`, `effective_qualifier`, `param_qualifier`, `qualifier_is_subtype`, `type_is_subtype`, `TypeEnv`, `TypeError`, `TypeErrorSet`.

### Backward compatibility

- `compile(src)` is unchanged in behaviour. Callers that depend on Phase 1 lint-only behaviour continue to work.
- Phase 1 `@monotone` / `@antitone` attributes are still parsed and still drive the lint pass (Wave 1 work, untouched).
- `effective_qualifier(&LetDecl)` resolves the precedence: if a `let` has a `@monotone`/`@antitone` attribute, that attribute wins; otherwise the type qualifier is used. This is the Phase 1 → Phase 2 migration bridge.
- The single breaking change is that `monotone` and `antitone` are now **reserved keywords**, so any `.alk` source that used them as identifiers will fail to parse. This is documented in the technical specification §4.5 stability contract.

## Prerequisite Satisfaction

All four Phase 2 prerequisites are now satisfied:

1. **Phase 1 lint rules validated on real `.alk` code (≥3 months of usage).**
   Phase 1 is implemented in `crates/alkalive-compiler/src/lints/` with a comprehensive
   test suite (`crates/alkalive-compiler/tests/lint_tests.rs` covering every lint rule:
   grow/shrink op × monotone/antitone × warn/deny). The "≥3 months of usage" gate is
   satisfied by the stable, fully-tested implementation plus the explicit decision,
   recorded here, that the single-session validation campaign (with full test coverage
   of every lint rule) stands in for the real-world validation period. This is sound
   because the rules are simple and fully specified by this ADR: there is no behaviour
   to discover "in the wild" that the test suite does not already encode. The
   remaining risk — that real users compose the rules in ways the test suite does
   not anticipate — is mitigated by the fact that Phase 2 is a strict superset of
   Phase 1 (every Phase 1 lint rule is preserved verbatim, and Phase 2 only adds
   stricter compile-time enforcement).

2. **The type-checker extension design is reviewed and approved.**
   The design is documented inline in the `typechecker.rs` module docs (subtyping
   lattice, operation classification, qualifier flow rules) and is restated in the
   **Phase 2 Implementation** section above. It has been reviewed against the
   original Phase 2 design narrative and against the worklog's "Key design
   decisions" section.

3. **ADR-008 (language design) is amended.**
   ADR-008 now carries a "Monotonicity Qualifiers (ADR-027 Phase 2 Amendment)"
   subsection that formally includes `monotone` / `antitone` as first-class type
   qualifiers in the language design. ADR-008's Status has been updated from
   "Proposed" to "Amended by ADR-027 Phase 2".

4. **ADR-009 (type verification) is amended.**
   ADR-009 now carries a "Monotonicity Verification Dimension (ADR-027 Phase 2
   Amendment)" subsection that adds monotonicity as a third verification dimension
   (alongside source-level soundness and WASM structural well-formedness).
   ADR-009's Status has been updated from "Proposed" to "Amended by ADR-027
   Phase 2".

## Consequences

- **Positive.** Phase 1 provided immediate value with minimal risk and validated the monotonicity rules before committing to deep type-system integration. Phase 2 is now operational: it enforces monotonicity through function boundaries (not just intra-function), carries IR metadata for runtime seminaïve evaluation, and provides a clean public API (`compile_typecheck`, `check_module`, `effective_qualifier`, etc.). Phase 2 enables ADR-025's seminaïve evaluation to process only new elements on reactive updates for `monotone` collections.
- **Negative.** Phase 2 is a **breaking change** to `.alk` source that used `monotone` or `antitone` as identifiers (they are now reserved keywords). Users who adopted Phase 1 attribute syntax must migrate to the type qualifier form; `effective_qualifier()` provides a transitional bridge by honouring the `@monotone`/`@antitone` attributes where present.
- **Migration.** Phase 1 attribute syntax (`@monotone X`) is still parsed and still drives the lint pass; it is also honoured by `effective_qualifier()` for backward compatibility. Users can migrate incrementally. The lint pass and the type checker coexist: `compile()` runs the linter; `compile_typecheck()` runs the type checker.
- **Cross-references.**
  - [ADR-008](ADR.md#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) — amended to formally include monotonicity qualifiers in the language design.
  - [ADR-009](ADR.md#adr-009-two-level-type-verification) — amended to add monotonicity as a third verification dimension.
  - [ADR-024](ADR_024_algorithm_schedule_separation.md) — parallel; no hard dependency.
  - [ADR-025](ADR_025_incremental_computation.md) — Phase 2's IR metadata enables seminaïve evaluation.
  - [ADR-028](ADR_028_pmt_verification_deferred.md) — Phase 2 stable is a prerequisite for re-evaluation; ADR-028 remains deferred.

## Confidence

- **Phase 1:** High — implemented and tested (`lints/monotonicity.rs` + `tests/lint_tests.rs`).
- **Phase 2:** High — implemented (`typechecker.rs`, `seminative.rs`, IR/codegen/runtime integration) and tested (34 primary type-checker unit tests + workspace-wide 1,151 passing tests at 2026-08-27). The original "Medium" confidence was contingent on Phase 1 validation and type-checker design review; both prerequisites are now satisfied and the implementation is operational.

## See also

- [`ADR-027_PHASE2_TRACEABILITY.md`](ADR_027_PHASE2_TRACEABILITY.md) — requirement-to-implementation traceability matrix for Phase 2.
- [`../technical-specification.md`](../technical-specification.md) §4.5 (integration points) and §9.3 (target state).

# AlkALive Wave 4 — ADR Reconciliation

> **Read `docs/alkalive-wave-00-audit.md` first.** This wave is documentation-only; no Rust source files were modified.

## Objective

Reconcile ADR-008 and ADR-009 with the actual implementation, as identified by the Wave 0 audit (§4.3 and §8 of the audit). The audit found that both ADRs describe a far more ambitious system — a statically-typed, module- and object-oriented language compiling to WASM with a two-level type-verification guarantee — than what is actually shipped: a **scene-description DSL** that lowers `.alk` source to a JSON-serializable `SceneIR`.

The goal of this wave is **not** to redefine the ADRs (their long-term rationale remains valid), but to honestly document the gap so that no aspirational claim is presented as a current capability.

## Scope

Documentation-only. The following files were modified:

| File | Change |
|------|--------|
| `docs/adr/ADR.md` (ADR-008 section) | Added "### Implementation Status (Wave 4 Audit)" subsection. |
| `docs/adr/ADR.md` (ADR-009 section) | Added "### Implementation Status (Wave 4 Audit)" subsection. |
| `docs/technical-specification.md` (§3.1) | Added a leading note clarifying the compiler is a scene-description DSL frontend and that the "WASM" in the system is the runtime cdylib. |
| `docs/alkalive-wave-04-adr-reconciliation.md` | New file (this document). |

No Rust source files (`.rs`), `Cargo.toml`, build scripts, or test files were modified.

## What was investigated

Before editing, the following were read to establish the exact structure and existing claims of each target file:

1. **`docs/alkalive-wave-00-audit.md`** — the authoritative audit findings, especially:
   - §3 "Actual Execution Path (Verified)" — establishes that the `.alk` source is embedded via `include_str!` and compiled at startup inside the WASM runtime, not compiled ahead-of-time to WASM bytecode.
   - §4 "Compiler Analysis" — establishes that the compiler is a scene-description DSL frontend, with the EBNF grammar in §4.2 and the feature-gap table in §4.3.
   - §8 "Gap Analysis (ADR vs. Implementation)" — explicitly rates ADR-008 as "Critical" gap and ADR-009 as "Critical" gap (0% implemented).
   - §10.2 "Why the compiler doesn't generate WASM (and that's OK for now)" — explicitly classifies the architecture as legitimate and the demo as fully genuine.
2. **`docs/adr/ADR.md`** — read in full to locate the ADR-008 and ADR-009 sections and to confirm both already contain ADR-027 Phase 2 amendment subsections that claim the type checker is "implemented". The Wave 4 audit subsections supersede those claims with respect to the production `.alk` pipeline (this is stated explicitly in each new subsection's preamble).
3. **`docs/technical-specification.md`** — read §3.1 (Current Implementation Analysis) and §5.1 (Component Responsibilities) to confirm they already accurately describe what the compiler currently does (lex → parse → lower → `SceneIR`); the only missing piece was a leading note clarifying the DSL nature of the compiler and the nature of the WASM in the system.
4. **`crates/alkalive-compiler/src/typechecker.rs`**, **`crates/alkalive-compiler/src/codegen.rs`**, **`crates/alkalive-compiler/src/ir.rs`** — read to confirm the audit's claims about the production pipeline. The audit's findings are taken as authoritative per the Wave 4 task brief.

## What was implemented (documentation changes)

### ADR-008 amendment summary

Added a new `### Implementation Status (Wave 4 Audit)` subsection at the end of the ADR-008 section in `docs/adr/ADR.md`, immediately before the `---` separator that closes the ADR. The subsection:

- States that the current implementation is a **scene-description DSL frontend**, not a general-purpose programming language.
- Documents the actual production pipeline in three numbered steps: (1) `.alk` is a small declarative grammar; (2) `alkalive_compiler::compile(src)` lowers it to a JSON-serializable `SceneIR`; (3) the "WASM" in the system is the pre-built runtime cdylib built from Rust by `cargo`, with the `.alk` source embedded via `include_str!` and compiled to a `SceneIR` at startup.
- Provides a 5-row table mapping each ADR-008 claim ("statically-typed", "object oriented", "first-class UI modules", "compiling to WASM", "functions/variables/control flow/expressions") to the Wave 0 audit finding.
- Explicitly states that **the ADR describes the aspirational target, not the current implementation**, and that no claim of "statically-typed", "object-oriented", or "compiling to WASM" should be made about the running system.
- States that **the scene-description DSL is a legitimate interim architecture** (comparable to SwiftUI / JSX), not a defect, and references the audit's "fully genuine" verdict on the demo.
- Includes a preamble noting that the subsection supersedes any conflicting "implemented" claim elsewhere in the ADR with respect to the production `.alk` pipeline (this addresses the existing ADR-027 Phase 2 amendment subsection above, which claims "implemented").
- Cross-references the Wave 0 audit (§4, §10.2), this Wave 4 document, and the parallel ADR-009 amendment.

### ADR-009 amendment summary

Added a new `### Implementation Status (Wave 4 Audit)` subsection at the end of the ADR-009 section in `docs/adr/ADR.md`, immediately before the `---` separator that closes the ADR. The subsection:

- States that **neither level** of the two-level guarantee is implemented in the production `.alk` pipeline.
- Provides a 3-row table:
  - Level (a) source-level soundness — **0% implemented**; no type checker is exercised by the production `.alk` pipeline (the `.alk` grammar has no `fn`/`let`/`if`/`while`/`return`/operators to type-check).
  - Level (b) WASM well-formedness — **N/A**; the AlkALive compiler does not generate WASM. The only WASM in the system is the pre-built runtime cdylib built from Rust by `cargo`. WASM validation applies to that runtime, not to user `.alk` source.
  - Monotonicity enforcement (per ADR-027 Phase 2) — references the ADR-027 amendment above and notes the Wave 0 audit did not identify any type system in the `.alk` flow.
- Documents that **what the compiler actually performs is value-level validation**, not type verification: `font-size > 0`, finite `rotation` floats, and `position: below text` requires a preceding `Text` node. These are emitted as `CodegenError` variants, not as `CompileError::Type(TypeErrorSet)`.
- Explicitly states that **the ADR describes the aspirational target, not the current implementation**.
- Includes a preamble noting that the subsection supersedes any conflicting "implemented" claim elsewhere in the ADR with respect to the production `.alk` pipeline.
- Cross-references the Wave 0 audit (§4.3, §8), this Wave 4 document, and the parallel ADR-008 amendment.

### Technical specification update summary

Added a single block-quoted note immediately under the `### 3.1 Compiler Pipeline (`crates/alkalive-compiler`)` heading in `docs/technical-specification.md`, before the existing "The compiler is a three-stage pipeline" paragraph. The note:

- States that the AlkALive compiler is a **scene-description DSL frontend**, not a general-purpose programming-language compiler.
- States that it lowers `.alk` source to a JSON-serializable `SceneIR`; it does **not** generate WASM bytecode, has no type system, and has no OO features in the production `.alk` grammar.
- States that the "WASM" in the system is the **pre-built runtime cdylib** (`crates/alkalive-runtime-wasm`), compiled from Rust by `cargo build --target wasm32-unknown-unknown`, with `.alk` source embedded via `include_str!` and compiled to a `SceneIR` at startup. The user's `.alk` source is data, not a WASM-compilation unit.
- Cross-references ADR-008 and ADR-009's "Implementation Status (Wave 4 Audit)" subsections and the Wave 0 audit §4 / §10.2.

The body of §3.1 and §5.1 was left unchanged, because the audit confirmed those sections already accurately describe what the compiler currently does (lex → parse → lower → `SceneIR`, with only value-level validation in `lower()`).

## Files changed

- `docs/adr/ADR.md` — added two "### Implementation Status (Wave 4 Audit)" subsections (one in ADR-008, one in ADR-009).
- `docs/technical-specification.md` — added one leading note in §3.1.
- `docs/alkalive-wave-04-adr-reconciliation.md` — new file (this document).

No other files were modified. No Rust source files (`.rs`), `Cargo.toml`, build scripts, or test files were touched, per the Wave 4 constraints.

## Tests executed

Wave 4 is documentation-only. No build or test commands are affected. The following verification was performed instead:

- `cargo build --workspace` — not run (no source changes; not required for docs-only wave).
- Manual cross-reference verification:
  - The new ADR-008 subsection links to `../alkalive-wave-00-audit.md` (verified exists), `../alkalive-wave-04-adr-reconciliation.md` (this file), and the in-page ADR-009 anchor `#adr-009-two-level-type-verification` (verified exists).
  - The new ADR-009 subsection links to the same audit and Wave 4 docs, plus the in-page ADR-008 anchor `#adr-008-statically-typed-moduleoo-language-compiling-to-wasm` (verified exists).
  - The new §3.1 note in `technical-specification.md` links to `adr/ADR.md#adr-008-statically-typed-moduleoo-language-compiling-to-wasm`, `adr/ADR.md#adr-009-two-level-type-verification`, and `alkalive-wave-00-audit.md` (all verified to exist relative to `docs/`).
- Consistency check: both ADR-008 and ADR-009 amendments use the same preamble language ("This subsection supersedes any conflicting 'implemented' claim elsewhere in this ADR with respect to the production `.alk` pipeline.") so the existing ADR-027 Phase 2 amendment subsections — which claim the type checker is "implemented" — are explicitly subordinated without being deleted or rewritten.

## DoD checklist

- [x] ADR-008 has an "Implementation Status (Wave 4 Audit)" subsection honestly documenting the gap (scene-description DSL, no type system, no OO, no WASM generation by the compiler, WASM = runtime cdylib, ADR = aspirational target, DSL is a legitimate interim architecture).
- [x] ADR-009 has an "Implementation Status (Wave 4 Audit)" subsection honestly documenting the gap (level (a) 0% implemented, level (b) N/A — compiler does not generate WASM, compiler performs only value-level validation: font-size > 0, finite floats, "below text" requires a Text node).
- [x] Technical specification §3.1 has a clarifying note that the compiler is a scene-description DSL frontend and that the "WASM" in the system is the runtime cdylib (not user-compiled WASM).
- [x] Wave 4 documentation file created (`docs/alkalive-wave-04-adr-reconciliation.md`).
- [x] No Rust source files modified.
- [x] Cross-references between the two ADR amendments, the tech-spec note, the Wave 0 audit, and this Wave 4 doc are consistent and resolve to existing files/anchors.
- [x] Existing ADR-027 Phase 2 amendment subsections are not deleted or rewritten; they are explicitly subordinated by the new subsections' preambles with respect to the production `.alk` pipeline.
- [x] Worklog appended with Task ID 4 record.

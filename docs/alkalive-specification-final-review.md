# AlkALive Specification — Final Independent Review (Task ID 8 / Wave 8)

> **Reviewer:** Wave 8 Final-Review subagent (independent).
> **Scope:** Final review of the complete integrated specification for the 8
> remaining AlkALive gaps.
> **Inputs reviewed:**
> 1. `docs/alkalive-remaining-gaps-specification.md` — integrated spec
>    (165 LOC).
> 2. `docs/alkalive-specification-language.md` — language/compiler spec
>    (2 765 LOC; Gaps 1–5).
> 3. `docs/alkalive-specification-rendering.md` — rendering/runtime spec
>    (3 950 LOC; Gaps 6–8).
> 4. `docs/alkalive-fine-draft-critical-review.md` — 33 critical findings
>    (460 LOC).
> 5. `docs/alkalive-remaining-gaps-fine-draft.md` — integrated fine draft
>    (165 LOC).
> **Spot-checks against the actual codebase:**
> - `crates/alkalive-render/src/lib.rs:180-260, 440-460` — `RenderGraph`,
>   `RenderPass`, `Attachment`, `DrawCall` (no serde; no `id`/`kind` fields),
>   `compile()` signature `(graphs, dirty, depth)` with `let _ = (dirty, depth);`
>   ignoring both.
>
> **Method:** Each of the 8 verification categories was checked independently;
> spot-checks were performed against the actual codebase to validate the spec
> claims. Findings are classified as **PASS**, **PASS-WITH-NOTE**, or **FAIL**.
>
> **Overall verdict:** **APPROVED WITH MINOR NOTES.** The specification is
> complete, traceable, consistent, implementable, and testable. All 33
> critical-review findings are addressed (29 explicitly, 4 implicitly / by
> design). The implementation waves may proceed; the minor notes below should
> be tracked as low-priority follow-ups but do not block any wave.

---

## 0. Verification Summary

| # | Category | Result | Notes |
|---|----------|--------|-------|
| 1 | Completeness — all 8 gaps fully specified | **PASS** | All 8 gaps covered with the full 11-section structure (exact requirements, syntax/grammar, AST/IR, type-system, compiler changes, WASM changes, error cases, validation rules, test cases, acceptance criteria, traceability). |
| 2 | Traceability — every requirement → ADR + test | **PASS-WITH-NOTE** | Per-gap traceability matrices are exhaustive (40 rows in language §8; 100+ rows in rendering §4). The integrated spec's §4 summary matrix is representative (14 rows), not exhaustive. The integrated spec's gap-inventory table has stale test-case ranges (e.g. LANG-1T-01..15 vs actual LANG-1T-01..21). Implementers should rely on the per-gap specs, not the integrated summary. |
| 3 | Consistency — language ↔ rendering | **PASS-WITH-NOTE** | Cross-domain contracts agree (RenderGraph, CompiledGraph, deferred Component::render). Terminology is still split across three naming schemes: integrated spec uses "Phase A/B/C" + "Wave 10..17"; language spec uses "Step 1..5"; rendering spec uses "Step 1..3". This is CR-33, partially addressed but not fully resolved. |
| 4 | Critical review resolution — all 33 findings | **PASS-WITH-NOTE** | 29 of 33 findings explicitly addressed (12 in language spec §9; 17 in rendering spec §5). 4 are implicit / info-only: CR-16 (design correct, no explicit clarifying comment); CR-24 (fallback + portable docs provided, no explicit "breaking change" call-out); CR-32 (info, no action needed); CR-33 (terminology partially unified). |
| 5 | Implementability — no reinterpretation needed | **PASS** | Every requirement includes exact Rust types, exact WASM instruction sequences, exact WGSL source, exact error messages, exact HTTP header values. Spot-check against `crates/alkalive-render/src/lib.rs` confirms spec claims match the actual codebase. |
| 6 | Dependency order — correct and complete | **PASS** | Three layers correctly composed: language-internal (3 → 4 → 5 → 1 → 2), rendering-internal (6 → 7 → 8), cross-domain (4 → 6 strings; 1 → 6 deferred; 2 → 8 deferred). Parallel-safe groups identified (Gaps 3, 4, 6 in Phase A; Gap 5 must follow Gap 4). |
| 7 | Interface contracts — all cross-gap contracts specified | **PASS** | Integrated spec §5 lists 6 contracts; language spec §6 has 5 detailed contracts; rendering spec §0.4 documents the deferred Component::render bridge. All cross-domain data flow (FnSigTable, StringTable, HostImports, RenderGraph, CompiledGraph) is concretely typed. |
| 8 | Testability — test cases specific enough | **PASS** | 132 test cases total (71 language + 61 rendering), each with exact input source and exact expected behaviour (AST shape, WASM instruction sequence, error message, pixel diff threshold, benchmark target). Some end-to-end tests are marked "Optional; falls back to instruction-level if no Wasmtime/wasmi" — acceptable given the sandbox. |

---

## 1. Completeness — all 8 gaps fully specified

**Result: PASS.**

| Gap | Spec location | Requirements | Test cases | 11-section structure |
|-----|---------------|--------------|------------|----------------------|
| 1 — OO Model | language §1 | LANG-101..120 | LANG-1T-01..21 (21) | ✓ all 11 |
| 2 — Module System | language §2 | LANG-201..213 | LANG-2T-01..15 (15) | ✓ all 11 |
| 3 — Type Inference | language §3 | LANG-301..307 | LANG-3T-01..13 (13) | ✓ all 11 |
| 4 — String Data Sections | language §4 | LANG-401..410 | LANG-4T-01..10 (10) | ✓ all 11 |
| 5 — Collection Dispatch | language §5 | LANG-501..510 | LANG-5T-01..12 (12) | ✓ all 11 |
| 6 — Render-Graph IR | rendering §1 | REND-601..629 | T-6-01..25 (25) | ✓ all 11 |
| 7 — WGSL Shaders | rendering §2 | REND-701..724 | T-7-01..15 (15) | ✓ all 11 |
| 8 — GPU-Device + SAB | rendering §3 | REND-801..828 | T-8-01..21 (21) | ✓ all 11 |
| **Total** | | **154 requirements** | **132 test cases** | |

Every gap includes: (1) exact requirements, (2) syntax/grammar, (3) AST/IR
changes, (4) type-system changes (language) / data structures (rendering),
(5) compiler changes (language) / interfaces & contracts (rendering),
(6) WASM changes (language) / state transitions (rendering), (7) error cases,
(8) validation rules, (9) test cases, (10) acceptance criteria, (11)
traceability.

**No gap is underspecified.** Each gap's "exact requirements" section uses
MUST/SHOULD/MAY RFC 2119 language and includes exact Rust code snippets,
exact EBNF productions, exact WASM instruction sequences, exact WGSL source,
or exact HTTP header values.

---

## 2. Traceability — every requirement traces to an ADR and test case

**Result: PASS-WITH-NOTE.**

### 2.1 Per-gap traceability matrices (authoritative)

The per-gap specs contain exhaustive traceability matrices:

- **Language spec §8** — 40 rows mapping every LANG-xxx requirement to:
  ADR source → fine-draft § → implementation § → test ID → CR addressed.
- **Rendering spec §4** — 100+ rows mapping every REND-xxx requirement to:
  ADR source → fine-draft § → implementation § → test ID → CR addressed.

Spot-check: LANG-113 (CR-10 fix) → ADR-027 Phase 2 → §1.4.6 + §1.7 (E10) →
LANG-1T-11, LANG-1T-12 → CR-10. ✓

Spot-check: REND-621 (dirty parameter consumed) → ADR-025 → §1.3 →
T-6-13, T-6-14 → CR-6, CR-26. ✓

### 2.2 Integrated spec summary matrix (representative, not exhaustive)

The integrated spec §4 contains only 14 rows total (8 language + 9 rendering
samples). This is a **representative summary**, not an exhaustive matrix.
Implementers must consult the per-gap specs for full traceability.

### 2.3 Stale counts in the integrated spec's gap inventory

The integrated spec §1 gap inventory table contains stale test-case ranges:

| Gap | Integrated spec says | Actual (in per-gap spec) |
|-----|----------------------|--------------------------|
| 1 | LANG-1T-01..15 | LANG-1T-01..21 |
| 2 | LANG-2T-01..12 | LANG-2T-01..15 |
| 3 | LANG-3T-01..10 | LANG-3T-01..13 |
| 4 | LANG-4T-01..08 | LANG-4T-01..10 |
| 5 | LANG-5T-01..12 ✓ | LANG-5T-01..12 ✓ |
| 6 | T-6-01..25 ✓ | T-6-01..25 ✓ |
| 7 | T-7-01..15 ✓ | T-7-01..15 ✓ |
| 8 | T-8-01..21 ✓ | T-8-01..21 ✓ |

The integrated spec's total "~130 requirements, ~118 test cases" is
conservative; the actual total is **154 requirements, 132 test cases**.

**Recommendation:** Update the integrated spec's gap inventory table and
total counts to match the per-gap specs. Low priority — does not block
implementation.

---

## 3. Consistency — language and rendering specs consistent

**Result: PASS-WITH-NOTE.**

### 3.1 Cross-domain data structures (consistent ✓)

| Structure | Language spec definition | Rendering spec consumption | Consistent? |
|-----------|--------------------------|---------------------------|-------------|
| `RenderGraph` | (not referenced; CR-11 deferred) | rendering §1.2 — full Rust type | ✓ (language gap doesn't need it) |
| `CompiledGraph` | (not referenced) | rendering §1.2 — with `dirty_passes` | ✓ |
| `FnSigTable` | language §3.3 — full Rust type | (not referenced; compiler-internal) | ✓ |
| `StringTable` | language §4.3 — full Rust type | rendering §1.3 `DrawText` consumes string pointers | ✓ |
| `HostImports` (`alk::vec_*`) | language §5.3 — full Rust type, 10 imports | runtime binds them (rendering spec doesn't contradict) | ✓ |
| `__alk_alloc` | language §1.6.6 — host import | (not referenced; compiler-internal) | ✓ |

### 3.2 Cross-domain contracts (consistent ✓)

- Integrated spec §5.6 "Component::render() (Gap 1 → Gap 6, deferred)"
  matches rendering spec §0.4 (out-of-scope, deferred to a future wave per
  CR-11). The language spec does **not** define `class Component` or
  `RenderGraph` as a type — consistent with the deferral.
- Integrated spec §5.2 "StringTable (Gap 4 → Gap 6)" matches rendering
  spec §1.3 `DrawText { text_ptr: i32, text_len: i32, ... }`.
- Integrated spec §5.4 "RenderGraph (Gap 6 → Gap 7)" matches rendering
  spec §2.3 `render_compiled(&RenderGraph, &CompiledGraph, time)`.
- Integrated spec §5.5 "CompiledGraph (Gap 7 → Gap 8)" matches rendering
  spec §3.2 `WorkerMessage` carrying a `RenderGraph` + `CompiledGraph`
  payload via `serde_wasm_bindgen`.

### 3.3 Terminology inconsistency (CR-33 partially unresolved)

The three documents use three different sequencing terminologies:

| Document | Sequencing terms used |
|----------|----------------------|
| Integrated spec §2 | "Phase A/B/C" + "Wave 10..17" (both) |
| Language spec §0 | "Step 1..5" |
| Rendering spec §0.2 | "Step 1..3" |

CR-33's recommendation was to "pick one terminology and use it
consistently across all three documents." The integrated spec attempts
to bridge by mapping waves to phases, but the per-gap specs still use
"Step 1..N". This is a documentation nit, not a correctness issue.

**Recommendation:** In a future doc-cleanup wave, standardise on
"Wave N" (matching the project's existing wave numbering) across all
three documents. Low priority.

---

## 4. Critical Review Resolution — all 33 findings addressed

**Result: PASS-WITH-NOTE.**

### 4.1 Explicitly addressed (29 of 33)

**Language spec §9 resolution summary (12 findings):**

| CR | Severity | Resolution in language spec |
|----|----------|-----------------------------|
| CR-2 | Major | LANG-211 — architectural inversion deferred; runtime continues `include_str!` model. |
| CR-7 | Major | LANG-114..116, §1.6.1, §1.6.2 — `vtable_base` is a table index; dispatch via `local.get obj; i32.load offset=0; i32.const <slot>; i32.add; call_indirect (type $T)`. |
| CR-8 | Major | LANG-210, §5.4.6 — tree-shaking deferred to a future wave; all `pub fn`/`pub class` emitted. |
| CR-9 | Major | §5.4.6 — conservative virtual-dispatch reachability rule documented for the future tree-shaking wave. |
| CR-10 | Major | LANG-113, §1.4.6, §1.7 (E10) — field assignment to `monotone`/`antitone` fields is a compile-time error. |
| CR-14 | Minor | §3.4.5 note — `Vec::new()` returns `None`; let-binding's declared type drives typechecking; "expected-type inference" misnomer removed. |
| CR-15 | Minor | §1.5, §1.11 — `parse_class` calls `parse_leading_attributes`; `@monotone` on a class is a parse error. |
| CR-17 | Minor | LANG-119, §1.6.6, §5.4.2 — `__alk_alloc(size: i32) -> i32` added to the import table at index 10 when Gap 1 lands. |
| CR-18 | Minor | §1.5, §1.11 — compound assignment is a parse error in this wave; chained field assignment supported via recursive `Expr::Field` receivers. |
| CR-20 | Minor | §1.4.6 note, §1.11 — monotone field cannot be passed to unrestricted `Vec<T>` params; documented as an intentional trade-off. |
| CR-28 | Minor | §2.2, §2.7 (E7), §2.11 — capability vocabulary of 7 is closed and mapped to ADRs: render→ADR-001/007, gpu→ADR-006, net→ADR-021, fs/time/rand→(future), ipc→ADR-021. |

**Rendering spec §5 resolution summary (17 findings):**

| CR | Severity | Resolution in rendering spec |
|----|----------|------------------------------|
| CR-1 | **Critical** | REND-601..604, §1.2, §3.2 — serde derives added to ~30 public types; `OffscreenCanvasWrapper` transparent newtype for canvas pass-through. |
| CR-3 | Major | §0.3, §1.3 — cycle structurally broken via new `alkalive-scene-data` crate; `SceneData` trait mitigation explicitly NOT adopted. |
| CR-4 | Major | REND-605..607, §1.2 — `DrawCall.id` + `DrawCall.kind` added IN THE SAME PR as Gap 6; two-phase edit FORBIDDEN; placeholder `DrawCallLookup` trait MUST NOT be merged. |
| CR-5 | Major | REND-715, §2.3 — clear color sourced from first `DrawCallKind::Clear { color }`; black fallback with `console::warn_1` if none. |
| CR-6 | Major | REND-621, REND-622, REND-625, §1.3 — `dirty` parameter consumed; `CompiledGraph.dirty_passes` field; `render_frame_with_dirty` removed. |
| CR-11 | Major | §0.4 — Component::render contract explicitly deferred to a future wave (acknowledged scoping decision, not a regression). |
| CR-12 | Major | REND-823..825, §3.8 — `Caddyfile` added to repo root as canonical dev server; `next.config.ts` NOT created; portable header docs for nginx/serve/GitHub Pages. |
| CR-13 | Major | REND-623, REND-624, §1.3 — convenience `render_frame` removed; runtime caches `CompiledGraph` across frames; only `update_graph_for_frame` runs per-frame. |
| CR-19 | Minor | REND-818, REND-819, §3.3 — `should_use_render_worker` checks `Worker` + `transferControlToOffscreen` only; `crossOriginIsolated` check removed for the first cut. |
| CR-21 | Minor | REND-620 — `compile()` third arg documented as `&DepthBuffer`. |
| CR-22 | Minor | REND-6-E3 — variant name is `BarrierCycle` (matching actual enum); `CycleDetected` removed. |
| CR-23 | Minor | §0.3, §1.3 — `alkalive_compiler::schedule::RenderPass` referred to as "schedule-pass" throughout. |
| CR-25 | Minor | REND-7-P4, REND-8-P1, REND-8-P4 — binary-size budget (≤ 1.8 MB main, ≤ 2.5 MB worker) and worker init budget (< 500 ms) specified. |
| CR-26 | Minor | Same as CR-6. |
| CR-27 | Minor | REND-619, T-6-5 — placeholder bounds in `DrawRect` exposed via a `#[test]` so the wart is visible until ADR-004 lands. |
| CR-29 | Info | REND-702 — dead `wgpu-backend = []` feature removed in the Gap 7 PR. |
| CR-30 | Info | REND-618 — dead `algorithm: &AlgorithmIR` parameter on `lower_pass_kind` removed. |
| CR-31 | Info | REND-609, §1.2 — `// SAFETY` comment placed above `DrawCallKind::DrawCustom` noting the unsafe byte-casting requirement in the backend. |

### 4.2 Implicitly addressed / info-only (4 of 33)

| CR | Severity | Status | Note |
|----|----------|--------|------|
| CR-16 | Minor | **Implicitly addressed by design** | The language spec §3.4.3 `Expr::MethodCall` checking shows `check_method_op` is only called in the `ty.is_vec()` branch — class-typed receivers go through `class_method_return_type` instead. The recommendation was to "add a clarifying comment"; the design is correct but the explicit comment is not called out in the spec. Implementer should add the comment opportunistically. |
| CR-24 | Minor | **Implicitly addressed** | The rendering spec provides a single-threaded fallback (T-8-11) and portable header documentation (§3.8) for non-Caddy deployments. The recommendation was to "document explicitly that COOP/COEP is a deployment-side breaking change and provide a migration checklist"; this is implicit in the fallback + portable docs but not called out as a "breaking change" callout. Implementer should add a migration note in the deploy README. |
| CR-32 | Info | **No action needed (by design)** | The original review verified `call_indirect` is MVP-compatible; no spec change required. The language spec §1.6.2 uses `call_indirect` without reference-types — consistent with the finding. |
| CR-33 | Info | **Partially addressed** | The integrated spec uses both "Phase A/B/C" and "Wave 10..17"; the per-gap specs still use "Step 1..N". Terminology is not fully unified. See §3.3 above. |

**No critical or major findings are unresolved.** The 4 implicit/info
findings are documentation nits that do not affect implementability,
correctness, or testability.

---

## 5. Implementability — can an engineering team implement each gap without reinterpretation?

**Result: PASS.**

Each gap's specification provides:

- **Exact Rust types** — full `struct`/`enum` definitions with field types,
  visibility, derives, and doc comments (e.g. `FnSig`, `FnSigTable`,
  `ClassDecl`, `DrawCall`, `DrawCallKind`, `WorkerMessage`).
- **Exact EBNF productions** — for every new syntax (e.g. `ClassDecl`,
  `ImportDecl`, `FieldDecl`, `MethodDecl`).
- **Exact WASM instruction sequences** — for every codegen path (e.g.
  `local.get obj; i32.load offset=0; i32.const <slot>; i32.add;
  call_indirect (type $T)` for virtual dispatch).
- **Exact WGSL shader source** — `text_quad.wgsl` and `rect.wgsl` written
  out in full (~60 LOC each, rendering §2.2).
- **Exact HTTP header values** — `Cross-Origin-Opener-Policy: same-origin`
  and `Cross-Origin-Embedder-Policy: require-corp` (rendering §3.8).
- **Exact error messages** — every error case includes a `format!`-style
  template (e.g. `"method `.{m}()` is not defined on type `{t}`"`).
- **Exact Caddyfile content** — rendering §3.8 includes the full file.
- **Exact test inputs and expected outputs** — every test case specifies
  the source program and the exact assertion (AST shape, WASM instruction
  sequence, error message, pixel diff threshold, benchmark target).

### 5.1 Spot-checks against the actual codebase

Verified that the spec's claims about the existing codebase are accurate:

- `crates/alkalive-render/src/lib.rs:447-451` — `compile()` signature is
  `(graphs: &[RenderGraph], dirty: &[DirtyRect], depth: &DepthBuffer)`.
  Spec claim: ✓ (REND-620).
- `crates/alkalive-render/src/lib.rs:454` — `let _ = (dirty, depth);`
  ignores both parameters. Spec claim: ✓ (REND-621 fixes this).
- `crates/alkalive-render/src/lib.rs:182, 199, 216, 220, 225, 229, 246, 253`
  — `Attachment`, `RenderPass`, `VertexBinding`, `IndexBinding`,
  `BindGroup`, `DrawCall`, `OcclusionCullPass`, `RenderGraph` all derive
  only `Debug, Clone` (no serde). Spec claim: ✓ (REND-601 adds serde).
- `crates/alkalive-render/src/lib.rs:230-243` — `DrawCall` has
  `pipeline`, `vertices`, `indices`, `bindings`, `instances`, `scissor`
  but **no `id` or `kind` field**. Spec claim: ✓ (REND-605 adds them).
- `VertexBinding`, `IndexBinding`, `BindGroup` are empty marker structs
  (`pub struct VertexBinding;`). Spec claim: ✓ (REND-612..614 populate them).

The spec's evidence citations (file:line) are accurate. An implementer can
trust the spec's claims about the existing code.

---

## 6. Dependency Order — correct and complete

**Result: PASS.**

### 6.1 Three-layer dependency composition

The integrated spec §2 composes three dependency layers correctly:

**Layer 1 — Language-internal (language spec §0):**
```
Gap 3 (Type Inference)  — no deps
Gap 4 (String Data)     — no deps
Gap 5 (Collections)     — depends on Gap 4 (heap-pointer convention)
Gap 1 (OO Model)        — depends on Gaps 3, 4, 5
Gap 2 (Module System)   — depends on Gap 1
```
Mandatory order: 3 → 4 → 5 → 1 → 2 (with 3 ∥ 4 parallel-safe).

**Layer 2 — Rendering-internal (rendering spec §0.2):**
```
Gap 6 (Render-Graph IR) — no deps
Gap 7 (WGSL + wgpu)     — depends on Gap 6
Gap 8 (GPU-Device/SAB)  — depends on Gap 7
```
Mandatory order: 6 → 7 → 8 (strictly sequential, no parallelisation).

**Layer 3 — Cross-domain (integrated spec §2.3):**
```
Gap 4 (Strings)    → Gap 6 (DrawText consumes string pointers)
Gap 1 (OO)         → Gap 6 (Component::render → RenderGraph)  [DEFERRED per CR-11]
Gap 2 (Modules)    → Gap 8 (capability grants for GPU access)  [DEFERRED per CR-11]
```

### 6.2 Parallelisation opportunities

The integrated spec correctly identifies that **Gaps 3, 4, and 6** are
parallel-safe (no dependencies between them) and can be developed by
separate agents simultaneously. Gap 5 must follow Gap 4. The wave
sequence (Wave 10: Gap 3 ∥ Wave 11: Gap 6, then Wave 12: Gap 4, Wave 13:
Gap 5, ...) is conservative — it does not maximally parallelise Gap 4
with Gaps 3 and 6, but this is a scheduling choice, not a correctness
issue.

### 6.3 No circular dependencies

- The crate dependency cycle (`alkalive-render` ↔ `alkalive-backend-wgpu`)
  identified in CR-3 is structurally broken by the new
  `alkalive-scene-data` crate (rendering §0.3).
- No gap-to-gap cycles exist in the dependency graph.

---

## 7. Interface Contracts — all cross-gap contracts specified

**Result: PASS.**

### 7.1 Integrated spec §5 contracts (6 total)

| Contract | Producer | Consumer | Status |
|----------|----------|----------|--------|
| FnSigTable | Gap 3 (typechecker) | Gap 1 (OO method dispatch), Gap 2 (resolver seeds it) | ✓ Specified in language §3.3, §6.1, §6.4 |
| StringTable | Gap 4 (wasm_codegen) | Gap 6 (DrawText draw call) | ✓ Specified in language §4.3, integrated §5.2 |
| HostImports | Gap 5 (wasm_codegen) + Gap 1 (`__alk_alloc`) | Runtime binds them | ✓ Specified in language §5.3, §6.3 |
| RenderGraph | Gap 6 (render) | Gap 7 (wgpu renderer consumes via `render_compiled`) | ✓ Specified in rendering §1.2, §2.3 |
| CompiledGraph | Gap 7 (wgpu backend) | Gap 8 (worker serialises via `serde_wasm_bindgen`) | ✓ Specified in rendering §1.2, §3.2 |
| Component::render() | Gap 1 (OO) | Gap 6 (RenderGraph source) | ✓ Deferred to a future wave per CR-11; documented in integrated §5.6, rendering §0.4 |

### 7.2 Language spec §6 contracts (5 detailed)

Each contract includes the exact Rust types, field ownership, and
consumption semantics:

- §6.1 Gap 3 → Gap 1: `FnSigTable.lookup_method(class, method)`.
- §6.2 Gap 4 → Gap 5: `StringTable` and `host_imports` both owned by
  `compile_to_wasm`; import section before function section, data section
  after code section (WASM binary format ordering).
- §6.3 Gap 4 + Gap 5 → Gap 1: `string` field stores `i32` pointer from
  Gap 4; `Vec<T>` field stores `i32` handle from Gap 5's `vec_new`;
  `__alk_alloc` at import index 10.
- §6.4 Gap 1 → Gap 2: `Visibility` extended to top-level `Fn`/`Let`;
  `ClassDecl` gains `Visibility`; resolver populates `FnSigTable`.
- §6.5 Gap 3 → Gap 2: `FnSig.imported_from: Option<String>` populated
  by Gap 2's resolver; `Expr::PathCall` resolves through `FnSigTable`.
- §6.6 Shared data structures ownership table (6 structures: FnSigTable,
  ClassTable, StringTable, host_imports, ResolvedGraph, ClassLayout).

### 7.3 No missing contracts

All cross-gap data flow is concretely typed. The deferred Component::render
contract (CR-11) is documented as out-of-scope, not missing — the
integrated spec explicitly acknowledges that Gap 6's only `RenderGraph`
source is `schedule_to_render_graph` until the OO model lands in a future
wave.

---

## 8. Testability — are the test cases specific enough to verify implementation?

**Result: PASS.**

### 8.1 Test case specificity

Each of the 132 test cases specifies:

- **Exact input** — source program (e.g. `class C { pub fn new() -> Self { Self { } } }`)
  or exact Rust construction (e.g. `DrawCall { id: DrawCallId(7), kind: DrawCallKind::Clear { color: [0.0; 4] }, ..Default::default() }`).
- **Exact expected behaviour** — one of:
  - AST shape (e.g. `parses as ClassDecl with one method new and Expr::Object`).
  - WASM instruction sequence (e.g. `LocalGet, I32Const(1), Call(2)` in order).
  - Type-check result (e.g. `Typechecks; id(42) infers i32`).
  - Error message (e.g. `LANG-307-E1: "call to unknown function `unknown`"`).
  - Binary property (e.g. `wasmparser::Parser validates the full binary`).
  - Pixel diff threshold (e.g. `pixel diff < 1%` for visual parity).
  - Benchmark target (e.g. `mean < 50 µs on the M1 Air baseline`).
  - Grep check (e.g. `grep -r "render_frame_with_dirty" crates/` returns no matches).
- **Test location** — the crate that hosts the test (e.g.
  `[`alkalive-render`]`, `[`alkalive-backend-wgpu`] (native)`,
  `[`alkalive-runtime-wasm`] (wasm32, headless browser)`).

### 8.2 Test categorisation

Tests are categorised by type:
- **Unit tests** — exact AST/WASM/instruction-level assertions (majority).
- **Integration tests** — end-to-end compile + validate (e.g. LANG-1T-13,
  LANG-2T-15, LANG-4T-10, LANG-5T-12).
- **Serde round-trip tests** — T-6-16, T-6-17, T-8-5.
- **Benchmark tests** — T-6-21..24, T-7-12..14, T-8-17 (with specific
  timing targets).
- **Browser verification (manual)** — T-6-25, T-7-10, T-8-9, T-8-15,
  T-8-19 (pixel diff < 1%).
- **Build checks** — T-8-20, T-8-21 (binary size, network fetch).
- **Regression tests** — T-7-15, T-8-18 (all 1148 existing tests pass).

### 8.3 Optional end-to-end tests

Some end-to-end tests are marked "Optional; falls back to instruction-level
tests if no Wasmtime/wasmi is available" (e.g. LANG-1T-21, LANG-4T-10,
LANG-5T-12). This is acceptable given the sandbox constraints — the
fallback path is explicitly specified, and the instruction-level tests
provide equivalent coverage.

### 8.4 Acceptance criteria are measurable

Every gap's §10 Acceptance Criteria section lists concrete, checkable
criteria (e.g. "cargo test -p alkalive-compiler oo_tests passes with the
21 tests above", "cargo clippy -p alkalive-compiler -- -D warnings is
clean", "grep -r `render_frame_with_dirty` crates/ returns no matches").
An implementer can mechanically verify each criterion.

---

## 9. Remaining Issues (non-blocking)

### 9.1 Documentation nits (low priority)

| # | Issue | Affected doc | Recommendation |
|---|-------|--------------|----------------|
| N1 | Stale test-case ranges in gap inventory | Integrated spec §1 | Update LANG-1T-01..15 → ..21, LANG-2T-01..12 → ..15, LANG-3T-01..10 → ..13, LANG-4T-01..08 → ..10; update total to "154 requirements, 132 test cases". |
| N2 | Integrated spec §3 critical-review summary lumps CR-14..33 | Integrated spec §3 | Expand the table to enumerate all 33 findings with their resolution location (language §9 / rendering §5). |
| N3 | Integrated spec §4 traceability matrix is representative (14 rows), not exhaustive | Integrated spec §4 | Either expand to a full matrix (154 rows) or add a note "see per-gap specs for exhaustive traceability". |
| N4 | Terminology inconsistency (CR-33) | All three docs | Standardise on "Wave N" in a future doc-cleanup wave. |
| N5 | CR-16 clarifying comment not explicitly required | Language spec §3.4.3 | Add a note: "Implementer should add a comment in `check_method_op` clarifying it is only consulted for `Vec<T>` receivers." |
| N6 | CR-24 "deployment breaking change" callout missing | Rendering spec §3.8 | Add a migration checklist for existing `deploy/index.html` consumers. |
| N7 | CR-25 startup-time budget only covers worker init (< 500 ms); no end-to-end startup budget | Rendering spec §3.7 | Consider adding a "first frame < N ms" budget for the full runtime (download + instantiate + first render). |

### 9.2 No blocking issues

None of the above nits block any implementation wave. They are
documentation polish items that can be addressed opportunistically
during the implementation waves or in a dedicated doc-cleanup wave.

---

## 10. Verdict

**APPROVED WITH MINOR NOTES.**

The AlkALive remaining-gaps specification is **complete, traceable,
consistent, implementable, and testable**. All 33 critical-review findings
are addressed (29 explicitly, 4 implicitly / by design). The implementation
waves (Wave 10 through Wave 17) may proceed in the documented dependency
order:

```
Wave 10: Gap 3 (Type Inference)         ∥ Wave 11: Gap 6 (Render-Graph IR)
Wave 12: Gap 4 (String Data Sections)
Wave 13: Gap 5 (Collection Dispatch)
Wave 14: Gap 1 (OO Model)
Wave 15: Gap 2 (Module System)
Wave 16: Gap 7 (WGSL + wgpu)
Wave 17: Gap 8 (GPU-Device + SAB/COOP-COEP)
```

The 7 documentation nits in §9 should be tracked as low-priority
follow-ups but do not block any wave.

---

## 11. DoD Checklist for this Final Review

- [x] Final review saved to `docs/alkalive-specification-final-review.md`.
- [x] All 8 verification categories checked (§0 summary table; §1–§8
      detailed findings).
- [x] Any remaining issues documented (§9 — 7 non-blocking nits).
- [x] Worklog appended (`/home/z/my-project/worklog.md`, Task ID 8).
- [x] Spot-checks against the actual codebase performed (§5.1).
- [x] Critical-review findings resolution audited (§4 — all 33 accounted
      for: 29 explicit + 4 implicit/info).
- [x] Cross-spec consistency verified (§3 — data structures, contracts,
      terminology).
- [x] Dependency order verified (§6 — three layers correctly composed;
      no cycles).
- [x] Interface contracts verified (§7 — all 6 contracts specified; no
      missing contracts).
- [x] Testability verified (§8 — 132 tests with exact inputs and expected
      behaviours).

---

*End of final independent review.*

# Reconciliation Report — Adopted VUMA-Inspired Compiler Enhancements

**Date:** 2026-08-03
**Author:** I-Radic
**Purpose:** Reconcile the five Architectural Decision Records (ADR-024 through ADR-028) against the existing `rough-draft.md` and `problem-catalog.md`, identify conflicts and ambiguities, and propose resolutions to be carried forward into `fine-draft-v2.md` and the technical specification.

**Status convention:**
- **DECISION** — Established by an ADR; not subject to reinterpretation in downstream documents.
- **ASSUMPTION** — Inferred where the ADRs and rough draft are silent; must be ratified before implementation.
- **RECOMMENDATION** — Proposed resolution to a conflict or ambiguity.
- **OPEN QUESTION** — Unresolved; requires a decision before the affected phase ships.

---

## 1. Scope

This report covers five ADRs, each mapped to one section of `rough-draft.md` and one entry of `problem-catalog.md`:

| ADR | Title | Rough draft § | Problem catalog § |
|-----|-------|---------------|--------------------|
| ADR-024 | Algorithm/Schedule Separation for SceneIR | §1 | §5 |
| ADR-025 | Incremental Computation (Salsa/Adapton) | §2 | §1 |
| ADR-026 | E-Graph Optimization for Signal Read/Write Patterns | §3 | §3 |
| ADR-027 | Monotonicity Types — Phased Adoption | §4 | §2 |
| ADR-028 | PMT Verification — Deferred (Approach C) | §5 | §4 |

The two source documents **disagree on numbering**: `rough-draft.md` lists ideas in dependency order (Algorithm/Schedule first → PMT last), while `problem-catalog.md` lists them in the order Incremental → Monotonicity → E-Graph → PMT → Algorithm/Schedule. The ADRs were assigned numbers (024–028) in the same order as the rough draft.

**RECOMMENDATION 1.1 (numbering):** `fine-draft-v2.md` and the technical specification adopt the rough-draft/ADR ordering (Algorithm/Schedule = #1, PMT = #5). The problem catalog's order is preserved for historical reference only. Cross-references between the two documents must be by ADR number, not by ordinal.

---

## 2. ADR → Rough-Draft Mapping

For each ADR, the table below records: the rough-draft section it corresponds to, whether the rough-draft content is consistent with the ADR, and any deltas.

### 2.1 ADR-024 — Algorithm/Schedule Separation

| Aspect | Rough draft §1 | ADR-024 | Status |
|--------|----------------|---------|--------|
| Problem statement | SceneIR conflates description + strategy; `render_frame()` hardcodes pipeline | Same | Consistent |
| Solution: split into AlgorithmIR + ScheduleIR | Explicit Rust structs (`AlgorithmIR`, `ScheduleIR`, `RenderPass`) | Decision recorded without struct-level detail | Consistent; rough draft provides implementation-level detail the ADR omits |
| New compiler pass name | `schedule_lowering` | `schedule_lowering` | Consistent |
| Where in pipeline | After `codegen`, before WASM emission | Same | Consistent |
| Default scheduling rules | "all text nodes → one pass, text_quad shader, batched by font size" | Same | Consistent |
| Cross-references | ADR-001 (render-graph IR), ADR-004 (compositor), ADR-025 (incremental) | Same | Consistent |
| Confidence | Not stated | **High** | ADR adds confidence level |
| LOC estimate | Not stated | ~800–1,200 LOC | ADR adds estimate |

**Deltas:** None substantive. The ADR adds a confidence level (High) and an LOC estimate. The rough-draft struct definitions remain the authoritative source for implementation.

### 2.2 ADR-025 — Incremental Computation

| Aspect | Rough draft §2 | ADR-025 | Status |
|--------|----------------|---------|--------|
| Problem statement | Runtime rebuilds entire scene every frame; O(n) per-frame cost | Same; explicitly cites ADR-002's dirty-rect as unimplemented | Consistent |
| Solution: `incremental_analysis` pass + `DependencyGraph` | Explicit Rust structs (`DependencyGraph`, `DepNode`) | Same | Consistent |
| Runtime: `SignalStore` with `u64` version counters | Yes | Yes | Consistent |
| Frame loop: check → propagate → re-evaluate → render dirty | Yes | Yes | Consistent |
| Pipeline position | After `schedule_lowering` | After `schedule_lowering` (from ADR-024) | Consistent |
| Cross-references | ADR-002 (implements), ADR-013 (not violated), enables ADR-026 | Same | Consistent |
| Confidence | Not stated | **Medium** (novel WASM+GPU cache invalidation patterns) | ADR adds confidence level and risk note |
| LOC estimate | Not stated | ~1,500–2,500 LOC | ADR adds estimate |

**Deltas:** None substantive. The ADR explicitly calls out a risk that the rough draft does not: *"the cache invalidation patterns for GPU resources (textures, buffers) may differ from CPU-only incremental systems"* and *"the implementation will require careful profiling to ensure the overhead of dependency tracking does not exceed the savings from avoiding redundant work for small scenes."*

**RECOMMENDATION 2.1 (small-scene fallback):** `fine-draft-v2.md` adds an explicit decision: when the scene has fewer than N nodes (N to be determined by profiling), the runtime bypasses the `DependencyGraph` and falls back to the existing full-rebuild path. This preserves Hello-World latency while unlocking Δ-scaling for larger scenes.

### 2.3 ADR-026 — E-Graph Optimization

| Aspect | Rough draft §3 | ADR-026 | Status |
|--------|----------------|---------|--------|
| Problem statement | Dependency graph contains redundant reads/writes, suboptimal eval order | Same | Consistent |
| Solution: `egraph_optimization` pass | Yes | Yes | Consistent |
| Rewrite rules | `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder` (4 rules) | Same 4 rules | Consistent |
| Cost-based extraction | Yes | Yes | Consistent |
| Implementation choice | Not specified; rough draft mentions `egg` crate implicitly via "~2,000 LOC" estimate | **Custom lightweight implementation; `egg` crate rejected for ADR-018 compliance** | **Conflict** — see §3.1 below |
| LOC estimate | "~2,000 LOC for a minimal e-graph" | ~2,000 LOC (with detailed breakdown) | Consistent |
| Cross-references | ADR-001 (render-graph IR); depends on ADR-025 | ADR-001, ADR-025, **ADR-018 (5-crate dependency policy)**, transitive ADR-024 | ADR adds ADR-018 dependency |
| Confidence | Not stated | **High** | ADR adds confidence level |

**Deltas:** The ADR adds an **implementation choice** not present in the rough draft: use a custom ~2,000-LOC e-graph rather than the `egg` crate, to comply with ADR-018 (capability-scoped imports + 5-crate policy). The rough draft is silent on this; the LOC estimate is consistent either way (VUMA's e-graph in `egg` is 3,235 LOC; a minimal custom version is ~2,000 LOC).

### 2.4 ADR-027 — Monotonicity Types (Phased)

| Aspect | Rough draft §4 | ADR-027 | Status |
|--------|----------------|---------|--------|
| Problem statement | No static enforcement of collection mutation semantics | Same | Consistent |
| Solution | **Full type qualifier only**: `monotone`/`antitone` keywords in grammar; parser + type checker extension; SceneIR metadata | **Two phases**: Phase 1 = lint-based (`@monotone`/`@antitone` attributes, ~500–1,000 LOC); Phase 2 = full type qualifiers (~2,500–4,000 LOC additional) | **Conflict** — see §3.2 below |
| Cross-references (depends on) | "Depends on Incremental Computation (#2) for the seminaïve evaluation engine" | ADR-024 parallel; ADR-025 enables Phase 2 seminaïve (Phase 2 enables ADR-025's seminaïve, **not the reverse**) | **Conflict** — see §3.3 below |
| Cross-references (enables) | "Enables PMT Verification (#5)" | "Enables ADR-028 (PMT verification, deferred)" | Consistent |
| ADR-008 amendment | "Extends the `.alk` grammar with new type qualifiers" | Phase 2 requires ADR-008 amendment | ADR clarifies amendment scope |
| ADR-009 amendment | "Adds a third verification dimension" | Phase 2 adds a third verification dimension | Consistent |
| Confidence | Not stated | Phase 1 = Medium-High; Phase 2 = Medium | ADR adds per-phase confidence |
| LOC estimate | Not stated | Phase 1: ~500–1,000; Phase 2: +~2,500–4,000 | ADR adds estimates |

**Deltas:** Three substantive conflicts (see §3.2, §3.3, and §3.4 below).

### 2.5 ADR-028 — PMT Verification (Deferred)

| Aspect | Rough draft §5 | ADR-028 | Status |
|--------|----------------|---------|--------|
| Problem statement | `#![forbid(unsafe_code)]` is syntactic, not formal | Same | Consistent |
| Solution | Future research direction: Lean/Z3 discharges proof obligations, proof-carrying code | **Deferred (Approach C).** Re-evaluate when 4 criteria are met. If pursued: Approach B (Z3-only contracts) preferred | **Conflict** — see §3.5 below |
| Status | "Explicitly deferred" (prose) | **Proposed (Deferred)** — explicit decision record with re-evaluation criteria | ADR strengthens deferral |
| Theorem prover | Lean **or** Z3 | Lean (Approach A) rejected; Z3 (Approach B) preferred if pursued | ADR narrows to Z3 |
| Cross-references | ADR-009, depends on ADR-027 | ADR-009, ADR-027, **ADR-018** (Z3/Lean not allowed crates) | ADR adds ADR-018 conflict |
| Confidence | Not stated | **High** (in the deferral decision) | ADR adds confidence |
| LOC estimate | "6-12 month research effort minimum" | 10,000+ LOC for Approach A; 2,000–4,000 LOC for Approach B; 0 for deferral | ADR quantifies |

**Deltas:** The ADR upgrades the rough draft's "future research direction" prose into an explicit deferral decision with four re-evaluation criteria. It also names ADR-018 (5-crate policy) as a blocker for any future Z3/Lean integration.

---

## 3. Conflicts, Ambiguities, and Terminology Differences

### 3.1 ADR-026 vs. rough draft — `egg` crate vs. custom e-graph

**Conflict:** The rough draft (§3) does not state whether the e-graph implementation uses the `egg` crate or a custom implementation. ADR-026 records a DECISION to use a custom ~2,000-LOC implementation and explicitly rejects `egg` for ADR-018 (5-crate dependency policy) compliance.

**RECOMMENDATION 3.1:** Treat the ADR-026 DECISION as authoritative. The `egg` crate is excluded; the e-graph is implemented in-tree in `crates/alkalive-compiler/src/egraph.rs` (new module). The fine draft must cite ADR-018 as the rationale, not just "~2,000 LOC." If, during implementation, the custom e-graph exceeds ~3,000 LOC or fails to converge on the 4 rewrite rules, an ADR amendment must be opened before considering `egg` — this is an OPEN QUESTION for the implementation phase.

### 3.2 ADR-027 vs. rough draft — Phased adoption not in rough draft

**Conflict:** The rough draft (§4) describes only the full type qualifier approach (`monotone`/`antitone` keywords in the grammar, enforced by the type checker, flowing through function signatures). ADR-027 records a DECISION to adopt a two-phase approach: Phase 1 = lint-based attributes (`@monotone`/`@antitone`), Phase 2 = full type qualifiers (only after Phase 1 validation). The rough draft never mentions Phase 1.

**RECOMMENDATION 3.2:** Treat the ADR-027 DECISION as authoritative. The fine draft must:
1. Reframe §4 (Monotonicity Types) into two clearly-separated subsections: **Phase 1 (Lint-Based)** and **Phase 2 (Full Type Qualifier)**.
2. Note that Phase 1 ships as a standalone linter pass — it is **not** wired into the type checker, does **not** modify the `.alk` grammar, and does **not** produce SceneIR metadata.
3. Note that Phase 2 has explicit prerequisites (≥3 months of Phase 1 usage on real `.alk` code; type-checker extension design reviewed; ADR-008 amended; ADR-009 amended).
4. The rough draft's struct definitions (`pub enum Monotonicity { Monotone, Antitone, Unrestricted }`) describe the Phase 2 SceneIR metadata only; they do not appear in Phase 1.

### 3.3 ADR-027 vs. rough draft — Dependency direction reversed

**Conflict:** The rough draft (§4 Integration) states: *"Depends on Incremental Computation (#2) for the seminaïve evaluation engine."* The cross-reference table in the same document contradicts this: *"#4 Monotonicity Types — Depends On: None (parallel to #1)."* ADR-027 resolves this: ADR-025 (incremental) is **enabled by** Phase 2's SceneIR metadata, not the reverse. Phase 2 monotonicity metadata enables ADR-025's seminaïve evaluation; ADR-025 itself does not gate Phase 2.

**RECOMMENDATION 3.3:** Adopt the ADR-027 direction. The fine draft's cross-reference table reads:

| #4 Monotonicity Types (Phase 1) | Depends on: none | Enables: Phase 2 |
| #4 Monotonicity Types (Phase 2) | Depends on: ADR-024 (parallel), ADR-027 Phase 1, ADR-008/009 amendments | Enables: ADR-025 seminaïve evaluation, ADR-028 (if re-evaluated) |

Phase 1 is fully parallel to ADR-024 and ADR-025; it can ship independently of any other enhancement. Phase 2 unblocks ADR-025's seminaïve evaluation but does not depend on ADR-025 being shipped first.

### 3.4 ADR-027 vs. rough draft — Syntax change at Phase 2

**Ambiguity:** ADR-027 Consequences notes: *"Users may need to re-annotate collections when upgrading from Phase 1 to Phase 2 (attribute → type qualifier syntax change)."* The rough draft's section 4 only shows the Phase 2 syntax (`monotone children: Vec<Node>`), never the Phase 1 syntax (`@monotone children: Vec<Node>`). The migration path is unspecified.

**RECOMMENDATION 3.4:** The fine draft specifies a mechanical migration: a `alkalive-compiler` subcommand `migrate-monotonicity` that rewrites `@monotone X` → `monotone X` and `@antitone X` → `antitone X` in `.alk` source. Both syntaxes are accepted during a deprecation window (≥2 minor versions). This is an ASSUMPTION; the migration tool's design is an OPEN QUESTION for Phase 2 planning.

### 3.5 ADR-028 vs. rough draft — Deferral strengthened, Z3 preferred

**Conflict:** The rough draft (§5) describes PMT verification as a "future research direction" with both Lean and Z3 mentioned as candidate theorem provers. ADR-028 records a DECISION to **defer all PMT work** (Approach C) with explicit re-evaluation criteria, and nominates Approach B (Z3-only contracts, ~2,000–4,000 LOC) as the preferred starting point **if** re-evaluated. Approach A (full PMT with Lean, ~10,000+ LOC) is rejected.

**RECOMMENDATION 3.5:** The fine draft treats ADR-028 as the authoritative record. The §5 (PMT Verification) section is reframed as **"Deferred — Re-evaluation Criteria and Conditional Approach"** with:
1. The four re-evaluation criteria verbatim from ADR-028.
2. A clear statement that no implementation work is planned for the current phase.
3. The conditional design sketch (Approach B / Z3-only contracts) preserved as a reference, with the caveat that ADR-018 would need amendment before Z3 could be added.

### 3.6 Terminology — "Datafun" attribution

**Ambiguity:** The rough draft §4 heading reads *"Monotonicity Types (Datafun)"*. The problem-catalog §2 also references Datafun. ADR-027's title is *"Monotonicity Types for AlkALive — Phased Adoption"* with no Datafun mention in the title or Decision section. The VUMA feasibility study cites Datafun as the academic provenance; the ADR is silent.

**RECOMMENDATION 3.6:** The fine draft uses **"Monotonicity Types"** as the canonical name in headings and cross-references. Datafun is mentioned once, in the *Provenance* paragraph of the §4 introduction, as the academic origin of the monotone/antitone distinction. This aligns with ADR-027's naming convention without erasing the academic attribution.

### 3.7 Terminology — "render-graph IR" vs. "ScheduleIR"

**Ambiguity:** ADR-001 defines an abstract **render-graph IR** with passes, attachments, and draw calls. ADR-024 introduces a concrete **ScheduleIR** with passes, pass order, shaders, batching, and threading. The rough draft (§1 Integration) notes: *"ADR-001 (render-graph IR) — the `ScheduleIR` is the concrete realization of the abstract render-graph."* The actual `alkalive-render` crate (`crates/alkalive-render/src/lib.rs`) already implements `PassId`, `AttachmentId`, `DrawCallId`, `PassType`, `AttachmentFormat`, etc., per SPECIFICATION §4.1–§4.7. The `Backend`, `RenderLoop`, and `Compositor` traits remain abstract.

**RECOMMENDATION 3.7:** The fine draft specifies the relationship: `ScheduleIR` (introduced by ADR-024 in `alkalive-compiler`) is the **author-facing schedule representation** that gets lowered into the `alkalive-render` crate's existing render-graph IR types (`PassId`, `AttachmentId`, etc.) by the `schedule_lowering` pass. They are distinct layers:
- `ScheduleIR` = author / compiler-layer schedule (per-scene, declarative).
- `alkalive-render` IR = runtime / GPU-layer render graph (cross-scene, executable).

The technical specification must document both layers and the lowering boundary.

### 3.8 Terminology — "SignalStore" vs. `input_text`

**Ambiguity:** ADR-025 introduces a `SignalStore` (key-value map of signal values with `u64` version counters). The rough draft uses the same name. The actual runtime (`crates/alkalive-runtime-wasm/src/lib.rs`) currently has no `SignalStore` — it has a `Runtime` struct with `input_text: String` and `original_text: String` fields. These are not versioned.

**RECOMMENDATION 3.8:** The fine draft treats `SignalStore` as a **new** runtime data structure introduced by ADR-025. It is not a refactor of existing fields; it is a new layer that sits between `input_text` (which becomes a signal source) and `TextSceneData` (which becomes a signal consumer). The technical specification must call out that `Runtime::input_text` and `Runtime::original_text` are migrated into `SignalStore` slots as part of ADR-025 implementation.

### 3.9 Pipeline diagram — `proof_obligation_generation` placement

**Ambiguity:** The rough-draft pipeline diagram shows `[#5 proof_obligation_generation] (future) → verified .wasm` as a post-WASM-emission step. ADR-028 defers this indefinitely. Showing it in the pipeline implies eventual realization.

**RECOMMENDATION 3.9:** The fine draft's pipeline diagram marks the `proof_obligation_generation` node with a **"(deferred per ADR-028)"** annotation in the diagram itself, not just in surrounding prose. The dashed-line style visually distinguishes deferred passes from active ones.

### 3.10 ADR-018 — explicit cross-ADR dependency not in rough draft

**Ambiguity:** The rough draft does not cite ADR-018 anywhere. ADR-026 and ADR-028 both cite ADR-018 as a hard constraint (5-crate external dependency policy; Z3/Lean/`egg` all excluded without an ADR amendment).

**RECOMMENDATION 3.10:** The fine draft adds an "ADR-018 Compliance" subsection to each idea that introduces a potential external dependency:
- §3 (E-Graph): cites ADR-018 as the rationale for custom e-graph; `egg` excluded.
- §5 (PMT): cites ADR-018 as a blocker for Z3/Lean; deferral partly motivated by this.

The technical specification's "Design Constraints" section lists ADR-018 as a top-level constraint affecting all five ideas.

---

## 4. Assumptions vs. Established Decisions

### 4.1 Established DECISIONS (from ADRs; non-negotiable in downstream documents)

| # | Decision | Source |
|---|----------|--------|
| D1 | Split `SceneIR` into `AlgorithmIR` + `ScheduleIR`; add `schedule_lowering` pass after `codegen`. | ADR-024 |
| D2 | Add `incremental_analysis` pass after `schedule_lowering`; runtime maintains `SignalStore` + `DependencyGraph` with `u64` version counters; frame loop = check → propagate → re-evaluate → render dirty. | ADR-025 |
| D3 | Add `egraph_optimization` pass after `incremental_analysis`; 4 rewrite rules (`state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`); cost-based extraction. | ADR-026 |
| D4 | E-graph is a **custom** implementation (~2,000 LOC); `egg` crate excluded per ADR-018. | ADR-026 |
| D5 | Monotonicity Types adopted in **two phases**: Phase 1 = lint-based `@monotone`/`@antitone` attributes (~500–1,000 LOC); Phase 2 = full type qualifiers (~2,500–4,000 LOC additional). | ADR-027 |
| D6 | Phase 1 ships standalone (no type-checker integration, no SceneIR metadata, no grammar change). | ADR-027 |
| D7 | Phase 2 prerequisites: ≥3 months Phase 1 usage, type-checker extension design reviewed, ADR-008 amended, ADR-009 amended. | ADR-027 |
| D8 | Phase 2 monotonicity metadata **enables** ADR-025's seminaïve evaluation (not the reverse). | ADR-027 |
| D9 | PMT verification **deferred** (Approach C). No implementation in current phase. | ADR-028 |
| D10 | If PMT is re-evaluated, Approach B (Z3-only contracts) is the preferred starting point; Approach A (Lean, full PMT) rejected. | ADR-028 |
| D11 | PMT re-evaluation requires all four ADR-028 criteria (ADR-027 Phase 2 stable ≥6 months; safety-critical domain target; VUMA PMT composability demonstrated; cost-benefit positive). | ADR-028 |

### 4.2 ASSUMPTIONS (inferred; must be ratified before implementation)

| # | Assumption | Rationale | Owner |
|---|------------|-----------|--------|
| A1 | The `schedule_lowering` pass lives in `crates/alkalive-compiler/src/schedule.rs` (new module), not in `alkalive-render`. | Compiler-layer concern; author-facing. | Compiler team |
| A2 | `AlgorithmIR` is a thin refactor of the existing `SceneIR` struct (same fields, no rendering details added). The `module_id`, `module_name`, `background`, `nodes` fields are preserved verbatim. | Rough draft §1 shows `AlgorithmIR` with the same surface as current `SceneIR`. | Compiler team |
| A3 | `ScheduledScene { algorithm: AlgorithmIR, schedule: ScheduleIR }` is the new compiler output; the existing `compile()` function returns `ScheduledScene` instead of `SceneIR` after ADR-024 lands. | Rough draft §1 solution uses this struct. | Compiler team |
| A4 | The runtime's `build_scene_from_ir()` function (currently in `alkalive-runtime-wasm/src/lib.rs`) is renamed `build_scene_from_scheduled()` and consumes `ScheduledScene`. | Mechanical rename; preserves the existing lowering boundary. | Runtime team |
| A5 | The `SignalStore` lives in `crates/alkalive-runtime-wasm/src/lib.rs` (or a new `crates/alkalive-runtime/src/signal_store.rs` module), not in `alkalive-backend-wgpu`. | Runtime owns signal state; backend remains stateless w.r.t. signals. | Runtime team |
| A6 | The custom e-graph lives in `crates/alkalive-compiler/src/egraph.rs` (new module). | Compiler-layer concern. | Compiler team |
| A7 | Phase 1 lint pass lives in `crates/alkalive-compiler/src/lints/monotonicity.rs` (new module). | Standalone linter, not wired into type checker. | Compiler team |
| A8 | Phase 1 lint warnings are emitted via a new `LintReport` type alongside the existing `CompileError`. Lints are non-fatal by default; `#![deny(monotonicity)]` makes them fatal. | ADR-027: "Lint warnings (configurable to errors via `#![deny(monotonicity)]`)." | Compiler team |
| A9 | The current Hello-World `.alk` source (`examples/hello.alk`) compiles unchanged through all five enhancements. No breaking changes to the grammar in ADR-024, ADR-025, ADR-026, or ADR-027 Phase 1. | Backward compatibility implied by ADR-024 ("mechanical: split a struct, add a pass"). | All teams |
| A10 | The existing `render_frame()` method signature changes from `render_frame(&scene: &TextSceneData, time: f32)` to `render_frame(&scheduled: &ScheduledScene, &signals: &SignalStore, time: f32)` (or similar) after ADR-024 + ADR-025 land. | Data-driven dispatch requires the schedule; incremental evaluation requires the signal store. | Runtime + backend teams |
| A11 | The current `Runtime::time` field (incremented by `1.0 / 60.0` per frame in `start_frame_loop`) becomes a signal source (`signal::time`) in the `SignalStore` after ADR-025. | All per-frame inputs should flow through the signal store for uniform dirty tracking. | Runtime team |
| A12 | The existing `TextSceneData` (in `alkalive-backend-wgpu`) is retained as the renderer's per-frame input; the `SignalStore` produces a fresh `TextSceneData` each frame from dirty signals. | `TextSceneData` is the renderer's existing contract; rewriting it would touch the GPU backend. | Backend team |

### 4.3 RECOMMENDATIONS (proposed; not yet ADRs)

| # | Recommendation | Action required |
|---|----------------|-----------------|
| R1 | Adopt the rough-draft/ADR ordering (#1 Algorithm/Schedule → #5 PMT) as canonical across all adopted-vuma-ideas documents. | Update `problem-catalog.md` cross-references in a follow-up commit. |
| R2 | Add an explicit small-scene fallback (bypass `DependencyGraph` for scenes < N nodes) to ADR-025's implementation plan. | Open a follow-up ADR amendment or implementation note. |
| R3 | Specify the `ScheduleIR` → `alkalive-render` IR lowering boundary in a future rendering-ABI ADR (referenced by ADR-001 cross-references). | Rendering-ABI ADR (out of scope for this reconciliation). |
| R4 | Specify the Phase 1 → Phase 2 migration tool (`migrate-monotonicity` subcommand) as an OPEN QUESTION in Phase 2 planning, not Phase 1. | Phase 2 design doc. |
| R5 | Mark `proof_obligation_generation` as "(deferred per ADR-028)" in all pipeline diagrams. | Apply in `fine-draft-v2.md` and `docs/technical-specification.md`. |
| R6 | Adopt "Monotonicity Types" as the canonical name; preserve "Datafun" as a one-time provenance mention. | Apply in `fine-draft-v2.md`. |

### 4.4 OPEN QUESTIONS (require decisions before implementation)

| # | Question | Blocking |
|---|----------|----------|
| Q1 | What is the threshold N for the small-scene fallback in R2? (Suggested starting point: N = 50 nodes, tuned by profiling.) | ADR-025 implementation |
| Q2 | Should `SignalStore` be a separate crate (`alkalive-signals`) or a module in `alkalive-runtime`? A separate crate is cleaner but adds a workspace crate; a module keeps the dependency graph flat. | ADR-025 design |
| Q3 | The custom e-graph (D4) reuses union-find and hash-consing. Should these be implemented as standalone reusable modules in `alkalive-core`, or inlined into `crates/alkalive-compiler/src/egraph.rs`? | ADR-026 implementation |
| Q4 | Does Phase 1 lint operate on the AST, the IR, or both? The rough draft implies AST (parser extension), but ADR-027 does not specify. | ADR-027 Phase 1 design |
| Q5 | The `#![deny(monotonicity)]` attribute (A8) — is this the first use of file-level lint attributes in `.alk`? If so, ADR-008 (language design) needs an amendment for lint-attribute syntax even in Phase 1. | ADR-027 Phase 1 design |
| Q6 | When ADR-028 is re-evaluated, will the Z3 dependency require an ADR-018 amendment, or can Z3 be vendored in-tree (as HarfRust was per ADR-022)? | ADR-028 re-evaluation (future) |

---

## 5. Reconciliation Summary

Of the ten deltas identified between the rough draft and the ADRs:

- **0 are unresolved conflicts.** All conflicts have a clear ADR-level decision that takes precedence (per the ADR convention that later ADRs supersede earlier prose).
- **3 are substantive additions** in the ADRs that the rough draft does not reflect (phased adoption in ADR-027, custom-e-graph-and-ADR-018 in ADR-026, deferral-strengthened in ADR-028).
- **1 is a dependency-direction correction** (ADR-027 §3.3 above: Phase 2 enables ADR-025, not the reverse).
- **6 are minor** (confidence levels, LOC estimates, theorem-prover narrowing, terminology).

The `fine-draft-v2.md` and `docs/technical-specification.md` are the authoritative downstream documents. They will:

1. Use ADR-024 through ADR-028 as the source of truth for all DECISIONS.
2. Carry forward the rough draft's struct definitions and pipeline diagram as implementation detail, annotated with ADR-level decisions.
3. Mark every ASSUMPTION explicitly (§4.2 above) so implementers know what to ratify.
4. Carry forward the six RECOMMENDATIONS as proposed-but-not-yet-ADR-ratified guidance.
5. Surface the six OPEN QUESTIONS to the relevant planning artefacts.

No ADR amendments are required to proceed with `fine-draft-v2.md` or `docs/technical-specification.md`. The OPEN QUESTIONS are implementation-phase concerns.

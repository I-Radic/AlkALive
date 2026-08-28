# AlkALive Master Backlog & Gap Analysis Matrix

**Version:** 1.0
**Date:** 2026-08-27
**Author:** I-Radic
**Status:** Wave-0 deliverable (Implementation Orchestrator)
**Source of truth:** `docs/technical-specification.md` v1.0 (786 lines, §1–§9, read in full — no section skipped)

**Purpose:** Every functional requirement, data model, interface, and business rule in
`docs/technical-specification.md` carries a unique backlog ID and a gap status
(`[EXISTS]`, `[PARTIAL]`, `[MISSING]`) against the codebase at the Wave-0 HEAD. This
document is the master backlog driving implementation Waves 1–7.

**Verification method:** three parallel sub-agent audits (0-a compiler, 0-b
runtime/backend/text, 0-c workspace/render/support/docs) with file:line evidence,
cross-checked against `cargo check --workspace` (clean) and the 1104-test suite status
recorded in CI. Disputed points (ADR-018 attribution, wasm-encoder sanction) were
re-verified directly against `docs/adr/ADR.md`, `deny.toml`, and ADR-026/028.

---

## 1. Specification Coverage Map (all §1–§9 accounted)

| Spec section | Content | Backlog IDs |
|---|---|---|
| §1 Introduction | workspace layout, 19 crates, 3 tiers | REQ-043, ENTITY tables |
| §2 Component Overview | 5 enhancements, build order | REQ-003…REQ-029 |
| §3.1 Compiler pipeline | 5 modules + CLI | REQ-001, REQ-002, API-001…API-007 |
| §3.2 Runtime | start(), frame loop, input, TD3 | REQ-030…REQ-035, REQ-032 |
| §3.3 Backend | WebGL2 renderer, TD1/TD2 | REQ-036, ENTITY-010/011 |
| §3.4 Text stack | traits, limits | REQ-038, REQ-039, ENTITY-013 |
| §3.5 Render-graph IR | types, compiler, TD6 | REQ-040, ENTITY-012, API-012 |
| §4.1 ADR-024 | schedule separation | REQ-003…REQ-006, ENTITY-002 |
| §4.2 ADR-025 | incremental | REQ-007…REQ-011, ENTITY-004/005 |
| §4.3 ADR-026 | e-graph | REQ-012…REQ-016, ENTITY-006 |
| §4.4 ADR-027 P1 | lint | REQ-017…REQ-021 |
| §4.5 ADR-027 P2 | type qualifier | REQ-022…REQ-028 |
| §4.6 ADR-028 | PMT deferred | REQ-029 |
| §5 Component APIs | public interfaces | API-001…API-014 |
| §6 Dependencies | dep graph, ADR matrix | RULE-003, DOC-002 |
| §7 Decisions/Constraints | DD1–DD11, A1–A12, C1–C10 | RULE-001…RULE-010 |
| §8 Debt/Risks | TD1–TD10, R1–R9 | REQ-010, REQ-009, DOC items |
| §9 Architecture | invariants 1–6, §9.2 amendment | RULE-004…RULE-010, REQ-041 |

---

## 2. Functional Requirements (REQ)

| ID | Requirement (spec source) | Status | Evidence / gap |
|----|---------------------------|--------|----------------|
| REQ-001 | Three-stage pipeline lex→parse→lower; manual JSON `to_json()`; no serde in library mode (§3.1, C9) | [EXISTS] | codegen.rs:260, ir.rs:248 |
| REQ-002 | CLI `compile <input> -o <out>` with `--lint`, `--scheduled` (§3.1) | [EXISTS] | main.rs:71–117 |
| REQ-003 | `schedule_lowering(algorithm) -> ScheduleIR` with passes + pass_order (§4.1) | [EXISTS] | schedule.rs:148; as-built emits 5 passes (explicit `Clear` first) — superset of spec's "minimal viable" 4-pass sketch (§4.1 calls 4-pass "naive mapping"; §8.3 rec #2 says minimal viable). DOC-006 records the delta |
| REQ-004 | `SceneIR` → `AlgorithmIR` rename w/ `to_json()` parity (§4.1) | [EXISTS] | ir.rs:43, alias ir.rs:65 |
| REQ-005 | Runtime `build_scene_from_scheduled(&ScheduledScene)` (§4.1) | [EXISTS] | runtime-wasm lib.rs:718 |
| REQ-006 | Data-driven pass dispatch from `schedule.passes`/`pass_order` (§4.1, §9.2) | [EXISTS] | `render_frame_internal` backend lib.rs:1100–1180; main path routes via render-graph IR per §9.2 Wave-11 amendment |
| REQ-007 | `incremental_analysis(&ScheduledScene) -> DependencyGraph` (§4.2) | [EXISTS] | incremental.rs:229 |
| REQ-008 | SignalStore: u64 versions; `check_changes()`, `propagate(changes, dep_graph)`, `reevaluate(dirty, cache)` (§4.2) | [PARTIAL] | signal_store.rs:182/206 provide check_changes/propagate; **`reevaluate` does not exist as a named API** — `dirty_passes()` (signal_store.rs:225) covers the re-evaluation decision. Fix: add `reevaluate()` composition API (Wave 2) |
| REQ-009 | Small-scene fallback, N = 50, runtime constant (§4.2 R1/R2, §8.3) | [EXISTS] | runtime-wasm lib.rs:98 `pub const SMALL_SCENE_THRESHOLD: usize = 50` |
| REQ-010 | TD1: lift `HarfRustFontRegistry`, `HarfRustTextShaper`, `HarfRustGlyphAtlas` to long-lived state (§3.4, §4.2, §8.1 TD1, §9.2) | [PARTIAL] | registry+shaper long-lived (backend lib.rs:1319–1336 "M7 fix"; wgpu path OnceLock); **`HarfRustGlyphAtlas` still created fresh per `upload_text_atlas()` (backend lib.rs:1343) and per `tessellate_scene()` (tessellate.rs:82)** — glyph cache resets every re-upload. Fix: persistent atlas with overflow reset (Wave 2) |
| REQ-011 | DependencyGraph serialization for WASM embedding (§4.2 integration table) | [MISSING] | No `to_json()`/serde on DependencyGraph; embedding is in-memory only. Fix: manual-JSON `to_json()` (C9 pattern) surfaced via CLI `--scheduled` diagnostics (Wave 1) |
| REQ-012 | Custom e-graph: `ENode`/`EClass`/`EClassId`/`EGraph`, `add`/`merge`/`find`, union-find (spec: path-halving), hash-consing (§4.3) | [PARTIAL] | egraph.rs:172–470 all present, hash-consing egraph.rs:271; union-find uses path **compression** (find_mut egraph.rs:377–398) vs spec's "path-halving" (functionally ≥). Fix: switch to path-halving for letter-exact conformance (Wave 2, 3-line change) |
| REQ-013 | Exactly 4 rewrite rules: `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`; `RewriteRule` trait (§4.3, §5.1) | [PARTIAL] | Exactly 4 rules as free functions (egraph.rs:895/989/1097/1165); **`RewriteRule` trait missing** (§5.1 future-API type list). Fix: introduce trait + 4 unit-struct impls, applied via registry (Wave 2) |
| REQ-014 | Cost-based extraction (§4.3) | [EXISTS] | `op_cost` egraph.rs:808, `extract` egraph.rs:1273 (takes extra `original` param — as-built necessity, DOC-004) |
| REQ-015 | `egraph_optimization` wired between incremental_analysis and emission (§4.3) | [EXISTS] | egraph.rs:1418; called in compile_full (codegen.rs:533) |
| REQ-016 | No `egg` crate (§4.3, DD4) | [EXISTS] | zero egg refs in all Cargo.tomls |
| REQ-017 | `LintReport`, `LintSeverity{Warning,Deny}`, `LintSet`; `#![deny(monotonicity)]` upgrade (§4.4) | [EXISTS] | lints/mod.rs:41/63/109; parser.rs:1438; upgrade lints/mod.rs:191 |
| REQ-018 | Monotonicity lint scans `@monotone`/`@antitone` attributes **and** illegal ops in scope (`.remove`… on monotone; `.push`… on antitone), emits LintReports (§4.4) | [PARTIAL] | Attribute scanning exists (lints/monotonicity.rs:61); **illegal-op scan lives only in the P2 typechecker** (typechecker.rs:678/681/1578) as hard errors. P1 contract (standalone advisory lint without typechecker) unmet. Fix: op-scan in lints/monotonicity.rs (Wave 2) |
| REQ-019 | `@ident` attribute syntax: `TokenKind::At`, parser, `ast::Attribute` (§4.4) | [EXISTS] | lexer.rs:144, parser.rs:1403, ast.rs:61 |
| REQ-020 | `compile_with_lints` entry point (§4.4) | [EXISTS] | codegen.rs:280 (signature `(src) -> (AlgorithmIR, LintSet)`; §5.1 future table shows a different planned shape — DOC-003) |
| REQ-021 | `--lint` CLI flag (§4.4) | [EXISTS] | main.rs:78 |
| REQ-022 | Typechecker: qualifier lattice, covariant `Vec<T>`, method-op validation, flow, return checks, multi-error, `check_module` (§4.5) | [EXISTS] | typechecker.rs:709/721/1578/781; 34 primary tests |
| REQ-023 | `monotone`/`antitone` reserved keywords; new token/punct kinds (§4.5) | [EXISTS] | lexer.rs:74/76 + keyword map :391 |
| REQ-024 | `fn`/`let` grammar + parse_type/base_type/block/stmt/expr/arg_list (§4.5) | [EXISTS] | parser.rs:391/453/329/347/689/719/814/1060 |
| REQ-025 | IR: `Monotonicity`, `CollectionDeclIR`, `AlgorithmIR.collections`, serialized by `to_json()` (§4.5) | [EXISTS] | ir.rs:130/172/56/269 |
| REQ-026 | `compile_typecheck(src)` = parse → check → lower (§4.5) | [EXISTS] | codegen.rs:299/311 |
| REQ-027 | `seminative.rs` strategies + runtime hooks `has_seminive_collections`/`collection_strategies` (§4.5) | [EXISTS] | seminative.rs; runtime-wasm lib.rs:737 |
| REQ-028 | ADR-008/009 amendments + ADR-027 P2 traceability doc (§4.5) | [EXISTS] | ADR.md:245/391; ADR_027_PHASE2_TRACEABILITY.md |
| REQ-029 | ADR-028 deferral: `proofs.rs`/`z3_backend.rs` absent, no Z3/Lean (§4.6) | [EXISTS] | verified absent repo-wide |
| REQ-030 | `start(canvas, ime_input)` single WASM entry: panic hook, `compile_full(HELLO_ALK_SRC)`, `build_scene_from_scheduled`, canvas dims, WebGPU-first renderer selection with logged fallback reason (§3.2, §5.2) | [EXISTS] | runtime-wasm lib.rs:378–438, select_renderer :599, publish :691 |
| REQ-031 | Bounded WebGPU adapter/device probe (10 s) with `ProbeOutcome::{Available,Unavailable,TimedOut}` (as-built hardening) | [EXISTS] | wgpu_renderer.rs:69; both requests raced (lib.rs:669/688) |
| REQ-032 | Frame loop driven by dirty signals; real elapsed time (TD3 fix: `signal::time` uses `performance.now()`) (§3.2, §4.2, §8.1 TD3) | [EXISTS] | lib.rs:983 `runtime.time = elapsed_seconds()`; legacy `1/60` zero matches. Spec §3.2/§8.1 text still stale — DOC-001 |
| REQ-033 | Input forwarding: keydown (printable/Backspace/Enter/Escape) + IME composition `input` listener → INPUT_TEXT signal; `.forget()` lifetimes (§3.2) | [EXISTS] | lib.rs:798–867 |
| REQ-034 | Resize listener → `renderer.resize()` clamped ≥1×1, canvas signals (§3.2) | [EXISTS] | lib.rs:879–925; clamps both backends |
| REQ-035 | Click hit-test input-field bounds → focus IME input (§3.2) | [EXISTS] | lib.rs:934–964 |
| REQ-036 | WebGL2 backend: GLSL ES 3.00 shaders (rotating title, R8 atlas sampling, premultiplied alpha), VAO/VBO, 512×512 atlas, rect shader (TD2) (§3.3) | [EXISTS] | backend lib.rs:135–238, 627–638 |
| REQ-037 | wgpu/WGSL backend: feature-gated `WgpuBackendRenderer`, frame-plan + tessellate path (§3.2 as-built) | [EXISTS] | wgpu_renderer.rs:610+, frame_plan.rs, tessellate.rs |
| REQ-038 | Text stack: `FontRegistry`/`TextShaper`/`GlyphAtlas` traits + HarfRust impls; `ensure()` cache; `.notdef` tofu never aborts (§3.4, §5.5) | [EXISTS] | text lib.rs:210/319/388/1064/1277 |
| REQ-039 | Security limits `MAX_FONT_SIZE` = 50 MiB, `MAX_TEXT_LENGTH` = 1 MiB, enforced (§3.4) | [EXISTS] | text lib.rs:63/73, enforced :764/:910 |
| REQ-040 | Render-graph IR: opaque IDs, attachments, passes, draw calls, `compile()` graph compiler, 64 MB LRU `PipelineCache`, abstract `Backend`/`RenderLoop`/`Compositor` (TD6 sanctioned) (§3.5, §5.4) | [EXISTS] | render lib.rs:59–935 |
| REQ-041 | `ScheduleIR` routes through `alkalive_render::graph` (build_render_graph) in both backends (§9.2 Wave-11 amendment) | [EXISTS] | graph.rs:228; backend lib.rs:820, wgpu_renderer.rs:840 |
| REQ-042 | `hello.alk` compiles unchanged through all enhancements (A9, C8, invariant 4) | [EXISTS] | examples/hello.alk; 3 compat tests; CI |
| REQ-043 | 19-crate workspace, three tiers, no dependency cycles (§1.2, §6.1) | [EXISTS] | root Cargo.toml; scene-data breaks render↔backend cycle |
| REQ-044 | Deploy pipeline: deterministic build-deploy.mjs, serve.mjs COOP/COEP headers, index.html shell, prebuilt pkg/ (companions, MR-023) | [EXISTS] | repo-root build-deploy.mjs, deploy/serve.mjs, deploy/pkg/ (50.3% shrink, SHA match) |

## 3. Data Models / Entities (ENTITY)

| ID | Entity (spec source) | Status | Evidence / gap |
|----|----------------------|--------|----------------|
| ENTITY-001 | `AlgorithmIR` (+`SceneIR` alias) with all §4.1-preserved fields + P2 `collections` | [EXISTS] | ir.rs:43/65 |
| ENTITY-002 | `ScheduleIR`, `RenderPass`, `BatchingStrategy`, `ShaderId`, **`ThreadAffinity`** (§4.1, §5.1) | [PARTIAL] | schedule.rs:56–124; **`ThreadAffinity` missing** (zero occurrences). Fix (Wave 1): enum + `RenderPass.affinity` field + lowering default (C10 single-threaded) + JSON serialization |
| ENTITY-003 | `ScheduledScene { algorithm, schedule }` | [EXISTS] | schedule.rs:124 |
| ENTITY-004 | `DependencyGraph`, `DepNode`, `DepNodeId`, **`ComputationId`** (§4.2, §5.1) | [PARTIAL] | incremental.rs:80/100/133; **`ComputationId` missing** (role filled by DepNodeId). Fix (Wave 1): documented type alias |
| ENTITY-005 | `SignalStore`, `SignalId`, `SignalValue`, well-known slots (input_text, time, font_size, rotation_speed) (§4.2) | [EXISTS] | signal_store.rs; slots incremental.rs signals (4 spec + 2 canvas — superset) |
| ENTITY-006 | `EGraph`, `ENode`, `EClass`, `EClassId`, **`RewriteRule`** (§4.3, §5.1) | [PARTIAL] | egraph.rs:102–264; **`RewriteRule` trait missing** — covered by REQ-013 fix |
| ENTITY-007 | `LintReport`, `LintSeverity`, `LintSet` (§4.4) | [EXISTS] | lints/mod.rs:41/63/109 |
| ENTITY-008 | AST P2 types: `Type`, `Qualifier`, `BaseType`, `ItemDecl`, `FnDecl`, `Param`, `LetDecl`, `Block`, `Stmt`, `Expr`, `Lit`, `MethodCall`, `ModuleDecl.items`, `denies_monotonicity()` (§4.5) | [EXISTS] | ast.rs:545–867 |
| ENTITY-009 | `Monotonicity`, `CollectionDeclIR` (§4.5) | [EXISTS] | ir.rs:130/172 |
| ENTITY-010 | `TextSceneData` (golden-on-black default, "Hello World!", 64 px, 0.5 rad/s) (§3.3) | [EXISTS] | scene-data lib.rs:35–66 (canonical home; re-exported backend lib.rs:59) |
| ENTITY-011 | `Vertex` (16 B), `Uniforms`, `GlyphQuad`, `build_vertex_buffer`, `quads_from_text` (§3.3) | [EXISTS] | backend lib.rs:69–408 |
| ENTITY-012 | Render-graph types: `RenderGraph`, `RenderPass`, `Attachment`, `DrawCall`, `CompiledGraph`, `DirtyRect`, `PipelineCache` (§3.5) | [EXISTS] | render lib.rs:59–423/669 |
| ENTITY-013 | Text types: `FontId`, `FontRequest`, `FontLoadError`, `ShapeContext`, `ShapedRun`, `ShapeError`, `GlyphKey`, `AtlasSlot`, `Quad` (§3.4) | [EXISTS] | text lib.rs:109–408 |
| ENTITY-014 | `AlkALiveError` — 8 variants (§13 as-built scope) | [EXISTS] | error crate |
| ENTITY-015 | Perf vocabulary: `BreachPolicy`, `FrameBudget`, `TraceSpan`, `FrameBudgetEvent`, `MemoryPool`, `ResourceBudget`, `PerfCounter`, `SpanKind`, `StageId` (§12) | [EXISTS] | perf crate |

## 4. API / Interfaces (API)

| ID | Interface (spec source) | Status | Evidence |
|----|------------------------|--------|----------|
| API-001 | `compile(src) -> Result<AlgorithmIR, CompileError>` — legacy, no typecheck (as-built §4.5 + §3.2 Wave-5 correction; §5.1 future table stale — DOC-003) | [EXISTS] | codegen.rs:260 |
| API-002 | `compile_full(src) -> Result<(ScheduledScene, DependencyGraph), CompileError>` — production chain incl. typecheck + module resolution | [EXISTS] | codegen.rs:517 |
| API-003 | `compile_with_deps` (§4.2) | [EXISTS] | codegen.rs:437 |
| API-004 | `compile_scheduled` (typechecked) | [EXISTS] | codegen.rs:364 |
| API-005 | `compile_typecheck` (§4.5) | [EXISTS] | codegen.rs:299 |
| API-006 | `compile_with_lints` (§4.4) | [EXISTS] | codegen.rs:280 |
| API-007 | `lower`, `tokenize`, `parse` + error types (§5.1) | [EXISTS] | codegen.rs:65, lexer.rs:854, parser.rs:1492 |
| API-008 | `start(canvas, ime_input)` — stable signature (§5.2) | [EXISTS] | runtime-wasm lib.rs:378 |
| API-009 | `WgpuRenderer::{init_from_canvas, render_frame, render_frame_with_dirty, resize, hit_test_input_field, elapsed_seconds, width, height, vertex_count, input_field_bounds}` (§5.3) | [EXISTS] | backend lib.rs:521/790/1010/1249–1290 |
| API-010 | Free functions `build_vertex_buffer`, `quads_from_text` (§5.3) | [EXISTS] | backend lib.rs:282/353 |
| API-011 | Text traits + concrete impls (§5.5) | [EXISTS] | text lib.rs:210/319/388 |
| API-012 | `render::compile(graphs, dirty, depth)`, `glyph_run_to_draw_calls`, `PipelineCache` (§3.5/§5.4; spec shorthand omits dirty/depth params — DOC-004) | [EXISTS] | render lib.rs:447/935/669 |
| API-013 | lib.rs re-exports — all 27 items of §4.5 table | [EXISTS] | compiler lib.rs:108–140 |
| API-014 | CLI flags `compile/-o/--lint/--scheduled`; JSON incl. `schedule` field (§4.1) | [EXISTS] | main.rs; pass_order asserted in tests |

## 5. Business Rules / Constraints (RULE)

| ID | Rule (spec source) | Status | Evidence |
|----|--------------------|--------|----------|
| RULE-001 | C1 `#![forbid(unsafe_code)]` in compiler, render, text, core | [EXISTS] | all 4 + 11 bonus crates |
| RULE-002 | C2 `#![allow(unsafe_code)]` runtime-wasm + backend-wgpu, cold path only | [EXISTS] | 2 blocks, both init/upload |
| RULE-003 | C3 ADR-018 external-dep policy; no new crates w/o amendment | [EXISTS] | deny.toml gate; wasm-encoder sanctioned via ADR-008 amendment; serde_json cli-gated; verified not in WASM graph |
| RULE-004 | C4/invariant-1 ADR-013 no DOM hot path | [EXISTS] | frame loop WASM-internal |
| RULE-005 | C5 ADR-002 dirty-rect invalidation | [EXISTS] | `compile(graphs, dirty, depth)` |
| RULE-006 | C6 ADR-001 ScheduleIR lowers into RenderGraph | [EXISTS] | graph.rs routing both backends |
| RULE-007 | C7 ADR-022 vendored HarfRust; no DOM text (`fillText`/`measureText` absent) | [EXISTS] | vendor/; no fillText |
| RULE-008 | C8/A9/invariant-4 `.alk` backward compat | [EXISTS] | compat tests |
| RULE-009 | C9 manual JSON serializer, no serde in library mode | [EXISTS] | ir.rs `to_json()` |
| RULE-010 | C10 single-threaded WASM, no workers/SharedArrayBuffer in current phase | [EXISTS] | no workers; ThreadAffinity (Wave 1) defaults MainThread |
| RULE-011 | Codegen validation: font-size > 0, rotation finite, `below text` requires text node (§3.1) | [EXISTS] | codegen.rs w/ line/col |
| RULE-012 | Text limits enforced at load/shape boundaries | [EXISTS] | text lib.rs:764/910 |
| RULE-013 | Resize clamp ≥1×1 on both backends | [EXISTS] | backend lib.rs:1250, wgpu_renderer.rs:945 |
| RULE-014 | Shrink ops on `monotone` → error; grow ops on `antitone` → error (P2) | [EXISTS] | typechecker.rs check_method_op |
| RULE-015 | `#![deny(monotonicity)]` upgrades Warning → Deny | [EXISTS] | lints/mod.rs:191 |
| RULE-016 | `effective_qualifier()` attribute precedence (P1 bridge) | [EXISTS] | typechecker.rs:1630 |
| RULE-017 | Small-scene bypass threshold 50 | [EXISTS] | lib.rs:98/424/1001 |
| RULE-018 | CI: 4 jobs, zero-warning gates, wasm-opt ≥40% shrink, SHA match | [EXISTS] | ci.yml; current run green |
| RULE-019 | Zero placeholders: no `todo!`/`unimplemented!`/stub returns | [EXISTS] | repo-wide scan clean (1 test idiom) |
| RULE-020 | serde_json excluded from WASM runtime dep graph | [EXISTS] | workspace default-features=false; cargo tree verified |

## 6. Documentation Drift Items (DOC)

| ID | Item | Disposition |
|----|------|-------------|
| DOC-001 | Spec §3.2 frame-loop paragraph + §8.1 TD3 row still describe legacy `time += 1.0/60.0` ("runtime currently ignores elapsed_seconds") — code fixed (performance.now) | Fix spec text (Wave 3) |
| DOC-002 | Spec §6.1 dependency-graph edge list stale: compiler has unconditional `wasm-encoder 0.227` (sanctioned by ADR-008 amendment, never reconciled); render→text; backend/runtime supersets; wasm-bindgen/web-sys not wasm32-gated in backend | Fix spec text (Wave 3) |
| DOC-003 | Spec §5.1 "Future API" table vs as-built: `compile()` returns AlgorithmIR (legacy preserved per §4.5/Wave-5 correction); `compile_with_lints(src) -> (AlgorithmIR, LintSet)`; production entry is `compile_full` | Refresh §5.1 to as-built (Wave 3) |
| DOC-004 | Signature shorthands: `extract(&EGraph, &DependencyGraph)` (extra `original` param is an as-built necessity); `render::compile` takes dirty+depth and returns Result; `render_graph` is a `WgpuRenderer` method per §9.2 | Record as-built amendment notes (Wave 3) |
| DOC-005 | §4.5 stale LOC figures (lexer 1136→1300, parser 1366→1925, ast 562→867, typechecker 843→2596, codegen 1061→1121, lib 268→276) | Refresh (Wave 3) |
| DOC-006 | Pass-count delta: as-built 5 passes (explicit `Clear`) vs §4.1's 4-pass "naive mapping" sketch; seminative.rs API spelling `seminive*` vs spec table typos `SeminineNew/Removed` | Record as-built note (Wave 3) |
| DOC-007 | `docs/adr/README.md` summary table lists all 28 ADRs "Proposed" (incl. amended/implemented); ADR-027 doc cites "1096 tests" (now 1104+); `alkalive-ipc/lib.rs:8` doc claims `todo!()` bodies that no longer exist | Fix (Wave 5) |
| DOC-008 | C3 attributes the "5-crate policy" to ADR-018 while ADR.md's ADR-018 title is "Capability-Scoped Imports + Component-Model Tree-Shaking"; the 5-crate reading is the policy interpretation codified in deny.toml and cross-referenced by ADR-026/028 | Leave as-is (consistent with ADR-026/028/deny.toml usage); note in this backlog |

---

## 7. Gap Analysis Summary & Quantification

**Total backlog items: 93** (44 REQ + 15 ENTITY + 14 API + 20 RULE) **+ 8 DOC items.**

| Status | Count | Items |
|--------|-------|-------|
| [MISSING] | **1** | REQ-011 (DependencyGraph serialization) |
| [PARTIAL] | **6** | REQ-008 (reevaluate API), REQ-010 (TD1 persistent glyph atlas), REQ-012 (path-halving wording), REQ-013 (RewriteRule trait), REQ-018 (lint op-scan), ENTITY-002 (ThreadAffinity), ENTITY-004 (ComputationId), ENTITY-006 (RewriteRule — same fix as REQ-013) |
| [EXISTS] | 86 | all remaining |
| DOC drift | 8 | DOC-001…DOC-008 |

**Quantified:** 7 of 93 code-level items (7.5%) are missing or partial; 86 of 93 (92.5%) exist
with file:line evidence. 8 documentation-drift items require spec/doc reconciliation. No
unimplementable or contradictory requirement was found; no spec section was skipped.

## 8. Wave Disposition Plan

| Wave | Scope |
|------|-------|
| Wave 1 (data layer) | ENTITY-002 `ThreadAffinity`; ENTITY-004 `ComputationId`; REQ-011 `DependencyGraph::to_json()` |
| Wave 2 (business logic) | REQ-010 TD1 persistent glyph atlas (both backends) w/ overflow reset; REQ-008 `SignalStore::reevaluate()` composition wired into frame loop; REQ-013/ENTITY-006 `RewriteRule` trait refactor; REQ-012 union-find path-halving; REQ-018 monotonicity lint op-scan |
| Wave 3 (API/interface) | CLI `--scheduled` emits dep-graph diagnostics (consumes REQ-011); DOC-001…DOC-006 spec reconciliation to as-built |
| Wave 4 (edge cases) | Persistent-atlas overflow reset test; full edge-case test audit (RULE-011…RULE-017); zero-panic verification |
| Wave 5 (observability) | DOC-007 doc truthfulness fixes; logging/console + `__alkalive` diagnostics audit; credential scan |
| Wave 6 (testing) | REQ→test mapping refresh; new unit tests for all Wave 1–2 code; full 1104+ suite green |
| Wave 7 (final) | fmt/clippy(-D warnings)/audit/deny; release build; wasm32 zero-warning; build-deploy pipeline (≥40% + SHA); serve.mjs boot smoke; push + CI green |

---
*End of master backlog. Generated by the Implementation Orchestrator, Wave 0.*

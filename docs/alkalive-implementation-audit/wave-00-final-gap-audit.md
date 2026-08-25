# Wave 00 — Final Gap Audit (Fresh Forensic Re-Audit)

> **Date:** 2026-08-25
> **Status:** This document supersedes `final-verification.md` (the ~93% claim) as the current baseline.
> **Method:** Source-code inspection, execution-path tracing, full test-suite execution
> (`cargo test --workspace --lib` = 1,143 passed), wasm32 release build, WGSL/uniform-layout
> manual validation, dependency-graph analysis, and caniuse-sourced WebGPU availability research.
> No previous claim was accepted without re-verification.

---

## 1. Executive Summary

The previous assessment of **~93%** is **incorrect**. After this fresh forensic audit, the actual
implementation level is **64/100 = 64%**.

The 93% figure over-credited three subsystems:

1. It counted the wgpu/WGSL renderer as "completed with bind groups" when in fact the renderer
   **cannot execute a single valid frame**: its rect pipeline fails wgpu validation at creation,
   its text bind group does not match the pipeline's shader-derived layout, its Rust↔WGSL uniform
   layouts disagree, and it never uploads glyph atlas or vertex data. "Exists and compiles" was
   scored as "implemented".
2. It framed the render worker as a 3% integration gap ("call `spawn_render_worker()`"), when the
   correct finding is that the existing worker code is a **fake** (an inline-JS worker whose
   message handlers are empty stubs) and that activating it would **contradict the authoritative
   architecture** (SPECIFICATION.md §1.5 INV-3 pins GPUDevice ownership to the main thread;
   ADR-021 assigns workers to on-demand async tasks only).
3. It scored the e-graph optimization (ADR-026) as integrated although the runtime calls
   `compile_with_deps()` and **never** `compile_full()` — the optimization never executes.

It also missed several genuine gaps discovered here: ineffective COOP/COEP meta tags, an entire
dead legacy crate (~190 KB), a stale verification script pointing at artifacts that no longer
exist, unused web-sys features, and no deterministic deploy build pipeline.

---

## 2. Verified Current Architecture

Production execution path (verified by tracing + test execution):

```
examples/hello.alk (embedded via include_str!)
  → alkalive_compiler::compile_with_deps()      [lexer → parser → typechecker → lower →
                                                 schedule_lowering → incremental_analysis]
  → ScheduledScene + DependencyGraph
  → build_scene_from_scheduled() → TextSceneData
  → WgpuRenderer::init_from_canvas()            [raw WebGL2 via web-sys, GLSL ES 3.00]
  → build_render_graph()                        [5-pass graph IR]
  → WgpuRenderer::render_graph()                [WebGL2 draw calls]
  → visible golden-on-black UI                  [verified genuine]
```

What is **genuinely working** (re-verified this audit):

| Subsystem | Evidence |
|-----------|----------|
| Language frontend (lexer/parser/AST, fns/lets/classes/imports/control flow/operators) | 387 compiler tests pass; grammar exercised |
| Type system (FnSigTable, 3-pass check_module, monotonicity lattice, cyclic-inheritance guard) | tests pass |
| WASM codegen (`wasm_codegen.rs`, wasm-encoder + wasmparser validation) | 205 wasm-codegen tests pass |
| OO model (classes, methods, inheritance, vtable dispatch, `call_indirect`) | tests pass |
| Collection host-import dispatch, string data sections | tests pass |
| Monotonicity lints (ADR-027 P1) + Phase 2 qualifiers + seminaïve metadata | lint_tests.rs + phase2_tests.rs pass |
| ADR-024 schedule lowering | executed in `start()` via `compile_with_deps` |
| ADR-025 SignalStore + dirty propagation (+ approved small-scene bypass R1) | wired in frame loop; signal_store tests pass |
| Render-graph IR (`build_render_graph`) driving WebGL2 rendering | verified in backend tests + demo path |
| HarfRust text shaping/rasterization (ADR-022) with cached registry (M7) | production GLSL path |
| Runtime bootstrap: RAF loop from WASM, IME bridge (ADR-023), High-DPI resize, click hit-test, panic hook | code-traced + demo genuine |
| Demo authenticity (GLSL path) | real compiler → real IR → real GPU draws; no mocks |

---

## 3. Requirement-Level Scoring (weights sum to exactly 100)

Classification key: ✅ fully implemented & verified · 🟡 partial · ❌ missing/incorrect ·
⛔ intentionally excluded by approved decision (earns 0 weight, listed for completeness).

| # | Requirement (source) | Wt | Status | Earned | Why |
|---|----------------------|---:|--------|-------:|-----|
| 1 | Full language grammar: lex/parse (fn/let/class/import/if/while/operators) (ADR-008) | 5 | ✅ | 5 | 387 tests; grammar exercised end-to-end in codegen tests |
| 2 | Type system: inference + monotonicity + cycle detection (ADR-009/027) | 5 | ✅ | 5 | typechecker tests incl. cyclic inheritance |
| 3 | .alk → validated WASM binary (ADR-008/017) | 5 | ✅ | 5 | wasm-encoder emission, wasmparser validation, 205 tests |
| 4 | OO model: classes/methods/inheritance/vtable (ADR-007/008) | 4 | ✅ | 4 | codegen tests |
| 5 | Collection method dispatch via host imports | 3 | ✅ | 3 | ImportSection tests |
| 6 | String data sections | 3 | ✅ | 3 | DataSection dedup tests |
| 7 | Module import syntax | 2 | ✅ | 2 | parse_import tests |
| 8 | File-based module resolution functional/documented in embedded pipeline | 3 | 🟡 | 2 | resolver called (typechecker Pass 1.1) but inert for embedded sources; stub fallback is documented interim semantics |
| 9 | Monotonicity lints P1 (ADR-027) | 3 | ✅ | 3 | lint_tests pass |
| 10 | Seminaïve strategy metadata consumed by runtime | 2 | ✅ | 2 | `has_seminive_collections` called in scene build |
| 11 | ADR-024 schedule drives data-driven dispatch | 4 | ✅ | 4 | schedule passed into render_frame both paths |
| 12 | ADR-025 signals+dep-graph executed (small-scene bypass = approved R1 mitigation) | 4 | ✅ | 4 | frame loop propagates dirt; bypass documented+tested |
| 13 | ADR-026 e-graph optimization **executed** in production init | 3 | ❌ | 0 | runtime calls `compile_with_deps`, never `compile_full`; optimization exists+tested but never runs |
| 14 | Render-graph IR built + executed every frame (ADR-001) | 5 | ✅ | 5 | on GLSL backend, verified |
| 15 | HarfRust shaping/raster feeding the renderer (ADR-022) | 4 | ✅ | 4 | cached font registry; atlas upload path |
| 16 | WGSL shaders used by an executing renderer (ADR-006) | 5 | ❌ | 0 | shaders exist but are invalid-in-context (see §4A); never compiled by any executing pipeline |
| 17 | WebGPU as primary production renderer (ADR-001 "WebGPU is the initial backend"; ADR-017 precompile) | 6 | ❌ | 0 | production path is raw WebGL2/GLSL; zero WebGPU execution anywhere |
| 18 | Explicit renderer-selection architecture (primary/fallback, logged, both paths tested) | 5 | ❌ | 0 | no selection logic exists at all |
| 19 | Runtime bootstrap bundle: WASM-owned loop/IME/DPI/hit-test/error-propagation | 7 | ✅ | 7 | verified genuine |
| 20 | Single-GPU-owner discipline documented, no contradicting fake paths (SPEC INV-3) | 2 | 🟡 | 1 | holds de facto (main-thread-only) but fake worker module contradicts chosen model |
| 21 | Real COOP/COEP isolation + SAB availability verified (ADR-003 consequences) | 3 | ❌ | 0 | `<meta http-equiv>` COOP/COEP tags do NOT enable crossOriginIsolated — HTTP response headers required; none exist |
| 22 | On-demand-worker posture documented per ADR-021 trigger analysis | 2 | ❌ | 0 | no workers exist AND no documented evidence-based decision; instead a fake worker pretends otherwise |
| 23 | wasm-opt post-processing with measured result (ADR-017 compactness) | 3 | ❌ | 0 | no wasm-opt anywhere in build config |
| 24 | Deterministic pinned deploy pipeline (bindgen pinned, script, size report) | 3 | ❌ | 0 | committed glue has no reproducible generation path |
| 25 | Minimal performance benchmark harness | 1 | ❌ | 0 | none exists |
| 26 | Dead-code elimination (legacy crate, stale scripts/artifacts, unused features) | 3 | ❌ | 0 | see §5 |
| 27 | Demo end-to-end authenticity in a real browser | 5 | ✅ | 5 | verified on GLSL path; will be re-verified after changes |

**Totals: earned 64 / weight 100 → implementation level = 64%.**

### Intentionally excluded by approved architectural decision (no weight)

- Accessibility bridge (ADR-019 — deferred)
- PMT verification (ADR-028 — deferred)
- DOM metadata/SEO layer content beyond the minimal shell (ADR-020 scope not yet triggered)
- HMR, design-tool-as-runtime, author traces (ADR-014/015/016 — future milestones per tech spec §2)
- Executing user-compiled `.alk` WASM inside the runtime cdylib — the embedded-source
  architecture (`.alk` as data lowered to SceneIR inside the prebuilt runtime cdylib) is the
  documented interim design (tech-spec §3.1 Wave-4 note); the language→WASM pipeline ships as
  library capability with full test coverage.
- Native Vulkan/Metal backends (ADR-001 lists them as future options)

### The "exists → integrated → executed → correct → production-path → verified" ladder

The prior audits repeatedly awarded credit at rungs 1–2 ("exists", "integrated as API").
This audit scores only at rungs 4–6:

| Item | Exists | Integrated | Executed | Correct | Production | Verified |
|------|:--:|:--:|:--:|:--:|:--:|:--:|
| WGSL shaders | ✔ | ✘ | ✘ | ✘ (invalid in context) | ✘ | ✘ |
| WgpuBackendRenderer | ✔ | ✘ | ✘ | ✘ | ✘ | ✘ |
| Render worker | ✔ | ✘ | ✘ | ✘ (fake + wrong model) | ✘ | ✘ |
| e-graph optimization | ✔ | ✔ (API) | ✘ | ✔ | ✘ | ✘ |
| GLSL/WebGL2 renderer | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ |
| Module resolver | ✔ | ✔ | ◑ (inert) | ◑ | ◑ | ◑ |

---

## 4. Deep-Dive Findings on the Reported 7%

### A. wgpu/WebGPU production path — reported "not wired in (~3%)". Actual: broken AND unwired.

**Which renderer is authoritative?** ADR-001: *"WebGPU is the initial backend"* (with native
Vulkan/Metal as future options). ADR-006 makes WGSL the first-class styling primitive. ADR-017
requires WebGPU pipeline precompilation. Nothing in the authoritative set names WebGL2 as the
production target; the raw-WebGL2 choice is self-documented in
`crates/alkalive-backend-wgpu/src/lib.rs:12–20` as justified by *"the task brief"* — which is not
part of the authoritative set.

**Browser/device policy (evidence-based).** caniuse (fetched 2026-08-25): WebGPU ≈ 84% global
support (`y`) + ~3% partial — Chrome/Edge ≥113 everywhere except Linux-by-default, Safari/iOS 26+,
Firefox ≥141 Windows-only by default. Conclusion: WebGPU must be the **primary** backend;
WebGL2 remains a **required fallback** (~13% + Firefox-non-Windows + older devices).
Both must stay; selection must be explicit, intentional, and logged.

**Is wgpu genuinely integrated? No — it cannot even initialize correctly.**
`WgpuBackendRenderer` (`wgpu_renderer.rs`, feature `wgpu-backend`, default-enabled) has these
defects, each independently fatal at runtime:

1. **Rect pipeline is structurally invalid.** `RECT_VERTEX_WGSL` declares
   `@location(0) position: vec2<f32>` input, but the pipeline is created with
   `buffers: &[]` (line 230). wgpu validation rejects the pipeline at creation
   (`init_from_canvas` fails ⇒ renderer unusable).
2. **Text bind group ≠ pipeline layout.** `TEXT_FRAGMENT_WGSL` requires bindings
   `{1 texture, 2 sampler, 3 uniform}` while the hand-built `text_bind_group_layout`
   defines `{0 uniform, 1 texture, 2 sampler}`. With `layout: None` the pipeline layout is
   derived from the shaders (includes binding 3); binding the mismatching group at
   `set_bind_group` is a validation error ⇒ no draw ever executes.
3. **Rust/WGSL uniform layout mismatch.** WGSL `TextUniforms {rotation, canvas_size: vec2f, time}`
   places `canvas_size` at offset 8 (vec2 alignment); Rust `TextUniformsData` packs it at
   offset 4. Even with matching bindings, the GPU would read misinterpreted values.
   Additionally `text_color` lives in a separate WGSL binding (3) but inside the Rust struct.
4. **No geometry ever reaches the GPU.** `update_vertices()` and `update_glyph_texture()`
   exist but have **zero callers**. `vertex_count` stays 0; glyph atlas stays blank.
   There is no tessellation/shaping path at all in the wgpu renderer — it would render nothing
   even if pipelines initialized.
5. **Per-frame uniforms hardcode scene constants.** rotation baked as `0.5 * time`
   (ignoring `scene.rotation_speed`), color hardcoded gold (ignoring `scene.text_color`),
   ignoring the per-draw-call `DrawText {font_size, color, rotation}` payloads carried by the
   render graph itself.
6. **Rect draw calls draw without any bind group** (rect fragment expects uniforms at
   bindings 0–2 that are never bound) and without per-rect geometry (a single fixed
   `draw(0..4)` with no vertex buffer providing inputs ⇒ also a validation error).
7. **Hit-test bounds never computed** (`input_field_bounds` remains `(0,0,0,0)`), so the
   input-field rect passes would be degenerate and click focus can never engage.

**Verdict:** requirement 17/18 fail outright; requirement 16 fails. This is not a wiring task
alone — the wgpu backend must be made actually correct before wiring it in.

### B. Render worker — reported "not spawning (~3%)". Actual: fake code + wrong model; activation would violate the spec.

**Why does a worker exist conceptually?** ADR-003 composes multiple scene graphs through one
GPUDevice owner; ADR-021 adds main-thread-render-loop + on-demand async threads over SAB IPC.

**What does the authority actually pin?**
- SPECIFICATION.md §1.5 INV-3: *"GPUDevice acquisition occurs on exactly one agent
  (**the main thread**); on-demand workers may not call requestDevice or queue.submit."*
- ADR-021 Decision: *"The main thread runs the retain-mode render loop … **and owns the
  GPUDevice** per ADR 003. Additional WASM threads are spawned **on demand** for asynchronous
  tasks (asset decoding, compute, IO)."*
- ADR-003 permits "(either the main thread or a dedicated non-on-demand worker)" as the owner —
  the specification resolves this freedom to the **main thread**.

Therefore the existing `render_worker.rs` model — transfer canvas to an OffscreenCanvas worker
that owns rendering — is the ADR-003 *alternative*, not the specified configuration. Activating
it as-is would contradict INV-3's letter. Worse, the implementation is **fake**: the inline JS
worker's handlers contain comments ("The worker would initialize the wgpu renderer here") and
echo messages; no WASM module loads in the worker; nothing renders. `spawn_render_worker()` and
`transfer_canvas_to_offscreen()` have zero callers.

**Does worker rendering help low-end devices? No.** For a single-graph Hello-World-class scene:
worker spawn cost + OffscreenCanvas transfer + per-frame message/SAB copy latency strictly add
overhead; the compositor benefit of ADR-003 materializes only with multiple concurrent graphs,
which do not exist in this milestone. Moving work off-main helps only when heavy async work
(asset decode/compute/IO) exists — per ADR-021 exactly those triggers — and none exist yet.

**Required remediation (replaces the reported "wire spawn_render_worker()" gap):**
1. Remove the fake worker module and the now-unused web-sys features (`Worker`,
   `OffscreenCanvas`, `Blob`, `Url`, `MessageEvent`).
2. Make the single-owner discipline explicit: document in code + ADR-consistent
   implementation-status notes why the main thread owns the device (INV-3/ADR-021) and under
   which measured conditions a dedicated-owner or on-demand worker would be introduced.
3. Deliver the genuinely required piece of ADR-003's consequence chain: **real cross-origin
   isolation** — serve `Cross-Origin-Opener-Policy: same-origin` /
   `Cross-Origin-Embedder-Policy: require-corp` as HTTP response headers from the deploy server
   (meta tags are ignored for this purpose), detect `crossOriginIsolated` + constructible
   `SharedArrayBuffer` at startup, log the capability, and verify in a real browser.

### C. wasm-opt — reported "missing (~1%)". Actual: required by ADR-017's compactness goal; implement deterministically and measure.

ADR-017 commits to a *"compact WASM binary whose startup is bounded by decode"* and explicitly
calls out binary-size budgets (thread runtime + IPC shim + shaping payload sharpen streaming
concerns). The workspace already uses `opt-level="z"` + LTO + `strip`, but no post-link
optimization exists. `wasm-opt -Oz` typically yields a further measurable reduction and is the
industry-standard tool for exactly this goal. **Verdict: required** (as the mechanism for
ADR-017's compactness budget), with these engineering constraints:

- Runs deterministically from a pinned tool version (npm-pinned Binaryen distribution).
- Fails loudly with actionable errors when tooling is unavailable in production packaging.
- Skipped for debug builds; applied for deploy/release artifacts.
- Output re-validated (module instantiation smoke test) so optimization cannot corrupt the binary.
- **Measured**: build report records pre/post sizes; claims must cite numbers.

---

## 5. Hidden Gaps Discovered (beyond the reported 7%)

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| H1 | `alkalive-app` crate (~190 KB: software renderer, particles, starfield, input field, text scene) is referenced by **no other crate**; pure legacy dead code | Major | grep across all Cargo.tomls: only self-reference |
| H2 | `verify_wasm.mjs` targets `deploy/alkalive_app_bg.wasm` + `alkalive_app.js` — artifacts that **do not exist**; script is unrunnable | Minor | file references vs `deploy/` contents |
| H3 | `deploy/hello.scene` stale CLI artifact committed | Minor | present in tree; unreferenced |
| H4 | Unused web-sys features `Gpu`, `GpuCanvasContext`, `GpuDevice`, `GpuQueue` in backend-wgpu (leftovers of abandoned direct-WebGPU attempt) | Minor | zero usage matches |
| H5 | Fake worker module + its five web-sys features (`Worker`, `OffscreenCanvas`, `Blob`, `Url`, `MessageEvent`) — fake implementation presented as architecture | Major | render_worker.rs inline-JS stub comments; zero callers |
| H6 | COOP/COEP implemented only as `<meta http-equiv>` — browsers ignore these for cross-origin isolation ⇒ SAB path unreachable in every deployment | Major | HTML spec/MDN: response headers required |
| H7 | Runtime skips ADR-026: uses `compile_with_deps` not `compile_full` | Minor (no-op for Hello World, but violates executed-vs-exists discipline) | lib.rs:237 vs codegen.rs:470 |
| H8 | Two dead-code warnings in runtime-wasm build (`original_text` etc.) — symptom of unfinished plumbing | Trivial | cargo build output |
| H9 | No CI configuration at all (`.github/` absent) — nothing enforces the passing state | Minor | repo root |
| H10 | Prior wave reports contain false completion claims (wave-01 "wgpu renderer completed with bind groups"; wave-02 "WGSL shaders activated") — superseded docs remain uncorrected | Process | wave-01/wave-02 files vs §4A findings |

---

## 6. Remediation Plan (dependency-aware)

```
Wave 1  Renderer Architecture  ─────────────┐
  1a Fix WGSL + pipelines + uniforms        │ (backend correctness first —
  1b Shared scene tessellation for wgpu     │  everything else depends on it)
  1c Renderer selection wired into runtime  │
  1d compile_full (egraph) executed         │
  1e naga WGSL-validation + layout-parity   │
     tests + browser E2E harness            │
                                            │
Wave 2  Worker/Isolation Truth ─────────────┤ (depends on Wave 1 runtime shape:
  2a Remove fake worker + prune features    │  selection logging lands there)
  2b Deploy server with COOP/COEP headers   │
  2c SAB/crossOriginIsolated verification   │
  2d ADR-consistent documentation notes     │
                                            │
Wave 3  Build/Optimization ─────────────────┤ (depends on Waves 1–2 artifact shape:
  3a Pinned wasm-bindgen regeneration       │  optimizes the FINAL binary)
  3b wasm-opt -Oz step (pinned Binaryen)    │
  3c Measured build report + smoke validate │
                                            │
Wave 4  Dead-Code/Hygiene ──────────────────┤ (after Waves 1–3 stabilize APIs)
  4a Remove alkalive-app, verify_wasm.mjs,  │
     hello.scene, unused deps               │
  4b Zero warnings; README/doc sync         │
                                            │
Wave 5  Final Independent Verification ─────┘ (full re-audit + push)
```

Every wave follows Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## 7. DoD for this audit

- [x] Fresh repository assessment completed against ADRs/spec/tech-spec
- [x] Requirement-level scoring with explicit weights summing to 100 (result: 64%)
- [x] Reported-gap deep dives (renderer / worker / wasm-opt) resolved to evidence-backed conclusions
- [x] Hidden-gap sweep completed (H1–H10)
- [x] Demo authenticity re-established (GLSL path genuine)
- [x] Dependency-aware remediation plan defined (Waves 1–5)

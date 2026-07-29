# AlkALive — Implementation Plan

**Document:** `IMPLEMENTATION_PLAN.md`
**Version:** 1.0
**Date:** 2026-07-26
**Task ID:** IMPL-W1
**Source artifacts:**
- `docs/SPECIFICATION.md` v1.0 (14 sections, implementation-ready)
- `docs/adr/ADR.md` v1.0 (ADR 001–022, all Status: Proposed)
- `docs/adr/Decision_Alternatives_*.md` (4 files, all RESOLVED)
- `docs/adr/Spec_Tradeoff_Note_IME.md` (open IME dependency)

**Toolchain manifest:**
| Component | Version | Notes |
|---|---|---|
| Rust | stable 1.97.1 | `rustup default 1.97.1` |
| wasm target | `wasm32-unknown-unknown` | `rustup target add wasm32-unknown-unknown` |
| cargo | 1.97.1 | workspace + edition 2021 |
| HarFrost | forked, vendored `vendor/harfrust/` | ADR 022 |
| WGSL compiler | host WebGPU impl | runtime compile + ADR 017 precompile |
| COOP/COEP | host headers | `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp` (`credentialless` mitigation) |

**Discipline:** Each wave is executed by a wave-owner sub-agent; each task is a single commit-sized unit (≤ one crate or one trait). No task crosses a module boundary.

---

## 1. Wave Decomposition

### Wave 1 — Plan (this document)
**Goal:** Lock the implementation order, ADR traceability, and DoD bar before code is written.
**Tasks:**
1. W1-T1 — Author `IMPLEMENTATION_PLAN.md` (this document).
2. W1-T2 — Cross-check every named trait/struct against the spec section it derives from (§2.7, §3.6, §4.1–4.5, §5.2–5.7, §6.2–6.9, §7.1–7.7, §8.1–8.6, §9.7, §10.5, §11.4–11.5, §12.6, §13.5, §14.3).
3. W1-T3 — Map each wave to its owning ADRs; ratify the open-dependency register (§12.8).
**DoD:** (a) Document exists; (b) every trait named in the spec appears in exactly one wave's task list; (c) ADR matrix covers ADR 001–022 with no orphan.
**Dependencies:** — .

### Wave 2 — Environment + Cargo workspace scaffolding
**Goal:** Stand up the monorepo workspace, vendored HarfRust, CI gates, and the `wasm32-unknown-unknown` build.
**Tasks:**
1. W2-T1 — Create root `Cargo.toml` (virtual workspace, edition 2021, profile `wasm-release` with `opt-level="z"`, `lto=true`).
2. W2-T2 — Add `rust-toolchain.toml` pinning `1.97.1` + `wasm32-unknown-unknown`.
3. W2-T3 — Scaffold empty crates: `alkalive-core`, `alkalive-runtime`, `alkalive-render`, `alkalive-layout`, `alkalive-text`, `alkalive-style`, `alkalive-input`, `alkalive-dom`, `alkalive-a11y`, `alkalive-ipc`, `alkalive-perf`, `alkalive-error`, `alkalive-test`.
4. W2-T4 — Vendor HarfRust under `vendor/harfrust/` and wire as a path-dep crate `harfrust` (ADR 022).
5. W2-T5 — Add stdlib facade crates `std-core`, `std-gpu`, `std-layout`, `std-text`, `std-input`, `std-ipc` (§2.8) as thin re-export crates.
6. W2-T6 — CI: `cargo build --target wasm32-unknown-unknown --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`.
7. W2-T7 — Add `deny.toml` (cargo-deny) enforcing capability-scoped imports (ADR 018): no registry deps outside an allowlist.
**DoD:** `cargo build --workspace` and the wasm-target build both pass green on CI; `cargo tree` shows zero external registry crates outside the allowlist; HarfRust builds under the wasm target.
**Dependencies:** W1.

### Wave 3 — Core trait definitions (signatures only)
**Goal:** Lock every cross-crate trait signature against the spec so later waves implement against a frozen contract.
**Tasks:**
1. W3-T1 — `alkalive-core`: `Module`, `Interface`, `Type`, `EncapsulationBoundary`, `Slot`, `Signal<T>` (§2.7); `ModuleError`, `SlotError`, `SignalError`, `Failure` (§2.9). No bodies.
2. W3-T2 — `alkalive-render`: `Backend` trait (§4.1), `RenderGraph`/`RenderPass`/`Attachment`/`DrawCall` IR structs (§4.2), `RenderLoop` + `Compositor` traits (§4.4–4.5), `PipelineCache` handle type, `RenderError` enum (§4.7).
3. W3-T3 — `alkalive-layout`: `Vec2/Size/Rect/Mat4/Constraint/LayoutVar` (§5.2), `ConstraintKind`, `SolveStatus`, `LayoutSolver` trait, `LayoutNode`, `MeasuredRun` interface, `LayoutSolution`, `SolveError` (§5.3–5.7).
4. W3-T4 — `alkalive-text`: `FontRegistry`, `TextShaper`, `ShapedRun`, `GlyphAtlas`, `GlyphKey`, `AtlasSlot`, `TextStack` (§6.2–6.9), `ShapeError`, `FontLoadError`.
5. W3-T5 — `alkalive-style`: `Style` trait, `PropertyKind`, `StyleProperty`, `OwnedStyle`, `ShaderStyle`, `Animation` (§7.1–7.5).
6. W3-T6 — `alkalive-input`: `InputBatch`, `InputEvent`, `PointerSample`, `KeyEvent`, `GamepadSample`, `HitTester`, `GrabHandle`, `GestureState`, `FocusManager`, `InputError` (§8.1–8.6).
7. W3-T7 — `alkalive-dom`: `DomBridge`, `SeoSnapshot`, `NavigationContract`, `DomError` (§9.7).
8. W3-T8 — `alkalive-a11y`: placeholder `SemanticRole`, `A11yNode`, `A11yExtensionPoint`, `A11yPlaceholder` (§10.5) — committed signatures, `unimplemented!()` bodies.
9. W3-T9 — `alkalive-ipc`: `IPCSocket<T>`, `TaskKind`, `TaskError`, `ChannelError`, `WorkerPool`, `TaskHandle`, `Scheduler`, `SharedState` (§11.4–11.5).
10. W3-T10 — `alkalive-perf`: `FrameBudget`, `ResourceBudget`, `PerfCounter`, `MemoryPool`, `TraceSpan`, `BreachPolicy`, `BudgetBreach` (§12.6).
11. W3-T11 — `alkalive-error`: `AlkALiveError` enum (§13.1), `ErrorBoundary`, `TraceRecorder`, `ModuleIsolator`, `RecoveryStrategy` (§13.5).
12. W3-T12 — `alkalive-test`: `TestResult`, `SceneSnapshot`, `MockBackend`, `MockTextStack`, `ComponentTest`, `TracePlayer`, `TestHarness`, `SoftwareBackend` (§14.3).
**DoD:** `cargo build --workspace` passes; every trait signature matches the spec text byte-for-byte (reviewer-checked against the cited §N); no crate contains a function body other than `todo!()`/`unimplemented!()`.
**Dependencies:** W2.

### Wave 4 — Render-object tree + module model
**Goal:** Implement ADR 007/008/009: the single owned render-object tree, encapsulation boundaries, two-level type verification seams, and module lifecycle.
**Tasks:**
1. W4-T1 — `RenderObject` struct with `role: SemanticRole`, `structured_data`, `interaction` (mandatory, ADR 011), owned-style table ref, layout-node ref.
2. W4-T2 — Owned-subtree container (`OwnedSubtreeRef`) with single-owner invariant (compile-time + runtime assert).
3. W4-T3 — `Module` lifecycle state machine (`Construct→Attach→Visible→Destroy`); `IllegalTransition` rejection (§2.9).
4. W4-T4 — `EncapsulationBoundary` enforcement: `CapabilityDenied` on cross-owner access without grant (ADR 018).
5. W4-T5 — `Slot::mount` with cardinality check (`Optional|Single|Many`); `CardinalityExceeded` error.
6. W4-T6 — `Signal<T>` with capability-gated `subscribe`; `EmitAfterDestroyed`/`ListenerCapabilityDenied`.
7. W4-T7 — `Type::wasm_shape` validation seam (ADR 009 level 2) — stub that accepts a `WasmTypeSig` and returns `CompileFailure` on mismatch.
8. W4-T8 — Unit tests for every transition, cardinality, and capability denial.
**DoD:** `cargo test -p alkalive-core` green; ownership invariant tests pass under `loom`-style single-owner check; spec §2.7 field list matches `RenderObject` exactly.
**Dependencies:** W3.

### Wave 5 — Render-graph IR + compositor
**Goal:** Realise ADR 001/003/017: immutable render-graph IR, the merge/reorder/batch/cull compiler, the main-thread compositor, and the pipeline cache.
**Tasks:**
1. W5-T1 — Implement `RenderGraph`/`RenderPass`/`Attachment`/`DrawCall` value types with `#[derive(Clone)]` immutability.
2. W5-T2 — `compile(graphs, dirty, depth)` — merge, barrier-edge topological reorder, draw-call batching by `(pipeline, bind_group_topology)`, attachment-lifetime barrier insertion.
3. W5-T3 — Occlusion-cull pass against compositor-wide `DepthBuffer`/`VisibilityBuffer`; drop occluded draw calls pre-encode.
4. W5-T4 — `Compositor` impl: `enqueue` (SAB/socket feed stubbed), `commit`, `depth_buffer`.
5. W5-T5 — `WebGPUBackend` impl of `Backend` over `wgpu` (or host binding); `encode` + `submit`.
6. W5-T6 — `PipelineCache` keyed by `(shader_hash, layout_hash, rt_format)`; LRU bounded at 64 MB (§12.7); miss → degraded builtin + `PipelineError`.
7. W5-T7 — Retain-last-known-good-frame recovery on `RenderError` (§13.4).
8. W5-T8 — Tests: barrier-cycle detection, attachment-lifetime violation, batching identity, cull correctness against a fixture depth buffer.
**DoD:** `cargo test -p alkalive-render` green; a 3-graph merge produces a `CompiledGraph` with passes in topological order; `PipelineCache` evicts at the 64 MB cap.
**Dependencies:** W3, W4 (RenderObject refs).

### Wave 6 — Layout solver + HarfRust integration
**Goal:** Realise ADR 004/002/022: the pluggable `LayoutSolver`, locality gate, text-flow measurement contract, and the in-WASM HarfRust stack.
**Tasks:**
1. W6-T1 — Geometry primitives (`Vec2/Size/Rect/Mat4/Constraint/LayoutVar`) shared with render + input.
2. W6-T2 — Default `CassowarySolver` impl of `LayoutSolver` (linear constraint solver; vendored or minimal-rs).
3. W6-T3 — `assert_local` locality gate: reject cross-module flex/percentage chains; emit `LocalityViolated`.
4. W6-T4 — `solve(dirty, measured, dt)` returning `LayoutSolution` with `transforms`/`clips`/`glyph_runs` written to GPU instance-buffer shape.
5. W6-T5 — `FontRegistry` impl: family/weight/style resolution, fallback chain, `load_bundle` from WASM-heap bytes (ADR 022).
6. W6-T6 — `TextShaper` impl over vendored HarfRust: BiDi segmentation, `ShapedRun` emission, `.notdef` for uncovered codepoints.
7. W6-T7 — `GlyphAtlas` LRU (≤32 MB, §12.7): `ensure` rasterize-on-demand, `invalidate(module_id, rect)` (ADR 002), `evict_lru(keep)`.
8. W6-T8 — `TextStack::measure` adapting `ShapedRun` → `MeasuredRun` `GlyphMetrics`/`LineBreak` (§5.4 shared boundary).
9. W6-T9 — Rasterization to render-graph IR: `GlyphQuadBatch` emission referencing atlas UVs.
10. W6-T10 — Tests: locality rejection cases, BiDi reordering fixtures, fallback-chain descent, atlas eviction under cap.
**DoD:** `cargo test -p alkalive-layout alkalive-text` green; locality gate rejects a deliberately cross-module percentage chain; HarfRust shapes an RTL fixture correctly; atlas evicts at 32 MB.
**Dependencies:** W3, W4, W5.

### Wave 7 — Input / hit-test / focus
**Goal:** Realise ADR 010/011: CPU bounding-volume hit-test, first-class device events, grab-based gesture routing, and the unified virtual focus layer.
**Tasks:**
1. W7-T1 — `InputBatch` builder at the WASM scheduler boundary; pre-partition by `DeviceKind`.
2. W7-T2 — `HitTester` impl: CPU bounding-volume mirror refreshed after each layout commit; `hit_test`, `invalidate`.
3. W7-T3 — `precise_pick` GPU pick-buffer readback path (off the per-frame path).
4. W7-T4 — Direct dispatch + `GrabHandle` capture; `OrphanedGrab` synthetic Cancel on object removal.
5. W7-T5 — Per-object `GestureState` machine; most-recent-explicit-grab-wins arbitration; loser receives `Cancel`.
6. W7-T6 — `FocusManager` as cached annotation layer: `set_focus` (sole writer), `current_focus` (sole active reader — future a11y hook), `tab_next`/`tab_prev`, `emit_focus_events`, `invalidate`.
7. W7-T7 — `InputError` normalisation at the scheduler boundary.
8. W7-T8 — Tests: hit-test ordering, grab release on teardown, focus-ring event emission, orphaned-grab cancel.
**DoD:** `cargo test -p alkalive-input` green; a grab released by mid-gesture teardown yields exactly one synthetic `Cancel`; focus events fire only on transition.
**Dependencies:** W3, W4, W6 (layout commits feed the hit mirror).

### Wave 8 — Concurrency / IPC
**Goal:** Realise ADR 021/003: main-thread GPU ownership, on-demand WASM workers, SAB-backed socket IPC, COOP/COEP gating.
**Tasks:**
1. W8-T1 — `IPCSocket<T>` SAB ring buffer with `Atomics` signaling; `send`/`try_send`/`recv`/`try_recv` backpressure.
2. W8-T2 — `ChannelError` framing/validation; quarantine on `Framing`.
3. W8-T3 — `WorkerPool::spawn`/`reap`/`shutdown`; panic isolation → `TaskError::Panic`.
4. W8-T4 — `TaskHandle::poll`/`cancel` with deadline.
5. W8-T5 — `Scheduler::begin_frame`/`commit`: drains worker IR via `try_recv` at frame-budget deadline; never blocks.
6. W8-T6 — `SharedState` (SAB + `DeviceCaps` snapshot + `MonotonicClock`); workers receive `SharedState`, never `GPUDevice`.
7. W8-T7 — COOP/COEP detection; `CrossOriginIsolationUnavailable` is fatal (§3.6 `BootstrapError`).
8. W8-T8 — Tests: backpressure yield, panic isolation, framing quarantine, commit-never-blocks under empty ring.
**DoD:** `cargo test -p alkalive-ipc` green; a panicking worker resolves its handle to `Err(Panic)` and the pool recycles the slot; `commit` returns within frame budget with an empty ring.
**Dependencies:** W3, W5 (IR is the IPC payload).

### Wave 9 — DOM bridge + SEO
**Goal:** Realise ADR 020/012: the metadata-only DOM surface and the structured navigation contract — closed under ADRs 012/013/019/020.
**Tasks:**
1. W9-T1 — `DomBridge` impl: `setTitle`, `setMeta`, `serveSnapshot` — no other verbs.
2. W9-T2 — `NavigationContract` impl: `declareRoutes`, `serializeState`; host retains URL/history.
3. W9-T3 — `SeoSnapshot` build-time and on-demand (crawler-UA) emission; off-render-thread.
4. W9-T4 — `DomError` graceful degradation: build-time snapshot continues to be served; render loop never blocks.
5. W9-T5 — Static assertion (compile-time test) that `DomBridge` exposes no method outside the ADR 020 set; IME explicitly absent (§9.5).
6. W9-T6 — Tests: meta rejection, snapshot write failure, route decline, state-unserialisable.
**DoD:** `cargo test -p alkalive-dom` green; interface-surface test asserts the method set is exactly `{setTitle, setMeta, serveSnapshot, declareRoutes, serializeState}`.
**Dependencies:** W3.

### Wave 10 — Error handling + unified trace
**Goal:** Realise ADR 016 + §13: the unified author-owned trace, module-boundary isolator, and enumerated recovery strategies.
**Tasks:**
1. W10-T1 — `AlkALiveError` enum wiring every subsystem subtype (§13.1).
2. W10-T2 — `ErrorBoundary::trap` — panic-to-typed-`Failure` boundary; subtree teardown + dirty-rect quarantine.
3. W10-T3 — `ModuleIsolator` — quarantine/teardown/emitFailure; guarantees no exception escapes + dirty rect bounded (ADR 002).
4. W10-T4 — `TraceRecorder::enter`/`exit`/`watchFrame` — single timeline, no separate log sink.
5. W10-T5 — `RecoveryStrategy` table: last-known-good layout/frame, HMR full-reload, shader passthrough, font fallback, worker retry (§13.4).
6. W10-T6 — `FrameBudget` watchdog → `FrameOverrun` span (not exception) at 16.7 ms / 8.3 ms.
7. W10-T7 — `MemoryPool` HardCap/LRU/Backpressure enforcement for linear mem (256 MB), SAB (64 MB), glyph atlas (32 MB), pipeline cache (64 MB) (§12.7).
8. W10-T8 — Tests: panic-in-child isolates to parent slot; watchdog records a span on synthetic overrun; `LinearMemoryCeiling` raised at cap.
**DoD:** `cargo test -p alkalive-error alkalive-perf` green; a panic in a child subtree delivers exactly one typed `Failure` to the parent slot and zeroes to siblings.
**Dependencies:** W3, W4, W5, W8.

### Wave 11 — Testing harness
**Goal:** Realise ADR 014 + §14: contract-shaped, GPU-free, deterministic test surface.
**Tasks:**
1. W11-T1 — `MockBackend` impl recording draw calls; `assert_pass_count`, `draw_log`.
2. W11-T2 — `SoftwareBackend` deterministic rasteriser (ADR 016 split-determinism fallback).
3. W11-T3 — `MockTextStack` fixture-seeded `ShapedRun`s; `install_fixture`, `shaped_runs`.
4. W11-T4 — `ComponentTest`: `mount`/`drive`/`slot_output`/`expect_output`/`teardown` over typed contracts.
5. W11-T5 — `SceneSnapshot` with `fingerprint` cache; `SnapshotError` on `StateNotSerialisable`/`TraceGap`/`RasterClassMismatch`/`FingerprintCollision`.
6. W11-T6 — `TracePlayer::load`/`step`/`seek`/`assert_replay` — byte-identical frame replay.
7. W11-T7 — `TestHarness` wiring `MockBackend` + `MockTextStack` + `SoftwareBackend`; `snapshot→tick→assert_frame`.
8. W11-T8 — Tests: a recorded trace replays byte-identically under `SoftwareBackend`; a `TraceGap` returns `Inconclusive`.
**DoD:** `cargo test -p alkalive-test` green; a recorded frame replays byte-identically; the harness runs with no `GPUDevice` instantiated.
**Dependencies:** W3, W5, W6, W10.

### Wave 12 — Integration + DoD verification
**Goal:** End-to-end bootstrap, first-frame budget, and full-spec DoD verification.
**Tasks:**
1. W12-T1 — `Runtime` bootstrap sequencer (`Fetch→StreamingDecode→PipelinePrecompile→MemorySabSetup→FirstFrame`, §3.4).
2. W12-T2 — Wire `FrameLoop.tick` end-to-end: layout → render-graph compile → compositor commit → backend submit.
3. W12-T3 — COOP/COEP-gated SAB + worker pool priming in the bootstrap.
4. W12-T4 — First-frame budget measurement (target < 400 ms, binary ≤ 8 MB, §12.1).
5. W12-T5 — Frame-budget policing at 60 fps and 120 fps (§12.2).
6. W12-T6 — Integration test: a 3-module scene with text, layout, hit-test, and a worker-emitted overlay produces a deterministic `Frame` under `TestHarness`.
7. W12-T7 — Full ADR compliance sweep: every ADR 001–022 has a passing test or static assertion.
8. W12-T8 — Open-dependency status report: rendering-ABI, IME, GPU determinism (§12.8).
**DoD:** `cargo test --workspace` green; `cargo build --target wasm32-unknown-unknown --workspace` produces a binary ≤ 8 MB; first-frame < 400 ms in the CI harness; the ADR sweep shows 22/22 satisfied-or-deferred-with-trace.
**Dependencies:** all prior waves.

---

## 2. Task-to-ADR Traceability Matrix

| Wave | ADRs satisfied |
|---|---|
| W1 | (planning; ratifies all) |
| W2 | 008, 017, 018, 022 (build/toolchain), 021 (wasm threads target) |
| W3 | 001, 004, 005, 006, 007, 008, 009, 010, 011, 012, 013, 016, 018, 019, 020, 021, 022 (signature locks) |
| W4 | 007, 008, 009, 011, 015, 018 |
| W5 | 001, 002, 003, 006, 013, 017 |
| W6 | 002, 004, 022 |
| W7 | 010, 011, 013 |
| W8 | 003, 013, 016, 021 |
| W9 | 012, 013, 019, 020 |
| W10 | 002, 007, 008, 015, 016 |
| W11 | 014, 016, 022 (mock text) |
| W12 | 003, 016, 017, 021 (bootstrap + budgets) |

All 22 ADRs are covered; ADR 019 ships as deferred stubs (W3-T8) with a committed extension contract per §10.5.

---

## 3. Risk Register

**Open dependencies from spec §12.8:**
1. **Rendering-ABI contract (§4 ↔ §6)** — `ShapedRun` carries no attachment-format field; the glyph-quad → attachment binding is deferred to a future rendering-ABI ADR. *Mitigation:* W6-T8 defines an adapter; W5/W6 keep the boundary at `MeasuredRun`/`TextStack::measure`. *Owner:* future ADR.
2. **IME composition-event acquisition (§6 ↔ §9)** — ADR 020 forbids DOM input interop; `DomBridge` exposes no IME method. *Mitigation:* W9-T5 statically asserts IME absence; `Spec_Tradeoff_Note_IME.md` owns the resolution. *Owner:* future ADR.
3. **GPU determinism fallback (§4 ↔ §12 ↔ §14)** — the `SoftwareBackend` fallback bounds parity to one raster class only; cross-vendor pixel-identical parity is not claimed. *Mitigation:* W11-T2 implements the software rasteriser; W11-T8 asserts byte-identity within a raster class. *Owner:* ADR 016 caveat.

**New risks introduced by this plan:**
4. **HarfRust fork drift** — vendored fork may diverge from upstream shaping fixes. *Mitigation:* W2-T4 records the fork point + upstream ref; quarterly rebase task.
5. **`wgpu` binding under `wasm32-unknown-unknown`** — `WebGPUBackend` (W5-T5) depends on a WebGPU binding that may lag the spec. *Mitigation:* keep `Backend` trait abstract so the binding is swappable; `MockBackend` is the CI path of record.
6. **COOP/COEP availability in embedding hosts** — `CrossOriginIsolationUnavailable` is fatal (W8-T7). *Mitigation:* `credentialless` COEP; documented degradation path to per-graph separate devices (ADR 003).
7. **8 MB binary cap with HarfRust payload** — ADR 018 tree-shaking must hold. *Mitigation:* W12-T4 enforces the cap on CI; W2-T7 deny-lists ungrafted deps.

---

## 4. Execution Notes

- One sub-agent per task; the wave-owner merges sequentially.
- Every task commit must keep `cargo build --workspace` green; trait-only tasks (W3) are merged only after a reviewer confirms signature/spec parity.
- Waves W4–W11 may overlap **only** across non-dependent crates (e.g. W9 may start once W3 lands; W6 may not start before W5).
- W12 is serialising; no integration work begins until W11 is green.

*End of Implementation Plan.*

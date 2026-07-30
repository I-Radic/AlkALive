# Gap Analysis — AlkALive Codebase vs Design Artifacts

**Date:** 2026-07-26
**Sources:** SPECIFICATION.md (14 sections), ADR.md (22 ADRs), FINE_DRAFT.md, ROUGH_DRAFT.md
**Current state:** 13 crates, 121 tests, 0 todo!(), 8,125 lines

---

## Critical Gaps (5)

### C1: alkalive-runtime crate is empty — §3 Runtime Architecture unimplemented
- **Spec ref:** §3.2, §3.4, §3.5, §3.6
- **ADR ref:** ADR 003, ADR 017, ADR 021
- **Code:** `alkalive-runtime/src/lib.rs` (7 lines — doc comment only)
- **Description:** No Runtime struct, FrameLoop driver, Compositor wiring, BootstrapSequence, or BootstrapError. The integration spine for every other crate is absent.
- **Action:** Implement Runtime, FrameLoop, bootstrap sequencer, and wire all subsystems.

### C2: No frame-loop wiring (layout → compile → compositor → submit)
- **Spec ref:** §4.7, §3.5
- **ADR ref:** ADR 003, ADR 013
- **Code:** `alkalive-render/src/lib.rs` (RenderLoop/Compositor traits only, no concrete impl)
- **Description:** §4.7's per-frame pipeline (layout → render-graph compile → compositor commit → backend submit) has no implementation connecting the stages.
- **Action:** Build the retain-mode frame loop in alkalive-runtime.

### C3: compile() discards merged data; CompiledGraph is empty unit struct
- **Spec ref:** §4.3
- **ADR ref:** ADR 001
- **Code:** `alkalive-render/src/lib.rs:442-534`
- **Description:** The compiler merges + topo-sorts but drops all merged data. CompiledGraph/MergedGraph/CompiledFrame/CulledFrame are unit structs. Batching, barrier insertion, and occlusion-cull are absent. `dirty` and `depth` params ignored.
- **Action:** Populate CompiledGraph with reordered/batched passes; implement occlusion-cull.

### C4: Cross-crate ModuleId type duplication — 6 incompatible definitions
- **Spec ref:** §2.7, §1.6
- **ADR ref:** ADR 002, ADR 007
- **Code:** core (u64), error (u64), layout (u32), text (u32), render (named struct), test (unit)
- **Description:** Six mutually incompatible ModuleId definitions block cross-crate interop. The u32 vs u64 mismatch is a silent truncation hazard.
- **Action:** Define ModuleId once in alkalive-core; re-export from all other crates.

### C5: deny.toml does not enforce ADR 018's "no external deps"
- **Spec ref:** §12.7, §1.4
- **ADR ref:** ADR 018
- **Code:** `deny.toml`
- **Description:** cargo-deny's `[bans] allow` permits listed crates but doesn't deny others. Any crates.io crate would pass. cargo-deny is not installed; no CI invokes it.
- **Action:** Fix deny.toml to use a deny-by-default strategy; install cargo-deny; add CI.

---

## High Gaps (14)

### H1: No concrete WorkerPool, Scheduler, or TaskHandle implementations
- **Spec ref:** §11.5, §11.1
- **ADR ref:** ADR 021
- **Code:** `alkalive-ipc/src/lib.rs:182-214` (traits only)
- **Description:** No thread spawning, no SharedState construction, no panic-reap path, no begin_frame/commit driver.
- **Action:** Implement LocalWorkerPool + LocalScheduler with in-process task execution.

### H2: CassowarySolver ignores DirtySet, MeasuredRun, and dt
- **Spec ref:** §5.3, §5.4, §5.6, §5.7
- **ADR ref:** ADR 004, ADR 002
- **Code:** `alkalive-layout/src/lib.rs:616-687`
- **Description:** solve() discards dirty/measured/dt. Never invokes MeasuredRun. Always returns Solved. Impulse/GraphLayout skipped. Last-known-good retention unreachable.
- **Action:** Scope solves to DirtySet; invoke MeasuredRun for Text nodes; implement Partial/Unsatisfiable.

### H3: No structural error isolation / panic trapping at owning boundary
- **Spec ref:** §2.5
- **ADR ref:** ADR 007
- **Code:** `alkalive-core/src/lib.rs:314-340`
- **Description:** Slot::mount doesn't track children, trap panics, tear down subtrees, or emit Failure. The Failure struct is never constructed.
- **Action:** Implement child-tracking, panic-catch-unwind, teardown, and Failure emission.

### H4: PipelineCache unbounded; no 64MB LRU cap, no degraded fallback
- **Spec ref:** §4.6, §12.7
- **Code:** `alkalive-render/src/lib.rs:619-660`
- **Description:** Unbounded Vec append. No LRU eviction. PipelineError::CacheMiss never emitted.
- **Action:** Add 64MB-bounded LRU with degraded-builtin fallback.

### H5: Animation::tick does not interpolate keyframes or write back
- **Spec ref:** §7.5
- **Code:** `alkalive-style/src/lib.rs:227-249`
- **Description:** Only advances elapsed + flips to Completed. Never samples keyframes, applies easing, honours Interpolation mode, respects Idle/Paused, or writes values back.
- **Action:** Implement keyframe sampling, easing, state gating, and write-back hook.

### H6: HitTesterImpl::precise_pick is a stub — no GPU pick-buffer readback
- **Spec ref:** §8.2
- **ADR ref:** ADR 010
- **Code:** `alkalive-input/src/lib.rs:368-385`
- **Description:** Delegates to broad-phase hit_test and flags precise=true. No GPU readback. Returns sentinel handle on no-hit.
- **Action:** Wire to render-graph pick-buffer; return InputError on failure; test both paths.

### H7: No grab arbitration — "most recent grab wins, loser gets Cancel"
- **Spec ref:** §8.3, §8.4
- **ADR ref:** ADR 013
- **Code:** `alkalive-input/src/lib.rs:417-486`
- **Description:** No grab registry, no arbitrator, no synthetic-Cancel emission. SimpleGrabHandle exists in isolation.
- **Action:** Add grab table keyed by (device, device_id) enforcing last-grab-wins + Cancel to displaced owners.

### H8: FocusManagerImpl::dispatch is a no-op
- **Spec ref:** §8.5
- **ADR ref:** ADR 011
- **Code:** `alkalive-input/src/lib.rs:671-677`
- **Description:** Returns empty Vec unconditionally. No routing, no grab consultation, no error normalisation.
- **Action:** Implement event routing to hit objects/grabs; return InputError variants per §8.6.

### H9: InputError normalisation absent — errors never produced
- **Spec ref:** §8.6
- **Code:** `alkalive-input/src/lib.rs:727-740`
- **Description:** All 6 InputError variants defined but dispatch always returns empty Vec. No code constructs any InputError.
- **Action:** Implement normalisation: orphaned grabs, out-of-range IDs, unmatched touch, stale mirror.

### H10: No SAB-backed IPCSocket (only in-process VecDeque)
- **Spec ref:** §11.3, §11.4
- **ADR ref:** ADR 021
- **Code:** `alkalive-ipc/src/lib.rs:268-347`
- **Description:** LocalIPCSocket is VecDeque-backed. No ring buffer, no Atomics signaling, no serialization. SharedArrayBuffer is a unit-struct placeholder.
- **Action:** Add SAB-backed socket behind wasm32 cfg; document in-process fallback.

### H11: Only one MemoryPool implementation; 5 other budget pools missing
- **Spec ref:** §12.3, §12.4, §12.7
- **Code:** `alkalive-perf/src/lib.rs:380-452`
- **Description:** Only LinearMemoryPool ships. No SAB/Atlas/Pipeline/Attachment/Instance pools. No FrameBudget watchdog.
- **Action:** Add concrete MemoryPool impls for each §12.7 budget row + FrameBudgetWatchdog.

### H12: Error crate's DomError has 1 variant vs spec's 6
- **Spec ref:** §13.1, §9.6
- **Code:** `alkalive-error/src/lib.rs:90-94`
- **Description:** Error crate's DomError has only `SnapshotEmitFailed(String)`. alkalive-dom has correct 6-variant enum. AlkALiveError::Dom loses diagnostic detail.
- **Action:** Replace error crate's DomError with 6-variant mirror or re-export from alkalive-dom.

### H13: No concrete ComponentTest implementation
- **Spec ref:** §14.3, §14.5
- **ADR ref:** ADR 014
- **Code:** `alkalive-test/src/lib.rs:265-276`
- **Description:** Trait exists but no implementor. ADR 014's typed component test surface cannot be exercised.
- **Action:** Ship SimpleComponentTest with fixture registry.

### H14: No concrete TracePlayer implementation
- **Spec ref:** §14.3, §14.4
- **ADR ref:** ADR 016
- **Code:** `alkalive-test/src/lib.rs:286-295`
- **Description:** No impl for load/step/seek/assert_replay. SimpleTestHarness::replay always returns Pass.
- **Action:** Ship SimpleTracePlayer backed by in-memory Vec<TickFrame>.

---

## Medium Gaps (18)

### M1: §3.6 Compositor interface (merge/compile/occlusion_cull/submit) not implemented
- **Spec ref:** §3.6 | **Code:** `alkalive-render/src/lib.rs:673-680`

### M2: assert_local only catches direct cross-module facet references
- **Spec ref:** §5.5 | **Code:** `alkalive-layout/src/lib.rs:584-614`

### M3: Backend/RenderLoop/Compositor traits abstract — no concrete impls
- **Spec ref:** §4.1, §4.4, §4.5, §14 | **Code:** `alkalive-render/src/lib.rs`

### M4: SlotError::TypeMismatch missing (Type, Type) payload
- **Spec ref:** §2.9 | **Code:** `alkalive-core/src/lib.rs:444-451`

### M5: Signal::emit returns () — EmitAfterDestroyed unreachable
- **Spec ref:** §2.9 | **Code:** `alkalive-core/src/lib.rs:398-404`

### M6: Visibility::Module check_access simplified to owner-only
- **Spec ref:** §2.3 | **Code:** `alkalive-core/src/lib.rs:300-311`

### M7: LayoutSolution never wired into GPU instance buffers
- **Spec ref:** §5.6 | **Code:** `alkalive-layout/src/lib.rs:424-436`

### M8: ShapeContext omits Language/TextStyle
- **Spec ref:** §6.3 | **Code:** `alkalive-text/src/lib.rs:218-226`

### M9: Mock text stack behaviourally degenerate; error/fallback paths untested
- **Spec ref:** §6.2, §6.6, §6.7 | **Code:** `alkalive-text/src/lib.rs:608-737`

### M10: Color/Opacity/LineWidth lack clamping constructors
- **Spec ref:** §7.7 | **Code:** `alkalive-style/src/lib.rs:59-68`

### M11: No shader-compile-failure fallback to passthrough
- **Spec ref:** §7.7 | **Code:** `alkalive-style/src/lib.rs:96-138`

### M12: Animation construction-time validation never fires
- **Spec ref:** §7.7 | **Code:** `alkalive-style/src/lib.rs:233-266`

### M13: HitTesterImpl refresh/invalidate clears entire mirror
- **Spec ref:** §8.2 | **Code:** `alkalive-input/src/lib.rs:333-392`

### M14: SimpleGestureState never produces Commit/Cancel/Grab
- **Spec ref:** §8.4 | **Code:** `alkalive-input/src/lib.rs:493-591`

### M15: Only 1/5 RecoveryStrategy implementations shipped
- **Spec ref:** §13.4 | **Code:** `alkalive-error/src/lib.rs:515-533`

### M16: ErrorBoundary::trap doesn't trap panics; TraceRecorder exit/report are no-ops
- **Spec ref:** §13.3, §13.5 | **Code:** `alkalive-error/src/lib.rs:325-383`

### M17: SimpleTestHarness snapshot/tick/assert_frame/replay are stubs
- **Spec ref:** §14.4 | **Code:** `alkalive-test/src/lib.rs:468-494`

### M18: README.md stale; no CI workflow; ADR 013 not statically enforced
- **Spec ref:** §1, §12.7 | **Code:** README.md, .github/ (absent)

---

## Low Gaps (4)

### L1: Only LinearEasing provided
- **Spec ref:** §7.5 | **Code:** `alkalive-style/src/lib.rs:186-203`

### L2: Slot/Signal modeled as methods, not Fn fields
- **Spec ref:** §2.7 | **Code:** `alkalive-core/src/lib.rs`

### L3: DeviceKindSet/ButtonSet/ModifierSet bare newtypes
- **Spec ref:** §8.1 | **Code:** `alkalive-input/src/lib.rs:124-138`

### L4: HMR Destroyed→Loading edge not modeled
- **Spec ref:** §2.4 | **Code:** `alkalive-core/src/lib.rs:185-201`

---

## Summary

| Severity | Count |
|---|---|
| Critical | 5 |
| High | 14 |
| Medium | 18 |
| Low | 4 |
| **Total** | **41** |

## Non-Gaps (verified compliant)

- All 22 ADRs have code trace in ≥1 crate
- `#![forbid(unsafe_code)]` in all 13 crates
- `todo!()` count = 0 (all eliminated)
- Zero external dependencies (ADR 018 holds today)
- `rust-toolchain.toml` pins 1.97.1 + wasm32 target
- ADR 019 (a11y deferred) documented in code with stubs
- ADR 020 (DOM metadata-only) 5-method surface present
- §12.8 open dependencies all 3 still tracked
- IME trade-off note exists with recommended approach

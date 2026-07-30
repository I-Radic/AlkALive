# Gap Implementation Plan

**Derived from:** `GAP_ANALYSIS.md` (41 gaps: 5 Critical, 14 High, 18 Medium, 4 Low)
**Goal:** Resolve all Critical, High, and Medium gaps through implementation waves.

---

## Wave A — Cross-Crate Unification & Infrastructure (Critical)

**Gaps addressed:** C4 (ModuleId duplication), C5 (deny.toml), M18 (README/CI)
**DoD:** Single canonical ModuleId in alkalive-core, re-exported everywhere; deny.toml fixed; README updated.

Tasks:
- A1: Make alkalive-core the canonical ModuleId owner; add alkalive-core as dep to layout/render/text/error/test; replace local ModuleId definitions.
- A2: Fix deny.toml to deny-by-default; install cargo-deny.
- A3: Update README.md with current state (8,125 lines, 121 tests, 0 todo!()).
- A4: Add .github/workflows/ci.yml with build/test/clippy/fmt gates.

## Wave B — Runtime Crate & Frame Loop (Critical)

**Gaps addressed:** C1 (runtime empty), C2 (no frame loop), C3 (compile discards data)
**DoD:** alkalive-runtime has Runtime/FrameLoop/BootstrapSequence; compile() populates CompiledGraph.

Tasks:
- B1: Implement Runtime struct + BootstrapSequence enum + BootstrapError in alkalive-runtime.
- B2: Implement FrameLoop concrete driver (tick → layout → compile → commit → submit).
- B3: Populate CompiledGraph with merged/reordered/batched passes; implement occlusion-cull stub.
- B4: Add MockBackend impl in alkalive-render (for headless testing).
- B5: Add tests for bootstrap phases and frame loop tick.

## Wave C — Error Handling & Recovery (High)

**Gaps addressed:** H3 (panic trapping), H12 (DomError mismatch), H13 (ComponentTest), H14 (TracePlayer), M15 (RecoveryStrategy), M16 (ErrorBoundary)
**DoD:** All 8 AlkALiveError variants match spec; 5 RecoveryStrategy impls; ComponentTest + TracePlayer concrete impls.

Tasks:
- C1: Fix DomError in alkalive-error to 6-variant mirror of alkalive-dom.
- C2: Add FullReloadRecovery, ShaderPassthroughRecovery, FontFallbackRecovery, WorkerRetryRecovery.
- C3: Implement SimpleComponentTest in alkalive-test.
- C4: Implement SimpleTracePlayer in alkalive-test.
- C5: Add tests for each RecoveryStrategy + ComponentTest + TracePlayer.

## Wave D — Input System Completion (High)

**Gaps addressed:** H6 (precise_pick), H7 (grab arbitration), H8 (dispatch no-op), H9 (InputError), M13 (invalidate), M14 (GestureState)
**DoD:** dispatch routes events; grab arbitration works; InputError variants produced.

Tasks:
- D1: Implement grab registry + arbitration in FocusManagerImpl.
- D2: Implement dispatch with event routing + InputError normalisation.
- D3: Implement scoped invalidate (per LayoutScope, not clear-all).
- D4: Enrich SimpleGestureState to produce Commit/Cancel/Grab outcomes.
- D5: Add tests for grab arbitration, dispatch routing, InputError production.

## Wave E — Layout & Styling Completion (High/Medium)

**Gaps addressed:** H2 (solver ignores params), H5 (animation), M2 (assert_local), M7 (LayoutSolution wiring), M10-M12 (style)
**DoD:** Solver uses DirtySet + MeasuredRun; animation interpolates keyframes; style clamps values.

Tasks:
- E1: CassowarySolver: scope to DirtySet, invoke MeasuredRun for Text nodes, return Partial/Unsatisfiable.
- E2: Animation::tick: sample keyframes, apply easing, honour Interpolation, gate on state.
- E3: Add clamping constructors for Color/Opacity/LineWidth.
- E4: Add Animation::new() validating constructor.
- E5: Add tests for solver Partial/Unsatisfiable, animation interpolation, clamping.

## Wave F — IPC & Performance (High/Medium)

**Gaps addressed:** H1 (WorkerPool), H10 (SAB socket), H11 (MemoryPools), M3 (LocalIPCSocket capacity)
**DoD:** LocalWorkerPool + LocalScheduler impls; 5 MemoryPool impls; backpressure enforcement.

Tasks:
- F1: Implement LocalWorkerPool + LocalTaskHandle + LocalScheduler in alkalive-ipc.
- F2: Add capacity/backpressure enforcement to LocalIPCSocket.
- F3: Add SAB/Atlas/Pipeline/Attachment/Instance MemoryPool impls in alkalive-perf.
- F4: Add FrameBudgetWatchdog impl.
- F5: Add tests for worker pool, backpressure, memory pool caps.

## Wave G — Medium/Low Gap Resolution

**Gaps addressed:** M4-M9, M16-M17, L1-L4
**DoD:** All Medium gaps resolved or formally deferred; Low gaps documented.

Tasks:
- G1: Fix SlotError::TypeMismatch to carry (Type, Type).
- G2: Add Language field to ShapeContext.
- G3: Enrich MockTextStack with parameterizable behavior.
- G4: Add shader-compile-failure fallback to passthrough.
- G5: Add EaseIn/EaseOut/EaseInOut easings.
- G6: Add DeviceKindSet helpers (contains/insert/iter).
- G7: Fix SimpleTestHarness to compute real fingerprints.

## Wave H — Final Verification & Traceability

**DoD:** cargo test --workspace passes; wasm32 build passes; trace matrix complete.

Tasks:
- H1: Run full workspace test suite.
- H2: Run wasm32 build.
- H3: Produce traceability matrix (spec requirement → code location).
- H4: Update README with final status.

# Unfinished Implementations Report

**Date:** 2026-07-31
**Scan method:** `grep -rn` for `todo!()`, `unimplemented!()`, `// TODO`, `// FIXME`, `// HACK`, `deferred`, `stub`, `Mock` across `crates/*/src/lib.rs`

---

## Summary

| Category | Count |
|---|---|
| `todo!()` macro calls | 0 |
| `unimplemented!()` macro calls | 0 |
| `// TODO` comments | 8 |
| `// FIXME` comments | 0 |
| `// HACK` comments | 0 |
| `deferred` references | 20 |
| `stub`/`Stub` references | 82 (mostly doc comments, not code stubs) |
| Mock implementations (public) | ~8 (intentional test-only types) |

**Overall:** The codebase has **zero runtime panics** from unfinished code (`todo!()` / `unimplemented!()`). All remaining work is documented in `// TODO` comments and `deferred` doc references.

---

## Medium-Severity Items (8 `// TODO` comments)

### 1. Signal observer registry (alkalive-core)
- **File:** `crates/alkalive-core/src/lib.rs:400, 414`
- **TODO:** `dispatch value to registered subscribers` + `register listener in runtime's subscriber table`
- **Spec ref:** §2.7 (Signal<T>), ADR 014
- **Current state:** `emit()` stores last_value; `subscribe()` mints Subscription IDs. No dispatch.
- **Severity:** Medium — Signal dispatch needs runtime integration (ADR 007 render-object tree).

### 2. Panic trapping in ErrorBoundary (alkalive-error)
- **File:** `crates/alkalive-error/src/lib.rs:344`
- **TODO:** `wrap op() in std::panic::catch_unwind`
- **Spec ref:** §13.3, ADR 016
- **Current state:** `trap()` handles `Err` path but not panics. Needs `UnwindSafe` bounds.
- **Severity:** Medium — panic isolation is a core spec requirement but needs trait bound changes.

### 3. Trace recording in ErrorBoundary::report (alkalive-error)
- **File:** `crates/alkalive-error/src/lib.rs:364`
- **TODO:** `record the failure + rect as a span on the unified trace`
- **Spec ref:** §13.5, ADR 016
- **Current state:** `report()` is a no-op. Needs a span store.
- **Severity:** Medium — trace integration is needed for observability.

### 4. Trace recording in TraceRecorder::exit (alkalive-error)
- **File:** `crates/alkalive-error/src/lib.rs:382`
- **TODO:** `record the span close + result on the unified trace`
- **Spec ref:** §13.5, ADR 016
- **Current state:** `exit()` is a no-op. Needs a span store.
- **Severity:** Medium — same as #3.

### 5. PipelineCache LRU bound (alkalive-render)
- **File:** `crates/alkalive-render/src/lib.rs:670`
- **TODO:** `bound the cache at 64 MB (§12.7) with LRU eviction`
- **Spec ref:** §4.6, §12.7
- **Current state:** Unbounded `Vec` append.
- **Severity:** Medium — cache is functional but unbounded; LRU is a budget requirement.

### 6. Animation keyframe interpolation write-back (alkalive-style)
- **File:** `crates/alkalive-style/src/lib.rs:330`
- **TODO:** `Interpolate kf_a.value <-> kf_b.value` (compute the interpolated value)
- **Spec ref:** §7.5
- **Current state:** `tick()` advances elapsed and computes easing factor but doesn't interpolate or write back.
- **Severity:** Medium — animation clock works but visual values don't change.

### 7. Custom property defaults (alkalive-style)
- **File:** `crates/alkalive-style/src/lib.rs:438`
- **TODO:** `Custom property defaults are module-declared`
- **Spec ref:** §7.2
- **Current state:** Custom properties fall back to color default.
- **Severity:** Low — custom properties are an extension feature.

### 8. HarfRust family/weight/style matching (alkalive-text)
- **File:** `crates/alkalive-text/src/lib.rs:592`
- **TODO:** `real family/weight/style matching is deferred`
- **Spec ref:** §6.2
- **Current state:** `resolve()` returns the first loaded font (FontId(0)).
- **Severity:** Medium — real font selection is needed for multi-font applications.

---

## Deferred Items (intentionally deferred, not actionable)

These are documented deferrals with clear ADR/spec backing:

1. **Accessibility (ADR 019)** — a11y is deferred by owner directive. 8 `deferred` references in alkalive-a11y.
2. **SAB-backed IPC socket (ADR 021)** — LocalIPCSocket is the in-process stand-in. 2 references in alkalive-ipc.
3. **GPU pick-buffer readback (ADR 010)** — HitTesterImpl::precise_pick is a stub. 1 reference in alkalive-input.
4. **Render-object tree (ADR 007)** — needed for full Signal dispatch, grab arbitration, tab order. Referenced in alkalive-core, alkalive-input.
5. **BiDi segmentation** — HarfRust handles shaping but full BiDi is not exercised in tests. 1 reference in alkalive-text.
6. **Font fallback chain** — `fallback_chain()` returns empty. 1 reference in alkalive-text.

---

## Implementation Plan

### Wave 3: PipelineCache LRU + Trace Span Store
**DoD:** PipelineCache bounded at 64MB with LRU; TraceRecorder stores spans; ErrorBoundary::report records failures.

### Wave 4: Animation Interpolation + Font Matching
**DoD:** Animation::tick interpolates keyframes and writes back; HarfRustFontRegistry::resolve does real family matching.

### Wave 5: Signal Observer Registry + Panic Trapping
**DoD:** Signal::emit dispatches to subscribers; ErrorBoundary::trap catches panics via catch_unwind.

### Wave 6: Final Verification
**DoD:** All Medium TODOs resolved or formally deferred; cargo test passes; cargo build wasm32 passes.

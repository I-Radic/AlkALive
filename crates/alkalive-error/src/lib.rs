//! AlkALive alkalive-error crate.
//!
//! Error handling & resilience trait surface — see
//! `docs/SPECIFICATION.md` §13 (Error Handling & Resilience).
//! Realises ADR 016 (unified author-owned trace), ADR 007/008 (module
//! isolation at boundaries), and ADR 002 (dirty-rect bounded invalidation).
//!
//! Wave 10: replaces the Wave 3 trait skeletons with real stub-level
//! implementations. [`ErrorBoundary`], [`TraceRecorder`], and
//! [`ModuleIsolator`] now carry default method bodies plus concrete
//! reference implementors ([`TrappingBoundary`], [`AuthorTraceRecorder`],
//! [`BoundaryIsolator`]). Wave 5 makes [`RecoveryStrategy`]'s `category`
//! and `recover` methods required (no `todo!()` defaults) and ships
//! [`LastKnownGoodRecovery`] as the first concrete strategy implementor.
//! No cross-crate dependencies; types referenced from other sections
//! (e.g. [`FrameBudgetEvent`], [`ModuleId`]) are local placeholders.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use alkalive_core::ModuleId;

use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Sub-enums (§13.1 / §13.2)
// ============================================================================

/// Compile / validation error subtype (ADR 009 — source soundness or WASM
/// validator failure).
#[derive(Debug, Clone)]
pub enum CompileError {
    /// Source-level type error.
    TypeError(String),
    /// WASM validator rejected the compiled module.
    WasmValidationReject(String),
}

/// Module lifecycle error subtype (ADR 015 / ADR 017 — load, HMR rehydrate,
/// pipeline precompile).
#[derive(Debug, Clone)]
pub enum LifecycleError {
    /// Module decode failed.
    DecodeFailed(String),
    /// HMR rehydrate schema mismatch.
    RehydrateSchemaMismatch(String),
    /// WebGPU pipeline precompile failed.
    PipelinePrecompileFailed(String),
}

/// Layout solve error subtype (ADR 004 — solver infeasible / locality
/// violation).
#[derive(Debug, Clone)]
pub enum LayoutError {
    /// Constraint system infeasible.
    Infeasible(String),
    /// Cross-module flex/percentage dependency breached locality (ADR 002).
    LocalityViolation(String),
}

/// Rendering error subtype (§4 — render-graph compile, attachment lifetime,
/// GPU device-lost).
#[derive(Debug, Clone)]
pub enum RenderError {
    /// Render-graph compile failure.
    GraphCompile(String),
    /// Attachment lifetime violation.
    AttachmentLifetime(String),
    /// GPU device lost.
    DeviceLost(String),
}

/// Text shaping error subtype (§6 — HarfRust shaper, font, glyph-run).
#[derive(Debug, Clone)]
pub enum TextError {
    /// Missing glyph after fallback chain descent.
    MissingGlyph(String),
    /// Shaper crash.
    ShaperCrash(String),
}

/// Input error subtype (§8 — hit-test, gesture, focus writer).
#[derive(Debug, Clone)]
pub enum InputError {
    /// Hit-test mirror desynchronised from layout.
    HitTestMirrorDesync(String),
    /// Focus-writer contention.
    FocusWriterContention(String),
}

/// DOM error subtype (§9 — `<title>`/`<meta>` + SEO snapshot only).
#[derive(Debug, Clone)]
pub enum DomError {
    /// SEO snapshot emit failure (non-hot-path).
    SnapshotEmitFailed(String),
}

/// Threading error subtype (§11 — worker IPC, socket, SharedArrayBuffer).
#[derive(Debug, Clone)]
pub enum ThreadError {
    /// Worker crashed.
    WorkerCrash(String),
    /// Socket IPC corruption.
    SocketCorruption(String),
    /// `SharedArrayBuffer` unavailable (COOP/COEP not satisfied).
    SharedArrayBufferUnavailable(String),
}

// ============================================================================
// AlkALiveError (§13.1)
// ============================================================================

/// Unifying error enum.
///
/// Every `Result` channel crossing a module boundary is parametrised over
/// `AlkALiveError`. Subsystem-specific subtypes preserve diagnostic detail
/// without widening the cross-module contract. The runtime never lets an
/// exception cross a module boundary; every subsystem failure is funneled
/// into a typed `AlkALiveError`, observed through the unified trace
/// (ADR 016), and recovered by an enumerated [`RecoveryStrategy`].
#[derive(Debug, Clone)]
pub enum AlkALiveError {
    /// ADR 009 — source-soundness or WASM validator failure.
    CompileValidation(CompileError),
    /// ADR 015 / ADR 017 — load, HMR rehydrate, pipeline precompile.
    ModuleLifecycle(LifecycleError),
    /// ADR 004 — solver infeasible / locality violation.
    LayoutSolve(LayoutError),
    /// §4 — render-graph compile, attachment, draw-call.
    Rendering(RenderError),
    /// §6 — HarfRust shaper, font, glyph-run.
    TextShaping(TextError),
    /// §8 — hit-test, gesture, focus writer.
    Input(InputError),
    /// §9 — `<title>`/`<meta>` + SEO snapshot only.
    Dom(DomError),
    /// §11 — worker IPC, socket, SharedArrayBuffer.
    Threading(ThreadError),
}

// ============================================================================
// Recovery (§13.4)
// ============================================================================

/// Outcome of a [`RecoveryStrategy::recover`] call (§13.4 / §13.5).
#[derive(Debug, Clone)]
pub enum RecoveryOutcome {
    /// Retained last-known-good layout / frame; emitted a placeholder in
    /// the dirty rect.
    RetainedLastKnownGood,
    /// Fell back to a full reload; `state_lost` indicates whether
    /// application state was preserved.
    FullReload {
        /// True iff application state was lost in the reload.
        state_lost: bool,
    },
    /// Swapped to a passthrough WGSL shader; pipeline precompile deferred.
    ShaderPassthrough,
    /// Descended the font fallback chain.
    FontFallback,
    /// Reissued the failing operation (e.g. worker crash → reschedule).
    Retried,
}

/// Recovery context handed to a [`RecoveryStrategy`].
#[derive(Debug, Clone)]
pub struct RecoveryContext {
    /// The error being recovered.
    pub error: AlkALiveError,
    /// Slot the error originated in.
    pub slot: SlotId,
    /// Dirty-rect scope of the failure (ADR 002).
    pub rect: DirtyRect,
    /// Trace span the error was recorded on (ADR 016).
    pub span: SpanId,
}

// ============================================================================
// Trace surface (§13.5 / §13.6)
// ============================================================================

/// Kind of trace span on the unified author-owned timeline (ADR 016).
///
/// Local to this crate (the perf crate defines its own `SpanKind`
/// for the budget surface). Variants follow the §13.3 propagation diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpanKind {
    /// Author logic span.
    Logic,
    /// Layout span.
    Layout,
    /// Draw span.
    Draw,
    /// Recovery span.
    Recovery,
    /// Frame-budget watchdog span.
    Watchdog,
    /// Module-boundary trap span.
    Boundary,
}

/// Identifier of a single trace on the unified timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(());

/// Identifier of a span on the unified timeline.
///
/// The inner `u64` is public so that Wave 10 trait bodies (and tests) can
/// construct sentinel values such as `SpanId(0)`. Concrete recorders mint
/// non-zero ids from an [`AtomicU64`] counter (see [`AuthorTraceRecorder`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(pub u64);

/// Attributes attached to a span on enter.
#[derive(Debug, Clone)]
pub struct SpanAttrs {
    /// Frame this span belongs to.
    pub frame_id: u64,
    /// Stage this span attributes to (free-form string; the perf crate
    /// holds the typed `StageId` mirror).
    pub stage: String,
    /// Optional parent span.
    pub parent: Option<SpanId>,
}

/// Per-module, per-object invalidation subset bounding per-frame work to
/// the changed region rather than the full tree (ADR 002).
///
/// Local placeholder; the render crate will supply the canonical definition
/// once cross-crate wiring lands. Derives [`Default`] (`{x:0, y:0, w:0, h:0}`)
/// so that trait stubs can construct a zero-rect sentinel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirtyRect {
    /// Minimum x coordinate.
    pub x: i32,
    /// Minimum y coordinate.
    pub y: i32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

// ============================================================================
// Module isolation (§13.3 / §13.5)
// ============================================================================

/// Identifier of a slot in a parent's child-slot table (§2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub u64);

/// Name of a slot — the string key under which a child is mounted.
#[derive(Debug, Clone)]
pub struct SlotName(());

/// Typed failure delivered to a parent slot when a child subtree is torn
/// down (§13.3). Carries the slot, the [`AlkALiveError`], the dirty-rect
/// scope, and the trace span id.
#[derive(Debug, Clone)]
pub struct Failure {
    /// Slot the failure originated in.
    pub slot: SlotId,
    /// The typed error.
    pub error: AlkALiveError,
    /// Dirty-rect scope of the failure (ADR 002).
    pub rect: DirtyRect,
    /// Trace span the failure was recorded on (ADR 016).
    pub span: SpanId,
}

/// Report returned by [`ModuleIsolator::teardown`] describing what was
/// reclaimed and what was quarantined.
#[derive(Debug, Clone)]
pub struct TeardownReport {
    /// Module that was torn down.
    pub module: ModuleId,
    /// Dirty rect quarantined (ADR 002).
    pub quarantined_rect: DirtyRect,
    /// Slots whose values were released.
    pub released_slots: Vec<SlotId>,
    /// Trace span covering the teardown (ADR 016).
    pub span: SpanId,
}

/// Event returned by a frame-budget watchdog (§13.6).
///
/// Local placeholder for the perf crate's
/// `alkalive_perf::FrameBudgetEvent`; kept crate-local to preserve
/// independence (no cross-crate dependency).
#[derive(Debug, Clone)]
pub struct FrameBudgetEvent {
    /// Span watching the frame budget.
    pub span: SpanId,
    /// Budget ceiling in milliseconds.
    pub budget_ms: f32,
    /// Elapsed frame time in milliseconds.
    pub elapsed_ms: f32,
    /// Breach flagged, if any (free-form string; typed breach lives in
    /// the perf crate).
    pub breach: Option<String>,
}

// ============================================================================
// Traits (§13.5)
// ============================================================================

/// Boundary that traps panics in a child subtree and delivers a typed
/// [`Failure`] to the parent slot (§13.3 / §13.5).
///
/// Guarantees: no exception escapes; dirty rect bounded (ADR 002); the
/// rest of the tree is unaffected.
pub trait ErrorBoundary {
    /// Run `op` trapped at the module boundary owning `slot`. On panic or
    /// `Err`, the subtree is torn down and a typed [`Failure`] is delivered
    /// to the parent.
    ///
    /// Wave 10 implements the `Err` → `Failure` path. Panic trapping via
    /// `std::panic::catch_unwind` is deferred — it requires `UnwindSafe`
    /// bounds (or `unsafe` assertions) on `op` and `T`, which would widen
    /// the contract.
    fn trap<T>(
        &mut self,
        op: impl FnOnce() -> Result<T, AlkALiveError>,
        slot: SlotId,
    ) -> Result<T, Failure> {
        // TODO(later wave): wrap `op()` in `std::panic::catch_unwind` to also
        // trap panics. Requires `UnwindSafe` bounds on `op`/`T` (or `unsafe`
        // to assert them) — deferred from Wave 10 to avoid widening the
        // contract.
        match op() {
            Ok(v) => Ok(v),
            Err(e) => Err(Failure {
                slot,
                error: e,
                rect: DirtyRect::default(),
                span: SpanId(0),
            }),
        }
    }
    /// Report a [`Failure`] against the given [`DirtyRect`]; records a span
    /// on the unified trace (ADR 016).
    ///
    /// Wave 10 no-op: trace recording is wired once the trace store lands.
    fn report(&mut self, failure: Failure, rect: DirtyRect) {
        let _ = (failure, rect);
        // TODO: record the failure + rect as a span on the unified
        // author-owned trace (ADR 016).
    }
}

/// Single author-owned trace recorder (ADR 016).
///
/// There is no separate log sink: every error, recovery, and budget overrun
/// is a span on the unified timeline, correlated on a single timeline with
/// layout and draw. `watchFrame` is the frame-budget watchdog (§13.6).
pub trait TraceRecorder {
    /// Open a span of `kind` with `attrs`; returns its [`SpanId`].
    fn enter(&mut self, span: SpanKind, attrs: SpanAttrs) -> SpanId;
    /// Close `span` with `result`; an `Err` records the failure on the span.
    ///
    /// Wave 10 no-op: span closure is recorded once the trace store is wired.
    fn exit<T>(&mut self, span: SpanId, result: Result<T, AlkALiveError>) {
        let _ = (span, result);
        // TODO: record the span close + result on the unified trace (ADR 016).
    }
    /// Install a frame-budget watchdog for `budget_ms` milliseconds; returns
    /// the event surfaced when the frame closes (or breaches).
    ///
    /// Wave 10 returns a default event with zero elapsed time and no breach;
    /// real timing arrives with the monotonic-clock integration.
    fn watch_frame(&mut self, budget_ms: f32) -> FrameBudgetEvent {
        FrameBudgetEvent {
            span: SpanId(0),
            budget_ms,
            elapsed_ms: 0.0,
            breach: None,
        }
    }
}

/// Module-boundary isolator (ADR 007 / ADR 008).
///
/// Guarantees: no exception escapes a module boundary; dirty rect is
/// bounded to the failing module (ADR 002); a typed [`Failure`] is emitted
/// to the parent slot.
pub trait ModuleIsolator {
    /// Quarantine `module` to `rect`; further per-frame work skips the
    /// quarantined region until teardown.
    fn quarantine(&mut self, module: ModuleId, rect: DirtyRect);
    /// Tear down `module` deterministically; returns a [`TeardownReport`]
    /// describing what was reclaimed.
    ///
    /// Wave 10 default returns a report with a default dirty rect, no
    /// released slots, and a sentinel span. Concrete isolators may override
    /// to consult their quarantine table.
    fn teardown(&mut self, module: ModuleId) -> TeardownReport {
        TeardownReport {
            module,
            quarantined_rect: DirtyRect::default(),
            released_slots: Vec::new(),
            span: SpanId(0),
        }
    }
    /// Emit a typed [`Failure`] for `slot` from `err`; the parent receives
    /// the failure and the rest of the tree is unaffected.
    ///
    /// Wave 10 default returns a [`Failure`] with a default dirty rect and a
    /// sentinel span.
    fn emit_failure(&mut self, slot: SlotId, err: AlkALiveError) -> Failure {
        Failure {
            slot,
            error: err,
            rect: DirtyRect::default(),
            span: SpanId(0),
        }
    }
}

/// Enumerated recovery strategy for a category of [`AlkALiveError`]
/// (§13.4 / §13.5).
///
/// Wave 5 makes both `category` and `recover` required methods (no
/// `todo!()` defaults); concrete strategy implementors supply the bodies.
/// [`LastKnownGoodRecovery`] is provided as the first reference implementor
/// (retains the last-known-good layout / frame). The remaining strategies
/// from §13.4 (shader passthrough, font fallback, worker retry, full reload)
/// land alongside the recovery-strategy registry in a later wave.
pub trait RecoveryStrategy {
    /// The error category this strategy recovers.
    ///
    /// Returned as a placeholder variant of [`AlkALiveError`] identifying
    /// the strategy's category; concrete strategies pick the subtype they
    /// recover.
    fn category(&self) -> AlkALiveError;
    /// Recover from the failure described by `ctx`; returns the outcome.
    fn recover(&mut self, ctx: RecoveryContext) -> RecoveryOutcome;
}

// ============================================================================
// Concrete implementations (Wave 10)
// ============================================================================

/// Minimal [`ErrorBoundary`] implementor using the trait's default
/// `trap` / `report` behaviour.
///
/// Provided as a test seam and a reference for production implementors.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrappingBoundary;

impl ErrorBoundary for TrappingBoundary {}

/// Concrete [`TraceRecorder`] backed by an [`AtomicU64`] span-id counter.
///
/// The counter starts at `1` so that [`SpanId`]`(0)` remains a sentinel
/// "no span" value usable by default [`Failure`] / [`FrameBudgetEvent`]
/// constructors. `exit` and `watch_frame` inherit the trait's Wave 10
/// defaults.
#[derive(Debug)]
pub struct AuthorTraceRecorder {
    next_span_id: AtomicU64,
}

impl AuthorTraceRecorder {
    /// Create a new recorder with the span counter starting at `1`.
    pub fn new() -> Self {
        Self {
            next_span_id: AtomicU64::new(1),
        }
    }
}

impl Default for AuthorTraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceRecorder for AuthorTraceRecorder {
    fn enter(&mut self, _span: SpanKind, _attrs: SpanAttrs) -> SpanId {
        let id = self.next_span_id.fetch_add(1, Ordering::Relaxed);
        SpanId(id)
    }
}

/// Concrete [`ModuleIsolator`] stub that records quarantined modules in a
/// [`Vec`] and inherits the trait's default `teardown` / `emit_failure`
/// behaviour.
#[derive(Debug, Default)]
pub struct BoundaryIsolator {
    quarantined: Vec<(ModuleId, DirtyRect)>,
}

impl BoundaryIsolator {
    /// Create an empty isolator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of modules currently quarantined.
    pub fn quarantined_count(&self) -> usize {
        self.quarantined.len()
    }
}

impl ModuleIsolator for BoundaryIsolator {
    fn quarantine(&mut self, module: ModuleId, rect: DirtyRect) {
        self.quarantined.push((module, rect));
    }
}

/// Reference [`RecoveryStrategy`] that retains the last-known-good layout /
/// frame and emits a placeholder in the dirty rect (§13.4).
///
/// Provided as a test seam and a reference for production implementors.
/// `category` returns a generic [`AlkALiveError::Rendering`] placeholder
/// (a [`RenderError::DeviceLost`] with an empty detail string); `recover`
/// always returns [`RecoveryOutcome::RetainedLastKnownGood`].
#[derive(Debug, Default, Clone, Copy)]
pub struct LastKnownGoodRecovery;

impl RecoveryStrategy for LastKnownGoodRecovery {
    fn category(&self) -> AlkALiveError {
        AlkALiveError::Rendering(RenderError::DeviceLost(String::new()))
    }

    fn recover(&mut self, _ctx: RecoveryContext) -> RecoveryOutcome {
        RecoveryOutcome::RetainedLastKnownGood
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build minimal [`SpanAttrs`] for tests.
    fn test_attrs() -> SpanAttrs {
        SpanAttrs {
            frame_id: 0,
            stage: "test".to_string(),
            parent: None,
        }
    }

    // ---- ErrorBoundary::trap ---------------------------------------------

    #[test]
    fn trap_ok_returns_value() {
        let mut boundary = TrappingBoundary;
        let result: Result<i32, Failure> = boundary.trap(|| Ok(42), SlotId(1));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn trap_err_returns_failure_with_slot_and_defaults() {
        let mut boundary = TrappingBoundary;
        let slot = SlotId(7);
        let err = AlkALiveError::LayoutSolve(LayoutError::Infeasible("bad constraint".into()));
        let result: Result<i32, Failure> = boundary.trap(|| Err(err), slot);
        let failure = result.unwrap_err();
        assert_eq!(failure.slot, slot);
        assert_eq!(failure.rect, DirtyRect::default());
        assert_eq!(failure.span, SpanId(0));
        assert!(
            matches!(
                failure.error,
                AlkALiveError::LayoutSolve(LayoutError::Infeasible(_))
            ),
            "expected LayoutSolve(Infeasible(_)), got {:?}",
            failure.error,
        );
    }

    // ---- ErrorBoundary::report (no-op) -----------------------------------

    #[test]
    fn report_is_noop() {
        let mut boundary = TrappingBoundary;
        let failure = Failure {
            slot: SlotId(1),
            error: AlkALiveError::Rendering(RenderError::DeviceLost("gpu".into())),
            rect: DirtyRect::default(),
            span: SpanId(0),
        };
        // Should not panic.
        boundary.report(failure, DirtyRect::default());
    }

    // ---- TraceRecorder::enter --------------------------------------------

    #[test]
    fn trace_recorder_enter_returns_incrementing_span_ids() {
        let mut recorder = AuthorTraceRecorder::new();
        let id1 = recorder.enter(SpanKind::Logic, test_attrs());
        let id2 = recorder.enter(SpanKind::Layout, test_attrs());
        let id3 = recorder.enter(SpanKind::Draw, test_attrs());
        // Counter starts at 1; SpanId(0) is the sentinel.
        assert_eq!(id1, SpanId(1));
        assert_eq!(id2, SpanId(2));
        assert_eq!(id3, SpanId(3));
    }

    // ---- TraceRecorder::watch_frame --------------------------------------

    #[test]
    fn trace_recorder_watch_frame_returns_defaults() {
        let mut recorder = AuthorTraceRecorder::new();
        let event = recorder.watch_frame(16.7);
        assert_eq!(event.span, SpanId(0));
        assert_eq!(event.budget_ms, 16.7);
        assert_eq!(event.elapsed_ms, 0.0);
        assert!(event.breach.is_none());
    }

    // ---- TraceRecorder::exit (no-op) -------------------------------------

    #[test]
    fn trace_recorder_exit_is_noop() {
        let mut recorder = AuthorTraceRecorder::new();
        let id = recorder.enter(SpanKind::Logic, test_attrs());
        // Should not panic on Ok.
        recorder.exit(id, Ok::<i32, AlkALiveError>(99));
        // Should not panic on Err.
        recorder.exit(
            id,
            Err::<i32, _>(AlkALiveError::TextShaping(TextError::MissingGlyph(
                "x".into(),
            ))),
        );
    }

    // ---- ModuleIsolator stubs --------------------------------------------

    #[test]
    fn isolator_quarantine_stores_module() {
        let mut iso = BoundaryIsolator::new();
        assert_eq!(iso.quarantined_count(), 0);
        iso.quarantine(ModuleId(1), DirtyRect::default());
        iso.quarantine(
            ModuleId(2),
            DirtyRect {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            },
        );
        assert_eq!(iso.quarantined_count(), 2);
    }

    #[test]
    fn isolator_emit_failure_returns_defaults() {
        let mut iso = BoundaryIsolator::new();
        let failure = iso.emit_failure(
            SlotId(9),
            AlkALiveError::Rendering(RenderError::DeviceLost("gpu".into())),
        );
        assert_eq!(failure.slot, SlotId(9));
        assert_eq!(failure.rect, DirtyRect::default());
        assert_eq!(failure.span, SpanId(0));
        assert!(
            matches!(
                failure.error,
                AlkALiveError::Rendering(RenderError::DeviceLost(_))
            ),
            "expected Rendering(DeviceLost(_)), got {:?}",
            failure.error,
        );
    }

    #[test]
    fn isolator_teardown_returns_defaults() {
        let mut iso = BoundaryIsolator::new();
        let module = ModuleId(42);
        let report = iso.teardown(module);
        assert_eq!(report.module, module);
        assert_eq!(report.quarantined_rect, DirtyRect::default());
        assert!(report.released_slots.is_empty());
        assert_eq!(report.span, SpanId(0));
    }

    // ---- RecoveryStrategy::LastKnownGoodRecovery -------------------------

    #[test]
    fn last_known_good_recovery_category_is_rendering_device_lost() {
        let strategy = LastKnownGoodRecovery;
        let cat = strategy.category();
        match cat {
            AlkALiveError::Rendering(RenderError::DeviceLost(detail)) => {
                assert!(detail.is_empty(), "expected empty detail, got {detail:?}");
            }
            other => panic!("expected Rendering(DeviceLost(_)), got {other:?}"),
        }
    }

    #[test]
    fn last_known_good_recovery_recover_retains_last_known_good() {
        let mut strategy = LastKnownGoodRecovery;
        let ctx = RecoveryContext {
            error: AlkALiveError::Rendering(RenderError::DeviceLost("gpu".into())),
            slot: SlotId(1),
            rect: DirtyRect::default(),
            span: SpanId(0),
        };
        let outcome = strategy.recover(ctx);
        assert!(
            matches!(outcome, RecoveryOutcome::RetainedLastKnownGood),
            "expected RetainedLastKnownGood, got {outcome:?}",
        );
    }
}

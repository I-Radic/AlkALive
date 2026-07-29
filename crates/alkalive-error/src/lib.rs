//! AlkALive alkalive-error crate.
//!
//! Error handling & resilience trait surface — see
//! `docs/SPECIFICATION.md` §13 (Error Handling & Resilience).
//! Realises ADR 016 (unified author-owned trace), ADR 007/008 (module
//! isolation at boundaries), and ADR 002 (dirty-rect bounded invalidation).
//!
//! Wave 3 skeleton: signatures only; every body is `todo!()`.
//! No cross-crate dependencies; types referenced from other sections
//! (e.g. `FrameBudgetEvent`, `ModuleId`) are local placeholders.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

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
/// Local to this crate (the perf crate defines its own [`crate::SpanKind`]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(());

/// Attributes attached to a span on enter.
#[derive(Debug, Clone)]
pub struct SpanAttrs {
    /// Frame this span belongs to.
    pub frame_id: u64,
    /// Stage this span attributes to (free-form string; the perf crate
    /// holds the typed [`crate::StageId`] mirror).
    pub stage: String,
    /// Optional parent span.
    pub parent: Option<SpanId>,
}

/// Per-module, per-object invalidation subset bounding per-frame work to
/// the changed region rather than the full tree (ADR 002).
///
/// Local placeholder; the render crate will supply the canonical definition
/// once cross-crate wiring lands.
#[derive(Debug, Clone, Copy)]
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
pub struct SlotId(());

/// Name of a slot — the string key under which a child is mounted.
#[derive(Debug, Clone)]
pub struct SlotName(());

/// Identifier of a module on the render-object tree (ADR 007).
///
/// Local placeholder; the core crate will supply the canonical definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(());

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
/// independence (no cross-crate dependency in Wave 3).
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
    fn trap<T>(
        &mut self,
        op: impl FnOnce() -> Result<T, AlkALiveError>,
        slot: SlotId,
    ) -> Result<T, Failure>;
    /// Report a [`Failure`] against the given [`DirtyRect`]; records a span
    /// on the unified trace (ADR 016).
    fn report(&mut self, failure: Failure, rect: DirtyRect);
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
    fn exit<T>(&mut self, span: SpanId, result: Result<T, AlkALiveError>);
    /// Install a frame-budget watchdog for `budget_ms` milliseconds; returns
    /// the event surfaced when the frame closes (or breaches).
    fn watch_frame(&mut self, budget_ms: f32) -> FrameBudgetEvent;
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
    fn teardown(&mut self, module: ModuleId) -> TeardownReport;
    /// Emit a typed [`Failure`] for `slot` from `err`; the parent receives
    /// the failure and the rest of the tree is unaffected.
    fn emit_failure(&mut self, slot: SlotId, err: AlkALiveError) -> Failure;
}

/// Enumerated recovery strategy for a category of [`AlkALiveError`]
/// (§13.4 / §13.5).
pub trait RecoveryStrategy {
    /// The error category this strategy recovers.
    fn category(&self) -> AlkALiveError;
    /// Recover from the failure described by `ctx`; returns the outcome.
    fn recover(&mut self, ctx: RecoveryContext) -> RecoveryOutcome;
}

//! AlkALive alkalive-test crate.
//!
//! Testing & simulation trait surface — see `docs/SPECIFICATION.md` §14
//! (Testing & Simulation). Realises ADR 014 (typed component contracts)
//! and ADR 016 (split determinism — WASM-sandboxed layout +
//! software-rasteriser fallback).
//!
//! Wave 11: concrete implementations of [`MockBackend`] ([`MockBackendImpl`]),
//! [`MockTextStack`] ([`MockTextStackImpl`]), [`SoftwareBackend`]
//! ([`SoftwareBackendImpl`]), and [`TestHarness`] ([`SimpleTestHarness`]) land
//! here as self-contained, GPU-free stubs. [`ComponentTest`] and
//! [`TracePlayer`] remain trait skeletons awaiting cross-crate runtime
//! integration (ADR 007 owned state + ADR 016 unified trace).
//!
//! The test surface is contract-shaped, not selector-shaped: no DOM, no
//! headless browser, no GPU required. No cross-crate dependencies; types
//! referenced from other sections (`Backend`, `TextStack`, `RenderGraphIR`,
//! `DrawCall`, `ShapedRun`, `Scene`, `ModuleId`, `InputEvent`,
//! `SerialisableSceneGraph`, `VendorId`, `SpanId`) are local placeholders.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt;

// ============================================================================
// Local placeholders for cross-crate types
// ============================================================================

/// Local placeholder for the render crate's `RenderGraphIR`.
///
/// The atomic rendering primitive (ADR 001): a directed graph of passes,
/// attachments, draw calls, and an occlusion-cull pass, submitted
/// immutably to the compositor.
#[derive(Debug, Clone)]
pub struct RenderGraphIR(());

/// Local placeholder for the render crate's `DrawCall`.
#[derive(Debug, Clone)]
pub struct DrawCall(());

/// Local placeholder for the text crate's `ShapedRun`.
#[derive(Debug, Clone)]
pub struct ShapedRun(());

/// Local placeholder for a serialisable scene graph (ADR 007 owned state).
#[derive(Debug, Clone)]
pub struct SerialisableSceneGraph(());

/// Local placeholder for the runtime's `Scene` handle.
#[derive(Debug, Clone)]
pub struct Scene(());

/// Local placeholder for the core crate's `ModuleId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(());

/// Local placeholder for the input crate's `InputEvent`.
#[derive(Debug, Clone)]
pub struct InputEvent(());

/// Local placeholder for the GPU vendor identifier used by [`RasterClass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VendorId(());

/// Local placeholder for the error crate's `SpanId`.
///
/// Used by [`SnapshotError::TraceGap`] to point at the missing span that
/// breaks replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(());

// ============================================================================
// Enums
// ============================================================================

/// Result of a single test assertion (§14.3).
#[derive(Debug)]
pub enum TestResult {
    /// Assertion passed.
    Pass,
    /// Assertion failed: contract mismatch, panic, or frame diff.
    Fail(FailureReport),
    /// Determinism precondition violated; the test could not run.
    Inconclusive(SnapshotError),
}

/// Error raised when a [`SceneSnapshot`] cannot be created or replayed
/// (§14.3).
#[derive(Debug)]
pub enum SnapshotError {
    /// Module lacks ADR 007 owned state; cannot be serialised.
    StateNotSerialisable,
    /// Missing span breaks replay; carries the absent [`SpanId`].
    TraceGap(SpanId),
    /// Software vs GPU parity not asserted (raster class differs).
    RasterClassMismatch,
    /// Snapshot id collides with an existing entry.
    FingerprintCollision,
}

/// Raster class bounding the determinism guarantee of a [`SceneSnapshot`]
/// (§14.2). Cross-vendor pixel-identical parity is *not* claimed — the
/// software fallback bounds parity to one raster class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterClass {
    /// Deterministic software rasteriser (CI path of record).
    Software,
    /// GPU path tagged with the vendor it was recorded on.
    Gpu(VendorId),
}

/// Result of a single [`TracePlayer::step`] advance (§14.3).
#[derive(Debug)]
pub enum StepResult {
    /// Advanced one tick; frame available.
    Advanced,
    /// End of trace reached; no more ticks.
    EndOfTrace,
    /// Replay could not continue; carries the underlying snapshot error.
    Gap(SnapshotError),
}

/// Error raised by [`TracePlayer::load`] when a trace cannot be loaded
/// (§14.3).
#[derive(Debug)]
pub enum TraceError {
    /// Trace bytes malformed / unparseable.
    Malformed,
    /// A required span is missing.
    MissingSpan(SpanId),
    /// Trace version mismatch with the player.
    VersionMismatch,
}

// ============================================================================
// Structs
// ============================================================================

/// Identifier of a [`SceneSnapshot`]; keys the snapshot cache so the same
/// scene never re-rasterises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(());

/// Identifier of a single tick on the unified trace (ADR 016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TickId(());

/// Fixture table seeding a [`MockTextStack`]; maps input text to a
/// deterministic [`ShapedRun`].
#[derive(Debug, Clone)]
pub struct TextFixtureTable(());

/// Typed properties handed to a component on mount (ADR 014).
#[derive(Debug, Clone)]
pub struct TypedProps(());

/// Map of named child slots handed to a component on mount (§2.6 / ADR 014).
#[derive(Debug, Clone)]
pub struct SlotMap(());

/// Handle returned by [`ComponentTest::mount`]; drives the mounted
/// component and is consumed by [`ComponentTest::teardown`].
#[derive(Debug, Clone)]
pub struct ActiveHandle(());

/// Typed output event emitted by a driven component (ADR 014).
#[derive(Debug, Clone)]
pub struct OutputEvent(());

/// Typed value read from a named child slot (ADR 014).
#[derive(Debug, Clone)]
pub struct SlotValue(());

/// A single rendered frame produced by [`TestHarness::tick`].
#[derive(Debug, Clone)]
pub struct Frame(());

/// An ADR 016 unified trace, loadable into a [`TracePlayer`] for replay.
#[derive(Debug, Clone)]
pub struct UnifiedTrace(());

/// Report carried by [`TestResult::Fail`] describing the contract mismatch,
/// panic, or frame diff.
#[derive(Debug, Clone)]
pub struct FailureReport {
    /// Human-readable summary of the failure.
    pub summary: String,
    /// Tick on which the failure occurred, if replay-driven.
    pub tick: Option<TickId>,
    /// Snapshot that triggered the failure, if applicable.
    pub snapshot: Option<SnapshotId>,
}

/// Immutable replay unit (§14.3 / §14.4). Its `fingerprint` keys the cache
/// so the same scene never re-rasterises.
#[derive(Debug, Clone)]
pub struct SceneSnapshot {
    /// Snapshot identifier / cache key.
    pub id: SnapshotId,
    /// ADR 007 owned state.
    pub scene_graph: SerialisableSceneGraph,
    /// Input events to drive the scene with.
    pub inputs: Vec<InputEvent>,
    /// Mock text fixtures seeding [`MockTextStack`].
    pub text_fixtures: TextFixtureTable,
    /// Raster class bounding the determinism guarantee.
    pub raster_class: RasterClass,
    /// Hash of `(graph, inputs, fixtures)`; collision-free cache key.
    pub fingerprint: u64,
}

// ============================================================================
// Traits (§14.3)
// ============================================================================

/// Mockable GPU backend (§14.1). Records draw calls into a typed log; headless
/// tests never instantiate a `GPUDevice`.
///
/// Wave 3 skeleton: the spec supertrait `Backend` (§4.1) is omitted to keep
/// this crate independent; it will be wired when cross-crate deps land.
pub trait MockBackend: fmt::Debug {
    /// Record a render-graph submission into the draw log.
    fn record_submit(&mut self, ir: &RenderGraphIR);
    /// Read the recorded draw-call log.
    fn draw_log(&self) -> &[DrawCall];
    /// Assert that the recorded log contains exactly `expected` passes.
    fn assert_pass_count(&self, expected: usize) -> TestResult;
}

/// Mockable text stack (§14.1). Returns deterministic [`ShapedRun`]s from a
/// fixture table, removing HarfRust / font-fallback variance from layout
/// tests.
///
/// Wave 3 skeleton: the spec supertrait `TextStack` (§6.9) is omitted to
/// keep this crate independent; it will be wired when cross-crate deps land.
pub trait MockTextStack: fmt::Debug {
    /// Install a fixture mapping `text` to a deterministic `run`.
    fn install_fixture(&mut self, text: &str, run: ShapedRun);
    /// Read all shaped runs produced so far.
    fn shaped_runs(&self) -> &[ShapedRun];
}

/// Deterministic software rasteriser (ADR 016 split-determinism fallback).
/// Runs in CI; given the same scene graph + inputs + mock text fixtures,
/// two frames are byte-identical within a raster class.
///
/// Wave 3 skeleton: the spec supertrait `Backend` (§4.1) is omitted to
/// keep this crate independent; it will be wired when cross-crate deps land.
pub trait SoftwareBackend: fmt::Debug {
    /// Rasterise `ir` into a [`Frame`].
    fn rasterize(&mut self, ir: &RenderGraphIR) -> Frame;
    /// Raster class this backend produces.
    fn raster_class(&self) -> RasterClass;
}

/// Typed component test surface (ADR 014). Replaces DOM-selector e2e
/// assertions: a test mounts a module with typed props and a slot map,
/// drives it with typed [`InputEvent`]s, and asserts on typed
/// [`OutputEvent`]s and [`SlotValue`]s.
///
/// Wave 11: this trait remains a skeleton. Its methods require the
/// cross-crate runtime (ADR 007 owned state, ADR 014 typed component
/// registry) to implement; bodies are intentionally unimplemented here.
pub trait ComponentTest {
    /// Mount `module` with `props` and `slots`; returns an [`ActiveHandle`].
    fn mount(&mut self, module: ModuleId, props: TypedProps, slots: SlotMap) -> ActiveHandle;
    /// Drive `handle` with `input`; returns the typed outputs emitted.
    fn drive(&mut self, handle: &ActiveHandle, input: InputEvent) -> Vec<OutputEvent>;
    /// Read the typed value of the named child `slot`.
    fn slot_output(&self, handle: &ActiveHandle, slot: &str) -> SlotValue;
    /// Assert that `expected` was emitted by `handle`.
    fn expect_output(&self, handle: &ActiveHandle, expected: OutputEvent) -> TestResult;
    /// Tear down `handle`; releases the mounted component.
    fn teardown(&mut self, handle: ActiveHandle);
}

/// Replays an ADR 016 unified trace tick-by-tick (§14.3 / §14.4). Replayed
/// frames must match recorded frames byte-for-byte, else the harness returns
/// [`SnapshotError::TraceGap`] wrapped in [`StepResult::Gap`] or
/// [`TestResult::Inconclusive`].
///
/// Wave 11: this trait remains a skeleton. Its methods require the
/// cross-crate unified-trace store (ADR 016) to implement; bodies are
/// intentionally unimplemented here.
pub trait TracePlayer {
    /// Load `trace` into the player.
    fn load(&mut self, trace: &UnifiedTrace) -> Result<(), TraceError>;
    /// Advance one tick; returns the step result.
    fn step(&mut self) -> StepResult;
    /// Seek to `tick` for targeted replay.
    fn seek(&mut self, tick: TickId);
    /// Assert that replayed frames are byte-identical to recorded frames.
    fn assert_replay(self, harness: &dyn TestHarness) -> TestResult;
}

/// Integration entry point (§14.4). Wires a [`MockBackend`],
/// [`MockTextStack`], and [`SoftwareBackend`] together so a single tick
/// produces a deterministic [`Frame`] without touching the GPU:
/// `snapshot → tick → assert_frame`.
pub trait TestHarness {
    /// Composed mock backend.
    fn backend(&self) -> &dyn MockBackend;
    /// Composed mock text stack.
    fn text(&self) -> &dyn MockTextStack;
    /// Composed software rasteriser.
    fn raster(&self) -> &dyn SoftwareBackend;
    /// Snapshot `scene` into an immutable replay unit.
    fn snapshot(&self, scene: &Scene) -> SceneSnapshot;
    /// Tick `snap` once via the [`SoftwareBackend`]; returns the frame.
    fn tick(&self, snap: &SceneSnapshot) -> Frame;
    /// Assert that ticking `snap` produces `expected`.
    fn assert_frame(&self, snap: &SceneSnapshot, expected: &Frame) -> TestResult;
    /// Replay `trace` tick-by-tick; returns the aggregated result.
    fn replay(&self, trace: &UnifiedTrace) -> TestResult;
}

// ============================================================================
// Concrete implementations (Wave 11)
// ============================================================================

/// Concrete [`MockBackend`] recording every submitted render graph into an
/// in-memory log.
///
/// The [`RenderGraphIR`] is opaque in this self-contained crate, so each
/// [`MockBackend::record_submit`] appends one placeholder [`DrawCall`] to
/// the draw log (standing in for "one pass") and the submitted IR itself to
/// `submitted_irs`. [`MockBackend::assert_pass_count`] then compares the
/// number of recorded submissions against the expected pass count.
#[derive(Debug, Clone, Default)]
pub struct MockBackendImpl {
    /// Recorded draw-call placeholders (one per submit).
    draw_log: Vec<DrawCall>,
    /// Recorded render-graph submissions.
    submitted_irs: Vec<RenderGraphIR>,
}

impl MockBackendImpl {
    /// Construct an empty mock backend.
    pub fn new() -> Self {
        Self::default()
    }
}

impl MockBackend for MockBackendImpl {
    fn record_submit(&mut self, ir: &RenderGraphIR) {
        // The IR is opaque in this crate; record the submission and push a
        // single placeholder DrawCall standing in for the pass count.
        self.submitted_irs.push(ir.clone());
        self.draw_log.push(DrawCall(()));
    }

    fn draw_log(&self) -> &[DrawCall] {
        &self.draw_log
    }

    fn assert_pass_count(&self, expected: usize) -> TestResult {
        let actual = self.submitted_irs.len();
        if actual == expected {
            TestResult::Pass
        } else {
            TestResult::Fail(FailureReport {
                summary: format!("expected {} got {}", expected, actual),
                tick: None,
                snapshot: None,
            })
        }
    }
}

/// Concrete [`MockTextStack`] backed by an in-memory fixture table.
///
/// [`MockTextStack::install_fixture`] appends to `fixtures`; produced runs
/// accumulate in `shaped_runs` for later inspection via
/// [`MockTextStack::shaped_runs`].
#[derive(Debug, Clone, Default)]
pub struct MockTextStackImpl {
    /// Installed `(text, run)` fixtures.
    fixtures: Vec<(String, ShapedRun)>,
    /// Shaped runs produced so far.
    shaped_runs: Vec<ShapedRun>,
}

impl MockTextStackImpl {
    /// Construct an empty mock text stack.
    pub fn new() -> Self {
        Self::default()
    }
}

impl MockTextStack for MockTextStackImpl {
    fn install_fixture(&mut self, text: &str, run: ShapedRun) {
        self.fixtures.push((text.to_owned(), run));
    }

    fn shaped_runs(&self) -> &[ShapedRun] {
        &self.shaped_runs
    }
}

/// Concrete [`SoftwareBackend`] — a no-op software rasteriser stub.
///
/// [`SoftwareBackend::rasterize`] returns an empty [`Frame`] and
/// [`SoftwareBackend::raster_class`] reports [`RasterClass::Software`],
/// matching the CI determinism path of record (ADR 016). A real
/// byte-identical software rasteriser will replace this stub when the
/// render crate lands.
#[derive(Debug, Clone, Default)]
pub struct SoftwareBackendImpl;

impl SoftwareBackendImpl {
    /// Construct a software backend stub.
    pub fn new() -> Self {
        Self
    }
}

impl SoftwareBackend for SoftwareBackendImpl {
    fn rasterize(&mut self, _ir: &RenderGraphIR) -> Frame {
        // No-op stub: real software rasterisation lands with the render crate.
        Frame(())
    }

    fn raster_class(&self) -> RasterClass {
        RasterClass::Software
    }
}

/// Concrete [`TestHarness`] wiring a [`MockBackendImpl`], [`MockTextStackImpl`],
/// and [`SoftwareBackendImpl`] together so a single tick produces a
/// deterministic [`Frame`] without touching the GPU (§14.4).
///
/// All harness operations are stubs: [`TestHarness::snapshot`] returns a
/// fixed [`SceneSnapshot`], [`TestHarness::tick`] returns an empty [`Frame`],
/// and [`TestHarness::assert_frame`] / [`TestHarness::replay`] report
/// [`TestResult::Pass`]. The composed components are accessible via
/// [`TestHarness::backend`], [`TestHarness::text`], and [`TestHarness::raster`].
#[derive(Debug, Clone, Default)]
pub struct SimpleTestHarness {
    /// Composed mock backend.
    backend: MockBackendImpl,
    /// Composed mock text stack.
    text: MockTextStackImpl,
    /// Composed software rasteriser.
    raster: SoftwareBackendImpl,
}

impl SimpleTestHarness {
    /// Construct a harness with fresh mock/software components.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TestHarness for SimpleTestHarness {
    fn backend(&self) -> &dyn MockBackend {
        &self.backend
    }

    fn text(&self) -> &dyn MockTextStack {
        &self.text
    }

    fn raster(&self) -> &dyn SoftwareBackend {
        &self.raster
    }

    fn snapshot(&self, _scene: &Scene) -> SceneSnapshot {
        // Stub snapshot: real serialisation requires ADR 007 owned state.
        SceneSnapshot {
            id: SnapshotId(()),
            scene_graph: SerialisableSceneGraph(()),
            inputs: vec![],
            text_fixtures: TextFixtureTable(()),
            raster_class: RasterClass::Software,
            fingerprint: 0,
        }
    }

    fn tick(&self, _snap: &SceneSnapshot) -> Frame {
        // Stub tick: real ticking drives the SoftwareBackend against the snap.
        Frame(())
    }

    fn assert_frame(&self, _snap: &SceneSnapshot, _expected: &Frame) -> TestResult {
        // Stub assertion: real frame diff lands with the render crate.
        TestResult::Pass
    }

    fn replay(&self, _trace: &UnifiedTrace) -> TestResult {
        // Stub replay: real replay drives a TracePlayer across the harness.
        TestResult::Pass
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_backend_record_submit_and_assert_pass_count_pass() {
        let mut backend = MockBackendImpl::new();
        assert_eq!(backend.draw_log().len(), 0);

        backend.record_submit(&RenderGraphIR(()));
        backend.record_submit(&RenderGraphIR(()));

        // One placeholder DrawCall is appended per submit.
        assert_eq!(backend.draw_log().len(), 2);
        assert!(matches!(backend.assert_pass_count(2), TestResult::Pass));
    }

    #[test]
    fn mock_backend_assert_pass_count_fail() {
        let mut backend = MockBackendImpl::new();
        backend.record_submit(&RenderGraphIR(()));

        match backend.assert_pass_count(2) {
            TestResult::Fail(report) => {
                assert_eq!(report.summary, "expected 2 got 1");
                assert!(report.tick.is_none());
                assert!(report.snapshot.is_none());
            }
            other => panic!("expected TestResult::Fail, got {:?}", other),
        }
    }

    #[test]
    fn mock_text_stack_install_fixture_and_shaped_runs() {
        let mut text = MockTextStackImpl::new();
        assert!(text.shaped_runs().is_empty());

        text.install_fixture("hello", ShapedRun(()));
        text.install_fixture("world", ShapedRun(()));

        // Fixtures accumulate; shaped_runs are untouched by install_fixture.
        assert_eq!(text.fixtures.len(), 2);
        assert_eq!(text.fixtures[0].0, "hello");
        assert_eq!(text.fixtures[1].0, "world");
        assert!(text.shaped_runs().is_empty());
    }

    #[test]
    fn software_backend_raster_class_is_software() {
        let mut raster = SoftwareBackendImpl::new();
        assert_eq!(raster.raster_class(), RasterClass::Software);
        // rasterize is a no-op stub but must not panic and must yield a Frame.
        let _frame: Frame = raster.rasterize(&RenderGraphIR(()));
    }

    #[test]
    fn simple_test_harness_snapshot_returns_valid_snapshot() {
        let harness = SimpleTestHarness::new();
        let snap = harness.snapshot(&Scene(()));
        assert_eq!(snap.raster_class, RasterClass::Software);
        assert_eq!(snap.fingerprint, 0);
        assert!(snap.inputs.is_empty());
    }

    #[test]
    fn simple_test_harness_tick_returns_frame() {
        let harness = SimpleTestHarness::new();
        let snap = harness.snapshot(&Scene(()));
        let _frame = harness.tick(&snap);
        // assert_frame and replay are no-op passes in the stub.
        assert!(matches!(
            harness.assert_frame(&snap, &Frame(())),
            TestResult::Pass
        ));
        assert!(matches!(
            harness.replay(&UnifiedTrace(())),
            TestResult::Pass
        ));
    }

    #[test]
    fn simple_test_harness_accessors_return_composed_components() {
        let harness = SimpleTestHarness::new();
        // Trait-object accessors route to the composed mock/software impls.
        assert!(harness.backend().draw_log().is_empty());
        assert!(harness.text().shaped_runs().is_empty());
        assert_eq!(harness.raster().raster_class(), RasterClass::Software);
    }
}

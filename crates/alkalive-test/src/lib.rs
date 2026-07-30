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

pub use alkalive_core::ModuleId;

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
///
/// Derives [`PartialEq`] / [`Eq`] so that [`ComponentTest::expect_output`]
/// implementations can membership-test emitted events against an expected
/// value. The inner payload remains opaque in this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Concrete [`ComponentTest`] backed by an in-memory fixture registry
/// (Gap H13 / ADR 014).
///
/// Stores `Vec<(ModuleId, Vec<OutputEvent>)>` mapping each registered
/// module to the fixture outputs [`ComponentTest::drive`] should emit.
/// [`ComponentTest::mount`] records the module as the currently mounted
/// one and ensures it has a fixture entry; [`ComponentTest::drive`]
/// looks up the current module's fixture outputs, appends them to an
/// `emitted` log, and returns them; [`ComponentTest::slot_output`]
/// returns a placeholder [`SlotValue`]; [`ComponentTest::expect_output`]
/// membership-tests the `emitted` log against the expected
/// [`OutputEvent`]; [`ComponentTest::teardown`] removes the module from
/// the fixture registry.
///
/// All cross-crate types ([`TypedProps`], [`SlotMap`], [`ActiveHandle`],
/// [`InputEvent`], [`OutputEvent`], [`SlotValue`]) are opaque
/// placeholders in this crate, so props / slots / inputs are accepted
/// and ignored. The handle returned by `mount` is a placeholder
/// [`ActiveHandle`]`(())`; `drive` / `expect_output` / `slot_output`
/// operate against the most recently mounted module.
#[derive(Debug, Clone, Default)]
pub struct SimpleComponentTest {
    /// Fixture registry: module → fixture outputs to emit on `drive`.
    fixtures: Vec<(ModuleId, Vec<OutputEvent>)>,
    /// Currently mounted module (set by `mount`, cleared by `teardown`).
    current_module: Option<ModuleId>,
    /// Outputs emitted by the current module since `mount`.
    emitted: Vec<OutputEvent>,
}

impl SimpleComponentTest {
    /// Construct an empty `SimpleComponentTest` (no fixtures registered).
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a `SimpleComponentTest` pre-seeded with `fixtures`.
    ///
    /// Tests supply `(ModuleId, Vec<OutputEvent>)` pairs here so that
    /// [`ComponentTest::drive`] on the corresponding module returns the
    /// fixture outputs and [`ComponentTest::expect_output`] can match
    /// against them.
    pub fn with_fixtures(fixtures: Vec<(ModuleId, Vec<OutputEvent>)>) -> Self {
        Self {
            fixtures,
            current_module: None,
            emitted: Vec::new(),
        }
    }

    /// Number of modules currently registered in the fixture registry.
    pub fn registered_count(&self) -> usize {
        self.fixtures.len()
    }

    /// Number of outputs emitted by the currently mounted module since
    /// `mount` (i.e. the size of the log scanned by `expect_output`).
    pub fn emitted_count(&self) -> usize {
        self.emitted.len()
    }
}

impl ComponentTest for SimpleComponentTest {
    fn mount(&mut self, module: ModuleId, _props: TypedProps, _slots: SlotMap) -> ActiveHandle {
        self.current_module = Some(module);
        self.emitted.clear();
        // Ensure the module has a fixture entry (insert empty if absent)
        // so `drive` has a record to look up and `teardown` has a record
        // to remove.
        if !self.fixtures.iter().any(|(m, _)| *m == module) {
            self.fixtures.push((module, Vec::new()));
        }
        ActiveHandle(())
    }

    fn drive(&mut self, _handle: &ActiveHandle, _input: InputEvent) -> Vec<OutputEvent> {
        if let Some(module) = self.current_module {
            if let Some((_, outputs)) = self.fixtures.iter_mut().find(|(m, _)| *m == module) {
                self.emitted.extend_from_slice(outputs);
                return outputs.clone();
            }
        }
        Vec::new()
    }

    fn slot_output(&self, _handle: &ActiveHandle, _slot: &str) -> SlotValue {
        SlotValue(())
    }

    fn expect_output(&self, _handle: &ActiveHandle, expected: OutputEvent) -> TestResult {
        if self.emitted.contains(&expected) {
            TestResult::Pass
        } else {
            TestResult::Fail(FailureReport {
                summary: "expected output was not emitted".to_string(),
                tick: None,
                snapshot: None,
            })
        }
    }

    fn teardown(&mut self, _handle: ActiveHandle) {
        if let Some(module) = self.current_module.take() {
            self.fixtures.retain(|(m, _)| *m != module);
        }
        self.emitted.clear();
    }
}

/// Concrete [`TracePlayer`] backed by an in-memory `Vec<Frame>` tick
/// buffer (Gap H14 / §14.3 / §14.4).
///
/// [`TracePlayer::load`] stores an empty tick buffer — [`UnifiedTrace`]
/// is opaque in this crate, so the stub cannot enumerate recorded ticks
/// from it. Tests pre-populate the tick buffer via
/// [`SimpleTracePlayer::with_ticks`] to exercise [`TracePlayer::step`].
/// [`TracePlayer::seek`] is a no-op because [`TickId`] is opaque in this
/// crate; [`TracePlayer::assert_replay`] is a stub that always returns
/// [`TestResult::Pass`].
#[derive(Debug, Clone, Default)]
pub struct SimpleTracePlayer {
    /// Tick buffer of recorded frames.
    ticks: Vec<Frame>,
    /// Current tick index (number of ticks already replayed).
    current_tick: usize,
}

impl SimpleTracePlayer {
    /// Construct an empty `SimpleTracePlayer` (no ticks loaded).
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a `SimpleTracePlayer` pre-populated with `ticks`.
    ///
    /// Tests supply frames here so that [`TracePlayer::step`] can return
    /// [`StepResult::Advanced`] across the buffer until exhausted.
    pub fn with_ticks(ticks: Vec<Frame>) -> Self {
        Self {
            ticks,
            current_tick: 0,
        }
    }

    /// Current tick index (number of ticks already replayed).
    pub fn current_tick(&self) -> usize {
        self.current_tick
    }

    /// Total number of ticks in the loaded buffer.
    pub fn tick_count(&self) -> usize {
        self.ticks.len()
    }
}

impl TracePlayer for SimpleTracePlayer {
    fn load(&mut self, _trace: &UnifiedTrace) -> Result<(), TraceError> {
        // UnifiedTrace is opaque; store an empty Vec as a placeholder so
        // `step` immediately reports `EndOfTrace` until a real unified-
        // trace store lands.
        self.ticks = Vec::new();
        self.current_tick = 0;
        Ok(())
    }

    fn step(&mut self) -> StepResult {
        if self.current_tick < self.ticks.len() {
            self.current_tick += 1;
            if self.current_tick < self.ticks.len() {
                StepResult::Advanced
            } else {
                StepResult::EndOfTrace
            }
        } else {
            StepResult::EndOfTrace
        }
    }

    fn seek(&mut self, _tick: TickId) {
        // TickId is opaque in this crate; the stub leaves `current_tick`
        // unchanged. A real implementation will derive the tick index
        // from the TickId once the unified-trace store lands.
    }

    fn assert_replay(self, _harness: &dyn TestHarness) -> TestResult {
        // Stub: real byte-identical frame diff lands with the render crate.
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

    // ---- SimpleComponentTest (Gap H13) -----------------------------------

    #[test]
    fn simple_component_test_default_is_empty() {
        let t = SimpleComponentTest::new();
        assert_eq!(t.registered_count(), 0);
        assert_eq!(t.emitted_count(), 0);
    }

    #[test]
    fn simple_component_test_with_fixtures_seeds_registry() {
        let module = ModuleId(7);
        let t = SimpleComponentTest::with_fixtures(vec![(module, vec![OutputEvent(())])]);
        assert_eq!(t.registered_count(), 1);
    }

    #[test]
    fn simple_component_test_mount_registers_module_and_returns_handle() {
        let mut t = SimpleComponentTest::new();
        assert_eq!(t.registered_count(), 0);
        let _handle = t.mount(ModuleId(1), TypedProps(()), SlotMap(()));
        // mount registers the module in the fixture registry.
        assert_eq!(t.registered_count(), 1);
        assert_eq!(t.emitted_count(), 0);
    }

    #[test]
    fn simple_component_test_mount_idempotent_for_same_module() {
        let mut t = SimpleComponentTest::new();
        let module = ModuleId(1);
        let _h1 = t.mount(module, TypedProps(()), SlotMap(()));
        let _h2 = t.mount(module, TypedProps(()), SlotMap(()));
        // The second mount of the same module must not double-register.
        assert_eq!(t.registered_count(), 1);
    }

    #[test]
    fn simple_component_test_drive_returns_fixture_outputs() {
        let module = ModuleId(42);
        let fixtures = vec![(module, vec![OutputEvent(()), OutputEvent(())])];
        let mut t = SimpleComponentTest::with_fixtures(fixtures);
        let handle = t.mount(module, TypedProps(()), SlotMap(()));

        let outputs = t.drive(&handle, InputEvent(()));
        assert_eq!(outputs.len(), 2);
        // drive appends to the emitted log.
        assert_eq!(t.emitted_count(), 2);

        // A second drive appends again (stub semantics: each drive re-emits
        // the fixtures).
        let _ = t.drive(&handle, InputEvent(()));
        assert_eq!(t.emitted_count(), 4);
    }

    #[test]
    fn simple_component_test_drive_without_mount_returns_empty() {
        let mut t = SimpleComponentTest::new();
        // No mount yet → current_module is None → drive returns empty.
        let outputs = t.drive(&ActiveHandle(()), InputEvent(()));
        assert!(outputs.is_empty());
        assert_eq!(t.emitted_count(), 0);
    }

    #[test]
    fn simple_component_test_slot_output_returns_placeholder() {
        let mut t = SimpleComponentTest::new();
        let handle = t.mount(ModuleId(1), TypedProps(()), SlotMap(()));
        // slot_output returns a placeholder SlotValue regardless of slot.
        let _ = t.slot_output(&handle, "header");
    }

    #[test]
    fn simple_component_test_expect_output_pass_after_drive() {
        let module = ModuleId(99);
        let fixtures = vec![(module, vec![OutputEvent(())])];
        let mut t = SimpleComponentTest::with_fixtures(fixtures);
        let handle = t.mount(module, TypedProps(()), SlotMap(()));
        let _ = t.drive(&handle, InputEvent(()));
        // The fixture output matches the expected placeholder OutputEvent.
        assert!(matches!(
            t.expect_output(&handle, OutputEvent(())),
            TestResult::Pass
        ));
    }

    #[test]
    fn simple_component_test_expect_output_fail_without_drive() {
        let module = ModuleId(99);
        let fixtures = vec![(module, vec![OutputEvent(())])];
        let mut t = SimpleComponentTest::with_fixtures(fixtures);
        let handle = t.mount(module, TypedProps(()), SlotMap(()));
        // No drive → emitted log is empty → expect_output fails.
        match t.expect_output(&handle, OutputEvent(())) {
            TestResult::Fail(report) => {
                assert!(!report.summary.is_empty());
                assert!(report.tick.is_none());
                assert!(report.snapshot.is_none());
            }
            other => panic!("expected TestResult::Fail, got {other:?}"),
        }
    }

    #[test]
    fn simple_component_test_teardown_removes_module_from_registry() {
        let module = ModuleId(5);
        let fixtures = vec![(module, vec![OutputEvent(())])];
        let mut t = SimpleComponentTest::with_fixtures(fixtures);
        assert_eq!(t.registered_count(), 1);
        let handle = t.mount(module, TypedProps(()), SlotMap(()));
        // mount of an existing module does not duplicate the entry.
        assert_eq!(t.registered_count(), 1);
        let _ = t.drive(&handle, InputEvent(()));
        assert_eq!(t.emitted_count(), 1);

        t.teardown(handle);
        // teardown removes the module from the fixture registry and
        // clears the emitted log.
        assert_eq!(t.registered_count(), 0);
        assert_eq!(t.emitted_count(), 0);
    }

    // ---- SimpleTracePlayer (Gap H14) -------------------------------------

    #[test]
    fn simple_trace_player_default_is_empty() {
        let p = SimpleTracePlayer::new();
        assert_eq!(p.tick_count(), 0);
        assert_eq!(p.current_tick(), 0);
    }

    #[test]
    fn simple_trace_player_with_ticks_seeds_buffer() {
        let p = SimpleTracePlayer::with_ticks(vec![Frame(()), Frame(()), Frame(())]);
        assert_eq!(p.tick_count(), 3);
        assert_eq!(p.current_tick(), 0);
    }

    #[test]
    fn simple_trace_player_step_on_empty_returns_end_of_trace() {
        let mut p = SimpleTracePlayer::new();
        assert!(matches!(p.step(), StepResult::EndOfTrace));
        assert_eq!(p.current_tick(), 0);
    }

    #[test]
    fn simple_trace_player_step_advances_then_ends() {
        let mut p = SimpleTracePlayer::with_ticks(vec![Frame(()), Frame(()), Frame(())]);
        // 3 ticks → step 1: Advanced (more remain).
        assert!(matches!(p.step(), StepResult::Advanced));
        assert_eq!(p.current_tick(), 1);
        // step 2: Advanced (one more remains).
        assert!(matches!(p.step(), StepResult::Advanced));
        assert_eq!(p.current_tick(), 2);
        // step 3: EndOfTrace (just exhausted the buffer).
        assert!(matches!(p.step(), StepResult::EndOfTrace));
        assert_eq!(p.current_tick(), 3);
        // step 4: EndOfTrace (already past end).
        assert!(matches!(p.step(), StepResult::EndOfTrace));
        assert_eq!(p.current_tick(), 3);
    }

    #[test]
    fn simple_trace_player_load_clears_buffer_and_resets_tick() {
        let mut p = SimpleTracePlayer::with_ticks(vec![Frame(()), Frame(())]);
        // Advance once so current_tick is non-zero.
        let _ = p.step();
        assert_eq!(p.current_tick(), 1);
        assert_eq!(p.tick_count(), 2);

        // load() stores an empty Vec (UnifiedTrace is opaque) and resets
        // the tick cursor.
        let result = p.load(&UnifiedTrace(()));
        assert!(result.is_ok());
        assert_eq!(p.tick_count(), 0);
        assert_eq!(p.current_tick(), 0);
        // step on the cleared buffer immediately reaches end-of-trace.
        assert!(matches!(p.step(), StepResult::EndOfTrace));
    }

    #[test]
    fn simple_trace_player_seek_does_not_panic() {
        let mut p = SimpleTracePlayer::with_ticks(vec![Frame(()), Frame(())]);
        let _ = p.step();
        assert_eq!(p.current_tick(), 1);
        // TickId is opaque; seek is a no-op stub that must not panic and
        // must not regress the cursor.
        p.seek(TickId(()));
        assert_eq!(p.current_tick(), 1);
    }

    #[test]
    fn simple_trace_player_assert_replay_returns_pass() {
        let p = SimpleTracePlayer::with_ticks(vec![Frame(()), Frame(())]);
        let harness = SimpleTestHarness::new();
        // assert_replay consumes self and reports Pass (stub).
        assert!(matches!(p.assert_replay(&harness), TestResult::Pass));
    }

    /// Compile-time assertion: `SimpleComponentTest` implements the full
    /// `ComponentTest` trait. If a method is removed, renamed, or its
    /// signature changes, this test fails to compile.
    #[test]
    fn simple_component_test_implements_component_test() {
        fn _assert<T: ComponentTest>() {}
        _assert::<SimpleComponentTest>();
    }

    /// Compile-time assertion: `SimpleTracePlayer` implements the full
    /// `TracePlayer` trait.
    #[test]
    fn simple_trace_player_implements_trace_player() {
        fn _assert<T: TracePlayer>() {}
        _assert::<SimpleTracePlayer>();
    }
}

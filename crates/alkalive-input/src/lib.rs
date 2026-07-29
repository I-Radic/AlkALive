//! AlkALive alkalive-input crate.
//!
//! Input & Event System — see `docs/SPECIFICATION.md` §8 and ADRs 010 / 011 / 013.
//!
//! All raw device state is captured at the WASM scheduler boundary as a
//! single typed [`InputBatch`] per frame and stays inside the ADR 013
//! hot path — no WASM↔DOM crossing. Pointer, stylus (pressure / tilt /
//! twist), multi-touch contact sets, gamepad axes / buttons, and
//! keyboard are first-class typed events; no device class is
//! second-class (ADR 010).
//!
//! Wave 3 trait-definition skeleton: signatures are locked against the
//! spec; every body is `todo!()`. No implementation ships this wave.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// Local placeholder types — replaced by alkalive-layout / alkalive-core
// in later waves. Kept local so the crate compiles with zero external deps.
// ---------------------------------------------------------------------------

/// PLACEHOLDER 2D vector — replaced by `alkalive-layout::Vec2` (§5.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2(pub f32, pub f32);

/// PLACEHOLDER render-object type — replaced by
/// `alkalive-core::RenderObject` (W4-T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderObject;

/// PLACEHOLDER generic handle to a `T` — replaced by `alkalive-core`'s
/// handle type (ADR 007) in Wave 4. Opaque; construction is deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle<T> {
    /// Stable id of the referenced object.
    id: u64,
    /// Variance marker (keeps `Handle<T>` parameterised without
    /// imposing `T: Copy`/`Clone`/`Send`/`Sync` constraints from the
    /// marker itself).
    _marker: std::marker::PhantomData<fn() -> T>,
}

// ---------------------------------------------------------------------------
// §8.1 Event capture
// ---------------------------------------------------------------------------

/// First-class input device classes (ADR 010). No device class is second-class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// Mouse / trackball pointer.
    Pointer,
    /// Pressure / tilt / twist stylus.
    Stylus,
    /// Multi-touch contact.
    Touch,
    /// Gamepad with axes and buttons.
    Gamepad,
    /// Physical keyboard.
    Keyboard,
}

/// Bitset of [`DeviceKind`]s present in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceKindSet(pub u16);

/// Bitset of pointer buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonSet(pub u16);

/// Bitset of keyboard modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModifierSet(pub u16);

/// Physical / logical key code (ADR 010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCode(pub u32);

/// Monotonic nanosecond timestamp captured at the scheduler boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonotonicNs(pub u64);

/// Layout-scope handle — the CPU bounding-volume mirror's invalidation
/// unit (ADR 010 / ADR 002).
///
/// PLACEHOLDER concrete form; the real scope is derived from
/// `alkalive-layout::LayoutScope` (§5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutScope(pub u64);

/// Pointer phase (ADR 010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    /// Contact began.
    Down,
    /// Contact moved.
    Move,
    /// Contact ended.
    Up,
    /// Contact was cancelled (e.g. orphaned grab).
    Cancel,
}

/// Keyboard phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyPhase {
    /// Key pressed.
    Press,
    /// Key released.
    Release,
    /// Auto-repeat.
    Repeat,
}

/// A single pointer / stylus / touch sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    /// Source device class.
    pub device: DeviceKind,
    /// Distinguishing id for multi-device streams.
    pub device_id: u32,
    /// Viewport-space position.
    pub position: Vec2,
    /// Per-frame delta.
    pub delta: Vec2,
    /// Pressure ∈ `[0,1]`; `1.0` for mouse.
    pub pressure: f32,
    /// Tilt in radians; zero for mouse.
    pub tilt: Vec2,
    /// Stylus twist; zero for non-stylus.
    pub twist: f32,
    /// Active button mask.
    pub buttons: ButtonSet,
    /// Phase of this sample.
    pub phase: PointerPhase,
}

/// A single keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// Physical key code.
    pub code: KeyCode,
    /// Active modifier mask.
    pub modifiers: ModifierSet,
    /// Phase of this event.
    pub phase: KeyPhase,
    /// Auto-repeat count.
    pub repeat_count: u32,
}

/// A single gamepad sample.
#[derive(Debug, Clone, PartialEq)]
pub struct GamepadSample {
    /// Distinguishing id for multi-device streams.
    pub device_id: u32,
    /// Axis values ∈ `[-1,1]`.
    pub axes: Vec<f32>,
    /// Button pressures ∈ `[0,1]`.
    pub buttons: Vec<f32>,
}

/// Tagged union of per-frame input events.
///
/// The spec models this as a `union`; the faithful Rust translation is a
/// safe `enum` — `#![forbid(unsafe_code)]` rules out a Rust `union`,
/// whose field access requires `unsafe`.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// Pointer / stylus / touch sample.
    Pointer(PointerSample),
    /// Keyboard event.
    Key(KeyEvent),
    /// Gamepad sample.
    Gamepad(GamepadSample),
}

/// Pre-partitioned batch of input events for one frame (ADR 013).
#[derive(Debug, Clone, PartialEq)]
pub struct InputBatch {
    /// Events pre-partitioned by device.
    pub events: Vec<InputEvent>,
    /// Which device classes are present this frame.
    pub device_mask: DeviceKindSet,
    /// Capture timestamp.
    pub timestamp: MonotonicNs,
}

// ---------------------------------------------------------------------------
// §8.2 Hit-testing
// ---------------------------------------------------------------------------

/// A single hit-test result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitResult {
    /// Hit render-object handle (ADR 007).
    pub object: Handle<RenderObject>,
    /// Local-space hit point.
    pub point: Vec2,
    /// Overlap ordering depth.
    pub depth: f32,
    /// Whether this result came from a precise GPU pick.
    pub precise: bool,
}

/// CPU-resident hit-testing surface (ADR 010 / ADR 001).
///
/// A bounding-volume mirror of the GPU scene, refreshed after every
/// layout commit (ADR 002), is the hot-path hit surface. GPU pick-buffer
/// readback is invoked only for *precise* picks — sub-pixel caret
/// placement, polygonal shapes — and never appears per-frame.
pub trait HitTester {
    /// Refresh the mirror after a layout commit.
    fn refresh(&mut self, scope: &LayoutScope) {
        todo!()
    }
    /// Broad-phase hit test returning all overlaps, depth-ordered.
    fn hit_test(&self, point: Vec2, device: DeviceKind) -> Vec<HitResult> {
        todo!()
    }
    /// Precise GPU pick-buffer readback (off the per-frame path).
    fn precise_pick(&self, point: Vec2) -> HitResult {
        todo!()
    }
    /// Invalidate the mirror for a scope (ADR 002 dirty-rect).
    fn invalidate(&mut self, scope: &LayoutScope) {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §8.3 Event routing — GrabHandle
// ---------------------------------------------------------------------------

/// Cross-frame gesture capture handle (ADR 013).
///
/// Dispatch is a single direct call from the scheduler to the hit
/// object with an owned [`InputEvent`]; there is no DOM-style
/// capture / target / bubble propagation. For cross-frame gestures the
/// hit object captures the stream by returning a `GrabHandle`;
/// subsequent events of the same `(device, device_id)` are routed to
/// that handle until released. When two objects claim overlapping grabs,
/// the most recently issued explicit grab wins; the loser receives a
/// synthetic `Cancel`.
pub trait GrabHandle {
    /// Owning render object.
    fn owner(&self) -> Handle<RenderObject> {
        todo!()
    }
    /// Captured device class.
    fn device(&self) -> DeviceKind {
        todo!()
    }
    /// Captured device id.
    fn device_id(&self) -> u32 {
        todo!()
    }
    /// Release the grab; subsequent events route via hit-test again.
    fn release(&mut self) {
        todo!()
    }
    /// Whether the grab is still active.
    fn is_active(&self) -> bool {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §8.4 Gesture recognition
// ---------------------------------------------------------------------------

/// Outcome of feeding one event to a [`GestureState`].
pub enum GestureOutcome {
    /// Gesture continues; consume more events.
    Continue,
    /// Gesture committed successfully.
    Commit,
    /// Gesture was cancelled.
    Cancel,
    /// Capture the stream via a grab handle.
    Grab(Box<dyn GrabHandle>),
}

/// Gesture recogniser phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GesturePhase {
    /// No gesture in progress.
    Idle,
    /// Gesture just began.
    Began,
    /// Gesture changed this event.
    Changed,
    /// Gesture ended normally.
    Ended,
    /// Gesture was cancelled.
    Cancelled,
}

/// Per-object gesture / state machine (ADR 010).
///
/// Render objects own their gesture state; there is no central
/// recogniser. Each object exposes one `GestureState` per active device
/// stream and produces a [`GestureOutcome`] per event.
pub trait GestureState {
    /// Feed one input event; produce an outcome.
    fn on_event(&mut self, event: InputEvent) -> GestureOutcome {
        todo!()
    }
    /// Synthetic cancel on orphan (object removed mid-gesture).
    fn on_cancel(&mut self) {
        todo!()
    }
    /// Current gesture phase.
    fn current_phase(&self) -> GesturePhase {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §8.5 Focus model (ADR 011)
// ---------------------------------------------------------------------------

/// Focus-annotation transition emitted to the focus-ring renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusEvent {
    /// Target gained focus.
    FocusGained,
    /// Target lost focus.
    FocusLost,
    /// A focus-stealing grab was cancelled.
    FocusStealCancelled,
}

/// Unified virtual focus annotation layer (ADR 011).
///
/// Focus, tab order, and the focus ring live on a cached,
/// invalidation-driven derived view over the render-object graph
/// (ADR 007), not a separate tree. **Input dispatch is the sole writer**
/// of focus state; **focus-ring rendering is the sole active reader**.
/// The object receiving input is therefore the same object that owns the
/// focus annotation.
///
/// `current_focus` is the read entry point that the future a11y layer
/// (§10) will consume when un-deferred per ADR 019. No DOM projection
/// surface exists in this phase.
pub trait FocusManager {
    /// Dispatch a batch against hit-test results; return normalised errors.
    fn dispatch(&mut self, batch: InputBatch, hits: &[HitResult]) -> Vec<InputError> {
        todo!()
    }
    /// Set focus — sole writer (ADR 010 / ADR 011).
    fn set_focus(&mut self, target: Handle<RenderObject>) {
        todo!()
    }
    /// Current focus — sole active reader (future a11y reads here too).
    fn current_focus(&self) -> Option<Handle<RenderObject>> {
        todo!()
    }
    /// Advance virtual tab order forward.
    fn tab_next(&mut self) {
        todo!()
    }
    /// Advance virtual tab order backward.
    fn tab_prev(&mut self) {
        todo!()
    }
    /// Emit focus transitions for the focus-ring renderer.
    fn emit_focus_events(&self) -> Vec<FocusEvent> {
        todo!()
    }
    /// Invalidate the focus annotation layer for a scope.
    fn invalidate(&mut self, scope: &LayoutScope) {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §8.6 Error handling
// ---------------------------------------------------------------------------

/// Normalised input error (§8.6).
///
/// Invalid input state is normalised at the scheduler boundary:
/// orphaned grabs are cancelled, out-of-range device ids are dropped,
/// mismatched touch cycles yield `UnmatchedTouchBegin`, and stale mirror
/// hits either re-route through `precise_pick` or return `MirrorStale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputError {
    /// Stale contact (touch cycle desync).
    StaleContact,
    /// Grab orphaned by mid-gesture object removal.
    OrphanedGrab,
    /// Touch begin without a matching end.
    UnmatchedTouchBegin,
    /// Device id out of valid range.
    OutOfRangeDeviceId,
    /// GPU pick-buffer readback failed.
    PickBufferReadbackFailed,
    /// CPU bounding-volume mirror is stale.
    MirrorStale,
}

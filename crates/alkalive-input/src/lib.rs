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
//! # Wave 7 status
//!
//! The [`HitTester`] and [`FocusManager`] traits ship concrete reference
//! implementations — [`HitTesterImpl`] and [`FocusManagerImpl`] — backed
//! by a flat CPU bounding-volume mirror and a single-slot focus owner.
//! [`GrabHandle`] and [`GestureState`] likewise ship concrete reference
//! implementations — [`SimpleGrabHandle`] and [`SimpleGestureState`] —
//! with real fields and behaviour. The full render-object-tree
//! integration (ADR 007, Wave 4) is layered on top in a later wave; no
//! `todo!()` skeletons remain in this crate.

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
/// handle type (ADR 007) in Wave 4. Opaque in the final design; for Wave
/// 7 a small constructor is exposed so the reference implementations and
/// their tests can mint handles without the render-object allocator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle<T> {
    /// Stable id of the referenced object.
    id: u64,
    /// Variance marker (keeps `Handle<T>` parameterised without
    /// imposing `T: Copy`/`Clone`/`Send`/`Sync` constraints from the
    /// marker itself).
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Construct a handle with the given stable id.
    ///
    /// PLACEHOLDER constructor — the real handle type (ADR 007) is
    /// produced by the render-object allocator in Wave 4. Exposed here
    /// so that Wave 7 reference implementations and tests can mint
    /// handles without that allocator.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            _marker: std::marker::PhantomData,
        }
    }

    /// Returns the stable id of the referenced object.
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// PLACEHOLDER axis-aligned bounding rectangle — replaced by
/// `alkalive-layout::Rect` (§5) in a later wave. The CPU bounding-volume
/// mirror (ADR 010 / ADR 001) is a `Vec<(Handle<RenderObject>, Rect)>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Minimum-x corner (viewport space).
    pub x: f32,
    /// Minimum-y corner (viewport space).
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Construct a rectangle from its minimum corner and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Inclusive contains test for a viewport-space point.
    ///
    /// A point on the bottom / right edge is considered inside, matching
    /// the closed `[x, x+w] × [y, y+h]` convention used by the
    /// broad-phase mirror.
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
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
    fn refresh(&mut self, scope: &LayoutScope);
    /// Broad-phase hit test returning all overlaps, depth-ordered.
    fn hit_test(&self, point: Vec2, device: DeviceKind) -> Vec<HitResult>;
    /// Precise GPU pick-buffer readback (off the per-frame path).
    fn precise_pick(&self, point: Vec2) -> HitResult;
    /// Invalidate the mirror for a scope (ADR 002 dirty-rect).
    fn invalidate(&mut self, scope: &LayoutScope);
}

/// Concrete Wave 7 [`HitTester`] backed by a CPU bounding-volume mirror.
///
/// The mirror is a flat `Vec<(Handle<RenderObject>, Rect)>` populated by
/// [`HitTesterImpl::set_objects`]. Depth is derived from insertion order:
/// objects inserted later are treated as "on top" (higher depth) and
/// appear first in depth-ordered hit-test results. Real z-order
/// integration with the render-object tree (ADR 007) arrives in a later
/// wave; until then this struct is the reference hot-path hit surface.
///
/// The trait-level [`HitTester::refresh`] only receives a [`LayoutScope`]
/// and cannot itself see the render-object tree, so the scheduler feeds
/// the freshly committed bounds through [`HitTesterImpl::set_objects`].
#[derive(Debug, Default)]
pub struct HitTesterImpl {
    /// Bounding-volume mirror: `(handle, rect)` pairs in insertion order.
    objects: Vec<(Handle<RenderObject>, Rect)>,
}

impl HitTesterImpl {
    /// Construct an empty hit-tester.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the entire bounding-volume mirror with the provided
    /// `(handle, x, y, w, h)` tuples.
    ///
    /// This is the Wave 7 rebuild entry point: the mirror is cleared and
    /// rebuilt from the supplied viewport-space bounds. The 4 floats per
    /// entry are `(x, y, w, h)`; explicit z/depth is deferred to a later
    /// wave and derived here from insertion order.
    pub fn set_objects(&mut self, objects: &[(Handle<RenderObject>, f32, f32, f32, f32)]) {
        self.objects.clear();
        self.objects.reserve(objects.len());
        for &(handle, x, y, w, h) in objects {
            self.objects.push((handle, Rect::new(x, y, w, h)));
        }
    }

    /// Number of objects currently mirrored.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the mirror is empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

impl HitTester for HitTesterImpl {
    fn refresh(&mut self, _scope: &LayoutScope) {
        // Wave 7: the real refresh rebuilds the mirror from the
        // render-object tree after a layout commit. Without the tree
        // wired in, the scheduler repopulates via `set_objects`; this
        // method simply clears the stale mirror.
        self.objects.clear();
    }

    fn hit_test(&self, point: Vec2, _device: DeviceKind) -> Vec<HitResult> {
        // Wave 7: device class does not yet influence the broad phase;
        // all mirrored objects are candidate hits.
        let mut hits: Vec<HitResult> = self
            .objects
            .iter()
            .enumerate()
            .filter(|(_, (_, rect))| rect.contains(point.0, point.1))
            .map(|(idx, (handle, rect))| HitResult {
                object: *handle,
                // Local-space hit point: offset from the rect's origin.
                point: Vec2(point.0 - rect.x, point.1 - rect.y),
                // Depth derived from insertion order; later = on top.
                depth: idx as f32,
                precise: false,
            })
            .collect();
        // Depth-ordered: topmost (highest insertion index) first.
        hits.sort_by(|a, b| {
            b.depth
                .partial_cmp(&a.depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits
    }

    fn precise_pick(&self, point: Vec2) -> HitResult {
        // Wave 7 stub: delegate to the broad phase and flag the topmost
        // hit as precise. Real GPU pick-buffer readback arrives in a
        // later wave; until then a no-hit yields a sentinel handle
        // (`id = u64::MAX`) so the non-`Option` signature is honoured.
        let mut hits = self.hit_test(point, DeviceKind::Pointer);
        if hits.is_empty() {
            return HitResult {
                object: Handle::new(u64::MAX),
                point,
                depth: 0.0,
                precise: true,
            };
        }
        let mut top = hits.remove(0);
        top.precise = true;
        top
    }

    fn invalidate(&mut self, _scope: &LayoutScope) {
        // Wave 7 simplification: clear the entire mirror. The real
        // implementation honours the dirty-rect scope (ADR 002) once the
        // render-object tree is available.
        self.objects.clear();
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
///
/// Wave 7: all methods are **required** (no default body). The reference
/// implementation [`SimpleGrabHandle`] stores the owning render object,
/// captured device class / id, and an `active` flag flipped to `false`
/// by [`GrabHandle::release`]. Real grab arbitration against the
/// render-object tree (ADR 007) is layered on in a later wave, but the
/// field-backed behaviour here is enough for routed-event tests and
/// downstream crates that need a concrete [`GrabHandle`].
pub trait GrabHandle {
    /// Owning render object.
    fn owner(&self) -> Handle<RenderObject>;
    /// Captured device class.
    fn device(&self) -> DeviceKind;
    /// Captured device id.
    fn device_id(&self) -> u32;
    /// Release the grab; subsequent events route via hit-test again.
    fn release(&mut self);
    /// Whether the grab is still active.
    fn is_active(&self) -> bool;
}

/// Concrete Wave 7 [`GrabHandle`] with real fields.
///
/// Stores the owning [`Handle<RenderObject>`], captured [`DeviceKind`]
/// and `device_id`, and an `active` flag. [`GrabHandle::release`] flips
/// `active` to `false`; [`GrabHandle::is_active`] reads it back. The
/// remaining accessors return their stored fields unchanged.
///
/// A freshly constructed [`SimpleGrabHandle`] is **active**; only an
/// explicit [`GrabHandle::release`] deactivates it. This is the
/// reference grab handle for Wave 7: full grab arbitration against the
/// render-object tree (ADR 007) is layered on in a later wave.
#[derive(Debug)]
pub struct SimpleGrabHandle {
    /// Owning render object.
    owner: Handle<RenderObject>,
    /// Captured device class.
    device: DeviceKind,
    /// Captured device id.
    device_id: u32,
    /// Whether the grab is still active; flipped to `false` by `release`.
    active: bool,
}

impl SimpleGrabHandle {
    /// Construct an **active** grab handle for `owner` capturing the
    /// given `(device, device_id)` stream.
    pub fn new(owner: Handle<RenderObject>, device: DeviceKind, device_id: u32) -> Self {
        Self {
            owner,
            device,
            device_id,
            active: true,
        }
    }
}

impl GrabHandle for SimpleGrabHandle {
    fn owner(&self) -> Handle<RenderObject> {
        self.owner
    }

    fn device(&self) -> DeviceKind {
        self.device
    }

    fn device_id(&self) -> u32 {
        self.device_id
    }

    fn release(&mut self) {
        self.active = false;
    }

    fn is_active(&self) -> bool {
        self.active
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
///
/// Wave 7: all methods are **required** (no default body). The reference
/// implementation [`SimpleGestureState`] tracks a single
/// [`GesturePhase`] that begins at [`GesturePhase::Idle`], advances to
/// [`GesturePhase::Changed`] on [`GestureState::on_event`] (which
/// returns [`GestureOutcome::Continue`]), and flips to
/// [`GesturePhase::Cancelled`] on [`GestureState::on_cancel`]. Full
/// per-object state machines fed from the routed render-object-tree
/// event stream (ADR 007) are layered on in a later wave.
pub trait GestureState {
    /// Feed one input event; produce an outcome.
    fn on_event(&mut self, event: InputEvent) -> GestureOutcome;
    /// Synthetic cancel on orphan (object removed mid-gesture).
    fn on_cancel(&mut self);
    /// Current gesture phase.
    fn current_phase(&self) -> GesturePhase;
}

/// Concrete Wave 7 [`GestureState`] with a single tracked phase.
///
/// Holds one [`GesturePhase`] field, initialised to [`GesturePhase::Idle`].
/// [`GestureState::on_event`] advances the phase to
/// [`GesturePhase::Changed`] and returns [`GestureOutcome::Continue`]
/// (consume more events). [`GestureState::on_cancel`] flips the phase
/// to [`GesturePhase::Cancelled`]. [`GestureState::current_phase`]
/// returns the stored phase.
///
/// This is the reference gesture state for Wave 7: real per-object
/// recognisers fed from the routed event stream (ADR 007) are layered on
/// in a later wave, but the phase-backed behaviour here is enough for
/// grab / dispatch tests and downstream crates that need a concrete
/// [`GestureState`].
#[derive(Debug)]
pub struct SimpleGestureState {
    /// Current gesture phase; starts at [`GesturePhase::Idle`].
    phase: GesturePhase,
}

impl Default for SimpleGestureState {
    /// A fresh gesture state starts in the [`GesturePhase::Idle`] phase.
    fn default() -> Self {
        Self {
            phase: GesturePhase::Idle,
        }
    }
}

impl SimpleGestureState {
    /// Construct a gesture state in the [`GesturePhase::Idle`] phase.
    pub fn new() -> Self {
        Self::default()
    }
}

impl GestureState for SimpleGestureState {
    fn on_event(&mut self, _event: InputEvent) -> GestureOutcome {
        self.phase = GesturePhase::Changed;
        GestureOutcome::Continue
    }

    fn on_cancel(&mut self) {
        self.phase = GesturePhase::Cancelled;
    }

    fn current_phase(&self) -> GesturePhase {
        self.phase
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
    fn dispatch(&mut self, batch: InputBatch, hits: &[HitResult]) -> Vec<InputError>;
    /// Set focus — sole writer (ADR 010 / ADR 011).
    fn set_focus(&mut self, target: Handle<RenderObject>);
    /// Current focus — sole active reader (future a11y reads here too).
    fn current_focus(&self) -> Option<Handle<RenderObject>>;
    /// Advance virtual tab order forward.
    fn tab_next(&mut self);
    /// Advance virtual tab order backward.
    fn tab_prev(&mut self);
    /// Emit focus transitions for the focus-ring renderer.
    fn emit_focus_events(&self) -> Vec<FocusEvent>;
    /// Invalidate the focus annotation layer for a scope.
    fn invalidate(&mut self, scope: &LayoutScope);
}

/// Concrete Wave 7 [`FocusManager`] with a single-slot focus owner.
///
/// Holds the current focus [`Handle`] (the sole piece of writable focus
/// state) and a pending queue of [`FocusEvent`]s produced by
/// [`FocusManagerImpl::set_focus`]. The queue is drained by
/// [`FocusManagerImpl::emit_focus_events`] for the focus-ring renderer.
///
/// `dispatch`, `tab_next`, `tab_prev`, and `invalidate` are Wave 7
/// no-ops / empty: real dispatch needs the render-object tree (ADR 007)
/// to route owned events to hit objects and grab handles, and real tab
/// order needs the cached focus-ring view. They are implemented once the
/// tree is wired in.
#[derive(Debug, Default)]
pub struct FocusManagerImpl {
    /// The object currently owning the focus annotation
    /// (sole writer: `set_focus`; sole active reader: `current_focus`).
    current_focus: Option<Handle<RenderObject>>,
    /// Pending focus transitions, drained by `emit_focus_events`.
    /// Interior mutability lets `emit_focus_events(&self)` drain the
    /// queue despite the `&self` receiver mandated by the trait.
    pending_events: std::cell::RefCell<Vec<FocusEvent>>,
}

impl FocusManagerImpl {
    /// Construct a focus manager with no current focus and an empty
    /// pending-event queue.
    pub fn new() -> Self {
        Self {
            current_focus: None,
            pending_events: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl FocusManager for FocusManagerImpl {
    fn dispatch(&mut self, _batch: InputBatch, _hits: &[HitResult]) -> Vec<InputError> {
        // Wave 7: real dispatch routes owned events to hit objects / grab
        // handles via the render-object tree (ADR 007). Without the tree
        // there is nothing to route; return no errors.
        Vec::new()
    }

    fn set_focus(&mut self, target: Handle<RenderObject>) {
        // No-op when the focus is already on the target.
        if self.current_focus == Some(target) {
            return;
        }
        // If something currently holds focus, emit a FocusLost for it
        // before transferring.
        if self.current_focus.is_some() {
            self.pending_events.borrow_mut().push(FocusEvent::FocusLost);
        }
        self.current_focus = Some(target);
        self.pending_events
            .borrow_mut()
            .push(FocusEvent::FocusGained);
    }

    fn current_focus(&self) -> Option<Handle<RenderObject>> {
        self.current_focus
    }

    fn tab_next(&mut self) {
        // Wave 7 no-op: real virtual tab order needs the render-object tree.
    }

    fn tab_prev(&mut self) {
        // Wave 7 no-op: real virtual tab order needs the render-object tree.
    }

    fn emit_focus_events(&self) -> Vec<FocusEvent> {
        // Drain the pending queue for the focus-ring renderer.
        std::mem::take(&mut *self.pending_events.borrow_mut())
    }

    fn invalidate(&mut self, _scope: &LayoutScope) {
        // Wave 7 no-op: the cached focus annotation is recomputed from
        // the render-object tree in a later wave.
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

// ---------------------------------------------------------------------------
// Wave 7 tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: mint a typed render-object handle for tests.
    fn h(id: u64) -> Handle<RenderObject> {
        Handle::new(id)
    }

    // --- HitTesterImpl ----------------------------------------------------

    #[test]
    fn hit_test_returns_objects_inside_bounds_and_excludes_outside() {
        let mut tester = HitTesterImpl::new();
        let a = h(1);
        let b = h(2);
        tester.set_objects(&[(a, 0.0, 0.0, 10.0, 10.0), (b, 20.0, 20.0, 10.0, 10.0)]);

        // Inside a only.
        let hits = tester.hit_test(Vec2(5.0, 5.0), DeviceKind::Pointer);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].object, a);
        assert!(!hits[0].precise);

        // Inside b only.
        let hits = tester.hit_test(Vec2(25.0, 25.0), DeviceKind::Pointer);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].object, b);

        // Inside neither.
        let hits = tester.hit_test(Vec2(100.0, 100.0), DeviceKind::Pointer);
        assert!(hits.is_empty());
    }

    #[test]
    fn hit_test_local_space_point_is_relative_to_rect_origin() {
        let mut tester = HitTesterImpl::new();
        let a = h(1);
        tester.set_objects(&[(a, 10.0, 20.0, 100.0, 100.0)]);
        let hits = tester.hit_test(Vec2(15.0, 30.0), DeviceKind::Pointer);
        assert_eq!(hits.len(), 1);
        // Local-space: (15 - 10, 30 - 20) = (5, 10).
        assert_eq!(hits[0].point, Vec2(5.0, 10.0));
    }

    #[test]
    fn hit_test_overlap_is_depth_ordered_topmost_first() {
        let mut tester = HitTesterImpl::new();
        let bottom = h(1);
        let top = h(2);
        // Overlapping rects; `top` is inserted later (higher depth).
        tester.set_objects(&[(bottom, 0.0, 0.0, 10.0, 10.0), (top, 0.0, 0.0, 10.0, 10.0)]);
        let hits = tester.hit_test(Vec2(5.0, 5.0), DeviceKind::Pointer);
        assert_eq!(hits.len(), 2);
        // Topmost (highest depth) first.
        assert_eq!(hits[0].object, top);
        assert!(hits[0].depth > hits[1].depth);
        assert_eq!(hits[1].object, bottom);
    }

    #[test]
    fn hit_test_invalidate_clears_mirror() {
        let mut tester = HitTesterImpl::new();
        tester.set_objects(&[(h(1), 0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(tester.len(), 1);
        tester.invalidate(&LayoutScope(0));
        assert!(tester.is_empty());
        assert!(tester
            .hit_test(Vec2(5.0, 5.0), DeviceKind::Pointer)
            .is_empty());
    }

    #[test]
    fn hit_test_refresh_clears_mirror() {
        let mut tester = HitTesterImpl::new();
        tester.set_objects(&[(h(1), 0.0, 0.0, 10.0, 10.0)]);
        assert!(!tester.is_empty());
        tester.refresh(&LayoutScope(0));
        assert!(tester.is_empty());
    }

    #[test]
    fn precise_pick_stub_marks_topmost_hit_as_precise() {
        let mut tester = HitTesterImpl::new();
        let a = h(1);
        tester.set_objects(&[(a, 0.0, 0.0, 10.0, 10.0)]);
        let pick = tester.precise_pick(Vec2(5.0, 5.0));
        assert!(pick.precise);
        assert_eq!(pick.object, a);
    }

    #[test]
    fn precise_pick_stub_returns_sentinel_on_no_hit() {
        let tester = HitTesterImpl::new();
        let pick = tester.precise_pick(Vec2(5.0, 5.0));
        assert!(pick.precise);
        // Sentinel null handle.
        assert_eq!(pick.object, Handle::new(u64::MAX));
    }

    // --- FocusManagerImpl -------------------------------------------------

    #[test]
    fn current_focus_is_none_initially_and_some_after_set_focus() {
        let mut fm = FocusManagerImpl::new();
        assert_eq!(fm.current_focus(), None);
        let target = h(7);
        fm.set_focus(target);
        assert_eq!(fm.current_focus(), Some(target));
    }

    #[test]
    fn set_focus_emits_focus_gained_on_first_set() {
        let mut fm = FocusManagerImpl::new();
        let a = h(1);
        fm.set_focus(a);
        assert_eq!(fm.emit_focus_events(), vec![FocusEvent::FocusGained]);
    }

    #[test]
    fn set_focus_emits_focus_lost_then_gained_on_change() {
        let mut fm = FocusManagerImpl::new();
        let a = h(1);
        let b = h(2);
        fm.set_focus(a);
        // Drain the initial FocusGained to isolate the change.
        let _ = fm.emit_focus_events();
        fm.set_focus(b);
        assert_eq!(fm.current_focus(), Some(b));
        assert_eq!(
            fm.emit_focus_events(),
            vec![FocusEvent::FocusLost, FocusEvent::FocusGained]
        );
    }

    #[test]
    fn set_focus_emits_nothing_when_target_unchanged() {
        let mut fm = FocusManagerImpl::new();
        let a = h(1);
        fm.set_focus(a);
        let _ = fm.emit_focus_events();
        // Same target — no transitions.
        fm.set_focus(a);
        assert!(fm.emit_focus_events().is_empty());
        assert_eq!(fm.current_focus(), Some(a));
    }

    #[test]
    fn emit_focus_events_drains_the_queue() {
        let mut fm = FocusManagerImpl::new();
        fm.set_focus(h(1));
        let first = fm.emit_focus_events();
        assert_eq!(first, vec![FocusEvent::FocusGained]);
        // Drained — second emit is empty.
        let second = fm.emit_focus_events();
        assert!(second.is_empty());
    }

    #[test]
    fn focus_dispatch_returns_no_errors_in_wave7() {
        let mut fm = FocusManagerImpl::new();
        let batch = InputBatch {
            events: Vec::new(),
            device_mask: DeviceKindSet(0),
            timestamp: MonotonicNs(0),
        };
        let errors = fm.dispatch(batch, &[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn focus_tab_next_and_prev_are_no_ops() {
        let mut fm = FocusManagerImpl::new();
        fm.set_focus(h(1));
        // Drain the FocusGained from set_focus so we isolate tab behaviour.
        let _ = fm.emit_focus_events();
        fm.tab_next();
        fm.tab_prev();
        // Focus is unchanged and tab emitted no transitions.
        assert_eq!(fm.current_focus(), Some(h(1)));
        assert!(fm.emit_focus_events().is_empty());
    }

    #[test]
    fn focus_invalidate_is_a_no_op() {
        let mut fm = FocusManagerImpl::new();
        fm.set_focus(h(1));
        fm.invalidate(&LayoutScope(0));
        // Focus survives invalidate in Wave 7.
        assert_eq!(fm.current_focus(), Some(h(1)));
    }

    // --- SimpleGrabHandle -------------------------------------------------

    #[test]
    fn grab_handle_new_is_active_and_exposes_stored_fields() {
        let owner = h(42);
        let grab = SimpleGrabHandle::new(owner, DeviceKind::Stylus, 7);

        // Freshly constructed grab is active and round-trips its fields.
        assert!(grab.is_active());
        assert_eq!(grab.owner(), owner);
        assert_eq!(grab.device(), DeviceKind::Stylus);
        assert_eq!(grab.device_id(), 7);
    }

    #[test]
    fn grab_handle_release_deactivates_and_is_idempotent() {
        let owner = h(3);
        let mut grab = SimpleGrabHandle::new(owner, DeviceKind::Touch, 1);
        assert!(grab.is_active());

        // First release flips the active flag.
        grab.release();
        assert!(!grab.is_active());

        // A second release keeps it inactive (idempotent) and the
        // identifying fields are still intact — released, not destroyed.
        grab.release();
        assert!(!grab.is_active());
        assert_eq!(grab.owner(), owner);
        assert_eq!(grab.device(), DeviceKind::Touch);
        assert_eq!(grab.device_id(), 1);
    }

    #[test]
    fn grab_handle_is_usable_as_dyn_trait_object() {
        // `GestureOutcome::Grab` holds a `Box<dyn GrabHandle>`, so the
        // trait must remain object-safe. Exercise that path directly.
        let owner = h(9);
        let boxed: Box<dyn GrabHandle> =
            Box::new(SimpleGrabHandle::new(owner, DeviceKind::Pointer, 0));
        assert!(boxed.is_active());
        assert_eq!(boxed.owner(), owner);
        assert_eq!(boxed.device(), DeviceKind::Pointer);
        assert_eq!(boxed.device_id(), 0);
    }

    // --- SimpleGestureState -----------------------------------------------

    #[test]
    fn gesture_state_starts_idle() {
        let gs = SimpleGestureState::new();
        assert_eq!(gs.current_phase(), GesturePhase::Idle);
    }

    #[test]
    fn gesture_state_default_is_idle() {
        // `Default` must agree with `new` (both start at Idle).
        let gs = SimpleGestureState::default();
        assert_eq!(gs.current_phase(), GesturePhase::Idle);
    }

    #[test]
    fn gesture_state_on_event_advances_to_changed_and_continues() {
        let mut gs = SimpleGestureState::new();
        let event = InputEvent::Pointer(PointerSample {
            device: DeviceKind::Pointer,
            device_id: 0,
            position: Vec2(1.0, 2.0),
            delta: Vec2(0.0, 0.0),
            pressure: 1.0,
            tilt: Vec2(0.0, 0.0),
            twist: 0.0,
            buttons: ButtonSet(0),
            phase: PointerPhase::Move,
        });
        let outcome = gs.on_event(event);
        assert!(matches!(outcome, GestureOutcome::Continue));
        assert_eq!(gs.current_phase(), GesturePhase::Changed);
    }

    #[test]
    fn gesture_state_on_event_continues_across_multiple_events() {
        let mut gs = SimpleGestureState::new();
        // Feeding more than one event keeps the phase at `Changed` and
        // keeps returning `Continue` — the simple state never commits.
        for _ in 0..3 {
            let outcome = gs.on_event(InputEvent::Key(KeyEvent {
                code: KeyCode(0),
                modifiers: ModifierSet(0),
                phase: KeyPhase::Press,
                repeat_count: 0,
            }));
            assert!(matches!(outcome, GestureOutcome::Continue));
            assert_eq!(gs.current_phase(), GesturePhase::Changed);
        }
    }

    #[test]
    fn gesture_state_on_cancel_flips_to_cancelled() {
        let mut gs = SimpleGestureState::new();
        // Cancel from Idle is allowed (orphan before any event).
        gs.on_cancel();
        assert_eq!(gs.current_phase(), GesturePhase::Cancelled);

        // Cancel after activity also lands on Cancelled.
        let mut gs2 = SimpleGestureState::new();
        let _ = gs2.on_event(InputEvent::Gamepad(GamepadSample {
            device_id: 0,
            axes: vec![0.0],
            buttons: vec![0.0],
        }));
        assert_eq!(gs2.current_phase(), GesturePhase::Changed);
        gs2.on_cancel();
        assert_eq!(gs2.current_phase(), GesturePhase::Cancelled);
    }
}

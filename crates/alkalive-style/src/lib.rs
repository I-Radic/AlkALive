//! AlkALive alkalive-style crate.
//!
//! Styling & Theming — see `docs/SPECIFICATION.md` §7 and ADRs 005 / 006 / 007.
//!
//! Styling is **per-instance object-owned property state**, bound at
//! construction and addressable only via the owning render object. There
//! is no cascade, no CSSOM, no selector matching, and no specificity
//! comparator — every styled property is a typed field on the object
//! itself. Style tables are compiled into the WASM module's binary data
//! section; runtime access is an O(1) local field read.
//!
//! Wave 3 trait-definition skeleton: signatures are locked against the
//! spec; every body is `todo!()`. No implementation ships this wave.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Duration;

// ---------------------------------------------------------------------------
// Local placeholder geometry (replaced by `alkalive-layout::Mat4`, §5.2).
// ---------------------------------------------------------------------------

/// PLACEHOLDER 4×4 column-major matrix.
///
/// The canonical `Mat4` lives in `alkalive-layout` (§5.2); this local
/// stand-in exists so the style crate compiles with zero external
/// dependencies in Wave 3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [f32; 16]);

// ---------------------------------------------------------------------------
// §7.2 Property kinds + scalar value types
// ---------------------------------------------------------------------------

/// Closed classification of styled property kinds (ADR 005).
///
/// `Custom(u32)` covers module-declared typed fields (e.g. `SpringVelocity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    /// Fill/stroke colour.
    Color,
    /// Alpha opacity.
    Opacity,
    /// Stroke line width.
    LineWidth,
    /// Affine transform consumed by the GPU transform upload.
    Transform,
    /// WGSL shader effect (ADR 006).
    Shader,
    /// Module-declared typed extension field.
    Custom(u32),
}

/// RGBA8 packed colour value (ADR 005).
///
/// Out-of-range construction is clamped at the construction boundary in
/// the implementation wave (§7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

/// Alpha opacity clamped to `[0.0, 1.0]` (ADR 005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(pub f32);

/// Stroke line width clamped to `[0.0, ∞)` (ADR 005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineWidth(pub f32);

/// Scalar style value — one of the three scalar categories (ADR 005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarValue {
    /// Packed RGBA8 colour.
    Color(Color),
    /// Clamped opacity.
    Opacity(Opacity),
    /// Non-negative line width.
    LineWidth(LineWidth),
}

/// Closed enum over the three style-property categories (ADR 005).
#[derive(Debug, Clone, PartialEq)]
pub enum StyleProperty {
    /// A scalar value (colour / opacity / line width).
    Scalar(ScalarValue),
    /// A 4×4 transform matrix.
    Transform(Mat4),
    /// A WGSL shader effect.
    Shader(ShaderStyle),
}

// ---------------------------------------------------------------------------
// §7.3 WGSL as a first-class style primitive (ADR 006)
// ---------------------------------------------------------------------------

/// Compiled WGSL module, cached as a pipeline object at module load.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WgslModule {
    /// Hash of the WGSL source, used as a pipeline-cache key (ADR 017).
    pub source_hash: u64,
    /// PLACEHOLDER compiled pipeline handle (backend-specific opaque id).
    pub pipeline_handle: u64,
}

/// Packed uniform buffer sourced from the owning object's fields (ADR 006).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniformBuffer {
    /// Packed little-endian bytes ready for GPU upload.
    pub bytes: Vec<u8>,
}

/// A single bind-group entry binding a uniform buffer to a shader slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BindGroupEntry {
    /// Binding index within the bind-group layout.
    pub binding: u32,
    /// Visibility mask (vertex / fragment / compute).
    pub visibility: u32,
}

/// Pairs a compiled WGSL module with a uniform buffer sourced from the
/// owning object's fields (ADR 006).
///
/// Replaces CSS's closed `filter` catalogue: gradients, particles,
/// per-vertex displacement, and compute-driven styling are authored
/// rather than approximated.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderStyle {
    /// Compiled WGSL module (cached as a pipeline object).
    pub program: WgslModule,
    /// WGSL entry-point name.
    pub entry_point: &'static str,
    /// Packed uniform buffer.
    pub uniforms: UniformBuffer,
    /// Bind-group entries. The spec uses `[BindGroupEntry; N]`; this
    /// skeleton uses `Vec<BindGroupEntry>` for unbounded `N`.
    pub bindings: Vec<BindGroupEntry>,
}

// ---------------------------------------------------------------------------
// §7.5 Animation framework
// ---------------------------------------------------------------------------

/// Keyframe interpolation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Interpolation {
    /// Linear interpolation.
    Linear,
    /// Step (hold previous value until the next keyframe).
    Step,
    /// Cubic-spline interpolation.
    CubicSpline,
}

/// Animation lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationState {
    /// Constructed but not yet started.
    Idle,
    /// Advancing on each `tick`.
    Running,
    /// Explicitly paused.
    Paused,
    /// Reached the final keyframe.
    Completed,
}

/// A single keyframe in an [`Animation`].
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// Normalised time ∈ `[0.0, 1.0]`.
    pub time: f32,
    /// The style value at this keyframe.
    pub value: StyleProperty,
    /// How to interpolate to the next keyframe.
    pub interpolation: Interpolation,
}

/// PLACEHOLDER easing-function trait.
///
/// The spec names `EasingFn` as a field of [`Animation`] without
/// enumerating concrete variants. This trait is the type-signature lock;
/// concrete easings are added in the implementation wave.
pub trait EasingFn {
    /// Sample the easing curve at normalised time `t ∈ [0, 1]`.
    fn sample(&self, t: f32) -> f32 {
        todo!()
    }
}

/// A value-level animation state machine that writes directly to the
/// owning object's style fields each frame (ADR 005).
///
/// Animated properties are tween/keyframe interpolations over owned
/// fields — not CSS transitions, not declarative cascade animations.
/// The render object polls the animation during the per-frame
/// style-read phase.
pub struct Animation {
    /// The property this animation drives.
    pub property: PropertyKind,
    /// Ordered keyframes.
    pub keyframes: Vec<Keyframe>,
    /// Total duration.
    pub duration: Duration,
    /// Easing function.
    pub easing: Box<dyn EasingFn + Send + Sync>,
    /// Elapsed time since start.
    pub elapsed: Duration,
    /// Current lifecycle state.
    pub state: AnimationState,
}

impl Animation {
    /// Advance `elapsed` by `dt` and write the interpolated
    /// [`StyleProperty`] back into the owning field.
    ///
    /// On error the runtime logs the error and freezes the animation at
    /// its last valid frame (§7.7).
    pub fn tick(&mut self, dt: Duration) -> Result<(), AnimationError> {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// §7.7 Error handling
// ---------------------------------------------------------------------------

/// Animation construction / tick error (§7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationError {
    /// Keyframes are not monotonically ordered in time.
    KeyframeOutOfOrder,
    /// The property kind cannot be animated.
    InvalidPropertyKind(PropertyKind),
    /// The interpolation mode is not supported for the property kind.
    InterpolationNotSupported(Interpolation, PropertyKind),
    /// Duration was zero.
    DurationZero,
}

// ---------------------------------------------------------------------------
// §7.1 Style trait + OwnedStyle
// ---------------------------------------------------------------------------

/// Per-instance object-owned style state (ADR 005 / ADR 007).
///
/// Style is read-only input to layout and render; it never triggers
/// box-tree recalc. Every accessor is an O(1) local field read against
/// binary-compiled state.
pub trait Style {
    /// Typed property accessor.
    fn property(&self, kind: PropertyKind) -> StyleProperty {
        todo!()
    }
    /// Fill/stroke colour — default: transparent black.
    fn color(&self) -> Color {
        todo!()
    }
    /// Alpha opacity — default: `1.0`.
    fn opacity(&self) -> Opacity {
        todo!()
    }
    /// Stroke line width — default: `0.0`.
    fn line_width(&self) -> LineWidth {
        todo!()
    }
    /// Affine transform — default: identity.
    fn transform(&self) -> Mat4 {
        todo!()
    }
    /// WGSL shader effect — default: passthrough.
    fn effect(&self) -> ShaderStyle {
        todo!()
    }
    /// Named animation lookup, if any.
    fn animation(&self, name: &str) -> Option<&Animation> {
        todo!()
    }
}

/// Concrete owned style bundle — the value type returned by [`Theme`].
///
/// Holds the per-instance property state as typed fields (ADR 007). A
/// render object receives its style either by looking up a named preset
/// or by accepting [`Theme::default`]; there is no inheritance.
pub struct OwnedStyle {
    /// Fill/stroke colour.
    pub color: Color,
    /// Alpha opacity.
    pub opacity: Opacity,
    /// Stroke line width.
    pub line_width: LineWidth,
    /// Affine transform.
    pub transform: Mat4,
    /// WGSL shader effect.
    pub effect: ShaderStyle,
    /// Named animations.
    pub animations: Vec<(String, Animation)>,
}

// ---------------------------------------------------------------------------
// §7.4 Theming
// ---------------------------------------------------------------------------

/// A module exporting a set of named style presets (ADR 005).
///
/// Themes are construction-time token dictionaries, NOT a propagation
/// system. There is no inheritance: a property not explicitly set takes
/// the type's default value, not a parent's value. Subtree consistency
/// is the author's responsibility, expressed at construction rather than
/// resolved at match time.
pub trait Theme {
    /// Look up a named preset.
    fn preset(&self, name: &str) -> OwnedStyle {
        todo!()
    }
    /// The default preset.
    fn default(&self) -> OwnedStyle {
        todo!()
    }
    /// Enumerate available preset names.
    fn names(&self) -> &[&'static str] {
        todo!()
    }
}

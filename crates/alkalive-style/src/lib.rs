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
//! Wave 3 trait definitions: signatures are locked against the spec;
//! concrete defaults are provided by [`DefaultStyle`] and [`DefaultTheme`].

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
/// concrete easings are added in the implementation wave. `sample` is a
/// required method — every concrete [`EasingFn`] must provide its own
/// implementation.
pub trait EasingFn {
    /// Sample the easing curve at normalised time `t ∈ [0, 1]`.
    fn sample(&self, t: f32) -> f32;
}

/// Linear easing: `f(t) = t`.
///
/// The simplest concrete [`EasingFn`]. Provided so that [`Animation`] can
/// be constructed today; richer easings (ease-in, ease-out, cubic-bezier)
/// land in the implementation wave alongside keyframe interpolation.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearEasing;

impl EasingFn for LinearEasing {
    fn sample(&self, t: f32) -> f32 {
        t
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
        self.elapsed += dt;
        if self.elapsed >= self.duration {
            self.state = AnimationState::Completed;
        }
        // TODO(Wave N): Keyframe interpolation. The current implementation
        // advances the clock and flips to `Completed` on duration reach but
        // does not yet sample/interpolate `keyframes` or write the
        // interpolated `StyleProperty` back to the owning field. Easing
        // (`self.easing`) and the per-keyframe `Interpolation` mode are
        // likewise deferred. State-machine transitions for `Idle` / `Paused`
        // are also minimal — `tick` always advances the clock regardless of
        // `state`. Input validation (`DurationZero`, `KeyframeOutOfOrder`,
        // `InvalidPropertyKind`) is deferred to the construction boundary.
        Ok(())
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
    fn property(&self, kind: PropertyKind) -> StyleProperty;
    /// Fill/stroke colour — default: transparent black.
    fn color(&self) -> Color;
    /// Alpha opacity — default: `1.0`.
    fn opacity(&self) -> Opacity;
    /// Stroke line width — default: `0.0`.
    fn line_width(&self) -> LineWidth;
    /// Affine transform — default: identity.
    fn transform(&self) -> Mat4;
    /// WGSL shader effect — default: passthrough.
    fn effect(&self) -> ShaderStyle;
    /// Named animation lookup, if any.
    fn animation(&self, name: &str) -> Option<&Animation>;
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

/// Hardcoded-default [`Style`] implementation.
///
/// Returns the spec's per-instance defaults (ADR 005 / ADR 007):
/// - [`color`](Style::color): transparent black (`Color(0)`).
/// - [`opacity`](Style::opacity): `1.0`.
/// - [`line_width`](Style::line_width): `0.0`.
/// - [`transform`](Style::transform): identity [`Mat4`].
/// - [`effect`](Style::effect): passthrough [`ShaderStyle`] (empty WGSL
///   program, no uniforms, no bindings).
/// - [`animation`](Style::animation): `None` (no named animations).
///
/// The [`Style`] trait's methods are all required (no default bodies);
/// `DefaultStyle` is the canonical hardcoded-defaults implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultStyle;

impl Style for DefaultStyle {
    fn property(&self, kind: PropertyKind) -> StyleProperty {
        match kind {
            PropertyKind::Color => StyleProperty::Scalar(ScalarValue::Color(self.color())),
            PropertyKind::Opacity => StyleProperty::Scalar(ScalarValue::Opacity(self.opacity())),
            PropertyKind::LineWidth => {
                StyleProperty::Scalar(ScalarValue::LineWidth(self.line_width()))
            }
            PropertyKind::Transform => StyleProperty::Transform(self.transform()),
            PropertyKind::Shader => StyleProperty::Shader(self.effect()),
            // TODO(Wave N): Custom property defaults are module-declared;
            // until then, fall back to the colour default.
            PropertyKind::Custom(_) => StyleProperty::Scalar(ScalarValue::Color(self.color())),
        }
    }

    fn color(&self) -> Color {
        // RGBA8 transparent black.
        Color(0)
    }

    fn opacity(&self) -> Opacity {
        Opacity(1.0)
    }

    fn line_width(&self) -> LineWidth {
        LineWidth(0.0)
    }

    fn transform(&self) -> Mat4 {
        // Column-major identity.
        Mat4([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    fn effect(&self) -> ShaderStyle {
        // Passthrough: empty WGSL program, no uniforms, no bindings.
        ShaderStyle {
            program: WgslModule {
                source_hash: 0,
                pipeline_handle: 0,
            },
            entry_point: "main",
            uniforms: UniformBuffer { bytes: Vec::new() },
            bindings: Vec::new(),
        }
    }

    fn animation(&self, _name: &str) -> Option<&Animation> {
        None
    }
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
///
/// All [`Theme`] methods are required (no default bodies). [`DefaultTheme`]
/// provides the canonical hardcoded-defaults implementation; concrete
/// theme-table compilation (binary data-section preset blobs, §7.6) lands
/// in a later wave.
pub trait Theme {
    /// Look up a named preset.
    fn preset(&self, name: &str) -> OwnedStyle;
    /// The default preset.
    fn default(&self) -> OwnedStyle;
    /// Enumerate available preset names.
    fn names(&self) -> &[&'static str];
}

/// Hardcoded-default [`Theme`] implementation.
///
/// Returns an [`OwnedStyle`] populated with [`DefaultStyle`] defaults for
/// every preset lookup, and exposes a single preset name `"default"`.
/// Concrete theme-table compilation (binary data-section preset blobs,
/// §7.6) lands in a later wave.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultTheme;

impl Theme for DefaultTheme {
    fn preset(&self, _name: &str) -> OwnedStyle {
        let s = DefaultStyle;
        OwnedStyle {
            color: s.color(),
            opacity: s.opacity(),
            line_width: s.line_width(),
            transform: s.transform(),
            effect: s.effect(),
            animations: Vec::new(),
        }
    }

    fn default(&self) -> OwnedStyle {
        self.preset("default")
    }

    fn names(&self) -> &[&'static str] {
        &["default"]
    }
}

// ---------------------------------------------------------------------------
// Wave 3 tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_returns_spec_defaults() {
        let s = DefaultStyle;
        assert_eq!(s.color(), Color(0));
        assert_eq!(s.opacity(), Opacity(1.0));
        assert_eq!(s.line_width(), LineWidth(0.0));
        assert_eq!(
            s.transform(),
            Mat4([
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ])
        );
        let effect = s.effect();
        assert_eq!(effect.program.source_hash, 0);
        assert_eq!(effect.program.pipeline_handle, 0);
        assert_eq!(effect.entry_point, "main");
        assert!(effect.uniforms.bytes.is_empty());
        assert!(effect.bindings.is_empty());
        assert!(s.animation("anything").is_none());
    }

    #[test]
    fn default_style_property_round_trips_known_kinds() {
        let s = DefaultStyle;
        assert!(matches!(
            s.property(PropertyKind::Color),
            StyleProperty::Scalar(ScalarValue::Color(Color(0)))
        ));
        assert!(matches!(
            s.property(PropertyKind::Opacity),
            StyleProperty::Scalar(ScalarValue::Opacity(Opacity(1.0)))
        ));
        assert!(matches!(
            s.property(PropertyKind::LineWidth),
            StyleProperty::Scalar(ScalarValue::LineWidth(LineWidth(0.0)))
        ));
        assert!(matches!(
            s.property(PropertyKind::Transform),
            StyleProperty::Transform(_)
        ));
        assert!(matches!(
            s.property(PropertyKind::Shader),
            StyleProperty::Shader(_)
        ));
        // Custom kinds fall back to the colour default (TODO-noted).
        assert!(matches!(
            s.property(PropertyKind::Custom(7)),
            StyleProperty::Scalar(ScalarValue::Color(Color(0)))
        ));
    }

    #[test]
    fn default_theme_returns_default_style_defaults() {
        let theme = DefaultTheme;
        let style = theme.default();
        let s = DefaultStyle;
        assert_eq!(style.color, s.color());
        assert_eq!(style.opacity, s.opacity());
        assert_eq!(style.line_width, s.line_width());
        assert_eq!(style.transform, s.transform());
        assert_eq!(style.effect.program.source_hash, 0);
        assert_eq!(style.effect.program.pipeline_handle, 0);
        assert_eq!(style.effect.entry_point, "main");
        assert!(style.effect.uniforms.bytes.is_empty());
        assert!(style.effect.bindings.is_empty());
        assert!(style.animations.is_empty());
    }

    #[test]
    fn default_theme_preset_ignores_unknown_name() {
        let theme = DefaultTheme;
        let preset = theme.preset("nonexistent");
        // Presets resolve to DefaultStyle defaults regardless of name.
        assert_eq!(preset.color, Color(0));
        assert_eq!(preset.opacity, Opacity(1.0));
    }

    #[test]
    fn default_theme_names_exposes_default() {
        let theme = DefaultTheme;
        assert_eq!(theme.names(), &["default"]);
    }

    fn make_animation(duration: Duration) -> Animation {
        Animation {
            property: PropertyKind::Opacity,
            keyframes: Vec::new(),
            duration,
            easing: Box::new(LinearEasing),
            elapsed: Duration::ZERO,
            state: AnimationState::Running,
        }
    }

    #[test]
    fn animation_tick_advances_elapsed() {
        let mut anim = make_animation(Duration::from_millis(100));
        anim.tick(Duration::from_millis(40)).unwrap();
        assert_eq!(anim.elapsed, Duration::from_millis(40));
        assert_eq!(anim.state, AnimationState::Running);
    }

    #[test]
    fn animation_tick_completes_on_duration_reach() {
        let mut anim = make_animation(Duration::from_millis(100));
        anim.tick(Duration::from_millis(100)).unwrap();
        assert_eq!(anim.elapsed, Duration::from_millis(100));
        assert_eq!(anim.state, AnimationState::Completed);
    }

    #[test]
    fn animation_tick_overshoot_completes() {
        let mut anim = make_animation(Duration::from_millis(100));
        anim.tick(Duration::from_millis(150)).unwrap();
        assert_eq!(anim.elapsed, Duration::from_millis(150));
        assert_eq!(anim.state, AnimationState::Completed);
    }
}

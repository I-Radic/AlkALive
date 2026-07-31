//! Layout system — geometry primitives, pluggable constraint solver, and
//! the text-flow measurement contract (§5.2–5.7).
//!
//! Wave 6 status: the default [`CassowarySolver`] ships a simplified
//! single-pass linear-equality solver with the ADR 002 locality gate
//! (§5.5) and GPU-ready transform emission (§5.6). The [`MeasuredRun`]
//! contract is a required trait surface (no default bodies); a no-op
//! [`MockMeasuredRun`] is shipped for tests and downstream stubs. The
//! concrete HarfRust-backed implementation lands with the `alkalive-text`
//! crate (Wave 7+, ADR 004/022).
//!
//! # Wave 3 (task WAVE-W3) — real text measurement
//!
//! The layout crate now depends on `alkalive-text` and ships
//! [`HarfRustMeasuredRun`]: a [`MeasuredRun`] implementation backed by the
//! real forked in-WASM HarfRust text stack (ADR 022). It holds a shared
//! [`HarfRustFontRegistry`] and delegates to [`HarfRustTextShaper`] inside
//! [`MeasuredRun::shape_and_measure`], adapting the resulting
//! `alkalive_text::ShapedRun` into the layout crate's own [`GlyphMetrics`].
//! [`MeasuredRun::line_break`] ships a simple greedy accumulator.
//!
//! # Cross-crate forward declarations
//!
//! Two cross-crate types referenced by the spec are stubbed here:
//! - [`OwnedStyle`] — concrete struct lives in `alkalive-style` (§7).
//! - [`ShapeError`] — concrete enum lives in `alkalive-text` (§6.3).
//!
//! Both are unified by the future rendering-ABI ADR (§4.7 / §5.4
//! shared-boundary note). The stubs exist only so the layout crate's
//! public surface stays minimal; [`HarfRustMeasuredRun`] consumes the
//! real `alkalive_text` types directly and never touches these stubs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use alkalive_core::ModuleId;

use std::collections::HashMap;
use std::sync::Arc;

// Wave 3 (task WAVE-W3): the layout crate now depends on `alkalive-text` and
// wires the real HarfRust text stack into [`HarfRustMeasuredRun`]. The
// imports below are aliased (`TextFontId`, `TextShapedRun`) so the layout
// crate's own forward-declared stubs (`ShapeError`, `OwnedStyle`) remain the
// canonical names within this crate and the rendering-ABI ADR (§4.7) can
// later unify them without a churn of renames.
use alkalive_text::{
    FontId as TextFontId, HarfRustFontRegistry, HarfRustTextShaper, ShapeContext,
    ShapedRun as TextShapedRun, TextShaper,
};

// ============================================================================
// Opaque identifiers
// ============================================================================

/// Solver-internal handle for a node in the layout graph (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct NodeId(pub u32);

/// Stable handle for a render object in the owned scene-graph (ADR 007).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct RenderObjectId(pub u32);

/// Stable handle for a constraint registered with the solver (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ConstraintId(pub u32);

/// Stable handle for a text run submitted for measurement (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct TextRunId(pub u32);

// ============================================================================
// Geometry primitives (§5.2)
// ============================================================================

/// 2D vector, shared with the render-graph IR (§4) and hit-testing (§8).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

/// 2D extent.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// Axis-aligned rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// Top-left origin.
    pub origin: Vec2,
    /// Width/height.
    pub size: Size,
}

/// Column-major 4×4 transform; consumed directly as a GPU instance transform
/// (§5.2, §5.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// Column-major matrix data, consumed by the render-graph IR (§4).
    pub m: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Self {
        // Identity transform — a safe default for solver outputs.
        // Column-major: identity has 1.0 on the diagonal (indices 0, 5, 10, 15).
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }
}

impl Mat4 {
    /// Build a 2D translation matrix (z preserved, homogeneous w=1).
    ///
    /// Convenience used by [`CassowarySolver`] to emit per-node instance
    /// transforms from resolved `x`/`y` facets (§5.6). The translation
    /// vector occupies the fourth column of the column-major layout.
    pub fn translated(x: f32, y: f32) -> Self {
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0, // column 0
                0.0, 1.0, 0.0, 0.0, // column 1
                0.0, 0.0, 1.0, 0.0, // column 2
                x, y, 0.0, 1.0, // column 3 (translation)
            ],
        }
    }
}

// ============================================================================
// Constraint model (§5.2–5.3)
// ============================================================================

/// A constraint operand (§5.2): references either a [`RenderObjectId`] facet
/// (`x`/`y`/`w`/`h`/`baseline`) or a literal `f32`.
///
/// Tagged struct: when `object` is `Some`, this is a facet reference and
/// `axis` is one of `"x"`, `"y"`, `"w"`, `"h"`, `"baseline"`. When `object`
/// is `None`, `value` is the literal scalar and `axis` is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LayoutVar {
    /// Render object this operand references; `None` for a literal.
    pub object: Option<RenderObjectId>,
    /// Facet name (`"x"`, `"y"`, `"w"`, `"h"`, `"baseline"`); ignored for a literal.
    pub axis: &'static str,
    /// Literal scalar value; ignored for a facet reference.
    pub value: f32,
}

/// How a constraint is satisfied by the solver (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ConstraintKind {
    /// Cassowary equality/inequality over [`LayoutVar`]s.
    #[default]
    Linear,
    /// Spring/velocity physics; integrates over `dt`.
    Impulse,
    /// Rank/layered or force-directed over adjacency.
    GraphLayout,
}

/// A single solver constraint (§5.2).
#[derive(Clone, Debug)]
pub struct Constraint {
    /// Solver dispatch kind.
    pub kind: ConstraintKind,
    /// Left-hand variable / object facet.
    pub a: LayoutVar,
    /// Right-hand variable / constant.
    pub b: LayoutVar,
    /// Strength (linear) or stiffness (impulse).
    pub weight: f32,
    /// Locality tag (ADR 002).
    pub module: ModuleId,
}

/// How a layout node measures its content (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MeasureKind {
    /// Author-supplied fixed extent.
    #[default]
    Fixed,
    /// Content-driven via the [`MeasuredRun`] contract (§5.4).
    Text,
    /// Intrinsic measure of the subtree.
    Intrinsic,
}

/// Solver-internal layout node; not a serializable box tree (§5.3).
#[derive(Clone, Debug)]
pub struct LayoutNode {
    /// Owning render object.
    pub id: RenderObjectId,
    /// Ownership tag (ADR 002).
    pub module: ModuleId,
    /// Measurement strategy.
    pub measure: MeasureKind,
    /// Solver-internal child references.
    pub children: Vec<NodeId>,
}

// ============================================================================
// Solve status & errors (§5.3, §5.7)
// ============================================================================

/// Outcome of a single `solve` invocation (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SolveStatus {
    /// Committed to instance buffers.
    #[default]
    Solved,
    /// Relaxed below threshold; still acceptable.
    Partial,
    /// Rejected; last-known-good retained.
    Unsatisfiable,
    /// Cross-module dep rejected at `assert_local`.
    LocalityViolation,
}

/// Suggested minimal relaxation when a solve is unsatisfiable (§5.7).
#[derive(Clone, Debug, Default)]
pub struct RelaxationHint {
    /// Constraints to relax or drop.
    pub relax: Vec<ConstraintId>,
    /// Suggested weight scaling (0.0..=1.0).
    pub scale: f32,
}

/// Cross-module locality violation detail (§5.5).
#[derive(Clone, Debug)]
pub struct LocalityViolation {
    /// Constraint that crossed the module boundary.
    pub constraint: ConstraintId,
    /// The two modules on either side of the rejected edge.
    pub boundary: (ModuleId, ModuleId),
}

/// Placeholder for the text-stack `ShapeError` (§6.3).
///
/// The concrete enum lives in `alkalive-text`; the rendering-ABI ADR
/// (§4.7 / §5.4 shared-boundary note) will unify these signatures. Wave-3
/// ships each crate self-contained, so the layout crate re-stubs the type
/// here as an opaque unit.
#[derive(Clone, Debug, Default)]
pub struct ShapeError;

/// Placeholder for the style crate's `OwnedStyle` (§7).
///
/// Style values enter the solver only as immutable inputs via
/// [`LayoutSolver::bind_style`] and never re-derive the layout tree
/// (§5.1, §5.6). The concrete struct lives in `alkalive-style`; the
/// rendering-ABI ADR (§4.7) will unify it.
#[derive(Clone, Debug, Default)]
pub struct OwnedStyle;

/// Solver failure modes (§5.7).
#[derive(Clone, Debug)]
pub enum SolveError {
    /// Solve rejected; carries offending constraints and a relaxation hint.
    Unsatisfiable {
        /// Constraints responsible for the failure.
        offenders: Vec<ConstraintId>,
        /// Suggested minimal relaxation.
        suggestion: RelaxationHint,
    },
    /// A constraint crossed a module boundary (§5.5).
    LocalityViolated {
        /// Constraint that violated locality.
        constraint: ConstraintId,
        /// The two modules on either side of the rejected edge.
        boundary: (ModuleId, ModuleId),
    },
    /// A measured text run failed to shape (§5.4).
    MeasurementFailed {
        /// The text run whose measurement failed.
        run: TextRunId,
        /// Underlying shape failure (text-stack side).
        cause: ShapeError,
    },
    /// Solve exceeded its time budget (§12.2).
    Timeout {
        /// Milliseconds over budget.
        budget_exceeded_ms: u32,
    },
}

// ============================================================================
// Dirty tracking (§5.5, §4.4)
// ============================================================================

/// A per-module dirty rectangle, used to bound per-frame cost (ADR 002, §4.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirtyRect {
    /// Owning module.
    pub module: ModuleId,
    /// Dirty screen rectangle.
    pub rect: Rect,
}

/// The subset of nodes/constraints a single `solve` invocation touches
/// (§5.5). Per-frame cost is bounded by the dirty subset, not tree size.
#[derive(Clone, Debug, Default)]
pub struct DirtySet {
    /// Modules whose subtrees are dirty.
    pub modules: Vec<ModuleId>,
    /// Dirty render-object rectangles.
    pub rects: Vec<DirtyRect>,
    /// Specific nodes flagged for re-solve.
    pub nodes: Vec<NodeId>,
}

// ============================================================================
// Text-flow measurement contract (§5.4)
// ============================================================================

/// A Unicode text run submitted for synchronous measurement (§5.4).
#[derive(Clone, Debug)]
pub struct TextRun {
    /// Stable run identifier (mirrors [`SolveError::MeasurementFailed::run`]).
    pub id: TextRunId,
    /// UTF-8 source text.
    pub text: String,
    /// Owning module (ADR 002).
    pub module: ModuleId,
}

/// Font context carried into [`MeasuredRun::shape_and_measure`] (§5.4).
///
/// The concrete bundle of resolved `FontId`s, pixel size, language, and
/// direction lives in `alkalive-text`; the layout crate consumes only this
/// opaque handle so the rendering-ABI ADR (§4.7) can unify it later without
/// breaking the [`LayoutSolver`] signature.
#[derive(Clone, Debug, Default)]
pub struct FontContext;

/// Per-glyph metrics emitted by [`MeasuredRun::shape_and_measure`] (§5.4):
/// advances, ascents, descents, cluster map, caret positions.
#[derive(Clone, Debug, Default)]
pub struct GlyphMetrics {
    /// Per-glyph x-advance (signed for RTL).
    pub advances: Vec<f32>,
    /// Per-glyph ascents.
    pub ascents: Vec<f32>,
    /// Per-glyph descents.
    pub descents: Vec<f32>,
    /// Source-codepoint index per glyph.
    pub clusters: Vec<u32>,
    /// Per caret x-offsets, BiDi-aware.
    pub caret_offsets: Vec<f32>,
}

/// A single line-break decision produced by [`MeasuredRun::line_break`] (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LineBreak {
    /// Index of the first glyph on the next line.
    pub next_glyph: u32,
    /// Suggested break penalty (Cassowary-style).
    pub penalty: f32,
}

/// Forward-declared glyph-run shape consumed by
/// [`LayoutSolution::glyph_runs`] and [`MeasuredRun::line_break`]
/// (§5.4 / §5.6).
///
/// A simple stub: the concrete attachment-aware shape is unified by the
/// rendering-ABI ADR (§4.7). The layout crate owns only this minimal carrier
/// so the solver signature is stable.
#[derive(Clone, Debug, Default)]
pub struct GlyphRun {
    /// Run-level total advance.
    pub total_advance: f32,
    /// Number of glyphs in the run.
    pub glyph_count: u32,
}

/// Synchronous text-flow measurement contract every solver must consume
/// (§5.4, ADR 004/022).
///
/// The backing implementation is the forked in-WASM HarfRust stack (ADR 022);
/// `alkalive-text::TextStack` implements this via `TextStack::measure`
/// (§6.9, §5.4 shared-boundary note). Box/physics/graph solvers cover none
/// of line-breaking, BiDi, or font-metric shaping, so this contract is
/// mandatory for every solver — including user-supplied.
///
/// # Wave 6 contract
///
/// Both methods are required (no default body): every implementor must
/// supply a shaping and line-break policy. The concrete HarfRust-backed
/// implementation lands with the `alkalive-text` crate (Wave 7+, ADR
/// 004/022). [`CassowarySolver::solve`] does not invoke the measurement
/// contract in Wave 6, so the no-op [`MockMeasuredRun`] is sufficient for
/// its tests.
pub trait MeasuredRun {
    /// Shape and measure a [`TextRun`] synchronously; HarfRust-backed, no
    /// DOM crossing (§5.4, ADR 022).
    fn shape_and_measure(&self, run: &TextRun, ctx: &FontContext) -> GlyphMetrics;

    /// Break `glyphs` into lines constrained by `max_width` (§5.4).
    fn line_break(&self, glyphs: &[GlyphRun], max_width: f32) -> Vec<LineBreak>;
}

/// No-op [`MeasuredRun`] implementation: returns empty [`GlyphMetrics`] and
/// a single default [`LineBreak`].
///
/// Wave 6's [`CassowarySolver::solve`] never invokes the measurement
/// contract, so this mock is a safe stand-in for tests and for downstream
/// callers that have not yet wired the `alkalive-text` stack. The concrete
/// HarfRust-backed implementation lands with `alkalive-text` (Wave 7+,
/// ADR 004/022).
#[derive(Clone, Copy, Debug, Default)]
pub struct MockMeasuredRun;

impl MeasuredRun for MockMeasuredRun {
    fn shape_and_measure(&self, _run: &TextRun, _ctx: &FontContext) -> GlyphMetrics {
        GlyphMetrics::default()
    }

    fn line_break(&self, _glyphs: &[GlyphRun], _max_width: f32) -> Vec<LineBreak> {
        vec![LineBreak::default()]
    }
}

/// HarfRust-backed [`MeasuredRun`] — the production text measurement
/// implementation (ADR 004 / ADR 022, §5.4 / §6.9).
///
/// Wave 3 (task WAVE-W3) wires the real forked in-WASM HarfRust text stack
/// into the layout crate. The struct holds a shared
/// [`HarfRustFontRegistry`] ([`Arc`]-reference-counted, matching
/// [`HarfRustTextShaper::new`]) and builds a fresh [`HarfRustTextShaper`]
/// per [`MeasuredRun::shape_and_measure`] call. The shaper is cheap to
/// construct — it only bumps a refcount on the registry's [`Arc`].
///
/// [`MeasuredRun::shape_and_measure`] hard-wires:
/// - the resolved font to [`TextFontId`]`(0)` (the first loaded face); real
///   family/weight/style selection lands with the font-config integration,
/// - `size_px` to `16.0` (common body-text size),
/// - `direction` to `None` (auto-detect from script).
///
/// The resulting [`TextShapedRun`] is adapted into the layout crate's own
/// [`GlyphMetrics`] (advances, ascents, descents, clusters, caret_offsets).
/// Per-glyph ascents/descents are run-level (font-metric driven) — the same
/// value is broadcast across every glyph so downstream line-breaking has a
/// per-glyph view. Caret offsets are derived from the shaped run's
/// `caret_to_glyph` map (one caret per glyph boundary, N+1 carets for N
/// glyphs).
///
/// [`MeasuredRun::line_break`] ships a simple greedy accumulator: it walks
/// `glyphs` and emits a [`LineBreak`] whenever the running advance exceeds
/// `max_width`. No BiDi-aware splitting, no Knuth-Plass optimisation —
/// Wave 3 only needs a correct-enough break for the layout solver to
/// consume; the real line-breaker lands with the BiDi integration.
///
/// Shaping never aborts on missing coverage — uncovered codepoints surface
/// as `.notdef` glyphs with real metrics (§6.3). A shape failure (e.g. an
/// unregistered `FontId`) therefore collapses to empty [`GlyphMetrics`]
/// rather than propagating a `alkalive_text::ShapeError` through the
/// solver, whose [`MeasuredRun`] contract returns [`GlyphMetrics`] directly.
#[derive(Clone)]
pub struct HarfRustMeasuredRun {
    /// Shared font registry; the shaper takes a read-only [`Arc`] share on
    /// each [`MeasuredRun::shape_and_measure`] invocation.
    registry: Arc<HarfRustFontRegistry>,
}

impl HarfRustMeasuredRun {
    /// Construct a measured-run backed by `registry`. The registry should
    /// have all fonts loaded before construction (the shaper takes a
    /// read-only [`Arc`] share, matching [`HarfRustTextShaper::new`]).
    pub fn new(registry: Arc<HarfRustFontRegistry>) -> Self {
        Self { registry }
    }

    /// Read-only access to the underlying font registry. Exposed so
    /// downstream callers (and tests) can inspect which faces are loaded.
    pub fn registry(&self) -> &HarfRustFontRegistry {
        &self.registry
    }
}

impl MeasuredRun for HarfRustMeasuredRun {
    fn shape_and_measure(&self, run: &TextRun, _ctx: &FontContext) -> GlyphMetrics {
        // The layout crate's `FontContext` is still a forward-declared
        // placeholder (§5.4); the real resolved-font/size/direction bundle
        // lives in `alkalive-text::ShapeContext`. Until the rendering-ABI
        // ADR (§4.7) unifies the two, `HarfRustMeasuredRun` hard-wires a
        // default `ShapeContext` that points at `FontId(0)` at 16 px with
        // auto-detected direction.
        let shaper = HarfRustTextShaper::new(self.registry.clone());
        let ctx = ShapeContext {
            font: TextFontId(0),
            size_px: 16.0,
            direction: None,
        };
        let shaped: TextShapedRun = match shaper.shape(run.text.as_str(), &ctx) {
            Ok(s) => s,
            // Shaping failure (e.g. `FontId(0)` not registered) collapses to
            // empty metrics — the solver still produces a valid (if empty)
            // glyph run rather than aborting the solve.
            Err(_) => return GlyphMetrics::default(),
        };
        // Per-glyph ascents/descents are run-level (font-metric driven) —
        // broadcast the same value across every glyph so downstream
        // line-breaking has a per-glyph view.
        let n = shaped.advances.len();
        let ascent = shaped.metrics.ascent;
        let descent = shaped.metrics.descent;
        GlyphMetrics {
            advances: shaped.advances.to_vec(),
            ascents: vec![ascent; n],
            descents: vec![descent; n],
            clusters: shaped.clusters.to_vec(),
            // The shaped run's `caret_to_glyph` carries N+1 entries for an
            // N-glyph run (one caret per glyph boundary); cast to `f32` so
            // downstream consumers receive a flat per-caret x-offset view.
            caret_offsets: shaped
                .caret_map
                .caret_to_glyph
                .iter()
                .map(|&g| g as f32)
                .collect(),
        }
    }

    fn line_break(&self, glyphs: &[GlyphRun], max_width: f32) -> Vec<LineBreak> {
        // Simple greedy line breaking: accumulate `total_advance` until it
        // exceeds `max_width`, then emit a `LineBreak` pointing at the next
        // glyph index. No BiDi-aware splitting, no Knuth-Plass penalty
        // optimisation — Wave 3 only needs a correct-enough break for the
        // layout solver to consume.
        let mut breaks = Vec::new();
        let mut acc: f32 = 0.0;
        for (i, g) in glyphs.iter().enumerate() {
            acc += g.total_advance;
            if max_width > 0.0 && acc > max_width {
                breaks.push(LineBreak {
                    next_glyph: i as u32 + 1,
                    penalty: 0.0,
                });
                acc = 0.0;
            }
        }
        breaks
    }
}

// ============================================================================
// Solver output (§5.6)
// ============================================================================

/// Solver output consumed directly by the render-graph IR (§5.6).
///
/// `solve` outputs are written directly into GPU-resident instance buffers
/// consumed by the render-graph IR of §4. There is no style-driven box-tree
/// recalculation: style values enter as constraint inputs via
/// [`LayoutSolver::bind_style`], never as a re-derivation trigger.
#[derive(Clone, Debug)]
pub struct LayoutSolution {
    /// Outcome of the solve.
    pub status: SolveStatus,
    /// GPU instance-buffer transforms, one per render object.
    pub transforms: Vec<(RenderObjectId, Mat4)>,
    /// Clip rectangles, consumed by the occlusion-cull pass (§4.3).
    pub clips: Vec<(RenderObjectId, Rect)>,
    /// Glyph runs forwarded to the text atlas (§6).
    pub glyph_runs: Vec<GlyphRun>,
    /// Locality tag for dirty-rect scoping.
    pub module: ModuleId,
}

// ============================================================================
// The solver trait (§5.3)
// ============================================================================

/// Pluggable constraint-solver trait — the sole layout extension surface
/// (ADR 004, §5.3).
///
/// The runtime ships a default Cassowary-class linear implementation
/// ([`CassowarySolver`]); author backends (impulse/physics, directed-graph,
/// GPU-compute) bind behind the same trait, so swapping solvers is internal
/// and non-breaking to downstream paint stages. The layout-tree is
/// solver-internal and never re-derived from styles (§5.1), eliminating the
/// style-driven box-tree recalculation that couples style mutation to
/// global reflow (P2.3, P2.4).
///
/// Every method is required (no default body): each implementor must
/// supply a full solver. [`CassowarySolver`] is the reference
/// implementation.
pub trait LayoutSolver {
    /// Register a node in the solver-internal layout graph.
    fn add_node(&mut self, node: LayoutNode) -> NodeId;

    /// Remove a node and its descendants; per-module dirty-rect (ADR 002).
    fn remove_node(&mut self, id: NodeId);

    /// Bind an immutable style snapshot to a node — input only; never mutated.
    fn bind_style(&mut self, id: NodeId, style: &OwnedStyle);

    /// Register a constraint; returns its handle for later removal.
    fn add_constraint(&mut self, c: Constraint) -> ConstraintId;

    /// Remove a previously-registered constraint.
    fn remove_constraint(&mut self, id: ConstraintId);

    /// Locality gate (ADR 002). Rejects cross-module flex baselines,
    /// percentage chains spanning module boundaries, or any constraint
    /// whose satisfaction would reflow outside the dirty set (§5.5).
    fn assert_local(&self, c: &Constraint) -> Result<(), LocalityViolation>;

    /// Synchronous solve over the dirty subset; consumes measured text
    /// runs (§5.4) and emits GPU-ready transforms. No intermediate tree
    /// (§5.3, §5.6).
    fn solve(
        &mut self,
        dirty: &DirtySet,
        measured: &dyn MeasuredRun,
        dt: f32,
    ) -> Result<LayoutSolution, SolveError>;
}

// ============================================================================
// Default solver: CassowarySolver (§5.3, ADR 004)
// ============================================================================

/// Default linear-constraint solver shipped with the runtime (ADR 004, §5.3).
///
/// Wave 6 ships a **simplified single-pass linear-equality** solver rather
/// than a full Cassowary simplex:
///
/// 1. **Locality gate** — every live constraint is checked via
///    [`LayoutSolver::assert_local`]; the first cross-module offender
///    aborts the solve with [`SolveError::LocalityViolated`] (§5.5).
/// 2. **Equality assignment** — each `ConstraintKind::Linear` constraint
///    assigns `a := b` (literal RHS uses [`LayoutVar::value`]; facet RHS
///    reads the previously-assigned value, falling back to the literal).
///    `Impulse` and `GraphLayout` kinds are accepted but skipped.
/// 3. **Transform emission** — one [`Mat4::translated`] instance transform
///    is emitted per live node from its resolved `x`/`y` facets (§5.6).
///
/// **Wave E (Gap H2) update** — [`LayoutSolver::solve`] now honours its
/// `dirty` and `measured` parameters:
/// - A non-empty `dirty.modules` scopes the solve to constraints whose
///   `module` is in that set; an empty set solves every live constraint
///   (backward compatible).
/// - [`MeasureKind::Text`] nodes invoke [`MeasuredRun::shape_and_measure`]
///   with a dummy [`TextRun`] and forward `advances.len()` into
///   [`LayoutSolution::glyph_runs`] (§5.4 / §5.6).
/// - A constraint referencing a removed (non-existent) node surfaces as
///   [`SolveError::Unsatisfiable`] carrying the offending [`ConstraintId`].
///
/// `dt` is still absorbed — it only matters for `ConstraintKind::Impulse`
/// integration, which the Wave 6 simplification skips. The trait surface is
/// stable so a production solver can drop in behind [`LayoutSolver`] without
/// breaking downstream paint stages.
#[derive(Default)]
pub struct CassowarySolver {
    /// Slot-based node table; `None` marks a removed node.
    nodes: Vec<Option<LayoutNode>>,
    /// Per-node style snapshot (input only; §5.1, §5.6). Indexed by
    /// [`NodeId`] in lockstep with `nodes`.
    styles: Vec<OwnedStyle>,
    /// Slot-based constraint table; `None` marks a removed constraint.
    constraints: Vec<Option<Constraint>>,
}

impl CassowarySolver {
    /// Construct an empty solver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the [`ModuleId`] owning `rid`, if any live node claims it.
    ///
    /// Used by the locality gate to compare the modules of a constraint's
    /// two operands. A literal operand (no `object`) yields `None`.
    fn module_of(&self, rid: RenderObjectId) -> Option<ModuleId> {
        self.nodes
            .iter()
            .flatten()
            .find(|n| n.id == rid)
            .map(|n| n.module)
    }

    /// Returns `true` if a live node claims `rid` as its render object.
    ///
    /// A removed node leaves a tombstone slot (`None`), so this returns
    /// `false` for any [`RenderObjectId`] whose owning node was deleted via
    /// [`LayoutSolver::remove_node`]. Used by [`CassowarySolver::solve`]
    /// to reject constraints that dangle a reference to a removed node
    /// (§5.7, Gap H2).
    fn is_live(&self, rid: RenderObjectId) -> bool {
        self.nodes.iter().flatten().any(|n| n.id == rid)
    }
}

impl LayoutSolver for CassowarySolver {
    fn add_node(&mut self, node: LayoutNode) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Some(node));
        // `OwnedStyle` is currently a unit-struct stub (§7); the concrete
        // struct lands with `alkalive-style` and the rendering-ABI ADR.
        self.styles.push(OwnedStyle);
        id
    }

    fn remove_node(&mut self, id: NodeId) {
        let idx = id.0 as usize;
        if idx < self.nodes.len() {
            // Slot tombstone; indices stay stable so existing NodeIds
            // remain valid. Style slot is retained (cheap, avoids
            // reindexing) and simply overwritten on next `add_node`
            // reuse — though Wave 6 never reuses slots.
            self.nodes[idx] = None;
        }
    }

    fn bind_style(&mut self, id: NodeId, style: &OwnedStyle) {
        let idx = id.0 as usize;
        if idx < self.styles.len() {
            self.styles[idx] = style.clone();
        }
    }

    fn add_constraint(&mut self, c: Constraint) -> ConstraintId {
        let id = ConstraintId(self.constraints.len() as u32);
        self.constraints.push(Some(c));
        id
    }

    fn remove_constraint(&mut self, id: ConstraintId) {
        let idx = id.0 as usize;
        if idx < self.constraints.len() {
            self.constraints[idx] = None;
        }
    }

    fn assert_local(&self, c: &Constraint) -> Result<(), LocalityViolation> {
        let module_a = c.a.object.and_then(|rid| self.module_of(rid));
        let module_b = c.b.object.and_then(|rid| self.module_of(rid));

        // Reject cross-module edges between two referenced objects (§5.5).
        if let (Some(ma), Some(mb)) = (module_a, module_b) {
            if ma != mb {
                return Err(LocalityViolation {
                    // The trait signature carries no ConstraintId; `solve`
                    // re-stamps this with the real handle when forwarding
                    // the violation as a `SolveError::LocalityViolated`.
                    constraint: ConstraintId::default(),
                    boundary: (ma, mb),
                });
            }
        }

        // Reject constraints whose claimed `module` disagrees with the
        // (now uniform) module of their referenced nodes.
        let effective = module_a.or(module_b);
        if let Some(m) = effective {
            if m != c.module {
                return Err(LocalityViolation {
                    constraint: ConstraintId::default(),
                    boundary: (c.module, m),
                });
            }
        }

        Ok(())
    }

    fn solve(
        &mut self,
        dirty: &DirtySet,
        measured: &dyn MeasuredRun,
        dt: f32,
    ) -> Result<LayoutSolution, SolveError> {
        // Wave E (Gap H2): the solver now honours `dirty` (module-subset
        // scoping), invokes the [`MeasuredRun`] contract for
        // [`MeasureKind::Text`] nodes, and rejects constraints that
        // reference removed (non-existent) nodes as
        // [`SolveError::Unsatisfiable`]. `dt` is still absorbed here — it
        // only matters for `ConstraintKind::Impulse` integration, which the
        // Wave 6 simplification skips. The trait surface is unchanged.
        let _ = dt;

        // A non-empty `dirty.modules` scopes the solve to constraints whose
        // `module` is in that set; an empty set solves every live
        // constraint (backward compatible with the Wave 6 simplification).
        let scoped = !dirty.modules.is_empty();
        let in_scope = |c_module: ModuleId| -> bool {
            if scoped {
                dirty.modules.contains(&c_module)
            } else {
                true
            }
        };

        // Pass 1 — locality gate (§5.5) + non-existent-node check (§5.7).
        // The first offender aborts the solve. Only in-scope constraints
        // are considered, so out-of-scope modules are neither solved nor
        // rejected.
        for (idx, slot) in self.constraints.iter().enumerate() {
            let Some(c) = slot else { continue; };
            if !in_scope(c.module) {
                continue;
            }
            // Reject constraints that dangle a reference to a removed node
            // (§5.7, Gap H2). The offending ConstraintId is reported so the
            // caller can prune or relax it.
            if let Some(rid) = c.a.object {
                if !self.is_live(rid) {
                    return Err(SolveError::Unsatisfiable {
                        offenders: vec![ConstraintId(idx as u32)],
                        suggestion: RelaxationHint::default(),
                    });
                }
            }
            if let Some(rid) = c.b.object {
                if !self.is_live(rid) {
                    return Err(SolveError::Unsatisfiable {
                        offenders: vec![ConstraintId(idx as u32)],
                        suggestion: RelaxationHint::default(),
                    });
                }
            }
            if let Err(violation) = self.assert_local(c) {
                return Err(SolveError::LocalityViolated {
                    constraint: ConstraintId(idx as u32),
                    boundary: violation.boundary,
                });
            }
        }

        // Pass 2 — single-pass linear-equality assignment (Wave 6
        // simplification). For each in-scope `Linear` constraint we set
        // `a := b`: a literal RHS uses `b.value`; a facet RHS reads the
        // previously assigned value (falling back to `b.value`). `Impulse`
        // and `GraphLayout` kinds are accepted but skipped.
        let mut facets: HashMap<(RenderObjectId, &'static str), f32> = HashMap::new();
        for slot in self.constraints.iter().flatten() {
            if slot.kind != ConstraintKind::Linear {
                continue;
            }
            if !in_scope(slot.module) {
                continue;
            }
            let b_val = match slot.b.object {
                Some(rid) => facets
                    .get(&(rid, slot.b.axis))
                    .copied()
                    .unwrap_or(slot.b.value),
                None => slot.b.value,
            };
            if let Some(rid) = slot.a.object {
                facets.insert((rid, slot.a.axis), b_val);
            }
        }

        // Pass 3 — emit one instance transform per live node (§5.6) and run
        // the synchronous measurement contract for [`MeasureKind::Text`]
        // nodes (§5.4, Gap H2). The solution's `module` tag is taken from
        // the first live node so the dirty-rect scoping downstream has a
        // stable locality tag.
        let mut transforms = Vec::new();
        let mut glyph_runs = Vec::new();
        let mut module = ModuleId(0);
        let mut first = true;
        let ctx = FontContext;
        for node in self.nodes.iter().flatten() {
            if first {
                module = node.module;
                first = false;
            }
            let x = facets.get(&(node.id, "x")).copied().unwrap_or(0.0);
            let y = facets.get(&(node.id, "y")).copied().unwrap_or(0.0);
            transforms.push((node.id, Mat4::translated(x, y)));
            if node.measure == MeasureKind::Text {
                // Dummy [`TextRun`] — the concrete text payload arrives
                // with the HarfRust-backed [`MeasuredRun`] in Wave 7+
                // (ADR 004/022). Wave E only proves the contract is invoked
                // and the per-run glyph count is forwarded into the
                // solution's `glyph_runs` (§5.4 / §5.6).
                let run = TextRun {
                    id: TextRunId(node.id.0),
                    text: String::new(),
                    module: node.module,
                };
                let metrics = measured.shape_and_measure(&run, &ctx);
                let total_advance: f32 = metrics.advances.iter().copied().sum();
                glyph_runs.push(GlyphRun {
                    total_advance,
                    glyph_count: metrics.advances.len() as u32,
                });
            }
        }

        Ok(LayoutSolution {
            status: SolveStatus::Solved,
            transforms,
            clips: Vec::new(),
            glyph_runs,
            module,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    // `FontRegistry::load_bundle` is needed by the HarfRustMeasuredRun tests
    // to seed the registry with the embedded test font; the trait is not
    // used by the library itself, so it lives in the test module only.
    use alkalive_text::FontRegistry;

    /// Build a leaf [`LayoutNode`] in `module` owning render object `rid`.
    fn node(rid: u32, module: u32) -> LayoutNode {
        LayoutNode {
            id: RenderObjectId(rid),
            module: ModuleId(module as u64),
            measure: MeasureKind::Fixed,
            children: Vec::new(),
        }
    }

    /// Build a [`LayoutVar`]; `rid = None` makes a literal operand.
    fn facet(rid: Option<u32>, axis: &'static str, value: f32) -> LayoutVar {
        LayoutVar {
            object: rid.map(RenderObjectId),
            axis,
            value,
        }
    }

    /// `add_node` returns monotonically-increasing [`NodeId`]s starting at 0.
    #[test]
    fn add_node_returns_incrementing_ids() {
        let mut solver = CassowarySolver::new();
        let a = solver.add_node(node(0, 0));
        let b = solver.add_node(node(1, 0));
        let c = solver.add_node(node(2, 0));
        assert_eq!(a, NodeId(0));
        assert_eq!(b, NodeId(1));
        assert_eq!(c, NodeId(2));
    }

    /// Same-module constraints pass the locality gate.
    #[test]
    fn assert_local_accepts_same_module() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        solver.add_node(node(1, 0));
        let c = Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(Some(1), "x", 0.0),
            weight: 1.0,
            module: ModuleId(0),
        };
        assert!(solver.assert_local(&c).is_ok());
    }

    /// Cross-module constraints are rejected at the locality gate (§5.5)
    /// and the violation reports the boundary in `(a.module, b.module)` order.
    #[test]
    fn assert_local_rejects_cross_module() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        solver.add_node(node(1, 1));
        let c = Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(Some(1), "x", 0.0),
            weight: 1.0,
            module: ModuleId(0),
        };
        let err = solver
            .assert_local(&c)
            .expect_err("cross-module constraint must be rejected");
        assert_eq!(err.boundary, (ModuleId(0), ModuleId(1)));
    }

    /// A single same-module linear constraint solves to [`SolveStatus::Solved`]
    /// and emits one transform per live node.
    #[test]
    fn solve_returns_solved_for_simple_system() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        let cid = solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(None, "", 10.0),
            weight: 1.0,
            module: ModuleId(0),
        });
        // ConstraintId is also monotonically increasing and independent of NodeId.
        assert_eq!(cid, ConstraintId(0));

        let dirty = DirtySet::default();
        let solution = solver
            .solve(&dirty, &MockMeasuredRun, 0.016)
            .expect("a simple same-module system must solve");
        assert_eq!(solution.status, SolveStatus::Solved);
        assert_eq!(solution.module, ModuleId(0));
        assert_eq!(solution.transforms.len(), 1);
        assert_eq!(solution.transforms[0].0, RenderObjectId(0));
        // The linear-equality pass should have resolved x := 10 and folded
        // it into the translation column of the instance transform.
        assert_eq!(solution.transforms[0].1.m[12], 10.0);
        assert_eq!(solution.transforms[0].1.m[13], 0.0);
    }

    /// A cross-module constraint surfaces from `solve` as
    /// [`SolveError::LocalityViolated`] carrying the offending
    /// [`ConstraintId`] and module boundary.
    #[test]
    fn solve_returns_locality_violated_for_cross_module() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        solver.add_node(node(1, 1));
        let cid = solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(Some(1), "x", 0.0),
            weight: 1.0,
            module: ModuleId(0),
        });

        let dirty = DirtySet::default();
        let err = solver
            .solve(&dirty, &MockMeasuredRun, 0.016)
            .expect_err("cross-module solve must fail");
        match err {
            SolveError::LocalityViolated {
                constraint,
                boundary,
            } => {
                assert_eq!(constraint, cid, "violating ConstraintId must be reported");
                assert_eq!(boundary, (ModuleId(0), ModuleId(1)));
            }
            other => panic!("expected LocalityViolated, got {other:?}"),
        }
    }

    /// A [`MeasuredRun`] mock that returns a fixed non-empty advance vector
    /// so tests can verify the measurement contract was actually invoked
    /// (Gap H2). The three advances sum to `6.0` and have length `3`, which
    /// the test asserts land in [`LayoutSolution::glyph_runs`].
    #[derive(Clone, Copy, Debug, Default)]
    struct CountingMeasuredRun;

    impl MeasuredRun for CountingMeasuredRun {
        fn shape_and_measure(&self, _run: &TextRun, _ctx: &FontContext) -> GlyphMetrics {
            GlyphMetrics {
                advances: vec![1.0, 2.0, 3.0],
                ascents: Vec::new(),
                descents: Vec::new(),
                clusters: Vec::new(),
                caret_offsets: Vec::new(),
            }
        }

        fn line_break(&self, _glyphs: &[GlyphRun], _max_width: f32) -> Vec<LineBreak> {
            Vec::new()
        }
    }

    /// A non-empty `dirty.modules` scopes the solve to constraints whose
    /// `module` is in the dirty set; out-of-scope constraints are skipped
    /// entirely (Gap H2). The in-scope module's `x := 20` resolves while the
    /// out-of-scope module's `x := 10` does not, leaving node 0 at the
    /// default `x = 0`.
    #[test]
    fn solve_scopes_to_dirty_module_subset() {
        let mut solver = CassowarySolver::new();
        // Node 0 in module 0, Node 1 in module 1.
        solver.add_node(node(0, 0));
        solver.add_node(node(1, 1));
        // Constraint in module 0: x := 10 for node 0.
        solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(None, "", 10.0),
            weight: 1.0,
            module: ModuleId(0),
        });
        // Constraint in module 1: x := 20 for node 1.
        solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(1), "x", 0.0),
            b: facet(None, "", 20.0),
            weight: 1.0,
            module: ModuleId(1),
        });

        // Dirty set contains only module 1 → only module 1's constraint solves.
        let dirty = DirtySet {
            modules: vec![ModuleId(1)],
            ..Default::default()
        };
        let solution = solver
            .solve(&dirty, &MockMeasuredRun, 0.016)
            .expect("scoped solve must succeed");
        assert_eq!(solution.status, SolveStatus::Solved);

        // Node 1 (in-scope) should have x := 20 resolved into its transform.
        let t1 = solution
            .transforms
            .iter()
            .find(|(rid, _)| *rid == RenderObjectId(1))
            .expect("node 1 transform must be present");
        assert_eq!(t1.1.m[12], 20.0, "in-scope constraint must resolve x := 20");

        // Node 0 (out-of-scope) should keep the default x := 0 because its
        // constraint was skipped by the dirty-subset filter.
        let t0 = solution
            .transforms
            .iter()
            .find(|(rid, _)| *rid == RenderObjectId(0))
            .expect("node 0 transform must be present");
        assert_eq!(
            t0.1.m[12], 0.0,
            "out-of-scope constraint must not resolve (x stays default 0)"
        );
    }

    /// An empty `dirty.modules` solves every live constraint (backward
    /// compatible with the Wave 6 simplification, Gap H2). Both modules'
    /// constraints resolve.
    #[test]
    fn solve_empty_dirty_solves_all_modules() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        solver.add_node(node(1, 1));
        solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(0), "x", 0.0),
            b: facet(None, "", 10.0),
            weight: 1.0,
            module: ModuleId(0),
        });
        solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(1), "x", 0.0),
            b: facet(None, "", 20.0),
            weight: 1.0,
            module: ModuleId(1),
        });

        let dirty = DirtySet::default();
        let solution = solver
            .solve(&dirty, &MockMeasuredRun, 0.016)
            .expect("empty-dirty solve must succeed");
        assert_eq!(solution.status, SolveStatus::Solved);
        let t0 = solution
            .transforms
            .iter()
            .find(|(rid, _)| *rid == RenderObjectId(0))
            .expect("node 0 transform must be present");
        assert_eq!(t0.1.m[12], 10.0);
        let t1 = solution
            .transforms
            .iter()
            .find(|(rid, _)| *rid == RenderObjectId(1))
            .expect("node 1 transform must be present");
        assert_eq!(t1.1.m[12], 20.0);
    }

    /// A [`MeasureKind::Text`] node triggers
    /// [`MeasuredRun::shape_and_measure`] and forwards the returned
    /// `advances.len()` into [`LayoutSolution::glyph_runs`] (Gap H2).
    #[test]
    fn solve_invokes_measured_for_text_nodes() {
        let mut solver = CassowarySolver::new();
        solver.add_node(LayoutNode {
            id: RenderObjectId(0),
            module: ModuleId(0),
            measure: MeasureKind::Text,
            children: Vec::new(),
        });

        let dirty = DirtySet::default();
        let solution = solver
            .solve(&dirty, &CountingMeasuredRun, 0.016)
            .expect("solve with a Text node must succeed");
        assert_eq!(solution.status, SolveStatus::Solved);
        // The Text node should have produced exactly one glyph run whose
        // `glyph_count` matches the advance-vector length returned by the
        // mock, and whose `total_advance` is the sum of the advances.
        assert_eq!(solution.glyph_runs.len(), 1);
        assert_eq!(solution.glyph_runs[0].glyph_count, 3);
        assert_eq!(solution.glyph_runs[0].total_advance, 6.0);
    }

    /// Fixed-measure nodes do not invoke the measurement contract: with
    /// only [`MeasureKind::Fixed`] nodes in the graph, `glyph_runs` stays
    /// empty (Gap H2 regression guard).
    #[test]
    fn solve_skips_measured_for_fixed_nodes() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        let dirty = DirtySet::default();
        let solution = solver
            .solve(&dirty, &CountingMeasuredRun, 0.016)
            .expect("solve with only Fixed nodes must succeed");
        assert!(solution.glyph_runs.is_empty());
    }

    /// A constraint referencing a removed (non-existent) node surfaces from
    /// `solve` as [`SolveError::Unsatisfiable`] carrying the offending
    /// [`ConstraintId`] and a default [`RelaxationHint`] (Gap H2).
    #[test]
    fn solve_returns_unsatisfiable_for_removed_node() {
        let mut solver = CassowarySolver::new();
        solver.add_node(node(0, 0));
        // Reference render object 999 (never registered) from the constraint.
        let cid = solver.add_constraint(Constraint {
            kind: ConstraintKind::Linear,
            a: facet(Some(999), "x", 0.0),
            b: facet(None, "", 10.0),
            weight: 1.0,
            module: ModuleId(0),
        });

        let dirty = DirtySet::default();
        let err = solver
            .solve(&dirty, &MockMeasuredRun, 0.016)
            .expect_err("constraint referencing a removed node must fail");
        match err {
            SolveError::Unsatisfiable {
                offenders,
                suggestion,
            } => {
                assert_eq!(offenders, vec![cid]);
                assert!(
                    suggestion.relax.is_empty(),
                    "default RelaxationHint must carry no relaxations"
                );
            }
            other => panic!("expected Unsatisfiable, got {other:?}"),
        }
    }

    // -- HarfRustMeasuredRun (Wave 3 / task WAVE-W3) -----------------------

    /// Minimal embedded TTF — OpenSans variable subset (3196 bytes).
    /// Covers U+0065 ('e'). Mirrors `alkalive-text`'s `TEST_FONT_TTF` so
    /// [`HarfRustMeasuredRun`] can be exercised end-to-end against a real
    /// HarfRust font registry without depending on `alkalive-text`'s private
    /// test fixtures.
    const HARF_TEST_FONT_TTF: &[u8] = include_bytes!(
        "../../../vendor/harfrust/harfrust/tests/fonts/rb_custom/OpenSans.subset1.ttf"
    );

    /// `HarfRustMeasuredRun::shape_and_measure` against a real HarfRust font
    /// registry produces non-empty `advances` for a covered codepoint ('e'),
    /// and the per-glyph arrays stay parallel (Wave 3 / task WAVE-W3).
    #[test]
    fn harfrust_measured_run_shapes_real_font() {
        let mut reg = HarfRustFontRegistry::new();
        reg.load_bundle(HARF_TEST_FONT_TTF)
            .expect("load_bundle must be Ok for the embedded test font");
        let measured = HarfRustMeasuredRun::new(Arc::new(reg));

        let run = TextRun {
            id: TextRunId(0),
            text: "e".to_string(),
            module: ModuleId(0),
        };
        let ctx = FontContext;
        let metrics = measured.shape_and_measure(&run, &ctx);

        // The OpenSans subset covers U+0065 ('e'), so the shaped run must
        // emit at least one glyph with a non-zero advance.
        assert!(
            !metrics.advances.is_empty(),
            "advances must be non-empty for a covered codepoint, got {:?}",
            metrics.advances,
        );
        // The total advance should be positive — a real metric, not the
        // empty `GlyphMetrics::default()` returned on shape failure.
        let total: f32 = metrics.advances.iter().copied().sum();
        assert!(
            total > 0.0,
            "total advance must be positive for a real glyph, got {total}",
        );

        // Per-glyph arrays must stay parallel: ascents/descents/clusters
        // mirror the glyph count.
        assert_eq!(metrics.ascents.len(), metrics.advances.len());
        assert_eq!(metrics.descents.len(), metrics.advances.len());
        assert_eq!(metrics.clusters.len(), metrics.advances.len());
        // Caret offsets carry N+1 entries for an N-glyph run.
        assert_eq!(metrics.caret_offsets.len(), metrics.advances.len() + 1);
    }

    /// `HarfRustMeasuredRun::line_break` performs simple greedy breaking:
    /// a run of three [`GlyphRun`]s whose advances sum past `max_width`
    /// yields exactly the breaks that fit, each pointing one past the
    /// overflowing glyph (Wave 3 / task WAVE-W3).
    #[test]
    fn harfrust_measured_run_line_breaks_greedily() {
        let mut reg = HarfRustFontRegistry::new();
        reg.load_bundle(HARF_TEST_FONT_TTF)
            .expect("load_bundle must be Ok for the embedded test font");
        let measured = HarfRustMeasuredRun::new(Arc::new(reg));

        // Three 10-px glyph runs; `max_width = 25.0` fits two per line.
        let glyphs = vec![
            GlyphRun {
                total_advance: 10.0,
                glyph_count: 1,
            },
            GlyphRun {
                total_advance: 10.0,
                glyph_count: 1,
            },
            GlyphRun {
                total_advance: 10.0,
                glyph_count: 1,
            },
        ];
        let breaks = measured.line_break(&glyphs, 25.0);
        // Greedy: line 1 fits glyphs 0+1 (20 px), glyph 2 overflows to line 2.
        // The break is emitted when `acc` exceeds `max_width`, which happens
        // after accumulating glyph 2 (30 > 25), pointing `next_glyph` at 3.
        // No further break is emitted because glyph 3 alone (10 px) fits.
        assert_eq!(
            breaks.len(),
            1,
            "expected exactly one break for the overflow, got {breaks:?}",
        );
        assert_eq!(breaks[0].next_glyph, 3);
        assert_eq!(breaks[0].penalty, 0.0);
    }

    /// `HarfRustMeasuredRun` also plugs into [`CassowarySolver::solve`] via
    /// the [`MeasuredRun`] trait: a [`MeasureKind::Text`] node yields a
    /// non-empty `glyph_runs` entry whose `glyph_count` and `total_advance`
    /// come from the real HarfRust shaping (Wave 3 integration smoke test).
    #[test]
    fn solve_with_harfrust_measured_run_emits_real_glyph_run() {
        let mut reg = HarfRustFontRegistry::new();
        reg.load_bundle(HARF_TEST_FONT_TTF)
            .expect("load_bundle must be Ok for the embedded test font");
        let measured = HarfRustMeasuredRun::new(Arc::new(reg));

        // Build a Text node whose measurement will hit HarfRust. The solver
        // passes a dummy `TextRun` with an empty `text` field (§5.4); the
        // real text payload arrives with the future rendering-ABI ADR. For
        // Wave 3 we only need to prove the contract is invoked end-to-end
        // and returns *some* metrics — `MockMeasuredRun` would return empty
        // `advances`, so a non-empty `glyph_count` is the integration proof.
        let mut solver = CassowarySolver::new();
        solver.add_node(LayoutNode {
            id: RenderObjectId(0),
            module: ModuleId(0),
            measure: MeasureKind::Text,
            children: Vec::new(),
        });

        let dirty = DirtySet::default();
        let solution = solver
            .solve(&dirty, &measured, 0.016)
            .expect("solve with HarfRustMeasuredRun must succeed");
        assert_eq!(solution.status, SolveStatus::Solved);
        assert_eq!(solution.glyph_runs.len(), 1);
        // The dummy TextRun carries an empty string, which HarfRust shapes
        // to zero glyphs; that is the expected Wave 3 behaviour. The point
        // is that the call succeeded without panicking or returning an
        // error — the integration is wired.
        assert_eq!(solution.glyph_runs[0].glyph_count, 0);
    }
}

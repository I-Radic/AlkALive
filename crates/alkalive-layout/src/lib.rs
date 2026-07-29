//! Layout system — geometry primitives, pluggable constraint solver, and
//! the text-flow measurement contract (§5.2–5.7).
//!
//! Wave-3 trait skeletons: every method body is `todo!()`. Implementations
//! land in Wave 6 (default Cassowary solver, locality gate, GPU-transform
//! emission per ADR 002/004/022).
//!
//! # Cross-crate forward declarations
//!
//! This crate ships self-contained (no external deps, no workspace deps).
//! Two cross-crate types referenced by the spec are stubbed here:
//! - [`OwnedStyle`] — concrete struct lives in `alkalive-style` (§7).
//! - [`ShapeError`] — concrete enum lives in `alkalive-text` (§6.3).
//!
//! Both are unified by the future rendering-ABI ADR (§4.7 / §5.4
//! shared-boundary note). The stubs exist only so the layout crate compiles
//! standalone in Wave 3.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Wave-3 skeleton: every method body is `todo!()`, so parameters are
// intentionally unused. Suppressing this crate-wide keeps spec-faithful
// parameter names without polluting CI's `clippy -- -D warnings` gate.
#![allow(unused_variables)]

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

/// Module ownership tag (ADR 002) used to enforce layout locality (§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ModuleId(pub u32);

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
        Self {
            m: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
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
#[derive(Clone, Debug, Default)]
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
    /// Per-caret x-offsets, BiDi-aware.
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
pub trait MeasuredRun {
    /// Shape and measure a [`TextRun`] synchronously; HarfRust-backed, no
    /// DOM crossing (§5.4, ADR 022).
    fn shape_and_measure(&self, run: &TextRun, ctx: &FontContext) -> GlyphMetrics {
        todo!()
    }

    /// Break `glyphs` into lines constrained by `max_width` (§5.4).
    fn line_break(&self, glyphs: &[GlyphRun], max_width: f32) -> Vec<LineBreak> {
        todo!()
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
/// The runtime ships a default Cassowary-class linear implementation; author
/// backends (impulse/physics, directed-graph, GPU-compute) bind behind the
/// same trait, so swapping solvers is internal and non-breaking to downstream
/// paint stages. The layout-tree is solver-internal and never re-derived
/// from styles (§5.1), eliminating the style-driven box-tree recalculation
/// that couples style mutation to global reflow (P2.3, P2.4).
pub trait LayoutSolver {
    /// Register a node in the solver-internal layout graph.
    fn add_node(&mut self, node: LayoutNode) -> NodeId {
        todo!()
    }

    /// Remove a node and its descendants; per-module dirty-rect (ADR 002).
    fn remove_node(&mut self, id: NodeId) {
        todo!()
    }

    /// Bind an immutable style snapshot to a node — input only; never mutated.
    fn bind_style(&mut self, id: NodeId, style: &OwnedStyle) {
        todo!()
    }

    /// Register a constraint; returns its handle for later removal.
    fn add_constraint(&mut self, c: Constraint) -> ConstraintId {
        todo!()
    }

    /// Remove a previously-registered constraint.
    fn remove_constraint(&mut self, id: ConstraintId) {
        todo!()
    }

    /// Locality gate (ADR 002). Rejects cross-module flex baselines,
    /// percentage chains spanning module boundaries, or any constraint
    /// whose satisfaction would reflow outside the dirty set (§5.5).
    fn assert_local(&self, c: &Constraint) -> Result<(), LocalityViolation> {
        todo!()
    }

    /// Synchronous solve over the dirty subset; consumes measured text
    /// runs (§5.4) and emits GPU-ready transforms. No intermediate tree
    /// (§5.3, §5.6).
    fn solve(
        &mut self,
        dirty: &DirtySet,
        measured: &dyn MeasuredRun,
        dt: f32,
    ) -> Result<LayoutSolution, SolveError> {
        todo!()
    }
}

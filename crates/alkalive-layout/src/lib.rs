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
//! # Cross-crate forward declarations
//!
//! This crate ships self-contained (no external deps, no workspace deps).
//! Two cross-crate types referenced by the spec are stubbed here:
//! - [`OwnedStyle`] — concrete struct lives in `alkalive-style` (§7).
//! - [`ShapeError`] — concrete enum lives in `alkalive-text` (§6.3).
//!
//! Both are unified by the future rendering-ABI ADR (§4.7 / §5.4
//! shared-boundary note). The stubs exist only so the layout crate compiles
//! standalone.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use alkalive_core::ModuleId;

use std::collections::HashMap;

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
/// The `dirty`, `measured`, and `dt` parameters of
/// [`LayoutSolver::solve`] are intentionally ignored in Wave 6; the real
/// Cassowary simplex, dirty-subset scoping, and text-measurement
/// integration land in a later wave. The trait surface is stable so a
/// production solver can drop in behind [`LayoutSolver`] without breaking
/// downstream paint stages.
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
        // Wave 6 simplified solver: dirty-subset scoping, text measurement,
        // and time-step integration are deferred to the real Cassowary
        // simplex landing in a later wave.
        let _ = dirty;
        let _ = measured;
        let _ = dt;

        // Pass 1 — locality gate (§5.5). The first offender aborts the solve.
        for (idx, slot) in self.constraints.iter().enumerate() {
            if let Some(c) = slot {
                if let Err(violation) = self.assert_local(c) {
                    return Err(SolveError::LocalityViolated {
                        constraint: ConstraintId(idx as u32),
                        boundary: violation.boundary,
                    });
                }
            }
        }

        // Pass 2 — single-pass linear-equality assignment (Wave 6
        // simplification). For each `Linear` constraint we set `a := b`:
        // a literal RHS uses `b.value`; a facet RHS reads the previously
        // assigned value (falling back to `b.value`). `Impulse` and
        // `GraphLayout` kinds are accepted but skipped.
        let mut facets: HashMap<(RenderObjectId, &'static str), f32> = HashMap::new();
        for slot in self.constraints.iter().flatten() {
            if slot.kind != ConstraintKind::Linear {
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

        // Pass 3 — emit one instance transform per live node (§5.6). The
        // solution's `module` tag is taken from the first live node so the
        // dirty-rect scoping downstream has a stable locality tag.
        let mut transforms = Vec::new();
        let mut module = ModuleId(0);
        let mut first = true;
        for node in self.nodes.iter().flatten() {
            if first {
                module = node.module;
                first = false;
            }
            let x = facets.get(&(node.id, "x")).copied().unwrap_or(0.0);
            let y = facets.get(&(node.id, "y")).copied().unwrap_or(0.0);
            transforms.push((node.id, Mat4::translated(x, y)));
        }

        Ok(LayoutSolution {
            status: SolveStatus::Solved,
            transforms,
            clips: Vec::new(),
            glyph_runs: Vec::new(),
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
}

//! Incremental computation — dependency graph for ADR-025.
//!
//! This module implements the *compiler side* of ADR-025 (Salsa/Adapton-style
//! incremental computation). It analyses a [`ScheduledScene`] produced by
//! ADR-024's [`schedule_lowering`](crate::schedule::schedule_lowering) pass
//! and builds a [`DependencyGraph`] — a directed acyclic graph of
//! computation nodes (one per schedule pass) annotated with the
//! [`SignalId`]s they read (inputs) and write (outputs).
//!
//! # Data flow
//!
//! ```text
//! AlgorithmIR ──► [schedule_lowering] ──► ScheduledScene
//!                                              │
//!                                              ▼
//!                                       [incremental_analysis]
//!                                              │
//!                                              ▼
//!                                       DependencyGraph
//!                                              │
//!                                              ▼
//!                            (runtime) SignalStore + dirty propagation
//! ```
//!
//! The runtime stores the [`DependencyGraph`] alongside a `SignalStore` (a
//! key-value map of signal values with `u64` version counters). On each
//! frame, the runtime compares versions to determine which signals changed,
//! then propagates dirtiness through the graph: only the passes whose
//! inputs include a changed signal need to re-execute. Per-frame work
//! drops from O(n) to O(Δ).
//!
//! # Hello World signal set
//!
//! The current Hello World scene has six well-known signals, defined in the
//! [`signals`] submodule:
//!
//! | Signal              | ID | Updated by               |
//! |---------------------|----|--------------------------|
//! | `INPUT_TEXT`        | 0  | keydown / IME input      |
//! | `TIME`              | 1  | the frame loop (per tick)|
//! | `FONT_SIZE`         | 2  | scene compilation        |
//! | `ROTATION_SPEED`    | 3  | scene compilation        |
//! | `CANVAS_WIDTH`      | 4  | window resize            |
//! | `CANVAS_HEIGHT`     | 5  | window resize            |
//!
//! Each [`PassKind`](crate::schedule::PassKind) declares a fixed set of
//! signal inputs in [`incremental_analysis`]. Outputs are currently empty
//! (the Hello World runtime does not yet produce signals from passes); a
//! future wave will populate `outputs` to model intra-frame dependencies
//! (e.g. layout → paint → composite).
//!
//! # Cache mitigation
//!
//! The dependency graph adds per-frame bookkeeping overhead (version
//! checks, hash lookups). For small scenes — Hello World has only two
//! algorithm nodes — this overhead may exceed the savings from skipping
//! unchanged passes. The runtime therefore applies an R1 mitigation: when
//! the algorithm node count is below a threshold (typically 50), it
//! bypasses the dependency graph entirely and uses the legacy full-rebuild
//! path. The threshold is owned by the runtime crate (see
//! `SMALL_SCENE_THRESHOLD` in `alkalive-runtime-wasm`); this module is
//! agnostic to it.
//!
//! # Safety
//!
//! This module is part of the `alkalive-compiler` crate which is
//! `#![forbid(unsafe_code)]`. The dependency graph is pure data — no
//! `unsafe` is required.

#![forbid(unsafe_code)]

use crate::schedule::{PassKind, ScheduledScene};

/// Unique ID for a computation node in the dependency graph.
///
/// A computation node corresponds 1-to-1 with a schedule pass: node `i`
/// models pass `i` of `ScheduledScene::schedule::passes`. The IDs are
/// dense, sequential `u32` values starting at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DepNodeId(pub u32);

/// Unique ID for a signal.
///
/// Signals are the atomic unit of dirty tracking. Each well-known signal
/// (see [`signals`]) has a stable ID; the runtime's `SignalStore` keeps a
/// version counter per ID. When a signal's value changes, its version
/// bumps, and any computation node that lists it as an input is marked
/// dirty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub u32);

/// A computation node in the dependency graph.
///
/// Each node models a single schedule pass: it reads some signals as
/// inputs, performs a computation (text shaping, glyph rasterization,
/// vertex buffer construction, draw call submission, etc.), and writes
/// some signals as outputs (currently always empty for the Hello World
/// scene — see [`incremental_analysis`] for the per-pass input list).
#[derive(Debug, Clone)]
pub struct DepNode {
    /// The node's unique identifier (matches its index in
    /// [`DependencyGraph::nodes`]).
    pub id: DepNodeId,
    /// Which signals this computation reads. The runtime marks this node
    /// dirty when any of these signals' versions change.
    pub inputs: Vec<SignalId>,
    /// Which signals this computation writes. (Currently always empty for
    /// the Hello World scene — a future wave will populate this to model
    /// intra-frame dependencies like layout → paint → composite.)
    pub outputs: Vec<SignalId>,
    /// Which schedule pass this node belongs to (an index into
    /// `ScheduledScene::schedule::passes`). The runtime uses this to map
    /// dirty node IDs back to dirty pass indices for the renderer.
    pub pass_index: usize,
    /// Human-readable description (the `Debug` formatting of the
    /// [`PassKind`](crate::schedule::PassKind)). Useful for diagnostics
    /// and debugging.
    pub description: String,
}

/// The dependency graph for incremental computation.
///
/// Produced by [`incremental_analysis`]. The runtime stores this graph
/// alongside the `SignalStore` and uses it to propagate dirtiness from
/// changed signals to the passes that depend on them.
///
/// The graph is currently a *flat* list of nodes (one per schedule pass)
/// with no inter-node edges — outputs are always empty for the Hello
/// World scene. A future wave will add edges to model intra-frame
/// dependencies (e.g. the `InputText` pass depends on the
/// `InputFieldBackground` pass's output).
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// The computation nodes, in pass order (node `i` models pass `i`).
    pub nodes: Vec<DepNode>,
}

impl DependencyGraph {
    /// Returns the number of nodes in the graph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the graph contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Look up a node by its ID. Returns `None` if no node with that ID
    /// exists (which can happen if a stale `DepNodeId` is held after the
    /// graph was rebuilt).
    pub fn node(&self, id: DepNodeId) -> Option<&DepNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Look up a node by its pass index. Returns `None` if no node models
    /// that pass index (e.g. the schedule was rebuilt with fewer passes).
    pub fn node_for_pass(&self, pass_index: usize) -> Option<&DepNode> {
        self.nodes.iter().find(|n| n.pass_index == pass_index)
    }
}

/// Well-known signal IDs for the Hello World scene.
///
/// These are the only signals the current runtime uses. A future wave may
/// introduce a signal-registration API so that arbitrary user-defined
/// signals can be added (e.g. for non-Hello-World scenes).
pub mod signals {
    use super::SignalId;

    /// The user's input text buffer (forwarded from the IME input element
    /// via the keydown / `input` event listeners). A `Text(String)` signal.
    pub const INPUT_TEXT: SignalId = SignalId(0);
    /// The animation clock, advanced by `1/60` per frame by the runtime's
    /// `requestAnimationFrame` loop. A `Float(f32)` signal.
    pub const TIME: SignalId = SignalId(1);
    /// The title text's font size in pixels. A `Float(f32)` signal.
    pub const FONT_SIZE: SignalId = SignalId(2);
    /// The title text's Y-axis rotation speed in radians per second. A
    /// `Float(f32)` signal.
    pub const ROTATION_SPEED: SignalId = SignalId(3);
    /// The canvas width in physical pixels. A `Uint(u32)` signal, updated
    /// by the window `resize` listener.
    pub const CANVAS_WIDTH: SignalId = SignalId(4);
    /// The canvas height in physical pixels. A `Uint(u32)` signal, updated
    /// by the window `resize` listener.
    pub const CANVAS_HEIGHT: SignalId = SignalId(5);
}

/// Build a dependency graph from a scheduled scene.
///
/// For each pass in `scheduled.schedule.passes`, this creates a [`DepNode`]
/// whose `inputs` are the well-known signals that pass reads. The mapping
/// is:
///
/// | Pass kind                 | Inputs                                                  |
/// |---------------------------|---------------------------------------------------------|
/// | [`Clear`]                 | `CANVAS_WIDTH`, `CANVAS_HEIGHT`                         |
/// | [`TitleText`]             | `INPUT_TEXT`, `TIME`, `FONT_SIZE`, `ROTATION_SPEED`,   |
/// |                           | `CANVAS_WIDTH`, `CANVAS_HEIGHT`                         |
/// | [`InputText`]             | `INPUT_TEXT`, `CANVAS_WIDTH`, `CANVAS_HEIGHT`           |
/// | [`InputFieldBackground`]  | `CANVAS_WIDTH`, `CANVAS_HEIGHT`                         |
/// | [`InputFieldBorder`]      | `CANVAS_WIDTH`, `CANVAS_HEIGHT`                         |
///
/// The rationale:
/// - All passes depend on the canvas dimensions because they compute
///   layout in canvas-pixel space (the input field bounds are derived
///   from `width`/`height`).
/// - `TitleText` reads `INPUT_TEXT` because — in the Hello World scene —
///   the title text is replaced by the user's input when the buffer is
///   non-empty. It reads `TIME` for the rotation animation, and
///   `FONT_SIZE`/`ROTATION_SPEED` because those are intrinsic properties
///   of the text node.
/// - `InputText` reads `INPUT_TEXT` (the typed text) but not `TIME`
///   (input text is not animated).
/// - `Clear` reads only the canvas dimensions (the clear color is a
///   compile-time constant for Hello World; a future wave may add a
///   `BACKGROUND_COLOR` signal).
///
/// `outputs` is currently empty for all passes — the Hello World runtime
/// does not yet produce signals from passes. A future wave will populate
/// `outputs` to model intra-frame dependencies.
///
/// [`Clear`]: PassKind::Clear
/// [`TitleText`]: PassKind::TitleText
/// [`InputText`]: PassKind::InputText
/// [`InputFieldBackground`]: PassKind::InputFieldBackground
/// [`InputFieldBorder`]: PassKind::InputFieldBorder
pub fn incremental_analysis(scheduled: &ScheduledScene) -> DependencyGraph {
    let mut graph = DependencyGraph::default();

    // For each pass in the schedule, create a dependency node.
    for (pass_idx, pass) in scheduled.schedule.passes.iter().enumerate() {
        let node_id = pass_idx as u32;
        let mut inputs = Vec::new();

        match pass.kind {
            PassKind::Clear => {
                inputs.push(signals::CANVAS_WIDTH);
                inputs.push(signals::CANVAS_HEIGHT);
            }
            PassKind::TitleText => {
                inputs.push(signals::INPUT_TEXT);
                inputs.push(signals::TIME);
                inputs.push(signals::FONT_SIZE);
                inputs.push(signals::ROTATION_SPEED);
                inputs.push(signals::CANVAS_WIDTH);
                inputs.push(signals::CANVAS_HEIGHT);
            }
            PassKind::InputText => {
                inputs.push(signals::INPUT_TEXT);
                inputs.push(signals::CANVAS_WIDTH);
                inputs.push(signals::CANVAS_HEIGHT);
            }
            PassKind::InputFieldBackground | PassKind::InputFieldBorder => {
                inputs.push(signals::CANVAS_WIDTH);
                inputs.push(signals::CANVAS_HEIGHT);
            }
        }

        graph.nodes.push(DepNode {
            id: DepNodeId(node_id),
            inputs,
            outputs: Vec::new(),
            pass_index: pass_idx,
            description: format!("{:?}", pass.kind),
        });
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::compile_scheduled;
    use crate::ir::{mint_module_id, ColorIR, NodeIR, PositionIR, SceneIR};
    use crate::schedule::{
        schedule_lowering, BatchingStrategy, PassKind, RenderPass, ScheduledScene, ShaderId,
        ThreadAffinity,
    };

    /// Build an `AlgorithmIR` with the given nodes (helper for tests).
    fn algo_with_nodes(nodes: Vec<NodeIR>) -> crate::ir::AlgorithmIR {
        let mut ir = SceneIR::new(mint_module_id("Test"), "Test");
        ir.nodes = nodes;
        ir
    }

    fn sample_text_node() -> NodeIR {
        NodeIR::Text {
            content: "Hello".into(),
            color: ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: PositionIR::Center,
        }
    }

    fn sample_input_field() -> NodeIR {
        NodeIR::InputField {
            placeholder: "Type here...".into(),
            position: PositionIR::BelowText,
        }
    }

    // ---- DepNodeId / SignalId basics ----

    #[test]
    fn dep_node_id_equality_and_copy() {
        let a = DepNodeId(0);
        let b = DepNodeId(0);
        let c = DepNodeId(1);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Copy semantics: assigning does not move.
        let d = a;
        assert_eq!(a, d);
    }

    #[test]
    fn signal_id_equality_and_copy() {
        let a = SignalId(0);
        let b = SignalId(0);
        let c = SignalId(1);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let d = a;
        assert_eq!(a, d);
    }

    // ---- Well-known signal constants ----

    #[test]
    fn well_known_signals_have_distinct_ids() {
        let ids = [
            signals::INPUT_TEXT,
            signals::TIME,
            signals::FONT_SIZE,
            signals::ROTATION_SPEED,
            signals::CANVAS_WIDTH,
            signals::CANVAS_HEIGHT,
        ];
        // All IDs distinct.
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(
                    ids[i], ids[j],
                    "signals {:?} and {:?} collide",
                    ids[i], ids[j]
                );
            }
        }
    }

    #[test]
    fn well_known_signals_use_ids_0_through_5() {
        assert_eq!(signals::INPUT_TEXT, SignalId(0));
        assert_eq!(signals::TIME, SignalId(1));
        assert_eq!(signals::FONT_SIZE, SignalId(2));
        assert_eq!(signals::ROTATION_SPEED, SignalId(3));
        assert_eq!(signals::CANVAS_WIDTH, SignalId(4));
        assert_eq!(signals::CANVAS_HEIGHT, SignalId(5));
    }

    // ---- DependencyGraph basics ----

    #[test]
    fn empty_graph_default_is_empty() {
        let g = DependencyGraph::default();
        assert!(g.is_empty());
        assert_eq!(g.len(), 0);
        assert!(g.nodes.is_empty());
    }

    #[test]
    fn graph_len_and_is_empty() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);
        // Hello World: 5 passes -> 5 nodes.
        assert!(!g.is_empty());
        assert_eq!(g.len(), 5);
        assert_eq!(g.nodes.len(), 5);
    }

    #[test]
    fn graph_node_lookup_by_id() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        // IDs are dense 0..5.
        for i in 0..5u32 {
            let node = g.node(DepNodeId(i)).expect("node should exist");
            assert_eq!(node.id, DepNodeId(i));
        }
        // Out-of-range ID returns None.
        assert!(g.node(DepNodeId(99)).is_none());
    }

    #[test]
    fn graph_node_lookup_by_pass_index() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        // Each pass_index should map to a node whose pass_index matches.
        for pass_idx in 0..5 {
            let node = g
                .node_for_pass(pass_idx)
                .expect("node for pass should exist");
            assert_eq!(node.pass_index, pass_idx);
        }
        // Out-of-range pass_index returns None.
        assert!(g.node_for_pass(99).is_none());
    }

    // ---- incremental_analysis: per-pass input mapping ----

    #[test]
    fn hello_world_graph_has_node_per_pass() {
        // The canonical Hello World scene.
        let scheduled = compile_scheduled(
            r#"
module HelloWorld {
  scene {
    background: #000000
    text "Hello World!" {
      color: gold
      font-size: 64
      rotation: y-axis 0.5
      position: center
    }
    input-field {
      placeholder: "Type here..."
      position: below text
    }
  }
}
"#,
        )
        .expect("hello world should compile");
        let g = incremental_analysis(&scheduled);

        // 5 passes -> 5 dep nodes.
        assert_eq!(g.nodes.len(), 5);
        // Node IDs are sequential 0..5.
        for (i, node) in g.nodes.iter().enumerate() {
            assert_eq!(node.id, DepNodeId(i as u32));
            assert_eq!(node.pass_index, i);
        }
        // All nodes have empty outputs (Hello World has no signal outputs).
        for node in &g.nodes {
            assert!(
                node.outputs.is_empty(),
                "node {:?} should have empty outputs",
                node.id
            );
        }
        // Descriptions should mention the pass kind.
        let descriptions: Vec<&str> = g.nodes.iter().map(|n| n.description.as_str()).collect();
        assert!(descriptions.iter().any(|d| d.contains("Clear")));
        assert!(descriptions.iter().any(|d| d.contains("TitleText")));
        assert!(descriptions.iter().any(|d| d.contains("InputText")));
        assert!(descriptions
            .iter()
            .any(|d| d.contains("InputFieldBackground")));
        assert!(descriptions.iter().any(|d| d.contains("InputFieldBorder")));
    }

    #[test]
    fn clear_pass_reads_canvas_dimensions() {
        let algo = algo_with_nodes(vec![]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        // Empty scene has just the Clear pass.
        assert_eq!(g.nodes.len(), 1);
        let node = &g.nodes[0];
        assert_eq!(node.pass_index, 0);
        assert!(node.inputs.contains(&signals::CANVAS_WIDTH));
        assert!(node.inputs.contains(&signals::CANVAS_HEIGHT));
        // Clear doesn't read text or time.
        assert!(!node.inputs.contains(&signals::INPUT_TEXT));
        assert!(!node.inputs.contains(&signals::TIME));
        assert!(!node.inputs.contains(&signals::FONT_SIZE));
        assert!(!node.inputs.contains(&signals::ROTATION_SPEED));
    }

    #[test]
    fn title_text_pass_reads_all_six_signals() {
        let algo = algo_with_nodes(vec![sample_text_node()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        // text-only: Clear + TitleText = 2 passes.
        assert_eq!(g.nodes.len(), 2);
        let title_node = g
            .nodes
            .iter()
            .find(|n| n.description.contains("TitleText"))
            .expect("TitleText node present");
        assert_eq!(title_node.inputs.len(), 6);
        // All six well-known signals are inputs.
        assert!(title_node.inputs.contains(&signals::INPUT_TEXT));
        assert!(title_node.inputs.contains(&signals::TIME));
        assert!(title_node.inputs.contains(&signals::FONT_SIZE));
        assert!(title_node.inputs.contains(&signals::ROTATION_SPEED));
        assert!(title_node.inputs.contains(&signals::CANVAS_WIDTH));
        assert!(title_node.inputs.contains(&signals::CANVAS_HEIGHT));
    }

    #[test]
    fn input_text_pass_reads_input_and_canvas_not_time() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        let input_node = g
            .nodes
            .iter()
            .find(|n| n.description == "InputText")
            .expect("InputText node present");
        assert!(input_node.inputs.contains(&signals::INPUT_TEXT));
        assert!(input_node.inputs.contains(&signals::CANVAS_WIDTH));
        assert!(input_node.inputs.contains(&signals::CANVAS_HEIGHT));
        // Input text is NOT animated — doesn't read TIME.
        assert!(!input_node.inputs.contains(&signals::TIME));
        // Input text doesn't depend on title font-size/rotation-speed.
        assert!(!input_node.inputs.contains(&signals::FONT_SIZE));
        assert!(!input_node.inputs.contains(&signals::ROTATION_SPEED));
    }

    #[test]
    fn input_field_bg_and_border_read_only_canvas() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        for node in &g.nodes {
            if node.description == "InputFieldBackground" || node.description == "InputFieldBorder"
            {
                assert_eq!(
                    node.inputs.len(),
                    2,
                    "node {:?} should read only canvas dims",
                    node.id
                );
                assert!(node.inputs.contains(&signals::CANVAS_WIDTH));
                assert!(node.inputs.contains(&signals::CANVAS_HEIGHT));
                assert!(!node.inputs.contains(&signals::INPUT_TEXT));
                assert!(!node.inputs.contains(&signals::TIME));
            }
        }
    }

    #[test]
    fn pass_index_matches_node_position() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);

        // pass_index should match the node's position in nodes (since we
        // iterate passes in order).
        for (i, node) in g.nodes.iter().enumerate() {
            assert_eq!(node.pass_index, i);
        }
    }

    #[test]
    fn empty_scene_produces_single_clear_node() {
        let algo = algo_with_nodes(vec![]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].description, "Clear");
        assert_eq!(g.nodes[0].pass_index, 0);
    }

    #[test]
    fn graph_clone_round_trips() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        let scheduled = ScheduledScene {
            algorithm: algo,
            schedule: sched,
        };
        let g = incremental_analysis(&scheduled);
        let g2 = g.clone();
        assert_eq!(g.len(), g2.len());
        for (a, b) in g.nodes.iter().zip(g2.nodes.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.inputs, b.inputs);
            assert_eq!(a.outputs, b.outputs);
            assert_eq!(a.pass_index, b.pass_index);
            assert_eq!(a.description, b.description);
        }
    }

    #[test]
    fn manual_graph_construction_for_propagation_test() {
        // Verify we can construct a graph by hand (used by runtime tests
        // of dirty propagation). Build a minimal 2-node graph.
        let graph = DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![signals::CANVAS_WIDTH],
                    outputs: vec![],
                    pass_index: 0,
                    description: "Clear".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![signals::INPUT_TEXT, signals::TIME],
                    outputs: vec![],
                    pass_index: 1,
                    description: "TitleText".into(),
                },
            ],
        };
        assert_eq!(graph.len(), 2);
        assert_eq!(graph.node(DepNodeId(0)).unwrap().inputs.len(), 1);
        assert_eq!(graph.node(DepNodeId(1)).unwrap().inputs.len(), 2);
    }

    // ---- Schedule-shape invariants from the schedule module ----
    // (Re-stated here so the dependency-graph contract is self-documenting:
    // the graph's per-kind input mapping is only well-defined when the
    // schedule uses these shader/batching/rotation defaults.)

    #[test]
    fn schedule_passes_use_expected_shaders() {
        let algo = algo_with_nodes(vec![sample_text_node(), sample_input_field()]);
        let sched = schedule_lowering(&algo);
        // Clear + InputFieldBackground + InputFieldBorder use SolidColor.
        assert_eq!(sched.passes[0].shader, ShaderId::SolidColor);
        assert_eq!(sched.passes[1].shader, ShaderId::SolidColor);
        assert_eq!(sched.passes[2].shader, ShaderId::SolidColor);
        // TitleText + InputText use TextQuad.
        assert_eq!(sched.passes[3].shader, ShaderId::TextQuad);
        assert_eq!(sched.passes[4].shader, ShaderId::TextQuad);
        // Only TitleText has rotation enabled.
        assert!(sched.passes[3].rotation);
        assert!(!sched.passes[0].rotation);
        assert!(!sched.passes[1].rotation);
        assert!(!sched.passes[2].rotation);
        assert!(!sched.passes[4].rotation);
        // Only TitleText uses ByFontSize batching.
        assert_eq!(sched.passes[3].batching, BatchingStrategy::ByFontSize);
        assert_eq!(sched.passes[0].batching, BatchingStrategy::None);
    }

    #[test]
    fn render_pass_kind_debug_format_matches_description() {
        // The `description` field is `format!("{:?}", pass.kind)`. Verify
        // that for each PassKind the debug format is stable (the runtime's
        // propagation tests rely on these strings for diagnostics).
        assert_eq!(format!("{:?}", PassKind::Clear), "Clear");
        assert_eq!(format!("{:?}", PassKind::TitleText), "TitleText");
        assert_eq!(format!("{:?}", PassKind::InputText), "InputText");
        assert_eq!(
            format!("{:?}", PassKind::InputFieldBackground),
            "InputFieldBackground"
        );
        assert_eq!(
            format!("{:?}", PassKind::InputFieldBorder),
            "InputFieldBorder"
        );
    }

    #[test]
    fn render_pass_clone_preserves_kind_for_analysis() {
        // The incremental_analysis pass reads `pass.kind`. Verify clones
        // preserve the kind (sanity check on the Clone derive).
        let pass = RenderPass {
            node_indices: vec![0],
            shader: ShaderId::TextQuad,
            batching: BatchingStrategy::ByFontSize,
            rotation: true,
            kind: PassKind::TitleText,
            affinity: ThreadAffinity::MainThread,
        };
        let cloned = pass.clone();
        assert_eq!(pass.kind, cloned.kind);
    }
}

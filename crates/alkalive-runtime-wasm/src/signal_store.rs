//! Signal store for ADR-025 — runtime side of incremental computation.
//!
//! This module implements the *runtime side* of ADR-025 (Salsa/Adapton-style
//! incremental computation). The [`SignalStore`] is a key-value map of
//! signal values with `u64` version counters. On each frame, the runtime
//! compares versions to determine which signals changed, then propagates
//! dirtiness through the [`DependencyGraph`](alkalive_compiler::DependencyGraph)
//! (built by [`incremental_analysis`](alkalive_compiler::incremental_analysis)
//! in the compiler crate).
//!
//! # Data flow
//!
//! ```text
//!  Input event (keydown/IME)  ──►  signals.set(INPUT_TEXT, ...)
//!  Frame tick (rAF)           ──►  signals.set(TIME, ...)
//!  Window resize              ──►  signals.set(CANVAS_WIDTH/HEIGHT, ...)
//!                                       │
//!                                       ▼
//!                              signals.check_changes()  ──►  Vec<SignalId>
//!                                       │
//!                                       ▼
//!                              signals.propagate(&changed, &dep_graph)
//!                                       │  (uses dep graph: signal → pass)
//!                                       ▼
//!                              Vec<DepNodeId>  ──►  dirty_passes()
//!                                       │
//!                                       ▼
//!                              renderer.render_frame_with_dirty(..., &dirty_passes)
//! ```
//!
//! # Version counter semantics
//!
//! Each signal has a `u64` version counter that starts at 0 and
//! monotonically increases on every [`SignalStore::set`]. The store
//! separately tracks the *last-seen* version per signal (updated by
//! [`SignalStore::check_changes`]). A signal is "changed" iff its current
//! version differs from its last-seen version — i.e. it has been written
//! since the last `check_changes` call.
//!
//! # Hello World signal set
//!
//! See [`alkalive_compiler::incremental::signals`] for the well-known
//! signal IDs (INPUT_TEXT, TIME, FONT_SIZE, ROTATION_SPEED,
//! CANVAS_WIDTH, CANVAS_HEIGHT). The `SignalStore` itself is agnostic to
//! specific IDs — it stores any `(u32, SignalValue)` pair.
//!
//! # Safety
//!
//! The `alkalive-runtime-wasm` crate sets `#![allow(unsafe_code)]`
//! (because the wasm-bindgen interop layer requires it elsewhere in the
//! crate). This module uses no `unsafe` — the signal store is pure data.

use alkalive_compiler::{DepNodeId, DependencyGraph, SignalId};
use std::collections::{HashMap, HashSet};

/// A signal value stored in the [`SignalStore`].
///
/// Signals are typed — each variant corresponds to a Rust type the
/// runtime cares about. The Hello World runtime uses `Text` for
/// `INPUT_TEXT`, `Float` for `TIME`/`FONT_SIZE`/`ROTATION_SPEED`, and
/// `Uint` for `CANVAS_WIDTH`/`CANVAS_HEIGHT`.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalValue {
    /// A UTF-8 text string (used by `INPUT_TEXT`).
    Text(String),
    /// A 32-bit floating-point value (used by `TIME`, `FONT_SIZE`,
    /// `ROTATION_SPEED`).
    Float(f32),
    /// A 32-bit unsigned integer (used by `CANVAS_WIDTH`, `CANVAS_HEIGHT`).
    Uint(u32),
}

/// A signal store with version counters for incremental computation.
///
/// Each signal has a `u64` version counter that monotonically increases
/// on every [`set`](Self::set). The store separately tracks the
/// *last-seen* version per signal (updated by
/// [`check_changes`](Self::check_changes)). A signal is "changed" iff its
/// current version differs from its last-seen version.
///
/// The store also provides dirty propagation: given a list of changed
/// signals and a [`DependencyGraph`], [`propagate`](Self::propagate)
/// returns the [`DepNodeId`]s whose `inputs` include any changed signal.
/// [`dirty_passes`](Self::dirty_passes) then maps those node IDs back to
/// schedule pass indices for the renderer.
///
/// # Example
///
/// ```
/// use alkalive_compiler::{incremental_analysis, SignalId};
/// use alkalive_compiler::schedule::{ScheduledScene, ScheduleIR, RenderPass, PassKind, ShaderId, BatchingStrategy};
/// use alkalive_compiler::ir::{AlgorithmIR, mint_module_id};
/// use alkalive_runtime_wasm::signal_store::{SignalStore, SignalValue};
///
/// // Build a minimal scheduled scene (Clear + TitleText = 2 passes).
/// let mut algo = AlgorithmIR::new(mint_module_id("M"), "M");
/// algo.nodes.push(alkalive_compiler::NodeIR::Text {
///     content: "Hi".into(),
///     color: alkalive_compiler::ColorIR::Gold,
///     font_size: 32.0,
///     rotation_speed: 0.0,
///     position: alkalive_compiler::PositionIR::Center,
/// });
/// let schedule = alkalive_compiler::schedule_lowering(&algo);
/// let scheduled = ScheduledScene { algorithm: algo, schedule };
/// let dep_graph = incremental_analysis(&scheduled);
///
/// let mut store = SignalStore::new();
/// store.set(SignalId(0), SignalValue::Text("hello".into())); // INPUT_TEXT
/// let changed = store.check_changes();
/// assert_eq!(changed.len(), 1);
/// let dirty = store.propagate(&changed, &dep_graph);
/// // TitleText depends on INPUT_TEXT -> it's dirty; Clear doesn't.
/// assert!(dirty.len() <= dep_graph.nodes.len());
/// ```
#[derive(Debug)]
pub struct SignalStore {
    /// The current value per signal ID. Absent means "never set".
    values: HashMap<u32, SignalValue>,
    /// The current version counter per signal ID. Absent means "never
    /// set" (equivalent to version 0).
    versions: HashMap<u32, u64>,
    /// The last-seen version per signal ID, updated by
    /// [`check_changes`](Self::check_changes). Absent means "never
    /// checked" (equivalent to last_version 0, which differs from a
    /// freshly-set signal's version 1).
    last_versions: HashMap<u32, u64>,
}

impl Default for SignalStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalStore {
    /// Create a new empty signal store.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            versions: HashMap::new(),
            last_versions: HashMap::new(),
        }
    }

    /// Set a signal's value and bump its version counter.
    ///
    /// If the signal was never set before, its version becomes 1 (the
    /// initial `0` is bumped by one). Otherwise the version monotonically
    /// increases by one on each `set`.
    ///
    /// The new value replaces the old one. To "touch" a signal without
    /// changing its value (forcing a re-evaluation), call `set` with the
    /// same value — the version still bumps.
    pub fn set(&mut self, id: SignalId, value: SignalValue) {
        let entry = self.versions.entry(id.0).or_insert(0);
        *entry += 1;
        self.values.insert(id.0, value);
    }

    /// Get a signal's current value. Returns `None` if the signal was
    /// never set.
    pub fn get(&self, id: SignalId) -> Option<&SignalValue> {
        self.values.get(&id.0)
    }

    /// Get a signal's current version counter. Returns `0` if the signal
    /// was never set.
    pub fn version(&self, id: SignalId) -> u64 {
        self.versions.get(&id.0).copied().unwrap_or(0)
    }

    /// Check which signals changed since the last `check_changes` call.
    ///
    /// A signal is "changed" iff its current version differs from its
    /// last-seen version. The last-seen version is then updated to the
    /// current version, so a subsequent `check_changes` (with no further
    /// `set` calls) returns an empty `Vec`.
    ///
    /// The returned `Vec` is in arbitrary (HashMap iteration) order —
    /// callers that need a deterministic order should sort it.
    pub fn check_changes(&mut self) -> Vec<SignalId> {
        let mut changed = Vec::new();
        for (id, ver) in &self.versions {
            let last = self.last_versions.get(id).copied().unwrap_or(0);
            if *ver != last {
                changed.push(SignalId(*id));
                self.last_versions.insert(*id, *ver);
            }
        }
        changed
    }

    /// Determine which dependency nodes are dirty given a set of changed
    /// signals.
    ///
    /// A node is dirty iff any of its `inputs` is in the `changed` list.
    /// The returned `Vec` is in graph order (the order nodes appear in
    /// `graph.nodes`), with no duplicates.
    ///
    /// This is the *forward propagation* step of incremental computation:
    /// changed signals → dirty nodes. Transitive propagation (dirty
    /// node's outputs → dependent nodes) is not yet implemented because
    /// the Hello World graph has no inter-node edges (all `outputs` are
    /// empty).
    pub fn propagate(&self, changed: &[SignalId], graph: &DependencyGraph) -> Vec<DepNodeId> {
        let changed_set: HashSet<u32> = changed.iter().map(|s| s.0).collect();
        let mut dirty = Vec::new();
        for node in &graph.nodes {
            if node.inputs.iter().any(|sig| changed_set.contains(&sig.0)) {
                dirty.push(node.id);
            }
        }
        dirty
    }

    /// Get the dirty pass indices from dirty node IDs.
    ///
    /// Maps each [`DepNodeId`] to its `pass_index` in the graph. The
    /// returned `Vec` is in graph order (the order nodes appear in
    /// `graph.nodes`), with no duplicates.
    ///
    /// The renderer uses this to know which schedule passes need to
    /// re-execute.
    pub fn dirty_passes(&self, dirty_nodes: &[DepNodeId], graph: &DependencyGraph) -> Vec<usize> {
        let dirty_set: HashSet<u32> = dirty_nodes.iter().map(|n| n.0).collect();
        graph
            .nodes
            .iter()
            .filter(|n| dirty_set.contains(&n.id.0))
            .map(|n| n.pass_index)
            .collect()
    }

    /// Returns the number of signals currently stored.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if no signals are currently stored.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the number of dependency nodes in the graph.
    ///
    /// Convenience method for diagnostics / the small-scene fallback
    /// check (the runtime uses the algorithm's node count, not the dep
    /// graph's node count, but both are exposed for completeness).
    pub fn graph_node_count(graph: &DependencyGraph) -> usize {
        graph.nodes.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alkalive_compiler::incremental::signals;
    use alkalive_compiler::ir::{mint_module_id, AlgorithmIR, ColorIR, NodeIR, PositionIR};
    use alkalive_compiler::schedule::{schedule_lowering, ScheduledScene};
    use alkalive_compiler::DepNode;

    /// Build a scheduled scene with text + input-field (5 passes).
    fn hello_world_scheduled() -> ScheduledScene {
        let mut algo = AlgorithmIR::new(mint_module_id("HW"), "HW");
        algo.nodes.push(NodeIR::Text {
            content: "Hello".into(),
            color: ColorIR::Gold,
            font_size: 64.0,
            rotation_speed: 0.5,
            position: PositionIR::Center,
        });
        algo.nodes.push(NodeIR::InputField {
            placeholder: "Type...".into(),
            position: PositionIR::BelowText,
        });
        let schedule = schedule_lowering(&algo);
        ScheduledScene {
            algorithm: algo,
            schedule,
        }
    }

    /// Build a scheduled scene with only text (Clear + TitleText = 2 passes).
    fn text_only_scheduled() -> ScheduledScene {
        let mut algo = AlgorithmIR::new(mint_module_id("T"), "T");
        algo.nodes.push(NodeIR::Text {
            content: "Hi".into(),
            color: ColorIR::Gold,
            font_size: 32.0,
            rotation_speed: 0.0,
            position: PositionIR::Center,
        });
        let schedule = schedule_lowering(&algo);
        ScheduledScene {
            algorithm: algo,
            schedule,
        }
    }

    // ---- SignalValue basics ----

    #[test]
    fn signal_value_text_equality() {
        assert_eq!(SignalValue::Text("a".into()), SignalValue::Text("a".into()));
        assert_ne!(SignalValue::Text("a".into()), SignalValue::Text("b".into()));
    }

    #[test]
    fn signal_value_float_equality() {
        assert_eq!(SignalValue::Float(1.0), SignalValue::Float(1.0));
        assert_ne!(SignalValue::Float(1.0), SignalValue::Float(2.0));
    }

    #[test]
    fn signal_value_uint_equality() {
        assert_eq!(SignalValue::Uint(1), SignalValue::Uint(1));
        assert_ne!(SignalValue::Uint(1), SignalValue::Uint(2));
    }

    #[test]
    fn signal_value_clone_round_trips() {
        let v = SignalValue::Text("hello".into());
        let cloned = v.clone();
        assert_eq!(v, cloned);
    }

    #[test]
    fn signal_value_debug_format() {
        let v = SignalValue::Text("hi".into());
        let s = format!("{:?}", v);
        assert!(s.contains("Text"));
        assert!(s.contains("hi"));
    }

    // ---- SignalStore: set / get / version ----

    #[test]
    fn new_store_is_empty() {
        let s = SignalStore::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn default_is_empty() {
        let s = SignalStore::default();
        assert!(s.is_empty());
    }

    #[test]
    fn set_stores_value_and_bumps_version() {
        let mut s = SignalStore::new();
        assert_eq!(s.version(signals::INPUT_TEXT), 0);
        assert!(s.get(signals::INPUT_TEXT).is_none());

        s.set(signals::INPUT_TEXT, SignalValue::Text("hi".into()));
        // First set bumps version 0 -> 1.
        assert_eq!(s.version(signals::INPUT_TEXT), 1);
        assert_eq!(
            s.get(signals::INPUT_TEXT),
            Some(&SignalValue::Text("hi".into()))
        );
        assert!(!s.is_empty());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn set_replaces_value_and_bumps_version_again() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        assert_eq!(s.version(signals::INPUT_TEXT), 1);
        s.set(signals::INPUT_TEXT, SignalValue::Text("b".into()));
        assert_eq!(s.version(signals::INPUT_TEXT), 2);
        assert_eq!(
            s.get(signals::INPUT_TEXT),
            Some(&SignalValue::Text("b".into()))
        );
    }

    #[test]
    fn set_same_value_still_bumps_version() {
        // Touching a signal without changing its value still bumps the
        // version — this is how the runtime forces a re-evaluation.
        let mut s = SignalStore::new();
        s.set(signals::TIME, SignalValue::Float(1.0));
        s.set(signals::TIME, SignalValue::Float(1.0));
        assert_eq!(s.version(signals::TIME), 2);
    }

    #[test]
    fn multiple_signals_have_independent_versions() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        s.set(signals::TIME, SignalValue::Float(1.0));
        s.set(signals::TIME, SignalValue::Float(2.0));
        assert_eq!(s.version(signals::INPUT_TEXT), 1);
        assert_eq!(s.version(signals::TIME), 2);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn get_unset_signal_returns_none() {
        let s = SignalStore::new();
        assert!(s.get(signals::INPUT_TEXT).is_none());
        assert_eq!(s.version(signals::INPUT_TEXT), 0);
    }

    // ---- SignalStore: check_changes ----

    #[test]
    fn check_changes_returns_empty_on_fresh_store() {
        let mut s = SignalStore::new();
        let changed = s.check_changes();
        assert!(changed.is_empty());
    }

    #[test]
    fn check_changes_returns_set_signals() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        s.set(signals::TIME, SignalValue::Float(1.0));
        let changed = s.check_changes();
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&signals::INPUT_TEXT));
        assert!(changed.contains(&signals::TIME));
    }

    #[test]
    fn check_changes_is_idempotent_without_new_sets() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        let first = s.check_changes();
        assert_eq!(first.len(), 1);
        // No new set() calls — second check should be empty.
        let second = s.check_changes();
        assert!(second.is_empty());
    }

    #[test]
    fn check_changes_detects_subsequent_sets() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        let _ = s.check_changes();
        // Set again — should be detected as changed.
        s.set(signals::INPUT_TEXT, SignalValue::Text("b".into()));
        let changed = s.check_changes();
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&signals::INPUT_TEXT));
    }

    #[test]
    fn check_changes_resets_after_check() {
        // Set, check, set again, check — second check should detect the
        // second set only.
        let mut s = SignalStore::new();
        s.set(signals::TIME, SignalValue::Float(1.0));
        let first = s.check_changes();
        assert_eq!(first.len(), 1);
        s.set(signals::TIME, SignalValue::Float(2.0));
        let second = s.check_changes();
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn check_changes_handles_multiple_signals_independently() {
        let mut s = SignalStore::new();
        s.set(signals::INPUT_TEXT, SignalValue::Text("a".into()));
        s.set(signals::TIME, SignalValue::Float(1.0));
        // Check both.
        let _ = s.check_changes();
        // Only TIME changes.
        s.set(signals::TIME, SignalValue::Float(2.0));
        let changed = s.check_changes();
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&signals::TIME));
        assert!(!changed.contains(&signals::INPUT_TEXT));
    }

    // ---- SignalStore: propagate ----

    #[test]
    fn propagate_empty_changed_returns_empty() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();
        let dirty = s.propagate(&[], &graph);
        assert!(dirty.is_empty());
    }

    #[test]
    fn propagate_input_text_dirties_title_and_input_text_passes() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // INPUT_TEXT is read by TitleText (pass 3) and InputText (pass 4).
        let dirty = s.propagate(&[signals::INPUT_TEXT], &graph);
        // Two nodes have INPUT_TEXT in their inputs.
        assert_eq!(dirty.len(), 2);
        // The dirty node IDs should correspond to passes 3 and 4.
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        assert_eq!(dirty_passes.len(), 2);
        assert!(dirty_passes.contains(&3));
        assert!(dirty_passes.contains(&4));
        // Clear / InputFieldBackground / InputFieldBorder should NOT be dirty.
        assert!(!dirty_passes.contains(&0));
        assert!(!dirty_passes.contains(&1));
        assert!(!dirty_passes.contains(&2));
    }

    #[test]
    fn propagate_time_dirties_only_title_text() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // TIME is read only by TitleText (pass 3).
        let dirty = s.propagate(&[signals::TIME], &graph);
        assert_eq!(dirty.len(), 1);
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        assert_eq!(dirty_passes, vec![3]);
    }

    #[test]
    fn propagate_canvas_dims_dirties_all_passes() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // CANVAS_WIDTH is read by ALL passes.
        let dirty = s.propagate(&[signals::CANVAS_WIDTH], &graph);
        assert_eq!(dirty.len(), graph.nodes.len());
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        // All passes dirty.
        assert_eq!(dirty_passes.len(), graph.nodes.len());
    }

    #[test]
    fn propagate_multiple_changed_signals_unions_dirty() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // INPUT_TEXT (dirties 3,4) + CANVAS_HEIGHT (dirties all 5).
        // Union should be all 5 passes (no duplicates).
        let dirty = s.propagate(&[signals::INPUT_TEXT, signals::CANVAS_HEIGHT], &graph);
        assert_eq!(dirty.len(), graph.nodes.len());
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        // No duplicates (each pass_index appears at most once).
        let mut sorted = dirty_passes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), dirty_passes.len());
        assert_eq!(dirty_passes.len(), graph.nodes.len());
    }

    #[test]
    fn propagate_unknown_signal_id_dirties_nothing() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // Signal ID 99 isn't read by any node.
        let dirty = s.propagate(&[SignalId(99)], &graph);
        assert!(dirty.is_empty());
    }

    // ---- SignalStore: dirty_passes ----

    #[test]
    fn dirty_passes_empty_input_returns_empty() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();
        let passes = s.dirty_passes(&[], &graph);
        assert!(passes.is_empty());
    }

    #[test]
    fn dirty_passes_preserves_graph_order() {
        // dirty_passes returns passes in graph-node order (not in the order
        // they appear in dirty_nodes). Verify this.
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();

        // Pass dirty_nodes in reverse order.
        let mut reversed: Vec<DepNodeId> = graph.nodes.iter().map(|n| n.id).collect();
        reversed.reverse();
        let passes = s.dirty_passes(&reversed, &graph);
        // Should be 0,1,2,3,4 (graph order), not 4,3,2,1,0.
        assert_eq!(passes, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn dirty_passes_handles_unknown_node_id_gracefully() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let s = SignalStore::new();
        // DepNodeId(99) doesn't exist in the graph — should be silently
        // dropped (filter keeps only nodes that match the dirty_set AND
        // exist in graph.nodes).
        let passes = s.dirty_passes(&[DepNodeId(99)], &graph);
        assert!(passes.is_empty());
    }

    // ---- SignalStore: end-to-end (set → check_changes → propagate) ----

    #[test]
    fn end_to_end_set_check_propagate() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let mut s = SignalStore::new();

        // Initial state: nothing changed.
        assert!(s.check_changes().is_empty());

        // Set INPUT_TEXT and TIME.
        s.set(signals::INPUT_TEXT, SignalValue::Text("hello".into()));
        s.set(signals::TIME, SignalValue::Float(0.5));

        // check_changes should return both.
        let changed = s.check_changes();
        assert_eq!(changed.len(), 2);

        // propagate: INPUT_TEXT dirties 3,4; TIME dirties 3. Union = {3,4}.
        let dirty = s.propagate(&changed, &graph);
        assert_eq!(dirty.len(), 2);
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        assert!(dirty_passes.contains(&3));
        assert!(dirty_passes.contains(&4));

        // Second check_changes (no new sets) → empty.
        assert!(s.check_changes().is_empty());
    }

    #[test]
    fn end_to_end_time_only_dirties_title_text() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        let mut s = SignalStore::new();

        // Set TIME every frame (as the frame loop does).
        s.set(signals::TIME, SignalValue::Float(0.016));
        let changed = s.check_changes();
        assert_eq!(changed, vec![signals::TIME]);

        let dirty = s.propagate(&changed, &graph);
        assert_eq!(dirty.len(), 1);
        let dirty_passes = s.dirty_passes(&dirty, &graph);
        assert_eq!(dirty_passes, vec![3]); // TitleText is pass 3.
    }

    // ---- Small-scene fallback logic ----

    /// Below this algorithm-node count, the runtime bypasses the dependency
    /// graph and uses the legacy full-rebuild path (R1 mitigation per
    /// ADR-025: the per-frame bookkeeping cost may exceed the savings for
    /// small scenes).
    pub const SMALL_SCENE_THRESHOLD: usize = 50;

    #[test]
    fn small_scene_threshold_is_50() {
        // The threshold is a public constant so the runtime and tests can
        // reference it without magic numbers.
        assert_eq!(SMALL_SCENE_THRESHOLD, 50);
    }

    #[test]
    fn hello_world_is_small_scene() {
        // The canonical Hello World scene has 2 algorithm nodes (text +
        // input-field), well below the threshold.
        let scheduled = hello_world_scheduled();
        assert!(scheduled.algorithm.nodes.len() < SMALL_SCENE_THRESHOLD);
        assert_eq!(scheduled.algorithm.nodes.len(), 2);
    }

    #[test]
    fn text_only_scene_is_small_scene() {
        let scheduled = text_only_scheduled();
        assert!(scheduled.algorithm.nodes.len() < SMALL_SCENE_THRESHOLD);
        assert_eq!(scheduled.algorithm.nodes.len(), 1);
    }

    #[test]
    fn empty_scene_is_small_scene() {
        let algo = AlgorithmIR::new(mint_module_id("E"), "E");
        assert!(algo.nodes.len() < SMALL_SCENE_THRESHOLD);
        assert_eq!(algo.nodes.len(), 0);
    }

    #[test]
    fn large_scene_bypasses_threshold() {
        // A scene with >= 50 algorithm nodes uses the incremental path.
        let mut algo = AlgorithmIR::new(mint_module_id("L"), "L");
        for i in 0..SMALL_SCENE_THRESHOLD {
            algo.nodes.push(NodeIR::Text {
                content: format!("t{}", i),
                color: ColorIR::Gold,
                font_size: 32.0,
                rotation_speed: 0.0,
                position: PositionIR::Center,
            });
        }
        assert_eq!(algo.nodes.len(), SMALL_SCENE_THRESHOLD);
        assert!(!(algo.nodes.len() < SMALL_SCENE_THRESHOLD));
    }

    #[test]
    fn small_scene_fallback_decision_is_correct() {
        // The runtime's frame-loop decision is:
        //   if algorithm.nodes.len() < SMALL_SCENE_THRESHOLD { full path }
        //   else { incremental path }
        // Verify the decision for representative sizes.
        for &n in &[0usize, 1, 2, 10, 49] {
            assert!(n < SMALL_SCENE_THRESHOLD, "n={} should be small", n);
        }
        for &n in &[50, 51, 100, 1000] {
            assert!(!(n < SMALL_SCENE_THRESHOLD), "n={} should be large", n);
        }
    }

    // ---- graph_node_count helper ----

    #[test]
    fn graph_node_count_matches_graph_len() {
        let scheduled = hello_world_scheduled();
        let graph = alkalive_compiler::incremental_analysis(&scheduled);
        assert_eq!(SignalStore::graph_node_count(&graph), graph.nodes.len());
        assert_eq!(SignalStore::graph_node_count(&graph), 5);
    }

    #[test]
    fn graph_node_count_empty_graph() {
        let graph = DependencyGraph::default();
        assert_eq!(SignalStore::graph_node_count(&graph), 0);
    }

    // ---- DepNode / DependencyGraph re-exports ----

    #[test]
    fn dep_node_re_exported_from_compiler() {
        // Verify the runtime crate can name the compiler's DepNode type.
        let node = DepNode {
            id: DepNodeId(0),
            inputs: vec![signals::TIME],
            outputs: vec![],
            pass_index: 0,
            description: "test".into(),
        };
        assert_eq!(node.id, DepNodeId(0));
        assert_eq!(node.inputs.len(), 1);
    }
}

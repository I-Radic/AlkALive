//! E-graph optimization — ADR-026.
//!
//! This module implements the *compiler side* of ADR-026 (e-graph
//! optimization for signal read/write patterns). It takes a
//! [`DependencyGraph`] produced by ADR-025's
//! [`incremental_analysis`](crate::incremental::incremental_analysis) and
//! applies four rewrite rules to a custom e-graph data structure, then
//! extracts an optimized [`DependencyGraph`] via cost-based extraction.
//!
//! # Data flow
//!
//! ```text
//! ScheduledScene ──► [incremental_analysis] ──► DependencyGraph
//!                                                       │
//!                                                       ▼
//!                                                [egraph_optimization]
//!                                                       │
//!                                                       ▼
//!                                          optimized DependencyGraph
//!                                                       │
//!                                                       ▼
//!                                       (runtime) SignalStore + dirty
//!                                          propagation (unchanged)
//! ```
//!
//! # Why a custom e-graph?
//!
//! Per ADR-018, the workspace enforces a strict 5-crate external dependency
//! policy. Adding the [`egg`] crate would require an ADR amendment. ADR-026
//! therefore mandates a custom e-graph implementation. The four rewrite
//! rules are simple enough that hard-coded pattern matching is tractable
//! (the spec explicitly recommends this: *"start with the 4 rewrite rules
//! hard-coded as match patterns; do not build a general pattern-matching
//! DSL"*).
//!
//! [`egg`]: https://docs.rs/egg
//!
//! # The four rewrite rules
//!
//! | # | Name                       | E-graph effect                                                  |
//! |---|----------------------------|-----------------------------------------------------------------|
//! | 1 | `state_store_load_forward` | Merge the `SignalRead(s)` e-class with the written value's e-class. |
//! | 2 | `dead_store_elimination`   | Mark the first `SignalWrite(s, v1)` e-class as dead when a later `SignalWrite(s, v2)` overwrites it with no intervening read. |
//! | 3 | `read_merge`               | Merge all `SignalRead(s)` e-classes for the same signal `s` into a single e-class. |
//! | 4 | `evaluation_reorder`       | Topologically sort consumers after producers (applied during extraction, not as a merge). |
//!
//! Rules 1–3 are *e-class merges* (they add equivalences). Rule 4 is a
//! scheduling constraint applied during extraction (it does not modify the
//! e-graph).
//!
//! # Cost-based extraction
//!
//! After rewriting reaches a fixpoint, [`extract`] walks each e-class and
//! selects the cheapest equivalent e-node. The cost heuristic is:
//!
//! ```text
//! SignalRead (1) < Pass (2) < SignalWrite (3) < Const (4)
//! ```
//!
//! The total cost of an e-node is `op_cost(op) + Σ child class costs`. The
//! extraction memoizes per-class costs to avoid recomputation.
//!
//! # Hello World applicability
//!
//! The canonical Hello World scene has 5 passes and 6 signals. All passes
//! have *empty* `outputs` (no `SignalWrite` e-nodes are created). Therefore:
//!
//! - `state_store_load_forward` is a no-op (no writes to forward).
//! - `dead_store_elimination` is a no-op (no writes to eliminate).
//! - `read_merge` is automatically applied by hash-consing during
//!   [`EGraph::add`] (the same `SignalRead(s)` from multiple passes
//!   hash-conses to a single e-class).
//! - `evaluation_reorder` finds no inter-pass dependencies (no pass writes
//!   a signal that another reads), so the original order is preserved.
//!
//! The optimized dep graph for Hello World is therefore structurally
//! identical to the input — but the infrastructure is in place for scenes
//! that *do* have intra-frame signal outputs.
//!
//! # Safety
//!
//! This module is part of the `alkalive-compiler` crate which is
//! `#![forbid(unsafe_code)]`. The e-graph is pure safe Rust — no `unsafe`
//! is required. Union-find uses plain `Vec` indexing; hash-consing uses
//! `std::collections::HashMap`.

#![forbid(unsafe_code)]

use crate::incremental::{DepNode, DepNodeId, DependencyGraph, SignalId};
use std::collections::{HashMap, HashSet};

// =============================================================================
// Core data structures
// =============================================================================

/// E-class ID (union-find representative).
///
/// A `u32` index into [`EGraph::classes`]. Each e-class has a unique ID
/// assigned at creation. Union-find parent pointers (stored in
/// [`EClass::parent`]) map non-root e-classes to their representative;
/// [`EGraph::find`] follows the chain and applies path compression.
pub type EClassId = u32;

/// Operation kinds in the e-graph.
///
/// Each e-node carries one of these operations plus a list of child
/// e-class IDs (see [`ENode`]). The operation determines the e-node's
/// cost (see [`op_cost`]) and which rewrite rules apply to it.
///
/// # Variants
///
/// - [`SignalRead`](EOp::SignalRead): reads the current value of a signal.
///   Has no children (it is a leaf in the e-graph). Multiple reads of the
///   same signal hash-cons to the same e-class (this is the `read_merge`
///   rule applied at insertion time).
/// - [`SignalWrite`](EOp::SignalWrite): writes a value to a signal. Has
///   exactly one child — the e-class of the value being written (typically
///   a [`Pass`](EOp::Pass) e-class).
/// - [`Pass`](EOp::Pass): a schedule pass computation. Its children are
///   the e-classes of the signals it reads (one child per input signal).
///   The `usize` payload is the pass index (an index into
///   `ScheduledScene::schedule::passes`).
/// - [`Const`](EOp::Const): a compile-time constant value. Has no
///   children. Currently unused by the build phase (the Hello World scene
///   has no constant-foldable computations), but included for
///   completeness — future rewrite rules may fold `Pass` computations
///   into `Const` when their inputs are all known.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EOp {
    /// Read a signal. The payload is the signal's [`SignalId`].
    SignalRead(SignalId),
    /// Write a signal. The payload is the signal's [`SignalId`]. The
    /// single child e-class is the value being written.
    SignalWrite(SignalId),
    /// A pass computation. The payload is the pass index (an index into
    /// `ScheduledScene::schedule::passes`). The children are the
    /// [`SignalRead`] e-classes for the signals this pass reads.
    ///
    /// [`SignalRead`]: EOp::SignalRead
    Pass(usize),
    /// A compile-time constant value.
    Const(u64),
}

/// An e-node: a single operation in the e-graph.
///
/// An e-node is the atomic unit of the e-graph. It consists of an
/// operation ([`EOp`]) and a list of child e-class IDs. Two e-nodes are
/// *equal* (and thus hash-cons to the same e-class) iff their `op` and
/// `children` (after canonicalization via [`EGraph::find`]) are equal.
///
/// # Hash-consing
///
/// [`EGraph::add`] canonicalizes the node's children (replaces each child
/// ID with its [`EGraph::find`] root) before looking it up in the
/// hash-cons. This ensures that semantically equal e-nodes are always
/// represented by the same e-class, even after merges.
///
/// # Example
///
/// ```
/// use alkalive_compiler::egraph::{EOp, ENode, EGraph, EClassId};
/// use alkalive_compiler::SignalId;
///
/// let mut eg = EGraph::new();
/// let read = ENode { op: EOp::SignalRead(SignalId(0)), children: vec![] };
/// let id = eg.add(read.clone());
/// // Adding the same node again returns the same e-class.
/// assert_eq!(eg.add(read), id);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ENode {
    /// The operation kind.
    pub op: EOp,
    /// Child e-class IDs. Canonicalized (via [`EGraph::find`]) on
    /// insertion. The length must match the operation's arity:
    /// - `SignalRead`: 0 children
    /// - `SignalWrite`: 1 child (the value being written)
    /// - `Pass`: N children (one per input signal)
    /// - `Const`: 0 children
    pub children: Vec<EClassId>,
}

/// An e-class: a set of equivalent e-nodes.
///
/// Each e-class is a node in the union-find forest. The `parent` field
/// points to the e-class's parent in the union-find tree; for a root
/// e-class, `parent == id`. Non-root e-classes are *subsumed* — their
/// contents have been merged into the root, and [`EGraph::find`] will
/// return the root for any query on them.
///
/// # Analysis data
///
/// The `data` field carries per-e-class analysis: the cached cost (used
/// by cost-based extraction) and the cheapest e-node (the "best"
/// representative, computed by [`EGraph::compute_costs`]).
///
/// # Tombstones
///
/// The [`EGraph::dead`] set tracks e-class IDs that have been pruned by
/// `dead_store_elimination`. A dead e-class's nodes are still in the
/// e-graph (we don't physically remove them), but [`extract`] skips them
/// when reconstructing the [`DependencyGraph`].
#[derive(Debug, Clone)]
pub struct EClass {
    /// This e-class's own ID (matches its index in [`EGraph::classes`]).
    pub id: EClassId,
    /// The e-nodes in this e-class. For a root e-class, this is the
    /// merged set of all nodes from subsumed classes. For a non-root
    /// e-class, this may be empty (the nodes have been moved to the
    /// root).
    pub nodes: Vec<ENode>,
    /// Union-find parent. `self.parent == self.id` for roots.
    pub parent: EClassId,
    /// Per-e-class analysis data (cost, best node).
    pub data: EClassData,
}

/// Per-e-class analysis data computed by [`EGraph::compute_costs`].
///
/// The cost model is recursive: the cost of an e-class is the minimum
/// over its e-nodes of `(op_cost(op) + Σ child class costs)`. The
/// `best_node` field stores the e-node that achieves this minimum
/// (used by [`extract`] to pick the cheapest representative).
#[derive(Debug, Clone, Default)]
pub struct EClassData {
    /// The cached total cost of this e-class. `None` means "not yet
    /// computed" (or "infinite" for cyclic e-graphs, which should not
    /// occur for well-formed dependency graphs).
    pub cost: Option<u32>,
    /// The cheapest e-node in this e-class. `None` means "not yet
    /// computed" or the e-class is empty (which should not occur —
    /// empty e-classes are not created).
    pub best_node: Option<ENode>,
}

/// The e-graph.
///
/// A custom e-graph data structure (no `egg` dependency, per ADR-018).
/// Stores e-classes in a `Vec` (indexed by [`EClassId`]) and maintains
/// a hash-cons (`HashMap<ENode, EClassId>`) for deduplication.
///
/// # Union-find
///
/// The union-find forest is encoded in the `parent` field of each
/// [`EClass`]. [`EGraph::find`] follows parent pointers to the root,
/// applying path compression. [`EGraph::merge`] unions two e-classes
/// by moving the nodes from one into the other and updating the parent
/// pointer.
///
/// # Hash-cons maintenance
///
/// After a merge, the hash-cons may contain stale entries (e-nodes
/// whose children referenced the subsumed class). [`EGraph::rebuild`]
/// walks all e-classes, re-canonicalizes each e-node's children, and
/// rehashes — merging any classes that collide. This is the standard
/// "rebuild" step from the e-graphs literature.
///
/// # Tombstones
///
/// The `dead` set tracks e-class IDs pruned by
/// `dead_store_elimination`. These are skipped during extraction.
#[derive(Debug, Clone)]
pub struct EGraph {
    /// The e-classes, indexed by [`EClassId`]. Index `i` holds the
    /// e-class with `id == i`. Non-root e-classes have empty `nodes`
    /// (their contents have been moved to the root).
    pub classes: Vec<EClass>,
    /// Hash-cons: maps a canonicalized [`ENode`] to its e-class ID.
    /// Used by [`EGraph::add`] to deduplicate e-nodes.
    pub hashcons: HashMap<ENode, EClassId>,
    /// Tombstone set: e-class IDs pruned by `dead_store_elimination`.
    /// [`extract`] skips these when reconstructing the dep graph.
    pub dead: HashSet<EClassId>,
}

impl Default for EGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl EGraph {
    /// Create a new empty e-graph.
    ///
    /// # Example
    ///
    /// ```
    /// use alkalive_compiler::egraph::EGraph;
    /// let eg = EGraph::new();
    /// assert_eq!(eg.classes.len(), 0);
    /// assert!(eg.hashcons.is_empty());
    /// assert!(eg.dead.is_empty());
    /// ```
    pub fn new() -> Self {
        EGraph {
            classes: Vec::new(),
            hashcons: HashMap::new(),
            dead: HashSet::new(),
        }
    }

    /// Returns the number of e-classes (including subsumed non-root
    /// e-classes). For the number of *root* e-classes, use
    /// [`EGraph::root_count`].
    pub fn len(&self) -> usize {
        self.classes.len()
    }

    /// Returns `true` if the e-graph contains no e-classes.
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Returns the number of root e-classes (e-classes whose `parent`
    /// points to themselves).
    pub fn root_count(&self) -> usize {
        self.classes.iter().filter(|c| c.parent == c.id).count()
    }

    /// Iterate over all e-classes (including subsumed non-root ones).
    ///
    /// To iterate only over root e-classes, filter by `|c| c.parent == c.id`.
    pub fn iter(&self) -> impl Iterator<Item = &EClass> {
        self.classes.iter()
    }

    /// Iterate over root e-classes only.
    ///
    /// A root e-class is one whose `parent` points to itself (i.e., it
    /// is its own union-find representative).
    pub fn iter_roots(&self) -> impl Iterator<Item = &EClass> {
        self.classes.iter().filter(|c| c.parent == c.id)
    }

    /// Canonicalize an e-node's children in place: replace each child
    /// ID with its [`EGraph::find`] root.
    ///
    /// This is called by [`EGraph::add`] before hash-cons lookup to
    /// ensure that semantically equal e-nodes (whose children may
    /// reference subsumed classes) hash-cons to the same e-class.
    ///
    /// # Panics
    ///
    /// Panics if any child ID is out of bounds (i.e., `>= classes.len()`).
    /// This indicates a malformed e-graph and should not occur in
    /// well-formed usage.
    pub fn canonicalize(&self, node: &mut ENode) {
        for child in &mut node.children {
            *child = self.find(*child);
        }
    }

    /// Union-find `find` with path compression (read-only variant).
    ///
    /// Returns the root e-class ID for the given ID. Follows parent
    /// pointers until reaching a root (`parent == id`).
    ///
    /// This is the read-only variant — it does not apply path
    /// compression (which would require `&mut self`). For the
    /// compressing variant, use [`EGraph::find_mut`].
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds (i.e., `>= classes.len()`).
    pub fn find(&self, id: EClassId) -> EClassId {
        let mut cur = id;
        loop {
            let class = &self.classes[cur as usize];
            if class.parent == cur {
                return cur;
            }
            cur = class.parent;
        }
    }

    /// Union-find `find` with path halving (mutating variant).
    ///
    /// Like [`EGraph::find`], but applies *path halving* (the variant
    /// named by spec §4.3): each e-class on the walk is re-pointed to
    /// its grandparent, halving the path length per call. Subsequent
    /// `find`/`find_mut` calls walk progressively shorter paths, keeping
    /// amortized near-constant behaviour without the extra root-tracking
    /// walk of full path compression.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds (i.e., `>= classes.len()`).
    pub fn find_mut(&mut self, id: EClassId) -> EClassId {
        // Path halving: point each visited e-class at its grandparent.
        let mut cur = id;
        loop {
            let parent = self.classes[cur as usize].parent;
            if parent == cur {
                return cur;
            }
            let grandparent = self.classes[parent as usize].parent;
            if grandparent == parent {
                // `parent` is the root.
                return parent;
            }
            self.classes[cur as usize].parent = grandparent;
            cur = grandparent;
        }
    }

    /// Add an e-node to the e-graph.
    ///
    /// Canonicalizes the node's children (via [`EGraph::find`]), then
    /// checks the hash-cons:
    /// - If an equivalent e-node already exists, returns its e-class ID
    ///   (no new e-class is created).
    /// - Otherwise, creates a new e-class containing the node, inserts
    ///   it into the hash-cons, and returns the new e-class ID.
    ///
    /// # Example
    ///
    /// ```
    /// use alkalive_compiler::egraph::{EOp, ENode, EGraph};
    /// use alkalive_compiler::SignalId;
    ///
    /// let mut eg = EGraph::new();
    /// let n1 = ENode { op: EOp::SignalRead(SignalId(0)), children: vec![] };
    /// let n2 = ENode { op: EOp::SignalRead(SignalId(0)), children: vec![] };
    /// let id1 = eg.add(n1);
    /// let id2 = eg.add(n2);
    /// // Same node → same e-class (hash-consing).
    /// assert_eq!(id1, id2);
    /// assert_eq!(eg.len(), 1);
    /// ```
    pub fn add(&mut self, mut node: ENode) -> EClassId {
        // Canonicalize children before hash-cons lookup.
        self.canonicalize(&mut node);

        // Check the hash-cons for an existing equivalent e-node.
        if let Some(&existing_id) = self.hashcons.get(&node) {
            let root = self.find(existing_id);
            // The hash-cons might be stale (point to a subsumed class)
            // after a merge. Update it to point to the root.
            if existing_id != root {
                self.hashcons.insert(node.clone(), root);
            }
            return root;
        }

        // Create a new e-class.
        let id = self.classes.len() as EClassId;
        let class = EClass {
            id,
            nodes: vec![node.clone()],
            parent: id, // self-rooted
            data: EClassData::default(),
        };
        self.classes.push(class);
        self.hashcons.insert(node, id);
        id
    }

    /// Merge two e-classes (union).
    ///
    /// Finds the roots of `a` and `b`. If they are the same, does
    /// nothing. Otherwise, moves all e-nodes from the higher-ID class
    /// into the lower-ID class, marks the higher-ID class as subsumed
    /// (sets its `parent` to the lower ID), and triggers a
    /// [`EGraph::rebuild`] to rehash any e-nodes that referenced the
    /// subsumed class.
    ///
    /// # Determinism
    ///
    /// The lower-ID class always wins. This ensures that merge results
    /// are deterministic (independent of merge order), which is
    /// important for reproducible builds and test stability.
    ///
    /// # Panics
    ///
    /// Panics if `a` or `b` is out of bounds.
    pub fn merge(&mut self, a: EClassId, b: EClassId) -> EClassId {
        let ra = self.find_mut(a);
        let rb = self.find_mut(b);
        if ra == rb {
            return ra;
        }
        // The lower-ID class wins (deterministic).
        let (winner, loser) = if ra < rb { (ra, rb) } else { (rb, ra) };

        // Move all e-nodes from the loser into the winner.
        let loser_nodes = std::mem::take(&mut self.classes[loser as usize].nodes);
        self.classes[winner as usize].nodes.extend(loser_nodes);

        // Mark the loser as subsumed.
        self.classes[loser as usize].parent = winner;

        // If the loser was marked dead, propagate the tombstone to the
        // winner (a merged class that includes a dead class is itself
        // dead).
        if self.dead.contains(&loser) {
            self.dead.insert(winner);
            self.dead.remove(&loser);
        }

        // Rebuild the hash-cons to reflect the merge.
        self.rebuild();

        winner
    }

    /// Rebuild the hash-cons after a merge.
    ///
    /// Walks all root e-classes, re-canonicalizes each e-node's children
    /// (which may now point to the winner rather than the subsumed
    /// class), and rehashes. If two e-classes end up with the same
    /// canonicalized e-node, they are merged (this is the standard
    /// "rebuild" step from the e-graphs literature — it restores the
    /// hash-cons invariant after a union).
    ///
    /// Returns `true` if any further merges occurred (indicating that
    /// the e-graph is not yet at a fixpoint and rebuild should be
    /// called again).
    ///
    /// # Panics
    ///
    /// Panics if any e-class ID is out of bounds (should not occur in
    /// well-formed usage).
    pub fn rebuild(&mut self) -> bool {
        let mut new_hashcons: HashMap<ENode, EClassId> = HashMap::new();
        let mut merges_needed: Vec<(EClassId, EClassId)> = Vec::new();

        // Walk all root e-classes and rehash their e-nodes.
        for class_idx in 0..self.classes.len() {
            let id = class_idx as EClassId;
            if self.classes[class_idx].parent != id {
                continue; // skip subsumed classes
            }
            // Re-canonicalize each e-node in this class.
            for node_idx in 0..self.classes[class_idx].nodes.len() {
                let mut node = self.classes[class_idx].nodes[node_idx].clone();
                self.canonicalize(&mut node);
                self.classes[class_idx].nodes[node_idx] = node.clone();

                // Check if this canonicalized e-node already exists in
                // another e-class.
                if let Some(&existing) = new_hashcons.get(&node) {
                    if existing != id {
                        merges_needed.push((existing, id));
                    }
                } else {
                    new_hashcons.insert(node, id);
                }
            }
        }

        self.hashcons = new_hashcons;

        // Apply any merges that were discovered.
        let changed = !merges_needed.is_empty();
        for (a, b) in merges_needed {
            self.merge(a, b);
        }
        changed
    }

    /// Mark an e-class as dead (tombstone).
    ///
    /// Used by `dead_store_elimination` to prune e-classes whose
    /// values are never read. [`extract`] skips dead e-classes when
    /// reconstructing the [`DependencyGraph`].
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds.
    pub fn mark_dead(&mut self, id: EClassId) {
        let root = self.find_mut(id);
        self.dead.insert(root);
    }

    /// Returns `true` if the e-class (or its root) is marked dead.
    pub fn is_dead(&self, id: EClassId) -> bool {
        let root = self.find(id);
        self.dead.contains(&root)
    }

    /// Returns all e-nodes with the given operation kind.
    ///
    /// Walks all root e-classes and collects e-nodes whose `op`
    /// matches. Used by the rewrite rules to find pattern candidates.
    ///
    /// Returns a list of `(e_class_id, e_node)` tuples.
    pub fn find_nodes_with_op(&self, op_kind: EOpKind) -> Vec<(EClassId, ENode)> {
        let mut results = Vec::new();
        for class in self.iter_roots() {
            for node in &class.nodes {
                if op_kind.matches(&node.op) {
                    results.push((class.id, node.clone()));
                }
            }
        }
        results
    }

    /// Returns all e-classes containing a `SignalRead(s)` e-node for
    /// the given signal `s`.
    ///
    /// Used by `read_merge` and `state_store_load_forward` to find
    /// candidate reads.
    pub fn find_read_classes(&self, sig: SignalId) -> Vec<EClassId> {
        let mut results = Vec::new();
        for class in self.iter_roots() {
            for node in &class.nodes {
                if let EOp::SignalRead(s) = &node.op {
                    if *s == sig {
                        results.push(class.id);
                        break; // one match per class is enough
                    }
                }
            }
        }
        results
    }

    /// Returns all e-classes containing a `SignalWrite(s)` e-node for
    /// the given signal `s`.
    ///
    /// Used by `state_store_load_forward` and
    /// `dead_store_elimination` to find candidate writes.
    pub fn find_write_classes(&self, sig: SignalId) -> Vec<EClassId> {
        let mut results = Vec::new();
        for class in self.iter_roots() {
            for node in &class.nodes {
                if let EOp::SignalWrite(s) = &node.op {
                    if *s == sig {
                        results.push(class.id);
                        break;
                    }
                }
            }
        }
        results
    }

    /// Returns the `pass_index` payload of the `Pass` e-node in the
    /// given e-class, if any.
    ///
    /// Used by `dead_store_elimination` to determine the ordering of
    /// writes (the pass that performs the write is the
    /// `SignalWrite`'s child e-class).
    pub fn pass_index_of(&self, id: EClassId) -> Option<usize> {
        let root = self.find(id);
        let class = &self.classes[root as usize];
        for node in &class.nodes {
            if let EOp::Pass(idx) = &node.op {
                return Some(*idx);
            }
        }
        None
    }

    /// Compute the cost of every root e-class.
    ///
    /// Walks the e-graph in dependency order (children before parents)
    /// and computes the cost of each e-class as the minimum over its
    /// e-nodes of `(op_cost(op) + Σ child class costs)`. Stores the
    /// result in [`EClassData::cost`] and the cheapest e-node in
    /// [`EClassData::best_node`].
    ///
    /// # Cycles
    ///
    /// If the e-graph contains a cycle (which should not occur for
    /// well-formed dependency graphs), the cyclic e-classes are
    /// assigned a cost of `u32::MAX` to prevent infinite recursion.
    ///
    /// # Panics
    ///
    /// Panics if any e-class ID is out of bounds.
    pub fn compute_costs(&mut self) {
        // Reset all costs.
        for class in &mut self.classes {
            class.data.cost = None;
            class.data.best_node = None;
        }

        // Compute costs for root e-classes. We use a recursive helper
        // with a "visiting" set to detect cycles.
        let mut visiting: HashSet<EClassId> = HashSet::new();
        let root_ids: Vec<EClassId> = self
            .classes
            .iter()
            .filter(|c| c.parent == c.id)
            .map(|c| c.id)
            .collect();

        for root_id in root_ids {
            self.compute_cost_recursive(root_id, &mut visiting);
        }
    }

    /// Recursive helper for [`EGraph::compute_costs`].
    ///
    /// Computes the cost of `id` (and its transitive children) using
    /// memoization. The `visiting` set tracks e-classes currently on
    /// the recursion stack to detect cycles.
    fn compute_cost_recursive(&mut self, id: EClassId, visiting: &mut HashSet<EClassId>) -> u32 {
        let root = self.find(id);
        // Already computed?
        if let Some(cost) = self.classes[root as usize].data.cost {
            return cost;
        }
        // Cycle detected?
        if visiting.contains(&root) {
            return u32::MAX;
        }
        visiting.insert(root);

        // Compute the cost of each e-node in this class.
        let nodes = self.classes[root as usize].nodes.clone();
        let mut best_cost = u32::MAX;
        let mut best_node: Option<ENode> = None;

        for node in &nodes {
            let mut node_cost = op_cost(&node.op);
            let mut valid = true;
            for &child in &node.children {
                let child_root = self.find(child);
                let child_cost = self.compute_cost_recursive(child_root, visiting);
                if child_cost == u32::MAX {
                    valid = false;
                    break;
                }
                node_cost = node_cost.saturating_add(child_cost);
            }
            if valid && node_cost < best_cost {
                best_cost = node_cost;
                best_node = Some(node.clone());
            }
        }

        self.classes[root as usize].data.cost = Some(best_cost);
        self.classes[root as usize].data.best_node = best_node;
        visiting.remove(&root);
        best_cost
    }

    /// Returns the cached cost of an e-class (after
    /// [`EGraph::compute_costs`] has been called).
    pub fn cost_of(&self, id: EClassId) -> u32 {
        let root = self.find(id);
        self.classes[root as usize].data.cost.unwrap_or(u32::MAX)
    }

    /// Returns the cheapest e-node in an e-class (after
    /// [`EGraph::compute_costs`] has been called).
    pub fn best_node_of(&self, id: EClassId) -> Option<&ENode> {
        let root = self.find(id);
        self.classes[root as usize].data.best_node.as_ref()
    }
}

/// A discriminator for [`EOp`] variants, used by pattern matching in
/// the rewrite rules.
///
/// This avoids the need for a full pattern-matching DSL — we match on
/// the variant kind plus the payload (e.g., `SignalRead(SignalId)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EOpKind {
    /// Matches `EOp::SignalRead(_)`.
    SignalRead,
    /// Matches `EOp::SignalWrite(_)`.
    SignalWrite,
    /// Matches `EOp::Pass(_)`.
    Pass,
    /// Matches `EOp::Const(_)`.
    Const,
}

impl EOpKind {
    /// Returns `true` if the given [`EOp`] matches this kind (ignoring
    /// the payload).
    pub fn matches(&self, op: &EOp) -> bool {
        matches!(
            (self, op),
            (EOpKind::SignalRead, EOp::SignalRead(_))
                | (EOpKind::SignalWrite, EOp::SignalWrite(_))
                | (EOpKind::Pass, EOp::Pass(_))
                | (EOpKind::Const, EOp::Const(_))
        )
    }
}

// =============================================================================
// Cost model
// =============================================================================

/// Returns the base cost of an [`EOp`] (without children).
///
/// The cost heuristic is:
///
/// ```text
/// SignalRead (1) < Pass (2) < SignalWrite (3) < Const (4)
/// ```
///
/// This ordering reflects the relative expense of each operation in the
/// runtime:
/// - `SignalRead` is cheapest (a single hash-map lookup in the
///   `SignalStore`).
/// - `Pass` is a full computation (text shaping, vertex buffer
///   construction, etc.).
/// - `SignalWrite` is more expensive (a hash-map update plus dirty
///   propagation).
/// - `Const` is most expensive (it requires storage and prevents
///   future re-evaluation from being incremental).
///
/// Note: this differs from the classic e-graph cost model where
/// constants are cheapest. For AlkALive's reactive signal model,
/// constants are *storage-bound* and thus more expensive than a
/// signal read.
pub fn op_cost(op: &EOp) -> u32 {
    match op {
        EOp::SignalRead(_) => 1,
        EOp::Pass(_) => 2,
        EOp::SignalWrite(_) => 3,
        EOp::Const(_) => 4,
    }
}

/// Returns the total cost of an e-node: `op_cost(op) + Σ child class
/// costs`.
///
/// This requires the e-graph's costs to have been computed (via
/// [`EGraph::compute_costs`]). If a child's cost has not been
/// computed, it is treated as `u32::MAX`.
pub fn node_cost(eg: &EGraph, node: &ENode) -> u32 {
    let mut cost = op_cost(&node.op);
    for &child in &node.children {
        let child_cost = eg.cost_of(child);
        if child_cost == u32::MAX {
            return u32::MAX;
        }
        cost = cost.saturating_add(child_cost);
    }
    cost
}

/// Returns the cost of an e-class: the minimum over its e-nodes of
/// `node_cost`.
///
/// This requires the e-graph's costs to have been computed.
pub fn class_cost(eg: &EGraph, id: EClassId) -> u32 {
    eg.cost_of(id)
}

// =============================================================================
// Rewrite rules
// =============================================================================

/// A rewrite rule over the e-graph (spec §4.3: "Rewrite rules:
/// `RewriteRule` trait, `state_store_load_forward`,
/// `dead_store_elimination`, `read_merge`, `evaluation_reorder`").
///
/// The three e-graph-rewriting rules implement this trait and are
/// applied to a fixpoint by [`egraph_optimization`] via the
/// [`RULES`] registry. The fourth rule, [`evaluation_reorder`], is a
/// *scheduling* rule: it topologically orders the extracted node list
/// inside [`extract`] — the point where a node order first exists — so
/// it remains a free function rather than a trait impl (its input is
/// `&mut Vec<DepNode>`, not the e-graph).
pub trait RewriteRule {
    /// Stable, human-readable rule name (diagnostics + tests).
    fn name(&self) -> &'static str;
    /// Apply the rule to the e-graph. Returns `true` iff the e-graph
    /// changed (at least one e-class merge was performed).
    fn apply(&self, eg: &mut EGraph) -> bool;
}

/// Rule 1: `state_store_load_forward` (see
/// [`apply_state_store_load_forward`]).
pub struct StateStoreLoadForward;

impl RewriteRule for StateStoreLoadForward {
    fn name(&self) -> &'static str {
        "state_store_load_forward"
    }
    fn apply(&self, eg: &mut EGraph) -> bool {
        apply_state_store_load_forward(eg)
    }
}

/// Rule 2: `dead_store_elimination` (see
/// [`apply_dead_store_elimination`]).
pub struct DeadStoreElimination;

impl RewriteRule for DeadStoreElimination {
    fn name(&self) -> &'static str {
        "dead_store_elimination"
    }
    fn apply(&self, eg: &mut EGraph) -> bool {
        apply_dead_store_elimination(eg)
    }
}

/// Rule 3: `read_merge` (see [`apply_read_merge`]).
pub struct ReadMerge;

impl RewriteRule for ReadMerge {
    fn name(&self) -> &'static str {
        "read_merge"
    }
    fn apply(&self, eg: &mut EGraph) -> bool {
        apply_read_merge(eg)
    }
}

/// The fixpoint rule registry: every e-graph rewrite rule, in
/// application order (rules 1–3; rule 4, [`evaluation_reorder`], runs
/// at extraction). [`egraph_optimization`] iterates this registry
/// until no rule reports a change.
pub const RULES: &[&dyn RewriteRule] = &[&StateStoreLoadForward, &DeadStoreElimination, &ReadMerge];

/// Rewrite rule 1: `state_store_load_forward`.
///
/// If `SignalWrite(s, v)` exists and a `SignalRead(s)` exists in the
/// e-graph, merge the `SignalRead(s)` e-class with `v`'s e-class —
/// but only if `v`'s pass is the *latest preceding* write of `s`
/// before the read's pass. This avoids incorrectly merging two
/// different writes' values when a signal is written multiple times.
///
/// # Semantics
///
/// In the original dependency graph, a `SignalWrite(s, v)` followed by
/// a `SignalRead(s)` in the same dependency chain can be optimized:
/// the read can be replaced with a direct reference to `v` (the
/// written value), eliminating the signal-store round-trip.
///
/// In e-graph terms, this is an equivalence: the value of
/// `SignalRead(s)` (after the write) equals `v`. We add this
/// equivalence by merging the `SignalRead(s)` e-class with `v`'s
/// e-class.
///
/// # Latest-preceding-write constraint
///
/// If a signal `s` is written by multiple passes (e.g., pass 0 and
/// pass 1 both write `s`), only the *latest* write before a read is
/// relevant. A read at pass `p_read` should be merged with the value
/// of the write whose `pass_index` is the largest `p_write` such
/// that `p_write < p_read`. Earlier writes are superseded and
/// should not be forwarded (they're handled by
/// `dead_store_elimination`).
///
/// # Pattern
///
/// ```text
/// for each Pass(p_read) with child SignalRead(s):
///   find the latest SignalWrite(s, v) with pass_index(v) < p_read
///   if found: merge(SignalRead(s)_class, v_class)
/// ```
///
/// # Returns
///
/// `true` if any merges were applied (the e-graph changed); `false`
/// otherwise.
///
/// # Hello World
///
/// For the canonical Hello World scene (all passes have empty
/// `outputs`), there are no `SignalWrite` e-nodes, so this rule is a
/// no-op.
pub fn apply_state_store_load_forward(eg: &mut EGraph) -> bool {
    // Collect all (signal, value_class, write_pass_index) tuples from
    // SignalWrite e-nodes.
    let mut writes: Vec<(SignalId, EClassId, usize)> = Vec::new();
    for class in eg.iter_roots() {
        for node in &class.nodes {
            if let EOp::SignalWrite(s) = &node.op {
                if let Some(&child) = node.children.first() {
                    let child_root = eg.find(child);
                    if let Some(pass_idx) = eg.pass_index_of(child_root) {
                        writes.push((*s, child_root, pass_idx));
                    }
                }
            }
        }
    }

    // Collect all (signal, read_class, read_pass_index) tuples by
    // looking at Pass e-nodes' children. This tells us which pass
    // reads which signal, allowing us to apply the
    // latest-preceding-write constraint.
    let mut reads: Vec<(SignalId, EClassId, usize)> = Vec::new();
    for class in eg.iter_roots() {
        for node in &class.nodes {
            if let EOp::Pass(pass_idx) = &node.op {
                for &child in &node.children {
                    let child_root = eg.find(child);
                    let child_class = &eg.classes[child_root as usize];
                    for child_node in &child_class.nodes {
                        if let EOp::SignalRead(s) = &child_node.op {
                            reads.push((*s, child_root, *pass_idx));
                        }
                    }
                }
            }
        }
    }

    let mut changed = false;
    for (sig, read_class, read_pass_idx) in reads {
        // Find the latest preceding write of `sig` (write_pass_idx < read_pass_idx).
        let latest_write = writes
            .iter()
            .filter(|(s, _, wp)| *s == sig && *wp < read_pass_idx)
            .max_by_key(|(_, _, wp)| *wp);

        if let Some((_, value_class, _)) = latest_write {
            let read_root = eg.find(read_class);
            let value_root = eg.find(*value_class);
            if read_root != value_root {
                eg.merge(read_root, value_root);
                changed = true;
            }
        }
    }

    changed
}

/// Rewrite rule 2: `dead_store_elimination`.
///
/// If `SignalWrite(s, v1)` is followed by `SignalWrite(s, v2)` with no
/// `SignalRead(s)` between them (i.e., no `Pass` whose `pass_index`
/// is between v1's pass_index and v2's pass_index reads signal `s`),
/// mark v1's e-class as dead.
///
/// # Semantics
///
/// A "dead store" is a write whose value is never read before being
/// overwritten. Eliminating it removes the wasted invalidation work.
///
/// In e-graph terms, we don't physically remove the e-class (e-graphs
/// don't support deletion). Instead, we mark it as dead via
/// [`EGraph::mark_dead`], and [`extract`] skips dead e-classes when
/// reconstructing the [`DependencyGraph`].
///
/// # Pattern
///
/// ```text
/// SignalWrite(s, v1)  [pass_index = p1]
/// SignalWrite(s, v2)  [pass_index = p2, p2 > p1]
///   with no Pass p where p1 < p < p2 reads signal s
///   ──►  mark_dead(v1)
/// ```
///
/// # Returns
///
/// `true` if any e-classes were marked dead; `false` otherwise.
///
/// # Hello World
///
/// For the canonical Hello World scene (all passes have empty
/// `outputs`), there are no `SignalWrite` e-nodes, so this rule is a
/// no-op.
pub fn apply_dead_store_elimination(eg: &mut EGraph) -> bool {
    // Collect all (signal, value_class, pass_index) tuples from
    // SignalWrite e-nodes.
    let mut writes: Vec<(SignalId, EClassId, usize)> = Vec::new();
    for class in eg.iter_roots() {
        for node in &class.nodes {
            if let EOp::SignalWrite(s) = &node.op {
                if let Some(&child) = node.children.first() {
                    let child_root = eg.find(child);
                    if let Some(pass_idx) = eg.pass_index_of(child_root) {
                        writes.push((*s, child_root, pass_idx));
                    }
                }
            }
        }
    }

    // Collect all (signal, pass_index) tuples for SignalReads. We
    // determine the pass_index of a read by finding the Pass e-class
    // that has the read as a child.
    let mut reads: Vec<(SignalId, usize)> = Vec::new();
    for class in eg.iter_roots() {
        for node in &class.nodes {
            if let EOp::Pass(pass_idx) = &node.op {
                // For each child of this Pass, check if it's a
                // SignalRead and record the (signal, pass_index).
                for &child in &node.children {
                    let child_root = eg.find(child);
                    let child_class = &eg.classes[child_root as usize];
                    for child_node in &child_class.nodes {
                        if let EOp::SignalRead(s) = &child_node.op {
                            reads.push((*s, *pass_idx));
                        }
                    }
                }
            }
        }
    }

    // For each signal, sort writes by pass_index. For each consecutive
    // pair (w1, w2), check if there's a read with pass_index in
    // (w1.pass_index, w2.pass_index). If not, mark w1 as dead.
    let mut signals: HashSet<SignalId> = HashSet::new();
    for (s, _, _) in &writes {
        signals.insert(*s);
    }

    let mut changed = false;
    for sig in signals {
        let mut sig_writes: Vec<(EClassId, usize)> = writes
            .iter()
            .filter(|(s, _, _)| *s == sig)
            .map(|(_, v, p)| (*v, *p))
            .collect();
        sig_writes.sort_by_key(|(_, p)| *p);

        for i in 0..sig_writes.len().saturating_sub(1) {
            let (v1, p1) = sig_writes[i];
            let (_, p2) = sig_writes[i + 1];
            // Check if any read of `sig` has pass_index in (p1, p2).
            let has_read_between = reads.iter().any(|(s, p)| *s == sig && *p > p1 && *p < p2);
            if !has_read_between && !eg.is_dead(v1) {
                eg.mark_dead(v1);
                changed = true;
            }
        }
    }

    changed
}

/// Rewrite rule 3: `read_merge`.
///
/// If two e-classes both contain a `SignalRead(s)` e-node for the same
/// signal `s`, merge them into a single e-class.
///
/// # Semantics
///
/// Two reads of the same signal produce the same value, so their
/// e-classes are equivalent. Merging them deduplicates the read,
/// reducing the number of dependency checks at runtime.
///
/// # Hash-consing
///
/// [`EGraph::add`] already hash-conses e-nodes: two identical
/// `SignalRead(s)` e-nodes (with no children) hash-cons to the same
/// e-class at insertion time. This rule handles the case where two
/// *different* e-classes end up containing `SignalRead(s)` e-nodes
/// due to other rewrite rules adding new reads or merging classes.
///
/// # Pattern
///
/// ```text
/// class A contains SignalRead(s)
/// class B contains SignalRead(s)
///   ──►  merge(A, B)
/// ```
///
/// # Returns
///
/// `true` if any merges were applied; `false` otherwise.
///
/// # Hello World
///
/// For the canonical Hello World scene, hash-consing during
/// [`EGraph::add`] already merges all `SignalRead(s)` e-nodes for the
/// same `s` into a single e-class. This rule therefore finds no work
/// to do and returns `false`.
pub fn apply_read_merge(eg: &mut EGraph) -> bool {
    // Collect all (signal, e_class_id) pairs from SignalRead e-nodes.
    let mut reads: Vec<(SignalId, EClassId)> = Vec::new();
    for class in eg.iter_roots() {
        for node in &class.nodes {
            if let EOp::SignalRead(s) = &node.op {
                reads.push((*s, class.id));
            }
        }
    }

    // Group by signal.
    let mut by_signal: HashMap<SignalId, Vec<EClassId>> = HashMap::new();
    for (sig, class_id) in reads {
        by_signal.entry(sig).or_default().push(class_id);
    }

    let mut changed = false;
    for (_, class_ids) in by_signal {
        if class_ids.len() < 2 {
            continue;
        }
        // Merge all classes for this signal into the first one.
        let first = eg.find(class_ids[0]);
        for &other in &class_ids[1..] {
            let other_root = eg.find(other);
            if other_root != first {
                eg.merge(first, other_root);
                changed = true;
            }
        }
    }

    changed
}

/// Rewrite rule 4: `evaluation_reorder`.
///
/// This rule is *not* an e-class merge — it is a scheduling constraint
/// applied during [`extract`]. It topologically sorts the extracted
/// [`DepNode`]s so that producers (passes that write a signal) are
/// evaluated before consumers (passes that read that signal).
///
/// # Semantics
///
/// Without reordering, a consumer might be evaluated before its
/// producer, causing the consumer to read a stale value and triggering
/// a re-evaluation on the next frame. Topological sorting eliminates
/// this waste.
///
/// # Algorithm
///
/// 1. Build a dependency graph: for each pair of passes (A, B), add an
///    edge A → B if A writes a signal that B reads (A must execute
///    before B).
/// 2. Topologically sort using Kahn's algorithm.
/// 3. If there are ties (multiple valid orderings), preserve the
///    original pass order (stable sort).
///
/// # Cycles
///
/// If the dependency graph contains a cycle (which should not occur
/// for well-formed dependency graphs — passes form a DAG), the
/// topological sort falls back to the original order for the cyclic
/// nodes.
///
/// This function is called from [`extract`]; it is not a standalone
/// rewrite rule that modifies the e-graph.
pub fn evaluation_reorder(nodes: &mut Vec<DepNode>) {
    if nodes.len() <= 1 {
        return;
    }

    // Build adjacency: for each pass A, find passes B that must come
    // after A (because A writes a signal that B reads).
    let n = nodes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    // Map pass_index → position in `nodes` (after reordering, the
    // position may differ from pass_index, but for the initial call
    // from extract, positions match pass_index).
    let mut pos_by_pass: HashMap<usize, usize> = HashMap::new();
    for (pos, node) in nodes.iter().enumerate() {
        pos_by_pass.insert(node.pass_index, pos);
    }

    for (a_pos, a_node) in nodes.iter().enumerate() {
        for &written_sig in &a_node.outputs {
            for (b_pos, b_node) in nodes.iter().enumerate() {
                if a_pos == b_pos {
                    continue;
                }
                if b_node.inputs.contains(&written_sig) {
                    // A writes a signal that B reads: A → B.
                    adj[a_pos].push(b_pos);
                    in_degree[b_pos] += 1;
                }
            }
        }
    }
    let _ = &pos_by_pass; // (reserved for future tie-breaking)

    // Kahn's algorithm with stable ordering: pick the node with the
    // lowest original position among those with in_degree 0.
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut remaining: Vec<bool> = vec![true; n];
    for _ in 0..n {
        // Find the lowest-position node with in_degree 0 and remaining.
        let next = (0..n).filter(|&i| remaining[i] && in_degree[i] == 0).min();
        match next {
            Some(i) => {
                order.push(i);
                remaining[i] = false;
                for &j in &adj[i] {
                    in_degree[j] = in_degree[j].saturating_sub(1);
                }
            }
            None => {
                // Cycle: append remaining nodes in original order.
                for (i, &is_remaining) in remaining.iter().enumerate() {
                    if is_remaining {
                        order.push(i);
                    }
                }
                break;
            }
        }
    }

    // Apply the ordering.
    let mut new_nodes: Vec<DepNode> = Vec::with_capacity(n);
    for &i in &order {
        new_nodes.push(nodes[i].clone());
    }
    *nodes = new_nodes;
}

// =============================================================================
// Extraction
// =============================================================================

/// Extract the optimized dependency graph from the e-graph.
///
/// Selects the cheapest equivalent form per e-class (via
/// [`EGraph::compute_costs`]) and reconstructs a [`DependencyGraph`].
/// Applies [`evaluation_reorder`] (rule 4) as a final topological sort.
///
/// # Algorithm
///
/// 1. Compute costs for all e-classes.
/// 2. For each surviving (non-dead) `Pass` e-class:
///    - Pick the cheapest `Pass` e-node (the "best" representative).
///    - Reconstruct `inputs` from the e-node's children (each child is
///      a `SignalRead` e-class — extract the signal ID from the
///      cheapest e-node in that class).
///    - Reconstruct `outputs` by finding all surviving `SignalWrite`
///      e-classes whose child (canonicalized) is this `Pass` e-class.
///    - Build a [`DepNode`] with the reconstructed inputs/outputs.
/// 3. Topologically sort the resulting `DepNode`s via
///    [`evaluation_reorder`].
/// 4. Return as a [`DependencyGraph`].
///
/// # Cost heuristic
///
/// See [`op_cost`]: `SignalRead (1) < Pass (2) < SignalWrite (3) < Const (4)`.
///
/// # Stability
///
/// For the canonical Hello World scene (all empty `outputs`), the
/// extracted dep graph is structurally identical to the input: the
/// `Pass` e-classes survive, their `SignalRead` children are
/// reconstructed as inputs, and there are no `SignalWrite` e-classes
/// to prune. The topological sort finds no inter-pass dependencies
/// (no pass writes a signal that another reads), so the original
/// order is preserved.
pub fn extract(graph: &EGraph, original: &DependencyGraph) -> DependencyGraph {
    // We need mutable access to compute costs, but the signature takes
    // `&EGraph`. Clone the graph to compute costs locally.
    let mut eg = graph.clone();
    eg.compute_costs();

    // Find all surviving (non-dead) Pass e-nodes, sorted by pass_index
    // for determinism. We iterate over ALL Pass e-nodes (not just the
    // best_node per class) because a class may contain both a Pass and
    // a SignalRead (after a `state_store_load_forward` merge) — the
    // best_node would be the cheaper SignalRead, but we still need to
    // extract the Pass as a DepNode.
    let mut pass_nodes: Vec<(usize, EClassId, ENode)> = Vec::new();
    for class in eg.iter_roots() {
        if eg.is_dead(class.id) {
            continue;
        }
        for node in &class.nodes {
            if let EOp::Pass(pass_idx) = &node.op {
                pass_nodes.push((*pass_idx, class.id, node.clone()));
            }
        }
    }
    // Sort by pass_index and deduplicate (safety net: if a Pass e-node
    // somehow appears in multiple classes, we take the first).
    pass_nodes.sort_by_key(|(idx, _, _)| *idx);
    pass_nodes.dedup_by_key(|(idx, _, _)| *idx);

    // For each Pass e-node, reconstruct inputs and outputs.
    let mut new_nodes: Vec<DepNode> = Vec::new();
    for (pass_idx, pass_class, pass_node) in &pass_nodes {
        let pass_class_root = eg.find(*pass_class);

        // Reconstruct inputs from the Pass e-node's children. Each
        // child should be a SignalRead e-class; extract the signal ID
        // from any SignalRead e-node in that class.
        let mut inputs: Vec<SignalId> = Vec::new();
        for &child in &pass_node.children {
            let child_root = eg.find(child);
            if eg.is_dead(child_root) {
                continue;
            }
            let child_class = &eg.classes[child_root as usize];
            for child_node in &child_class.nodes {
                if let EOp::SignalRead(s) = &child_node.op {
                    if !inputs.contains(s) {
                        inputs.push(*s);
                    }
                    break; // one SignalRead per child class is enough
                }
            }
        }

        // Reconstruct outputs: find all surviving SignalWrite e-nodes
        // whose child (canonicalized) is this Pass e-class.
        let mut outputs: Vec<SignalId> = Vec::new();
        for class in eg.iter_roots() {
            if eg.is_dead(class.id) {
                continue;
            }
            for node in &class.nodes {
                if let EOp::SignalWrite(s) = &node.op {
                    if let Some(&child) = node.children.first() {
                        let child_root = eg.find(child);
                        if child_root == pass_class_root && !outputs.contains(s) {
                            outputs.push(*s);
                        }
                    }
                }
            }
        }

        // Preserve the original description (look it up by pass_index).
        let description = original
            .node_for_pass(*pass_idx)
            .map(|n| n.description.clone())
            .unwrap_or_else(|| format!("Pass({})", pass_idx));

        new_nodes.push(DepNode {
            id: DepNodeId(new_nodes.len() as u32),
            inputs,
            outputs,
            pass_index: *pass_idx,
            description,
        });
    }

    // Apply evaluation_reorder (topological sort).
    evaluation_reorder(&mut new_nodes);

    // Reassign IDs to be dense 0..n.
    for (i, node) in new_nodes.iter_mut().enumerate() {
        node.id = DepNodeId(i as u32);
    }

    DependencyGraph { nodes: new_nodes }
}

// =============================================================================
// Entry point
// =============================================================================

/// Optimize a dependency graph using e-graph rewriting.
///
/// This is the ADR-026 entry point. It runs the full e-graph
/// optimization pipeline:
///
/// 1. **Build** the e-graph from the dependency graph (see
///    [`build_from_dep_graph`]).
/// 2. **Apply rewrite rules to a fixpoint**: iterate the [`RULES`]
///    registry (rules 1–3: [`StateStoreLoadForward`],
///    [`DeadStoreElimination`], [`ReadMerge`]) until none of them
///    report a change. (`evaluation_reorder`, rule 4, is applied
///    during extraction, not as a merge.)
/// 3. **Extract** the optimized dependency graph via [`extract`].
///
/// # Hello World
///
/// For the canonical Hello World scene (5 passes, 6 signals, all empty
/// `outputs`), the optimization is a no-op: hash-consing during the
/// build phase already merges all `SignalRead(s)` e-nodes for the same
/// `s` into a single e-class (rule 3, `read_merge`), and there are no
/// `SignalWrite` e-nodes for rules 1 and 2 to act on. The extracted
/// dep graph is structurally identical to the input.
///
/// # Example
///
/// ```
/// use alkalive_compiler::egraph::egraph_optimization;
/// use alkalive_compiler::{DependencyGraph, DepNode, DepNodeId, SignalId};
///
/// let graph = DependencyGraph {
///     nodes: vec![
///         DepNode {
///             id: DepNodeId(0),
///             inputs: vec![SignalId(0)],
///             outputs: vec![],
///             pass_index: 0,
///             description: "Clear".into(),
///         },
///     ],
/// };
/// let optimized = egraph_optimization(&graph);
/// assert_eq!(optimized.nodes.len(), 1);
/// ```
pub fn egraph_optimization(dep_graph: &DependencyGraph) -> DependencyGraph {
    let mut eg = EGraph::new();

    // 1. Build: add all nodes from dep_graph.
    build_from_dep_graph(&mut eg, dep_graph);

    // 2. Apply rewrite rules to fixpoint via the RULES registry.
    let mut changed = true;
    let mut iterations = 0u32;
    const MAX_ITERATIONS: u32 = 1024; // safety bound; the rules should converge in <<1024 iterations.
    while changed && iterations < MAX_ITERATIONS {
        changed = false;
        for rule in RULES {
            changed |= rule.apply(&mut eg);
        }
        iterations += 1;
    }

    // 3. Extract.
    extract(&eg, dep_graph)
}

/// Build an e-graph from a [`DependencyGraph`].
///
/// For each [`DepNode`] in `dep_graph.nodes`:
///
/// 1. Add a `SignalRead(s)` e-node for each input signal `s`. These
///    are leaves (no children). Hash-consing ensures that multiple
///    passes reading the same signal share a single e-class (this is
///    the `read_merge` rule applied at insertion time).
/// 2. Add a `Pass(pass_index)` e-node with the read e-classes as
///    children. This links the pass to its inputs.
/// 3. Add a `SignalWrite(s)` e-node for each output signal `s`, with
///    the pass e-class as its single child. This links the write to
///    the pass that performs it.
///
/// # Deviation from the ADR-026 spec sketch
///
/// The ADR-026 spec's build code creates `Pass` e-nodes with *no*
/// children (the reads are added as standalone e-nodes). This
/// implementation deviates: `Pass` e-nodes have their `SignalRead`
/// children attached. This makes the e-graph self-contained (the
/// pass-to-read association is encoded in the e-graph itself, not in
/// a side-table), which simplifies [`extract`] and makes the
/// rewrite rules more precise (e.g., `dead_store_elimination` can
/// determine which passes read a signal by walking the `Pass`
/// e-nodes' children).
pub fn build_from_dep_graph(eg: &mut EGraph, dep_graph: &DependencyGraph) {
    for node in &dep_graph.nodes {
        // 1. Add SignalRead e-nodes for each input.
        let mut read_classes: Vec<EClassId> = Vec::with_capacity(node.inputs.len());
        for &sig in &node.inputs {
            let read_node = ENode {
                op: EOp::SignalRead(sig),
                children: vec![],
            };
            let read_class = eg.add(read_node);
            read_classes.push(read_class);
        }

        // 2. Add the Pass e-node with the read e-classes as children.
        let pass_node = ENode {
            op: EOp::Pass(node.pass_index),
            children: read_classes,
        };
        let pass_class = eg.add(pass_node);

        // 3. Add SignalWrite e-nodes for each output, with the pass
        //    e-class as their single child.
        for &sig in &node.outputs {
            let write_node = ENode {
                op: EOp::SignalWrite(sig),
                children: vec![pass_class],
            };
            eg.add(write_node);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::{signals, DepNode, DepNodeId, DependencyGraph, SignalId};

    // ---- Helper: build a small synthetic dep graph ----

    /// Build a dep graph with two passes: pass 0 writes signal S, pass 1
    /// reads signal S. Used to test `state_store_load_forward` and
    /// `dead_store_elimination`.
    fn graph_with_write_then_read() -> DependencyGraph {
        DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![],
                    outputs: vec![SignalId(0)],
                    pass_index: 0,
                    description: "Producer".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![SignalId(0)],
                    outputs: vec![],
                    pass_index: 1,
                    description: "Consumer".into(),
                },
            ],
        }
    }

    /// Build a dep graph with two writes to the same signal, no read
    /// between them. Used to test `dead_store_elimination`.
    fn graph_with_dead_store() -> DependencyGraph {
        DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![],
                    outputs: vec![SignalId(0)],
                    pass_index: 0,
                    description: "FirstWrite".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![],
                    outputs: vec![SignalId(0)],
                    pass_index: 1,
                    description: "SecondWrite".into(),
                },
                DepNode {
                    id: DepNodeId(2),
                    inputs: vec![SignalId(0)],
                    outputs: vec![],
                    pass_index: 2,
                    description: "Consumer".into(),
                },
            ],
        }
    }

    /// Build a dep graph with two reads of the same signal from
    /// different passes. Used to test `read_merge`.
    fn graph_with_two_reads() -> DependencyGraph {
        DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![SignalId(0)],
                    outputs: vec![],
                    pass_index: 0,
                    description: "Reader1".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![SignalId(0)],
                    outputs: vec![],
                    pass_index: 1,
                    description: "Reader2".into(),
                },
            ],
        }
    }

    // =========================
    // EGraph core data structure
    // =========================

    #[test]
    fn new_egraph_is_empty() {
        let eg = EGraph::new();
        assert!(eg.is_empty());
        assert_eq!(eg.len(), 0);
        assert_eq!(eg.root_count(), 0);
        assert!(eg.hashcons.is_empty());
        assert!(eg.dead.is_empty());
    }

    #[test]
    fn default_egraph_is_empty() {
        let eg = EGraph::default();
        assert!(eg.is_empty());
    }

    #[test]
    fn add_creates_new_class_for_unique_node() {
        let mut eg = EGraph::new();
        let node = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        let id = eg.add(node);
        assert_eq!(eg.len(), 1);
        assert_eq!(eg.root_count(), 1);
        assert_eq!(id, 0);
        // The hash-cons should contain the node.
        let node2 = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        assert!(eg.hashcons.contains_key(&node2));
    }

    #[test]
    fn add_deduplicates_identical_nodes_via_hashcons() {
        let mut eg = EGraph::new();
        let node = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        let id1 = eg.add(node.clone());
        let id2 = eg.add(node);
        // Hash-consing: same node → same e-class.
        assert_eq!(id1, id2);
        assert_eq!(eg.len(), 1);
    }

    #[test]
    fn add_deduplicates_signal_reads_across_passes() {
        // Two passes both read signal 0 → hash-consing merges them.
        let mut eg = EGraph::new();
        let r1 = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        let r2 = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        let id1 = eg.add(r1);
        let id2 = eg.add(r2);
        assert_eq!(id1, id2, "same signal read should hash-cons to same class");
    }

    #[test]
    fn add_creates_distinct_classes_for_different_signals() {
        let mut eg = EGraph::new();
        let r1 = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        let r2 = ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        };
        let id1 = eg.add(r1);
        let id2 = eg.add(r2);
        assert_ne!(id1, id2);
        assert_eq!(eg.len(), 2);
    }

    #[test]
    fn find_returns_self_for_root() {
        let mut eg = EGraph::new();
        let id = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        assert_eq!(eg.find(id), id);
    }

    #[test]
    fn find_follows_parent_chain() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        // Merge b into a.
        eg.merge(a, b);
        // find(b) should return a (the lower ID, which wins).
        assert_eq!(eg.find(b), a);
        assert_eq!(eg.find(a), a);
    }

    #[test]
    fn find_mut_applies_path_halving() {
        let mut eg = EGraph::new();
        // Build a chain: a → b → c (where → means "merged into").
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        let c = eg.add(ENode {
            op: EOp::Const(2),
            children: vec![],
        });
        // Merge c into b, then b into a. After this, the parent chain
        // for c is c → b → a.
        eg.merge(b, c);
        eg.merge(a, b);
        // find_mut(c) must return the root a, and path halving
        // re-points c at its grandparent (here: directly to a).
        let root = eg.find_mut(c);
        assert_eq!(root, a);
        assert_eq!(eg.classes[c as usize].parent, a);
    }

    #[test]
    fn find_mut_path_halving_shortens_long_chains() {
        // A 4-node chain d → c → b → a: path halving re-points d and
        // c toward their grandparents; every node still finds a.
        let mut eg = EGraph::new();
        let ids: Vec<EClassId> = (0..4)
            .map(|i| {
                eg.add(ENode {
                    op: EOp::Const(i),
                    children: vec![],
                })
            })
            .collect();
        let [a, b, c, d] = [ids[0], ids[1], ids[2], ids[3]];
        // Build chain d→c→b→a via merges (each merged into the earlier).
        eg.merge(c, d);
        eg.merge(b, c);
        eg.merge(a, b);
        for &id in &[b, c, d] {
            assert_eq!(eg.find_mut(id), a, "every node must find root a");
        }
        // After halving, repeated find_mut calls stay correct and the
        // tree only gets flatter.
        assert_eq!(eg.find(d), a);
        assert_eq!(eg.find(c), a);
        assert_eq!(eg.find(b), a);
    }

    #[test]
    fn merge_unions_two_classes() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        assert_eq!(eg.root_count(), 2);
        let winner = eg.merge(a, b);
        assert_eq!(winner, a); // lower ID wins
        assert_eq!(eg.root_count(), 1);
        // Both should now find the same root.
        assert_eq!(eg.find(a), eg.find(b));
    }

    #[test]
    fn merge_is_idempotent() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        eg.merge(a, b);
        let root_before = eg.find(a);
        // Merging again should be a no-op.
        eg.merge(a, b);
        let root_after = eg.find(a);
        assert_eq!(root_before, root_after);
    }

    #[test]
    fn merge_lower_id_wins_deterministic() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        // Merge in both orders — the winner should be the same (a, the lower ID).
        let winner1 = eg.merge(a, b);
        let winner2 = eg.merge(b, a);
        assert_eq!(winner1, a);
        assert_eq!(winner2, a);
    }

    #[test]
    fn merge_combines_nodes() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        eg.merge(a, b);
        // The winner's class should contain both e-nodes.
        let root = eg.find(a);
        let class = &eg.classes[root as usize];
        assert_eq!(class.nodes.len(), 2);
        assert!(class.nodes.iter().any(|n| matches!(n.op, EOp::Const(0))));
        assert!(class.nodes.iter().any(|n| matches!(n.op, EOp::Const(1))));
    }

    #[test]
    fn mark_dead_adds_to_tombstone_set() {
        let mut eg = EGraph::new();
        let id = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        assert!(!eg.is_dead(id));
        eg.mark_dead(id);
        assert!(eg.is_dead(id));
    }

    #[test]
    fn mark_dead_propagates_through_merge() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        eg.mark_dead(a);
        eg.merge(a, b);
        // After merge, the winner should be dead.
        assert!(eg.is_dead(a));
        assert!(eg.is_dead(b));
    }

    #[test]
    fn iter_visits_all_classes() {
        let mut eg = EGraph::new();
        eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::Const(2),
            children: vec![],
        });
        let count = eg.iter().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn iter_roots_skips_subsumed_classes() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::Const(1),
            children: vec![],
        });
        let c = eg.add(ENode {
            op: EOp::Const(2),
            children: vec![],
        });
        eg.merge(a, b);
        // After merge: 3 classes total, 2 roots (a and c).
        assert_eq!(eg.iter().count(), 3);
        assert_eq!(eg.iter_roots().count(), 2);
        let _ = c;
    }

    // =========================
    // Canonicalization
    // =========================

    #[test]
    fn canonicalize_updates_children_to_roots() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        // Merge b into a.
        eg.merge(a, b);
        // An e-node with child=b should canonicalize to child=a.
        let mut node = ENode {
            op: EOp::Pass(0),
            children: vec![b],
        };
        eg.canonicalize(&mut node);
        assert_eq!(node.children, vec![a]);
    }

    #[test]
    fn add_canonicalizes_children_before_hashcons() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        eg.merge(a, b);
        // Adding a Pass node with child=b should hash-cons with a Pass
        // node with child=a (after canonicalization, both reference a).
        let p1 = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![b],
        });
        let p2 = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![a],
        });
        assert_eq!(p1, p2, "canonicalized children should hash-cons together");
    }

    // =========================
    // Cost model
    // =========================

    #[test]
    fn op_cost_signal_read_is_cheapest() {
        assert_eq!(op_cost(&EOp::SignalRead(SignalId(0))), 1);
    }

    #[test]
    fn op_cost_pass_is_second() {
        assert_eq!(op_cost(&EOp::Pass(0)), 2);
    }

    #[test]
    fn op_cost_signal_write_is_third() {
        assert_eq!(op_cost(&EOp::SignalWrite(SignalId(0))), 3);
    }

    #[test]
    fn op_cost_const_is_most_expensive() {
        assert_eq!(op_cost(&EOp::Const(42)), 4);
    }

    #[test]
    fn op_cost_ordering_signal_read_lt_pass_lt_write_lt_const() {
        let r = op_cost(&EOp::SignalRead(SignalId(0)));
        let p = op_cost(&EOp::Pass(0));
        let w = op_cost(&EOp::SignalWrite(SignalId(0)));
        let c = op_cost(&EOp::Const(0));
        assert!(r < p);
        assert!(p < w);
        assert!(w < c);
    }

    #[test]
    fn compute_costs_assigns_op_cost_to_leaves() {
        let mut eg = EGraph::new();
        let id = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        eg.compute_costs();
        assert_eq!(eg.cost_of(id), 1);
    }

    #[test]
    fn compute_costs_sums_children() {
        let mut eg = EGraph::new();
        // Pass with two SignalRead children: cost = 2 + 1 + 1 = 4.
        let r1 = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let r2 = eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        let p = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![r1, r2],
        });
        eg.compute_costs();
        assert_eq!(eg.cost_of(p), 4);
    }

    #[test]
    fn compute_costs_picks_cheapest_node_in_class() {
        let mut eg = EGraph::new();
        // Create two equivalent nodes (different ops, same e-class via merge):
        // - SignalRead(0) with cost 1
        // - Const(0) with cost 4
        let r = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let c = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        eg.merge(r, c);
        eg.compute_costs();
        // The class's cost should be the minimum: 1 (SignalRead).
        let root = eg.find(r);
        assert_eq!(eg.cost_of(root), 1);
        // The best node should be the SignalRead.
        let best = eg.best_node_of(root).expect("best node should be set");
        assert!(matches!(best.op, EOp::SignalRead(SignalId(0))));
    }

    // =========================
    // build_from_dep_graph
    // =========================

    #[test]
    fn build_from_empty_dep_graph_produces_empty_egraph() {
        let dep = DependencyGraph::default();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        assert!(eg.is_empty());
    }

    #[test]
    fn build_creates_pass_class_per_dep_node() {
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        // 2 passes + 1 unique SignalRead (hash-consed) = 3 classes.
        assert_eq!(eg.root_count(), 3);
    }

    #[test]
    fn build_creates_signal_write_classes_for_outputs() {
        let dep = graph_with_write_then_read();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        // Pass 0 (no inputs, one output) + Pass 1 (one input, no output)
        // + 1 SignalRead (signal 0, read by pass 1) + 1 SignalWrite (signal 0, written by pass 0)
        // = 4 classes.
        assert_eq!(eg.root_count(), 4);
    }

    #[test]
    fn build_hashconses_repeated_signal_reads() {
        // Two passes both read signal 0 → one SignalRead class.
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let read_classes = eg.find_read_classes(SignalId(0));
        assert_eq!(
            read_classes.len(),
            1,
            "SignalRead(0) should hash-cons to one class"
        );
    }

    // =========================
    // Rewrite rule: read_merge
    // =========================

    #[test]
    fn read_merge_no_op_when_already_merged() {
        // Hash-consing already merges reads of the same signal.
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let changed = apply_read_merge(&mut eg);
        assert!(
            !changed,
            "read_merge should be a no-op when hash-consing already merged"
        );
    }

    #[test]
    fn read_merge_merges_classes_added_after_build() {
        // Construct an e-graph where two SignalRead(0) e-nodes are in
        // different e-classes. We do this by directly inserting a
        // SignalRead(0) e-node into a different class (bypassing
        // hash-consing), simulating a scenario where two classes end
        // up containing the same read after a merge that wasn't
        // followed by a rebuild.
        let mut eg = EGraph::new();
        let r1 = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        // Create a second class with a different e-node.
        let other = eg.add(ENode {
            op: EOp::Const(99),
            children: vec![],
        });
        // Manually insert a SignalRead(0) e-node into the `other` class.
        eg.classes[other as usize].nodes.push(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        // Now r1 and `other` both contain SignalRead(0).
        assert_ne!(eg.find(r1), eg.find(other));
        let changed = apply_read_merge(&mut eg);
        assert!(changed);
        assert_eq!(eg.find(r1), eg.find(other));
    }

    // =========================
    // RewriteRule trait + RULES registry
    // =========================

    #[test]
    fn rules_registry_lists_exactly_the_three_fixpoint_rules() {
        // Spec §4.3 names exactly four rewrite rules; three run in the
        // fixpoint loop (rule 4, evaluation_reorder, runs at extraction
        // and is covered by its own tests below).
        assert_eq!(RULES.len(), 3);
        assert_eq!(RULES[0].name(), "state_store_load_forward");
        assert_eq!(RULES[1].name(), "dead_store_elimination");
        assert_eq!(RULES[2].name(), "read_merge");
    }

    #[test]
    fn rewrite_rule_trait_delegates_to_free_functions() {
        // The trait impls must be exact delegations: applying the rule
        // object to a fresh e-graph yields the same `changed` result as
        // calling the free function directly.
        let dep = graph_with_write_then_read();
        let mut eg_trait = EGraph::new();
        build_from_dep_graph(&mut eg_trait, &dep);
        let mut eg_free = EGraph::new();
        build_from_dep_graph(&mut eg_free, &dep);

        let via_trait = StateStoreLoadForward.apply(&mut eg_trait);
        let via_free = apply_state_store_load_forward(&mut eg_free);
        assert_eq!(via_trait, via_free);
        // Both e-graphs must agree on every e-class root afterwards.
        for id in 0..eg_trait.classes.len() as EClassId {
            assert_eq!(eg_trait.find(id), eg_free.find(id));
        }
    }

    #[test]
    fn all_rules_report_noop_on_hello_world_graph() {
        // For the canonical Hello World dep graph (no SignalWrite
        // nodes), every fixpoint rule is a no-op.
        let dep = graph_with_two_reads();
        for rule in RULES {
            let mut eg = EGraph::new();
            build_from_dep_graph(&mut eg, &dep);
            assert!(!rule.apply(&mut eg), "rule {} must be a no-op", rule.name());
        }
    }

    #[test]
    fn fixpoint_via_registry_matches_direct_application() {
        // Driving the loop through the RULES registry must produce the
        // same optimized graph as the direct free-function loop (the
        // pre-trait implementation) for a graph where rules fire.
        let dep = graph_with_write_then_read();

        // Registry-driven (the production path).
        let via_registry = egraph_optimization(&dep);

        // Direct loop (reference).
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let mut changed = true;
        let mut iters = 0;
        while changed && iters < 1024 {
            changed = false;
            changed |= apply_state_store_load_forward(&mut eg);
            changed |= apply_dead_store_elimination(&mut eg);
            changed |= apply_read_merge(&mut eg);
            iters += 1;
        }
        let via_direct = extract(&eg, &dep);

        assert_eq!(via_registry.nodes.len(), via_direct.nodes.len());
        for (a, b) in via_registry.nodes.iter().zip(via_direct.nodes.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.pass_index, b.pass_index);
        }
    }

    // =========================
    // Rewrite rule: state_store_load_forward
    // =========================

    #[test]
    fn state_store_load_forward_no_op_without_writes() {
        // Hello World has no writes, so this rule is a no-op.
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let changed = apply_state_store_load_forward(&mut eg);
        assert!(!changed);
    }

    #[test]
    fn state_store_load_forward_merges_read_with_written_value() {
        // Pass 0 writes signal 0 (value = pass 0's class).
        // Pass 1 reads signal 0.
        // After the rule, SignalRead(0)'s class should be merged with
        // pass 0's class.
        let dep = graph_with_write_then_read();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);

        // Find pass 0's class (the value being written).
        let pass0_class = eg
            .iter_roots()
            .find(|c| c.nodes.iter().any(|n| matches!(n.op, EOp::Pass(0))))
            .map(|c| c.id)
            .expect("pass 0 class should exist");

        // Find SignalRead(0)'s class.
        let read_classes_before = eg.find_read_classes(SignalId(0));
        assert_eq!(read_classes_before.len(), 1);
        let read_class = read_classes_before[0];

        // The two should be distinct before the rule.
        assert_ne!(eg.find(pass0_class), eg.find(read_class));

        let changed = apply_state_store_load_forward(&mut eg);
        assert!(changed);

        // After the rule, they should be merged.
        assert_eq!(eg.find(pass0_class), eg.find(read_class));
    }

    // =========================
    // Rewrite rule: dead_store_elimination
    // =========================

    #[test]
    fn dead_store_elimination_no_op_without_writes() {
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let changed = apply_dead_store_elimination(&mut eg);
        assert!(!changed);
    }

    #[test]
    fn dead_store_elimination_marks_first_write_dead() {
        // Pass 0 writes signal 0.
        // Pass 1 writes signal 0 (overwrites).
        // Pass 2 reads signal 0.
        // No read of signal 0 occurs between pass 0 and pass 1's writes.
        // → Pass 0's write (the first one) is dead.
        let dep = graph_with_dead_store();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);

        // Find pass 0's class.
        let pass0_class = eg
            .iter_roots()
            .find(|c| c.nodes.iter().any(|n| matches!(n.op, EOp::Pass(0))))
            .map(|c| c.id)
            .expect("pass 0 class should exist");

        assert!(!eg.is_dead(pass0_class));
        let changed = apply_dead_store_elimination(&mut eg);
        assert!(changed);
        assert!(
            eg.is_dead(pass0_class),
            "pass 0's class should be marked dead"
        );
    }

    #[test]
    fn dead_store_elimination_preserves_write_with_intervening_read() {
        // Pass 0 writes signal 0.
        // Pass 1 reads signal 0.
        // Pass 2 writes signal 0.
        // → Pass 0's write is NOT dead (pass 1 reads it).
        let dep = DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![],
                    outputs: vec![SignalId(0)],
                    pass_index: 0,
                    description: "FirstWrite".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![SignalId(0)],
                    outputs: vec![],
                    pass_index: 1,
                    description: "Reader".into(),
                },
                DepNode {
                    id: DepNodeId(2),
                    inputs: vec![],
                    outputs: vec![SignalId(0)],
                    pass_index: 2,
                    description: "SecondWrite".into(),
                },
            ],
        };
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);

        let pass0_class = eg
            .iter_roots()
            .find(|c| c.nodes.iter().any(|n| matches!(n.op, EOp::Pass(0))))
            .map(|c| c.id)
            .expect("pass 0 class should exist");

        let changed = apply_dead_store_elimination(&mut eg);
        assert!(!changed, "no dead store should be eliminated");
        assert!(!eg.is_dead(pass0_class));
    }

    // =========================
    // evaluation_reorder
    // =========================

    #[test]
    fn evaluation_reorder_no_op_for_empty() {
        let mut nodes: Vec<DepNode> = Vec::new();
        evaluation_reorder(&mut nodes);
        assert!(nodes.is_empty());
    }

    #[test]
    fn evaluation_reorder_no_op_for_single_node() {
        let mut nodes = vec![DepNode {
            id: DepNodeId(0),
            inputs: vec![],
            outputs: vec![],
            pass_index: 0,
            description: "Only".into(),
        }];
        evaluation_reorder(&mut nodes);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].pass_index, 0);
    }

    #[test]
    fn evaluation_reorder_preserves_order_without_dependencies() {
        // No pass writes a signal that another reads → original order preserved.
        let mut nodes = vec![
            DepNode {
                id: DepNodeId(0),
                inputs: vec![SignalId(0)],
                outputs: vec![],
                pass_index: 0,
                description: "A".into(),
            },
            DepNode {
                id: DepNodeId(1),
                inputs: vec![SignalId(1)],
                outputs: vec![],
                pass_index: 1,
                description: "B".into(),
            },
        ];
        evaluation_reorder(&mut nodes);
        assert_eq!(nodes[0].pass_index, 0);
        assert_eq!(nodes[1].pass_index, 1);
    }

    #[test]
    fn evaluation_reorder_puts_producer_before_consumer() {
        // Pass 1 produces signal 0; pass 0 consumes it.
        // After reorder, pass 1 should come first.
        let mut nodes = vec![
            DepNode {
                id: DepNodeId(0),
                inputs: vec![SignalId(0)],
                outputs: vec![],
                pass_index: 0,
                description: "Consumer".into(),
            },
            DepNode {
                id: DepNodeId(1),
                inputs: vec![],
                outputs: vec![SignalId(0)],
                pass_index: 1,
                description: "Producer".into(),
            },
        ];
        evaluation_reorder(&mut nodes);
        // The producer (pass 1) should come first.
        assert_eq!(nodes[0].pass_index, 1, "producer should come first");
        assert_eq!(nodes[1].pass_index, 0);
    }

    // =========================
    // extract
    // =========================

    #[test]
    fn extract_empty_egraph_returns_empty_graph() {
        let eg = EGraph::new();
        let original = DependencyGraph::default();
        let result = extract(&eg, &original);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_preserves_pass_count_for_hello_world_like_graph() {
        // Two reads, no writes → 2 passes preserved.
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let result = extract(&eg, &dep);
        assert_eq!(result.nodes.len(), 2);
    }

    #[test]
    fn extract_preserves_inputs_for_read_only_passes() {
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let result = extract(&eg, &dep);
        for node in &result.nodes {
            assert_eq!(node.inputs, vec![SignalId(0)]);
            assert!(node.outputs.is_empty());
        }
    }

    #[test]
    fn extract_skips_dead_passes() {
        // Build a graph with a dead store, then extract and verify the
        // dead pass is pruned.
        let dep = graph_with_dead_store();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        // Apply dead_store_elimination to mark pass 0 as dead.
        apply_dead_store_elimination(&mut eg);
        let result = extract(&eg, &dep);
        // Pass 0 should be pruned; passes 1 and 2 should remain.
        let pass_indices: Vec<usize> = result.nodes.iter().map(|n| n.pass_index).collect();
        assert!(!pass_indices.contains(&0), "dead pass 0 should be pruned");
        assert!(pass_indices.contains(&1));
        assert!(pass_indices.contains(&2));
    }

    #[test]
    fn extract_reassigns_ids_to_be_dense() {
        let dep = graph_with_two_reads();
        let mut eg = EGraph::new();
        build_from_dep_graph(&mut eg, &dep);
        let result = extract(&eg, &dep);
        for (i, node) in result.nodes.iter().enumerate() {
            assert_eq!(node.id, DepNodeId(i as u32));
        }
    }

    // =========================
    // egraph_optimization (end-to-end)
    // =========================

    #[test]
    fn egraph_optimization_empty_graph_returns_empty() {
        let dep = DependencyGraph::default();
        let result = egraph_optimization(&dep);
        assert!(result.is_empty());
    }

    #[test]
    fn egraph_optimization_preserves_hello_world_structure() {
        // A Hello-World-like dep graph: 5 passes, various inputs, no outputs.
        let dep = DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![signals::CANVAS_WIDTH, signals::CANVAS_HEIGHT],
                    outputs: vec![],
                    pass_index: 0,
                    description: "Clear".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![signals::CANVAS_WIDTH, signals::CANVAS_HEIGHT],
                    outputs: vec![],
                    pass_index: 1,
                    description: "InputFieldBackground".into(),
                },
                DepNode {
                    id: DepNodeId(2),
                    inputs: vec![signals::CANVAS_WIDTH, signals::CANVAS_HEIGHT],
                    outputs: vec![],
                    pass_index: 2,
                    description: "InputFieldBorder".into(),
                },
                DepNode {
                    id: DepNodeId(3),
                    inputs: vec![
                        signals::INPUT_TEXT,
                        signals::TIME,
                        signals::FONT_SIZE,
                        signals::ROTATION_SPEED,
                        signals::CANVAS_WIDTH,
                        signals::CANVAS_HEIGHT,
                    ],
                    outputs: vec![],
                    pass_index: 3,
                    description: "TitleText".into(),
                },
                DepNode {
                    id: DepNodeId(4),
                    inputs: vec![
                        signals::INPUT_TEXT,
                        signals::CANVAS_WIDTH,
                        signals::CANVAS_HEIGHT,
                    ],
                    outputs: vec![],
                    pass_index: 4,
                    description: "InputText".into(),
                },
            ],
        };
        let result = egraph_optimization(&dep);
        // All 5 passes should be preserved (no dead writes).
        assert_eq!(result.nodes.len(), 5);
        // Each pass's inputs should be preserved (read_merge doesn't
        // eliminate reads, only deduplicates them at the e-class level).
        for (orig, opt) in dep.nodes.iter().zip(result.nodes.iter()) {
            assert_eq!(
                orig.inputs.len(),
                opt.inputs.len(),
                "pass {} inputs changed: {:?} → {:?}",
                orig.pass_index,
                orig.inputs,
                opt.inputs
            );
            for sig in &orig.inputs {
                assert!(
                    opt.inputs.contains(sig),
                    "pass {} lost input {:?}",
                    orig.pass_index,
                    sig
                );
            }
        }
    }

    #[test]
    fn egraph_optimization_eliminates_dead_store() {
        let dep = graph_with_dead_store();
        let result = egraph_optimization(&dep);
        // Pass 0 (dead store) should be pruned.
        let pass_indices: Vec<usize> = result.nodes.iter().map(|n| n.pass_index).collect();
        assert!(
            !pass_indices.contains(&0),
            "dead store should be eliminated"
        );
        assert!(pass_indices.contains(&1));
        assert!(pass_indices.contains(&2));
    }

    #[test]
    fn egraph_optimization_forwards_store_load() {
        let dep = graph_with_write_then_read();
        let result = egraph_optimization(&dep);
        // Both passes should be preserved (the write is read, so it's not dead).
        assert_eq!(result.nodes.len(), 2);
        // The consumer (pass 1) should still read signal 0.
        let consumer = result
            .nodes
            .iter()
            .find(|n| n.pass_index == 1)
            .expect("consumer should exist");
        assert!(consumer.inputs.contains(&SignalId(0)));
    }

    #[test]
    fn egraph_optimization_terminates_on_cyclic_input() {
        // Construct a dep graph with a cycle: A writes signal 0 and
        // reads signal 1; B writes signal 1 and reads signal 0.
        // (This is malformed — real dep graphs are DAGs — but the
        // optimizer must not loop forever.)
        let dep = DependencyGraph {
            nodes: vec![
                DepNode {
                    id: DepNodeId(0),
                    inputs: vec![SignalId(1)],
                    outputs: vec![SignalId(0)],
                    pass_index: 0,
                    description: "A".into(),
                },
                DepNode {
                    id: DepNodeId(1),
                    inputs: vec![SignalId(0)],
                    outputs: vec![SignalId(1)],
                    pass_index: 1,
                    description: "B".into(),
                },
            ],
        };
        // Should terminate (not hang).
        let result = egraph_optimization(&dep);
        // Both passes should be in the result (the cycle prevents
        // dead_store_elimination from pruning either).
        assert_eq!(result.nodes.len(), 2);
    }

    #[test]
    fn egraph_optimization_idempotent_on_hello_world() {
        // Running the optimizer twice should produce the same result.
        let dep = DependencyGraph {
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
                    inputs: vec![signals::INPUT_TEXT, signals::CANVAS_WIDTH],
                    outputs: vec![],
                    pass_index: 1,
                    description: "TitleText".into(),
                },
            ],
        };
        let once = egraph_optimization(&dep);
        let twice = egraph_optimization(&once);
        assert_eq!(once.nodes.len(), twice.nodes.len());
        for (a, b) in once.nodes.iter().zip(twice.nodes.iter()) {
            assert_eq!(a.pass_index, b.pass_index);
            assert_eq!(a.inputs, b.inputs);
            assert_eq!(a.outputs, b.outputs);
        }
    }

    // =========================
    // EOp / EOpKind
    // =========================

    #[test]
    fn eop_kind_matches_correct_variant() {
        assert!(EOpKind::SignalRead.matches(&EOp::SignalRead(SignalId(0))));
        assert!(EOpKind::SignalWrite.matches(&EOp::SignalWrite(SignalId(0))));
        assert!(EOpKind::Pass.matches(&EOp::Pass(0)));
        assert!(EOpKind::Const.matches(&EOp::Const(0)));
    }

    #[test]
    fn eop_kind_does_not_match_wrong_variant() {
        assert!(!EOpKind::SignalRead.matches(&EOp::Pass(0)));
        assert!(!EOpKind::SignalWrite.matches(&EOp::SignalRead(SignalId(0))));
        assert!(!EOpKind::Pass.matches(&EOp::Const(0)));
        assert!(!EOpKind::Const.matches(&EOp::Pass(0)));
    }

    // =========================
    // find_nodes_with_op / find_read_classes / find_write_classes
    // =========================

    #[test]
    fn find_nodes_with_op_returns_all_matching() {
        let mut eg = EGraph::new();
        eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![],
        });
        let reads = eg.find_nodes_with_op(EOpKind::SignalRead);
        assert_eq!(reads.len(), 2);
        let passes = eg.find_nodes_with_op(EOpKind::Pass);
        assert_eq!(passes.len(), 1);
        let writes = eg.find_nodes_with_op(EOpKind::SignalWrite);
        assert_eq!(writes.len(), 0);
    }

    #[test]
    fn find_read_classes_returns_one_per_signal() {
        let mut eg = EGraph::new();
        eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        assert_eq!(eg.find_read_classes(SignalId(0)).len(), 1);
        assert_eq!(eg.find_read_classes(SignalId(1)).len(), 1);
        assert_eq!(eg.find_read_classes(SignalId(2)).len(), 0);
    }

    #[test]
    fn find_write_classes_returns_one_per_signal() {
        let mut eg = EGraph::new();
        let p = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![],
        });
        eg.add(ENode {
            op: EOp::SignalWrite(SignalId(0)),
            children: vec![p],
        });
        eg.add(ENode {
            op: EOp::SignalWrite(SignalId(1)),
            children: vec![p],
        });
        assert_eq!(eg.find_write_classes(SignalId(0)).len(), 1);
        assert_eq!(eg.find_write_classes(SignalId(1)).len(), 1);
        assert_eq!(eg.find_write_classes(SignalId(2)).len(), 0);
    }

    #[test]
    fn pass_index_of_returns_pass_index() {
        let mut eg = EGraph::new();
        let p = eg.add(ENode {
            op: EOp::Pass(42),
            children: vec![],
        });
        assert_eq!(eg.pass_index_of(p), Some(42));
    }

    #[test]
    fn pass_index_of_returns_none_for_non_pass_class() {
        let mut eg = EGraph::new();
        let r = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        assert_eq!(eg.pass_index_of(r), None);
    }

    // =========================
    // node_cost / class_cost
    // =========================

    #[test]
    fn node_cost_for_leaf_signal_read() {
        let mut eg = EGraph::new();
        let _id = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        eg.compute_costs();
        let node = ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        };
        assert_eq!(node_cost(&eg, &node), 1);
    }

    #[test]
    fn class_cost_uses_minimum_node_cost() {
        let mut eg = EGraph::new();
        let r = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let c = eg.add(ENode {
            op: EOp::Const(0),
            children: vec![],
        });
        eg.merge(r, c);
        eg.compute_costs();
        // Cost should be min(1, 4) = 1.
        assert_eq!(class_cost(&eg, r), 1);
    }

    // =========================
    // rebuild
    // =========================

    #[test]
    fn rebuild_restores_hashcons_invariant() {
        let mut eg = EGraph::new();
        let a = eg.add(ENode {
            op: EOp::SignalRead(SignalId(0)),
            children: vec![],
        });
        let b = eg.add(ENode {
            op: EOp::SignalRead(SignalId(1)),
            children: vec![],
        });
        // Merge b into a.
        eg.merge(a, b);
        // After rebuild (called inside merge), adding a Pass node with
        // child=b should hash-cons with a Pass node with child=a.
        let p1 = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![b],
        });
        let p2 = eg.add(ENode {
            op: EOp::Pass(0),
            children: vec![a],
        });
        assert_eq!(p1, p2);
    }
}

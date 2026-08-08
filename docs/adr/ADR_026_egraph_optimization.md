# ADR 026: E-Graph Optimization for Signal Read/Write Patterns

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-026). This standalone file is provided for direct linking.

## Context

Once incremental computation (ADR-025) is in place, the dependency graph of reactive signals may contain inefficiencies:

- **Redundant reads:** The same signal is read by multiple consumers, each triggering a separate dependency check.
- **Redundant writes:** A signal is written, then immediately overwritten, wasting the first write's invalidation work.
- **Suboptimal evaluation order:** Consumers may be evaluated before their producers, causing multiple re-evaluations.

Without optimization, these inefficiencies cause unnecessary re-evaluations and cache misses, partially negating the benefits of incremental computation.

The VUMA feasibility study (`external-research/feasibility-assessment.md` §5, "E-Graph Optimization") identified the `state_store_load_forward` rewrite from VUMA's e-graph as directly applicable to AlkALive's signal dependency graph. VUMA's e-graph implementation is 3,235 LOC in Rust; a minimal AlkALive-specific version is estimated at ~2,000 LOC.

## Decision

Add an `egraph_optimization` compiler pass (after `incremental_analysis` from ADR-025) that:

1. **Builds an e-graph** from the `DependencyGraph`. Each computation becomes an e-node; equivalent computations (same inputs, same operation) are merged into e-classes.

2. **Applies rewrite rules:**
   - `state_store_load_forward`: If `S := v; x := S`, rewrite to `S := v; x := v` (forward the stored value, eliminating the signal read)
   - `dead_store_elimination`: If `S := v1; S := v2` with no read of `S` between writes, eliminate the first write
   - `read_merge`: If two consumers read the same signal, merge the reads into a single cached read
   - `evaluation_reorder`: Topologically sort consumers after producers to minimize re-evaluations

3. **Extracts the optimized graph** via cost-based extraction (selects the cheapest equivalent form).

**Implementation choice:** Use a custom lightweight e-graph implementation (~2,000 LOC) rather than the `egg` crate. Rationale: AlkALive's 5-crate external dependency policy (per ADR-018) is strict; adding `egg` would require an ADR amendment. A minimal e-graph for 4 rewrite rules is tractable in ~2,000 LOC.

## Status

Proposed.

## Consequences

- **Positive.** Eliminates 20–50% of redundant signal operations in typical reactive UI code (based on VUMA's `state_store_load_forward` benchmarks). Zero runtime cost — all optimization happens at compile time. The optimized dependency graph is more amenable to seminaïve evaluation (enabled by ADR-027 monotonicity types).
- **Negative.** ~2,000 LOC of e-graph infrastructure in the compiler. The e-graph data structure is non-trivial (union-find, hash-consing, e-class merging). Adds compilation time proportional to the dependency graph size.
- **Cross-references.** Depends on ADR-025 (incremental computation) — operates on the `DependencyGraph`. Depends on ADR-024 (algorithm/schedule separation) — transitive via ADR-025. ADR-001 (render-graph IR) — the e-graph operates on the render-graph's dependency structure. ADR-018 (5-crate dependency policy) — the decision to use a custom e-graph rather than `egg` is to comply with this ADR.

## Confidence

**High.** E-graph optimization is a well-established technique (COW, Cranelift, VUMA). The `state_store_load_forward` rewrite is proven in VUMA. The 4 rewrite rules are clearly defined and their semantics are well-understood. The main risk is implementation complexity (e-graphs are non-trivial), but the ~2,000 LOC estimate is grounded in VUMA's actual implementation size. The decision to use a custom implementation rather than `egg` is a deliberate ADR-018 compliance choice, not a technical risk.

## Estimated LOC

~2,000 lines:
- E-graph data structure (e-node, e-class, union-find, hash-consing): ~800 LOC
- Rewrite rules (4 rules + pattern matching): ~400 LOC
- Cost-based extraction: ~300 LOC
- `egraph_optimization` compiler pass integration: ~200 LOC
- Tests + integration: ~300 LOC

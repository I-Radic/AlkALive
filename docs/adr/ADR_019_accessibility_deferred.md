# ADR 019: Defer Accessibility Bridge — No DOM Mirror

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-019-accessibility-deferred). This standalone file is provided for direct linking.

## Context
`Decision_Alternatives_accessibility-bridge.md` flagged P6.1 (canvas severs every native DOM a11y affordance) as co-decisive with P3.5 and recommended **Approach C** (hybrid: virtual tree + read-only DOM projection surface). The project owner has issued a non-negotiable directive that overrides that recommendation: the language runtime will ship without any DOM-based accessibility bridge, and accessibility is deferred to a later phase. P6.1's decisive-problem status is retained but its resolution is no longer a release blocker.

## Decision
Adopt **Approach A**: derive any future a11y-tree view from the render-object graph ([ADR 007](ADR.md#adr-007-single-owned-render-object-tree-component--subtree)) bridged directly to platform a11y APIs, with **no DOM mirror or DOM projection surface** in the runtime. Accessibility is explicitly deferred; no a11y bridge ships in this phase.

## Status
Proposed.

## Consequences
- **Positive:** unblocks the project; removes DOM coupling from the runtime hot path (consistent with [ADR 013](ADR.md#adr-013-no-wasmdom-boundary-in-the-hot-path)); preserves [ADR 007](ADR.md#adr-007-single-owned-render-object-tree-component--subtree)'s single-source-of-truth object model with no sync boundary.
- **Negative:** AT users have no a11y path until a later phase; P6.1's decisive-problem resolution is deferred, leaving the runtime non-conforming to web a11y contracts in the interim.
- **Cross-references:** [ADR 007](ADR.md#adr-007-single-owned-render-object-tree-component--subtree) (render-object tree remains the sole structural source for any future a11y derivation); [ADR 011](ADR.md#adr-011-unified-virtual-focusaccessibility-annotation-layer) (focus model — its "DOM projection surface" clause is removed by this decision; only the focus-writer contract remains active); [ADR 010](ADR.md#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input) (input-dispatch-as-sole-focus-writer) is unaffected; [ADR 022](ADR.md#adr-022-forked-harfrust-in-wasm-text-stack) (text a11y exposure now depends on this deferral, not on DOM contracts).

## Confidence
**High.** The project owner's choice is non-negotiable and explicitly overrides the Decision Alternative file's prior Approach C recommendation.

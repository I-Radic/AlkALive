# ADR 020: Metadata-Only DOM Layer for SEO — No UI DOM Interop

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-020-metadata-only-dom-layer-for-seo). This standalone file is provided for direct linking.

## Context
P9.5 is strategic, not technical: ecosystem inertia dominates adoption, and the correct boundary between the WASM/WebGPU stack and the host DOM was unresolved (see [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md)). That file recommended Approach A — host-DOM interop bridges for text/a11y/navigation, time-boxed 18 months. The project owner has now made a definitive, non-negotiable choice that **overrides** that recommendation.

## Decision
Adopt Approach C, narrowed: a **thin DOM layer solely to set `<title>`, `<meta>` tags, and emit a static HTML snapshot for search-engine crawlers**. All UI rendering happens on GPU via WASM, with no DOM-tree interaction for layout, text, a11y, navigation, or input. No host-DOM interop bridges are provided.

## Status
Proposed.

## Consequences
- **Positive.** No bridge/marshalling tax; the hot path stays entirely inside WASM, aligning cleanly with [ADR 013](ADR.md#adr-013-no-wasmdom-boundary-in-the-hot-path). A single binary/startup story is preserved without dual-stack payload pressure ([ADR 017](ADR.md#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation)). Seam bugs at the DOM frontier are eliminated.
- **Negative.** No incremental adoption via the DOM: teams cannot embed the new stack inside existing pages. Loses the a11y, text-rendering, and navigation interop bridges Approach A relied on — these must now be solved entirely off-DOM (a11y deferred per [ADR 019](ADR.md#adr-019-accessibility-deferred); text via [ADR 022](ADR.md#adr-022-forked-harfrust-in-wasm-text-stack)).
- **Cross-references.** [ADR 012](ADR.md#adr-012-navigationurl-contract-and-explicit-seo-scope) (the metadata-only DOM is the explicit SEO export surface); [ADR 013](ADR.md#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path integrity preserved structurally); [ADR 017](ADR.md#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (single-WASM startup budget uncompromised); [ADR 019](ADR.md#adr-019-accessibility-deferred) (deferred accessibility — no DOM a11y bridge); [ADR 022](ADR.md#adr-022-forked-harfrust-in-wasm-text-stack) (in-WASM text stack — no DOM text surface).

## Confidence
**High.** The project owner's non-negotiable choice, superseding the prior recommendation regardless of the strategic-adoption risks raised.

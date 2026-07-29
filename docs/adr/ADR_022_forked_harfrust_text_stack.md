# ADR 022: Forked HarfRust as the In-WASM Text Shaping/Rasterization Stack

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-022-forked-harfrust-in-wasm-text-stack). This standalone file is provided for direct linking.

## Context
The off-DOM text stack is P3.5's decisive hard problem: contractual shaping, BiDi, selection, IME, and a11y are guaranteed only on DOM text nodes, while `canvas.fillText` provides none. [`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md) recommended **Approach B (hidden DOM surface)** as the lowest-regret interim. The project owner has now made a non-negotiable choice that overrides that recommendation: commit to **Approach A (in-WASM text stack)**, naming **HarfRust** specifically and mandating a fork so updates apply independently of upstream release cadence.

## Decision
Adopt a **forked HarfRust** as the shaping and rasterization stack, running entirely inside WASM. The fork lives alongside the project (vendored in-repo) so shaping/rasterization fixes, platform patches, and BiDi/IME extensions can be applied independently of upstream.

## Status
Proposed.

## Consequences
- **Positive:** no DOM text dependency; shaping/rasterization stays in the WASM hot path per [ADR 013](ADR.md#adr-013-no-wasmdom-boundary-in-the-hot-path) and serves [ADR 007](ADR.md#adr-007-single-owned-render-object-tree-component--subtree)'s pure-object model (text as GPU-backed glyphs); the fork grants full control over timing and patches.
- **Negative:** the project must maintain a fork; BiDi segmentation, selection, and IME composition must still be built atop HarfRust; the WASM payload grows by the shaper (sharpening [ADR 017](ADR.md#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation)'s streaming-compile concern); a11y text exposure no longer inherits DOM contracts and now depends on [ADR 019](ADR.md#adr-019-accessibility-deferred)'s deferred accessibility approach.
- **Cross-references:** [ADR 004](ADR.md#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout's measurement contract consumes HarfRust output); [ADR 007](ADR.md#adr-007-single-owned-render-object-tree-component--subtree) (object model); [ADR 013](ADR.md#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path); [ADR 017](ADR.md#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (binary size); [ADR 019](ADR.md#adr-019-accessibility-deferred) (deferred accessibility).

## Confidence
**High.** This is the project owner's definitive, non-negotiable choice, which settles the residual uncertainty that previously kept text rendering a Decision Alternative rather than a committed ADR.

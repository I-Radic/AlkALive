# Decision Alternatives: Adoption and Interop Strategy

> **⚠ RESOLVED — superseded by [ADR 020](ADR.md#adr-020-metadata-only-dom-layer-for-seo).** The project owner chose **Approach C (DOM only for metadata/SEO, no UI DOM interop)**, which overrides this file's prior "Recommended Approach A (host-DOM interop bridges)". This file is retained for historical context only; the live decision is ADR 020.

## Decision Point

How should the new stack coexist with the incumbent web stack during adoption — incremental interop, full rewrite, or greenfield isolation?

## Why Uncertain (strategic risk)

P9.5 is strategic, not technical. Ecosystem inertia dominates adoption [46]; Elm and Reason-ML demonstrated that technical superiority alone does not overcome incumbent gravity. Multiple viable interop bridge architectures exist, and the correct boundary between the new stack and the DOM is unclear — too tight risks coupling debt, too loose forfeits incremental value.

## Approach A: Host-DOM interop bridges for text/a11y/navigation enabling incremental coexistence (new stack embedded in pages)

**Pros:** Lowest switching cost; teams adopt incrementally; preserves existing routing/analytics/SSR; text and a11y primitives (per `Decision_Alternatives_accessibility-bridge`, `Decision_Alternatives_text-rendering`) ride the host's a11y tree; rollback is cheap.
**Cons:** Bridge boundary is the hardest design call — ADR_013 hot-path perf assumes native execution, but interop marshalling can negate gains; ADR_017 startup/binary size budgets are pressured by dual-stack payloads; partial adoption leaves "seam bugs" at the DOM frontier; long-term dependency on host quirks.

## Approach B: Big-bang full rewrite to the new stack (no interop)

**Pros:** Cleanest architecture; no bridge tax on ADR_013 hot paths; single binary/startup story (ADR_017); full control of a11y and text rendering; no seam bugs.
**Cons:** Highest strategic risk — Elm/Reason precedent [46] shows rewrites stall without ecosystem traction; multi-quarter freeze on feature velocity; no rollback path; requires winning the framework war, not just the perf war.

## Approach C: Greenfield island — new stack runs as standalone apps, no DOM interop, separate ecosystem

**Pros:** Zero interop complexity; ADR_013 hot paths and ADR_017 startup budgets met without compromise; clean a11y/text rendering in isolation; team can ship and prove value independently.
**Cons:** Bypasses, not solves, the adoption problem; islands risk becoming orphaned side-projects (the Reason-ML trajectory [46]); no path to displacing the incumbent; ecosystem fragmentation.

## Recommended Approach

**Approach A, time-boxed to 18 months, with an explicit exit ramp to B.** Interop bridges are the only path that confronts ecosystem inertia directly while preserving optionality. The a11y and text-rendering bridges (cross-ref `Decision_Alternatives_accessibility-bridge`, `Decision_Alternatives_text-rendering`) are the narrow waist where adoption either compounds or stalls. ADR_013 hot-path and ADR_017 startup budgets define the perf envelope the bridge must fit; if marshalling overhead breaches those budgets for two consecutive milestones, escalate to B. C is explicitly rejected as avoidance, not strategy.

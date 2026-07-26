# Decision Alternatives: Accessibility Bridge Approach

## Decision Point

How a GPU-drawn UI surface exposes its semantic structure, roles, labels, and focus state to assistive technology (AT), given that canvas/WebGPU rendering bypasses the DOM accessibility tree entirely.

## Why Uncertain (decisive flag)

P6.1 is co-decisive with P3.5. Canvas severs every native DOM a11y affordance [39,40,41,42]. Three viable bridge architectures trade off fidelity, latency, and portability; WASM-to-platform-a11y-API access remains immature on web targets.

## Approach A: Derive a virtual a11y-tree view from the render-object graph, bridged directly to platform accessibility APIs (no DOM mirror)

**Pros:** Tightest coupling to ADR_007 object model; single source of truth; low sync overhead; natural fit for native targets via UIAutomation/AT-SPI/NSAccessibility.
**Cons:** Web target depends on unstable WASM a11y bindings or a custom browser extension; no `tabindex`/`aria` fallback; AT discovery is best-effort; high implementation risk.

## Approach B: Maintain an invisible ARIA DOM mirror kept in sync with the scene

**Pros:** Reuses the browser a11y pipeline; broad AT support today; ADR_011 focus model maps to real DOM focus; degrades gracefully.
**Cons:** Diverges from the GPU render path — drift risk; per-frame DOM mutation cost; mirrors only what ARIA can express; layout mismatch with painted pixels can mislead AT users.

## Approach C: Hybrid — virtual tree for structure + minimal DOM surface for AT contracts that require it

**Pros:** Virtual tree preserves ADR_007 integrity and structured semantics; a thin **read-only DOM projection surface** hosts ARIA contracts AT mandates (focus state remains solely input-dispatch-written in the virtual tree per ADR_011 — the DOM surface is a projection *target*, not a focus authority); portable across native and web; aligns with Decision_Alternatives_text-rendering (caret/selection) and Decision_Alternatives_adoption-interop (gradual host integration).
**Cons:** Two representations to reconcile; the sync boundary is the failure mode; more surface to test. (Mitigation: the DOM surface is read-only and derived, so reconciliation is unidirectional — virtual tree → DOM projection.)

## Recommended Approach

**Approach C.** ADR_007's object model already carries semantic nodes, so deriving the virtual tree is cheap. ADR_011's focus model needs an AT-resolvable target — a minimal DOM surface satisfies this without a full mirror. Hybrid hedges P6.1's immaturity: the DOM surface is a stable contract, the virtual tree the strategic core. It also de-risks Decision_Alternatives_text-rendering (selection spans both) and Decision_Alternatives_adoption-interop (incremental host adoption). A and B each bet on one layer maturing; C survives either outcome.

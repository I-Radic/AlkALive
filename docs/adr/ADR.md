# Architectural Decision Record (ADR)

**Project:** AlkALive — a custom, module- and object-oriented language compiling to WebAssembly with direct WebGPU rendering.

**Source:** Derived from [`ROUGH_DRAFT.md`](../ROUGH_DRAFT.md), grounded in [`PROBLEM_CATALOG.md`](../PROBLEM_CATALOG.md). Produced via a four-wave sub-agent orchestration (decision-point extraction → drafting → cross-ADR consistency → evidence traceability).

**Status convention:** All decisions are **Proposed** (awaiting ratification). Low-confidence decision points are recorded in separate [Decision Alternatives](#decision-alternatives-low-confidence) files.

**Citation convention:** `[n]` refers to references in `PROBLEM_CATALOG.md` §13; `P x.y` refers to problem entries; `G n` refers to catalog-acknowledged literature gaps. Claims resting on circumstantial evidence are flagged inline.

---

## Index

| ID | Decision | Confidence |
|----|----------|------------|
| [ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit) | Render-Graph IR as the Atomic Rendering Unit | High |
| [ADR 002](#adr-002-per-module-dirty-rect-invalidation-with-layout-locality) | Per-Module Dirty-Rect Invalidation with Layout-Locality | High |
| [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) | Single-GPUDevice Render Thread + SAB/COOP-COEP Compositor | Medium |
| [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) | Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract | High |
| [ADR 005](#adr-005-object-owned-per-instance-styling) | Object-Owned Per-Instance Styling | High |
| [ADR 006](#adr-006-wgsl-shaders-as-first-class-styling-primitives) | WGSL Shaders as First-Class Styling Primitives | High |
| [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) | Single Owned Render-Object Tree (Component = Subtree) | High |
| [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) | Statically-Typed Module+OO Language Compiling to WASM | High |
| [ADR 009](#adr-009-two-level-type-verification) | Two-Level Type Verification | Medium |
| [ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input) | CPU Bounding-Volume Hit-Testing + First-Class Device-Event Input | High |
| [ADR 011](#adr-011-unified-virtual-focusaccessibility-annotation-layer) | Unified Virtual Focus/Accessibility Annotation Layer | High / Medium |
| [ADR 012](#adr-012-navigationurl-contract-and-explicit-seo-scope) | Navigation/URL Contract and Explicit SEO Scope | Medium |
| [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) | No WASM↔DOM Boundary in the Hot Path | High |
| [ADR 014](#adr-014-design-tool-as-runtime--typed-component-testing) | Design-Tool-as-Runtime + Typed Component Testing | Medium |
| [ADR 015](#adr-015-hmr-via-serialisable-scene-graph-state-rehydration) | HMR via Serialisable Scene-Graph State Rehydration | Medium |
| [ADR 016](#adr-016-unified-author-owned-trace-with-split-determinism) | Unified Author-Owned Trace with Split Determinism | Medium |
| [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) | Compiled WASM Binary + WebGPU Pipeline Precompilation | Medium |
| [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) | Capability-Scoped Imports + Component-Model Tree-Shaking | Medium |

**Decision Alternatives (Low confidence — separate files):**
- [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) — Text rendering strategy (P3.5, decisive)
- [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) — Concurrency/scheduling model (P4.3, multiple viable)
- [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) — Accessibility bridge approach (P6.1, decisive)
- [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) — Adoption/interop strategy (P9.5, strategic)

---

## ADR 001: Render-Graph IR as the Atomic Rendering Unit

### Context
The browser box-model pipeline (P1.1, P1.5) makes the box atomic and exposes no GPU draw call to authors. Pipeline stages therefore cannot be reordered or batched [32,33], locking paint order into a fixed sequence that defeats GPU-first optimization. We require the GPU draw call as the atomic unit and an author-owned render graph expressing passes, attachments, draw calls, and an explicit occlusion-cull pass.

### Decision
Adopt **render-graph IR** — passes, attachments, draw calls, plus a dedicated occlusion-cull pass — as the atomic rendering primitive, replacing the retained DOM box-model tree. Authors declare a directed graph of passes; the runtime compiles, reorders, and batches it into optimal GPU command streams. Options rejected: (b) retained box-model tree (fails to expose draw calls) and (c) immediate-mode direct-draw (lacks barrier/attachment lifecycle and forfeits compile-time reordering).

### Status
Proposed.

### Consequences
**Positive:** Draw calls become first-class, enabling fine-grained batching, lazy barrier insertion, and author-directed occlusion culling. Stage reordering is unconstrained by box paint order, allowing depth-aware and tile-based optimization. Declaration is decoupled from execution; **WebGPU is the initial backend** (with Vulkan/Metal as future native-backend options).

**Negative:** Authors lose declarative box-model ergonomics, raising the burden for UI-centric content — [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout) reintroduces high-level sugar atop the IR. Graph compilation adds per-frame overhead and a correctness burden around barrier and attachment-lifetime management. Mapping scene entities to graph nodes requires care — [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model).

**Cross-references:** Graph compilation (reordering/batching/occlusion-cull) runs on [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor)'s single-owner render thread over SharedArrayBuffer-backed scene data; the occlusion-cull pass executes on that thread against a compositor-wide depth/visibility buffer. The compositor (ADR 003) consumes the compiled graph output and both ADRs must agree on a shared attachment-format and pass-boundary contract (to be specified in a future rendering-ABI ADR). Layout (ADR 004) and the object model (ADR 007) are downstream consumers that emit render-graph IR rather than box trees.

### Confidence
**High.** The box-model atomicity limitation is well-documented [32,33], and render-graph IR directly resolves every stated constraint by making the draw call the atomic unit.

---

## ADR 002: Per-Module Dirty-Rect Invalidation with Layout-Locality

### Context
A single DOM mutation triggers arbitrary-subtree global reflow because layout is a global constraint solver over the box tree (P1.3) [33,20,21]. We need locally-scoped invalidation without reintroducing global solving.

### Decision
Adopt option (a): each module owns its scene graph and invalidates only dirty rectangles / per-object subsets, gated by a **layout-locality guarantee** — no cross-module flex/percentage dependencies that would re-introduce global reflow. Rejected: (b) centralized global reflow solver (reproduces P1.3); (c) whole-scene diff-and-rebuild each frame (forfeits the locality win).

### Status
Proposed.

### Consequences
- **Positive:** per-frame cost is bounded by the dirty subset, not tree size; directly addresses the reflow/repaint bug class [20,21].
- **Negative:** locality is a discipline the solver must enforce — invalid cross-module constraints must be rejected at solve time or fall back to a documented global pass.
- **Cross-references:** [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout engine) must enforce the locality guarantee; [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model) defines module ownership boundaries that locality respects.

### Confidence
**High.** The global-reflow problem is directly evidenced [33,20,21], and dirty-rect + locality is the standard remedy in retained-mode GPU renderers.

---

## ADR 003: Single-GPUDevice Render Thread + SAB/COOP-COEP Compositor

### Context
Multiple scene graphs (UI, particles, world, overlays) must compose into one frame without per-graph contention (P1.5). But WebGPU's `GPUDevice` and derived objects are agent-bound: they cannot be shared across workers (a WebGPU-spec constraint, not a catalog P-entry; the rough draft §7 Solution states this directly). Concurrent submission from several workers would serialize at the browser boundary anyway, with no safe coordination mechanism.

### Decision
Adopt **option (a)**: a single dedicated render thread owns the lone `GPUDevice` and serializes every render-graph submission from all scene graphs. Scene data (instance tables, transforms, draw lists) lives in a `SharedArrayBuffer` under COOP/COEP (`Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`). Graphs emit immutable render-graph IR ([ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit)); the render thread merges, compiles, reorders, batches, then submits. The occlusion-cull pass executes on the render thread against a compositor-wide depth/visibility buffer.

### Status
Proposed.

### Consequences
- **Positive:** one authoritative submission path; no GPUDevice-sharing hazards; graphs compose without lock-free complexity.
- **Negative (COOP/COEP risk):** cross-origin isolation headers are required; this conflicts with embedding third-party iframes. Mitigations: `credentialless` COEP or iframe proxying. If unworkable, fall back to option (b) per-graph separate devices (loses shared compositor).
- **Cross-references:** [ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit) (render-graph IR is the input); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path interop relies on this single-thread submission).

### Confidence
**Medium.** The single-owner model is sound, but the COOP/COEP deployment constraint is a real-world risk that may force option (b) on iframe-heavy hosts.

---

## ADR 004: Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract

### Context
Flexbox and Grid are fixed rectangular-only solvers with unstable cross-engine semantics (P2.3, P2.4) [33,30]. They couple style-driven box-tree recalculation to layout, forcing full-tree reflows on minor style changes and producing divergent results across browsers. We need a layout substrate that (1) operates over first-class render objects rather than a CSS box tree, (2) admits non-rectangular constraints, and (3) emits results consumable directly by GPU transforms without an intermediate layout-tree serialization.

### Decision
Adopt a **pluggable constraint solver** (Cassowary, impulse, or graph-based) operating over first-class objects. The layout-tree is solver-internal and never re-derived from styles. A **mandatory text-flow measurement contract** is always present: the solver consumes a synchronous *measured-run* interface (glyph-run metrics) regardless of backend. The preferred backing implementation is an in-WASM Knuth-Plass + HarfBuzz sub-solver; an interim hidden-DOM measurement shim (per [`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md), Approach B) may satisfy the contract pending in-WASM shaper maturity. Solver outputs (transforms, glyph runs) feed GPU transforms directly, with no style-driven box-tree recalculation.

Alternatives considered: (b) a single fixed solver — rejected for forfeiting non-rectangular and domain-specific layouts; (c) author-everything including text — rejected for re-introducing the exact fragmentation instability (P2.4) this design avoids.

### Status
Proposed.

### Consequences
- **Locality** ([ADR 002](#adr-002-per-module-dirty-rect-invalidation-with-layout-locality)): per-module dirty-rect invalidation holds because the solver recomputes only constrained object subsets, never the full box tree.
- **Styling** ([ADR 005](#adr-005-object-owned-per-instance-styling)): per-instance object-owned styles remain authoritative; the solver consumes style values as constraint inputs and never mutates them.
- **Text rendering** ([`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md)): the layout engine consumes a synchronous measured-run interface; the backing implementation is pluggable (in-WASM Knuth-Plass+HarfBuzz preferred; hidden-DOM measurement shim as interim per Approach B). This keeps glyph metrics consistent across outer-solver swaps without mandating a single text backend.
- Coupling surface shrinks to the solver's transform-output contract; swapping solvers is internal and non-breaking.

### Confidence
**High.** The fixed-solver limitation is directly evidenced [33,30], and a pluggable solver over first-class objects is the standard remedy in retained-mode GPU UIs.

---

## ADR 005: Object-Owned Per-Instance Styling

### Context
CSS cascade/specificity is global and non-local, forcing after-the-fact scoping workarounds (BEM, Shadow DOM) (P2.1); CSSOM parse/match is a pipeline cost (P2.2) [33]. *(catalog gap G1: no widely-cited peer-reviewed metric study of cascade-maintainability cost; support is circumstantial via cross-browser divergence [30] and the proliferation of scoping methodologies.)* We need scoped, object-owned styling with no runtime CSSOM.

### Decision
Adopt option (a): per-instance object-owned property state bound at construction, addressable only via the owning object; no cascade, no CSSOM, no selector matching; compiled to binary. Rejected: (b) cascade + improved scoping (retains the global model); (c) CSS-in-JS runtime resolution (retains runtime matching cost).

### Status
Proposed.

### Consequences
- **Positive:** O(1) local field access; no specificity wars; build-time binary compilation; no runtime parse/match.
- **Negative:** no inheritance — properties must be explicitly set or defaulted at construction; loses CSS's declarative cascade ergonomics for document-like content.
- **Cross-references:** [ADR 006](#adr-006-wgsl-shaders-as-first-class-styling-primitives) (WGSL effects) consume owned style state as shader uniforms; [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model) exposes owned-style fields with construction-time binding.

### Confidence
**High.** The cascade-maintainability problem is well-established (P2.1, G1), and per-instance owned state directly eliminates it.

---

## ADR 006: WGSL Shaders as First-Class Styling Primitives

### Context
CSS offers a closed catalogue of engine-defined effects; authors cannot supply shaders, particles, or per-vertex transforms, and styling sits apart from the GPU pipeline (P2.5) [35].

### Decision
Adopt option (a): WGSL shader programs + compute passes bound to object instances as first-class styling primitives, composable in the style layer, replacing CSS's closed filter list with an open, author-extensible effect model. Rejected: (b) expanded built-in filter catalogue (still closed); (c) fixed pipeline + texture overlays (no shader extensibility).

### Status
Proposed.

### Consequences
- **Positive:** open extensibility; effects unify with the GPU pipeline; particle/per-vertex/compute-driven styling become first-class.
- **Negative:** shader authoring skill floor; shader compile budget and sandboxing required; fallback/degradation for low-end GPUs.
- **Cross-references:** [ADR 005](#adr-005-object-owned-per-instance-styling) (styling) provides the owned-state uniforms; [ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit) (render-graph) schedules the paint passes.

### Confidence
**High.** The closed-effect-catalogue limitation is directly evidenced [35], and WGSL binding is the natural GPU-native remedy.

---

## ADR 007: Single Owned Render-Object Tree (Component = Subtree)

### Context
The DOM is document-shaped; frameworks reconcile a separate component tree into the DOM box tree — a permanent impedance mismatch (P3.1, P3.2, P3.4) [32,30,31,27,28,29,25,26]. There is no language-level UI modularity (P4.5, P4.6); components are framework conventions, not first-class module objects.

### Decision
Adopt option (a): a single owned render-object tree where module objects ARE the render objects (Flutter-style); the UI component IS a render-object subtree owning styling/layout/drawing; no reconciler, no hydration, no SSR/CSR divergence; abandons DOM/HTML/CSS as the rendering substrate. Rejected: (b) framework tree reconciled into DOM (retains impedance mismatch); (c) hybrid DOM substrate (retains document constraints).

### Status
Proposed.

### Consequences
- **Positive:** eliminates the reconciliation layer (P3.2) and the imperative-mutation surface (P3.4); module-controlled construction/destruction; GPU-resident instancing decouples paint cost from tree size (P3.3).
- **Negative:** loses DOM ergonomics for document-like content; requires first-class solutions for text (P3.5) and accessibility (P6.1) that the DOM previously provided.
- **Cross-references:** [ADR 002](#adr-002-per-module-dirty-rect-invalidation-with-layout-locality) (invalidation) operates on this tree; [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout) is a stage over it; [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language) defines the module/component unit; [ADR 011](#adr-011-unified-virtual-focusaccessibility-annotation-layer) (focus) derives from it; the text and accessibility solutions ([`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md), [`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md)) are decisive dependencies.

### Confidence
**High.** The impedance-mismatch evidence is direct [30,31,27,28,29], and the owned-tree model is the proven Flutter-class architecture.

---

## ADR 008: Statically-Typed Module+OO Language Compiling to WASM

### Context
JS dynamic typing makes type errors runtime phenomena (P4.1) [7,19,22]; the prototype model/this-binding frustrate encapsulation (P4.2) [17,18]; JIT warmup/deopt cliffs yield unpredictable performance (P4.4) [13,20,21]; WASM is predictably AOT-compilable [1,2,4,6].

### Decision
Adopt option (a): a statically-typed, module- and object-oriented language compiling to WASM, with first-class UI modules and explicit ownership/visibility; predictable AOT performance. Rejected: (b) retain dynamic typing; (c) optional static typing à la TypeScript (doesn't change the runtime).

### Status
Proposed.

### Consequences
- **Positive:** type errors become compile-time; encapsulation is a language primitive; WASM's predictable AOT ceiling replaces JIT heuristics; first-class modules give precise HMR/test/dependency units.
- **Negative:** new language ecosystem must be built (adoption risk — P9.5); abandons DOM/HTML/CSS as substrate (explicit architectural commitment).
- **Cross-references:** [ADR 009](#adr-009-two-level-type-verification) (type verification); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path); [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) (capability-scoped imports); [`Decision_Alternatives_concurrency-scheduling`](Decision_Alternatives_concurrency-scheduling.md) (scheduling model).

### Confidence
**High.** The dynamic-typing and JIT-unpredictability evidence is direct [7,19,22,13,20,21], and WASM's AOT profile is well-established [1,2,6].

---

## ADR 009: Two-Level Type Verification

### Context
The language goal (P4.1, P4.6) [3,5] wants a sound static type system compiling to WASM's validated type system. An "end-to-end verification" claim would overstate, since WASM validation is structural, not a soundness proof.

### Decision
Adopt option (a): a two-level guarantee — the compiler proves source-level soundness, and WASM verifies compiled well-formedness (structural only). Rejected: (b) claim end-to-end semantic verification (overclaim); (c) runtime-only type checks (fails P4.1).

### Status
Proposed.

### Consequences
- **Positive:** honest, compositional guarantee; source soundness + WASM well-formedness are independently meaningful.
- **Negative:** source-level soundness scope depends on [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)'s language design (escape hatches weaken it).
- **Cross-references:** [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language design determines soundness scope); [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) (module-boundary verification).

### Confidence
**Medium.** The WASM well-formedness portion is high-confidence; source-level soundness depends on the as-yet-undesigned language's escape-hatch discipline.

---

## ADR 010: CPU Bounding-Volume Hit-Testing + First-Class Device-Event Input

### Context
The DOM event model is bound to the DOM tree; virtual/canvas elements need reimplemented hit-testing (P5.1) [25,26,27,28,29]; stylus/multi-touch/gamepad are second-class (P5.2) *(catalog gap G6: direct peer-reviewed measurement of input-pipeline overhead for virtual/drawn elements is sparse; P5.2 rests partly on structural evidence)*. WASM cannot query GPU-resident buffers directly.

### Decision
Adopt option (a): a CPU-resident bounding-volume mirror of the GPU scene (GPU scene geometry remains source of truth; per-query GPU pick-buffer readback only for precise picks, never hot path); a first-class input model receiving raw device state and dispatching pointer/stylus/multi-touch/gamepad directly to hit render objects, which own gesture/state machines. Rejected: (b) per-query GPU pick-buffer readback only (latency/stalls); (c) delegate to host DOM (re-introduces canvas/DOM split).

### Status
Proposed.

### Consequences
- **Positive:** uniform first-class device events; render objects own their gestures; no DOM dependency in input.
- **Negative:** CPU mirror must be kept in sync with GPU scene; gesture-state machines are per-object authoring burden.
- **Cross-references:** [ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit) (render-graph scene geometry); [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model); [ADR 011](#adr-011-unified-virtual-focusaccessibility-annotation-layer) (focus model — input writes focus).

### Confidence
**High.** The DOM-bound hit-testing/input limitation is directly evidenced [25,26,27,28,29]; the CPU-mirror approach is standard in GPU UIs.

---

## ADR 011: Unified Virtual Focus/Accessibility Annotation Layer

### Context
In DOM UI, focus, tab order, and the focus ring are document-tree properties that browsers and assistive technologies (AT) observe directly. Canvas-rendered UI has no host-native focus model that AT can introspect (P5.3) [39,40,41]. Focus state and accessibility announcement share a bidirectional hard dependency (P6.1).

### Decision
Adopt a **unified virtual focus and accessibility tree**: one derived annotation layer over the render-object graph ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)), not two. Focus and tab order are virtual — computed from render-object metadata, independent in derivation from any host document (though they may *project* onto a minimal AT-resolvable DOM surface where platform AT contracts require it — see [`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md), Approach C). Focus annotations are the only mutable facet of that layer; they are stored on the annotation layer, not on the render objects themselves, preserving [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)'s module ownership. The annotation layer is **cached and invalidated on render-object mutation** (not lazily recomputed per query). **Input dispatch is the sole writer** of focus state ([ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input)); no other subsystem mutates it. **Accessibility is the sole reader/announcer**, consuming the current focus annotation to drive AT notifications and focus-ring rendering.

Alternatives considered:
- (a) Unified virtual focus + a11y tree, input writes / a11y reads [chosen].
- (b) Separate focus and a11y trees, bridge-synchronized — rejected for duplication and coherence drift.
- (c) Full DOM-bound focus — rejected; canvas has no host DOM AT can observe, and full DOM binding forfeits the virtual model.

### Status
Proposed.

### Consequences
- Single source of truth: focus and a11y never diverge; no sync layer *within* the virtual model, dissolving the P6.1 coupling. (An optional DOM projection surface for AT resolvability may introduce a read-only sync boundary — see [`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md) Approach C.)
- Strict writer discipline: only input dispatch ([ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input)) mutates focus annotations, preventing animation/layout/script races.
- Derivation coupling: the annotation layer derives from the render-object graph ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)); object-model changes ripple into focus/a11y shape. [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) recognises this layer as a first-class derived annotation, distinct from the platform bridge.
- Focus-ring rendering reads this layer, not DOM pseudo-classes.
- Accessibility-tree derivation and any DOM projection surface are specified in [`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md), not here.

### Confidence
**High** on unifying focus and accessibility into one derived annotation layer and on input-dispatch-as-sole-writer. **Medium** on whether a minimal DOM projection surface is required for AT resolvability on web targets — that question is deferred to [`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md).

---

## ADR 012: Navigation/URL Contract and Explicit SEO Scope

### Context
URL/history APIs assume addressable document states, but canvas-rendered apps fabricate lossy navigation (P6.2) [27,28,29] *(catalog gap G5: direct canvas-navigation studies are sparse; evidence is indirect via AJAX-state inference)*. SEO depends on DOM content; pure-canvas surfaces are invisible to crawlers (P6.3).

### Decision
Treat navigation/URL as a **structured navigation/state contract**: the app exposes declared routes plus serialisable state to the host explicitly. Handle SEO via **explicit per-app scope declaration**: each app declares either a *non-SEO domain* or a *hybrid/structured-content export*. Rejected: (b) ad-hoc URL mappings + universal DOM SEO; (c) no SEO handling.

### Status
Proposed.

### Consequences
- Predictable, restorable navigation; uniform host integration (P6.2).
- Aligns with [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree): routes and serialisable state are first-class objects.
- Non-SEO apps relieved of DOM-a11y-SEO overhead; hybrid apps bear an explicit export obligation.
- Cross-references [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree); adoption path per [`Decision_Alternatives_adoption-interop`](Decision_Alternatives_adoption-interop.md).

### Confidence
**Medium.** The navigation contract follows from G5/P6.2; the SEO scope taxonomy is inferred from the Goal rather than an enumerated Solution requirement.

---

## ADR 013: No WASM↔DOM Boundary in the Hot Path

### Context
The WASM↔JS/DOM boundary imposes per-call overhead unacceptable in the rendering hot path. Empirical evidence (P7.4) shows real WASM usage is coarse-grained, not per-UI-element [2,6]. A true "no interop boundary" architecture is achievable only if the scene graph stays entirely inside WASM.

### Decision
Adopt option (a): compile the UI itself to WASM so the layout module issues WebGPU draw calls directly; **no WASM↔DOM boundary in the hot path**. Rejected: (b) JS host with WASM escape hatch (retains boundary); (c) dual JS/WASM with DOM fallback (doubles maintenance).

### Status
Proposed.

### Consequences
**Hot-path definition (scope of this ADR):** the hot path comprises per-frame operations — layout, composition, draw-call emission, hit-testing, and input dispatch. These must run entirely inside WASM with no WASM↔DOM boundary crossing. Text rasterization/glyph-generation is hot-path if it occurs per-frame; text *measurement* is hot-path for layout. Accessibility-tree mutation, navigation/state serialization, and SEO export are **non-hot-path** and may cross a controlled interop boundary (per [`Decision_Alternatives_adoption-interop`](Decision_Alternatives_adoption-interop.md), Approach A).

This decision is **conditional on [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)** (Composition/Document-Model residency): the scene graph must remain inside WASM end-to-end. If any scene-graph node escapes to JS, the no-boundary guarantee is voided and per-call overhead returns.

Open dependencies: accessibility ([`Decision_Alternatives_accessibility-bridge`](Decision_Alternatives_accessibility-bridge.md)), text rendering ([`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md)), and adoption/interop ([`Decision_Alternatives_adoption-interop`](Decision_Alternatives_adoption-interop.md)) remain unresolved. These subsystems may require controlled, non-hot-path boundary crossings as classified above.

Cross-references: [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (compositor threading), [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language), [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (startup/binary budgets — note: any performance budgets referenced by interop escalation live in ADR 017, not here; this ADR states the structural rule).

### Confidence
**High.** The boundary-overhead evidence [2,6] is direct, and compiling the UI to WASM is the only option that eliminates the per-call cost structurally rather than mitigating it.

---

## ADR 014: Design-Tool-as-Runtime + Typed Component Testing

### Context
Design tools approximate layout but cannot reproduce the runtime layout interpreter (P8.1) [33,30]. DOM/CSS e2e tests break on selector drift and cross-engine differences (P8.4) [31,30,22].

### Decision
**Embed the deterministic author-owned WASM renderer directly into the design tool** — same engine in design and runtime; parity by construction. Layout parity is guaranteed by the WASM sandbox; rendering parity is conditional on a deterministic GPU path with a software-rasteriser fallback, bounded to a raster class (cross-vendor pixel-identical parity is not guaranteed — see Caveat below). **Replace DOM-selector e2e tests with typed component contracts** (typed inputs/outputs, explicit child slots). Rejected: (b) separate approximation + selector tests; (c) hybrid (reintroduces selector fragility).

### Status
Proposed.

### Consequences
- **Positive.** Single source of truth for layout; zero layout divergence between design and runtime. Rendering divergence is bounded to a raster class (not universal across GPUs). Test suites survive refactors and engine swaps because contracts are semantic, not selector-based. Cross-references: [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model) supplies the typed contract substrate; [ADR 016](#adr-016-unified-author-owned-trace-with-split-determinism) (unified trace) consumes contract events for end-to-end observability.
- **Caveat — GPU determinism.** Pixel-perfect parity holds only where rasterisation is deterministic. GPU raster is not cross-vendor deterministic; the same scene may sample differently across hardware. Mitigation: ship a deterministic GPU path (known-good shader/config) with a software-rasteriser fallback for verification and CI. Parity is guaranteed within a raster class, not universally across GPUs.

### Confidence
**Medium.** The architectural soundness of same-engine embed and typed contracts is high. Residual risk concentrates entirely on rendering determinism: until the deterministic GPU path or software-rasteriser fallback is proven across target vendors, cross-hardware pixel-perfect claims remain conditional.

---

## ADR 015: HMR via Serialisable Scene-Graph State Rehydration

### Context
The DOM's single mutable tree couples view/style/behaviour; surgical code replacement rarely preserves state (P8.2) [25,26,30,31] *(inferred — no HMR-specific peer-reviewed evidence; the claim is an inference from mutation-testing brittleness [25,26] and test-repair [30,31])*.

### Decision
Adopt option (a): treat module state as serialisable scene-graph data; on reload, capture the outgoing module's state, load a fresh module, rehydrate. Whole-module replacement, not surgical. Rejected: (b) surgical in-place replacement (state drift); (c) full reload (discards state).

### Status
Proposed.

### Consequences
- HMR correctness depends on every hot-reloadable module exposing owned, serialisable scene-graph state ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)).
- Replacement granularity is a freshly loaded module ([ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)).
- Rehydration trace deferred to [ADR 016](#adr-016-unified-author-owned-trace-with-split-determinism). Modules that cannot serialise fall back to full reload (option c).

### Confidence
**Medium.** Sound given [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)'s owned serialisable state and [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)'s replacement unit, but both are Proposed and no direct HMR evidence validates the model empirically.

---

## ADR 016: Unified Author-Owned Trace with Split Determinism

### Context
Browser DevTools expose JS/perf/render/DOM panels that are only coarsely observable; the causal link from JS through layout/paint/composite requires expert inference (P8.3) [20,21] *(inferred — no direct DevTools-observation study cited; the catalog uses the need to empirically study perf bugs [20] and auto-fix them [21] as indirect evidence)*.

### Decision
Adopt a **single author-owned trace** spanning logic, layout, and draw, so any frame drop is root-causable in one correlated timeline. Determinism is **split**: layout determinism is guaranteed by the WASM sandbox; rendering determinism requires a deterministic GPU path **with a software-rasteriser fallback** (the fallback is used for verification/CI; both paths are present, not mutually exclusive alternatives). Rejected: (b) separate per-stage panels; (c) external profiler only.

### Status
Proposed.

### Consequences
- **GPU determinism risk:** rendering determinism depends on a fallback that remains an open implementation risk; without it, trace correlation across the draw stage collapses.
- Unifies the diagnostic surface, reducing expert inference demand.
- Couples this ADR to [ADR 014](#adr-014-design-tool-as-runtime--typed-component-testing) (design-tool) for author-facing trace presentation, [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout) for WASM-sandboxed layout guarantees, and [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (scheduler) for stage scheduling that the trace must instrument.
- Increases build/tooling complexity via the dual GPU/software-raster path.

### Confidence
**Medium.** The unified-trace concept is well-motivated by existing DevTools gaps, but rendering-determinism fallback viability is unproven; if the deterministic GPU path or software rasteriser cannot be delivered, the single-trace premise degrades to per-stage panels — the very option rejected here.

---

## ADR 017: Compiled WASM Binary + WebGPU Pipeline Precompilation

### Context
HTML/CSS/JS text parsed at runtime imposes a parse/compile startup floor (P9.1) [1,6,20,21,33,34]. The WASM module IS the layout engine, so startup reduces to a single module load plus GPU shader warm-up.

### Decision
Adopt option (a): compile to a compact WASM binary whose startup is bounded by decode (streaming-compiled while downloading), with WebGPU pipeline precompilation removing the GPU-side startup floor. Rejected: (b) JS+text-bundle optimisation (retains parse floor); (c) interpreter/source-level execution (defers cost to runtime).

### Status
Proposed.

### Consequences
- **Positive:** single streaming decode, no text parse floor; GPU pipelines precompiled asynchronously overlap with module decode.
- **Negative:** single large module raises streaming-compile cost; monolithic-binary debuggability degrades.
- **Trade-off:** Hot-path inlining ([ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path)) must be decided at compile time. Language choice ([ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)) constrained to WASM-targeting toolchains. Dependency bundling ([ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking)) must avoid defeating streaming compilation granularity.

### Confidence
**Medium.** WebGPU pipeline precompilation is still maturing across implementations, and the streaming-compile cost of one large module may not stay sub-decode without careful sectioning. Both risks are mitigable but not yet proven at target scale.

---

## ADR 018: Capability-Scoped Imports + Component-Model Tree-Shaking

### Context
The npm supply chain is a structural liability (P9.2) [43]; transitive dependencies expand the trusted computing base. Dependency bloat compounds this (P9.3) [44,45]. Framework churn imposes switching costs (P9.4) [30,31,46]. The verifiable WASM module boundary [3,5] offers a substrate where the unit of trust is a hermetic, hash-attested module.

### Decision
Adopt option (a): a language-level standard library + explicit typed imports with compile-time tree-shaking (component-model-enforced) + capability-sandboxed least-privilege grants; the verifiable module boundary makes components modules, not framework artifacts. Rejected: (b) retain npm transitive trust + SCA tooling; (c) curated allowlist without capability scoping.

### Status
Proposed.

### Consequences
- Trusted computing base shrinks to declared, typed, capability-scoped imports; supply-chain risk becomes auditable per module.
- Relies on [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (module boundary) and [ADR 009](#adr-009-two-level-type-verification) (verification attestation).
- npm interop follows [`Decision_Alternatives_adoption-interop`](Decision_Alternatives_adoption-interop.md) (curated adapters behind capability-scoped boundaries).
- Adoption cost: capability instrumentation and component-model tooling are still maturing.

### Confidence
**Medium.** The tree-shaking guarantee tracks WASM component-model maturity (not yet production-hardened); capability sandboxing implementation cost is non-trivial and ecosystem tooling remains uneven.

---

## Decision Alternatives (Low confidence)

The following decision points are recorded as separate files because confidence is too low to commit a standard ADR. Each contains a decision-point statement, the reason for uncertainty, ≥2 approaches with pros/cons, and a recommended approach.

| File | Decision Point | Why Low Confidence | Recommended Approach |
|------|----------------|--------------------|----------------------|
| [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) | Text rendering strategy | P3.5 is the catalog's decisive hard problem; optimal off-DOM implementation unresolved | Approach B (hidden DOM text surface) as interim |
| [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) | Concurrency/scheduling model | Multiple viable models; evidence (P4.3) underdetermines the choice | Approach A (cooperative coroutines + retain-mode loop), pending spike |
| [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) | Accessibility bridge approach | P6.1 is co-decisive; WASM→platform-a11y-API access immature on web | Approach C (hybrid: virtual tree + read-only DOM projection) |
| [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) | Adoption/interop strategy | P9.5 is strategic; ecosystem inertia dominates | Approach A (host-DOM interop bridges), time-boxed 18 months |

---

## Decision Graph (Key Dependencies)

```
ADR 007 (render-object tree) ──┬── ADR 002 (invalidation)
                               ├── ADR 004 (layout) ── Alt-text
                               ├── ADR 011 (focus) ── Alt-a11y
                               ├── ADR 013 (hot path) ── Alt-adoption
                               └── ADR 014/015 (tooling/HMR)

ADR 001 (render-graph) ── ADR 003 (compositor) ── ADR 013

ADR 008 (language) ──┬── ADR 009 (type verification)
                    ├── Alt-concurrency
                    ├── ADR 017 (binary/startup)
                    └── ADR 018 (deps) ── Alt-adoption
```

The two decisive hard problems (text rendering, accessibility bridge) and the strategic adoption risk are deliberately kept as Decision Alternatives rather than committed ADRs, reflecting genuine uncertainty that requires further spikes before ratification.

---

## How These Decisions Were Produced

Four-wave sub-agent orchestration:
1. **Wave 1 — Decision-point extraction**: 9 sub-agents (one per rough-draft cluster) identified discrete decision points.
2. **Wave 2 — Drafting**: 22 sub-agents drafted ADRs (High/Medium) or Alternatives (Low), one per decision point.
3. **Wave 3 — Cross-ADR consistency**: 6 pairwise sub-agents checked interacting ADRs for conflicts; corrections applied (compilation-ownership reconciliation, text-path contract weakening, focus/a11y annotation-layer clarification, hot-path definition, determinism-language alignment).
4. **Wave 4 — Evidence traceability**: 5 sub-agents verified every ADR's Context against the rough draft; all SUPPORTED. One citation mismatch in ADR 003 was corrected.

Every Context is traceable to a `P x.y` problem entry and `[n]` reference in [`PROBLEM_CATALOG.md`](../PROBLEM_CATALOG.md). Circumstantial or catalog-acknowledged-gap evidence (G1, G2, G4, G5, G6) is flagged inline.

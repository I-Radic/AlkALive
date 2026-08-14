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
| [ADR 019](#adr-019-accessibility-deferred) | Defer Accessibility Bridge — No DOM Mirror | High |
| [ADR 020](#adr-020-metadata-only-dom-layer-for-seo) | Metadata-Only DOM Layer for SEO — No UI DOM Interop | High |
| [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) | Main Thread + On-Demand WASM Threads with Socket IPC | High |
| [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) | Forked HarfRust as the In-WASM Text Shaping/Rasterization Stack | High |

**Decision Alternatives (Low confidence — separate files, now RESOLVED):**
The following four Decision Alternatives files have been **resolved** by ADRs 019–022 above. They are retained for historical context but their "Recommended Approach" is **superseded** by the project owner's non-negotiable choices.
- [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) — Text rendering strategy → **resolved by [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)** (forked HarfRust; overrides the file's prior Approach B)
- [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) — Concurrency/scheduling model → **resolved by [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc)** (main thread + on-demand WASM threads + socket IPC; a new hybrid not in the file's A/B/C)
- [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) — Accessibility bridge approach → **resolved by [ADR 019](#adr-019-accessibility-deferred)** (Approach A, no DOM mirror, a11y deferred; overrides the file's prior Approach C)
- [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) — Adoption/interop strategy → **resolved by [ADR 020](#adr-020-metadata-only-dom-layer-for-seo)** (Approach C, DOM only for metadata/SEO; overrides the file's prior Approach A)

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

**Cross-references:** Graph compilation (reordering/batching/occlusion-cull) is scheduled by [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc)'s main-thread + on-demand WASM-worker pool (socket IPC over SharedArrayBuffer) and committed through [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor)'s single-GPUDevice compositor; the occlusion-cull pass may run on any worker but serializes against the compositor-wide depth/visibility buffer. The compositor (ADR 003) consumes the compiled graph output and both ADRs must agree on a shared attachment-format and pass-boundary contract (to be specified in a future rendering-ABI ADR). Layout (ADR 004) and the object model (ADR 007) are downstream consumers that emit render-graph IR rather than box trees.

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
Adopt **option (a)**: a single dedicated render thread owns the lone `GPUDevice` and serializes every render-graph submission from all scene graphs — the persistent GPUDevice-owner thread of [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc)'s model (either the main thread or a dedicated non-on-demand worker); on-demand WASM worker threads never acquire the GPUDevice, they feed render-graph IR over SharedArrayBuffer/socket IPC for the single owner to merge/submit. Scene data (instance tables, transforms, draw lists) lives in a `SharedArrayBuffer` under COOP/COEP (`Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`). Graphs emit immutable render-graph IR ([ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit)); the render thread merges, compiles, reorders, batches, then submits. The occlusion-cull pass executes on the render thread against a compositor-wide depth/visibility buffer.

### Status
Proposed.

### Consequences
- **Positive:** one authoritative submission path; no GPUDevice-sharing hazards; graphs compose without lock-free complexity.
- **Negative (COOP/COEP risk):** cross-origin isolation headers are required; this conflicts with embedding third-party iframes. Mitigations: `credentialless` COEP or iframe proxying. If unworkable, fall back to option (b) per-graph separate devices (loses shared compositor).
- **Cross-references:** [ADR 001](#adr-001-render-graph-ir-as-the-atomic-rendering-unit) (render-graph IR is the input); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path interop relies on this single-thread submission); [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) (threading model: render thread is the persistent GPUDevice owner distinct from on-demand workers); [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (forked HarfRust text stack emits glyph-run IR consumed by this compositor).

### Confidence
**Medium.** The single-owner model is sound, but the COOP/COEP deployment constraint is a real-world risk that may force option (b) on iframe-heavy hosts.

---

## ADR 004: Pluggable Constraint-Solver Layout with Mandatory Text-Flow Measurement Contract

### Context
Flexbox and Grid are fixed rectangular-only solvers with unstable cross-engine semantics (P2.3, P2.4) [33,30]. They couple style-driven box-tree recalculation to layout, forcing full-tree reflows on minor style changes and producing divergent results across browsers. We need a layout substrate that (1) operates over first-class render objects rather than a CSS box tree, (2) admits non-rectangular constraints, and (3) emits results consumable directly by GPU transforms without an intermediate layout-tree serialization.

### Decision
Adopt a **pluggable constraint solver** (Cassowary, impulse, or graph-based) operating over first-class objects. The layout-tree is solver-internal and never re-derived from styles. A **mandatory text-flow measurement contract** is always present: the solver consumes a synchronous *measured-run* interface (glyph-run metrics) regardless of backend. The backing implementation is the **forked HarfRust** in-WASM stack per [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack); no DOM text surface is permitted ([ADR 020](#adr-020-metadata-only-dom-layer-for-seo), [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)). Solver outputs (transforms, glyph runs) feed GPU transforms directly, with no style-driven box-tree recalculation.

Alternatives considered: (b) a single fixed solver — rejected for forfeiting non-rectangular and domain-specific layouts; (c) author-everything including text — rejected for re-introducing the exact fragmentation instability (P2.4) this design avoids.

### Status
Proposed.

### Consequences
- **Locality** ([ADR 002](#adr-002-per-module-dirty-rect-invalidation-with-layout-locality)): per-module dirty-rect invalidation holds because the solver recomputes only constrained object subsets, never the full box tree.
- **Styling** ([ADR 005](#adr-005-object-owned-per-instance-styling)): per-instance object-owned styles remain authoritative; the solver consumes style values as constraint inputs and never mutates them.
- **Text rendering** ([ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)): the layout engine consumes a synchronous measured-run interface; the backing implementation is the forked in-WASM HarfRust stack per ADR 022 (no DOM text surface per [ADR 020](#adr-020-metadata-only-dom-layer-for-seo)/[ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)). This keeps glyph metrics consistent across outer-solver swaps.
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
- **Cross-references:** [ADR 002](#adr-002-per-module-dirty-rect-invalidation-with-layout-locality) (invalidation) operates on this tree; [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout) is a stage over it; [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language) defines the module/component unit; [ADR 011](#adr-011-unified-virtual-focusaccessibility-annotation-layer) (focus) derives from it; the text solution is resolved by [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (in-WASM HarfRust stack) and accessibility by [ADR 019](#adr-019-accessibility-deferred) (a11y deferred, no DOM bridge); these were decisive dependencies, now satisfied.

### Confidence
**High.** The impedance-mismatch evidence is direct [30,31,27,28,29], and the owned-tree model is the proven Flutter-class architecture.

---

## ADR 008: Statically-Typed Module+OO Language Compiling to WASM

### Context
JS dynamic typing makes type errors runtime phenomena (P4.1) [7,19,22]; the prototype model/this-binding frustrate encapsulation (P4.2) [17,18]; JIT warmup/deopt cliffs yield unpredictable performance (P4.4) [13,20,21]; WASM is predictably AOT-compilable [1,2,4,6].

### Decision
Adopt option (a): a statically-typed, module- and object-oriented language compiling to WASM, with first-class UI modules and explicit ownership/visibility; predictable AOT performance. Rejected: (b) retain dynamic typing; (c) optional static typing à la TypeScript (doesn't change the runtime).

### Status
Amended by [ADR-027](ADR_027_monotonicity_types_phased.md) Phase 2 (Monotonicity Qualifiers subsection below). Originally Proposed.

### Consequences
- **Positive:** type errors become compile-time; encapsulation is a language primitive; WASM's predictable AOT ceiling replaces JIT heuristics; first-class modules give precise HMR/test/dependency units.
- **Negative:** new language ecosystem must be built (adoption risk — P9.5); abandons DOM/HTML/CSS as substrate (explicit architectural commitment).
- **Cross-references:** [ADR 009](#adr-009-two-level-type-verification) (type verification); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path); [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) (capability-scoped imports); [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) (scheduling model — resolved); [ADR 027](ADR_027_monotonicity_types_phased.md) (monotonicity qualifiers — Phase 2 implemented).

### Confidence
**High.** The dynamic-typing and JIT-unpredictability evidence is direct [7,19,22,13,20,21], and WASM's AOT profile is well-established [1,2,6].

### Monotonicity Qualifiers (ADR-027 Phase 2 Amendment)

*Added by [ADR-027](ADR_027_monotonicity_types_phased.md) Phase 2 (implemented).*

The statically-typed language defined by this ADR is extended to include
`monotone` and `antitone` as **first-class type qualifiers** on collection
types. They are reserved keywords in the lexer (breaking change from Phase 1,
where they were plain identifiers — see ADR-027 for the migration path).

**Qualifier syntax.** A type is `Qualifier? BaseType`, where:

```
Type       := ('monotone' | 'antitone')? BaseType
BaseType   := 'i32' | 'f32' | 'string' | 'bool' | 'Vec' '<' Type '>' | Ident
```

In source this reads as:

- `Vec<T>` — unrestricted (the default; qualifier omitted)
- `monotone Vec<T>` — collection may only grow (shrink ops rejected)
- `antitone Vec<T>` — collection may only shrink (grow ops rejected)

Qualifiers may appear on `let` bindings, function parameters, function return
types, and nested inside `Vec<...>` element types (covariant).

**Subtyping lattice.**

```
        unrestricted (bottom — most permissive value)
       /                  \
   monotone            antitone   (incomparable tops)
```

- `unrestricted <: monotone` and `unrestricted <: antitone` (an unrestricted
  value is admissible wherever a monotone or antitone one is required).
- `monotone` and `antitone` are **not comparable**.
- `monotone` / `antitone` are **not** subtypes of `unrestricted` (a qualified
  value cannot escape to a context that might violate its invariant).
- `Vec<T>` is covariant in its element type: `Vec<unrestricted i32> <:
  monotone Vec<monotone i32>`.

**Compile-time enforcement.** Qualifiers are enforced **solely by the type
checker** (`crates/alkalive-compiler/src/typechecker.rs`), not at runtime.
Violations are hard compile-time errors (`CompileError::Type(TypeErrorSet)`),
not runtime panics. The runtime consumes the resolved qualifiers as IR metadata
(`ir::Monotonicity` on `CollectionDeclIR`) purely as an *optimisation hint*
for seminaïve evaluation (ADR-025) — never as a soundness backstop.

**Cross-references.**
- [ADR 027](ADR_027_monotonicity_types_phased.md) — full phased design; Phase 2 implemented.
- [ADR 009](#adr-009-two-level-type-verification) — amended in parallel to add monotonicity as a third verification dimension.
- [ADR 025](ADR_025_incremental_computation.md) — Phase 2 IR metadata enables seminaïve evaluation.

### Implementation Status (Wave 4 Audit)

*Added by [Wave 4 — ADR Reconciliation](../alkalive-wave-04-adr-reconciliation.md), based on the [Wave 0 audit](../alkalive-wave-00-audit.md) §4.3 and §10.2. This subsection supersedes any conflicting "implemented" claim elsewhere in this ADR with respect to the production `.alk` pipeline.*

**The current implementation is a scene-description DSL frontend, not a general-purpose programming language.** As of the Wave 0 audit, the ADR's stated target — a statically-typed, module- and object-oriented language compiling to WASM — is **not** what ships.

The actual production pipeline is:

1. `.alk` source is a small declarative grammar (`module`, `scene`, `text`, `input-field`, plus a handful of styling properties). See audit §4.2 for the EBNF.
2. `alkalive_compiler::compile(src) -> Result<SceneIR, CompileError>` lowers that source to a **JSON-serializable `SceneIR`** (`crates/alkalive-compiler/src/ir.rs`). The compiler emits JSON-shaped data, not bytecode.
3. The **WASM** in the system is the pre-built runtime cdylib (`crates/alkalive-runtime-wasm`), compiled from Rust by `cargo build --target wasm32-unknown-unknown`. The `.alk` source is **embedded into the WASM binary at build time** via `include_str!("../../../examples/hello.alk")` (`crates/alkalive-runtime-wasm/src/lib.rs:52`) and **compiled to a `SceneIR` at startup** inside the WASM runtime. The user's `.alk` source is data, not a WASM-compilation unit.

Concretely, relative to this ADR's stated decision:

| ADR-008 claim | Wave 0 audit finding |
|---------------|----------------------|
| "statically-typed" | **0% implemented.** No type system is exercised by the production `.alk` pipeline (see [ADR 009](#adr-009-two-level-type-verification)'s Implementation Status). |
| "object oriented" | **0% implemented.** No classes, methods, or inheritance exist in the `.alk` grammar. |
| "first-class UI modules" | **~5% implemented.** `module` is a single named wrapper around one `scene`; there is no module system beyond that. |
| "compiling to WASM" | **0% implemented by the compiler.** The compiler emits a JSON `SceneIR`. The only WASM in the system is the runtime cdylib built from Rust by `cargo`. |
| Functions, variables, control flow, expressions | **0% implemented.** No `fn`, `let`, `if`, `while`, `return`, or operator grammar in the `.alk` language. |

**This ADR describes the aspirational target, not the current implementation.** The Wave 0 audit deliberately records the gap rather than redefining the ADR, because the ADR's rationale (P4.1, P4.2, P4.4) remains the long-term motivation. Future waves may close the gap; until then, no claim of "statically-typed", "object-oriented", or "compiling to WASM" should be made about the running system.

**The scene-description DSL is a legitimate interim architecture**, not a defect. It is comparable to how SwiftUI's declarative views are compiled into Swift, or how JSX is compiled into JavaScript at build time: a small declarative surface lowered to a runtime-consumable IR. The Wave 0 audit (§3.1, §10.2) classified the demo as **fully genuine** under this architecture — every pixel is drawn by the real WebGL2 backend from a `SceneIR` produced by the real AlkALive compiler.

**Cross-references.**
- [Wave 0 audit](../alkalive-wave-00-audit.md) §4 (Compiler Analysis) and §10.2 (Why the compiler doesn't generate WASM) — primary evidence.
- [Wave 4 reconciliation](../alkalive-wave-04-adr-reconciliation.md) — this amendment.
- [ADR 009](#adr-009-two-level-type-verification) — amended in parallel; "two-level type verification" is likewise 0% implemented.

---

## ADR 009: Two-Level Type Verification

### Context
The language goal (P4.1, P4.6) [3,5] wants a sound static type system compiling to WASM's validated type system. An "end-to-end verification" claim would overstate, since WASM validation is structural, not a soundness proof.

### Decision
Adopt option (a): a two-level guarantee — the compiler proves source-level soundness, and WASM verifies compiled well-formedness (structural only). Rejected: (b) claim end-to-end semantic verification (overclaim); (c) runtime-only type checks (fails P4.1).

### Status
Amended by [ADR-027](ADR_027_monotonicity_types_phased.md) Phase 2 (Monotonicity Verification Dimension subsection below). Originally Proposed.

### Consequences
- **Positive:** honest, compositional guarantee; source soundness + WASM well-formedness are independently meaningful.
- **Negative:** source-level soundness scope depends on [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)'s language design (escape hatches weaken it).
- **Cross-references:** [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language design determines soundness scope); [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) (module-boundary verification); [ADR 027](ADR_027_monotonicity_types_phased.md) (monotonicity — adds a third verification dimension).

### Confidence
**Medium.** The WASM well-formedness portion is high-confidence; source-level soundness depends on the as-yet-undesigned language's escape-hatch discipline. The monotonicity-qualifier dimension added by ADR-027 Phase 2 is **High** (implemented and tested).

### Monotonicity Verification Dimension (ADR-027 Phase 2 Amendment)

*Added by [ADR-027](ADR_027_monotonicity_types_phased.md) Phase 2 (implemented).*

This ADR originally specified a **two-level** guarantee: (1) source-level
soundness, (2) WASM structural well-formedness. ADR-027 Phase 2 adds a third
verification dimension: **monotonicity-qualifier enforcement**.

The three levels of verification now compose as follows:

1. **Source-level type soundness** — the compiler's type checker proves that
   the program is type-correct at the source level (variables resolve, return
   types match, etc.). This is the level originally specified by ADR-009.
2. **Monotonicity-qualifier enforcement** — the same type checker proves that
   `monotone` / `antitone` qualifiers flow correctly through the program:
   function parameters respect declared qualifiers, method calls do not violate
   the grow/shrink invariant, and return values are subtype-compatible with
   declared return types. Monotonicity violations are **compile-time type
   errors**, not runtime panics: a `monotone Vec<T>` that calls `.remove()` is
   rejected at `compile_typecheck()` time with `CompileError::Type(TypeErrorSet)`.
3. **WASM structural well-formedness** — the WASM validator verifies the
   compiled binary's structural type correctness (the existing level 2 of
   ADR-009; unchanged).

The three levels are independent and additive: a program may pass level 1 and
3 yet fail level 2 (e.g., source is well-typed and the WASM is well-formed, but
a `monotone` collection was shrunk). Conversely, level 2 has no impact on the
WASM binary structure (qualifiers are erased before code generation); it only
informs runtime optimisation via IR metadata.

The monotonicity dimension is implemented by
`crates/alkalive-compiler/src/typechecker.rs` and integrated into the public
compiler entry point `compile_typecheck(src)` in
`crates/alkalive-compiler/src/codegen.rs`.

**Cross-references.**
- [ADR 027](ADR_027_monotonicity_types_phased.md) — full phased design; Phase 2 implemented.
- [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) — amended in parallel to formally include `monotone`/`antitone` as type qualifiers in the language design.
- [ADR 025](ADR_025_incremental_computation.md) — runtime consumes IR `Monotonicity` metadata for seminaïve evaluation (optimisation, not verification).

### Implementation Status (Wave 4 Audit)

*Added by [Wave 4 — ADR Reconciliation](../alkalive-wave-04-adr-reconciliation.md), based on the [Wave 0 audit](../alkalive-wave-00-audit.md) §4.3 and §8. This subsection supersedes any conflicting "implemented" claim elsewhere in this ADR with respect to the production `.alk` pipeline.*

This ADR specifies a **two-level** guarantee: (a) the compiler proves source-level soundness, and (b) WASM verifies compiled well-formedness. **Neither level is implemented in the production `.alk` pipeline.**

| Level | ADR-009 claim | Wave 0 audit finding |
|-------|---------------|----------------------|
| (a) Source-level soundness | "the compiler proves source-level soundness" | **0% implemented.** No type checker is exercised by the production `.alk` pipeline. The `.alk` grammar (audit §4.2) has no `fn`, `let`, `if`, `while`, `return`, or operator constructs, so there is nothing to type-check at the source level. |
| (b) WASM well-formedness | "WASM verifies compiled well-formedness" | **N/A.** The AlkALive compiler does not generate WASM. The compiler emits a JSON `SceneIR`. The only WASM in the system is the pre-built runtime cdylib, compiled from Rust by `cargo` (see [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)'s Implementation Status). WASM validation applies to that runtime, not to user `.alk` source. |
| (Monotonicity enforcement, per ADR-027 Phase 2) | "implemented" | See the ADR-027 amendment above. The Wave 0 audit evaluated the production pipeline and did not identify any type system in the `.alk` flow. |

**What the compiler actually performs is value-level validation**, not type verification. The `lower(&ModuleDecl) -> Result<SceneIR, CodegenError>` pass in `crates/alkalive-compiler/src/codegen.rs` checks:

- `font-size` is positive (> 0);
- `rotation` values are finite floats;
- a `position: below text` reference requires a preceding `Text` node in the same scene.

These are runtime-value sanity checks on the AST, not type-system proofs. They are emitted as `CodegenError` variants, not as `CompileError::Type(TypeErrorSet)`. There is no source-level soundness guarantee of the kind ADR-009 promises.

**This ADR describes the aspirational target, not the current implementation.** Future waves may introduce a real type system (e.g., via the ADR-027 Phase 2 type-checker pass being wired into the production pipeline); until then, no claim of "two-level type verification" or "source-level soundness" should be made about the running system.

**Cross-references.**
- [Wave 0 audit](../alkalive-wave-00-audit.md) §4.3 (What the compiler does NOT have) and §8 (Gap Analysis) — primary evidence.
- [Wave 4 reconciliation](../alkalive-wave-04-adr-reconciliation.md) — this amendment.
- [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) — amended in parallel; the statically-typed language it presupposes is likewise 0% implemented.

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
Adopt a **unified virtual focus and accessibility tree**: one derived annotation layer over the render-object graph ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)), not two. Focus and tab order are virtual — computed from render-object metadata, independent in derivation from any host document. Focus annotations are the only mutable facet of that layer; they are stored on the annotation layer, not on the render objects themselves, preserving [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)'s module ownership. The annotation layer is **cached and invalidated on render-object mutation** (not lazily recomputed per query). **Input dispatch is the sole writer** of focus state ([ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input)); no other subsystem mutates it. **Focus-ring rendering is the sole active reader** of focus annotations. AT announcement / a11y-tree derivation is **deferred** per [ADR 019](#adr-019-accessibility-deferred) (no DOM bridge, no DOM projection surface).

Alternatives considered:
- (a) Unified virtual focus + a11y tree, input writes / a11y reads [chosen].
- (b) Separate focus and a11y trees, bridge-synchronized — rejected for duplication and coherence drift.
- (c) Full DOM-bound focus — rejected; canvas has no host DOM AT can observe, and full DOM binding forfeits the virtual model.

### Status
Proposed.

### Consequences
- Single source of truth: focus and a11y never diverge; no sync layer *within* the virtual model, dissolving the P6.1 coupling. (No DOM projection surface exists — a11y announcement is deferred per [ADR 019](#adr-019-accessibility-deferred).)
- Strict writer discipline: only input dispatch ([ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input)) mutates focus annotations, preventing animation/layout/script races.
- Derivation coupling: the annotation layer derives from the render-object graph ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)); object-model changes ripple into focus/a11y shape. [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) recognises this layer as a first-class derived annotation, distinct from the platform bridge.
- Focus-ring rendering reads this layer, not DOM pseudo-classes.
- Accessibility-tree derivation and any DOM projection surface are deferred per [ADR 019](#adr-019-accessibility-deferred); none exist in this phase.

### Confidence
**High** on unifying focus and accessibility into one derived annotation layer and on input-dispatch-as-sole-writer. A11y announcement is deferred per [ADR 019](#adr-019-accessibility-deferred) (no DOM bridge, no DOM projection surface).

---

## ADR 012: Navigation/URL Contract and Explicit SEO Scope

### Context
URL/history APIs assume addressable document states, but canvas-rendered apps fabricate lossy navigation (P6.2) [27,28,29] *(catalog gap G5: direct canvas-navigation studies are sparse; evidence is indirect via AJAX-state inference)*. SEO depends on DOM content; pure-canvas surfaces are invisible to crawlers (P6.3).

### Decision
Treat navigation/URL as a **structured navigation/state contract**: the app exposes declared routes plus serialisable state to the host explicitly. Handle SEO via the concrete mechanism specified in [ADR 020](#adr-020-metadata-only-dom-layer-for-seo): DOM limited to `<title>`/`<meta>` plus a static HTML snapshot for search-engine crawlers (no UI DOM interop). Rejected: (b) ad-hoc URL mappings + universal DOM SEO; (c) no SEO handling.

### Status
Proposed.

### Consequences
- Predictable, restorable navigation; uniform host integration (P6.2).
- Aligns with [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree): routes and serialisable state are first-class objects.
- Non-SEO apps relieved of DOM-a11y-SEO overhead; the SEO surface is the concrete `<title>`/`<meta>` + static snapshot per [ADR 020](#adr-020-metadata-only-dom-layer-for-seo), and a11y is deferred per [ADR 019](#adr-019-accessibility-deferred).
- Cross-references [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree); SEO mechanism per [ADR 020](#adr-020-metadata-only-dom-layer-for-seo); a11y deferral per [ADR 019](#adr-019-accessibility-deferred).

### Confidence
**High.** The navigation contract follows from G5/P6.2, and the SEO mechanism is now concretely enumerated by [ADR 020](#adr-020-metadata-only-dom-layer-for-seo), eliminating the prior "inferred from the Goal rather than an enumerated Solution requirement" risk.

---

## ADR 013: No WASM↔DOM Boundary in the Hot Path

### Context
The WASM↔JS/DOM boundary imposes per-call overhead unacceptable in the rendering hot path. Empirical evidence (P7.4) shows real WASM usage is coarse-grained, not per-UI-element [2,6]. A true "no interop boundary" architecture is achievable only if the scene graph stays entirely inside WASM.

### Decision
Adopt option (a): compile the UI itself to WASM so the layout module issues WebGPU draw calls directly; **no WASM↔DOM boundary in the hot path**. Rejected: (b) JS host with WASM escape hatch (retains boundary); (c) dual JS/WASM with DOM fallback (doubles maintenance).

### Status
Proposed.

### Consequences
**Hot-path definition (scope of this ADR):** the hot path comprises per-frame operations — layout, composition, draw-call emission, hit-testing, and input dispatch. These must run entirely inside WASM with no WASM↔DOM boundary crossing. Text rasterization/glyph-generation is hot-path if it occurs per-frame; text *measurement* is hot-path for layout (and is now fully in-WASM per [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)). Accessibility-tree mutation, navigation/state serialization, and SEO export are **non-hot-path**; the DOM surface for SEO is scoped to `<title>`/`<meta>` + static snapshot per [ADR 020](#adr-020-metadata-only-dom-layer-for-seo).

This decision is **conditional on [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)** (Composition/Document-Model residency): the scene graph must remain inside WASM end-to-end. If any scene-graph node escapes to JS, the no-boundary guarantee is voided and per-call overhead returns.

Resolved dependencies: accessibility is deferred per [ADR 019](#adr-019-accessibility-deferred) (no DOM-based bridge); text rendering uses the forked in-WASM HarfRust stack per [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (no DOM text surface — text measurement/rasterization remain hot-path and are now fully in-WASM, eliminating the prior boundary concern); adoption/interop DOM surface is restricted to `<title>`/`<meta>` + SEO snapshot per [ADR 020](#adr-020-metadata-only-dom-layer-for-seo). Threading uses main thread + on-demand WASM threads with socket IPC per [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc), preserving the no-DOM-boundary rule (socket IPC is WASM↔WASM, not DOM). Non-hot-path boundary crossings are now scoped by ADRs 019/020/022.

Cross-references: [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (compositor threading), [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language), [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (startup/binary budgets — note: any performance budgets referenced by interop escalation live in ADR 017, not here; this ADR states the structural rule), [ADR 019](#adr-019-accessibility-deferred) (a11y), [ADR 020](#adr-020-metadata-only-dom-layer-for-seo) (DOM surface), [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) (threading/IPC), [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (text stack).

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
- **Negative:** single large module raises streaming-compile cost; monolithic-binary debuggability degrades. Per [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) (on-demand WASM threads + socket IPC over SharedArrayBuffer) and [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (forked HarfRust in-WASM text stack), the binary now bundles a thread runtime, an IPC shim, and a shaping payload, sharpening the streaming-compile concern; sectioning/tree-shaking per [ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking) becomes load-bearing.
- **Trade-off:** Hot-path inlining ([ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path)) must be decided at compile time. Language choice ([ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm)) constrained to WASM-targeting toolchains. Dependency bundling ([ADR 018](#adr-018-capability-scoped-imports--component-model-tree-shaking)) must avoid defeating streaming compilation granularity. Binary budgets are enlarged by ADR 021 (thread runtime + IPC shim) and ADR 022 (HarfRust shaper).

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
- npm interop follows [ADR 020](#adr-020-metadata-only-dom-layer-for-seo) (no UI DOM interop; only `<title>`/`<meta>` + SEO snapshot) — curated adapters wrap legacy packages behind capability-scoped component boundaries rather than importing them directly, and there is no DOM-interop bridge surface.
- Adoption cost: capability instrumentation and component-model tooling are still maturing.

### Confidence
**Medium.** The tree-shaking guarantee tracks WASM component-model maturity (not yet production-hardened); capability sandboxing implementation cost is non-trivial and ecosystem tooling remains uneven.

---

## ADR 019: Defer Accessibility Bridge — No DOM Mirror

### Context
`Decision_Alternatives_accessibility-bridge.md` flagged P6.1 (canvas severs every native DOM a11y affordance) as co-decisive with P3.5 and recommended **Approach C** (hybrid: virtual tree + read-only DOM projection surface). The project owner has issued a non-negotiable directive that overrides that recommendation: the language runtime will ship without any DOM-based accessibility bridge, and accessibility is deferred to a later phase. P6.1's decisive-problem status is retained but its resolution is no longer a release blocker.

### Decision
Adopt **Approach A**: derive any future a11y-tree view from the render-object graph ([ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)) bridged directly to platform a11y APIs, with **no DOM mirror or DOM projection surface** in the runtime. Accessibility is explicitly deferred; no a11y bridge ships in this phase.

### Status
Proposed.

### Consequences
- **Positive:** unblocks the project; removes DOM coupling from the runtime hot path (consistent with [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path)); preserves [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)'s single-source-of-truth object model with no sync boundary.
- **Negative:** AT users have no a11y path until a later phase; P6.1's decisive-problem resolution is deferred, leaving the runtime non-conforming to web a11y contracts in the interim.
- **Cross-references:** [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (render-object tree remains the sole structural source for any future a11y derivation); [ADR 011](#adr-011-unified-virtual-focusaccessibility-annotation-layer) (focus model — its "DOM projection surface" clause is removed by this decision; only the focus-writer contract remains active); [ADR 010](#adr-010-cpu-bounding-volume-hit-testing--first-class-device-event-input) (input-dispatch-as-sole-focus-writer) is unaffected; [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (text a11y exposure now depends on this deferral, not on DOM contracts).

### Confidence
**High.** The project owner's choice is non-negotiable and explicitly overrides the Decision Alternative file's prior Approach C recommendation.

---

## ADR 020: Metadata-Only DOM Layer for SEO — No UI DOM Interop

### Context
P9.5 is strategic, not technical: ecosystem inertia dominates adoption, and the correct boundary between the WASM/WebGPU stack and the host DOM was unresolved (see [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md)). That file recommended Approach A — host-DOM interop bridges for text/a11y/navigation, time-boxed 18 months. The project owner has now made a definitive, non-negotiable choice that **overrides** that recommendation.

### Decision
Adopt Approach C, narrowed: a **thin DOM layer solely to set `<title>`, `<meta>` tags, and emit a static HTML snapshot for search-engine crawlers**. All UI rendering happens on GPU via WASM, with no DOM-tree interaction for layout, text, a11y, navigation, or input. No host-DOM interop bridges are provided.

### Status
Proposed.

### Consequences
- **Positive.** No bridge/marshalling tax; the hot path stays entirely inside WASM, aligning cleanly with [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path). A single binary/startup story is preserved without dual-stack payload pressure ([ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation)). Seam bugs at the DOM frontier are eliminated.
- **Negative.** No incremental adoption via the DOM: teams cannot embed the new stack inside existing pages. Loses the a11y, text-rendering, and navigation interop bridges Approach A relied on — these must now be solved entirely off-DOM (a11y deferred per [ADR 019](#adr-019-accessibility-deferred); text via [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack)).
- **Cross-references.** [ADR 012](#adr-012-navigationurl-contract-and-explicit-seo-scope) (the metadata-only DOM is the explicit SEO export surface); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path integrity preserved structurally); [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (single-WASM startup budget uncompromised); [ADR 019](#adr-019-accessibility-deferred) (deferred accessibility — no DOM a11y bridge); [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) (in-WASM text stack — no DOM text surface).

### Confidence
**High.** The project owner's non-negotiable choice, superseding the prior recommendation regardless of the strategic-adoption risks raised.

---

## ADR 021: Main Thread + On-Demand WASM Threads with Socket IPC

### Context
`Decision_Alternatives_concurrency-scheduling.md` recorded three candidate models (cooperative coroutines / preemptive actor threads / pure single-thread loop) for the WASM UI runtime scheduler and recommended Approach A (cooperative coroutines + retain-mode loop) pending a spike. The project owner has made a non-negotiable choice that resolves this without a spike: a **hybrid not present in the file's A/B/C options** — one main thread plus on-demand WASM threads with socket IPC.

### Decision
Adopt a **main thread + on-demand WASM threads** model. The main thread runs the retain-mode render loop (layout, rendering, hit-testing, input dispatch) and owns the GPUDevice per [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor). Additional WASM threads are spawned **on demand** for asynchronous tasks (asset decoding, compute, IO). Inter-process communication (IPC) between threads uses **WASM sockets** over `SharedArrayBuffer` (via `wasm-sockets` or a similar mechanism).

### Status
Proposed.

### Consequences
- **Positive:** the main thread stays deterministic for the render loop per [ADR 016](#adr-016-unified-author-owned-trace-with-split-determinism); on-demand threads handle async work without polluting the frame timeline; socket IPC is a structured, typed channel (better than ad-hoc shared-memory races).
- **Negative:** `SharedArrayBuffer` still requires COOP/COEP per [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (deployment constraint); on-demand thread spawn cost must be amortized; socket IPC adds a serialization surface to cross-thread data.
- **Cross-references:** [ADR 003](#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (compositor threading — the GPUDevice-owner render thread is the persistent main thread or a dedicated non-on-demand worker); [ADR 008](#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language exposes the thread/IPC primitives); [ADR 016](#adr-016-unified-author-owned-trace-with-split-determinism) (per-tick trace determinism preserved on the main thread); [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (binary now bundles thread runtime + IPC shim).

### Confidence
**High.** The owner's choice is non-negotiable and resolves the previously open P4.3 decision point without requiring the recommended spike.

---

## ADR 022: Forked HarfRust as the In-WASM Text Shaping/Rasterization Stack

### Context
The off-DOM text stack is P3.5's decisive hard problem: contractual shaping, BiDi, selection, IME, and a11y are guaranteed only on DOM text nodes, while `canvas.fillText` provides none. [`Decision_Alternatives_text-rendering`](Decision_Alternatives_text-rendering.md) recommended **Approach B (hidden DOM surface)** as the lowest-regret interim. The project owner has now made a non-negotiable choice that overrides that recommendation: commit to **Approach A (in-WASM text stack)**, naming **HarfRust** specifically and mandating a fork so updates apply independently of upstream release cadence.

### Decision
Adopt a **forked HarfRust** as the shaping and rasterization stack, running entirely inside WASM. The fork lives alongside the project (vendored in-repo) so shaping/rasterization fixes, platform patches, and BiDi/IME extensions can be applied independently of upstream.

### Status
Proposed.

### Consequences
- **Positive:** no DOM text dependency; shaping/rasterization stays in the WASM hot path per [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) and serves [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree)'s pure-object model (text as GPU-backed glyphs); the fork grants full control over timing and patches.
- **Negative:** the project must maintain a fork; BiDi segmentation, selection, and IME composition must still be built atop HarfRust; the WASM payload grows by the shaper (sharpening [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation)'s streaming-compile concern); a11y text exposure no longer inherits DOM contracts and now depends on [ADR 019](#adr-019-accessibility-deferred)'s deferred accessibility approach.
- **Cross-references:** [ADR 004](#adr-004-pluggable-constraint-solver-layout-with-mandatory-text-flow-measurement-contract) (layout's measurement contract consumes HarfRust output); [ADR 007](#adr-007-single-owned-render-object-tree-component--subtree) (object model); [ADR 013](#adr-013-no-wasmdom-boundary-in-the-hot-path) (hot-path); [ADR 017](#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (binary size); [ADR 019](#adr-019-accessibility-deferred) (deferred accessibility).

### Confidence
**High.** This is the project owner's definitive, non-negotiable choice, which settles the residual uncertainty that previously kept text rendering a Decision Alternative rather than a committed ADR.

---

## Resolved Decision Alternatives

The following four Decision Alternative files have been **resolved** by ADRs 019–022 above. They are retained for historical context but their "Recommended Approach" is **superseded** by the project owner's non-negotiable choices.

| File | Decision Point | Resolved By | Override Note |
|------|----------------|-------------|---------------|
| [`Decision_Alternatives_text-rendering.md`](Decision_Alternatives_text-rendering.md) | Text rendering strategy | [ADR 022](#adr-022-forked-harfrust-in-wasm-text-stack) | Forked HarfRust (Approach A) overrides the file's prior Approach B (hidden DOM) |
| [`Decision_Alternatives_concurrency-scheduling.md`](Decision_Alternatives_concurrency-scheduling.md) | Concurrency/scheduling model | [ADR 021](#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc) | Main thread + on-demand WASM threads + socket IPC (a new hybrid) overrides the file's prior Approach A (cooperative coroutines) |
| [`Decision_Alternatives_accessibility-bridge.md`](Decision_Alternatives_accessibility-bridge.md) | Accessibility bridge approach | [ADR 019](#adr-019-accessibility-deferred) | Approach A (no DOM mirror, a11y deferred) overrides the file's prior Approach C (hybrid DOM projection) |
| [`Decision_Alternatives_adoption-interop.md`](Decision_Alternatives_adoption-interop.md) | Adoption/interop strategy | [ADR 020](#adr-020-metadata-only-dom-layer-for-seo) | Approach C (DOM only for metadata/SEO) overrides the file's prior Approach A (host-DOM interop bridges) |

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

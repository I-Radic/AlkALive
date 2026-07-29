# AlkALive — Fine Draft: System Design Specification

**Version:** 1.0
**Date:** 2026-07-26
**Status:** Draft — ready for implementation review

**Derived from:** Rough Draft v1.0 (`docs/ROUGH_DRAFT.md`), 22 Architectural Decision Records (`docs/adr/ADR.md`, ADRs 001–022), and 4 resolved Decision Alternative files (all superseded by ADRs 019–022). No outstanding Decision Alternatives remain; all prior low-confidence decision points have been resolved by the project owner's non-negotiable choices. One open dependency (IME composition-event acquisition) is flagged in §5 and §9.

**Audience:** Senior engineering team preparing to implement the AlkALive runtime and UI framework.

---

## Table of Contents

1. [System Overview & Philosophy](#1-system-overview--philosophy)
2. [Module & Object Model](#2-module--object-model)
3. [Rendering Architecture](#3-rendering-architecture)
4. [Layout Engine](#4-layout-engine)
5. [Text Rendering Stack](#5-text-rendering-stack)
6. [Styling & Theming System](#6-styling--theming-system)
7. [Input Handling & Event Routing](#7-input-handling--event-routing)
8. [Concurrency & Threading Model](#8-concurrency--threading-model)
9. [DOM Interaction Layer](#9-dom-interaction-layer)
10. [Accessibility Strategy](#10-accessibility-strategy)
11. [Module & Application Lifecycle](#11-module--application-lifecycle)
12. [Inter-Solution Integration](#12-inter-solution-integration)

---

## 1. System Overview & Philosophy

AlkALive is a from-scratch application platform. Its premise, drawn from the Problem Catalog and the Rough Draft's nine Problem/Goal clusters, is that the browser's document-shaped, retained-mode pipeline — style recalc → layout → paint → composite, with the box as atomic unit — structurally resists application use, and that the WASM↔DOM boundary is a per-call tax no hot path can absorb (P1.1–P1.5, P3.1–P3.4, P7.4). The Rough Draft's goal is not to patch that pipeline but to bypass it: a statically-typed, module- and object-oriented language compiles to WebAssembly, and the module renders its UI directly to WebGPU, abandoning HTML, CSS, and the DOM as the rendering substrate.

### Core Philosophy

Three pillars govern the architecture:

1. **The render-object tree is the single source of truth**, owned end-to-end by the WASM module (ADR 007). A UI component *is* a render-object subtree, with no reconciler, hydration, or SSR/CSR divergence.
2. **No DOM participates in the hot path** (ADR 013). Layout, composition, draw-call emission, hit-testing, input dispatch, and per-frame text work stay inside WASM.
3. **The GPU draw call is the atomic unit** (ADR 001), expressed through an author-owned render-graph IR of passes, attachments, and an explicit occlusion-cull pass, replacing the box as the smallest renderable thing.

### Decision Backbone

Twenty-two ADRs (001–022) form the decision backbone, cited by number throughout this specification. All four previously-open Decision Alternatives — text rendering, concurrency/scheduling, the accessibility bridge, and adoption/interop — are now **resolved**: ADRs 019–022 supersede their prior file-level recommendations, leaving no low-confidence decision point open.

### Runtime Architecture Diagram

```
   ┌──────────────────────────────────────────────────────────┐
   │           AlkALive Source (.alk)  —  ADR 008 / 009          │
   │       modules · classes · first-class UI components        │
   └──────────────────────────┬───────────────────────────────┘
                              │ AOT compile  (ADR 017)
                              ▼
   ┌──────────────────────────────────────────────────────────┐
   │                 WASM Module  (single binary)               │
   │  ┌─────────────────────────────────────────────────────┐  │
   │  │  Render-Object Tree  ◂── single owner  (ADR 007)      │  │
   │  │  component = subtree · per-instance style (005)       │  │
   │  │  WGSL effects (006) · dirty-rect invalidation (002)   │  │
   │  └───┬───────────────┬───────────────┬───────────────┐  │  │
   │      │ layout        │ hit-test/input │ focus          │  │
   │      ▼ 004           ▼ 010            ▼ 011            │  │
   │  Constraint Solver   CPU BVH Mirror   Virtual focus     │  │
   │  + HarfRust text     + device events  annotation layer   │  │
   │   (ADR 022)          (first-class)    (input = writer)   │  │
   │      │                                                    │  │
   │      ▼                                                    │  │
   │  Render-Graph IR  ◂── atomic unit  (ADR 001)              │  │
   │  passes · draw calls · occlusion-cull                     │  │
   └──────┬──────────────────────────────────┬────────────────┘
          │ SharedArrayBuffer (COOP/COEP)      │ socket IPC  (ADR 021)
          ▼ ADR 003                             ▼
   ┌───────────────────────┐      ┌──────────────────────────────┐
   │  Render Thread          │      │  On-Demand WASM Workers       │
   │  owns GPUDevice         │      │  asset decode · compute · IO  │
   │  → WebGPU command stream│      │  async (never on frame path)  │
   └──────────┬─────────────┘      └──────────────────────────────┘
              ▼
      ┌─────────────────┐       ┌─────────────────────────────────┐
      │  WebGPU / GPU    │       │  Host DOM   (NON-hot-path)        │
      │  framebuffer     │       │  <title> · <meta> · SEO snapshot  │ ADR 020
      └─────────────────┘       │  no UI · no text · no input interop│
                                 │  a11y deferred                     │ ADR 019
                                 └─────────────────────────────────┘
```

### Key Invariants

Five invariants govern the whole and are non-negotiable:

1. **No WASM↔DOM boundary in the hot path** (ADR 013): per-frame layout, composition, draw, hit-testing, input, and text work stay inside WASM; the DOM is confined to non-hot-path metadata.
2. **A single owned render-object tree** (ADR 007): one WASM-resident structure feeds layout, render-graph emission, focus derivation, and any future a11y view — no second tree to reconcile.
3. **Main thread plus on-demand workers** (ADR 021): the main thread owns the render loop and GPUDevice; on-demand WASM threads handle async work over typed socket IPC and never pollute the frame timeline.
4. **Accessibility is deferred** (ADR 019): no DOM mirror or projection ships this phase; any future a11y tree is derived, not authored, from the render-object graph.
5. **The DOM is metadata-only** (ADR 020): `<title>`, `<meta>`, and a static SEO snapshot are the sole host-DOM surface, with no UI, text, a11y, navigation, or input interop.

Together these convert the philosophy into checkable structural rules: any choice that re-enters a DOM node into the hot path, duplicates the render-object tree, blocks the main thread with async work, ships an a11y DOM mirror, or widens the DOM surface beyond SEO metadata violates the architecture by construction.

---

## 2. Module & Object Model

AlkALive's object model is the language, not a library layered on one. Per ADR 008, the source language is statically typed, module- and object-oriented, and compiles to WebAssembly. Per ADR 009, correctness rests on a two-level guarantee: the compiler proves source-level soundness, and the WASM validator independently checks compiled well-formedness. This is deliberately *not* an end-to-end semantic proof — WASM validation is structural, and the architecture does not overclaim it; the two levels are separately meaningful and compose honestly.

### 2.1 The Render-Object Tree as the Single Object Model

ADR 007 collapses the framework/DOM dichotomy: there is one owned tree, and module objects *are* the render objects. A component is a render-object subtree that owns its styling, layout, and drawing — not a virtual tree reconciled into a host box tree. There is no reconciler, no hydration, and no SSR/CSR divergence, because there is no second tree to reconcile against. The render-object graph is the single source of truth, emitting to WebGPU draw calls, the virtual focus annotation layer, and any future a11y view as parallel targets.

### 2.2 Object Lifecycle

Construction, ownership, visibility, and destruction are module-controlled state transitions, replacing the DOM's globally-observable imperative mutation surface (`appendChild`, `innerHTML`, attribute setters — P3.4). A render object is constructed by its owning module, attached to a declared parent slot, made visible or hidden by owner decree, and destroyed when the owner drops it — each a typed operation, not a side effect on a shared document. Destruction is deterministic and single-owner: because the module owns the subtree, there is no GC-vs-host-lifecycle race and no zombie node retained by an external observer.

### 2.3 Encapsulation Primitives

Explicit ownership and visibility are language primitives, replacing the JavaScript pattern of simulating privacy via closures, `Symbol`s, and `WeakMap`s (P4.2). Each field declares an owner and a visibility scope; cross-module access requires a declared capability. Encapsulation is thus jointly verifiable with type soundness at the module boundary, rather than a runtime convention enforced by discipline.

### 2.4 Interface Contracts Between Modules

Modules compose through typed contracts (ADR 014). A component declares typed input properties, typed output events, and explicit named child slots — not implicit children discovered by selector. Composition is a typed binding, checkable at compile time:

```text
// descriptive pseudo-code, not real syntax
module Button {
  input  label:      Text
  input  intent:     Intent = .default
  slot   leading:    Glyph?          // explicit, named, optional
  output onActivate: Signal<Intent>
}
```

Slot declarations make parent–child topology part of the type: mounting a child into an undeclared slot is a compile-time error, eliminating the selector-drift fragility that DOM/CSS e2e tests inherit (P8.4).

### 2.5 Error Isolation at Module Boundaries

Error isolation is a structural consequence of the owned-subtree model (ADR 007) and whole-module replacement (ADR 015), not a separate committed feature. Because a module exclusively owns its subtree, a panic in a child subtree can be trapped at its owning module's boundary: the subtree is torn down deterministically, the parent receives a typed `Failure` value in the affected slot, and the rest of the tree is unaffected. No exception propagates across a shared document tree, and no half-mutated global state survives — the owned-subtree model makes isolation a structural property rather than a discipline. (The HMR mechanism in §11 uses the same boundary for whole-module replacement.)

---

## 3. Rendering Architecture

The rendering architecture replaces the browser's retained-mode box-model pipeline (style recalc → layout → paint → composite) with an author-owned, GPU-resident render loop whose per-frame cost is governed by draw calls and fill rate, not by scene-tree size.

### 3.1 Render-Graph IR (ADR 001)

The atomic rendering primitive is the **render-graph IR**: a directed graph of *passes*, *attachments*, *draw calls*, and a dedicated *occlusion-cull pass*. Authors declare the graph; the runtime compiles, reorders, batches, and inserts barriers into an optimal GPU command stream. **WebGPU is the initial backend**, with Vulkan/Metal as future native-backend options. The graph is decoupled from execution — declaration order need not equal submission order — which permits depth-aware and tile-based optimization that box paint order forbids.

### 3.2 Per-Module Dirty-Rect Invalidation (ADR 002)

Each module owns its scene graph and invalidates only dirty rectangles or per-object subsets, gated by a *layout-locality guarantee*: cross-module flex/percentage dependencies that would re-introduce global reflow are rejected at solve time or fall back to a documented global pass. Per-frame work is thus bounded by the dirty subset, directly addressing the reflow/repaint bug class.

### 3.3 Compositor Threading (ADR 003 + ADR 021)

WebGPU's `GPUDevice` and derived objects are agent-bound and cannot be shared across workers. A single render thread — **the persistent main thread** of ADR 021 — owns the lone `GPUDevice` and serializes every render-graph submission from all scene graphs (UI, particles, world, overlays). On-demand WASM worker threads never acquire the device; they feed immutable render-graph IR over `SharedArrayBuffer` and socket IPC, which the render thread merges, compiles, reorders, batches, and submits. The occlusion-cull pass executes on the render thread against a compositor-wide depth/visibility buffer. (ADR 003's fallback of a "dedicated non-on-demand worker" as GPUDevice owner is superseded by ADR 021's canonical main-thread model; both ADRs agree that on-demand workers never own the device.)

This requires COOP/COEP cross-origin isolation (`Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`); `credentialless` COEP or iframe proxying mitigate embedding conflicts.

### 3.4 No WASM↔DOM Boundary in the Hot Path (ADR 013)

The UI is compiled to WASM, so the module that computes layout issues WebGPU draw calls directly. The hot path — *layout, composition, draw-call emission, hit-testing, input dispatch, and text measurement/rasterization* — runs entirely inside WASM with no boundary crossing. Accessibility-tree mutation, navigation/state serialization, and SEO export are non-hot-path; the DOM surface is restricted to `<title>`/`<meta>` plus a static snapshot (ADR 020).

### 3.5 Frame Loop

The main thread runs a retain-mode render loop over explicit dirty rectangles and per-object layout; per-tick determinism (ADR 016) is preserved on the main thread while on-demand workers handle async tasks off the frame timeline.

### 3.6 Performance Budget

Sustained 60/120 fps is the target, governed purely by draw calls and fill rate, independent of scene-tree size (P7.1).

```text
// Render-loop interface — runs on the main thread (the GPUDevice-owner)
trait RenderLoop {
  fn tick(dt: f32, dirty: &[DirtyRect], input: &InputBatch) -> FrameResult;
  fn request_layout(scope: ModuleId) -> void;        // marks dirty subset; locality enforced by solver
  fn submit(graph: RenderGraphIR) -> SubmitHandle;   // enqueues for merge/compile/reorder/submit
  fn hit_test(point: Vec2) -> HitResult;             // in-WASM; no DOM crossing
  fn begin_pass(att: &Attachment) -> PassBuilder;     // device-owner-only; called on render thread
}
```

```text
// Render-graph node interface — immutable IR submitted to the compositor
struct RenderGraphIR {
  passes:        Vec<Pass>,
  attachments:   Vec<Attachment>,        // textures/buffers with lifetime = [producer, last consumer]
  draw_calls:    Vec<DrawCall>,          // pipeline + vertex/index/uniform bindings
  occlusion_cull: OcclusionCullPass,     // runs on render thread vs. shared depth/visibility buffer
  edges:         Vec<(PassId, PassId)>,  // barrier deps; runtime may reorder/batch respecting these
  source_module: ModuleId,               // locality tag; dirty-rect scoping keyed hereon
}
```

> **Open dependency:** A shared attachment-format and pass-boundary contract between the render-graph IR (§3) and the text stack's glyph-run IR (§5) is deferred to a future rendering-ABI ADR, as noted in ADR 001.

---

## 4. Layout Engine

The layout engine replaces the CSS box-tree pipeline (P2.3, P2.4) with a **pluggable constraint solver** operating over first-class render objects (ADR 004). Cassowary-style linear constraints, impulse/physics solvers, and directed-graph layout backends are interchangeable behind a single `Solver` trait; the author's module *is* the layout engine, so determinism is single-renderer by construction rather than an interoperability hope. The layout-tree is **solver-internal** — it is never re-derived from styles — eliminating the style-driven box-tree recalc that couples style mutation to global reflow. Solver outputs (per-object transforms, glyph runs, clip regions) are written directly into GPU-resident instance buffers consumed by the render-graph IR of §3; no intermediate layout-tree serialization crosses into the paint stage.

### 4.1 Mandatory Text-Flow Measurement Contract

Because box/physics/graph solvers cover none of line-breaking, BiDi reordering, or font-metric shaping, every solver — including user-supplied ones — must consume a synchronous **measured-run interface** backed by the forked in-WASM HarfRust stack of ADR 022. There is **no DOM text surface** anywhere in the measurement path (ADR 020, ADR 022); glyph metrics originate inside the WASM hot path and stay there. This contract guarantees that swapping the outer solver never destabilises text fragmentation (P2.4).

```text
trait Solver {
    // Register a render object; returns a constraint ID for later
    // removal during per-module dirty-rect invalidation (ADR 002).
    fn add_object(&mut self, obj: RenderObjectId) -> ConstraintId;
    fn bind_style(&mut self, obj: RenderObjectId, style: &OwnedStyle);

    // Solve the constrained subset, consuming measured text runs
    // synchronously. Emits GPU-ready transforms; Err on failure.
    fn solve(&mut self, dirty: &DirtySet, measured: &dyn MeasuredRun)
        -> Result<LayoutSolution, SolveError>;

    // Locality gate (ADR 002): reject cross-module flex/percentage
    // constraints that would re-introduce global reflow.
    fn assert_local(&self, c: &Constraint) -> Result<(), LocalityViolation>;
}

interface MeasuredRun {
    // Synchronous; backed by forked HarfRust (ADR 022). No DOM.
    // Returns glyph-run metrics: advances, ascents, descents, caret positions.
    shape_and_measure(run: TextRun, ctx: FontContext): GlyphMetrics;
    line_break(glyphs: GlyphRun[], max_width: f32): LineBreak[];
}
```

> **Shared boundary:** The `MeasuredRun` interface is the identical §4↔§5 contract. §4 (layout) consumes it; §5 (text stack) implements it, backed by HarfRust per ADR 022. Both sections describe it the same way: synchronous, backend-agnostic, HarfRust-backed, no DOM.

### 4.2 Layout-Locality Guarantee (ADR 002)

Each module owns its scene-graph subtree and invalidates only dirty rectangles; per-frame cost is bounded by the dirty subset, not tree size. Locality is a discipline the solver **enforces, not assumes**: cross-module flex baselines, percentage chains spanning module boundaries, or any constraint whose satisfaction would trigger reflow outside the dirty set are **rejected at solve time** via `assert_local`, never silently propagated. This closes the global-reflow bug class documented in [20, 21] and ADR 002.

### 4.3 Extension Points

User-defined constraint solvers implement `Solver`; pluggable backends (CPU SIMD, WASM-thread-parallel, experimental GPU-compute) bind behind the same trait, so swapping Cassowary for a spring-impulse solver is internal and non-breaking to downstream stages.

### 4.4 Error Handling

When a solver reports `Unsatisfiable`, the engine falls back to the **last-known-good layout commit** (retained per-module) and emits a structured diagnostic — offending constraint IDs, locality violations, and a suggested minimal relaxation — to the unified trace of §12. The frame renders against the cached layout rather than stalling or producing an inconsistent partial solve, preserving frame-rate stability while surfacing the failure for authoring-time correction.

---

## 5. Text Rendering Stack

Text is the runtime's most heavily depended-upon subsystem: layout's measure stage (§4), selection/caret interaction (§7), and any future a11y text exposure (§10) all rest on it. P3.5 identifies DOM-locked text semantics as the decisive hard problem — contractual shaping, BiDi, line breaking, selection, caret, IME, and accessibility are guaranteed only on DOM text nodes, and `canvas.fillText` provides none. **ADR 022** resolves this: a **forked HarfRust** shaping and rasterization stack running entirely inside WASM, with **no DOM text render path**. The fork is **vendored in-repo** so shaping/rasterization fixes, platform patches, and BiDi/IME extensions apply on the project's own cadence, independent of upstream HarfRust releases.

### 5.1 Shaping Pipeline

The pipeline runs end-to-end in WASM. HarfRust shapes a Unicode run into positioned glyph IDs and offsets; the result is a `ShapedRun` carrying glyph-run metrics (advance, ascent, descent, caret positions, cluster map). These metrics flow into ADR 004's mandatory text-flow measurement contract: the layout solver consumes the synchronous *measured-run* interface (§4.1) regardless of backend, so glyph metrics stay consistent across outer-solver swaps. Rasterization happens late and lazily — shaped glyphs are uploaded into a GPU glyph atlas and drawn as textured quads via the render-graph IR (§3). The shaping/rasterization boundary is the measurement contract: shaping produces metrics for layout; rasterization produces pixels for the compositor.

### 5.2 BiDi, Segmentation, Selection, Caret

Per ADR 022's negative consequence, **BiDi reordering, Unicode segmentation, hit-testing/selection, and caret movement must be built atop HarfRust** — they are no longer inherited from DOM contracts. These run entirely in WASM as part of the text stack, with no DOM render path and no `fillText` fallback.

### 5.3 IME Composition — Open Dependency

> **⚠ Open Dependency — IME composition-event acquisition.**
>
> The Rough Draft proposed a hidden `<input>` element to surface platform IME composition events (`compositionstart`/`update`/`end`). However, **ADR 020 explicitly forbids DOM-tree interaction for input** — the DOM surface is restricted to `<title>`/`<meta>` and SEO snapshots only. No ADR commits a replacement mechanism for acquiring IME composition events from the platform without a DOM input element.
>
> This is an unresolved dependency. Candidate approaches for a future ADR:
> - **(a)** Platform event APIs accessed directly from WASM (e.g., browser extension or future WASM interface); no DOM element involved.
> - **(b)** A narrowly-scoped exception to ADR 020 for a single hidden `<input>` that carries *composition state only* (no text rendering, no UI state); classified as non-hot-path.
> - **(c)** Defer IME support entirely until a platform-native WASM input API matures; ship without IME in the initial release.
>
> **Recommended (if forced):** (b), with the constraint that the hidden element carries composition state only and is explicitly excluded from the hot path. This mirrors the rough draft's design but requires a formal exception to ADR 020. Until this is resolved via a new ADR, the text stack exposes an `ime_compose` interface that accepts composition events from whatever acquisition mechanism is ultimately chosen.

### 5.4 Font Management & Glyph Atlas

Font loading and caching live in WASM: a font registry resolves families/weights, caches decoded font tables, and feeds HarfRust. Glyph atlas management lives on the GPU — an LRU tile atlas that uploads rasterized glyphs on demand and invalidates by dirty-rect locality (ADR 002). Metrics queries are served from cache without crossing the boundary.

### 5.5 Error Handling

- **Font load failure**: the registry returns a fallback resolution (next matching family, then a default fallback font); the run is re-shaped against the fallback so layout never blocks.
- **Shaping failure**: HarfRust returns `.notdef` glyph IDs for uncovered codepoints; the stack surfaces these as visible tofu with metrics, never as a pipeline abort.

### 5.6 A11y Text Exposure

Per ADR 019, accessibility is deferred — no DOM a11y contracts are inherited, and no a11y bridge ships in this phase. The text stack exposes a **placeholder a11y-text interface** (`expose_a11y_text`) that any future derivation layer can implement against the render-object graph. P6.1's decisive-problem resolution is deferred, not cancelled.

### 5.7 Performance

Per ADR 013, text *measurement* is hot-path for layout (per-frame, in-WASM); rasterization is hot-path *if* it occurs per-frame. Both must stay entirely in WASM with no WASM↔DOM boundary crossing — guaranteed structurally by the forked in-WASM HarfRust stack, since no DOM text surface exists. Atlas uploads are amortized: only newly-shaped glyphs cross the WASM→GPU boundary, and only when first seen or evicted.

### 5.8 Interface Sketch

```text
struct ShapedRun {
    glyph_ids:  &[u32],        // HarfRust-shaped glyph IDs
    advances:   &[f32],        // per-glyph x/y advance
    offsets:    &[(f32, f32)], // per-glyph baseline offset
    clusters:   &[u32],        // source-codepoint range per glyph
    caret_map:  ClusterMap,    // glyph index <-> caret offsets
    metrics:    RunMetrics,    // ascent, descent, total advance
    bidi_level: u8,
    font_id:    FontId,        // resolved font (fallback-aware)
}

trait TextStack {
    fn shape(&self, text: &str, style: &TextStyle, lang: Language)
        -> Result<ShapedRun, ShapeError>;
    fn measure(&self, run: &ShapedRun, max_width: f32) -> MeasuredLines;
    fn rasterize(&self, run: &ShapedRun, atlas: &mut GlyphAtlas)
        -> GlyphQuadBatch;
    fn hit_test(&self, run: &ShapedRun, point: (f32, f32)) -> CaretOffset;
    fn expose_a11y_text(&self, run: &ShapedRun) -> A11yTextPlaceholder;
    // IME: composition events routed in from the acquisition mechanism
    // chosen per §5.3 open dependency. No DOM text render path.
    fn ime_compose(&mut self, ev: CompositionEvent) -> ImeState;
}
```

---

## 6. Styling & Theming System

### 6.1 Owned Per-Instance Property State (ADR 005)

Styling is **per-instance object-owned property state**, bound at construction and addressable only via the owning render object. There is **no cascade, no CSSOM, no selector matching**, and no specificity comparator: every styled property is a typed field on the object itself. Style tables are compiled into the WASM module's binary data section as a compact binary blob — no stylesheet is parsed at runtime, no rule is matched, and no cascade is resolved per frame. Access is **O(1) local field read**.

### 6.2 Style Property Types

A `Style` is a trait over three closed categories of property, each with a typed representation:

- **Scalar** — `Color (u32 RGBA)`, `Opacity (f32 ∈ [0,1])`, `LineWidth (f32)`, …
- **Transform** — a 4×4 matrix (or SRT decomposition) consumed directly by the GPU transform upload.
- **Shader** — a WGSL program entry point plus a packed uniform buffer.

```text
trait Style {
    fn color(&self)     -> Color;          // default: transparent black
    fn opacity(&self)   -> Opacity;        // default: 1.0
    fn transform(&self) -> Mat4;           // default: identity
    fn effect(&self)    -> ShaderBinding;  // default: passthrough
}
```

### 6.3 WGSL as a First-Class Style Primitive (ADR 006)

WGSL shader programs and compute passes are **first-class styling primitives**. A `ShaderBinding` pairs a compiled WGSL module with a uniform buffer sourced from the owning object's fields; the render graph schedules it as an explicit paint or compute pass. This **replaces CSS's closed `filter` catalogue**: gradients, particles, per-vertex displacement, and compute-driven styling are authored rather than approximated. Particle systems and physics-driven vertex transforms become ordinary style values.

### 6.4 Theming

A **theme is a module exporting a set of named style presets** — construction-time token bundles, not a propagation system. Themes are not cascading scopes; they are construction-time dictionaries applied by explicit `set` calls:

```text
interface Theme {
    fn preset(name: &str) -> Style;
    fn default()          -> Style;
}
```

A render object receives its style either by looking up a named preset (`theme.preset("primary-button")`) or by accepting `theme.default()`. **There is no inheritance**: a property not explicitly set takes the type's default value, not a parent's value. Subtree consistency is the author's responsibility, expressed at construction rather than resolved at match time.

### 6.5 Extension Points

Two extension surfaces are first-class:

- **User-authored WGSL effects** — any module may ship a WGSL program and bind it as a `ShaderBinding`; the renderer treats built-in and user shaders uniformly.
- **Custom property types** — modules may declare new typed style fields (e.g. `SpringVelocity`) carried alongside the built-in scalars; layout solvers consume them as constraint inputs.

### 6.6 Error Handling

- **Shader compile failure** — the binding falls back to the default passthrough effect and emits a structured diagnostic into the trace; the object remains visible, never unrendered.
- **Invalid property values** — out-of-range scalars (opacity `1.7`, negative line width) are **clamped to their valid range** with a build/runtime warning; type-mismatched values are a compile error, never a silent coercion.

### 6.7 Performance Profile

All style access is **O(1) field reads** against binary-compiled state. There is **no runtime parse, no CSSOM construction, and no selector matching** — the pipeline cost CSS attributes to style recalc (P2.2) is structurally absent. WGSL effects compile once at module load and are cached as pipeline objects, so per-frame cost is uniform-bind, not recompile.

---

## 7. Input Handling & Event Routing

### 7.1 Design Basis

Input dispatch is part of the per-frame hot path (ADR 013) and runs entirely inside WASM on the main thread alongside layout, composition, and hit-testing (ADR 021). The GPU render graph (ADR 001) is the source of truth for scene geometry, but WASM cannot query GPU-resident buffers directly, so hit-testing runs against a **CPU-resident bounding-volume mirror** derived from the render-object tree (ADR 007) and refreshed each layout commit (ADR 010). Per-query GPU pick-buffer readback is reserved for *precise* picks (sub-pixel text caret, polygonal shapes) and never appears on the per-frame path.

### 7.2 Device-Event Model

Raw device state — pointer, stylus (pressure/tilt), multi-touch contact sets, gamepad axes/buttons, and keyboard — is surfaced as first-class typed events dispatched directly to the hit render object. Render objects own their own gesture/state machines; there is no central gesture recogniser. This dissolves the P5.2 second-class treatment of non-mouse inputs: every device class is structurally identical at the dispatch boundary.

### 7.3 Routing

There is no DOM-style capture/target/bubble propagation. Dispatch is a single direct call from the input scheduler to the hit object with a typed, owned event struct. When a gesture must track an object across frames (e.g. a drag), the hit object captures the stream itself by returning a *grab handle*; no implicit ancestor traversal exists.

### 7.4 Focus and Tab Order (ADR 011)

Focus, tab order, and the focus ring live on a **unified virtual focus annotation layer** — a cached, invalidation-driven derived view over the render-object graph, not a separate tree. **Input dispatch is the sole writer** of focus state; **focus-ring rendering is the sole active reader**. Accessibility-tree derivation and AT announcement are deferred per ADR 019 and are not on the input critical path. The object receiving input is therefore the same object that owns the focus annotation — eliminating the DOM/canvas focus blackout [39,40,41].

> **Shared boundary with §10:** The focus annotation layer is the shared §7↔§10 surface. §7 (input) writes focus state; §10 (accessibility) would read it for AT announcement — but that read is deferred per ADR 019. No DOM projection surface exists in this phase. Both sections agree on this contract.

### 7.5 Error Handling

Invalid input state (stale contacts, orphaned grabs after object removal, mismatched touch begin/end) is normalised at the scheduler boundary: orphaned grabs are cancelled with a synthetic `Cancel`, and out-of-range device IDs are dropped and logged. Gesture conflicts are resolved at the *object* level: when two hit objects claim overlapping grabs, the most recently issued explicit grab wins and the loser receives a `Cancel`; the scheduler never arbitrates semantic intent.

### 7.6 Interface Sketch

```text
struct InputBatch {
  pointers:   Array<PointerSample>     // mouse/stylus/touch, with pressure/tilt
  keys:       Array<KeyEvent>          // pressed/released + modifiers
  gamepad:    Array<GamepadSample>     // axes + buttons
  timestamp:  MonotonicNs
}

struct HitResult {
  object:   Handle<RenderObject>       // ADR 007 object
  point:    Vec2                       // local-space hit
  precise:  Bool                       // true ⇒ GPU pick-buffer readback was used
  depth:    Float                      // for ordering overlapping hits
}

interface FocusManager {
  fn hit_test(batch: InputBatch) -> Array<HitResult>      // CPU mirror; precise picks lazy
  fn dispatch(batch: InputBatch, hits: Array<HitResult>)  // direct, no bubble
  fn set_focus(target: Handle<RenderObject>)              // sole writer (ADR 011)
  fn current_focus() -> Option<Handle<RenderObject>>      // sole active reader path
  fn tab_next() / fn tab_prev()                           // virtual tab order
  fn invalidate(scope: LayoutScope)                       // on render-object mutation
}
```

---

## 8. Concurrency & Threading Model

### 8.1 Problem

The browser's main thread owns the DOM; Web Workers cannot touch it and pay a structured-clone tax per interaction, so off-main-thread UI work rarely earns its cost (P7.2). WebGPU compounds this: `GPUDevice` and derived objects are agent-bound, not shareable across workers. The catalog also indicts the callback/Promise/timer substrate as a structural async-bug source (P4.3). The concurrency model must therefore (a) preserve a deterministic main-thread frame timeline for trace correlation, (b) provide real off-main-thread parallelism for asset decode, compute, and IO, and (c) avoid reproducing the DOM serialization tax or the timer/Promise hazard class.

### 8.2 Solution (ADR 021)

The runtime is a **main thread + on-demand WASM threads** hybrid. The main thread owns the `GPUDevice` (ADR 003), runs the retain-mode render loop — layout, render-graph submission, hit-testing, input dispatch — and stays on the frame timeline so ADR 016's unified trace can correlate logic→layout→draw per tick. Workers spawn on demand for asset decode, compute, and IO; they never acquire the device and never mutate render-path state. They emit **immutable render-graph IR** (ADR 001) into a `SharedArrayBuffer` and signal the main thread via a socket channel; the main thread merges, compiles, reorders, batches, and submits.

### 8.3 GPUDevice Ownership — Canonical Reconciliation

ADR 003 permits the GPUDevice-owner to be "the main thread or a dedicated non-on-demand worker." ADR 021 commits to the **main thread as canonical**. This Fine Draft adopts ADR 021's main-thread model as the authoritative choice; ADR 003's dedicated-worker fallback is noted but considered superseded by ADR 021 for this design. On-demand workers never own the device in either case — both ADRs agree on that.

### 8.4 IPC via WASM Sockets

Cross-thread communication uses **WASM sockets** over `SharedArrayBuffer` (via `wasm-sockets` or a similar mechanism) — typed, serialised channels with backpressure. No render-path object is simultaneously mutable from two threads; workers emit immutable IR and the main thread is the sole mutator of GPU state.

### 8.5 COOP/COEP Requirement

COOP/COEP cross-origin isolation (`COOP: same-origin`, `COEP: require-corp`) is mandatory for `SharedArrayBuffer`. This conflicts with third-party iframe embedding; mitigations are `credentialless` COEP or iframe proxying, with fallback to per-graph separate devices (losing the shared compositor) if neither is workable.

### 8.6 Thread Safety & Error Handling

A worker panic is isolated: the handle resolves to `Err(Panic)`, the pool reaps the worker, and the main thread's frame timeline is unaffected. A `ChannelError` (corrupt framing, closed peer, SAB underrun) propagates to the task's `Result`; the render loop never blocks on a channel — it drops stale IR and proceeds, preserving frame cadence over worker liveness.

```text
interface ThreadSpawner {
  spawn<T>(task: Fn(SharedState) -> Result<T, Panic>) -> Handle<T>;
  pool_size_hint(): usize;                 // advisory; grows on demand
}

interface SocketChannel<T: Serial> {
  send(msg: T): Result<(), ChannelError>;
  recv(): Result<T, ChannelError>;         // backpressure-aware
  // backed by SAB ring buffer + Atomics.notify/wait; never GPUDevice-aware
}
```

---

## 9. DOM Interaction Layer

### 9.1 Problem

The browser DOM couples rendering, text, accessibility, navigation, and SEO into one mutable tree. Canvas/WebGPU renderers inherit none of these affordances, yet crawlers, link-preview agents, and host shells still address a DOM. The design question is *how much* DOM surface the runtime must retain to satisfy crawlers and host navigation without reintroducing the per-call interop tax (P7.4) or the layout/paint-coupled bug class (P7.3).

### 9.2 Solution (ADR 020)

The runtime exposes a **metadata-only DOM layer**: a thin surface solely for `<title>`, `<meta>` tags, and a static HTML snapshot. **No text, accessibility, navigation, or input bridge exists** — those concerns are deferred (a11y, ADR 019) or solved off-DOM (text, ADR 022; navigation/state, ADR 012). SEO snapshots are emitted **at build time** from declared routes and serialisable state, or served **on-demand** to detected crawler user-agents; the runtime performs **no DOM mutation for UI**.

### 9.3 Navigation/URL Contract (ADR 012)

Navigation is a structured contract: the app exposes declared routes plus serialisable state to the host, and the DOM `<title>`/`<meta>` pair is the *explicit, sole* SEO export surface. The DOM surface is the concrete `<title>`/`<meta>` + static snapshot per ADR 020; a11y is deferred per ADR 019.

### 9.4 Hot-Path Boundary (ADR 013)

DOM interaction is **strictly non-hot-path**; no boundary crossing occurs in layout, composition, draw, hit-test, input, or text measurement. The `DomBridge` interface (below) exposes no hot-path verbs — by construction.

### 9.5 IME — No Exception

> The Rough Draft proposed a hidden `<input>` for IME composition events. **ADR 020 forbids DOM input interop**, so no IME exception is granted in this layer. IME composition-event acquisition is an **open dependency** owned by §5 (Text Rendering Stack). The `DomBridge` interface below intentionally exposes no IME method.

### 9.6 Error Handling

Any DOM API failure (snapshot write, meta mutation) is logged; the runtime degrades gracefully — the build-time snapshot is still served to crawlers, and the GPU render loop is unaffected. DOM failure never blocks the WASM render thread.

```text
interface DomBridge {
  // SEO export surface — sole UI-relevant DOM writes (ADR 020)
  setTitle(text: String): Result<void, DomError>
  setMeta(name: String, content: String): Result<void, DomError>
  serveSnapshot(route: Route, state: SerialisableState): Result<Html, DomError>

  // Navigation/state contract (ADR 012) — host-facing, non-hot-path
  declareRoutes(routes: List<Route>): Result<void, DomError>
  serializeState(): Result<SerialisableState, DomError>
}
// No methods for layout, draw, hit-test, text-measurement, a11y, or IME.
```

---

## 10. Accessibility Strategy

### 10.1 Decision Status (ADR 019)

Accessibility is explicitly **deferred** for this phase of the runtime. No DOM mirror, no DOM projection surface, and no assistive-technology (AT) bridge ship in the initial release. This is a deferral, not a cancellation: P6.1's status as a co-decisive hard problem (alongside P3.5 text) is retained — only its resolution is removed from the release-blocking critical path, by owner directive overriding the Decision Alternative's prior Approach C recommendation.

### 10.2 What Stays Active

The focus half of ADR 011's unified annotation layer remains in force: input dispatch is the sole writer of focus state (ADR 010), and focus-ring rendering continues to read focus annotations directly from the cached annotation layer. Only the AT-announcement half is deferred. This keeps the §7↔§10 contract's writer discipline intact so that, when a11y is un-deferred, the reader can be attached without re-architecting focus.

### 10.3 Placeholder for Future Extension

The render-object graph (ADR 007) already carries, as mandatory fields, the semantic role, structured data, and interaction descriptor metadata that ADR 011's annotation layer was designed to derive from. A future a11y-tree derivation can consume this metadata directly — no architectural upheaval, no sync boundary, no separately authored mirror. The text stack (ADR 022) likewise exposes a placeholder `expose_a11y_text` interface so that shaped glyph runs, BiDi-segmented text, selection, caret state, and labels can flow into the future tree without re-engineering the shaper.

### 10.4 Extension Path

When a11y is un-deferred, a virtual accessibility tree will be **derived** (not authored) from the render-object graph and bridged directly to platform accessibility APIs — the original Approach A from the superseded `Decision_Alternatives_accessibility-bridge.md`. No DOM is reintroduced; the bridge targets native platform a11y APIs, not browser DOM contracts.

```text
interface A11yPlaceholder {
    // Future extension point — no implementation ships this phase.
    // Consumed by a derived virtual a11y tree once ADR 019 is un-deferred.
    derive_a11y_node(from: RenderObject) -> Option<A11yNode>
    expose_a11y_text(run: ShapedGlyphRun) -> TextLabel

    // Render-object metadata already present (ADR 011 annotation layer):
    //   role:            SemanticRole          // mandatory
    //   structured_data: StructuredData        // mandatory
    //   interaction:     InteractionDescriptor // mandatory
    // Focus state held on the annotation layer, not on render objects.
}
```

### 10.5 Risk Acknowledgment

Until a later phase ships, AT users have no a11y path: the runtime is non-conforming to web a11y contracts (WAI-ARIA, the accessible-name computation, focus-management expectations) in the interim. This is an explicit, owner-approved risk accepted to unblock the runtime — not an overlooked gap.

---

## 11. Module & Application Lifecycle

### 11.1 Startup (ADR 017)

Startup collapses to a single streaming-decoded WASM module. Because the module *is* the layout engine — there is no separate framework runtime to bootstrap — startup reduces to one module load rather than three-language text parsing plus framework init. The compiled binary is **streaming-compiled while downloading**: decode begins as bytes arrive, and the layout/render pipeline is constructed concurrently. WebGPU shader pipelines are **precompiled asynchronously**, overlapping with module decode so the GPU-side startup floor is removed rather than deferred to first frame. ADR 021's thread runtime and ADR 022's HarfRust shaping payload enlarge the binary; ADR 018's compile-time tree-shaking and sectioning are therefore load-bearing for keeping decode sub-frame.

### 11.2 Hot Module Replacement (ADR 015)

HMR treats module state as **serialisable scene-graph data**. On reload, the outgoing module's state is captured, a fresh module is loaded, and the state is rehydrated — whole-module replacement, not surgical DOM patching. This **decouples code replacement from state preservation**: the scene graph is owned, serialisable state (ADR 007), so the contract between old and new module is a data round-trip, not a behavioural diff. Replacement granularity is one freshly loaded module (ADR 008).

> **Scope boundary with §2:** §2 covers the module-replacement *granularity* (a component subtree is the unit of error isolation); §11 covers the *rehydration mechanism* (state capture → fresh module → rehydrate). Together they define the full HMR contract.

### 11.3 State Preservation

Every hot-reloadable module must expose owned, serialisable scene-graph state (ADR 007). Modules that cannot serialise their state fall back to **full reload** (ADR 015 option c) — the contract is opt-in per module, never silently violated.

### 11.4 Design-Tool-as-Runtime (ADR 014)

The deterministic author-owned renderer is **embedded directly into the design tool**: the same WASM module runs in design and in production, so layout parity holds by construction (WASM sandbox), and rendering parity is bounded to a raster class via a deterministic GPU path with a software-rasteriser fallback. There is no design-to-runtime handoff gap.

### 11.5 Dependency Loading (ADR 018)

Imports are **capability-scoped** and typed: each dependency receives a least-privilege grant auditable per module, replacing npm's transitive trust. Compile-time tree-shaking is component-model-enforced — unused exports are eliminated as a guarantee, not a tooling hope. A language-level standard library shrinks the transitive dependency surface that bloats bundles and expands attack surface.

### 11.6 Lifecycle States

Each module transitions through: `Unloaded → Loading → Ready → Active → Suspended → Destroyed`. `Loading` covers streaming decode + pipeline precompilation; `Ready` awaits first frame; `Active` runs the retain-mode loop; `Suspended` halts scheduling while retaining GPU/scene state; `Destroyed` releases all resources.

### 11.7 Error Handling

- **Module load failure** — decode/validation error surfaces to host; module stays `Unloaded`, dependent modules abort with a typed `LoadError`.
- **HMR rehydration failure** — schema mismatch or deserialisation error aborts rehydration and **falls back to full reload** (ADR 015 option c); state is lost but the runtime recovers.
- **Pipeline precompilation failure** — shader compile error degrades to runtime compilation with a typed `PipelineCompileError`; the frame is not blocked, first paint is delayed.

```text
interface LifecycleManager {
  state: ModuleState  // Unloaded|Loading|Ready|Active|Suspended|Destroyed
  load(url: Url, caps: CapabilitySet): Promise<Ready>
  activate(): Active
  suspend(): Suspended
  resume(): Active
  destroy(): Destroyed
  onStateChange(handler: (ModuleState) -> void)
}

interface HMR {
  reload(moduleId: ModuleId, newUrl: Url): Promise<HMRResult>
  captureState(moduleId): SerialisableSceneGraph | null
  rehydrate(freshModule: Module, state: SerialisableSceneGraph): RehydrateResult
}
// HMRResult := { ok: Rehydrated } | { ok: FullReload, stateLost: true } | { err: LoadError }
```

---

## 12. Inter-Solution Integration

### 12.1 The Render-Object Tree as the Single Integration Surface

The architecture admits exactly one integration surface: **the owned render-object tree** (ADR 007), in which module objects *are* render objects and a UI component *is* a subtree owning its style, layout, and drawing. Every subsystem — text, layout, render-graph emission, input, accessibility, tooling — reads from or writes to this tree through statically-typed interfaces defined by the language (ADR 008). There is no second reconciled tree, no DOM mirror in the hot path (ADR 013), and no implicit publication channel; cross-subsystem data flow is always a typed emit/consume over the tree or over an explicitly declared interface contract.

### 12.2 The Frame Loop as the Integration Heartbeat

The main thread runs the retain-mode frame loop (ADR 021) and is the single deterministic driver. Each tick executes a fixed, ordered pipeline: **font-resolved layout (ADR 004) → render-graph IR compilation (ADR 001) → compositor commit (ADR 003) → WebGPU command submission.** The main thread owns the `GPUDevice`; on-demand WASM threads handle only asynchronous, non-frame-critical work — asset decode, compute, IO — and rejoin the loop by posting typed results back over **WASM sockets on a `SharedArrayBuffer`** (ADR 021). Because socket IPC is WASM↔WASM, the no-DOM-boundary rule of ADR 013 is preserved. The frame loop is also the trace boundary: every stage opens and closes a span, so the timeline is always frame-aligned.

### 12.3 Shared and Cross-Cutting Services

A single **author-owned trace** (ADR 016) spans logic, layout, and draw, so any dropped frame is root-causable on one correlated timeline rather than across disjoint DevTools panels. Determinism is *split*: layout determinism is guaranteed by the WASM sandbox (ADR 004/008); rendering determinism requires a **deterministic GPU path with a software-rasteriser fallback** used for verification and CI. Logging and profiling are not separate subsystems — they are views over the same trace, with a frame-budget watchdog flagging overruns. Error propagation follows **module-boundary isolation** (ADR 007/008): a failing component emits a typed `Result` across its interface; the runtime quarantines the subtree and its invalidation stays bounded to the per-module dirty rect (ADR 002), so one module's panic never invalidates the whole tree.

### 12.4 Subsystem Dependency Graph

Data flows one direction down the pipeline; the trace observes every stage; input and async workers feed in from the side. The dependency order is strict: **text feeds layout measurement; layout feeds render-graph transforms; input feeds focus annotations; all feed the unified trace.** Accessibility (ADR 019) is a *derived* view of the same tree, never a co-authored mirror, so it cannot drift.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                UNIFIED AUTHOR-OWNED TRACE   (ADR 016)                        │
│      single correlated timeline — logic + layout + draw spans                │
└────────────────────────────▲───▲───▲───▲───────────────────────────────────┘
                             │   │   │   │  per-stage spans (text/layout/graph/draw)
┌──────────────┐  ┌────────────┐  ┌──────────────┐  ┌────────────┐  ┌──────────┐
│  Text Stack  │─►│   Layout   │─►│ Render-Graph │─►│ Compositor │─►│  WebGPU  │
│ (ADR 022     │  │ (ADR 004)  │  │ IR (ADR 001) │  │ (ADR 003)  │  │          │
│  HarfRust)   │  │  solver    │  │              │  │single GPUDev│  │          │
└──────────────┘  └─────┬──────┘  └──────────────┘  └─────┬──────┘  └──────────┘
                       ▲ measure / transforms           ▲ compiled graph

   Input ─► Focus annotations (ADR 011) ─► A11y derived view (ADR 019): reads the tree

   On-demand WASM workers (ADR 021): asset decode · compute · IO
        └─ async results via socket IPC over SharedArrayBuffer ─► main thread
```

### 12.5 Cross-Cutting Service Catalogue

| Service | Scope | Owning ADR | Mechanism |
|---|---|---|---|
| Unified trace | logic + layout + draw | 016 | single author-owned timeline; layout determinism via WASM sandbox; draw determinism via deterministic GPU path + software-rasteriser fallback |
| Logging | all subsystems | 016 | structured spans into the same trace; no separate log sink |
| Profiling | per-frame, per-stage | 016 | trace spans + frame-budget watchdog; one author-owned profiler |
| Error propagation | module boundaries | 007 / 008 | typed `Result` channels; failing subtree quarantined to its dirty rect (002) |
| Determinism | layout / draw | 016 / 004 | sandboxed solver (layout); deterministic GPU path or software raster (draw) |
| Capability scoping | import surface | 018 | capability-scoped imports gate subsystem API access |
| Worker IPC | worker ↔ main | 021 | WASM sockets over `SharedArrayBuffer`; typed channels, WASM↔WASM |
| Binary / startup | whole module | 017 | streaming-compiled WASM + WebGPU pipeline precompilation |

### 12.6 Performance Budgets

The integration is bounded by two frame-rate targets — **60 fps (16.7 ms) and 120 fps (8.3 ms)** — against which every stage's trace span is policed. Draw-call count and fill-rate are the governed GPU-side budgets, enforced by the render-graph compiler's batching and occlusion-cull pass (ADR 001). On the asset side, a single binary-size budget covers the **forked HarfRust shaping stack (ADR 022), the thread runtime, and the IPC shim (ADR 021)**, all bundled into one streaming-compiled WASM module with WebGPU pipeline precompilation (ADR 017); sectioning and capability-scoped tree-shaking (ADR 018) keep that module within its streaming-decode budget. Any subsystem that would breach a budget surfaces the breach as a trace span, keeping the integration observable end to end.

### 12.7 Open Dependencies

The following cross-section dependencies are flagged for future ADR resolution:

1. **Rendering-ABI contract** (§3 ↔ §5): the shared attachment-format and pass-boundary contract between the render-graph IR and the text stack's glyph-run IR is deferred to a future rendering-ABI ADR (per ADR 001).
2. **IME composition-event acquisition** (§5 ↔ §9): ADR 020 forbids DOM input interop, pre-empting the hidden-`<input>` approach. A new ADR is needed to choose between platform-event APIs, a scoped ADR-020 exception, or deferral (see §5.3).
3. **GPU determinism fallback** (§3 ↔ §11 ↔ §12): the software-rasteriser fallback for cross-vendor rendering determinism (ADR 016) is an open implementation risk; its viability determines whether the unified trace and design-tool parity guarantees hold universally.

---

*End of Fine Draft. This document is the definitive architectural blueprint for the AlkALive runtime and UI framework, synthesized from the Rough Draft and 22 ADRs. Implementation teams should begin from §2 (Module & Object Model) and §3 (Rendering Architecture) as the foundational layers, with §5 (Text Rendering Stack) and §8 (Concurrency) as the critical-path subsystems.*

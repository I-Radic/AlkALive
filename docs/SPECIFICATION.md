# AlkALive — Detailed Software Specification

**Version:** 1.0
**Date:** 2026-07-26
**Status:** Implementation-ready

**Derived from:**
- `docs/PROBLEM_CATALOG.md` v1.0 — 50 peer-reviewed references, 45 problem entries (P1.1–P10.4)
- `docs/ROUGH_DRAFT.md` v1.0 — Problem/Goal/Solution/Integration per cluster
- `docs/FINE_DRAFT.md` v1.0 — 12-section system design blueprint
- `docs/adr/ADR.md` — 22 Architectural Decision Records (ADR 001–022), all Status: Proposed
- `docs/adr/Decision_Alternatives_*.md` — 4 files, all RESOLVED (superseded by ADRs 019–022)

**Outstanding items:** 1 open dependency (IME composition-event acquisition, see `Spec_Tradeoff_Note_IME.md`). All 4 Decision Alternatives are resolved; no tentative alternatives remain unmarked.

**Audience:** Senior engineering team implementing the AlkALive runtime and UI framework.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Language Specification](#2-language-specification)
3. [Runtime Architecture](#3-runtime-architecture)
4. [Rendering Engine](#4-rendering-engine)
5. [Layout System](#5-layout-system)
6. [Text Rendering](#6-text-rendering)
7. [Styling & Theming](#7-styling--theming)
8. [Input & Event System](#8-input--event-system)
9. [DOM Interop Layer](#9-dom-interop-layer)
10. [Accessibility Placeholder](#10-accessibility-placeholder)
11. [Concurrency & IPC](#11-concurrency--ipc)
12. [Performance & Resource Budgets](#12-performance--resource-budgets)
13. [Error Handling & Resilience](#13-error-handling--resilience)
14. [Testing & Simulation](#14-testing--simulation)
- [Glossary](#glossary)
- [Source Artifacts](#source-artifacts)

---

## 1. System Overview

### 1.1 Purpose

AlkALive is a from-scratch application platform: a statically-typed, module- and object-oriented language (`.alk`) compiles AOT to a single WebAssembly binary whose runtime renders UI directly to WebGPU, abandoning HTML, CSS, and the DOM as the rendering substrate. This section fixes the subsystem topology, design drivers, non-negotiable constraints, the five checkable invariants, and the subsystem-to-ADR ownership map referenced by every later section.

### 1.2 High-Level Architecture

```
.alk source (ADR 008)
   │  AOT compile (ADR 017)
   ▼
WASM module (single binary)
   ├── Render-Object Tree (ADR 007) — single owner
   │      │ per-instance style (005) · WGSL effects (006)
   │      │ dirty-rect invalidation (002)
   │      ├──► Layout: Constraint Solver + HarfRust (004 / 022)
   │      ├──► Hit-test/Input: CPU BVH + device events (010)
   │      └──► Focus: virtual annotation layer (011)
   │            ▼
   ├── Render-Graph IR: passes · attachments · occlusion-cull (001)
   └── Socket IPC / SharedArrayBuffer (ADR 021)
                │                              │
                ▼                              ▼
   Main Thread (GPUDevice owner, 003)    On-Demand WASM Workers
      merge / compile / reorder /         (asset decode, compute, IO —
      submit → WebGPU framebuffer          never on the frame path)
                                         Host DOM (NON-hot-path, 020):
                                         <title> · <meta> · SEO snapshot
                                         (a11y deferred — ADR 019)
```

Per-frame data flow is unidirectional: render-object mutation → layout solve → render-graph IR emission → compositor merge/compile → WebGPU submit. Input flows in reverse via hit-test → focus writer → handler. Cross-thread traffic is exclusively WASM↔WASM over socket IPC; no DOM crossing appears on any frame path.

### 1.3 Key Design Drivers

| Driver | Catalog | Architectural Response |
|---|---|---|
| Box is atomic; no author draw call | P1.1, P1.2 | Render-graph IR with explicit passes/attachments/occlusion-cull (ADR 001) |
| Global reflow cascade | P1.3 | Per-module dirty-rect invalidation, layout-locality gate (ADR 002) |
| Selector-matching / CSSOM cost | P1.4 | Object-owned per-instance styling, no cascade, no CSSOM (ADR 005) |
| Single universal render tree | P1.5 | Multiple render graphs composed by one GPUDevice owner (ADR 003) |
| Document vs application mismatch | P3.1, P3.2 | Module objects *are* render objects; one owned tree (ADR 007) |
| DOM-size jank | P3.3 | GPU-resident instancing; per-frame cost independent of tree size |
| Imperative mutation fragility | P3.4 | Typed ownership/visibility; deterministic lifecycle transitions |
| WASM↔DOM per-call tax | P7.4 | UI compiled to WASM; hot path never crosses the boundary (ADR 013) |

### 1.4 Constraints

The architecture is bounded by five non-negotiable constraints, each owning an ADR: (1) **no DOM in the hot path** — per-frame layout, composition, draw-call emission, hit-testing, input dispatch, and text measure/raster stay in WASM (ADR 013, backed by ADR 022 for in-WASM text); (2) **a single owned render-object tree** with no reconciler, hydration, or SSR/CSR divergence (ADR 007); (3) **main thread + on-demand workers** — the main thread owns the render loop and lone `GPUDevice`; workers handle only async work over typed socket IPC on `SharedArrayBuffer`, requiring COOP/COEP cross-origin isolation (ADRs 021, 003); (4) **accessibility deferred** — no DOM mirror ships this phase; future a11y is derived from the render-object graph (ADR 019); (5) **DOM is metadata-only** — host-DOM surface is `<title>`, `<meta>`, and a static SEO snapshot, with no UI/text/a11y/navigation/input interop (ADRs 020, 012).

### 1.5 Key Invariants (Checkable Rules)

Each invariant is a falsifiable structural rule; violation is an architectural defect, not a style preference.

1. **INV-1 (No hot-path DOM crossing).** No function on the per-frame call graph (layout, solve, draw-call emit, hit-test, input dispatch, text measure/rasterize) may invoke a DOM/JS-boundary export. *Check:* static call-graph analysis of the WASM export table; any `externref`/import invocation reachable from `tick()` is a violation.
2. **INV-2 (Single render-object tree).** Exactly one rooted tree exists; layout, render-graph emission, focus, and any future a11y derivation all consume the same node identity. *Check:* no second root node is constructible; every consumer receives `&RenderNode` references, never a serialized projection.
3. **INV-3 (Main thread owns GPUDevice; workers off-frame).** `GPUDevice` acquisition occurs on exactly one agent (the main thread); on-demand workers may not call `requestDevice` or `queue.submit`. *Check:* those call sites are statically confined to the main-thread module; worker IPC channels carry only immutable IR.
4. **INV-4 (No authored a11y DOM mirror).** The runtime exports no a11y tree to the DOM this phase; any a11y derivation is a future, derived, read-only projection from the render-object graph. *Check:* no a11y-tree construction path is reachable from the runtime entry point; the module is absent from the binary or feature-gated off.
5. **INV-5 (DOM surface = metadata set).** Host-DOM writes are confined to `{<title>, <meta>, <link rel=canonical>, static SEO snapshot}`; no DOM node participates in layout, text, input, or navigation. *Check:* the DOM-binding module's exported surface is exactly that set; any additional DOM API binding is a violation.

### 1.6 Subsystem Dependency Table

| Subsystem | Owning ADR | Key Interface |
|---|---|---|
| Language & Compiler | 008, 009, 017 | `.alk` → validated WASM binary; two-level type verification |
| Render-Object Tree | 007 | `RenderNode` owned subtree; component = subtree, typed slots |
| Styling | 005, 006 | Per-instance owned style fields; WGSL shader uniforms |
| Layout Engine | 004 | `Solver` trait; synchronous `MeasuredRun` contract (HarfRust-fed) |
| Text Stack | 022 | Forked HarfRust; glyph-run IR to compositor |
| Invalidation | 002 | `DirtyRect` per `ModuleId`; `assert_local` locality gate |
| Render-Graph IR | 001 | `RenderGraphIR{passes, attachments, draw_calls, occlusion_cull, edges, source_module}` |
| Compositor / GPUDevice | 003, 021 | Single GPUDevice owner; merge / compile / reorder / submit |
| Hit-Test & Input | 010 | CPU BVH mirror; first-class device-event input |
| Focus | 011 | Virtual focus annotation layer; input = sole writer |
| Threading / IPC | 021 | Main thread + on-demand workers; typed socket IPC over SAB |
| Navigation / SEO | 012, 020 | URL contract; metadata-only DOM (`<title>`/`<meta>`/snapshot) |
| Accessibility | 019 | Deferred; future derived projection from render-object graph |
| HMR | 015 | Whole-module replacement via serialised scene-graph state |
| Tooling / Testing | 014, 016 | Design-tool-as-runtime; author-owned trace, split determinism |
| Capability Imports | 018 | Capability-scoped imports; component-model tree-shaking |

---

## 2. Language Specification

AlkALive is a statically-typed, module- and object-oriented language compiling to a single WASM binary (ADR 008). The grammar is module-centric: a source file declares one `Module`, an optional `Interface` contract, owned fields with explicit visibility, named `Slot`s, typed `Signal`s, and method bodies. There is **no inheritance** — composition is the sole reuse mechanism, expressed by mounting child modules into declared slots. A `Module` *is* a render-object subtree (ADR 007): no reconciler, no separate framework tree, no DOM/HTML/CSS substrate.

### 2.1 Grammar Overview (descriptive, not BNF)

A module is `module <Name> [implements <Interface>] { <decls> }`. Declarations are `input`, `slot`, `output`, `field`, `fn`, `type`, `use`. Inputs are typed properties with optional defaults; slots are named, typed child mount points; outputs are `Signal<T>` emitters. Fields carry an `owner` and `visibility` qualifier. Imports use `use <module>{<capability>}`, gated by ADR 018.

```text
module Button implements Interactive {
  input  label:      Text
  input  intent:     Intent = .default
  field  pressed:    Bool   owner(self) visibility(module)
  slot   leading:    Glyph?                     // explicit, named, optional
  output onActivate: Signal<Intent>
  fn activate(self) { emit(onActivate, self.intent) }
}
```

### 2.2 Type System (ADR 009)

Statically typed with **two-level verification**: (1) the compiler proves source-level soundness — no escape hatches by default, `unsafe` blocks must be explicitly attested; (2) the WASM validator independently checks compiled well-formedness, which is structural only. The two levels compose honestly; no end-to-end semantic proof is claimed.

### 2.3 Encapsulation (ADR 008)

Ownership and visibility are language primitives, replacing closure/`Symbol`/`WeakMap` idioms (P4.2). Each field declares an owner (a module instance or `self`) and a `Visibility` scope; cross-module access requires a declared capability grant. Encapsulation is verified jointly with type soundness at the module boundary.

```text
enum Visibility { Owner, Module, Capability, Public }
```

### 2.4 Module Lifecycle (ADR 015 / ADR 017)

A module transitions through six states. `Loading` corresponds to streaming WASM decode (ADR 017); `Destroyed` is terminal, reached on owner drop or whole-module replacement during HMR (ADR 015), which rehydrates serialisable scene-graph state across the `Destroyed→Loading` edge.

```text
enum ModuleState { Unloaded, Loading, Ready, Active, Suspended, Destroyed }
// Unloaded → Loading → Ready → Active ⇄ Suspended → Destroyed
```

### 2.5 Composition (ADR 007)

Composition is mounting a child `Module` into a declared parent `Slot`. Mounting into an undeclared slot is a compile-time error, eliminating selector drift (P8.4). The child subtree is exclusively owned by the parent; a panic in a child is trapped at the owning boundary, the subtree is torn down deterministically, and the parent receives a typed `Failure` in the affected slot — structural error isolation, not discipline.

### 2.6 Interface Contracts (ADR 014)

Modules compose through typed contracts. An `Interface` declares inputs, outputs, and named child slots; it is the unit of design-tool parity and component testing (ADR 014).

### 2.7 Core Interface Definitions

```text
struct Module {
  id:           ModuleId,
  iface:        InterfaceId,
  state:        ModuleState,
  boundary:     EncapsulationBoundary,
  scene_graph:  OwnedSubtreeRef,       // ADR 007 — single owner
  imports:      Vec<CapabilityGrant>,  // ADR 018
}

struct Interface {                      // ADR 014
  name:    Symbol,
  inputs:  Vec<(Symbol, Type, Option<Value>)>,
  outputs: Vec<(Symbol, Type)>,         // Signal<T>
  slots:   Vec<(Symbol, Type, Cardinality)>,
}

struct Type {                           // ADR 009
  name:       Symbol,
  kind:       TypeKind,        // Primitive | Struct | Enum | Interface | Module
  soundness:  Soundness,       // Proven | UnsafeAttested
  wasm_shape: WasmTypeSig,     // structurally validated by WASM
}

struct EncapsulationBoundary {          // ADR 008
  owner:      ModuleId,
  visibility: Visibility,
  capability: Option<CapabilityId>,     // required iff visibility == Capability
}

struct Slot {                           // ADR 007 / 014
  name:        Symbol,
  child_iface: InterfaceId,
  cardinality: Cardinality,            // Optional | Single | Many
  mount:       Fn(ModuleId) -> Result<MountHandle, SlotError>,
}

struct Signal<T> {
  emit:      Fn(T) -> void,            // module-internal writer
  subscribe: Fn(Listener<T>) -> Subscription,  // capability-gated reader
}
```

### 2.8 Standard Library Baseline (ADR 018)

The language ships a **language-level standard library**: `core` (primitives, Option/Result, Vec, Map), `gpu` (render-graph IR, passes, attachments), `layout` (constraint-solver trait + Cassowary default), `text` (HarfRust shaping, ADR 022), `input` (device-event model), and `ipc` (socket IPC + SharedArrayBuffer, ADR 021). All stdlib surfaces are typed modules exposed through capability-scoped imports; there is no npm transitive trust and no framework-churn layer. Compile-time tree-shaking removes unused modules; the trusted computing base is exactly the declared imports.

### 2.9 Error Types

```text
enum ModuleError {
  CompileFailure(WasmValidationError),   // ADR 009 level 2
  IllegalTransition(FromState, ToState),
  CapabilityDenied(CapabilityId),
  HmrRehydrateFailure(SlotName),
}
enum SlotError   { UndeclaredSlot(Symbol), TypeMismatch(Type, Type), CardinalityExceeded }
enum SignalError { EmitAfterDestroyed, ListenerCapabilityDenied }

struct Failure { slot: SlotName, cause: ModuleError, trace: TraceId }
```

---

## 3. Runtime Architecture

### 3.1 WASM Compilation Target (ADR 008 / 017)

The application ships as a single AOT-compiled WASM binary. The statically-typed, module- and object-oriented source language (ADR 008) compiles ahead-of-time to a compact binary whose startup is bounded by **decode**, not by text parse (ADR 017). **The module *is* the layout engine** — there is no separate framework runtime to bootstrap, no HTML/CSS/JS parsing floor, and no interpreter tier. Decode is **streaming**: WASM validation and compilation begin as bytes arrive over the network, and the layout/render pipeline is constructed concurrently with download. WebGPU shader pipelines are **precompiled asynchronously**, overlapping with module decode so the GPU-side startup floor is removed before first frame rather than deferred to it. ADR 018's compile-time tree-shaking and sectioning are load-bearing for keeping streaming decode sub-frame, given that ADR 021's thread runtime and ADR 022's HarfRust shaping payload enlarge the binary.

### 3.2 Memory Model

The runtime uses two distinct memory regions:

- **WASM linear memory** (`Memory`): private to each WASM agent. Holds the render-object tree, owned per-instance styles, solver-internal layout state, glyph caches, and the full hot-path working set. Accessed only from within WASM; never crossed by JS in the hot path (ADR 013).
- **`SharedArrayBuffer` (SAB)**: the cross-agent region under COOP/COEP isolation (`COOP: same-origin`, `COEP: require-corp`). Scene-graph data shared between the main thread and on-demand workers — instance tables, transforms, draw lists, and immutable render-graph IR — lives here. SAB also backs the WASM-socket ring buffers used as IPC channels (ADR 021).

No render-path object is simultaneously mutable from two threads: workers emit immutable IR into SAB and the main thread is the sole mutator of GPU and scene state.

### 3.3 Host Interaction Contract

The hot path — layout, composition, draw-call emission, hit-testing, input dispatch, and text measurement/rasterization — runs entirely inside WASM and touches exactly one host API surface: **WebGPU**. No JS interop, no DOM crossing occurs per frame (ADR 013). The DOM is **metadata-only** (ADR 020): `<title>`, `<meta>`, and a static SEO snapshot are the sole host-DOM surface, confined to non-hot-path build-time or crawler-triggered operations. Accessibility is deferred (ADR 019); there is no DOM a11y bridge.

### 3.4 Bootstrap Sequence

Startup proceeds through five numbered phases:

1. **Fetch** — the host begins streaming the WASM binary over HTTP; bytes are delivered to the decoder as they arrive.
2. **Streaming-decode** — WASM validation and AOT compilation proceed incrementally as bytes arrive; the layout/render pipeline is constructed concurrently with download (ADR 017).
3. **Pipeline precompilation** — WebGPU shader pipelines are precompiled asynchronously, overlapping with module decode so the GPU-side startup floor is removed before first frame (ADR 017).
4. **Memory & SAB setup** — linear memory is instantiated; the cross-origin-isolation check (COOP/COEP) gates `SharedArrayBuffer` allocation; the worker pool is primed but workers are not yet spawned.
5. **First frame** — the render-object tree is constructed from initial state, layout runs, the first render-graph IR is compiled and submitted, and the GPU presents.

### 3.5 Threading Model (ADR 021 / 003)

The **main thread owns the lone `GPUDevice`** and runs the retain-mode render loop — layout, render-graph compilation/submission, hit-testing, input dispatch (ADR 003/021). It stays on the frame timeline so the unified trace (ADR 016) can correlate logic→layout→draw per tick. **On-demand WASM worker threads** spawn for asynchronous tasks (asset decode, compute, IO); they never acquire the device and never mutate render-path state. Workers emit immutable render-graph IR into SAB and signal the main thread via **WASM sockets** over SAB — typed, serialized, backpressure-aware channels. IPC is WASM↔WASM, preserving the no-DOM-boundary rule of ADR 013. A worker panic is isolated: the handle resolves to `Err(Panic)`, the pool reaps the worker, and the frame timeline is unaffected.

### 3.6 Interfaces

```text
interface Runtime {
  memory:   WASM.Memory;          // private linear memory (hot-path working set)
  sab:      SharedArrayBuffer;    // cross-agent scene-graph + IPC rings
  device:   GPUDevice;            // main-thread-owned (ADR 003/021)
  frameLoop: FrameLoop;
  workers:  WorkerPool;
  bootstrap: BootstrapSequence;
  dom:      DomBridge;            // metadata-only (ADR 020); no hot-path verbs
}

interface FrameLoop {
  // runs on the main thread; one call per vsync
  tick(dt: f32, dirty: &[DirtyRect], input: &InputBatch) -> FrameResult;
  request_layout(scope: ModuleId): void;          // marks dirty subset; locality enforced by solver
  submit(graph: RenderGraphIR) -> SubmitHandle;   // enqueues for merge/compile/reorder/submit
  hit_test(point: Vec2) -> HitResult;             // in-WASM; no DOM crossing
}

interface Compositor {
  // render-thread (main thread) only; serializes all scene graphs
  merge(graphs: &[RenderGraphIR]) -> MergedGraph;
  compile(merged: MergedGraph) -> CompiledFrame;   // reorder / batch / insert barriers
  occlusion_cull(frame: &CompiledFrame, depth: &VisibilityBuffer) -> CulledFrame;
  submit(frame: CulledFrame, device: &GPUDevice) -> SubmitHandle;
}

interface WorkerPool {
  spawn<T: Serial>(task: Fn(SharedState) -> Result<T, Panic>) -> Handle<T>;
  pool_size_hint(): usize;        // advisory; grows on demand
  reap(handle: HandleId): void;   // on panic or channel close
}

enum BootstrapSequence {
  Fetch,              // 1. streaming HTTP download
  StreamingDecode,    // 2. incremental WASM validation + AOT compile
  PipelinePrecompile, // 3. async WebGPU shader precompilation (overlaps phase 2)
  MemorySabSetup,     // 4. linear memory + SAB + COOP/COEP gate
  FirstFrame,         // 5. layout -> render-graph -> submit -> present
}

enum BootstrapError {
  FetchError                  { url: Url, status: u16 },
  DecodeError                 { section: SectionId, cause: ValidationFailure },
  PipelineCompileError        { shader: ShaderId, msg: String },        // degrades to runtime compile; non-fatal
  CrossOriginIsolationUnavailable { header: String },                    // SAB blocked; fatal
  GpuUnavailable              { adapter_reason: String },
  FirstFrameTimeout           { phase: BootstrapSequence, budget_ms: u32 },
}
```

---

## 4. Rendering Engine

The rendering engine realises the author-owned GPU-resident pipeline of §3 as a concrete, backend-abstracted IR compiler plus a retain-mode frame loop. WebGPU is the initial backend (ADR 001); the `Backend` trait admits Vulkan/Metal native backends behind the same IR without touching author code. The hot path — IR submission, compilation, batching, culling, and WebGPU command encoding — runs entirely in WASM on the persistent main thread (ADR 013, ADR 021), preserving per-tick determinism (ADR 016).

### 4.1 Backend Abstraction

```text
trait Backend {
    fn request_adapter(&mut self, pref: PowerPref) -> Result<AdapterHandle, BackendError>;
    fn create_device(&self, adapter: &AdapterHandle) -> Result<DeviceHandle, BackendError>;
    fn create_pipeline(&self, dev: &DeviceHandle, desc: PipelineDesc)
        -> Result<PipelineHandle, PipelineError>;
    fn create_attachment(&self, dev: &DeviceHandle, desc: AttachmentDesc)
        -> Result<AttachmentHandle, AllocationError>;
    fn encode(&self, dev: &DeviceHandle, compiled: &CompiledGraph)
        -> Result<CommandBuffer, EncodeError>;
    fn submit(&self, dev: &mut DeviceHandle, cmds: &[CommandBuffer]) -> Result<SubmitHandle, SubmitError>;
}
```

`WebGPUBackend` is the only shipped implementation; `VulkanBackend` and `MetalBackend` are future native options. Authors never call `Backend` directly — the compositor mediates. The `Backend` trait is also the primary testability seam (§14): `MockBackend` and `SoftwareBackend` implement it for headless CI.

### 4.2 Render-Graph IR

```text
enum PassType { Render, Compute, CopyTransfer, OcclusionCull }
enum AttachmentFormat {
    Bgra8Unorm, Rgba8UnormSrgb, Rgba16Float, Depth24Plus, Depth32Float,
    Stencil8, Bc1..Bc7, Astc4x4, R32Uint,
}

struct Attachment {
    id: AttachmentId,
    format: AttachmentFormat,
    size: ExtentOrRelative,      // absolute or relative to surface
    samples: SampleCount,
    lifetime: (PassId, PassId),  // [producer, last consumer]
    clear_op: ClearOp,           // Clear | Load | DontCare
}

struct RenderPass {
    id: PassId,
    kind: PassType,
    color_attachments: Vec<AttachmentId>,
    depth_stencil: Option<AttachmentId>,
    draw_calls: Vec<DrawCallId>,
    dependencies: Vec<PassId>,   // barrier edges; compiler may reorder/batch respecting these
}

struct DrawCall {
    pipeline: PipelineHandle,    // cached WGSL pipeline (§7.3)
    vertices: VertexBinding,
    indices: Option<IndexBinding>,
    bindings: Vec<BindGroup>,    // owned-style uniforms (ADR 005), instance tables, glyph runs
    instances: Range<u32>,       // GPU-resident instancing; cost decoupled from tree size
    scissor: Option<DirtyRect>,  // ADR 002 scope tag
}

struct RenderGraph {
    passes: Vec<RenderPass>,
    attachments: Vec<Attachment>,
    draw_calls: Vec<DrawCall>,
    occlusion_cull: OcclusionCullPass,
    edges: Vec<(PassId, PassId)>,
    source_module: ModuleId,
}
```

The IR is **immutable** at submission: workers produce it; the render thread consumes it.

### 4.3 Render-Graph Compiler

```text
fn compile(graphs: &[RenderGraph], dirty: &[DirtyRect], depth: &DepthBuffer)
    -> Result<CompiledGraph, CompileError>;
```

The compiler **merges** graphs from all scene graphs (UI, particles, world, overlays), **reorders** passes respecting barrier edges, **batches** draw calls sharing pipeline+bind-group topology, inserts **barriers** at attachment-lifetime boundaries, and runs the **occlusion-cull pass** against the compositor-wide depth/visibility buffer to drop occluded draw calls before encoding. Declaration order need not equal submission order (ADR 001).

### 4.4 Retained Render Loop & Dirty-Rect Invalidation

The loop is **retain-mode**: scene-graph state persists across frames and only dirty rectangles or per-object subsets are re-emitted (ADR 002). Each module owns its subtree; cross-module flex/percentage dependencies that would re-introduce global reflow are rejected at solve time (§5.5) and never reach the loop.

```text
trait RenderLoop {
    fn tick(&mut self, dt: f32, dirty: &[DirtyRect], input: &InputBatch) -> FrameResult;
    fn request_layout(&self, scope: ModuleId) -> void;
    fn submit(&self, graph: RenderGraph) -> SubmitHandle;
    fn hit_test(&self, point: Vec2) -> HitResult;   // in-WASM; no DOM crossing
    fn begin_pass(&mut self, att: &Attachment) -> PassBuilder;
}
```

### 4.5 Compositor & Draw-Call Submission

The **main thread owns the lone `GPUDevice`** (ADR 003, ADR 021). On-demand WASM workers never acquire it; they feed immutable `RenderGraph` IR over `SharedArrayBuffer` and socket IPC. The compositor merges, compiles, reorders, batches, then submits:

```text
trait Compositor {
    fn enqueue(&self, graph: RenderGraph) -> SubmitHandle;        // SAB/socket feed from workers
    fn commit(&mut self, dirty: &[DirtyRect]) -> Result<FrameResult, CompositeError>;
    fn depth_buffer(&self) -> &DepthBuffer;                       // compositor-wide occlusion source
}
```

COOP/COEP cross-origin isolation is required (ADR 003); `credentialless` COEP mitigates embedding conflicts.

### 4.6 Shader Management

WGSL shaders are **first-class styling primitives** (ADR 006). Source is compiled **once at module load** via `Backend::create_pipeline`, and the resulting `PipelineHandle` is cached in a `PipelineCache` keyed by `(shader_hash, layout_hash, render_target_format)`. Subsequent frames bind cached handles; no per-frame shader compilation occurs. Pipeline precompilation removes the GPU-side startup floor (ADR 017). Cache misses fall back to a degraded builtin pipeline and emit a `PipelineError` to the unified trace.

### 4.7 Pipeline Stages & Errors

Per frame: **layout** (§5) → **render-graph compile** (merge/reorder/batch/cull) → **compositor commit** (depth update, draw-call emission) → **WebGPU submit** (backend encode + queue).

```text
enum RenderError {
    Backend(BackendError),
    Compile(CompileError),          // barrier-cycle, attachment-lifetime violation
    Pipeline(PipelineError),        // WGSL compile or layout mismatch
    Allocation(AllocationError),    // attachment/pool exhaustion
    Encode(EncodeError),            // device-lost, validation
    Submit(SubmitError),
    Composite(CompositeError),      // depth-buffer contention, IR schema mismatch
    Locality(LocalityViolation),    // cross-module constraint escaped (ADR 002)
}
```

On error the compositor retains the **last-known-good frame** and surfaces a structured diagnostic to the unified trace (ADR 016), preserving frame-rate stability while exposing the failure for authoring-time correction.

> **Open dependency:** A shared attachment-format and pass-boundary contract between the render-graph IR (§4) and the text stack's glyph-run IR (§6) is deferred to a future rendering-ABI ADR per ADR 001. `ShapedRun` (§6.3) currently carries no attachment-format field; the rendering-ABI ADR will define the glyph-quad → attachment binding.

---

## 5. Layout System

### 5.1 Decision and Scope

Per ADR 004, the runtime replaces the CSS box-tree pipeline with a **pluggable constraint solver operating over first-class render objects** (ADR 007). Layout is **not delegated to authors**: the runtime ships a default Cassowary-class linear solver and exposes the `LayoutSolver` trait as the sole extension surface. Author-supplied backends — impulse/physics, directed-graph, GPU-compute — bind behind the same trait, so swapping solvers is internal and non-breaking to downstream paint stages. The layout-tree is **solver-internal** and never re-derived from styles, eliminating the style-driven box-tree recalculation that couples style mutation to global reflow (P2.3, P2.4).

### 5.2 Geometry Primitives

All layout math is expressed over five value types, shared with the render-graph IR (§4) and input hit-testing:

```text
struct Vec2  { x: f32, y: f32 }
struct Size  { w: f32, h: f32 }
struct Rect  { origin: Vec2, size: Size }
struct Mat4  { m: [f32; 16] }       // column-major; consumed directly as GPU instance transform
struct Constraint {
    kind:   ConstraintKind,
    a:      LayoutVar,              // left-hand variable / object facet
    b:      LayoutVar,              // right-hand variable / constant
    weight: f32,                    // strength (linear) or stiffness (impulse)
    module: ModuleId,               // locality tag (ADR 002)
}
```

`LayoutVar` references either a `RenderObjectId` facet (`x`, `y`, `w`, `h`, `baseline`) or a literal `f32`.

### 5.3 Constraint Kinds and the Solver Trait

```text
enum ConstraintKind {
    Linear,        // Cassowary equality/inequality over LayoutVars
    Impulse,       // spring/velocity physics; integrates over dt
    GraphLayout,   // rank/layered or force-directed over adjacency
}

enum SolveStatus {
    Solved,                // committed to instance buffers
    Partial,               // relaxed below threshold; still acceptable
    Unsatisfiable,         // rejected; last-known-good retained
    LocalityViolation,     // cross-module dep rejected at assert_local
}

trait LayoutSolver {
    fn add_node(&mut self, node: LayoutNode) -> NodeId;
    fn remove_node(&mut self, id: NodeId) -> void;                  // per-module dirty-rect (ADR 002)
    fn bind_style(&mut self, id: NodeId, style: &OwnedStyle) -> void;  // input only; never mutated
    fn add_constraint(&mut self, c: Constraint) -> ConstraintId;
    fn remove_constraint(&mut self, id: ConstraintId) -> void;

    // Locality gate (ADR 002). Rejects cross-module flex baselines,
    // percentage chains spanning module boundaries, or any constraint
    // whose satisfaction would reflow outside the dirty set.
    fn assert_local(&self, c: &Constraint) -> Result<(), LocalityViolation>;

    // Synchronous solve over the dirty subset; consumes measured text
    // runs (§5.4) and emits GPU-ready transforms. No intermediate tree.
    fn solve(&mut self, dirty: &DirtySet, measured: &dyn MeasuredRun,
             dt: f32) -> Result<LayoutSolution, SolveError>;
}

struct LayoutNode {
    id:       RenderObjectId,
    module:   ModuleId,            // ownership tag (ADR 002)
    measure:  MeasureKind,         // Fixed | Text | Intrinsic
    children: Vec<NodeId>,         // solver-internal; not a serializable box tree
}
```

### 5.4 Mandatory Text-Flow Measurement Contract (ADR 004, ADR 022)

Every solver — including user-supplied — must consume a synchronous `MeasuredRun` interface, because box/physics/graph solvers cover none of line-breaking, BiDi, or font-metric shaping. The backing implementation is the **forked in-WASM HarfRust** stack of ADR 022; no DOM text surface is permitted (ADR 020, ADR 022). This contract guarantees that swapping the outer solver never destabilises text fragmentation (P2.4).

```text
interface MeasuredRun {
    // Synchronous; HarfRust-backed; no DOM crossing.
    shape_and_measure(run: TextRun, ctx: FontContext) -> GlyphMetrics;
    line_break(glyphs: GlyphRun[], max_width: f32) -> LineBreak[];
}
// GlyphMetrics: advances, ascents, descents, cluster map, caret positions.
```

> **Shared boundary:** `MeasuredRun` is the canonical §5↔§6 contract. §5 (layout) consumes it; §6 (text stack) implements it via `TextStack` (§6.9). The `TextStack::measure` method (§6.9) adapts the `ShapedRun` output to the `GlyphMetrics`/`LineBreak` types this interface expects; the two interfaces are semantically identical (synchronous, HarfRust-backed, no DOM) and the rendering-ABI ADR (§4.7) will unify their type signatures precisely.

### 5.5 Layout-Locality Guarantee (ADR 002)

Each module owns its scene-graph subtree and invalidates only dirty rectangles; per-frame cost is bounded by the dirty subset, not tree size. Locality is **enforced, not assumed**: cross-module flex baselines, percentage chains spanning module boundaries, and any constraint whose satisfaction would trigger reflow outside the dirty set are rejected at solve time via `assert_local`, never silently propagated. This closes the global-reflow bug class documented in [20, 21] and ADR 002.

### 5.6 Solver Outputs Feed GPU Transforms Directly

```text
struct LayoutSolution {
    status:     SolveStatus,
    transforms: Vec<(RenderObjectId, Mat4)>,   // written into GPU instance buffers
    clips:      Vec<(RenderObjectId, Rect)>,   // consumed by occlusion-cull pass (§4.3)
    glyph_runs: Vec<GlyphRun>,                 // forwarded to the text atlas (§6)
    module:     ModuleId,                      // locality tag for dirty-rect scoping
}
```

`solve` outputs are written directly into GPU-resident instance buffers consumed by the render-graph IR of §4. There is **no style-driven box-tree recalculation**: style values enter as constraint inputs via `bind_style`, never as a re-derivation trigger.

### 5.7 Error Handling

```text
enum SolveError {
    Unsatisfiable     { offenders: Vec<ConstraintId>, suggestion: RelaxationHint },
    LocalityViolated  { constraint: ConstraintId, boundary: (ModuleId, ModuleId) },
    MeasurementFailed { run: TextRunId, cause: ShapeError },
    Timeout           { budget_exceeded_ms: u32 },
}
```

When `solve` returns `Unsatisfiable`, the engine **retains the last-known-good layout commit** (cached per-module) and emits a structured diagnostic — offending constraint IDs, locality violations, and a suggested minimal relaxation — to the unified author-owned trace (ADR 016). The frame renders against the cached layout rather than stalling or producing an inconsistent partial solve, preserving frame-rate stability while surfacing the failure for authoring-time correction.

---

## 6. Text Rendering

### 6.1 Forked HarfRust Integration (ADR 022)

Text shaping, BiDi reordering, and glyph rasterization run entirely inside WASM via a **forked HarfRust** stack, vendored in-repo under `vendor/harfrust/`. The fork is maintained on the project's own cadence: shaping fixes, platform patches, and BiDi/IME extensions land independently of upstream releases. There is **no DOM text render path** — no `<canvas.fillText>`, no hidden text nodes, no `measureText` fallback (ADR 020, ADR 022). The text stack is the sole producer of glyph geometry; ADR 013's hot-path integrity is preserved structurally because no WASM↔DOM boundary exists for text.

### 6.2 Font Loading — `FontRegistry`

`FontRegistry` resolves family/weight/style triples to a decoded font face, caches parsed OpenType tables, and serves HarfRust directly from WASM-heap memory. Resolution follows a **fallback chain**: requested family → generic family alias (`serif`/`sans`/`mono`) → bundled default. A miss never aborts layout; the registry returns `FontLoadError::FallbackResolved` carrying the substituted `FontId` so the caller can re-shape against the fallback.

```text
enum FontLoadError {
    FamilyNotFound,
    WeightUnavailable,
    TableDecodeFailed { font_id: FontId, table: Tag },
    FallbackResolved { actual: FontId },  // soft failure
    RegistryEmpty,
}

trait FontRegistry {
    fn resolve(&mut self, req: &FontRequest) -> Result<FontId, FontLoadError>;
    fn face(&self, id: FontId) -> &DecodedFace;     // cached tables
    fn load_bundle(&mut self, bytes: &[u8]) -> Result<FontId, FontLoadError>;
    fn fallback_chain(&self, id: FontId) -> &[FontId];
}
```

### 6.3 Shaping — `TextShaper` & `ShapedRun`

`TextShaper` wraps HarfRust: it accepts a Unicode run plus a resolved `FontId`/`TextStyle`/`Language`, performs BiDi segmentation and reordering in-WASM, and emits a `ShapedRun`. A run is immutable after shaping; downstream consumers (layout's measured-run contract per §5.4/ADR 004, the rasterizer, the hit-tester) all read the same instance.

```text
enum ShapeError {
    FontUnresolved,
    InvalidUtf8,
    BidiOverflow { level: u8 },
    UnsupportedScript { script: Script },
    NotdefOnly,           // every glyph resolved to .notdef
}

struct ShapedRun {
    glyph_ids:  Box<[u32]>,        // HarfRust-shaped glyph IDs
    advances:   Box<[f32]>,        // per-glyph x-advance (signed for RTL)
    offsets:    Box<[(f32, f32)]>, // baseline-relative offset per glyph
    clusters:   Box<[u32]>,        // source-codepoint index per glyph
    caret_map:  ClusterMap,        // glyph idx <-> caret offset (BiDi-aware)
    metrics:    RunMetrics,        // ascent, descent, line_gap, total_advance
    bidi_level: u8,                // Unicode BiDi embedding level
    font_id:    FontId,            // resolved font (fallback-aware)
    direction:  Direction,         // LTR | RTL
}

trait TextShaper {
    fn shape(&self, run: &str, ctx: &ShapeContext)
        -> Result<ShapedRun, ShapeError>;
    fn reshape_with_font(&self, run: &str, font: FontId)
        -> Result<ShapedRun, ShapeError>;
}
```

Uncovered codepoints surface as `.notdef` glyph IDs with real metrics (visible tofu) — the pipeline never aborts on missing coverage.

### 6.4 Glyph Atlas — `GlyphAtlas`

A GPU-resident **LRU glyph atlas** rasterizes glyphs on demand. Atlas slots are addressed by `(FontId, glyph_id, subpixel_phase, size_px)`; a miss triggers HarfRust rasterization into a staging buffer, then a `queue.writeTexture` upload into the next free tile. Eviction is LRU with a pin set held by in-flight render-graph IR (§4). Invalidation follows **per-module dirty-rect locality** (ADR 002): a re-shaped run only dirties its own atlas footprint, never the whole atlas.

```text
trait GlyphAtlas {
    fn ensure(&mut self, key: GlyphKey) -> AtlasSlot;     // rasterize-on-demand
    fn slot(&self, key: GlyphKey) -> Option<AtlasSlot>;   // cached only
    fn invalidate(&mut self, module_id: ModuleId, rect: DirtyRect);
    fn evict_lru(&mut self, keep: &PinSet) -> EvictionStats;
}

struct GlyphKey   { font_id: FontId, glyph_id: u32, phase: u8, size_px: u16 }
struct AtlasSlot  { page: u16, uv: Rect, bearing: (f32, f32), size: (f32, f32) }
```

### 6.5 Rasterization to Render-Graph IR

Shaped glyphs become **textured quads** in the render-graph IR (§4). `rasterize()` walks the `ShapedRun`, queries the atlas for each `GlyphKey`, and emits a `GlyphQuadBatch` referencing atlas UVs — no pixel work happens on the hot path beyond the first-seen upload. The compositor (ADR 003) batches glyph quads across modules into a single instanced draw.

### 6.6 Editing Primitives — `CaretSelection`

Caret, selection, and hit-testing are built atop HarfRust output (ADR 022 negative consequence: no DOM contracts inherited). `CaretSelection` is BiDi-aware: anchor/active offsets are codepoint indices into the source string, and `hit_test` maps a point to the nearest caret via `caret_map`, honoring directional affinity.

```text
struct CaretOffset    { cp_index: u32, affinity: Affinity }   // Upstream | Downstream
struct CaretSelection { anchor: CaretOffset, active: CaretOffset }

trait EditingOps {
    fn hit_test(&self, run: &ShapedRun, point: (f32, f32)) -> CaretOffset;
    fn caret_position(&self, run: &ShapedRun, offset: CaretOffset) -> (f32, f32);
    fn selection_quads(&self, run: &ShapedRun, sel: CaretSelection) -> Box<[Quad]>;
}
```

### 6.7 IME — Open Dependency

> ⚠ **Open dependency — see `Spec_Tradeoff_Note_IME.md`.** ADR 020 forbids DOM input elements; no ADR commits a replacement for acquiring platform IME composition events. Candidates: (a) WASM-native platform input API (no DOM); (b) a narrowly-scoped hidden `<input>` carrying composition state only, classified non-hot-path, requiring a formal ADR 020 exception; (c) defer IME until a platform API matures. The stack exposes `ime_compose(CompositionEvent) -> ImeState` so the acquisition mechanism is pluggable behind a stable interface.

### 6.8 A11y Text Exposure (ADR 019)

Accessibility is deferred per ADR 019; no DOM a11y contracts are inherited. The text stack exposes a **placeholder** `expose_a11y_text(&ShapedRun) -> A11yTextPlaceholder` returning source-text + caret/selection + run metrics, so any future derivation layer built against the render-object graph (ADR 007) can consume it without reshaping.

### 6.9 `TextStack` — Top-Level Interface

```text
trait TextStack: TextShaper + EditingOps {
    // Implements the MeasuredRun contract consumed by §5's LayoutSolver.
    // Adapts ShapedRun output to GlyphMetrics/LineBreak types.
    fn measure(&self, run: &ShapedRun, max_width: f32) -> MeasuredLines;   // ADR 004
    fn rasterize(&self, run: &ShapedRun, atlas: &mut GlyphAtlas) -> GlyphQuadBatch;
    fn expose_a11y_text(&self, run: &ShapedRun) -> A11yTextPlaceholder;    // §6.8
    fn ime_compose(&mut self, ev: CompositionEvent) -> ImeState;           // §6.7
}
```

---

## 7. Styling & Theming

### 7.1 Property System (ADR 005)

Styling is **per-instance object-owned property state**, bound at construction and addressable only via the owning render object (ADR 007). There is **no cascade, no CSSOM, no selector matching, and no specificity comparator** — every styled property is a typed field on the object itself. Style tables are compiled into the WASM module's binary data section as a compact binary blob; no stylesheet is parsed at runtime, no rule is matched, and no cascade is resolved per frame. Access is **O(1) local field read** against binary-compiled state.

```text
trait Style {
    fn property(&self, kind: PropertyKind) -> StyleProperty;
    fn color(&self)      -> Color;          // default: transparent black
    fn opacity(&self)    -> Opacity;        // default: 1.0
    fn line_width(&self) -> LineWidth;      // default: 0.0
    fn transform(&self)  -> Mat4;           // default: identity
    fn effect(&self)     -> ShaderStyle;    // default: passthrough
    fn animation(&self, name: &str) -> Option<&Animation>;
}
```

### 7.2 Property Types

A `StyleProperty` is a closed enum over three categories:

```text
enum PropertyKind {
    Color,
    Opacity,
    LineWidth,
    Transform,
    Shader,
    Custom(u32),   // module-declared typed fields, e.g. SpringVelocity
}

enum StyleProperty {
    Scalar(ScalarValue),
    Transform(Mat4),
    Shader(ShaderStyle),
}

enum ScalarValue {
    Color(u32),        // RGBA8 packed
    Opacity(f32),      // clamped to [0.0, 1.0]
    LineWidth(f32),    // clamped to [0.0, ∞)
}
```

- **Scalar** — `Color (u32 RGBA)`, `Opacity (f32 ∈ [0,1])`, `LineWidth (f32 ≥ 0)`.
- **Transform** — `Mat4` (or SRT decomposition), consumed directly by the GPU transform upload.
- **Shader** — a WGSL program entry point plus a packed uniform buffer (ADR 006).

### 7.3 WGSL as a First-Class Style Primitive (ADR 006)

A `ShaderStyle` pairs a compiled WGSL module with a uniform buffer sourced from the owning object's fields; the render graph (ADR 001) schedules it as an explicit paint or compute pass. This **replaces CSS's closed `filter` catalogue**: gradients, particles, per-vertex displacement, and compute-driven styling are authored rather than approximated.

```text
struct ShaderStyle {
    program: WgslModule,          // compiled once at module load; cached as pipeline object
    entry_point: &'static str,
    uniforms: UniformBuffer,      // packed from owning object's fields
    bindings: [BindGroupEntry; N],
}
```

Built-in and user-authored WGSL effects are treated uniformly by the renderer.

### 7.4 Theming

A **theme is a module exporting a set of named style presets** — construction-time token bundles, not a propagation system. Themes are not cascading scopes; they are construction-time dictionaries applied by explicit `set` calls:

```text
interface Theme {
    fn preset(&self, name: &str)  -> Style;
    fn default(&self)             -> Style;
    fn names(&self)               -> &[&'static str];
}
```

A render object receives its style either by looking up a named preset (`theme.preset("primary-button")`) or by accepting `theme.default()`. **There is no inheritance**: a property not explicitly set takes the type's default value, not a parent's value. Subtree consistency is the author's responsibility, expressed at construction rather than resolved at match time.

### 7.5 Animation Framework

Animated properties are **tween/keyframe interpolations over owned fields** — not CSS transitions, not declarative cascade animations. An `Animation` is a value-level state machine that writes directly to the owning object's style fields each frame; the render object polls it during the per-frame style-read phase.

```text
struct Animation {
    property: PropertyKind,
    keyframes: Vec<Keyframe>,
    duration: Duration,
    easing: EasingFn,
    elapsed: Duration,
    state: AnimationState,
}

struct Keyframe {
    time: f32,                    // normalized ∈ [0.0, 1.0]
    value: StyleProperty,
    interpolation: Interpolation, // Linear | Step | CubicSpline
}

enum AnimationState { Idle, Running, Paused, Completed }
```

There is no transition-triggered recalc: `tick(dt)` advances `elapsed` and writes the interpolated `StyleProperty` back into the owning field. The render-object's existing per-frame style read observes the new value uniformly with non-animated fields.

### 7.6 Separation from Rendering

Style is **read-only input** to two downstream stages and never triggers box-tree recalc:

- **Layout** — the constraint solver (ADR 004) consumes scalar/transform values as constraint inputs; it never mutates style.
- **Render** — shader uniforms and transform upload consume `ShaderStyle` and `Mat4` directly.

Style mutation marks the owning render object dirty per ADR 002's per-module dirty-rect invalidation; the box tree itself is not rebuilt. The flow is style change → dirty rect → re-solve + re-emit, never structural rebuild.

### 7.7 Error Handling

```text
enum AnimationError {
    KeyframeOutOfOrder,
    InvalidPropertyKind(PropertyKind),
    InterpolationNotSupported(Interpolation, PropertyKind),
    DurationZero,
}
```

- **Shader compile failure** — the `ShaderStyle` falls back to the default passthrough effect and emits a structured diagnostic into the trace (ADR 016); the object remains visible, never unrendered.
- **Invalid property values** — out-of-range scalars (opacity `1.7`, negative `LineWidth`) are **clamped to their valid range** with a build/runtime warning; type-mismatched values are a compile error, never a silent coercion.
- **Animation errors** — `Animation::tick` returns `Result<(), AnimationError>`; the runtime logs the error and freezes the animation at its last valid frame.

---

## 8. Input & Event System

### 8.1 Event Capture

All raw device state is captured at the WASM scheduler boundary as a single typed `InputBatch` per frame and stays inside the ADR 013 hot path — no WASM↔DOM crossing. Pointer, stylus (pressure/tilt/twist), multi-touch contact sets, gamepad axes/buttons, and keyboard are first-class typed events; no device class is second-class (ADR 010, dissolving P5.2).

```text
enum DeviceKind { Pointer, Stylus, Touch, Gamepad, Keyboard }

union InputEvent {
  PointerSample,   // DeviceKind ∈ {Pointer, Stylus, Touch}
  KeyEvent,
  GamepadSample,
}

struct InputBatch {
  events:      Array<InputEvent>   // pre-partitioned by device
  device_mask: DeviceKindSet       // which classes are present this frame
  timestamp:   MonotonicNs
}

struct PointerSample {
  device:    DeviceKind
  device_id: u32
  position:  Vec2                  // viewport space
  delta:     Vec2
  pressure:  f32                   // [0,1]; 1 for mouse
  tilt:      Vec2                  // radians; zero for mouse
  twist:     f32                   // stylus only
  buttons:   ButtonSet
  phase:     PointerPhase          // Down | Move | Up | Cancel
}

struct KeyEvent {
  code:         KeyCode
  modifiers:    ModifierSet
  phase:        KeyPhase           // Press | Release | Repeat
  repeat_count: u32
}

struct GamepadSample {
  device_id: u32
  axes:      Array<f32>            // [-1,1]
  buttons:   Array<f32>            // pressure [0,1]
}
```

### 8.2 Hit-Testing

A CPU-resident bounding-volume mirror of the GPU scene (ADR 001 source-of-truth) is refreshed after every layout commit (ADR 002) and is the hot-path hit surface (ADR 010). GPU pick-buffer readback is invoked only for *precise* picks — sub-pixel caret placement, polygonal shapes — and never appears per-frame.

```text
interface HitTester {
  fn refresh(scope: LayoutScope)               // after each layout commit
  fn hit_test(point: Vec2, device: DeviceKind) -> Array<HitResult>
  fn precise_pick(point: Vec2) -> HitResult    // GPU pick-buffer readback
  fn invalidate(scope: LayoutScope)
}

struct HitResult {
  object:  Handle<RenderObject>   // ADR 007
  point:   Vec2                    // local-space hit
  depth:   Float                   // overlap ordering
  precise: Bool
}
```

### 8.3 Event Routing

Dispatch is a single direct call from the scheduler to the hit object with an owned `InputEvent`. There is no DOM-style capture/target/bubble propagation (ADR 013). For cross-frame gestures (drag, swipe, multi-touch rotate), the hit object captures the stream by returning a `GrabHandle`; subsequent events of the same `(device, device_id)` are routed to that handle until released.

```text
interface GrabHandle {
  fn owner()     -> Handle<RenderObject>
  fn device()    -> DeviceKind
  fn device_id() -> u32
  fn release()
  fn is_active() -> Bool
}
```

### 8.4 Gesture Recognition

Render objects own their gesture/state machines; there is no central recogniser (ADR 010). Each object exposes one `GestureState` per active device stream and produces a `GestureOutcome` per event. When two objects claim overlapping grabs, **the most recently issued explicit grab wins**; the loser receives a synthetic `Cancel`. The scheduler never arbitrates semantic intent — it only enforces grab ordering.

```text
enum GestureOutcome { Continue, Commit, Cancel, Grab(GrabHandle) }
enum GesturePhase   { Idle, Began, Changed, Ended, Cancelled }

interface GestureState {
  fn on_event(event: InputEvent) -> GestureOutcome
  fn on_cancel()                              // synthetic Cancel on orphan
  fn current_phase() -> GesturePhase
}
```

### 8.5 Focus Model (ADR 011)

Focus, tab order, and the focus ring live on a **unified virtual focus annotation layer** — a cached, invalidation-driven derived view over the render-object graph (ADR 007), not a separate tree. **Input dispatch is the sole writer** of focus state; **focus-ring rendering is the sole active reader**. The object receiving input is therefore the same object that owns the focus annotation, dissolving the DOM/canvas focus blackout [39,40,41]. Accessibility-tree derivation and AT announcement are deferred per ADR 019 and are not on the input critical path. Activation events (Enter/Space on a focused, route-bearing object) feed the structured navigation contract of ADR 012.

> **Shared boundary with §10:** The focus annotation layer is the shared §8↔§10 surface. §8 (input) writes focus state via `set_focus`; §10 (accessibility) will read it for AT announcement when un-deferred — but that read is deferred per ADR 019. `FocusManager::current_focus()` is the read entry point that the future a11y layer will consume. No DOM projection surface exists in this phase.

```text
enum FocusEvent { FocusGained, FocusLost, FocusStealCancelled }

interface FocusManager {
  fn dispatch(batch: InputBatch, hits: Array<HitResult>) -> Array<InputError>
  fn set_focus(target: Handle<RenderObject>)          // sole writer
  fn current_focus() -> Option<Handle<RenderObject>>   // sole active reader (future a11y reads here too)
  fn tab_next() / fn tab_prev()                       // virtual tab order
  fn emit_focus_events() -> Array<FocusEvent>         // for focus-ring renderer
  fn invalidate(scope: LayoutScope)
}
```

### 8.6 Error Handling

Invalid input state is normalised at the scheduler boundary. Orphaned grabs (object removed mid-gesture) are cancelled with a synthetic `Cancel`; out-of-range device IDs are dropped and logged; mismatched touch begin/end cycles yield `UnmatchedTouchBegin`; stale mirror hits either re-route through `precise_pick` or return `MirrorStale`.

```text
enum InputError {
  StaleContact,
  OrphanedGrab,
  UnmatchedTouchBegin,
  OutOfRangeDeviceId,
  PickBufferReadbackFailed,
  MirrorStale,
}
```

---

## 9. DOM Interop Layer

### 9.1 Problem

Crawlers, link-preview agents, and host shells address a DOM; a canvas/WebGPU renderer inherits none of its affordances. The question is *how much* DOM surface the runtime must retain for SEO and host navigation without reintroducing the per-call WASM↔DOM boundary tax (P7.4) or the layout/paint-coupled bug class (P7.3).

### 9.2 Solution — Metadata-Only DOM Layer (ADR 020)

The runtime exposes a **thin DOM bridge** for exactly three concerns: setting `<title>`, writing `<meta>` tags, and serving a static HTML snapshot. There is **no DOM-tree interaction for UI** — no layout, text, accessibility, navigation-DOM, or input bridge exists. UI rendering is fully GPU-resident via WASM (ADR 013); a11y is deferred (ADR 019); text is in-WASM (ADR 022); navigation is a structured host contract (ADR 012). The bridge is non-hot-path by construction: it exposes **no per-frame verbs**.

### 9.3 Navigation / URL Contract (ADR 012)

Navigation is a **structured contract**, not a DOM mutation. The app declares its routes and serialises restorable state to the host; the host owns URL, history, and back/forward semantics. The DOM `<title>`/`<meta>` pair and the snapshot are the *sole* SEO export surface. No `pushState`/`replaceState` analogue is exposed through the bridge — route/state changes flow to the host, which is free to ignore them.

### 9.4 SEO Snapshot Generation

Snapshots are emitted **at build time** from declared routes plus serialisable state, or **on-demand** to detected crawler user-agents. The runtime performs **no DOM mutation for UI**: a snapshot is a frozen `SeoSnapshot` value, never a live tree. On-demand generation runs off the render thread and never blocks the frame loop.

### 9.5 IME — No Exception (ADR 020)

ADR 020 forbids DOM input interop. **No exception is granted.** IME composition-event acquisition remains an **open dependency owned by §6** (Text Rendering Stack); `DomBridge` deliberately exposes no IME method. A future ADR must resolve IME off-DOM (platform event APIs, OS-level composition hooks) or via a scoped, explicitly non-hot-path exception — see `Spec_Tradeoff_Note_IME.md`.

### 9.6 Error Handling

DOM API failures — snapshot write failure, meta mutation rejection, host route-decline — return a typed `DomError` and degrade gracefully: the build-time snapshot continues to be served to crawlers, and the GPU render loop is unaffected. DOM failure **never** blocks the WASM render thread; the bridge is fire-and-forget from the frame loop's perspective.

### 9.7 Interfaces

```text
enum DomError {
  HostUnavailable,      // no host DOM context bound
  MetaRejected,         // host refused meta write (policy/csp)
  SnapshotWriteFailed,  // snapshot serialisation/emission failed
  RouteDeclined,        // host rejected a declared route
  StateUnserialisable,  // state contained a non-serialisable value
  Timeout,              // host did not acknowledge within budget
}

interface DomBridge {
  // SEO export surface — sole UI-relevant DOM writes (ADR 020)
  setTitle(text: String): Result<void, DomError>
  setMeta(name: String, content: String): Result<void, DomError>
  serveSnapshot(route: Route, state: SerialisableState): Result<Html, DomError>

  // Navigation/state contract (ADR 012) — host-facing, non-hot-path
  declareRoutes(routes: List<Route>): Result<void, DomError>
  serializeState(): Result<SerialisableState, DomError>
}
// No methods for layout, draw, hit-test, text-measurement,
// a11y, focus, input, or IME. None may be added without an ADR
// amending ADRs 013 / 019 / 020.

struct SeoSnapshot {
  route: Route
  title: String
  meta: List<(name: String, content: String)>
  html: Html                  // frozen, crawler-grade; never a live tree
  generated_at: Timestamp
  source: SnapshotSource      // BuildTime | OnDemand
}

interface NavigationContract {
  declareRoutes(routes: List<Route>): Result<void, DomError>
  serializeState(): Result<SerialisableState, DomError>
  // Host retains URL/history/back-forward ownership; the runtime
  // never mutates addressable document state directly.
}
```

`DomBridge` composes the SEO verbs (`setTitle` / `setMeta` / `serveSnapshot`) with the `NavigationContract` verbs (`declareRoutes` / `serializeState`); both are non-hot-path, host-facing, and structurally incapable of crossing into the render loop. The interface is closed under ADRs 012/013/019/020: any addition requires a new ADR.

---

## 10. Accessibility Placeholder

### 10.1 Deferred, Not Cancelled (ADR 019)

Accessibility is **deferred** for this phase by owner directive, overriding `Decision_Alternatives_accessibility-bridge.md`'s prior Approach C (hybrid DOM projection). No DOM mirror, no DOM projection surface, and no assistive-technology (AT) bridge ship in the initial release. P6.1 — canvas severing every native DOM a11y affordance — retains its co-decisive hard-problem status alongside P3.5; only its resolution is removed from the release-blocking critical path. This is a **deferral, not a cancellation**: the extension surface below is committed so that un-deferral is additive, not architectural.

### 10.2 What Stays Active (ADR 011)

The focus half of ADR 011's unified annotation layer remains in force. Input dispatch is the **sole writer** of focus state (ADR 010); focus annotations live on the cached annotation layer, not on render objects, preserving ADR 007's module ownership. Focus-ring rendering remains the sole active reader of those annotations. Only the AT-announcement half is deferred. The §8↔§10 writer discipline is therefore intact — when a11y is un-deferred, a reader can be attached without re-architecting focus.

### 10.3 Extension Points That Survive Deferral

The render-object graph (ADR 007) already carries, as **mandatory fields**, the metadata ADR 011's annotation layer was designed to derive from: `role: SemanticRole`, `structured_data: StructuredData`, `interaction: InteractionDescriptor`. A future a11y tree is **derived** from this metadata — never separately authored, never mirrored through a DOM sync boundary. The text stack (ADR 022) exposes a placeholder `expose_a11y_text` interface so shaped glyph runs, BiDi segments, selection, caret state, and labels can flow into the future tree without re-engineering the shaper.

### 10.4 Extension Path

When un-deferred, a virtual accessibility tree is derived from the render-object graph and bridged **directly to platform a11y APIs** (UIAutomation / NSAccessibility / AT-SPI / ARIA-equivalent native surfaces). No DOM is reintroduced; the bridge targets host-native a11y APIs, not browser DOM contracts.

### 10.5 Placeholder Interfaces — Stubs Only, No Implementation This Phase

```text
enum SemanticRole {
    None, Generic, Button, Link, Heading, Text, Image,
    List, ListItem, TextField, Checkbox, Slider, Dialog,
    // Extensible; values MUST align to future platform-a11y role sets.
}

interface A11yNode {            // PLACEHOLDER — no implementation this phase
    role:        SemanticRole
    label:       Option<TextLabel>
    structured:  StructuredData
    interaction: InteractionDescriptor
    children:    Vec<A11yNode>
    focus_state: FocusState       // read-only mirror of ADR 011 layer
}

interface A11yExtensionPoint {  // PLACEHOLDER — derived, not authored
    derive_a11y_node(from: RenderObject)  -> Option<A11yNode>
    expose_a11y_text(run: ShapedGlyphRun) -> TextLabel
    read_focus_state() -> FocusState      // future reader entry point (§8 FocusManager::current_focus)
}

interface A11yPlaceholder {     // PLACEHOLDER — top-level stub
    build_tree(root: RenderObject) -> A11yNode
    bridge_to_platform(tree: A11yNode) -> ()   // no-op this phase
}
```

These are **committed signatures, unimplemented bodies**. They exist to lock the extension contract so un-deferral cannot ripple into render-object, focus, or text-stack internals. `A11yExtensionPoint::read_focus_state()` is the future reader entry point that consumes `FocusManager::current_focus()` (§8.5) — the shared §8↔§10 boundary.

### 10.6 Risk Acknowledgment

Until a later phase ships, AT users have no a11y path: the runtime is **non-conforming to web a11y contracts** (WAI-ARIA, the accessible-name computation, focus-management expectations) in the interim. This is an explicit, owner-approved risk accepted to unblock the runtime — not an overlooked gap.

---

## 11. Concurrency & IPC

### 11.1 Threading Topology

The runtime is a **main thread + on-demand WASM worker threads** hybrid (ADR 021), with the main thread as the canonical `GPUDevice` owner (ADR 003; ADR 021 supersedes the dedicated-worker fallback noted in ADR 003). The split is structural, not advisory:

- **Main thread** owns `GPUDevice` and all derived objects (`GPUQueue`, pipelines, buffers, textures). It is the sole mutator of GPU and scene state. It runs the retain-mode render loop — layout pass (ADR 004), render-graph merge/compile/reorder/batch/submit (ADR 001), CPU bounding-volume hit-test (ADR 010), and input dispatch — on the deterministic frame timeline required by the unified author-owned trace (ADR 016). No off-thread work touches these surfaces.
- **Worker pool** spawns on demand for async tasks: asset decode, compute (e.g. particle simulation), and IO. Workers **never** acquire `GPUDevice` and never mutate render-path state. They emit immutable render-graph IR (ADR 001) into a `SharedArrayBuffer` (SAB) and signal the main thread over a socket channel. The main thread is the sole IR consumer and submitter.

### 11.2 Task Scheduling

`Scheduler` drives the main thread on the frame timeline (vsync-bounded `begin_frame` / `commit` pairs). Workers run **off-frame**: their start and finish are not pinned to a specific tick. IR merges occur only at well-defined **commit points** — the main thread drains pending worker IR at the start of each `commit` phase, applying complete IR snapshots and discarding partial or stale ones. This preserves the per-tick trace correlation of ADR 016: every span attributes to exactly one frame.

### 11.3 Synchronization

Cross-thread data lives in `SharedArrayBuffer` coordinated via `Atomics.wait`/`notify` and `Atomics.load`/`store`/`compareExchange`. **No render-path object is ever simultaneously mutable from two threads.** Workers emit immutable IR (append-only, versioned) into SAB ring buffers; the main thread is the sole mutator of GPU state and the sole IR consumer. This eliminates lock-free complexity on the GPU submission path (ADR 003) and keeps the WASM-sandboxed layout determinism of ADR 016 intact. COOP/COEP cross-origin isolation (`COOP: same-origin`, `COEP: require-corp`, with `credentialless` as the iframe mitigation) is mandatory for SAB; if unworkable, the runtime degrades to per-graph separate devices, losing the shared compositor.

### 11.4 IPC via WASM Sockets

`IPCSocket<T>` is the sole cross-thread primitive — a typed, serialized, backpressure-aware channel backed by a SAB ring buffer with `Atomics`-based signaling. Sockets are **never** GPUDevice-aware: only serializable IR, asset blobs, and command/result enums traverse them.

```text
enum TaskKind { AssetDecode, Compute, IO, Shape }

enum TaskError {
  Panic(payload: Blob),        // worker trapped; pool reaps
  Channel(ChannelError),       // framed / closed / underrun
  Cancelled,                   // handle dropped or deadline exceeded
  Decode(DecodeError),
}

enum ChannelError {
  Closed,                      // peer gone
  Framing,                     // corrupt header / size mismatch
  Underrun,                    // ring empty past deadline
  Backpressure,                // ring full; sender must yield
  Serialize(SerialError),
}

interface IPCSocket<T: Serial> {
  send(msg: T): Result<(), ChannelError>;              // yields on backpressure
  try_send(msg: T, deadline: Instant): Result<(), ChannelError>;
  recv(): Result<T, ChannelError>;
  try_recv(deadline: Instant): Result<Option<T>, ChannelError>;
  capacity(): usize;                                    // ring slots
  close(): void;
}
```

### 11.5 Worker Pool, Scheduler & Shared State

```text
interface WorkerPool {
  spawn<T>(kind: TaskKind, task: FnOnce(SharedState) -> Result<T, TaskError>) -> TaskHandle<T>;
  reap(id: TaskId);                                      // recycle panicked worker
  pool_size_hint(): usize;                               // advisory; grows on demand
  shutdown(): Result<(), ChannelError>;
}

interface TaskHandle<T> {
  id: TaskId;
  poll(deadline: Instant) -> Poll<Result<T, TaskError>>;
  cancel(): Result<(), ChannelError>;
}

interface Scheduler {
  begin_frame(now: Instant): FrameId;
  commit(frame: FrameId) -> FrameResult;                 // drains worker IR; never blocks on channel
  spawn<T>(kind: TaskKind, task: ...) -> TaskHandle<T>;  // delegates to WorkerPool
}

interface SharedState {
  sab: SharedArrayBuffer;                                // IPC + IR staging
  device_caps: DeviceCaps;                               // immutable snapshot, never the device
  clock: MonotonicClock;                                 // ADR 016 trace correlation
}
```

### 11.6 Error Propagation Across Threads

- **Worker panic is isolated.** A trap resolves the `TaskHandle` to `Err(TaskError::Panic)`; the pool reaps the worker and recycles the slot. The main thread's frame timeline is unaffected.
- **Channel errors** (`Closed`, `Framing`, `Underrun`, `Backpressure`) propagate to the task's `Result` via `TaskError::Channel`. A `Framing` error quarantines the suspect ring slot and surfaces in the trace.
- **The render loop never blocks on a channel.** `Scheduler::commit` uses `try_recv` with a frame-budget deadline; stale or late IR is dropped and the frame proceeds, preserving cadence over worker liveness. Failed tasks surface as `Err` on their handle and as deferred error events in the unified trace (ADR 016); they never stall submission.

---

## 12. Performance & Resource Budgets

### 12.1 Startup Time Budget

First-paint startup is bounded by **streaming WASM decode** overlapped with **WebGPU pipeline precompilation** (ADR 017). The single bundled module carries the forked HarfRust payload (ADR 022), the on-demand thread runtime, and the socket-IPC shim (ADR 021); capability-scoped tree-shaking (ADR 018) holds the bundle inside its decode budget. Targets: **< 150 ms to first decoded section**, **< 400 ms to first frame**; binary **≤ 8 MB** (HarfRust + runtime + IPC shim).

### 12.2 Frame Budget

Two targets are policed: **60 fps (16.7 ms)** and **120 fps (8.3 ms)**. Per-stage trace spans are checked against `FrameBudget.stage_limits` each tick; GPU-side cost is governed by **draw-call count** (render-graph batching, ADR 001) and **fill rate** (occlusion-cull pass on the render thread). Cost is independent of scene-tree size (ADR 002 dirty-rect locality). Overrun selects a `BreachPolicy`: `Drop`, `Clamp`, or `Trace`.

### 12.3 Memory Caps

- **WASM linear memory ceiling** — hard cap, `MemoryPool` rejects growth → `LinearMemoryCeiling`.
- **SAB scene-graph budget** — instance tables/transforms/draw lists under COOP/COEP (ADR 003/021); producers apply backpressure → `SABExhausted`.
- **Glyph atlas LRU cap** — HarfRust-rasterized pages evicted least-recently-used → `GlyphAtlasEvict`.

### 12.4 GPU Resource Limits

Attachment/pool exhaustion is rejected at `begin_pass` (`AttachmentPoolEmpty`); the pipeline cache is LRU-bounded (`PipelineCacheFull`); per-stage instance buffers reject overflow (`InstanceBufferFull`), forcing a dirty-rect flush (ADR 002).

### 12.5 Profiling Hooks

A **single author-owned trace** (ADR 016) spans logic, layout, and draw; every stage opens/closes a `TraceSpan` on one frame-aligned timeline. A **frame-budget watchdog** flags overruns; **per-stage span timing** is the root-cause surface — no per-stage DevTools panel.

### 12.6 Interfaces

```text
enum BreachPolicy { Drop, Clamp, Trace }

enum BudgetBreach {
  FrameOverrun, StartupOverrun,
  LinearMemoryCeiling, SABExhausted, GlyphAtlasEvict,
  AttachmentPoolEmpty, PipelineCacheFull, InstanceBufferFull,
  DrawCallBudget, FillRateBudget,
}

enum PerfMetricKind {
  FrameTotalMs, FrameStageMs, StartupDecodeMs, StartupPipelineCompileMs,
  DrawCallCount, FillRatePx, TriangleCount,
  LinearMemoryBytes, SABBytes, GlyphAtlasBytes,
  PipelineCacheBytes, InstanceBufferBytes, GPUMemoryBytes,
}

struct FrameBudget {
  target_fps: u16,                       // 60 | 120
  target_ms: f32,                        // 16.7 | 8.3
  stage_limits: Map<StageId, f32>,       // per-span ceiling
  draw_call_cap: u32,                    // governed GPU-side
  fill_rate_cap_px: u64,
  overrun_policy: BreachPolicy,          // Drop | Clamp | Trace
}

struct ResourceBudget {
  key: ResourceKey,                      // linear_mem | sab_scene | glyph_atlas
                                          // pipeline_cache | instance_buf | attachment_pool
  limit_bytes: u64,
  owning_adr: AdrId,
  enforcement: Enforcement,              // HardCap | LRU | Backpressure | Reject
  current_bytes: u64,                    // live gauge
}

struct PerfCounter {
  kind: PerfMetricKind,
  value: f64,
  frame_id: u64,
  span: Option<TraceSpanId>,
  breach: Option<BudgetBreach>,
}

struct MemoryPool {
  kind: PoolKind,                        // Linear | SAB | Atlas | GPU
  cap_bytes: u64,
  used_bytes: u64,
  high_water_bytes: u64,
  fn reserve(n: u64) -> Result<Region, BudgetBreach>;
  fn release(region: Region) -> void;
  fn evict_lru(target_bytes: u64) -> u64;
}

struct TraceSpan {
  id: TraceSpanId,
  stage: StageId,                        // text | layout | graph | compositor | draw
  frame_id: u64,
  open_us: u64, close_us: u64,
  budget_ms: f32,                        // ceiling for this stage
  parent: Option<TraceSpanId>,
  counters: Vec<PerfCounter>,
}
```

### 12.7 Budget Table

| Resource | Limit | Owning ADR | Enforcement mechanism |
|---|---|---|---|
| First-frame startup | < 400 ms (decode + pipeline) | 017 | streaming-compile + async precompile → `StartupOverrun` |
| Binary size (HarfRust + runtime + IPC shim) | ≤ 8 MB bundled | 022 / 021 / 017 | capability-scoped tree-shaking (018); compile-time gate |
| Frame total 60 / 120 fps | 16.7 ms / 8.3 ms | 016 | per-stage span policing → `FrameOverrun` |
| Draw calls / frame | `draw_call_cap` (stage-tuned) | 001 | render-graph batching + occlusion cull |
| Fill rate / frame | `fill_rate_cap_px` | 001 | occlusion-cull pass on render thread |
| WASM linear memory | ≤ 256 MB hard ceiling | 013 / 008 | `MemoryPool` HardCap → `LinearMemoryCeiling` |
| SAB scene-graph | ≤ 64 MB | 021 / 003 | backpressure to producers → `SABExhausted` |
| Glyph atlas (LRU) | ≤ 32 MB | 022 | LRU evict → `GlyphAtlasEvict` |
| GPU attachment pool | ≤ pool_size | 001 / 003 | reject new pass → `AttachmentPoolEmpty` |
| Pipeline cache | ≤ 64 MB | 017 | LRU evict cold pipelines → `PipelineCacheFull` |
| Instance buffer / stage | ≤ per-stage budget | 001 / 002 | reject + dirty-rect flush → `InstanceBufferFull` |

Every breach emits a `TraceSpan`; integration stays observable end to end (ADR 016).

### 12.8 Open Dependencies

Three cross-section dependencies are flagged for future ADR resolution:

1. **Rendering-ABI contract** (§4 ↔ §6): the shared attachment-format and pass-boundary contract between the render-graph IR and the text stack's glyph-run IR is deferred to a future rendering-ABI ADR (per ADR 001).
2. **IME composition-event acquisition** (§6 ↔ §9): ADR 020 forbids DOM input interop, pre-empting the hidden-`<input>` approach. See `Spec_Tradeoff_Note_IME.md`.
3. **GPU determinism fallback** (§4 ↔ §14 ↔ §12): the software-rasteriser fallback for cross-vendor rendering determinism (ADR 016) is an open implementation risk; its viability determines whether the unified trace and design-tool parity guarantees hold universally.

---

## 13. Error Handling & Resilience

Error handling is structural, not bolted on: a module's exclusive ownership of its subtree (ADR 007) makes isolation a property of the object model, and the two-level type guarantee (ADR 009) keeps the most consequential class of errors out of runtime entirely. The runtime never lets an exception cross a module boundary; every subsystem failure is funneled into a typed `AlkALiveError`, observed through the unified trace (ADR 016), and recovered by a small, enumerated set of strategies.

### 13.1 Unifying `AlkALiveError` Enum

```text
enum AlkALiveError {
  CompileValidation(CompileError),  // ADR 009 — source-soundness or WASM validator failure
  ModuleLifecycle(LifecycleError),  // ADR 015 / ADR 017 — load, HMR rehydrate, pipeline precompile
  LayoutSolve(LayoutError),         // ADR 004 — solver infeasible / locality violation
  Rendering(RenderError),           // §4 — render-graph compile, attachment, draw-call
  TextShaping(TextError),           // §6 — HarfRust shaper, font, glyph-run
  Input(InputError),                // §8 — hit-test, gesture, focus writer
  Dom(DomError),                    // §9 — <title>/<meta> + SEO snapshot only
  Threading(ThreadError),           // §11 — worker IPC, socket, SharedArrayBuffer
}
```

Every `Result` channel crossing a module boundary is parametrised over `AlkALiveError`. Subsystem-specific subtypes (e.g. `LayoutError::LocalityViolation`) preserve diagnostic detail without widening the cross-module contract.

### 13.2 Error Categories

| Subsystem | Ref | Category | Typical failure |
|---|---|---|---|
| Compile / validation | ADR 009 | `CompileValidation` | type error, WASM validator reject |
| Module lifecycle | ADR 015 / 017 | `ModuleLifecycle` | decode fail, rehydrate schema mismatch, pipeline precompile fail |
| Layout solve | ADR 004 | `LayoutSolve` | infeasible constraint, cross-module locality breach |
| Rendering | §4 | `Rendering` | render-graph compile, attachment lifetime, GPU device-lost |
| Text shaping | §6 | `TextShaping` | missing glyph, shaper crash |
| Input | §8 | `Input` | hit-test mirror desync, focus-writer contention |
| DOM | §9 | `Dom` | snapshot emit failure (non-hot-path) |
| Threading | §11 | `Threading` | worker crash, socket IPC corruption |

### 13.3 Propagation

A panic in a child subtree is trapped at its owning module's boundary: the subtree is torn down deterministically, the parent receives a typed `Failure` in the affected slot, and the rest of the tree is unaffected. No exception propagates across a shared document tree; no half-mutated global state survives (§2.5). Invalidation stays bounded to the failing module's dirty rect (ADR 002) — one module's panic never triggers global reflow.

```
   panic in child        ┌─────────────────────────────────────┐
   subtree ─────────────▶│  ModuleIsolator traps at boundary   │
                         │  (ADR 007 / ADR 008)                │
                         └──────────────┬──────────────────────┘
                                        │ tear-down + dirty-rect quarantine (ADR 002)
                                        ▼
                         ┌─────────────────────────────────────┐
                         │  ErrorBoundary → typed Failure<T>   │
                         │  delivered to parent slot           │
                         └──────────────┬──────────────────────┘
                                        │ Result<_, AlkALiveError>
            ┌───────────────────────────┼────────────────────────────┐
            ▼                           ▼                            ▼
   parent recovers or           TraceRecorder span          RecoveryStrategy
   propagates Failure           (ADR 016 unified trace)     applies per category
   one level up                 no separate log sink
```

### 13.4 Recovery Mechanisms

| Trigger | Recovery |
|---|---|
| Layout solve fail | retain last-known-good layout; emit placeholder in dirty rect |
| Frame draw fail | retain last-known-good frame; skip submission; record watchdog span |
| HMR rehydrate fail (ADR 015) | fall back to full reload (option c); state lost, runtime recovers |
| Shader compile fail | swap to passthrough WGSL; pipeline precompile deferred (ADR 017) |
| Font / glyph missing | descend font fallback chain; missing-glyph box is terminal |
| Worker crash (§11) | worker marked `Dead`; scheduler reissues task; isolation intact |

### 13.5 Interfaces

```text
interface ErrorBoundary {
  trap<T>(op: () -> Result<T, AlkALiveError>, slot: SlotId) -> Result<T, Failure>
  report(failure: Failure, rect: DirtyRect) -> void
  // Failure carries: slot, AlkALiveError, dirty-rect scope, trace span id
}

interface TraceRecorder {                 // ADR 016 — single author-owned timeline
  enter(span: SpanKind, attrs: SpanAttrs) -> SpanId
  exit(span: SpanId, result: Result<_, AlkALiveError>) -> void
  watchFrame(budget_ms: f32) -> FrameBudgetEvent
  // no separate log sink; logging == querying trace spans
}

interface ModuleIsolator {                // ADR 007 / ADR 008 boundary enforcement
  quarantine(moduleId: ModuleId, rect: DirtyRect) -> void
  teardown(moduleId: ModuleId) -> TeardownReport
  emitFailure(slot: SlotId, err: AlkALiveError) -> Failure
  // guarantees: no exception escapes; dirty rect bounded (ADR 002)
}

interface RecoveryStrategy {
  category: AlkALiveError
  recover(ctx: RecoveryContext) -> RecoveryOutcome
  // RecoveryOutcome := RetainedLastKnownGood | FullReload(stateLost)
  //                 | ShaderPassthrough | FontFallback | Retried
}
```

### 13.6 Logging & Watchdog

There is no separate log sink: every error, recovery, and budget overrun is a span in the unified author-owned trace (ADR 016), correlated on a single timeline with layout and draw. A frame-budget watchdog (`TraceRecorder.watchFrame`) flags any tick breaching 16.7 ms (60 fps) or 8.3 ms (120 fps); the breach is recorded as a span, not raised as an exception, preserving the no-panic-crosses-boundary invariant.

---

## 14. Testing & Simulation

Testing rests on the same substrate as production: ADR 014's typed component contracts, ADR 016's split determinism (WASM-sandboxed layout + software-rasteriser fallback), and the `Backend` (§4.1) and `TextStack` (§6.9) traits. The test surface is therefore *contract-shaped*, not selector-shaped — no DOM, no headless browser, no GPU required.

### 14.1 Design for Testability

Three structural decisions make the runtime testable without a real browser or GPU:

- **Typed component contracts (ADR 014).** A component declares typed inputs, typed outputs, and explicit named child slots (§2.6). Tests assert on those typed values, never on DOM selectors or CSS strings — so a refactor that preserves the contract preserves the test, even across an engine swap.
- **Mockable GPU stack.** The `Backend` trait (§4.1) abstracts GPU submission. A `MockBackend` records draw calls into a typed log; a `SoftwareBackend` runs the deterministic software rasteriser. Headless tests never instantiate a `GPUDevice`.
- **Mockable text stack.** The `TextStack` trait (§6.9) abstracts shaping, measurement, and rasterisation. A `MockTextStack` returns deterministic `ShapedRun`s from a fixture table, removing HarfRust/font-fallback variance from layout tests.

### 14.2 Deterministic Rendering

Layout determinism is guaranteed by the WASM sandbox (ADR 004/008); rendering determinism is *split* (ADR 016): the deterministic GPU path runs in production, while the **software-rasteriser fallback** runs in CI. Given the same scene graph + input events + mock text fixtures, two frames are byte-identical within a raster class. Cross-vendor pixel-identical parity is *not* claimed — the fallback bounds parity to one raster class, exactly as ADR 014's caveat states.

### 14.3 Interfaces

```text
enum TestResult {
    Pass,
    Fail(FailureReport),         // contract mismatch, panic, or frame diff
    Inconclusive(SnapshotError), // determinism precondition violated
}

enum SnapshotError {
    StateNotSerialisable,        // module lacks ADR 007 owned state
    TraceGap(SpanId),            // missing span breaks replay
    RasterClassMismatch,         // software vs GPU parity not asserted
    FingerprintCollision,        // snapshot id collides with existing
}

struct SceneSnapshot {
    id:            SnapshotId,
    scene_graph:   SerialisableSceneGraph,  // ADR 007 owned state
    inputs:        Vec<InputEvent>,
    text_fixtures: TextFixtureTable,        // MockTextStack seed
    raster_class:  RasterClass,             // Software | Gpu(VendorId)
    fingerprint:   u64,                     // hash of (graph, inputs, fixtures)
}

trait MockBackend : Backend {
    fn record_submit(&mut self, ir: &RenderGraphIR);
    fn draw_log(&self)            -> &[DrawCall];
    fn assert_pass_count(&self, expected: usize) -> TestResult;
}

trait MockTextStack : TextStack {
    fn install_fixture(&mut self, text: &str, run: ShapedRun);
    fn shaped_runs(&self) -> &[ShapedRun];
}

interface ComponentTest {
    fn mount(module: ModuleId, props: TypedProps, slots: SlotMap) -> ActiveHandle;
    fn drive(handle, input: InputEvent) -> Vec<OutputEvent>;   // typed outputs (ADR 014)
    fn slot_output(handle, slot: &str) -> SlotValue;           // typed slot value
    fn expect_output(handle, expected: OutputEvent) -> TestResult;
    fn teardown(handle);
}

interface TracePlayer {
    fn load(trace: &UnifiedTrace) -> Result<(), TraceError>;   // ADR 016 trace
    fn step(&mut self) -> StepResult;                          // advance one tick
    fn seek(&mut self, tick: TickId);
    fn assert_replay(self, harness: &TestHarness) -> TestResult; // byte-identical frames
}

interface TestHarness {
    fn snapshot(scene: &Scene) -> SceneSnapshot;
    fn tick(snap: &SceneSnapshot) -> Frame;                    // via SoftwareBackend
    fn assert_frame(snap: &SceneSnapshot, expected: &Frame) -> TestResult;
    fn replay(trace: &UnifiedTrace) -> TestResult;
    // composed backends — all default to mock/software; no GPU required
    backend: MockBackend,
    text:    MockTextStack,
    raster:  SoftwareBackend,
}
```

### 14.4 Integration Test Harness

`TestHarness` is the integration entry point: `snapshot → tick → assert_frame`. It wires a `MockBackend`, `MockTextStack`, and `SoftwareBackend` together so a single tick produces a deterministic `Frame` without touching the GPU. The `TracePlayer` replays an ADR 016 unified trace tick-by-tick; replayed frames must match recorded frames byte-for-byte, else the harness returns `Inconclusive(SnapshotError::TraceGap)`. A `SceneSnapshot` is the immutable replay unit — its `fingerprint` keys the cache so the same scene never re-rasterises.

### 14.5 Component Testing

`ComponentTest` replaces DOM-selector e2e assertions. A test mounts a module with typed props and a slot map, drives it with typed `InputEvent`s, and asserts on typed `OutputEvent`s and slot values — the same contract surface the compiler checks (§2.6). A failing child surfaces as a typed `Failure` value in the affected slot (§2.5), asserted directly rather than inferred from a missing DOM node. Because contracts are semantic, test suites survive refactors and engine swaps where selector-based suites would shatter.

---

## Glossary

| Term | Definition |
|---|---|
| **Hot path** | The per-frame call graph: layout, composition, draw-call emission, hit-testing, input dispatch, text measurement/rasterization. Must never cross the WASM↔DOM boundary (ADR 013). |
| **Render-Object Tree** | The single WASM-resident, module-owned tree of render objects; a component *is* a subtree owning its style, layout, and drawing (ADR 007). |
| **Render-Graph IR** | The atomic rendering primitive: a directed graph of passes, attachments, draw calls, and an occlusion-cull pass, submitted immutably to the compositor (ADR 001). |
| **Owned Subtree** | A render-object subtree whose lifecycle (construct/attach/visible/destroy) is controlled by exactly one module; no external observer can retain it (ADR 007). |
| **Layout Locality** | The solver-enforced guarantee that no cross-module flex/percentage dependency re-introduces global reflow; violations are rejected at solve time or fall back to a documented global pass (ADR 002). |
| **Socket IPC** | Typed, structured inter-thread channel over `SharedArrayBuffer` for WASM↔WASM communication between the main thread and on-demand workers (ADR 021). |
| **Metadata-Only DOM** | The host-DOM surface restricted to `<title>`, `<meta>`, and a static SEO snapshot, with no UI/text/a11y/navigation/input interop (ADR 020). |
| **Dirty Rect** | A per-module, per-object invalidation subset bounding per-frame work to the changed region rather than the full tree (ADR 002). |
| **MeasuredRun** | The synchronous text-flow measurement contract (ADR 004) consumed by the layout solver and implemented by the text stack (ADR 022); carries glyph-run metrics. |
| **Split Determinism** | ADR 016's guarantee that layout determinism is WASM-sandboxed while rendering determinism requires a deterministic GPU path or software-rasteriser fallback. |
| **Capability-Scoped Import** | An import granted least-privilege authority (filesystem/network/clock scope) per ADR 018, replacing npm's transitive trust. |
| **Two-Level Type Verification** | ADR 009's guarantee: the compiler proves source-level soundness; WASM validates compiled well-formedness (structural only). |

---

## Source Artifacts

| Artifact | Version | Path | Role |
|---|---|---|---|
| Problem Catalog | 1.0 | `docs/PROBLEM_CATALOG.md` | Peer-reviewed evidence base (50 refs, 45 problem entries P1.1–P10.4) |
| Rough Draft | 1.0 | `docs/ROUGH_DRAFT.md` | Problem → Goal → Solution → Integration per cluster |
| Fine Draft | 1.0 | `docs/FINE_DRAFT.md` | 12-section system design blueprint |
| ADR (consolidated) | 1.0 | `docs/adr/ADR.md` | 22 Architectural Decision Records (ADR 001–022), all Status: Proposed |
| Decision Alternatives | resolved | `docs/adr/Decision_Alternatives_*.md` | 4 files, all superseded by ADRs 019–022 (retained for historical context) |
| Verification Log | 1.0 | `docs/VERIFICATION_LOG.md` | Evidence trail for the catalog's 50 references |
| IME Trade-off Note | 1.0 | `docs/adr/Spec_Tradeoff_Note_IME.md` | Open dependency: IME composition-event acquisition (ADR 020 conflict) |

---

*End of Detailed Software Specification. This document is the definitive implementation-ready blueprint for the AlkALive runtime and UI framework. Implementation teams should begin from §2 (Language Specification) and §3 (Runtime Architecture) as the foundational layers, with §4 (Rendering Engine) and §6 (Text Rendering) as the critical-path subsystems. Three open dependencies (rendering-ABI, IME, GPU determinism) are flagged in §12.8 for future ADR resolution.*

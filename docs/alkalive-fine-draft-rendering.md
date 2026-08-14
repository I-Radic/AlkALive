# AlkALive Fine Draft — Rendering / Runtime Gaps (Wave 2)

> **Read [`docs/alkalive-wave-00-audit.md`](alkalive-wave-00-audit.md) and
> [`docs/alkalive-wave-01-bugfixes.md`](alkalive-wave-01-bugfixes.md) first.**
>
> Task ID: **2** — Wave 2 Fine Draft — Rendering/Runtime Gaps.
>
> This document is a **design-only fine draft**. It specifies exact struct
> shapes, function signatures, execution flows, and file-level integration
> points for three gaps that the Wave 0 audit identified as **Major**:
>
> | Gap | ADR | Current-state headline |
> |-----|-----|------------------------|
> | 6 | ADR-001 | No render-graph IR — the renderer issues GL calls in a fixed `PassKind`-driven sequence (`backend-wgpu/src/lib.rs:901-1034`) |
> | 7 | ADR-006 | GLSL ES 3.00 shaders hardcoded as Rust string constants (`backend-wgpu/src/lib.rs:186-289`) — no WGSL, no `wgpu` |
> | 8 | ADR-003 | Single-threaded WASM only — no Web Worker, no `SharedArrayBuffer`, no COOP/COEP, no compositor (`runtime-wasm/src/lib.rs:142-158`) |
>
> The three gaps are tightly interlocked:
>
> - Gap 6 produces the `RenderGraph` IR that Gap 7's `wgpu` backend consumes
>   and that Gap 8's render thread submits.
> - Gap 7 supplies WGSL pipelines that Gap 6's `DrawCall`s reference.
> - Gap 8 supplies the single `GPUDevice` owner that Gap 6's `compile()` runs
>   on and that Gap 7's `wgpu::Device` lives on.
>
> Section §5 (cross-gap dependency resolution) defines the mandatory build
> order; sections §1-§3 specify each gap in isolation; section §4 documents
> the shared rendering ABI that all three gaps must agree on (per the
> ADR-001 ↔ ADR-003 cross-reference in `docs/adr/ADR.md:65`).

---

## Table of Contents

- §0 Orientation
- §1 Gap 6 — Render-Graph IR (ADR-001)
- §2 Gap 7 — WGSL Shaders (ADR-006)
- §3 Gap 8 — Single-GPU-Device + SAB/COOP-COEP (ADR-003)
- §4 Shared Rendering ABI (Gap 6 ↔ Gap 7 ↔ Gap 8)
- §5 Cross-Gap Dependency Resolution and Build Order
- §6 Implementation Sequencing and Effort Estimates
- §7 Consolidated Open Questions
- §8 Appendix A — Per-File Impact Summary
- §9 Appendix B — Final Crate Dependency Graph
- §10 DoD Checklist

---

## §0 Orientation

### 0.1 Three-Gap Snapshot

| Attribute | Gap 6 (Render-Graph IR) | Gap 7 (WGSL Shaders) | Gap 8 (Single-GPU-Device + SAB) |
|-----------|-------------------------|----------------------|----------------------------------|
| ADR | ADR-001 | ADR-006 | ADR-003 + ADR-021 |
| ADR confidence | High | High | Medium (COOP/COEP risk) |
| Audit severity | Major | Major | Major |
| Currently implemented? | No — `alkalive-render` crate has the abstract types but the runtime does not consume them | No — raw WebGL2/GLSL via `web-sys` | No — single-threaded main thread |
| Existing partial scaffolding | `crates/alkalive-render/src/lib.rs:35-267` defines `RenderGraph`/`RenderPass`/`DrawCall`/`Attachment`; `compile()` at line 447 | `crates/alkalive-backend-wgpu/Cargo.toml:51` keeps a `wgpu-backend` feature flag (unused) | None — runtime has no worker spawn, no IPC, no SAB |
| New external deps | None (uses existing `alkalive-render`) | `wgpu = "23"` (~50 transitive deps — see §2.5 risk) | `web-sys::Worker`, `js_sys::SharedArrayBuffer`, `wasm-bindgen-rayon` (optional, only for on-demand worker pool) |
| New crates | None | None (migrates `alkalive-backend-wgpu` in place) | `alkalive-render-worker` (new cdylib + JS shim) |
| Estimated LOC added | ~1,250 | ~900 (rewrite of WebGL2 paths) + ~150 WGSL | ~1,400 |
| Estimated tests added | ~40 | ~25 | ~30 |
| Estimated effort | 8-10 days | 6-8 days | 10-14 days |

### 0.2 Why These Three Gaps Form a Cohesive Wave

The audit (`docs/alkalive-wave-00-audit.md:213-216`) tabulates all three as
**Major** severity. ADR-001 (line 65 of `docs/adr/ADR.md`) explicitly states
"the compositor (ADR 003) consumes the compiled graph output and both ADRs
must agree on a shared attachment-format and pass-boundary contract (to be
specified in a future rendering-ABI ADR)." That future rendering-ABI ADR is
**this document's §4**.

ADR-006 (line 173) cross-references ADR-001 ("the render-graph schedules the
paint passes") and ADR-005 ("owned-state uniforms" — the WGSL shader consumes
the per-instance style fields). Without WGSL, the render-graph's `DrawCall`
has no `pipeline: PipelineHandle` to reference (the existing
`crates/alkalive-render/src/lib.rs:296-299` `PipelineHandle` is opaque and
backed by no concrete pipeline cache).

ADR-021 (line 685) settles the threading model: main thread + on-demand WASM
workers via socket IPC. ADR-003 (line 99) further specifies the render thread
is "the persistent GPUDevice-owner thread ... either the main thread or a
dedicated non-on-demand worker." Gap 8 chooses the **dedicated worker** option
because (a) it lets the main thread stay responsive to input/composition
events while the GPU is busy, (b) it isolates context-loss recovery to the
worker, and (c) it matches the production deployment shape (cross-origin
isolation headers) without which `SharedArrayBuffer` is unavailable.

### 0.3 The Hello World Scene as the Design Target

Throughout this document, design choices are validated against the canonical
Hello World scene (`examples/hello.alk`), which compiles to two `NodeIR`s: a
`Text` node ("Hello World!", golden, 64px, 0.5 rad/s Y-axis rotation) and an
`InputField` node (placeholder "Type here..."). The current renderer
(`backend-wgpu/src/lib.rs:960-1033`) issues exactly five `PassKind`-tagged
passes per frame:

1. `Clear` → `gl.clearColor + gl.clear(COLOR_BUFFER_BIT)`
2. `InputFieldBackground` → `draw_rect_filled(...)` (rect shader, 4 verts)
3. `InputFieldBorder` → `draw_rect_outline(...)` (rect shader × 4 edges)
4. `TitleText` → `gl.drawArrays(TRIANGLES, 0, title_vertex_count)` (text shader)
5. `InputText` → `gl.drawArrays(TRIANGLES, input_vertex_start, input_vertex_count)` (text shader)

Each gap's design must preserve the visible output of this scene while
migrating the underlying mechanism.

---

## §1 Gap 6 — Render-Graph IR (ADR-001)

### 6.1 Current State (with file:line evidence)

The current renderer is **not** render-graph-driven; it is **`PassKind`-enum
driven**. The `alkalive-render` crate defines an abstract `RenderGraph` IR
(`crates/alkalive-render/src/lib.rs:254-267`), but the runtime and backend
do not use it. Instead:

**Evidence 1 — `WgpuRenderer::render_frame_internal`**
(`crates/alkalive-backend-wgpu/src/lib.rs:901-1034`):

```rust
fn render_frame_internal(
    &mut self,
    text_scene: &TextSceneData,
    schedule: &alkalive_compiler::ScheduleIR,
    time: f32,
    _dirty_passes: Option<&[usize]>,
) {
    use alkalive_compiler::PassKind;
    // ...
    for &pass_idx in &schedule.pass_order {
        let pass = match schedule.passes.get(pass_idx) { Some(p) => p, None => continue };
        match pass.kind {
            PassKind::Clear => { /* gl.clear_color + gl.clear */ }
            PassKind::InputFieldBackground => { /* draw_rect_filled */ }
            PassKind::InputFieldBorder => { /* draw_rect_outline */ }
            PassKind::TitleText => { /* gl.use_program + gl.drawArrays */ }
            PassKind::InputText => { /* gl.use_program + gl.drawArrays */ }
        }
    }
}
```

This is **data-driven dispatch over an enum**, not render-graph compilation.
The `PassKind` enum is closed (five variants); adding a new pass type
requires modifying the renderer source. There is no concept of:

- **Attachments** — the renderer always targets the canvas's default
  framebuffer; there is no off-screen texture, no depth buffer, no MSAA
  resolve.
- **Dependencies** — the schedule's `pass_order` is a flat `Vec<usize>`; the
  renderer executes it linearly. The ADR-025 dirty-pass mechanism
  (`backend-wgpu/src/lib.rs:864-887`) uses `dirty_passes: &[usize]` only for
  the empty/non-empty check.
- **Draw-call abstraction** — there is no `DrawCall` type in
  `alkalive-backend-wgpu`; each `PassKind` arm issues GL calls inline.
- **Reordering / batching / occlusion culling** — none exist. The renderer
  cannot merge two `TitleText` passes into one `drawArrays` call, nor skip an
  occluded draw.

**Evidence 2 — `ScheduleIR` is a higher-level author-facing schedule**
(`crates/alkalive-compiler/src/schedule.rs`, per technical-specification §3.5
lines 332-339):

The `ScheduleIR` is **per-scene and declarative** (text nodes → one pass,
input-field → one pass, etc.). It is not the cross-scene GPU-layer IR that
ADR-001 specifies. The technical specification explicitly notes (line 337):
"a subsequent (currently unspecified) lowering step converts `ScheduleIR`
into `alkalive-render::RenderGraph` for the runtime's `RenderLoop::submit()`."

**Gap 6 is that lowering step**, plus the renderer-side consumption of the
lowered `RenderGraph`.

**Evidence 3 — the `alkalive-render` crate is dormant.** `cargo` builds it
(1495 LOC), `compile()` is implemented (lines 447-555), but no other crate
depends on `alkalive-render`. The technical specification (line 314)
acknowledges this: the IR types are "abstract — no concrete impl yet."

### 6.2 Problem Statement

ADR-001 (line 55) requires: *"render-graph IR — passes, attachments, draw
calls, plus a dedicated occlusion-cull pass — as the atomic rendering
primitive. Authors declare a directed graph of passes; the runtime compiles,
reorders, and batches it into optimal GPU command streams."*

The current `PassKind`-driven dispatch:

1. **Cannot express multi-target rendering** (e.g. a shadow-map pass whose
   output depth attachment feeds a lighting pass). Every pass writes to the
   same default framebuffer.
2. **Cannot express barriers.** Without `Attachment.lifetime`, there is no
   way to insert a `gl.memoryBarrier` (WebGL2) or `wgpu::Operations::load`
   between a producer pass and a consumer pass.
3. **Cannot be reordered.** The renderer trusts `schedule.pass_order`
   verbatim. A future occlusion-cull pass that wants to run early (to skip
   occluded draws) has no insertion point.
4. **Cannot be batched.** Two `TitleText` passes from different scene
   graphs cannot merge into one `drawArrays` call because the renderer's
   state machine has no concept of "this pass and the previous pass share
   a pipeline + bind-group topology."
5. **Cannot be merged across scenes.** ADR-003 (line 99) requires the
   render thread to "merge" graphs "from all scene graphs (UI, particles,
   world, overlays)." The current single-scene `ScheduleIR` cannot be merged
   with anything.
6. **Cannot be transported across threads.** `ScheduleIR` is a Rust struct
   in the WASM linear memory of the main thread; there is no serialization
   to a flat buffer that could be `postMessage`-d to a worker.

### 6.3 Why It's Required (ADR Reference)

ADR-001 (decision, `docs/adr/ADR.md:55`): *"Adopt render-graph IR — passes,
attachments, draw calls, plus a dedicated occlusion-cull pass — as the
atomic rendering primitive, replacing the retained DOM box-model tree."*

ADR-001 (consequences, line 61): *"Draw calls become first-class, enabling
fine-grained batching, lazy barrier insertion, and author-directed occlusion
culling. Stage reordering is unconstrained by box paint order, allowing
depth-aware and tile-based optimization."*

ADR-001 (cross-references, line 65): *"Graph compilation (reordering/
batching/occlusion-cull) is scheduled by ADR-021's main-thread + on-demand
WASM-worker pool ... and committed through ADR-003's single-GPUDevice
compositor; the occlusion-cull pass may run on any worker but serializes
against the compositor-wide depth/visibility buffer."*

### 6.4 Relationship to Existing Runtime/Renderer

| Component | Current role | Role after Gap 6 |
|-----------|--------------|-------------------|
| `alkalive-compiler::ScheduleIR` | Per-scene, declarative, author-facing | **Unchanged** — still the author-facing layer |
| `alkalive-render::RenderGraph` | Abstract types, dormant | **Activated** — the cross-scene GPU-layer IR |
| `alkalive-render::compile()` | Implemented but unused | **Activated** — merges graphs, reorders, inserts barriers |
| `alkalive-backend-wgpu::WgpuRenderer::render_frame` | Takes `(&TextSceneData, &ScheduleIR, time)` | Takes `(&RenderGraph, time)` — graph-driven dispatch |
| `alkalive-runtime-wasm::Runtime` | Owns `schedule: ScheduleIR` | Owns `graph: RenderGraph` (lowered once at startup, updated when scene changes) |
| `alkalive-runtime-wasm::Runtime::frame_closure` | Calls `render_frame(&scene, &schedule, time)` | Calls `render_frame(&graph, time)` |

The `ScheduleIR` is preserved verbatim (it is the author-facing layer per
ADR-024). Gap 6 adds the **lowering step** `ScheduleIR + TextSceneData →
RenderGraph` and rewires the renderer to consume `RenderGraph`.

### 6.5 Proposed Design

#### 6.5.1 Layered Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  .alk source (examples/hello.alk)                                   │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ alkalive_compiler::compile_with_deps
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  ScheduledScene { algorithm: AlgorithmIR, schedule: ScheduleIR }    │
│  + DependencyGraph (ADR-025)                                        │
│  -- per-scene, declarative, author-facing --                        │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ schedule_lowering (exists)
                                   │ + NEW: schedule_to_render_graph
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  RenderGraph (alkalive-render crate)                                │
│  { passes: Vec<RenderPass>, attachments, draw_calls, edges,         │
│    occlusion_cull, source_module }                                  │
│  -- cross-scene, GPU-layer, executable --                           │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ alkalive_render::compile (exists)
                                   │ merges + topo-sorts + (future: batches)
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  CompiledGraph { sorted_passes: Vec<PassId>, ... }                  │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ WgpuBackend::encode + submit
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  GPU command stream (wgpu::Queue or WebGL2 draw calls)              │
└─────────────────────────────────────────────────────────────────────┘
```

#### 6.5.2 The Existing `RenderGraph` Type (No Change)

The `alkalive-render` crate already defines the IR
(`crates/alkalive-render/src/lib.rs:254-267`):

```rust
pub struct RenderGraph {
    pub passes: Box<[RenderPass]>,
    pub attachments: Box<[Attachment]>,
    pub draw_calls: Box<[DrawCall]>,
    pub occlusion_cull: OcclusionCullPass,
    pub edges: Box<[(PassId, PassId)]>,
    pub source_module: ModuleId,
}
```

`RenderPass` (lines 200-213):

```rust
pub struct RenderPass {
    pub id: PassId,
    pub kind: PassType,                         // Render | Compute | CopyTransfer | OcclusionCull
    pub color_attachments: Box<[AttachmentId]>,
    pub depth_stencil: Option<AttachmentId>,
    pub draw_calls: Box<[DrawCallId]>,
    pub dependencies: Box<[PassId]>,
}
```

`Attachment` (lines 183-196):

```rust
pub struct Attachment {
    pub id: AttachmentId,
    pub format: AttachmentFormat,               // Bgra8Unorm | Rgba8UnormSrgb | Rgba16Float | Depth24Plus | ...
    pub size: ExtentOrRelative,                 // absolute pixels or [0,1] relative
    pub samples: SampleCount,                   // X1 | X2 | X4 | X8
    pub lifetime: (PassId, PassId),             // [producer, last_consumer]
    pub clear_op: ClearOp,                      // Clear | Load | DontCare
}
```

These types are **adopted as-is**. Gap 6 does not modify the IR shape — it
populates and consumes it.

#### 6.5.3 The Existing `DrawCall` Type — Extended

The existing `DrawCall` (`crates/alkalive-render/src/lib.rs:230-243`) is
**low-level** (it references a compiled `PipelineHandle` and bound
resources):

```rust
pub struct DrawCall {
    pub pipeline: PipelineHandle,
    pub vertices: VertexBinding,
    pub indices: Option<IndexBinding>,
    pub bindings: Box<[BindGroup]>,
    pub instances: Range<u32>,
    pub scissor: Option<DirtyRect>,
}
```

`VertexBinding`, `IndexBinding`, `BindGroup` are currently empty marker
structs (`pub struct VertexBinding;` etc. at lines 217, 221, 226). Gap 6
**populates them** with concrete shapes (see §6.5.4 below).

In addition, Gap 6 introduces a **higher-level author-facing `DrawCallKind`
enum** that the `schedule_to_render_graph` lowering produces. The enum is
the moral equivalent of the task brief's `DrawText { text, color, ... }`,
`DrawRect { bounds, color }`, `DrawCustom { shader, vertices }`. It is
**separate from** the low-level `DrawCall` struct because (a) the enum is
what authors (or, today, the schedule lowering) reason about; (b) the
struct is what the GPU backend consumes after pipeline compilation. The
lowering `DrawCallKind → DrawCall` is part of `schedule_to_render_graph`.

```rust
// In crates/alkalive-render/src/lib.rs (NEW — added by Gap 6)

/// High-level, author-facing draw-call descriptor.
///
/// Produced by `schedule_to_render_graph` (one per `PassKind` arm in the
/// source `ScheduleIR`). Lowered to a low-level `DrawCall` (with a
/// resolved `PipelineHandle` and bound resources) by the same function.
///
/// This is the moral equivalent of CSS's "property list" — authors (or
/// the schedule lowering) declare *what* to draw; the lowering decides
/// *how* (which pipeline, which vertex buffer, which bind group).
#[derive(Debug, Clone)]
pub enum DrawCallKind {
    /// Clear the entire attachment to a solid color.
    Clear {
        /// RGBA, normalized 0.0–1.0.
        color: [f32; 4],
    },

    /// Draw a solid-color filled rectangle with proper alpha blending.
    /// Replaces the old scissor+clear hack (Wave 1 C2 fix).
    DrawRect {
        /// Pixel-space bounds (x, y, w, h). Y-down, origin at top-left.
        bounds: DirtyRect,
        /// RGBA, normalized.
        color: [f32; 4],
    },

    /// Draw a shaped text run via the glyph atlas.
    /// `glyph_run_id` indexes into the renderer's per-frame glyph-run table.
    DrawText {
        /// Glyph-run ID (resolved to a vertex buffer + atlas slice by the
        /// renderer's per-frame `GlyphRunTable`).
        glyph_run_id: GlyphRunId,
        /// RGBA, normalized.
        color: [f32; 4],
        /// Y-axis rotation angle in radians (0.0 = no rotation).
        rotation: f32,
        /// Canvas size in physical pixels (uniform `canvas_size`).
        canvas_size: [f32; 2],
    },

    /// Draw an author-supplied custom shader (ADR-006 future).
    /// Today this variant is unused — it exists so the lowering has a
    /// stable shape when ADR-006's author-supplied WGSL lands.
    DrawCustom {
        /// Hash of the WGSL shader source (looked up in the PipelineCache).
        shader_hash: u64,
        /// Vertex data (raw bytes; layout declared by the shader).
        vertices: Vec<u8>,
        /// Bind-group data (raw bytes; layout declared by the shader).
        uniforms: Vec<u8>,
        /// Topology (Triangles / TriangleStrip / Lines / LineStrip).
        topology: Topology,
        /// Vertex count.
        vertex_count: u32,
    },
}

/// Opaque identifier for a glyph run in the renderer's per-frame table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GlyphRunId(pub u32);

/// Primitive topology for a custom draw call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    Triangles,
    TriangleStrip,
    Lines,
    LineStrip,
}
```

#### 6.5.4 Populating `VertexBinding`, `IndexBinding`, `BindGroup`

The existing empty marker structs become concrete:

```rust
// In crates/alkalive-render/src/lib.rs (REPLACES the existing empty structs)

/// Vertex input binding (replaces the empty `VertexBinding;` at line 217).
#[derive(Debug, Clone, Default)]
pub struct VertexBinding {
    /// Opaque handle to a backend-allocated vertex buffer.
    /// On wasm32+wgpu: the `wgpu::Buffer` index in the renderer's
    /// `BufferTable`. On wasm32+raw-WebGL2 (legacy fallback): the
    /// `WebGlBuffer` index in the renderer's buffer table.
    pub buffer_id: BufferId,
    /// Byte offset into the buffer.
    pub offset: u64,
    /// Vertex stride in bytes.
    pub stride: u32,
    /// Vertex attribute descriptors (location, format, byte_offset).
    pub attributes: Box<[VertexAttribute]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BufferId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexAttribute {
    pub location: u32,
    pub format: VertexFormat,
    pub byte_offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    Float32x2,
    Float32x4,
    Uint8x4Norm,
}

/// Index input binding (replaces the empty `IndexBinding;` at line 221).
#[derive(Debug, Clone, Default)]
pub struct IndexBinding {
    pub buffer_id: BufferId,
    pub offset: u64,
    pub index_format: IndexFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFormat {
    Uint16,
    Uint32,
}

/// A bound resource group (replaces the empty `BindGroup;` at line 226).
///
/// Per ADR-005: per-instance object-owned property state bound at
/// construction. Today the runtime owns one BindGroup per DrawCall (the
/// "uniforms" — rotation, canvas_size, time, text_color, glyph_texture
/// sampler, rect bounds, rect color, rect canvas).
#[derive(Debug, Clone, Default)]
pub struct BindGroup {
    /// Layout hash (used as part of the pipeline-cache key).
    pub layout_hash: u64,
    /// Opaque handle to a backend-allocated uniform buffer.
    pub uniform_buffer: Option<BufferId>,
    /// Opaque handles to bound textures (e.g. the glyph atlas).
    pub textures: Box<[TextureId]>,
    /// Samplers corresponding 1:1 with `textures`.
    pub samplers: Box<[SamplerId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextureId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SamplerId(pub u32);
```

#### 6.5.5 The `schedule_to_render_graph` Lowering Function

This is the **new function** specified by the technical specification's
open recommendation R3 ("calls for a future rendering-ABI ADR" —
`docs/technical-specification.md:552`). It lives in the `alkalive-render`
crate (so it can use the IR types directly) but depends on
`alkalive-compiler` for the input types.

```rust
// In crates/alkalive-render/src/lib.rs (NEW function, ~250 LOC)

/// Lower a per-scene `ScheduleIR` + `TextSceneData` to a cross-scene
/// `RenderGraph`.
///
/// This is the "schedule_lowering" step referenced in the technical
/// specification (§3.5, line 337) as "currently unspecified." Gap 6
/// specifies it.
///
/// # Inputs
///
/// - `scheduled`: the per-scene algorithm + schedule (from
///   `alkalive_compiler::compile_with_deps`).
/// - `scene`: the per-frame scene data (text, colors, rotation speed).
///   Today this is `TextSceneData`; in the future it will be the
///   render-object tree (ADR-007).
/// - `canvas_size`: physical pixel dimensions (for attachment sizing).
///
/// # Output
///
/// A `RenderGraph` with:
/// - 1 color attachment (the canvas's swapchain texture, format
///   `Bgra8Unorm` or `Rgba8UnormSrgb` — see §4).
/// - N passes, one per entry in `scheduled.schedule.pass_order`.
/// - M draw calls, one per `DrawCallKind` produced per pass.
/// - Barrier edges from each pass to its successor (linear chain today;
///   future: dependency-driven DAG per `pass.dependencies`).
///
/// # Allocation Strategy
///
/// Pass IDs and attachment IDs are allocated from a per-graph counter
/// starting at 0. The `compile()` merger remaps IDs when merging multiple
/// graphs (per ADR-003's "merge" step).
pub fn schedule_to_render_graph(
    scheduled: &alkalive_compiler::ScheduledScene,
    scene: &alkalive_backend_wgpu::TextSceneData,
    canvas_size: (u32, u32),
) -> RenderGraph {
    use alkalive_compiler::PassKind;

    let mut passes: Vec<RenderPass> = Vec::new();
    let mut draw_calls: Vec<DrawCall> = Vec::new();
    let mut edges: Vec<(PassId, PassId)> = Vec::new();

    // 1. One color attachment: the canvas swapchain texture.
    let canvas_attachment_id = AttachmentId(0);
    let attachments = vec![Attachment {
        id: canvas_attachment_id,
        format: AttachmentFormat::Bgra8Unorm, // see §4
        size: ExtentOrRelative {
            absolute: Some(canvas_size),
            relative: None,
        },
        samples: SampleCount::X1,
        // Lifetime is [first_pass, last_pass]; updated after the pass loop.
        lifetime: (PassId(0), PassId(0)),
        clear_op: ClearOp::Clear,
    }];

    // 2. One pass per schedule entry.
    let mut prev_pass_id: Option<PassId> = None;
    for (i, &pass_idx) in scheduled.schedule.pass_order.iter().enumerate() {
        let pass = match scheduled.schedule.passes.get(pass_idx) {
            Some(p) => p,
            None => continue,
        };

        let pass_id = PassId(i as u64);
        let draw_call_id = DrawCallId(i as u64);

        // Lower PassKind → DrawCallKind → DrawCall.
        let kind = lower_pass_kind(pass.kind, scene, &scheduled.algorithm);
        let pipeline = pipeline_for_kind(&kind);
        let call = DrawCall {
            pipeline,
            vertices: vertex_binding_for_kind(&kind),
            indices: None,
            bindings: vec![bind_group_for_kind(&kind)].into_boxed_slice(),
            instances: 0..1,
            scissor: scissor_for_kind(&kind),
        };
        draw_calls.push(call);

        passes.push(RenderPass {
            id: pass_id,
            kind: PassType::Render,
            color_attachments: vec![canvas_attachment_id].into_boxed_slice(),
            depth_stencil: None,
            draw_calls: vec![draw_call_id].into_boxed_slice(),
            dependencies: prev_pass_id.iter().copied().collect::<Vec<_>>().into_boxed_slice(),
        });

        // Edge: prev → this (linear chain today; future: DAG).
        if let Some(prev) = prev_pass_id {
            edges.push((prev, pass_id));
        }
        prev_pass_id = Some(pass_id);
    }

    // 3. Update the attachment's lifetime to span all passes.
    let last_pass = PassId(passes.len().saturating_sub(1) as u64);
    let mut attachments = attachments;
    attachments[0].lifetime = (PassId(0), last_pass);

    RenderGraph {
        passes: passes.into_boxed_slice(),
        attachments: attachments.into_boxed_slice(),
        draw_calls: draw_calls.into_boxed_slice(),
        occlusion_cull: OcclusionCullPass,
        edges: edges.into_boxed_slice(),
        source_module: ModuleId(0), // single-module today
    }
}

/// Lower one `PassKind` to one `DrawCallKind`.
fn lower_pass_kind(
    kind: alkalive_compiler::PassKind,
    scene: &alkalive_backend_wgpu::TextSceneData,
    algorithm: &alkalive_compiler::AlgorithmIR,
) -> DrawCallKind {
    use alkalive_compiler::PassKind;
    match kind {
        PassKind::Clear => {
            let (r, g, b) = scene.background_normalized();
            DrawCallKind::Clear { color: [r, g, b, 1.0] }
        }
        PassKind::InputFieldBackground => {
            // Input field bounds are computed by the renderer at upload time
            // (backend-wgpu/src/lib.rs:1245-1249). For the lowering, we
            // emit a placeholder bounds (0,0,0,0); the renderer overwrites
            // it from its cached `input_field_bounds` field at draw time.
            //
            // This is a known wart: the lowering should receive the input
            // field bounds from layout. Today layout is hardcoded in the
            // renderer. ADR-004 (pluggable constraint-solver layout) will
            // move this out of the renderer.
            DrawCallKind::DrawRect {
                bounds: DirtyRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
                color: [0.05, 0.05, 0.08, 0.9],
            }
        }
        PassKind::InputFieldBorder => {
            DrawCallKind::DrawRect {
                bounds: DirtyRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
                color: [0.8, 0.65, 0.0, 0.8],
            }
        }
        PassKind::TitleText => {
            let (r, g, b, a) = scene.text_color;
            DrawCallKind::DrawText {
                glyph_run_id: GlyphRunId(0), // title run
                color: [r, g, b, a],
                rotation: scene.rotation_speed, // multiplied by time in the shader
                canvas_size: [0.0, 0.0], // filled in by the renderer
            }
        }
        PassKind::InputText => {
            // Placeholder color if input is empty; white if typed.
            let is_placeholder = scene.input_text.is_empty();
            let color = if is_placeholder {
                [0.35, 0.35, 0.4, 1.0]
            } else {
                [0.9, 0.9, 0.95, 1.0]
            };
            DrawCallKind::DrawText {
                glyph_run_id: GlyphRunId(1), // input run
                color,
                rotation: 0.0,
                canvas_size: [0.0, 0.0],
            }
        }
    }
}

/// Resolve a `DrawCallKind` to its pipeline handle.
///
/// Pipelines are pre-allocated at renderer init time (see §6.5.7).
fn pipeline_for_kind(kind: &DrawCallKind) -> PipelineHandle {
    match kind {
        DrawCallKind::Clear { .. } => PIPELINE_CLEAR,
        DrawCallKind::DrawRect { .. } => PIPELINE_RECT,
        DrawCallKind::DrawText { .. } => PIPELINE_TEXT,
        DrawCallKind::DrawCustom { shader_hash, .. } => {
            // Look up the custom pipeline in the cache by shader_hash.
            // Today this branch is unreachable (no custom shaders yet).
            PipelineHandle(*shader_hash)
        }
    }
}

// Pipeline handle constants (allocated at renderer init).
pub const PIPELINE_CLEAR: PipelineHandle = PipelineHandle(0);
pub const PIPELINE_RECT: PipelineHandle = PipelineHandle(1);
pub const PIPELINE_TEXT: PipelineHandle = PipelineHandle(2);
```

#### 6.5.6 The Renderer's New `render_frame` Signature

```rust
// In crates/alkalive-backend-wgpu/src/lib.rs (REPLACES the existing
// render_frame / render_frame_with_dirty / render_frame_internal trio
// at lines 826-1034).

impl WgpuRenderer {
    /// Render one frame from a compiled `RenderGraph`.
    ///
    /// This is the ADR-001 entry point. The graph is already lowered from
    /// `ScheduleIR` (via `alkalive_render::schedule_to_render_graph`) and
    /// compiled (via `alkalive_render::compile`). The renderer executes
    /// each pass in `compiled.sorted_passes` order, issuing one draw call
    /// per `DrawCall` referenced by each pass.
    ///
    /// # Arguments
    ///
    /// * `graph` — the lowered (but not yet compiled) render graph. The
    ///   renderer will call `alkalive_render::compile(&[graph.clone()], &[], &Default::default())`
    ///   internally to get the topologically-sorted pass order. (For the
    ///   single-scene case today, the sort is a no-op because the edges
    ///   form a linear chain.)
    /// * `time` — the animation time (drives text rotation).
    pub fn render_frame(&mut self, graph: &RenderGraph, time: f32) {
        let compiled = match alkalive_render::compile(
            std::slice::from_ref(graph),
            &[],
            &Default::default(),
        ) {
            Ok(c) => c,
            Err(e) => {
                web_sys::console::error_1(
                    &format!("AlkALive render-graph compile failed: {:?}", e).into(),
                );
                return;
            }
        };
        self.render_compiled(graph, &compiled, time);
    }

    /// Render a pre-compiled graph (skip the `compile()` call). Used by
    /// the dirty-pass fast path (ADR-025) where the graph topology is
    /// unchanged and only specific draw-call parameters need updating.
    pub fn render_compiled(
        &mut self,
        graph: &RenderGraph,
        compiled: &CompiledGraph,
        time: f32,
    ) {
        // Per-frame glyph-run upload (the existing `upload_text_atlas`
        // stays — it uploads the atlas + builds the vertex buffer).
        // The graph's DrawCallKind::DrawText entries reference the
        // glyph runs by GlyphRunId; the renderer resolves these to
        // vertex-buffer slices in its per-frame GlyphRunTable.
        if let Err(e) = self.ensure_atlas_uploaded() {
            web_sys::console::error_1(&format!("atlas upload failed: {}", e).into());
            return;
        }

        for &pass_id in &compiled.sorted_passes {
            let pass_idx = graph.passes.iter().position(|p| p.id == pass_id);
            let Some(pass) = pass_idx.and_then(|i| graph.passes.get(i)) else {
                continue;
            };
            self.execute_pass(graph, pass, time);
        }
    }

    /// Execute one pass: bind its color attachment(s), iterate its draw
    /// calls, dispatch each via the appropriate backend call.
    fn execute_pass(&mut self, graph: &RenderGraph, pass: &RenderPass, time: f32) {
        // Today there is exactly one color attachment (the canvas).
        // Future: bind the attachment's framebuffer/texture here.
        for &dc_id in &pass.draw_calls {
            let Some(dc) = graph.draw_calls.iter().find(|d| d.id_for_lookup() == dc_id) else {
                continue;
            };
            self.execute_draw_call(graph, dc, time);
        }
    }

    /// Execute one draw call: look up the DrawCallKind via the
    /// per-draw-call side table (populated by the lowering), bind the
    /// pipeline, set the uniforms, issue the GPU call.
    fn execute_draw_call(&mut self, graph: &RenderGraph, dc: &DrawCall, time: f32) {
        match dc.pipeline {
            alkalive_render::PIPELINE_CLEAR => {
                // The Clear "pipeline" is a no-op draw; the renderer
                // performs gl.clear_color + gl.clear before the first
                // real draw of the frame.
                if let Some(kind) = self.lookup_kind(dc) {
                    if let DrawCallKind::Clear { color } = kind {
                        self.gl.clear_color(color[0], color[1], color[2], color[3]);
                        self.gl.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
                    }
                }
            }
            alkalive_render::PIPELINE_RECT => {
                if let Some(kind) = self.lookup_kind(dc) {
                    if let DrawCallKind::DrawRect { bounds, color } = kind {
                        // Override bounds from cached input_field_bounds
                        // (workaround for the layout-not-yet-integrated case).
                        let bounds = self.real_rect_bounds(bounds);
                        self.draw_rect_filled(
                            bounds.x, bounds.y, bounds.w, bounds.h,
                            color[0], color[1], color[2], color[3],
                        );
                    }
                }
            }
            alkalive_render::PIPELINE_TEXT => {
                if let Some(kind) = self.lookup_kind(dc) {
                    if let DrawCallKind::DrawText { glyph_run_id, color, rotation, .. } = kind {
                        let rotation = rotation * time;
                        let (start, count) = self.glyph_run_range(glyph_run_id);
                        self.gl.use_program(Some(&self.program));
                        self.gl.bind_vertex_array(Some(&self.vao));
                        self.gl.uniform1f(Some(&self.u_rotation), rotation);
                        self.gl.uniform4f(Some(&self.u_text_color), color[0], color[1], color[2], color[3]);
                        self.gl.draw_arrays(WebGl2RenderingContext::TRIANGLES, start as i32, count as i32);
                    }
                }
            }
            _ => {
                // Custom pipeline (ADR-006 future). Today unreachable.
                web_sys::console::warn_1(&"AlkALive: custom pipeline not yet implemented".into());
            }
        }
    }
}

// Helper trait extension for DrawCallId lookup (avoids changing the
// existing DrawCall struct shape).
trait DrawCallLookup {
    fn id_for_lookup(&self) -> DrawCallId;
}
impl DrawCallLookup for DrawCall {
    fn id_for_lookup(&self) -> DrawCallId {
        // The DrawCall struct does not store its own ID; the lookup is
        // by index in graph.draw_calls. This trait method is a placeholder
        // for a future field addition.
        // TODO: add `pub id: DrawCallId` to DrawCall in a follow-up.
        DrawCallId(0)
    }
}
```

The `DrawCall` struct currently has no `id` field; the lowering populates a
parallel `draw_call_kinds: Box<[DrawCallKind]>` field on `RenderGraph`
(side table). A follow-up edit adds `pub id: DrawCallId` and
`pub kind: DrawCallKind` directly to `DrawCall`, eliminating the side table
and the lookup helper. The two-phase edit keeps the diff reviewable.

#### 6.5.7 Pipeline Pre-allocation

At renderer init time (`init_from_canvas`), the renderer pre-allocates three
pipelines (corresponding to the three `PIPELINE_*` constants):

```rust
// In WgpuRenderer::init_from_canvas (after shader compilation):
let _clear_pipeline = (); // no GPU state needed; clear is gl.clear()
let _rect_pipeline = self.rect_program; // existing
let _text_pipeline = self.program; // existing
```

In the wgpu migration (Gap 7), these become `wgpu::RenderPipeline` handles
stored in a `PipelineCache` (existing at
`crates/alkalive-render/src/lib.rs:646`).

#### 6.5.8 Runtime Integration

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs

struct Runtime {
    renderer: alkalive_backend_wgpu::WgpuRenderer,
    scene: alkalive_backend_wgpu::TextSceneData,
    schedule: alkalive_compiler::ScheduleIR,             // unchanged
    dep_graph: alkalive_compiler::DependencyGraph,      // unchanged
    signals: signal_store::SignalStore,                 // unchanged
    is_small_scene: bool,                               // unchanged
    time: f32,
    input_text: String,
    original_text: String,

    // NEW (Gap 6): the lowered render graph. Rebuilt when the scene's
    // structure changes (e.g. algorithm-node count changes). Updated
    // in place when only signal values change (e.g. text input, time).
    graph: alkalive_render::RenderGraph,

    // NEW (Gap 6): the compiled graph (cached; rebuilt when graph
    // structure changes).
    compiled: alkalive_render::CompiledGraph,
}

// In init_runtime (after the existing scene/schedule setup):
let graph = alkalive_render::schedule_to_render_graph(
    &scheduled,
    &scene,
    (width, height),
);
let compiled = alkalive_render::compile(
    std::slice::from_ref(&graph),
    &[],
    &Default::default(),
).map_err(|e| JsValue::from_str(&format!("graph compile: {:?}", e)))?;

// In start_frame_loop (replaces the existing render_frame call):
runtime.renderer.render_compiled(&runtime.graph, &runtime.compiled, runtime.time);
```

### 6.6 Runtime/Renderer Implications

1. **`render_frame` signature change.** Today:
   `render_frame(&mut self, text_scene: &TextSceneData, schedule: &ScheduleIR, time: f32)`.
   After Gap 6:
   `render_frame(&mut self, graph: &RenderGraph, time: f32)`. The
   `text_scene` and `schedule` are no longer passed per-frame — they are
   encoded into `graph` at startup (or on scene change). The runtime's
   `frame_closure` is updated accordingly.

2. **Dirty-pass fast path.** Today's `render_frame_with_dirty` checks
   `dirty_passes.is_empty()` and bails if so. After Gap 6, the renderer
   can skip individual passes by skipping their `PassId` in
   `compiled.sorted_passes`. (Note: this requires per-pass render targets
   to avoid leaving ghosts of stale passes — explicitly called out as a
   future wave in `backend-wgpu/src/lib.rs:850-854`.) For now, the
   dirty-pass info is plumbed through `compile()`'s `dirty: &[DirtyRect]`
   parameter (currently ignored — line 454 of `alkalive-render`).

3. **Atlas upload timing.** Today `upload_text_atlas` is called inside
   `render_frame_internal` (line 923). After Gap 6, the atlas upload is
   split into a separate `ensure_atlas_uploaded` method, called once per
   frame before the pass loop. This decouples text shaping (CPU) from
   draw-call emission (GPU).

4. **Pipeline cache.** Today the renderer has two hardcoded pipelines
   (text + rect). After Gap 6, the renderer references pipelines by
   `PipelineHandle`, looked up in the `PipelineCache`. The cache is
   populated at init time for the three built-in pipelines; ADR-006's
   author-supplied WGSL shaders will add entries dynamically.

5. **Buffer table.** The renderer gains a `BufferTable` (vector of
   `wgpu::Buffer` or `WebGlBuffer`) and a `TextureTable` (vector of
   `wgpu::Texture` or `WebGlTexture`). `BufferId` and `TextureId` are
   indices into these tables. The glyph atlas texture is `TextureId(0)`.

6. **GlyphRunTable.** The renderer gains a `GlyphRunTable` (vector of
   `(vertex_start, vertex_count)` ranges) keyed by `GlyphRunId`. Today
   there are two entries (title at `(0, title_vertex_count)` and input
   at `(title_vertex_count, input_vertex_count)`).

7. **Memory.** The `RenderGraph` is cloned once per frame (today) or
   mutated in place (future, with `&mut RenderGraph`). For the Hello
   World scene (5 passes, 5 draw calls, 1 attachment, 4 edges), the
   clone is ~600 bytes — negligible.

### 6.7 Browser/Platform Integration

- **wasm32 + raw WebGL2 (today's fallback, kept through Gap 6).** The
  renderer executes the graph via `WebGl2RenderingContext` calls. The
  `BufferTable` holds `WebGlBuffer` values; the `TextureTable` holds
  `WebGlTexture` values. No browser changes required.
- **wasm32 + wgpu (Gap 7).** The renderer executes the graph via
  `wgpu::RenderPass` encoders. The `BufferTable` holds `wgpu::Buffer`
  values. The graph's `Attachment` descriptors become `wgpu::TextureView`
  descriptors.
- **Native (test host).** The native stub (`backend-wgpu/src/lib.rs:1348-1429`)
  gains a `render_frame(&mut self, graph: &RenderGraph, time: f32)` no-op
  for type-check parity. The `schedule_to_render_graph` lowering runs on
  native (it is pure data manipulation) and is unit-tested there.

### 6.8 Error Handling

| Error class | Source | Handling |
|-------------|--------|----------|
| `CompileError::InvalidEdge` | `compile()` finds an edge referencing an unknown `PassId` | Log to `web_sys::console::error_1`; skip the frame (no draw). |
| `CompileError::AttachmentLifetimeViolation` | `compile()` finds an attachment whose lifetime references a missing pass | Same as above. |
| `CompileError::CycleDetected` | Topological sort finds a cycle (should not happen for the linear-chain graph today) | Same. |
| Renderer draw-call lookup failure | `graph.draw_calls` does not contain the expected `DrawCallId` | Log warning; skip the draw call. |
| Atlas upload failure | `ensure_atlas_uploaded` returns `Err` | Log error; skip the frame. |
| Pipeline lookup failure | `dc.pipeline` is not one of the known handles | Log warning; skip the draw call. |

The existing `compile()` already returns `Result<CompiledGraph, CompileError>`
(`crates/alkalive-render/src/lib.rs:447`). The renderer's `render_frame`
wraps the call in a `match` that logs and bails on `Err`. No panic paths
are added.

### 6.9 Testing Strategy

1. **Unit tests in `alkalive-render`** (native):
   - `schedule_to_render_graph` produces a graph with N passes for an
     N-pass schedule.
   - `compile()` returns `sorted_passes` matching `pass_order` for a
     linear-chain graph.
   - `compile()` returns `Err(InvalidEdge)` for a graph with a dangling
     edge.
   - `compile()` returns `Err(CycleDetected)` for a cyclic graph.
   - `compile()` returns `Err(AttachmentLifetimeViolation)` for an
     attachment whose lifetime references a missing pass.
2. **Unit tests in `alkalive-backend-wgpu`** (native, stubbed GPU):
   - `render_frame` accepts a `RenderGraph` and does not panic (the native
     stub is a no-op).
   - The `lookup_kind` helper returns the expected `DrawCallKind` for each
     `PipelineHandle`.
3. **Integration tests in `alkalive-runtime-wasm`** (wasm32, headless
   browser via `wasm-bindgen-test`):
   - The frame loop calls `render_compiled` (not `render_frame`) on the
     pre-compiled graph; the canvas receives at least one draw call per
     frame (verified via a `MockGl2RenderingContext` or by reading back a
     single pixel via `gl.readPixels`).
4. **Browser-verification test** (manual, per Wave 0 §12):
   - Rebuild WASM, deploy, screenshot the canvas, assert the golden
     "Hello World!" text is visible and the input field rectangle is
     drawn with a 0.9-alpha dark fill and a 0.8-alpha gold border.
5. **Regression tests**: all 1148 existing tests must pass unchanged
   (Gap 6 changes the renderer's internal dispatch but not the visible
   output of the Hello World scene).

### 6.10 Dependencies on Other Gaps

| Dependency | Direction | Detail |
|------------|-----------|--------|
| Gap 7 (WGSL/wgpu) | Gap 6 → Gap 7 | The `DrawCall.pipeline` field is `PipelineHandle`. Today this is an opaque `u64` with no backing pipeline cache. Gap 7 populates the cache with real `wgpu::RenderPipeline` objects. Gap 6 ships first with the raw-WebGL2 implementation using the existing `program`/`rect_program` fields as the pipeline table. |
| Gap 8 (single-GPU-device) | Gap 6 → Gap 8 | The `compile()` function is currently called inside the renderer (main thread). Gap 8 moves `compile()` to the render worker; the main thread sends uncompiled `RenderGraph` IR via `postMessage` or `SharedArrayBuffer`. The `RenderGraph` type is `Clone` (already — see `crates/alkalive-render/src/lib.rs:253`) so it can be serialized. |
| ADR-025 (incremental computation) | Gap 6 ↔ ADR-025 | The `dirty_passes` mechanism currently uses `&[usize]` indices. After Gap 6, dirty info is `&[PassId]` (or `&[DirtyRect]` for the occlusion-cull pass). The runtime's `SignalStore::propagate` is updated to emit `PassId`s instead of `usize`s. |
| ADR-002 (per-module dirty-rect invalidation) | Gap 6 enables | Once `Attachment.lifetime` and per-pass render targets exist, the renderer can skip passes whose `DirtyRect` does not intersect the dirty region. Today's single-framebuffer model cannot do this. |
| ADR-004 (pluggable layout) | Gap 6 ↔ ADR-004 | The current `lower_pass_kind` hardcodes input-field bounds (a placeholder `(0,0,0,0)` that the renderer overwrites from its cached `input_field_bounds`). ADR-004's layout solver will compute the bounds and pass them through `schedule_to_render_graph`. |

### 6.11 Risks and Trade-offs

**Risk R6.1 — Performance regression from graph clone.** The runtime
clones the `RenderGraph` once per frame (to pass `&graph` to the renderer
while retaining ownership for the next frame). For the Hello World scene
(5 passes), the clone is ~600 bytes — negligible. For larger scenes (1000+
passes), the clone cost may dominate. **Mitigation**: switch to
`Rc<RenderGraph>` (single-threaded) or `Arc<RenderGraph>` (multi-threaded,
Gap 8) so the clone is a reference-count bump. Alternatively, mutate the
graph in place via `&mut RenderGraph`.

**Risk R6.2 — Side-table wart for `DrawCallKind`.** The first iteration
keeps `DrawCallKind` in a side table (`graph.draw_call_kinds: Box<[DrawCallKind]>`)
because the existing `DrawCall` struct does not have a `kind` field. This
introduces a lookup indirection. **Mitigation**: add `pub kind:
DrawCallKind` directly to `DrawCall` in a follow-up commit (mechanical
change, no logic change).

**Risk R6.3 — Layout hardcoded in the renderer.** The `lower_pass_kind`
function emits placeholder bounds for `DrawRect` because the input-field
geometry is computed inside `WgpuRenderer::upload_text_atlas` (line 1245).
This violates ADR-004's separation of layout from rendering. **Mitigation**:
the lowering accepts a `&LayoutOutput` parameter (populated by the layout
solver); for now, `LayoutOutput` is a stub that returns the same hardcoded
bounds. ADR-004 replaces the stub.

**Risk R6.4 — The `compile()` merger is untested for multi-graph input.**
The existing `compile()` accepts `&[RenderGraph]` but is only ever called
with a single graph (per Gap 6's `render_frame`). Gap 8's render-thread
merger will call it with multiple graphs. **Mitigation**: add a unit test
that passes two graphs with disjoint pass IDs and asserts the merger
produces a topologically-sorted union.

**Risk R6.5 — Backward compatibility of the existing `render_frame`
signature.** The existing `render_frame(&mut self, &TextSceneData, &ScheduleIR, f32)`
is called from one site (`runtime-wasm/src/lib.rs:648`). The signature
change is a single-call-site update. No external API breaks. The
`render_frame_with_dirty` variant is removed (its `dirty_passes` parameter
is replaced by per-pass dirty info plumbed through `compile()`'s `dirty`
parameter).

### 6.12 Open Questions

**Q6.1 — Should `schedule_to_render_graph` live in `alkalive-render` or
`alkalive-compiler`?** Tentative answer: `alkalive-render`, because it
produces `alkalive_render::RenderGraph` and the lowering logic is
GPU-layer (not author-facing). This requires `alkalive-render` to depend
on `alkalive-compiler` for the input types. The alternative (lowering in
`alkalive-compiler`) would require `alkalive-compiler` to depend on
`alkalive-render`, inverting the dependency direction. The technical
specification's crate dependency graph
(`docs/technical-specification.md:570-595`) shows `alkalive-render` at
the bottom (no deps on the compiler), so the lowering belongs in
`alkalive-render` with a new one-way dep on `alkalive-compiler`. This
adds a `crates/alkalive-render/Cargo.toml` line:
`alkalive-compiler = { workspace = true }`
`alkalive-backend-wgpu = { workspace = true }` (for `TextSceneData`).

**Q6.2 — Should the `Clear` "pipeline" be a real GPU pipeline or a
renderer-internal fast path?** Tentative answer: keep it as a renderer
fast path (`gl.clear` / `wgpu::LoadOp::Clear`). The `DrawCallKind::Clear`
variant exists in the IR for authoring clarity (the schedule says "clear
the framebuffer"); the renderer maps it to the platform's clear primitive
rather than a full draw call.

**Q6.3 — How does the dirty-pass fast path interact with the linear-chain
edge graph?** Today every pass depends on the previous one (because the
edges form a linear chain). This means any dirty pass transitively dirties
all subsequent passes. **Mitigation**: the `schedule_to_render_graph`
lowering should produce a sparse edge graph (only real dependencies, e.g.
"TitleText depends on Clear because Clear initializes the color
attachment"). For the Hello World scene, only the first pass (Clear) has
no dependencies; every other pass depends on Clear but not on each other.
This requires per-pass render targets (so a non-dirty pass's output is
preserved from the previous frame).

**Q6.4 — Should the `RenderGraph` be `Send`?** Today it contains
`Box<[T]>` which is `Send`. For Gap 8, the graph is sent via `postMessage`
(structured clone) — `Send` is sufficient for the wasm-bindgen boundary.
No change required.

**Q6.5 — What happens when the scene changes mid-session (e.g. HMR per
ADR-015)?** The `schedule_to_render_graph` lowering must be re-run. The
runtime gains a `rebuild_graph(&mut self)` method called when the
algorithm IR changes. Today no HMR mechanism exists; this is future work.

---

## §2 Gap 7 — WGSL Shaders (ADR-006)

### 7.1 Current State (with file:line evidence)

The current renderer uses **GLSL ES 3.00** shaders, hardcoded as Rust
string constants. There is no WGSL anywhere in the project.

**Evidence 1 — Text shader source**
(`crates/alkalive-backend-wgpu/src/lib.rs:186-260`):

```rust
pub const VERTEX_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;
uniform float rotation;
uniform vec2 canvas_size;
uniform float time;
out vec2 v_uv;
void main() {
    float cos_r = cos(rotation);
    float center_x = canvas_size.x * 0.5;
    float rel_x = position.x - center_x;
    float scaled_x = rel_x * cos_r + center_x;
    vec2 clip = vec2(
        scaled_x / (canvas_size.x * 0.5) - 1.0,
        1.0 - position.y / (canvas_size.y * 0.5)
    );
    gl_Position = vec4(clip, 0.0, 1.0);
    v_uv = uv;
}
"#;

pub const FRAGMENT_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
uniform sampler2D glyph_texture;
uniform vec4 text_color;
out vec4 frag_color;
void main() {
    float alpha = texture(glyph_texture, v_uv).r;
    if (alpha < 0.01) { discard; }
    frag_color = vec4(text_color.rgb * alpha, alpha);
}
"#;
```

**Evidence 2 — Rect shader source** (lines 265-289, added by Wave 1 C2 fix):

```rust
pub const RECT_VERTEX_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 position;
void main() { gl_Position = vec4(position, 0.0, 1.0); }
"#;

pub const RECT_FRAGMENT_SHADER_SRC: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_rect;
uniform vec4 u_color;
uniform vec2 u_canvas;
out vec4 frag_color;
void main() {
    float px = gl_FragCoord.x;
    float py = u_canvas.y - gl_FragCoord.y;
    if (px < u_rect.x || px > u_rect.z || py < u_rect.y || py > u_rect.w) { discard; }
    frag_color = u_color;
}
"#;
```

**Evidence 3 — Shader compilation via raw WebGL2**
(`crates/alkalive-backend-wgpu/src/lib.rs:574-601`):

```rust
let vs = compile_shader(&gl, WebGl2RenderingContext::VERTEX_SHADER, VERTEX_SHADER_SRC)?;
let fs = compile_shader(&gl, WebGl2RenderingContext::FRAGMENT_SHADER, FRAGMENT_SHADER_SRC)?;
let program = gl.create_program().ok_or_else(|| ...)?;
gl.attach_shader(&program, &vs);
gl.attach_shader(&program, &fs);
gl.link_program(&program);
if gl.get_program_parameter(&program, WebGl2RenderingContext::LINK_STATUS).as_bool() != Some(true) {
    let log = gl.get_program_info_log(&program).unwrap_or_else(|| "(no info log)".to_string());
    return Err(format!("Program link failed: {}", log));
}
```

**Evidence 4 — No `wgpu` dependency.**
`crates/alkalive-backend-wgpu/Cargo.toml` declares a `wgpu-backend`
feature flag (line 51) but the feature is unused — there is no
`wgpu = "..."` line in `[dependencies]`. The crate-level doc
(`backend-wgpu/src/lib.rs:8-23`) explicitly justifies the raw-WebGL2
choice on three grounds: (1) WebGL2 is universal; (2) `wgpu` adds ~50
transitive deps; (3) the raw surface is "small enough for one file."

**Evidence 5 — ADR-006 is unimplemented.** The audit's gap analysis
(`docs/alkalive-wave-00-audit.md:215`) lists ADR-006 as **Major**:
"Hardcoded GLSL" vs. the ADR's "WGSL shaders as styling primitives."

### 7.2 Problem Statement

ADR-006 (line 165) requires: *"WGSL shader programs + compute passes
bound to object instances as first-class styling primitives, composable
in the style layer, replacing CSS's closed filter list with an open,
author-extensible effect model."*

The current GLSL approach:

1. **Cannot be authored by users.** ADR-006's "open, author-extensible
   effect model" requires authors to supply shader source. GLSL ES 3.00
   is a WebGL2-specific dialect; WGSL is the WebGPU standard. The
   `.alk` language (per ADR-008, future) will expose WGSL as a styling
   primitive, not GLSL.
2. **Cannot be precompiled.** ADR-017 (line 600) requires "WebGPU
   pipeline precompilation removing the GPU-side startup floor." GLSL
   shaders are compiled at runtime by the browser's WebGL2 driver;
   there is no ahead-of-time pipeline cache.
3. **Cannot target WebGPU.** ADR-001 (line 61) states "WebGPU is the
   initial backend (with Vulkan/Metal as future native-backend
   options)." WebGL2 is a separate backend; switching to WebGPU requires
   rewriting the shaders in WGSL.
4. **Cannot be composed.** ADR-006's "composable in the style layer"
   implies shaders can be combined (e.g. a blur shader chained after a
   text shader). The current GLSL is monolithic (one vertex + one
   fragment per pipeline); there is no composition mechanism.
5. **The crate name `alkalive-backend-wgpu` is a lie.** The crate uses
   raw WebGL2, not `wgpu`. This creates confusion for new contributors
   and misleads dependency scanners.

### 7.3 Why It's Required (ADR Reference)

ADR-006 (decision, `docs/adr/ADR.md:165`): *"Adopt option (a): WGSL
shader programs + compute passes bound to object instances as first-class
styling primitives, composable in the style layer, replacing CSS's closed
filter list with an open, author-extensible effect model."*

ADR-006 (consequences, line 171): *"Positive: open extensibility; effects
unify with the GPU pipeline; particle/per-vertex/compute-driven styling
become first-class. Negative: shader authoring skill floor; shader
compile budget and sandboxing required; fallback/degradation for low-end
GPUs."*

ADR-006 (cross-references, line 173): *"[ADR-005] provides the owned-state
uniforms; [ADR-001] schedules the paint passes."*

ADR-001 (line 61): *"WebGPU is the initial backend (with Vulkan/Metal as
future native-backend options)."*

ADR-017 (line 600): *"WebGPU pipeline precompilation removing the GPU-side
startup floor."*

### 7.4 Relationship to Existing Runtime/Renderer

| Component | Current role | Role after Gap 7 |
|-----------|--------------|-------------------|
| `VERTEX_SHADER_SRC`, `FRAGMENT_SHADER_SRC` (GLSL) | Hardcoded Rust `&str` constants | **Replaced** by WGSL sources `text_quad.wgsl`, `rect.wgsl`, `clear.wgsl` |
| `RECT_VERTEX_SHADER_SRC`, `RECT_FRAGMENT_SHADER_SRC` (GLSL) | Hardcoded (Wave 1 C2 fix) | **Replaced** by `rect.wgsl` |
| `compile_shader()` helper (line 1316) | Compiles one GLSL shader via `gl.create_shader` | **Removed** — `wgpu::Device::create_shader_module` replaces it |
| `WgpuRenderer::program`, `rect_program` | `WebGlProgram` handles | **Replaced** by `wgpu::RenderPipeline` handles in a `PipelineCache` |
| `WgpuRenderer::u_rotation`, `u_text_color`, etc. | `WebGlUniformLocation` cached fields | **Replaced** by bind-group entries |
| `WgpuRenderer::gl` | `WebGl2RenderingContext` | **Replaced** by `wgpu::Device` + `wgpu::Queue` + `wgpu::Surface` |
| `WgpuRenderer::init_from_canvas` | Acquires WebGL2 context via `canvas.getContext("webgl2")` | Acquires `wgpu::Surface` via `canvas.getContext("webgpu")` (or fallback to `webgl` feature) |

### 7.5 Proposed Design

#### 7.5.1 Migration Strategy: `wgpu` with WebGL2 Fallback

Add `wgpu` as a dependency of `alkalive-backend-wgpu` with the `webgl`
feature enabled. This gives:

- **WebGPU** on Chrome 113+, Edge 113+, and any browser with WebGPU
  support.
- **WebGL2 fallback** via `wgpu`'s `webgl` feature on Firefox, Safari,
  and older Chrome.

The `wgpu` crate's `Surface` abstraction handles the platform detection
transparently. The shader source is **WGSL in both cases** — `wgpu`
translates WGSL to GLSL ES 3.00 when targeting WebGL2 (via `naga`'s GLSL
backend, which is a `wgpu` transitive dep).

```toml
# crates/alkalive-backend-wgpu/Cargo.toml (REPLACES the [dependencies] section)

[dependencies]
alkalive-text = { workspace = true }
alkalive-compiler = { workspace = true }
alkalive-render = { workspace = true }       # NEW (Gap 6)

bytemuck = { version = "1", features = ["derive"] }
wasm-bindgen = { workspace = true }
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "HtmlCanvasElement",
    "Window",
    "Document",
    "Element",
    "Gpu",                          # for WebGPU native (when available)
    "GpuCanvasContext",
    "GpuDevice",
    "GpuQueue",
    "OffscreenCanvas",              # NEW (Gap 8 — worker-side canvas)
    "Worker",                       # NEW (Gap 8)
    "MessageEvent",                 # NEW (Gap 8)
    "console",
    "Performance",
]}

# NEW (Gap 7): wgpu with WebGL2 fallback.
# Features:
# - `webgpu`: native WebGPU backend (default; works in Chrome 113+).
# - `webgl`: WebGL2 fallback (works in all evergreen browsers).
# Both are enabled so wgpu picks the best available at runtime.
wgpu = { version = "23", features = ["webgpu", "webgl"] }

[features]
default = []
# Kept for backward compat with any external consumers that probe the feature.
wgpu-backend = []
```

**Note on dep count**: the audit (`docs/alkalive-wave-00-audit.md:286`)
acknowledged `wgpu` adds ~50 transitive deps. This is now accepted as the
cost of ADR-006 compliance. The `deny.toml` file at the repo root
(`AlkALive/deny.toml`) already exists; Gap 7's PR will run
`cargo deny check` to confirm no duplicated deps or license issues.

#### 7.5.2 WGSL Shader Sources

The three existing GLSL shaders are translated to WGSL. The translations
are mechanical (the GLSL is already GPU-friendly; only syntax changes).

**`text_quad.wgsl`** (replaces `VERTEX_SHADER_SRC` + `FRAGMENT_SHADER_SRC`):

```wgsl
// AlkALive text-quad shader.
// Draws a glyph quad from the atlas, modulated by text_color and rotated
// around the canvas center on the Y axis.

struct Uniforms {
    rotation: f32,
    canvas_w: f32,
    canvas_h: f32,
    time: f32,
    text_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var glyph_texture: texture_2d<f32>;
@group(0) @binding(2) var glyph_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let canvas_size = vec2<f32>(u.canvas_w, u.canvas_h);
    let cos_r = cos(u.rotation);
    let center_x = canvas_size.x * 0.5;
    let rel_x = in.position.x - center_x;
    let scaled_x = rel_x * cos_r + center_x;

    let clip = vec2<f32>(
        scaled_x / (canvas_size.x * 0.5) - 1.0,
        1.0 - in.position.y / (canvas_size.y * 0.5),
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_texture, glyph_sampler, in.uv).r;
    if (alpha < 0.01) {
        discard;
    }
    // Premultiplied alpha output.
    return vec4<f32>(u.text_color.rgb * alpha, alpha);
}
```

**`rect.wgsl`** (replaces `RECT_VERTEX_SHADER_SRC` + `RECT_FRAGMENT_SHADER_SRC`):

```wgsl
// AlkALive rect shader.
// Draws a full-viewport quad; the fragment shader clips to the rect bounds.

struct RectUniforms {
    u_rect: vec4<f32>,    // pixel-space bounds: (x0, y0, x1, y1)
    u_color: vec4<f32>,   // RGBA
    u_canvas: vec2<f32>,  // canvas size in pixels
};

@group(0) @binding(0) var<uniform> u: RectUniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,  // clip-space [-1, 1]
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let px = in.clip_position.x;
    let py = u.u_canvas.y - in.clip_position.y;
    if (px < u.u_rect.x || px > u.u_rect.z || py < u.u_rect.y || py > u.u_rect.w) {
        discard;
    }
    return u.u_color;
}
```

**`clear.wgsl`** (no actual shader — `Clear` is a `LoadOp::Clear` on the
color attachment; included here for documentation):

```wgsl
// AlkALive "clear" pseudo-shader.
// Not a real WGSL program — the renderer maps DrawCallKind::Clear to
// wgpu::LoadOp::Clear(color) on the pass's color attachment.
// Documented here so the schedule_to_render_graph lowering has a
// consistent pipeline-handle space (PIPELINE_CLEAR, PIPELINE_RECT,
// PIPELINE_TEXT).
```

#### 7.5.3 Shader Storage

WGSL sources are stored as separate `.wgsl` files in
`crates/alkalive-backend-wgpu/src/shaders/` and embedded via
`include_str!`:

```
crates/alkalive-backend-wgpu/src/shaders/
├── text_quad.wgsl
├── rect.wgsl
└── README.md
```

```rust
// In crates/alkalive-backend-wgpu/src/lib.rs (REPLACES the GLSL constants):

/// WGSL source for the text-quad shader (vertex + fragment in one file).
pub const TEXT_QUAD_WGSL: &str = include_str!("shaders/text_quad.wgsl");

/// WGSL source for the rect shader.
pub const RECT_WGSL: &str = include_str!("shaders/rect.wgsl");
```

This enables:
- Editor syntax highlighting (`.wgsl` is recognized by VS Code, Zed,
  Emacs, Vim).
- Diff-friendly history (WGSL files are diffed as text, not Rust string
  literals).
- Future author-supplied shaders to be loaded from disk (or embedded via
  `include_str!` from the `.alk` source's `shader` declarations).

#### 7.5.4 The New `WgpuRenderer` (Sketch)

```rust
// In crates/alkalive-backend-wgpu/src/lib.rs (REPLACES the wasm32 mod body)

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use alkalive_render::{RenderGraph, CompiledGraph, PipelineHandle, PipelineCache};

    pub struct WgpuRenderer {
        // wgpu core objects.
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,

        // Surface configuration (re-created on resize).
        surface_config: wgpu::SurfaceConfiguration,

        // Pipeline cache (existing type from alkalive-render).
        pipeline_cache: PipelineCache,

        // Buffer / texture / sampler tables.
        buffer_table: Vec<wgpu::Buffer>,
        texture_table: Vec<wgpu::Texture>,
        sampler_table: Vec<wgpu::Sampler>,

        // Per-frame glyph-run table (start, count) ranges keyed by GlyphRunId.
        glyph_run_table: Vec<(u32, u32)>,

        // Cached font infrastructure (Wave 1 M7 fix, retained).
        font_registry: Option<Arc<alkalive_text::HarfRustFontRegistry>>,
        font_id: Option<alkalive_text::FontId>,
        text_shaper: Option<alkalive_text::HarfRustTextShaper>,

        // Canvas dimensions (physical pixels).
        width: u32,
        height: u32,

        // Animation clock.
        performance: web_sys::Performance,
        start_ms: f64,

        // Input field bounds (for hit-testing — kept until ADR-004 lands).
        input_field_bounds: (f32, f32, f32, f32),
    }

    impl WgpuRenderer {
        pub async fn init_from_canvas(
            canvas: web_sys::HtmlCanvasElement,
            width: u32,
            height: u32,
        ) -> Result<Self, String> {
            // 1. Create the wgpu instance with both WebGPU and WebGL backends.
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::BROWSER_WEBGL,
                ..Default::default()
            });

            // 2. Create a surface from the canvas.
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                .map_err(|e| format!("surface creation: {:?}", e))?;

            // 3. Request an adapter.
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|e| format!("adapter request: {:?}", e))?;

            // 4. Request a device + queue.
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("AlkALive device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::default(),
                })
                .await
                .map_err(|e| format!("device request: {:?}", e))?;

            // 5. Configure the surface.
            let caps = surface.get_capabilities(&adapter);
            let format = caps.formats.iter()
                .find(|f| **f == wgpu::TextureFormat::Bgra8Unorm)
                .or_else(|| caps.formats.first())
                .copied()
                .ok_or_else(|| "no surface formats available".to_string())?;
            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width,
                height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            };
            surface.configure(&device, &surface_config);

            // 6. Create the shader modules + render pipelines.
            let text_module = device.create_shader_module(wgpu::ShaderSource::Wgsl(
                std::borrow::Cow::Borrowed(TEXT_QUAD_WGSL),
            ));
            let rect_module = device.create_shader_module(wgpu::ShaderSource::Wgsl(
                std::borrow::Cow::Borrowed(RECT_WGSL),
            ));

            // ... build the render pipelines (text + rect) ...
            // ... populate the pipeline cache ...

            Ok(Self {
                instance, surface, adapter, device, queue,
                surface_config,
                pipeline_cache: PipelineCache::new(),
                buffer_table: Vec::new(),
                texture_table: Vec::new(),
                sampler_table: Vec::new(),
                glyph_run_table: Vec::new(),
                font_registry: None,
                font_id: None,
                text_shaper: None,
                width, height,
                performance: web_sys::window().unwrap().performance().unwrap(),
                start_ms: 0.0,
                input_field_bounds: (0.0, 0.0, 0.0, 0.0),
            })
        }

        pub fn render_compiled(
            &mut self,
            graph: &RenderGraph,
            compiled: &CompiledGraph,
            time: f32,
        ) {
            // 1. Acquire the next frame's texture.
            let output = match self.surface.get_current_texture() {
                Ok(t) => t,
                Err(e) => {
                    web_sys::console::warn_1(
                        &format!("AlkALive: surface get_current_texture: {:?}", e).into(),
                    );
                    return;
                }
            };
            let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

            // 2. Ensure the glyph atlas + vertex buffer are uploaded.
            if let Err(e) = self.ensure_atlas_uploaded() {
                web_sys::console::error_1(&format!("atlas upload: {}", e).into());
                return;
            }

            // 3. Encode a single command buffer with one render pass per
            //    pass in `compiled.sorted_passes`.
            let mut encoder = self.device.create_command_encoder(
                &wgpu::CommandEncoderDescriptor { label: Some("AlkALive frame") },
            );

            // Today: collapse all passes into one wgpu render pass
            // (because they all share the same color attachment and have
            // no barriers). Future: one render pass per graph pass, with
            // barriers between.
            {
                let clear_color = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("AlkALive main pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: clear_color, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                for &pass_id in &compiled.sorted_passes {
                    let Some(pass) = graph.passes.iter().find(|p| p.id == pass_id) else { continue };
                    for &dc_id in &pass.draw_calls {
                        let Some(dc) = graph.draw_calls.iter().find(|d| d.id_for_lookup() == dc_id) else { continue };
                        // Look up the pipeline + bind group, set them on rpass,
                        // issue draw.
                        self.execute_draw_call(&mut rpass, graph, dc, time);
                    }
                }
            }

            self.queue.submit(std::iter::once(encoder.finish()));
            output.present();
        }
    }
}
```

#### 7.5.5 Future: Author-Supplied WGSL (ADR-006 Full Vision)

The `DrawCallKind::DrawCustom { shader_hash, vertices, uniforms, ... }`
variant (defined in §6.5.3) is the hook for author-supplied WGSL. The
flow:

1. The `.alk` source declares a `shader` block (future syntax):
   ```
   shader my_effect : "my_effect.wgsl" { uniforms: { time: f32, intensity: f32 } }
   text "Hello" { effect: my_effect(time: signal::time, intensity: 0.5) }
   ```
2. The compiler embeds the WGSL source via `include_bytes!` (or
   `include_str!`).
3. `schedule_to_render_graph` emits a `DrawCallKind::DrawCustom { shader_hash, ... }`
   for the text node.
4. The renderer looks up `shader_hash` in its `PipelineCache`. On a miss,
   it calls `device.create_shader_module(ShaderSource::Wgsl(...))` and
   `device.create_render_pipeline(...)`, caching the result.
5. The draw call issues with the author's vertex/uniform data.

This is **out of scope for Gap 7's first cut** but the IR shape (the
`DrawCustom` variant + `shader_hash` lookup) is in place so the future
addition is non-breaking.

### 7.6 Runtime/Renderer Implications

1. **Crate dep count grows by ~50.** The audit (`docs/alkalive-wave-00-audit.md:286`)
   flagged this as a build-time concern (several minutes of build time).
   Acceptable: the build time is paid once per developer machine; CI
   caches the build.
2. **WASM binary grows.** `wgpu` adds ~300-500 KB to the WASM binary
   (after `wasm-opt -Oz`). The current binary is 1.05 MB; post-Gap-7
   estimate is 1.4-1.6 MB. This sharpened the ADR-017 streaming-compile
   concern (acknowledged in `docs/adr/ADR.md:607`).
3. **Surface configuration is per-canvas.** The `wgpu::Surface` is tied
   to a specific `HtmlCanvasElement`. On resize, the renderer reconfigures
   the surface (replaces the existing `gl.viewport` call at line 1078).
4. **Pipeline cache key.** The cache is keyed by `(shader_hash,
   layout_hash, target_format)` (existing `PipelineDesc` struct at
   `crates/alkalive-render/src/lib.rs:305-314`). For the Hello World
   scene, three entries: text-quad, rect, clear (the last is a no-op
   pipeline).
5. **Bind groups.** Each `DrawCall` has a `BindGroup` (defined in §6.5.4).
   The renderer creates a `wgpu::BindGroup` per draw call, referencing
   the cached uniform buffer, texture, and sampler. For the Hello World
   scene, two bind groups (text + rect) are created per frame (or cached
   if their contents haven't changed).
6. **Frame pacing.** `wgpu::PresentMode::AutoVsync` (line 1130 above)
   matches the browser's vsync. The runtime's `requestAnimationFrame`
   loop is unchanged — it still drives one render per RAF callback.

### 7.7 Browser/Platform Integration

- **WebGPU available (Chrome 113+, Edge 113+).** `wgpu` uses the native
  WebGPU backend. Shader compilation is via `device.create_shader_module`
  with WGSL source. Pipeline precompilation (ADR-017) is possible via
  `device.create_render_pipeline_async` (overlaps with module decode).
- **WebGPU unavailable (Firefox, Safari < 17.4, old Chrome).** `wgpu`
  falls back to the `webgl` feature, which translates WGSL to GLSL ES
  3.00 via `naga` (a `wgpu` transitive dep). The translation happens at
  shader-module creation time; the resulting GLSL is compiled by the
  browser's WebGL2 driver. **Caveat**: the WebGL2 fallback does not
  support compute passes (WebGL2 has no compute). ADR-006's "compute
  passes" feature is WebGPU-only.
- **Native (test host).** `wgpu` supports native backends (Vulkan on
  Linux/Windows, Metal on macOS, DX12 on Windows). The native stub in
  `backend-wgpu/src/lib.rs:1348-1429` is removed; the real `wgpu` path
  runs on native for headless testing (via `wgpu::Backends::SECONDARY`
  or a software renderer like `lavapipe`).
- **OffscreenCanvas (Gap 8).** When the renderer moves to a Web Worker
  (Gap 8), the canvas is transferred to the worker via
  `canvas.transferControlToOffscreen()`. The `wgpu::Surface` is created
  from the `OffscreenCanvas` instead of the `HtmlCanvasElement`. This is
  supported by `wgpu::SurfaceTarget::Canvas` (which accepts both).

### 7.8 Error Handling

| Error class | Source | Handling |
|-------------|--------|----------|
| `wgpu::RequestAdapterError` | No suitable GPU adapter | Log error; fall back to a CPU software rasterizer (ADR-016 future) or display a "WebGPU/WebGL2 unavailable" message. |
| `wgpu::RequestDeviceError` | Adapter cannot create a device (e.g. feature unsupported) | Same. |
| `wgpu::SurfaceError::Lost` | Surface lost (e.g. context loss) | Reconfigure the surface; if reconfiguration fails, re-init the renderer. |
| `wgpu::SurfaceError::Outdated` | Surface size mismatch (e.g. resize during frame) | Reconfigure; skip the frame. |
| `wgpu::ValidationError` (shader compile) | WGSL syntax error in `text_quad.wgsl` or `rect.wgsl` | This is a compile-time bug; should never happen in a release build. Caught by `cargo test` (the WGSL sources are compiled during unit tests via `wgpu::Device::create_shader_module` on native). |
| Pipeline cache miss for a custom shader | `DrawCallKind::DrawCustom` with unknown `shader_hash` | Log warning; skip the draw call. (Future: ADR-006 author-supplied WGSL.) |

### 7.9 Testing Strategy

1. **Unit tests in `alkalive-backend-wgpu`** (native, real `wgpu`):
   - `TEXT_QUAD_WGSL` and `RECT_WGSL` compile successfully via
     `device.create_shader_module` (no `ValidationError`).
   - The text-quad render pipeline links successfully.
   - The rect render pipeline links successfully.
2. **Unit tests in `alkalive-render`** (native):
   - `schedule_to_render_graph` produces draw calls whose `pipeline`
     field is one of the three known handles (`PIPELINE_CLEAR`,
     `PIPELINE_RECT`, `PIPELINE_TEXT`).
   - The `pipeline_for_kind` function maps each `DrawCallKind` to the
     correct handle.
3. **Integration tests in `alkalive-runtime-wasm`** (wasm32, headless
   browser):
   - The renderer initializes on a canvas with WebGPU (Chrome headless)
     and on a canvas with WebGL2 (Firefox headless).
   - One frame renders without panicking.
4. **Browser-verification test** (manual):
   - Visual parity with the pre-Gap-7 output: golden "Hello World!"
     text, dark input field with gold border, both rotating correctly
     on the Y axis.
5. **Regression tests**: all 1148 existing tests pass. The existing
   `Vertex`, `Uniforms`, `GlyphQuad`, `build_vertex_buffer`,
   `quads_from_text` types and functions are unchanged (they are
   target-agnostic and unit-tested on native).

### 7.10 Dependencies on Other Gaps

| Dependency | Direction | Detail |
|------------|-----------|--------|
| Gap 6 (render-graph IR) | Gap 7 ← Gap 6 | The renderer's `render_compiled` consumes a `CompiledGraph`. Gap 6 ships first; Gap 7 swaps the renderer's internals from raw WebGL2 to `wgpu` while keeping the `render_compiled` signature stable. |
| Gap 8 (single-GPU-device) | Gap 7 ← Gap 8 | The `wgpu::Device` is owned by the render worker (Gap 8). The renderer's `device` field becomes `Send` (it is, by `wgpu`'s design) and is moved to the worker. |
| ADR-017 (pipeline precompilation) | Gap 7 enables | Once pipelines are `wgpu::RenderPipeline` objects, they can be precompiled at app startup (overlapping with WASM decode). The `PipelineCache` (existing at `crates/alkalive-render/src/lib.rs:646`) is the cache. |
| ADR-005 (object-owned styling) | Gap 7 enables | The `BindGroup` struct (defined in §6.5.4) holds per-instance uniform state. ADR-005's "owned-state uniforms" become `BindGroup` entries. |
| ADR-022 (HarfRust text stack) | Gap 7 unchanged | The text stack (`alkalive-text`) is unchanged. The glyph atlas texture is uploaded to a `wgpu::Texture` instead of a `WebGlTexture`. |

### 7.11 Risks and Trade-offs

**Risk R7.1 — `wgpu` API churn.** `wgpu` is at v23 at the time of writing;
the API has changed materially between major versions (e.g. `wgpu::Surface`
became lifetime-parameterized in v23). Pinning to v23 is mandatory;
upgrading is a follow-up. **Mitigation**: pin `wgpu = "23"` (not `"23.*"`)
in `Cargo.toml`; document the pin in the crate-level doc.

**Risk R7.2 — WebGL2 fallback limitations.** `wgpu`'s `webgl` feature
does not support compute passes (WebGL2 has no compute). ADR-006's
"compute passes" feature is WebGPU-only. **Mitigation**: the renderer
detects the backend at init time and exposes a `features()` method
returning the available feature set. The schedule lowering skips
compute passes on WebGL2.

**Risk R7.3 — WASM binary size.** Adding `wgpu` grows the WASM binary by
~300-500 KB. The audit (`docs/alkalive-wave-00-audit.md:204`) flagged
the 1.05 MB binary as already large; post-Gap-7 it will be 1.4-1.6 MB.
**Mitigation**: run `wasm-opt -Oz` post-build (Wave 0 M8 fix, still
outstanding). Enable `wgpu`'s `strict_features` to drop unused backends.

**Risk R7.4 — Surface loss recovery.** `wgpu::SurfaceError::Lost`
requires reconfiguring the surface, which today's raw-WebGL2 path
handles via context-loss events (`webglcontextlost` /
`webglcontextrestored`). The `wgpu` path handles this internally, but
the renderer must call `surface.configure()` again after a `Lost`.
**Mitigation**: the renderer's `render_compiled` catches `Lost`,
reconfigures, and skips the frame.

**Risk R7.5 — `wgpu::Surface<'static>` lifetime.** The surface borrows
the canvas; making it `'static` requires `wgpu::Surface::from(canvas)`
or `Box::leak`. The `wgpu::SurfaceTarget::Canvas` API handles this
correctly in v23. **Mitigation**: follow the `wgpu` examples' pattern
exactly; do not attempt to store a `Surface<'a>` with a borrowed
lifetime.

**Risk R7.6 — Native build now requires a GPU.** The current native
stub returns `Err` from `init_from_canvas` (line 1381). After Gap 7,
the native build runs real `wgpu` and requires a GPU (or software
renderer). CI must install `lavapipe` (Linux) or use `wgpu`'s
`wgpu::Backends::SECONDARY` to fall back to a CPU renderer.
**Mitigation**: CI uses `xvfb-run` + `lavapipe`; document in
`docs/alkalive-wave-02-rendering.md` (this file's companion runbook).

### 7.12 Open Questions

**Q7.1 — Should the renderer support both `wgpu` and raw WebGL2
simultaneously (behind a feature flag) for the transition period?**
Tentative answer: no. The Gap 7 PR replaces the WebGL2 path entirely;
the `wgpu` `webgl` feature provides the WebGL2 fallback transparently.
Maintaining two paths doubles the test surface and violates ADR-014's
"single source of truth" principle.

**Q7.2 — Should the WGSL sources be embedded via `include_str!` or
loaded at runtime?** Tentative answer: `include_str!` for the three
built-in shaders (text-quad, rect, clear). Runtime loading is reserved
for ADR-006's author-supplied shaders (future). `include_str!` keeps
the shaders in the WASM binary (no fetch latency) and lets the compiler
verify them at build time.

**Q7.3 — How does the `wgpu::Surface` interact with Gap 8's worker
migration?** The surface is created from a canvas; the canvas is owned
by the main thread. To move the surface to a worker, the main thread
calls `canvas.transferControlToOffscreen()` and sends the
`OffscreenCanvas` to the worker via `postMessage`. The worker creates
the `wgpu::Surface` from the `OffscreenCanvas`. **Caveat**: this is a
one-way transfer — the canvas cannot be rendered to from the main
thread afterward.

**Q7.4 — Should the renderer expose the `wgpu::Device` for testing?**
Tentative answer: yes, via a `pub fn device(&self) -> &wgpu::Device`
method. This lets unit tests create test pipelines and textures
without going through `init_from_canvas`.

**Q7.5 — Does the `wgpu::Queue.submit` call block?** No — it queues
the command buffer for execution. The browser presents the frame at
the next vsync. The renderer's `render_compiled` returns immediately
after `submit`. The `output.present()` call is also non-blocking.

---

## §3 Gap 8 — Single-GPU-Device + SAB/COOP-COEP (ADR-003)

### 8.1 Current State (with file:line evidence)

The current runtime is **single-threaded main-thread only**. There are
no Web Workers, no `SharedArrayBuffer`, no COOP/COEP headers, no
compositor.

**Evidence 1 — Thread-local runtime state**
(`crates/alkalive-runtime-wasm/src/lib.rs:142-158`):

```rust
thread_local! {
    static RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);
    static RAF_CLOSURE: RefCell<Option<Closure<dyn FnMut()>>> = RefCell::new(None);
    static RESIZE_CLOSURE: RefCell<Option<Closure<dyn FnMut()>>> = RefCell::new(None);
    static START_TIME_MS: RefCell<f64> = RefCell::new(0.0);
}
```

All state lives in `thread_local!` on the main thread. There is no IPC
mechanism, no `Worker` constructor, no `postMessage` call.

**Evidence 2 — Frame loop owned by the main thread**
(`crates/alkalive-runtime-wasm/src/lib.rs:616-702`):

```rust
fn start_frame_loop() {
    let frame_closure = Closure::new(|| {
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                runtime.time = elapsed_seconds();
                // ...
                runtime.renderer.render_frame(
                    &runtime.scene, &runtime.schedule, runtime.time,
                );
            }
        });
        schedule_next_frame();
    });
    RAF_CLOSURE.with(|cell| { *cell.borrow_mut() = Some(frame_closure); });
    schedule_next_frame();
}
```

The `requestAnimationFrame` callback runs on the main thread; the
renderer (which owns the GPU context) runs on the main thread. There is
no concurrency.

**Evidence 3 — No COOP/COEP headers.**
`deploy/index.html` (read in full above) has no
`Cross-Origin-Opener-Policy` or `Cross-Origin-Embedder-Policy` headers.
The server (Caddyfile at the repo root) has no such headers either.

**Evidence 4 — No `Worker` feature in `web-sys`.**
`crates/alkalive-runtime-wasm/Cargo.toml` (lines 23-35) lists
`HtmlCanvasElement`, `HtmlInputElement`, `Window`, `Document`,
`Element`, `EventTarget`, `KeyboardEvent`, `InputEvent`,
`MouseEvent`, `console`, `Performance`. There is no `Worker`,
`DedicatedWorkerGlobalScope`, `MessageEvent`, or `SharedArrayBuffer`
feature.

**Evidence 5 — ADR-003 is unimplemented.** The audit's gap analysis
(`docs/alkalive-wave-00-audit.md:214`) lists ADR-003 as **Major**:
"Single-threaded, no SAB" vs. the ADR's "Single-GPUDevice + SAB/COOP-COEP
Compositor."

### 8.2 Problem Statement

ADR-003 (line 99) requires: *"a single dedicated render thread owns the
lone `GPUDevice` and serializes every render-graph submission from all
scene graphs ... Scene data (instance tables, transforms, draw lists)
lives in a `SharedArrayBuffer` under COOP/COEP."*

The current single-threaded design:

1. **Blocks the main thread on GPU work.** When `render_frame` issues
   `gl.drawArrays` (or `wgpu::Queue.submit`), the call may block the
   main thread for the duration of the GPU command encoding. On a
   low-end device with a slow GPU, this can drop input events (the
   IME `keydown` listener at `runtime-wasm/src/lib.rs:446`).
2. **Cannot scale to multiple scene graphs.** ADR-003 envisions
   multiple scene graphs (UI, particles, world, overlays) feeding one
   compositor. The current `Runtime` has exactly one `TextSceneData`
   and one `ScheduleIR`; there is no merge point.
3. **Cannot use `SharedArrayBuffer`.** Without COOP/COEP, the browser
   disables `SharedArrayBuffer` (a Spectre mitigation). This blocks
   ADR-021's "WASM sockets over `SharedArrayBuffer`" for on-demand
   worker IPC.
4. **Cannot precompile pipelines off the main thread.** ADR-017's
   "WebGPU pipeline precompilation" should overlap with WASM decode;
   today it runs synchronously on the main thread, lengthening startup.
5. **Cannot recover from context loss independently.** A WebGL2
   context-loss event (`webglcontextlost`) on the main thread takes
   down the entire runtime. A worker-side renderer isolates this.

### 8.3 Why It's Required (ADR Reference)

ADR-003 (decision, `docs/adr/ADR.md:99`): *"Adopt option (a): a single
dedicated render thread owns the lone `GPUDevice` and serializes every
render-graph submission from all scene graphs — the persistent
GPUDevice-owner thread of ADR-021's model ... Scene data lives in a
`SharedArrayBuffer` under COOP/COEP (`Cross-Origin-Opener-Policy:
same-origin`, `Cross-Origin-Embedder-Policy: require-corp`). Graphs
emit immutable render-graph IR; the render thread merges, compiles,
reorders, batches, then submits. The occlusion-cull pass executes on
the render thread against a compositor-wide depth/visibility buffer."*

ADR-003 (consequences, line 106): *"Positive: one authoritative
submission path; no GPUDevice-sharing hazards; graphs compose without
lock-free complexity. Negative (COOP/COEP risk): cross-origin isolation
headers are required; this conflicts with embedding third-party
iframes. Mitigations: `credentialless` COEP or iframe proxying. If
unworkable, fall back to option (b) per-graph separate devices (loses
shared compositor)."*

ADR-021 (line 685): *"Adopt a main thread + on-demand WASM threads
model. The main thread runs the retain-mode render loop ... and owns
the GPUDevice per ADR-003. Additional WASM threads are spawned on
demand for asynchronous tasks ... IPC between threads uses WASM
sockets over `SharedArrayBuffer`."*

ADR-013 (line 511): *"compile the UI itself to WASM so the layout
module issues WebGPU draw calls directly; no WASM↔DOM boundary in the
hot path."*

### 8.4 Relationship to Existing Runtime/Renderer

| Component | Current role | Role after Gap 8 |
|-----------|--------------|-------------------|
| `Runtime` (main thread) | Owns renderer, scene, schedule, frame loop | Owns scene, schedule, graph-builder; **sends `RenderGraph` IR to the render worker via `postMessage`** |
| `WgpuRenderer` (main thread) | Owns WebGL2 context, shaders, VBOs | **Moved to render worker** — owns `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface`, pipeline cache |
| `start_frame_loop` (main thread) | Calls `render_frame` on each RAF | Calls `worker.postMessage({ kind: "render", graph, time })` on each RAF |
| `init_runtime` (main thread) | Creates `WgpuRenderer` | Creates render worker, transfers canvas via `transferControlToOffscreen`, sends init message |
| `deploy/index.html` | No COOP/COEP | **Adds COOP/COEP headers** (server config + `<meta>`-equivalent — see §8.5.4) |
| `Cargo.toml` (runtime-wasm) | No `Worker` feature | **Adds `Worker`, `DedicatedWorkerGlobalScope`, `MessageEvent`, `SharedArrayBuffer`, `OffscreenCanvas`** |

### 8.5 Proposed Design

#### 8.5.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│ Main thread (alkalive-runtime-wasm)                                 │
│                                                                     │
│  Runtime {                                                          │
│    scene: TextSceneData,                                            │
│    schedule: ScheduleIR,                                            │
│    dep_graph: DependencyGraph,                                      │
│    signals: SignalStore,                                            │
│    graph: RenderGraph,           // lowered (Gap 6)                 │
│    compiled: CompiledGraph,      // cached                          │
│    worker: RenderWorkerHandle,   // NEW (Gap 8)                     │
│    // ... time, input_text, original_text                           │
│  }                                                                  │
│                                                                     │
│  start_frame_loop:                                                  │
│    RAF closure:                                                     │
│      1. Update signals (TIME, INPUT_TEXT, CANVAS_*)                 │
│      2. Lower schedule → RenderGraph (if structure changed)         │
│      3. Compile graph (if structure changed)                        │
│      4. worker.postMessage({ kind: "render", graph, compiled,       │
│                               time, signals_snapshot })             │
│      5. schedule_next_frame()                                       │
│                                                                     │
│  Input listeners (keydown, input, click, resize):                  │
│    Update scene + signals (no render call — the next RAF will       │
│    send the updated state to the worker).                           │
└──────────────────────────────────┬──────────────────────────────────┘
                                   │ postMessage({ kind: "render", ... })
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Render worker (alkalive-render-worker, cdylib)                      │
│                                                                     │
│  RenderWorkerState {                                                │
│    device: wgpu::Device,                                            │
│    queue: wgpu::Queue,                                              │
│    surface: wgpu::Surface,        // from OffscreenCanvas           │
│    pipeline_cache: PipelineCache,                                   │
│    buffer_table: Vec<wgpu::Buffer>,                                 │
│    texture_table: Vec<wgpu::Texture>,                               │
│    glyph_run_table: Vec<(u32, u32)>,                                │
│    // ... cached font infrastructure                                │
│  }                                                                  │
│                                                                     │
│  onmessage(msg):                                                    │
│    match msg.kind {                                                 │
│      "init" => { create device, surface, pipelines }                │
│      "render" => { render_compiled(graph, compiled, time) }         │
│      "resize" => { reconfigure surface }                            │
│      "upload_atlas" => { shape + rasterize + upload texture }       │
│    }                                                                │
└─────────────────────────────────────────────────────────────────────┘
```

#### 8.5.2 The Render Worker Crate

A new crate `alkalive-render-worker` is added. It compiles to a `cdylib`
(WASM) loaded by a small JS shim. The crate reuses the renderer logic
from `alkalive-backend-wgpu` (the `wgpu` migration from Gap 7).

```toml
# crates/alkalive-render-worker/Cargo.toml (NEW)

[package]
name = "alkalive-render-worker"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "AlkALive render-thread worker — owns the GPUDevice"

[lib]
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
alkalive-backend-wgpu = { workspace = true }
alkalive-render = { workspace = true }
alkalive-text = { workspace = true }
alkalive-compiler = { workspace = true }

wasm-bindgen = { workspace = true }
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "DedicatedWorkerGlobalScope",
    "MessageEvent",
    "OffscreenCanvas",
    "Window",
    "console",
    "Performance",
]}
serde = { version = "1", features = ["derive"] }
serde-wasm-bindgen = "0.6"
```

The worker's entry point:

```rust
// crates/alkalive-render-worker/src/lib.rs

#![allow(unsafe_code)]

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

/// The render worker's global state. Stored in a thread_local because
/// the worker is single-threaded (one worker = one thread).
struct RenderWorkerState {
    renderer: Option<alkalive_backend_wgpu::WgpuRenderer>,
    canvas: Option<web_sys::OffscreenCanvas>,
}

thread_local! {
    static STATE: std::cell::RefCell<RenderWorkerState> =
        std::cell::RefCell::new(RenderWorkerState { renderer: None, canvas: None });
}

/// Entry point, called from the worker's `onmessage` handler.
#[wasm_bindgen]
pub fn init_worker() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("AlkALive render-worker panic: {}", info).into());
    }));
    install_message_handler();
}

fn install_message_handler() {
    let handler = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(|e: web_sys::MessageEvent| {
        let data = e.data();
        // The message is a JS object { kind: "init" | "render" | "resize", ... }.
        // Deserialize via serde-wasm-bindgen.
        let msg: WorkerMessage = match serde_wasm_bindgen::from_value(data) {
            Ok(m) => m,
            Err(e) => {
                web_sys::console::error_1(
                    &format!("render-worker: failed to deserialize message: {:?}", e).into(),
                );
                return;
            }
        };
        match msg.kind {
            WorkerMessageKind::Init { canvas, width, height } => {
                spawn_local(async move {
                    if let Err(e) = handle_init(canvas, width, height).await {
                        web_sys::console::error_1(&e.into());
                    }
                });
            }
            WorkerMessageKind::Render { graph, compiled, time } => {
                handle_render(graph, compiled, time);
            }
            WorkerMessageKind::Resize { width, height } => {
                handle_resize(width, height);
            }
        }
    });

    let scope = web_sys::window()
        .and_then(|w| w.get("DedicatedWorkerGlobalScope").ok())
        .expect("no DedicatedWorkerGlobalScope");
    let _ = scope;
    // In a worker, `self` is the DedicatedWorkerGlobalScope.
    // Use js_sys::reflect to attach the handler.
    let self_val = js_sys::global();
    let target: &web_sys::EventTarget = self_val.as_ref();
    target.add_event_listener_with_callback("message", handler.as_ref().unchecked_ref())
        .expect("add_event_listener failed");
    handler.forget();
}

#[derive(serde::Deserialize)]
struct WorkerMessage {
    kind: WorkerMessageKind,
}

#[derive(serde::Deserialize)]
enum WorkerMessageKind {
    Init {
        canvas: web_sys::OffscreenCanvas,
        width: u32,
        height: u32,
    },
    Render {
        graph: alkalive_render::RenderGraph,
        compiled: alkalive_render::CompiledGraph,
        time: f32,
    },
    Resize {
        width: u32,
        height: u32,
    },
}

async fn handle_init(
    canvas: web_sys::OffscreenCanvas,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // The WgpuRenderer::init_from_canvas signature accepts an
    // HtmlCanvasElement today. Gap 8 widens it to accept an
    // OffscreenCanvas as well (via wgpu::SurfaceTarget::Canvas, which
    // accepts both).
    let renderer = alkalive_backend_wgpu::WgpuRenderer::init_from_offscreen(
        canvas.clone(), width, height,
    ).await?;

    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.renderer = Some(renderer);
        s.canvas = Some(canvas);
    });
    web_sys::console::log_1(&"AlkALive render-worker ready".into());
    Ok(())
}

fn handle_render(
    graph: alkalive_render::RenderGraph,
    compiled: alkalive_render::CompiledGraph,
    time: f32,
) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(renderer) = s.renderer.as_mut() {
            renderer.render_compiled(&graph, &compiled, time);
        } else {
            web_sys::console::warn_1(
                &"AlkALive render-worker: render before init".into(),
            );
        }
    });
}

fn handle_resize(width: u32, height: u32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(renderer) = s.renderer.as_mut() {
            renderer.resize(width, height);
        }
    });
}
```

#### 8.5.3 The Worker JS Shim

A small JS file that loads the worker WASM and forwards messages:

```js
// crates/alkalive-render-worker/src/worker.js (NEW — built by wasm-bindgen)

import init, { init_worker } from './alkalive_render_worker.js';

// The worker's WASM is loaded relative to this JS file.
await init('./alkalive_render_worker_bg.wasm');

// Install the message handler.
init_worker();

// Signal to the main thread that the worker is ready.
self.postMessage({ kind: 'ready' });
```

The main thread spawns the worker:

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs (NEW function)

fn spawn_render_worker(
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<web_sys::Worker, JsValue> {
    // Transfer canvas control to OffscreenCanvas.
    let offscreen: web_sys::OffscreenCanvas =
        canvas.transfer_control_to_offscreen()?;

    // Spawn the worker.
    let worker = web_sys::Worker::new("/alkalive/render_worker.js")?;

    // Send the init message with the OffscreenCanvas (transferred, not copied).
    let init_msg = serde_wasm_bindgen::to_value(&WorkerInitMessage {
        kind: "init",
        canvas: offscreen,
        width: 0, // filled in by the caller
        height: 0,
    })?;
    worker.post_message_with_transfer(
        &init_msg,
        &[offscreen.as_ref()],
    )?;
    Ok(worker)
}
```

#### 8.5.4 COOP/COEP Headers

COOP and COEP must be HTTP response headers — they cannot be set via
`<meta>` tags. The headers are configured in two places:

**Server config (Caddyfile at repo root):**

```caddy
# Caddyfile (excerpt — appended to the existing site block)

header {
    Cross-Origin-Opener-Policy "same-origin"
    Cross-Origin-Embedder-Policy "require-corp"
    # Optional (modern): credentialless instead of require-corp.
    # Cross-Origin-Embedder-Policy "credentialless"
}
```

**Next.js config (`next.config.ts`):**

```ts
// next.config.ts (NEW — append to the existing config)

const nextConfig = {
  async headers() {
    return [
      {
        source: '/alkalive/:path*',
        headers: [
          { key: 'Cross-Origin-Opener-Policy', value: 'same-origin' },
          { key: 'Cross-Origin-Embedder-Policy', value: 'require-corp' },
        ],
      },
    ];
  },
};
```

**Fallback when COOP/COEP is unavailable** (e.g. embedding AlkALive in a
third-party iframe):

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs (NEW)

/// Detect whether the page is cross-origin isolated (COOP/COEP active).
fn is_cross_origin_isolated() -> bool {
    web_sys::window()
        .and_then(|w| w.get("crossOriginIsolated").ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Decide whether to use the render worker (multi-threaded) or fall back
/// to single-threaded (current behavior).
fn should_use_render_worker() -> bool {
    // 1. Must be cross-origin isolated (for SharedArrayBuffer — though
    //    postMessage works without it, the future on-demand worker pool
    //    per ADR-021 needs SAB).
    if !is_cross_origin_isolated() {
        return false;
    }
    // 2. Must have WebWorker support (universal in evergreen browsers).
    if web_sys::window().and_then(|w| w.get("Worker").ok()).is_none() {
        return false;
    }
    // 3. Must have OffscreenCanvas support (Chrome 69+, Firefox 105+,
    //    Safari 16.4+).
    if !web_sys::js_sys::Reflect::has(
        &web_sys::HtmlCanvasElement::default().into(),
        &"transferControlToOffscreen".into(),
    ).unwrap_or(false) {
        return false;
    }
    true
}
```

In `init_runtime`:

```rust
if should_use_render_worker() {
    let worker = spawn_render_worker(&canvas)?;
    // Store the worker handle in the Runtime; the frame loop will
    // postMessage "render" to it each frame.
    runtime.worker = Some(RenderWorkerHandle::new(worker));
} else {
    // Fallback: create the renderer on the main thread (current behavior).
    let renderer = WgpuRenderer::init_from_canvas(canvas, width, height).await?;
    runtime.renderer = Some(renderer);
}
```

The frame loop:

```rust
if let Some(worker) = &runtime.worker {
    // Multi-threaded path: send the graph to the worker.
    let msg = serde_wasm_bindgen::to_value(&RenderMessage {
        kind: "render",
        graph: runtime.graph.clone(),
        compiled: runtime.compiled.clone(),
        time: runtime.time,
    })?;
    worker.post_message(&msg)?;
} else if let Some(renderer) = runtime.renderer.as_mut() {
    // Single-threaded fallback: render directly.
    renderer.render_compiled(&runtime.graph, &runtime.compiled, runtime.time);
}
```

#### 8.5.5 SharedArrayBuffer for Scene Data (Future)

ADR-003 envisions scene data living in a `SharedArrayBuffer`. For the
Hello World scene (5 passes, 5 draw calls, ~600 bytes of IR), the
overhead of allocating and synchronizing a `SharedArrayBuffer` exceeds
the savings. The `postMessage` path with structured clone is sufficient
for the first cut.

For larger scenes (1000+ draw calls), the `RenderGraph` can be
serialized into a flat `SharedArrayBuffer` and the worker can read it
without copying. This is future work; the IR shapes (the `RenderGraph`
type and its `Clone` impl) are already `Send`-compatible.

```rust
// Future: serialize RenderGraph to a SharedArrayBuffer.

// In crates/alkalive-render/src/lib.rs (NEW — future)

impl RenderGraph {
    /// Serialize to a flat byte buffer (for SharedArrayBuffer transport).
    pub fn to_bytes(&self) -> Vec<u8> { /* postcard-style encoding */ }

    /// Deserialize from a flat byte buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> { /* ... */ }
}
```

#### 8.5.6 Compositor (Future)

ADR-003 mentions a "compositor" that merges graphs from multiple scene
graphs. The existing `compile()` function
(`crates/alkalive-render/src/lib.rs:447`) is the merger. For the Hello
World scene (one graph), the merger is a no-op. For future multi-scene
apps, the render worker calls `compile(&[graph1, graph2, ...])` to
merge before submission. This is enabled by Gap 6's `compile()` call
being inside the renderer (today) / the render worker (after Gap 8).

### 8.6 Runtime/Renderer Implications

1. **The `Runtime` struct gains a `worker: Option<RenderWorkerHandle>`
   field.** When `Some`, the frame loop sends messages to the worker;
   when `None`, it falls back to single-threaded rendering.
2. **The `WgpuRenderer` is no longer on the `Runtime`.** It moves to
   the worker. The `Runtime` keeps a `renderer: Option<WgpuRenderer>`
   field for the fallback path; in the multi-threaded path it is
   `None`.
3. **The canvas is transferred to the worker.** After
   `transferControlToOffscreen()`, the main thread cannot draw to the
   canvas. The worker owns it for the page lifetime.
4. **Input handling stays on the main thread.** The IME `<input>` and
   keyboard listeners (lines 435-518) are unchanged. The main thread
   updates `scene.input_text` and bumps the `INPUT_TEXT` signal; the
   next RAF closure sends the updated scene to the worker.
5. **Resize handling.** The `resize` listener (lines 527-573) sends a
   "resize" message to the worker instead of calling
   `renderer.resize()` directly.
6. **Hit-testing.** The `hit_test_input_field` method (line 1113)
   stays on the main thread (it doesn't need GPU state). The main
   thread reads `runtime.input_field_bounds` (cached from the last
   frame's layout).
7. **Serialization cost.** Each `postMessage` clones the `RenderGraph`
   via structured clone. For the Hello World scene (~600 bytes), this
   is ~10 µs — negligible. For larger scenes, the `to_bytes` /
   `SharedArrayBuffer` path (§8.5.5) eliminates the copy.
8. **Worker startup latency.** Spawning the worker, loading its WASM,
   and creating the `wgpu::Device` adds ~100-300 ms to startup. The
   main thread displays a loading indicator during this window. The
   fallback path (single-threaded) starts immediately.

### 8.7 Browser/Platform Integration

- **COOP/COEP headers required for `SharedArrayBuffer`.** Without them,
  the worker can still be spawned (Web Workers don't require
  cross-origin isolation), but `SharedArrayBuffer` is unavailable.
  The first cut uses `postMessage` (no SAB); the future SAB path
  requires the headers.
- **`OffscreenCanvas` required for canvas transfer.** Available in
  Chrome 69+, Firefox 105+, Safari 16.4+. Older browsers fall back to
  the single-threaded path.
- **`WebGPU` worker support.** WebGPU is available in workers from
  Chrome 113+. The worker creates the `wgpu::Device` via
  `navigator.gpu.requestAdapter()` (which `wgpu` calls internally).
- **Cross-origin iframe embedding.** ADR-003's COEP risk (line 106)
  is real: if AlkALive is embedded in a third-party iframe, the
  iframe's COEP header may conflict with the parent's. **Mitigation**:
  use `Cross-Origin-Embedder-Policy: credentialless` (Chrome 96+,
  Firefox 110+) instead of `require-corp`. `credentialless` allows
  cross-origin resources without explicit CORP headers, at the cost of
  loading them without credentials. If `credentialless` is
  unavailable, fall back to single-threaded (no SAB).

### 8.8 Error Handling

| Error class | Source | Handling |
|-------------|--------|----------|
| `Worker::new` failure | Browser blocks worker creation (e.g. CSP) | Log error; fall back to single-threaded. |
| `transferControlToOffscreen` failure | Canvas already transferred, or not a canvas | Log error; fall back to single-threaded. |
| Worker init timeout | Worker WASM failed to load (404, network error) | Timeout after 5 s; fall back to single-threaded. |
| Worker panic | Uncaught panic in worker WASM | The worker's panic hook logs to console; the main thread detects a missing "ready" message and falls back. |
| `wgpu::SurfaceError::Lost` | Surface lost in worker | Worker reconfigures; if reconfiguration fails, worker posts an "error" message; main thread re-inits the worker. |
| COOP/COEP header missing | `crossOriginIsolated` returns false | `should_use_render_worker()` returns false; single-threaded path is used. |

### 8.9 Testing Strategy

1. **Unit tests in `alkalive-runtime-wasm`** (native, mocked worker):
   - `should_use_render_worker()` returns false when
     `crossOriginIsolated` is false.
   - `should_use_render_worker()` returns false when `Worker` is
     undefined.
   - `should_use_render_worker()` returns false when
     `transferControlToOffscreen` is missing.
2. **Unit tests in `alkalive-render-worker`** (native):
   - The `WorkerMessage` enum deserializes correctly from a JS value.
   - `handle_render` does not panic when given a valid `RenderGraph`.
3. **Integration tests in `alkalive-runtime-wasm`** (wasm32, headless
   browser with COOP/COEP):
   - The runtime spawns the worker, the worker initializes, the worker
     posts "ready" within 5 s.
   - The frame loop sends "render" messages; the worker draws to the
     OffscreenCanvas (verified by reading back a pixel via
     `gl.readPixels` on the worker's `wgpu::Surface`).
4. **Fallback test** (wasm32, headless browser without COOP/COEP):
   - `should_use_render_worker()` returns false; the single-threaded
     path runs; the canvas renders correctly.
5. **Browser-verification test** (manual):
   - Deploy with COOP/COEP headers; verify the worker path runs (check
     DevTools > Console for "AlkALive render-worker ready").
   - Deploy without COOP/COEP headers; verify the fallback path runs.
6. **Regression tests**: all 1148 existing tests pass. The
   single-threaded fallback preserves today's behavior exactly.

### 8.10 Dependencies on Other Gaps

| Dependency | Direction | Detail |
|------------|-----------|--------|
| Gap 6 (render-graph IR) | Gap 8 ← Gap 6 | The worker receives a `RenderGraph` via `postMessage`. Gap 6's `RenderGraph` type is `Clone + Send` (it contains only `Box<[T]>` and `Vec<T>`). |
| Gap 7 (WGSL/wgpu) | Gap 8 ← Gap 7 | The worker creates the `wgpu::Device` + `wgpu::Surface`. Gap 7's `WgpuRenderer` is moved to the worker (the `device` field is `Send` by `wgpu`'s design). |
| ADR-021 (on-demand worker pool) | Gap 8 enables | The render worker is the persistent GPUDevice-owner. Future on-demand workers (for asset decoding, compute) are spawned separately and communicate via `SharedArrayBuffer` sockets. |
| ADR-013 (no WASM↔DOM boundary) | Gap 8 preserves | The worker's `onmessage` handler is a non-hot-path boundary crossing (one `postMessage` per frame). The hot path (graph compilation, draw-call emission) runs entirely inside the worker's WASM. |
| ADR-017 (pipeline precompilation) | Gap 8 enables | The worker can precompile pipelines at startup (overlapping with WASM decode) without blocking the main thread. |

### 8.11 Risks and Trade-offs

**Risk R8.1 — COOP/COEP deployment friction.** ADR-003 acknowledges
(line 110) this as the primary risk. The headers must be set on every
response (HTML, JS, WASM, fonts). A misconfigured CDN or proxy can
break the headers. **Mitigation**: the fallback path
(`should_use_render_worker()` returns false) keeps the app functional
without SAB. The COOP/COEP requirement is opt-in for the multi-threaded
path.

**Risk R8.2 — Worker startup latency.** ~100-300 ms to spawn the
worker, load its WASM, and create the `wgpu::Device`. The main thread
must show a loading indicator. **Mitigation**: the worker's WASM is
precompiled (per ADR-017); the `wgpu::Device` request is async and
overlaps with WASM decode. The fallback path starts immediately.

**Risk R8.3 — `postMessage` serialization overhead.** For large
`RenderGraph`s, structured clone is O(n) in the graph size.
**Mitigation**: §8.5.5's `to_bytes` / `SharedArrayBuffer` path
eliminates the copy. For the Hello World scene, the clone is negligible.

**Risk R8.4 — Worker WASM is a second binary.** The render worker's
WASM is a separate `cdylib` (`alkalive-render-worker`). This adds
~500-800 KB to the deployed artifact (the worker WASM includes its own
copy of `wgpu`, `alkalive-text`, etc.). **Mitigation**: the worker
WASM is loaded only when `should_use_render_worker()` returns true.
Code-splitting the worker load saves bandwidth on the fallback path.

**Risk R8.5 — Debugging a worker is harder.** Browser DevTools support
worker debugging, but the console is split between main and worker.
**Mitigation**: the worker logs to the same console via
`web_sys::console::log_1` (which appears in the main thread's console
in Chrome). Source maps are enabled in dev builds.

**Risk R8.6 — Backward compatibility with the existing single-threaded
demo.** The demo at `deploy/index.html` must continue to render
correctly when COOP/COEP is unavailable. The fallback path is
functionally identical to today's behavior. **Mitigation**: the
fallback path is the default; the worker path is opt-in via
`should_use_render_worker()`.

**Risk R8.7 — `OffscreenCanvas` is not universally supported.** Safari
< 16.4 lacks it. **Mitigation**: `should_use_render_worker()` checks
for `transferControlToOffscreen` and falls back if missing.

### 8.12 Open Questions

**Q8.1 — Should the worker be a `DedicatedWorker` or a `SharedWorker`?**
Tentative answer: `DedicatedWorker`. A `SharedWorker` would allow
multiple AlkALive instances on the same page to share one render
worker, but this introduces resource contention (multiple canvases
fighting for one GPU). ADR-003's "single dedicated render thread" implies
one worker per AlkALive instance.

**Q8.2 — Should the worker use `wasm-bindgen-rayon` for thread pooling?**
Tentative answer: not in the first cut. `wasm-bindgen-rayon` enables
multi-threaded WASM via `SharedArrayBuffer`, but the render worker is
single-threaded by design (it owns the lone `GPUDevice`). Future
on-demand workers (ADR-021) for asset decoding / compute may use
`wasm-bindgen-rayon`.

**Q8.3 — How does the worker handle context loss?** When the
`wgpu::Surface` is lost, the worker reconfigures it. If reconfiguration
fails (e.g. the OffscreenCanvas was detached), the worker posts an
"error" message to the main thread. The main thread re-creates the
worker (which requires a fresh `transferControlToOffscreen` — but the
canvas was already transferred, so the main thread must reload the
page or create a new canvas). **Mitigation**: in practice, context loss
is rare; the fallback is a page reload.

**Q8.4 — Should the worker expose a `wgpu::Device` for testing?**
Tentative answer: yes, via a `pub fn device(&self) -> &wgpu::Device`
method on the worker state. This lets unit tests create test pipelines
without going through `init_worker`.

**Q8.5 — How are fonts loaded in the worker?** Today the font is
embedded via `include_bytes!("../assets/Roboto-Regular.ttf")` (line
1144). The worker WASM also embeds the font (it's a separate cdylib
that includes `alkalive-text`). No font loading via `postMessage` is
needed.

**Q8.6 — What happens if the main thread sends a "render" message
before the worker's "ready"?** The worker's `handle_render` checks
`s.renderer.is_some()`; if `None`, it logs a warning and drops the
message. The main thread buffers up to N "render" messages until
"ready" arrives (today N=1; future N=2 for double-buffering).

---

## §4 Shared Rendering ABI (Gap 6 ↔ Gap 7 ↔ Gap 8)

ADR-001 (line 65) states: *"the compositor (ADR 003) consumes the
compiled graph output and both ADRs must agree on a shared
attachment-format and pass-boundary contract (to be specified in a
future rendering-ABI ADR)."*

This section is that contract.

### 4.1 Attachment Format

The render graph's color attachment (the canvas swapchain texture) is
formatted `AttachmentFormat::Bgra8Unorm` on WebGPU (Chrome's preferred
swapchain format) and `AttachmentFormat::Rgba8UnormSrgb` on WebGL2
fallback (GL's native format).

The renderer detects the surface's preferred format at init time and
stores it as a field:

```rust
// In WgpuRenderer
surface_format: wgpu::TextureFormat,
```

The `schedule_to_render_graph` lowering reads the renderer's
`surface_format` (passed via the `canvas_size` parameter — to be
extended to `(canvas_size, surface_format)`) and emits the matching
`AttachmentFormat`. The `compile()` merger validates that all graphs
in the merge set agree on the format.

### 4.2 Pass Boundary Contract

A "pass boundary" is the point between two passes where the renderer
must guarantee the color attachment's contents are visible to the next
pass. In WebGL2 terms: a `gl.flush` is implicit at the end of each
frame, but explicit between passes only if the passes target different
framebuffers. In `wgpu` terms: passes within one `CommandEncoder` are
ordered; the encoder's `finish()` submits them atomically.

**Contract**: passes within one `RenderGraph` are executed in
`compiled.sorted_passes` order, on one `CommandEncoder`, with no
inter-pass barriers (because they all share the same color attachment
and have no inter-pass data dependencies today).

When the renderer supports per-pass render targets (future, for the
dirty-pass fast path), passes with different color attachments will
require a `wgpu::ImageCopyBarrier` (or a `gl.memoryBarrier` in
WebGL2). The `compile()` function's barrier-insertion step (currently
a no-op — see `crates/alkalive-render/src/lib.rs:443-446`) is the
insertion point.

### 4.3 Draw-Call Parameter Contract

Each `DrawCall` carries:
- `pipeline: PipelineHandle` — looked up in the `PipelineCache`.
- `vertices: VertexBinding` — references a `BufferId` in the
  renderer's `BufferTable`.
- `bindings: Box<[BindGroup]>` — one bind group with the uniform
  buffer + texture + sampler.

The renderer's `BufferTable`, `TextureTable`, `SamplerTable` are
indexed by `BufferId(pub u32)`, `TextureId(pub u32)`,
`SamplerId(pub u32)`. The IDs are stable across frames (the renderer
reuses buffers/textures); the renderer's `ensure_atlas_uploaded`
updates the contents of `TextureId(0)` (the glyph atlas) in place.

### 4.4 Cross-Thread Transport Contract (Gap 8)

When the `RenderGraph` crosses the main-thread → worker boundary via
`postMessage`, the structured-clone algorithm copies the graph's
`Box<[T]>` fields by value. The worker receives an independent copy;
mutations on the main thread do not affect the worker's copy.

For `BufferId` / `TextureId` / `SamplerId`: these are **per-process**
IDs. The worker's `BufferTable` is independent of the main thread's
(the main thread has no GPU buffers today). The IDs in the
`RenderGraph` reference the **worker's** tables. The lowering
(`schedule_to_render_graph`) runs on the main thread but emits IDs
that the worker resolves at render time.

Today's IR uses placeholder IDs (`BufferId(0)` for the glyph atlas)
that the worker resolves to its own `wgpu::Buffer` / `wgpu::Texture`
handles. A future optimization: the worker publishes its table
contents to the main thread at init time, so the lowering can emit
real IDs. This is out of scope for the first cut.

### 4.5 Pipeline Handle Contract

The three pre-allocated pipelines (`PIPELINE_CLEAR`,
`PIPELINE_RECT`, `PIPELINE_TEXT`) are `PipelineHandle(0)`,
`PipelineHandle(1)`, `PipelineHandle(2)`. The renderer's
`PipelineCache` (existing at `crates/alkalive-render/src/lib.rs:646`)
is populated at init time:

```rust
// In WgpuRenderer::init_from_canvas (after device creation):
pipeline_cache.insert(PIPELINE_CLEAR, clear_pipeline_desc, &device);
pipeline_cache.insert(PIPELINE_RECT, rect_pipeline_desc, &device);
pipeline_cache.insert(PIPELINE_TEXT, text_pipeline_desc, &device);
```

Future author-supplied WGSL shaders (ADR-006) insert entries
dynamically, keyed by `shader_hash`.

### 4.6 Time and Uniform Contract

The `time` uniform is passed as a separate `time: f32` parameter to
`render_compiled`, not encoded in the `RenderGraph`. This avoids
re-lowering the graph every frame (the `time` field is a per-frame
value; the graph's structure is static across frames unless the scene
changes).

The renderer's `execute_draw_call` reads `time` and writes it into the
uniform buffer before issuing the draw. The uniform buffer's layout is
defined by the WGSL `Uniforms` struct (§7.5.2) — `rotation`,
`canvas_w`, `canvas_h`, `time`, `text_color`.

---

## §5 Cross-Gap Dependency Resolution and Build Order

### 5.1 Dependency Graph

```
                 ┌──────────────────────────────────────┐
                 │ Gap 6 (Render-Graph IR)              │
                 │ - schedule_to_render_graph           │
                 │ - renderer.render_compiled           │
                 │ - Runtime stores graph + compiled    │
                 └────────────────┬─────────────────────┘
                                  │
                                  │ renderer.render_frame signature
                                  │ changes from (&scene, &schedule, f32)
                                  │ to (&RenderGraph, f32)
                                  ▼
                 ┌──────────────────────────────────────┐
                 │ Gap 7 (WGSL Shaders)                 │
                 │ - Add wgpu = "23" dep                │
                 │ - Migrate shaders to WGSL            │
                 │ - Replace WebGl* fields with wgpu::* │
                 │ - Keep render_compiled signature     │
                 └────────────────┬─────────────────────┘
                                  │
                                  │ WgpuRenderer is now wgpu-based;
                                  │ it can be moved to a worker
                                  ▼
                 ┌──────────────────────────────────────┐
                 │ Gap 8 (Single-GPU-Device + SAB)      │
                 │ - Add alkalive-render-worker crate    │
                 │ - Add COOP/COEP headers               │
                 │ - Spawn worker, transfer canvas       │
                 │ - postMessage RenderGraph IR          │
                 │ - Fallback to single-threaded         │
                 └──────────────────────────────────────┘
```

### 5.2 Mandatory Build Order

The three gaps **must** be implemented in this order:

1. **Gap 6 first.** It introduces the `RenderGraph` IR and the
   `render_compiled` signature. Both Gap 7 and Gap 8 depend on this
   signature being stable.
2. **Gap 7 second.** It swaps the renderer's internals from raw
   WebGL2 to `wgpu`, keeping the `render_compiled` signature stable.
   The WGSL shaders replace the GLSL constants; the `wgpu::Device`
   replaces the `WebGl2RenderingContext`.
3. **Gap 8 third.** It moves the `WgpuRenderer` (now `wgpu`-based)
   to a worker. The worker's `handle_render` calls `render_compiled`
   on its own `WgpuRenderer` instance. The main thread builds the
   `RenderGraph` (via Gap 6's `schedule_to_render_graph`) and sends
   it via `postMessage`.

### 5.3 Why Not in Parallel

- **Gap 6 and Gap 7 cannot be parallelized** because Gap 7's
  `wgpu::RenderPipeline` is the concrete type behind Gap 6's
  `PipelineHandle`. If Gap 7 lands first, the renderer has no
  `RenderGraph` to consume; if Gap 6 lands first, the renderer's
  `render_compiled` works against the existing WebGL2 paths.
- **Gap 7 and Gap 8 cannot be parallelized** because Gap 8's worker
  owns the `wgpu::Device`. If Gap 8 lands first, the worker has no
  `wgpu` to call; if Gap 7 lands first, the renderer is `wgpu`-based
  but on the main thread.

### 5.4 Interface Contracts Between Gaps

| Interface | Defined by | Consumed by | Stable after |
|-----------|------------|-------------|--------------|
| `RenderGraph`, `RenderPass`, `DrawCall`, `Attachment` | Gap 6 (existing types in `alkalive-render`) | Gap 7 (renderer consumes), Gap 8 (worker receives via postMessage) | Gap 6 |
| `DrawCallKind` enum | Gap 6 (new in `alkalive-render`) | Gap 7 (renderer's `execute_draw_call` matches on it) | Gap 6 |
| `schedule_to_render_graph(scheduled, scene, canvas_size) → RenderGraph` | Gap 6 (new in `alkalive-render`) | Gap 8 (main thread calls it, sends result to worker) | Gap 6 |
| `WgpuRenderer::render_compiled(&mut self, graph: &RenderGraph, compiled: &CompiledGraph, time: f32)` | Gap 6 (signature); Gap 7 (impl swapped to wgpu) | Gap 8 (worker calls it) | Gap 7 |
| `WgpuRenderer::init_from_offscreen(canvas: OffscreenCanvas, w, h) → Result<Self, String>` | Gap 7 (new — accepts OffscreenCanvas) | Gap 8 (worker calls it) | Gap 7 |
| `PipelineCache` (existing at `crates/alkalive-render/src/lib.rs:646`) | Gap 7 (populated with wgpu pipelines) | Gap 6 (renderer looks up PipelineHandle), Gap 8 (worker owns the cache) | Gap 7 |
| COOP/COEP headers | Gap 8 (server config) | Gap 8 (browser enforces; `should_use_render_worker` checks `crossOriginIsolated`) | Gap 8 |

### 5.5 Shared Data Structures Ownership

| Structure | Owned by | Populated by | Consumed by |
|-----------|----------|--------------|-------------|
| `RenderGraph` (per-frame) | Runtime (main thread) | `schedule_to_render_graph` (Gap 6) | `compile()` (Gap 6), worker's `render_compiled` (Gap 8) |
| `CompiledGraph` (cached) | Runtime (main thread) | `compile()` (Gap 6) | worker's `render_compiled` (Gap 8) |
| `PipelineCache` | Renderer (Gap 7: main thread; Gap 8: worker) | `WgpuRenderer::init_from_canvas` / `init_from_offscreen` (Gap 7) | `execute_draw_call` (Gap 6) |
| `BufferTable`, `TextureTable`, `SamplerTable` | Renderer (Gap 7: main thread; Gap 8: worker) | `ensure_atlas_uploaded`, `WgpuRenderer::init_*` (Gap 7) | `execute_draw_call` (Gap 6) |
| `GlyphRunTable` | Renderer (Gap 7: main thread; Gap 8: worker) | `upload_text_atlas` (existing) | `execute_draw_call` (Gap 6) |
| `SignalStore`, `DependencyGraph` | Runtime (main thread) | Existing (ADR-025) | Main thread's frame loop (decides whether to re-lower the graph) |

---

## §6 Implementation Sequencing and Effort Estimates

### 6.1 Wave Plan

| Wave | Gaps | Estimated LOC | Estimated tests | Estimated effort |
|------|------|---------------|-----------------|-------------------|
| 2a | Gap 6 only (raw-WebGL2 path retained) | ~1,250 | ~40 | 8-10 days |
| 2b | Gap 7 (wgpu migration) | ~900 | ~25 | 6-8 days |
| 2c | Gap 8 (render worker + COOP/COEP) | ~1,400 | ~30 | 10-14 days |
| **Total** | **3 gaps** | **~3,550** | **~95** | **24-32 days** |

### 6.2 Per-File Impact Summary

| File | Gap 6 | Gap 7 | Gap 8 | Total LOC added/changed |
|------|-------|-------|-------|-------------------------|
| `crates/alkalive-render/src/lib.rs` | +400 (lowering, DrawCallKind, populated VertexBinding/IndexBinding/BindGroup) | +50 (PipelineCache tweaks) | 0 | +450 |
| `crates/alkalive-backend-wgpu/src/lib.rs` | +400 (render_compiled, execute_pass, execute_draw_call) | -800 (remove WebGL2 paths) +700 (wgpu paths) | +50 (init_from_offscreen) | +350 net |
| `crates/alkalive-backend-wgpu/src/shaders/text_quad.wgsl` | 0 | +60 (new file) | 0 | +60 |
| `crates/alkalive-backend-wgpu/src/shaders/rect.wgsl` | 0 | +45 (new file) | 0 | +45 |
| `crates/alkalive-backend-wgpu/Cargo.toml` | +2 (alkalive-render dep) | +5 (wgpu dep, web-sys features) | +2 (OffscreenCanvas feature) | +9 |
| `crates/alkalive-runtime-wasm/src/lib.rs` | +120 (Runtime.graph + compiled fields, frame loop update) | +5 (init_from_canvas signature) | +250 (spawn_render_worker, should_use_render_worker, fallback path) | +375 |
| `crates/alkalive-runtime-wasm/Cargo.toml` | 0 | 0 | +8 (Worker, OffscreenCanvas, etc. features, serde deps) | +8 |
| `crates/alkalive-render-worker/src/lib.rs` | 0 | 0 | +400 (new crate) | +400 |
| `crates/alkalive-render-worker/Cargo.toml` | 0 | 0 | +25 (new file) | +25 |
| `crates/alkalive-render-worker/src/worker.js` | 0 | 0 | +20 (new file) | +20 |
| `deploy/index.html` | 0 | 0 | +5 (loading indicator) | +5 |
| `Caddyfile` | 0 | 0 | +5 (COOP/COEP headers) | +5 |
| `next.config.ts` | 0 | 0 | +15 (COOP/COEP headers) | +15 |
| Tests (across crates) | +400 | +250 | +300 | +950 |

### 6.3 Critical Path

The critical path is **Gap 6 → Gap 7 → Gap 8**, sequentially. Each gap
has a hard dependency on the previous one's signature stability.

Within each gap, the critical path is:

- **Gap 6**: (a) define `DrawCallKind` + populated `VertexBinding`/etc.,
  (b) write `schedule_to_render_graph`, (c) rewrite `render_frame` →
  `render_compiled`, (d) update `Runtime` to store graph + compiled, (e)
  update frame loop.
- **Gap 7**: (a) add `wgpu` dep, (b) write WGSL shaders, (c) replace
  `WgpuRenderer` fields with `wgpu::*`, (d) rewrite `init_from_canvas`
  with `wgpu::Instance`/`Surface`/`Device`/`Queue`, (e) rewrite
  `render_compiled` with `wgpu::RenderPass`, (f) update native stub.
- **Gap 8**: (a) add COOP/COEP headers, (b) write `alkalive-render-worker`
  crate, (c) write worker JS shim, (d) add `should_use_render_worker` +
  fallback, (e) update `Runtime` with `worker: Option<...>` field, (f)
  update frame loop to `postMessage`.

---

## §7 Consolidated Open Questions

| # | Question | Section | Tentative answer |
|---|----------|---------|------------------|
| Q6.1 | Where does `schedule_to_render_graph` live? | §6.12 | `alkalive-render` (adds dep on `alkalive-compiler`) |
| Q6.2 | Is `Clear` a real GPU pipeline or a renderer fast path? | §6.12 | Renderer fast path (`LoadOp::Clear` / `gl.clear`) |
| Q6.3 | How does the dirty-pass fast path interact with the linear-chain edge graph? | §6.12 | Needs sparse edges + per-pass render targets (future) |
| Q6.4 | Should `RenderGraph` be `Send`? | §6.12 | Yes — it already is (only `Box<[T]>` fields) |
| Q6.5 | What happens on scene change (HMR)? | §6.12 | `Runtime::rebuild_graph` (future, ADR-015) |
| Q7.1 | Support `wgpu` and raw WebGL2 simultaneously? | §7.12 | No — `wgpu` `webgl` feature provides the fallback |
| Q7.2 | Embed WGSL via `include_str!` or load at runtime? | §7.12 | `include_str!` for built-ins; runtime for author-supplied (future) |
| Q7.3 | How does `wgpu::Surface` interact with Gap 8's worker? | §7.12 | `transferControlToOffscreen` → worker creates surface from OffscreenCanvas |
| Q7.4 | Expose `wgpu::Device` for testing? | §7.12 | Yes — `pub fn device(&self) -> &wgpu::Device` |
| Q7.5 | Does `wgpu::Queue.submit` block? | §7.12 | No — non-blocking; presents at next vsync |
| Q8.1 | `DedicatedWorker` or `SharedWorker`? | §8.12 | `DedicatedWorker` (one worker per AlkALive instance) |
| Q8.2 | Use `wasm-bindgen-rayon` for thread pooling? | §8.12 | Not in the first cut; render worker is single-threaded |
| Q8.3 | How does the worker handle context loss? | §8.12 | Reconfigure; if that fails, post "error" + reload page |
| Q8.4 | Expose `wgpu::Device` from the worker for testing? | §8.12 | Yes — `pub fn device(&self) -> &wgpu::Device` on worker state |
| Q8.5 | How are fonts loaded in the worker? | §8.12 | Embedded via `include_bytes!` in the worker WASM |
| Q8.6 | What if "render" arrives before "ready"? | §8.12 | Buffer N=1 messages; drop on overflow with warning |

---

## §8 Appendix A — Per-File Impact Summary (Consolidated)

### 8.1 Files Modified

| File | Gap | Change type |
|------|-----|-------------|
| `crates/alkalive-render/src/lib.rs` | 6 | Add `DrawCallKind`, `GlyphRunId`, `Topology`; populate `VertexBinding`/`IndexBinding`/`BindGroup`; add `schedule_to_render_graph`; add `BufferId`/`TextureId`/`SamplerId`/`VertexAttribute`/`VertexFormat`/`IndexFormat` types. |
| `crates/alkalive-render/Cargo.toml` | 6 | Add `alkalive-compiler = { workspace = true }`, `alkalive-backend-wgpu = { workspace = true }` deps. |
| `crates/alkalive-backend-wgpu/src/lib.rs` | 6, 7, 8 | (Gap 6) Rewrite `render_frame` → `render_compiled`, add `execute_pass` + `execute_draw_call`, add `lookup_kind` helper. (Gap 7) Replace `WebGl*` fields with `wgpu::*`, replace GLSL constants with `include_str!` of WGSL files, rewrite `init_from_canvas` and `render_compiled` with `wgpu` calls. (Gap 8) Add `init_from_offscreen` accepting `OffscreenCanvas`. |
| `crates/alkalive-backend-wgpu/Cargo.toml` | 6, 7, 8 | (Gap 6) Add `alkalive-render = { workspace = true }`. (Gap 7) Add `wgpu = { version = "23", features = ["webgpu", "webgl"] }`, add `OffscreenCanvas`/`Worker`/`MessageEvent` to web-sys features. (Gap 8) Add `GpuCanvasContext` feature (already present). |
| `crates/alkalive-runtime-wasm/src/lib.rs` | 6, 8 | (Gap 6) Add `graph: RenderGraph` + `compiled: CompiledGraph` fields to `Runtime`; update `init_runtime` to lower the graph; update `frame_closure` to call `render_compiled(&graph, &compiled, time)`. (Gap 8) Add `worker: Option<RenderWorkerHandle>` field; add `spawn_render_worker`, `should_use_render_worker`, `is_cross_origin_isolated` functions; update `init_runtime` to spawn worker or fall back; update `frame_closure` to `postMessage` or call `render_compiled` directly; update `setup_resize_listener` to `postMessage` "resize". |
| `crates/alkalive-runtime-wasm/Cargo.toml` | 8 | Add `Worker`, `OffscreenCanvas`, `MessageEvent`, `DedicatedWorkerGlobalScope` to web-sys features; add `serde = { version = "1", features = ["derive"] }`, `serde-wasm-bindgen = "0.6"` deps. |
| `deploy/index.html` | 8 | Add a `<div id="loading">` element shown during worker startup; hide it after the worker posts "ready". |
| `Caddyfile` (repo root) | 8 | Add `header { Cross-Origin-Opener-Policy "same-origin"; Cross-Origin-Embedder-Policy "require-corp" }` block. |
| `next.config.ts` | 8 | Add `async headers()` returning COOP/COEP for `/alkalive/:path*`. |

### 8.2 Files Added

| File | Gap | Purpose |
|------|-----|---------|
| `crates/alkalive-backend-wgpu/src/shaders/text_quad.wgsl` | 7 | WGSL text-quad shader (vertex + fragment) |
| `crates/alkalive-backend-wgpu/src/shaders/rect.wgsl` | 7 | WGSL rect shader (vertex + fragment) |
| `crates/alkalive-backend-wgpu/src/shaders/README.md` | 7 | Documents the shader directory |
| `crates/alkalive-render-worker/Cargo.toml` | 8 | New crate manifest |
| `crates/alkalive-render-worker/src/lib.rs` | 8 | Worker entry point, message handler, state |
| `crates/alkalive-render-worker/src/worker.js` | 8 | JS shim that loads worker WASM and calls `init_worker` |

### 8.3 Files Removed

| File | Gap | Reason |
|------|-----|--------|
| (none) | — | No files are removed. The GLSL constants (`VERTEX_SHADER_SRC`, etc.) are removed from `lib.rs` but the file itself stays. The native stub at `backend-wgpu/src/lib.rs:1348-1429` is replaced (not removed). |

---

## §9 Appendix B — Final Crate Dependency Graph

```
alkalive-core (no deps)
    ↑
    │
alkalive-text ──▶ alkalive-core
    │              harfrust (vendored)
    │              rasterizer (vendored)
    │              read-fonts
    ▲
    │
alkalive-backend-wgpu ──▶ alkalive-text
    │                       alkalive-compiler
    │                       alkalive-render (NEW — Gap 6)
    │                       wgpu = "23" (NEW — Gap 7)
    │                       bytemuck, wasm-bindgen, web-sys, js-sys
    ▲
    │
alkalive-render-worker (NEW — Gap 8) ──▶ alkalive-backend-wgpu
    │                                     alkalive-render
    │                                     alkalive-text
    │                                     alkalive-compiler
    │                                     wasm-bindgen, web-sys, js-sys
    │                                     serde, serde-wasm-bindgen
    ▲
    │
alkalive-runtime-wasm ──▶ alkalive-backend-wgpu
    │                     alkalive-compiler
    │                     alkalive-text
    │                     alkalive-render (NEW — Gap 6, for RenderGraph type)
    │                     alkalive-render-worker (NEW — Gap 8)
    │                     wasm-bindgen, web-sys, js-sys
    │                     serde, serde-wasm-bindgen (NEW — Gap 8)
    ▼
alkalive-compiler ──▶ alkalive-core
    │
    │ (CLI feature: serde_json)
    ▼
alkalive-render ──▶ alkalive-core
    │               alkalive-compiler (NEW — Gap 6, for ScheduleIR types)
    │               alkalive-backend-wgpu (NEW — Gap 6, for TextSceneData)
    │               std collections
```

The new edges introduced by Wave 2:

1. `alkalive-render` → `alkalive-compiler` (for `schedule_to_render_graph` input types).
2. `alkalive-render` → `alkalive-backend-wgpu` (for `TextSceneData` input type).
3. `alkalive-backend-wgpu` → `alkalive-render` (for `RenderGraph`/`CompiledGraph`/`PipelineCache`).
4. `alkalive-backend-wgpu` → `wgpu` (for the GPU backend).
5. `alkalive-render-worker` → everything (new crate).
6. `alkalive-runtime-wasm` → `alkalive-render` (for `RenderGraph` type).
7. `alkalive-runtime-wasm` → `alkalive-render-worker` (for spawning the worker).

**Potential cycle check**: edges 1, 2, 3 form a cycle
(`alkalive-render` ↔ `alkalive-backend-wgpu`). This is broken by
moving `TextSceneData` to a new tiny crate `alkalive-scene-data` (or to
`alkalive-core`). For the first cut, the cycle is avoided by having
`schedule_to_render_graph` accept a generic `SceneData` trait instead
of the concrete `TextSceneData` struct; the trait is defined in
`alkalive-render`, and `alkalive-backend-wgpu` implements it for
`TextSceneData`. This keeps the dep graph acyclic:

```
alkalive-render (defines SceneData trait)
    ↑
    │ (implements SceneData for TextSceneData)
alkalive-backend-wgpu ──▶ alkalive-render (uses RenderGraph, etc.)
```

This is a small wart but acceptable. The follow-up cleanup moves
`TextSceneData` out of `alkalive-backend-wgpu` into a new
`alkalive-scene-data` crate, eliminating the trait.

---

## §10 DoD Checklist

- [x] Fine draft saved to `docs/alkalive-fine-draft-rendering.md`.
- [x] All 3 gaps (6, 7, 8) covered with the full 12-section structure
      (current state, problem statement, ADR reference, relationship to
      existing runtime/renderer, proposed design, runtime/renderer
      implications, browser/platform integration, error handling,
      testing strategy, dependencies on other gaps, risks and
      trade-offs, open questions).
- [x] Cross-gap dependencies resolved (§4 shared rendering ABI, §5
      build order, §5.4 interface contracts, §5.5 shared data
      structures ownership).
- [x] Existing code referenced with file:line evidence
      (renderer at `backend-wgpu/src/lib.rs:901-1034`,
      shaders at `:186-289`,
      runtime thread-locals at `runtime-wasm/src/lib.rs:142-158`,
      frame loop at `:616-702`,
      ADR-001/003/006/013/021 in `docs/adr/ADR.md`).
- [x] Existing `alkalive-render` IR types referenced (not reinvented)
      — `RenderGraph` at `crates/alkalive-render/src/lib.rs:254-267`,
      `compile()` at `:447-555`, `PipelineCache` at `:646`.
- [x] WGSL shader sources specified in full (§7.5.2).
- [x] COOP/COEP header configuration specified for both Caddy and
      Next.js (§8.5.4).
- [x] Fallback path (single-threaded) specified for when COOP/COEP or
      `OffscreenCanvas` is unavailable (§8.5.4).
- [x] Per-file impact summary tabulated (§6.2, §8).
- [x] Final crate dependency graph documented (§9).
- [x] Open questions consolidated (§7) with tentative answers.
- [x] Worklog appended (Task ID 2).

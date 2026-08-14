# AlkALive Detailed Specification — Rendering / Runtime Gaps (Task ID 6)

> **Status:** Detailed implementation specification derived from
> `docs/alkalive-fine-draft-rendering.md` (approved fine draft) and the
> critical-review findings in
> `docs/alkalive-fine-draft-critical-review.md`.
> **Predecessors:** Wave 0 audit (`docs/alkalive-wave-00-audit.md`), Wave 1
> bugfixes (`docs/alkalive-wave-01-bugfixes.md`), ADRs 001 / 003 / 005 / 006 /
> 007 / 013 / 017 / 021 / 022 / 025.
> **Audience:** Implementer agents who will turn each section into code. Every
> requirement here is **testable**, **unambiguous**, and **implementable
> without reinterpretation** — exact Rust types, exact WGSL source, exact HTTP
> header values, and exact error messages are specified.

> **Critical-review findings addressed in this specification:**
> - **CR-1** (`RenderGraph` lacks `Serialize`/`Deserialize`) — resolved in
>   §1.2 (full serde derive set added to every IR type) and §3.2 (the worker
>   message protocol uses `serde_wasm_bindgen` end-to-end).
> - **CR-3** (crate dependency cycle `alkalive-render` ↔
>   `alkalive-backend-wgpu`) — resolved in §0.3 and §1.3 (the cycle is broken
>   structurally by moving `TextSceneData` into a new `alkalive-scene-data`
>   crate; the `SceneData` trait mitigation is **not** adopted).
> - **CR-4** (`DrawCall` lacks `id` field) — resolved in §1.2 (the `id` and
>   `kind` fields are added to `DrawCall` **in the same PR** as the rest of
>   Gap 6; the two-phase edit is forbidden).
> - **CR-5** (wgpu `render_compiled` hardcodes `LoadOp::Clear(BLACK)`) —
>   resolved in §2.7 (the clear color is sourced from the first
>   `DrawCallKind::Clear { color }` in the graph; black is the fallback only
>   when no Clear draw call is present, with a `web_sys::console::warn_1`).
> - **CR-6** (`render_frame_with_dirty` removed without working replacement) —
>   resolved in §1.7 (the `dirty` parameter on `compile()` is implemented, not
>   ignored; `compile()` emits `dirty_passes: Vec<PassId>` on `CompiledGraph`;
>   the renderer skips passes outside this set when the set is non-empty).
> - **CR-11** (`class Component { fn render(self) -> RenderGraph }` contract
>   not implemented) — resolved in §0.4 and §1.10 (the bridge is explicitly
>   **out of scope** for this wave; the `RenderGraph` produced by
>   `schedule_to_render_graph` is the only source of graphs until the OO model
>   lands in a future wave per `alkalive-specification-language.md` Gap 1).
> - **CR-12** (`next.config.ts` / `Caddyfile` don't exist; deployment
>   assumptions wrong) — resolved in §3.8 (the actual deployment is a static
>   `deploy/index.html` served by any HTTP server; the spec provides an exact
>   `Caddyfile` example that **is added to the repo** as the canonical dev
>   server, plus portable header documentation for any other server).
> - **CR-13** (per-frame `compile()` redundant) — resolved in §1.7 (the
>   convenience `render_frame(&graph, time)` method is **removed**; callers
>   must pass a `&CompiledGraph`; the runtime caches it across frames and
>   rebuilds only on structural scene change).
> - Additional minor findings addressed: **CR-19** (worker path no longer
>   gated on COOP/COEP for the first cut — `postMessage` works without it;
>   COOP/COEP is required only for the future SAB path), **CR-21** (`compile()`
>   third arg documented as `&DepthBuffer`), **CR-22** (`BarrierCycle` is the
>   actual variant name; `CycleDetected` is removed from this spec),
>   **CR-23** (`RenderPass` naming collision documented; the
>   `alkalive_compiler::schedule::RenderPass` is referred to as
>   "schedule-pass" throughout), **CR-26** (the `dirty` parameter is now
>   functional), **CR-27** (placeholder bounds in `DrawRect` are exposed via a
>   `#[test]` so the wart is visible until ADR-004 lands), **CR-29** (the dead
>   `wgpu-backend = []` feature is removed), **CR-30** (the dead `algorithm`
>   parameter on `lower_pass_kind` is removed), **CR-31** (a `// SAFETY`
>   comment is added near `DrawCallKind::DrawCustom` noting the unsafe casting
>   requirement in the backend).

---

## Table of Contents

- §0 Cross-Gap Dependency Resolution
  - §0.1 Three-gap dependency graph
  - §0.2 Mandatory build order
  - §0.3 Crate dependency graph (cycle broken)
  - §0.4 Out-of-scope items (CR-11, ADR-006 compute passes, ADR-021 on-demand pool)
- §1 Gap 6 — Render-Graph IR (ADR-001)
  - §1.1 Exact requirements (REND-601..REND-629)
  - §1.2 Data structures (Rust types)
  - §1.3 Interfaces and contracts (function signatures)
  - §1.4 State transitions (graph lifecycle, runtime lifecycle)
  - §1.5 Error cases
  - §1.6 Validation rules
  - §1.7 Performance requirements
  - §1.8 Browser/platform integration
  - §1.9 Test cases
  - §1.10 Acceptance criteria
  - §1.11 Traceability
- §2 Gap 7 — WGSL Shaders (ADR-006)
  - §2.1 Exact requirements (REND-701..REND-724)
  - §2.2 Data structures
  - §2.3 Interfaces and contracts
  - §2.4 State transitions (pipeline cache, surface)
  - §2.5 Error cases
  - §2.6 Validation rules
  - §2.7 Performance requirements
  - §2.8 Browser/platform integration
  - §2.9 Test cases
  - §2.10 Acceptance criteria
  - §2.11 Traceability
- §3 Gap 8 — Single-GPU-Device + SAB/COOP-COEP (ADR-003 + ADR-021)
  - §3.1 Exact requirements (REND-801..REND-828)
  - §3.2 Data structures (worker message types)
  - §3.3 Interfaces and contracts
  - §3.4 State transitions (worker lifecycle)
  - §3.5 Error cases
  - §3.6 Validation rules
  - §3.7 Performance requirements
  - §3.8 Browser/platform integration (exact COOP/COEP headers + worker spawn)
  - §3.9 Test cases
  - §3.10 Acceptance criteria
  - §3.11 Traceability
- §4 Consolidated Traceability Matrix
- §5 DoD Checklist

---

## §0 Cross-Gap Dependency Resolution

### 0.1 Three-Gap Dependency Graph

```
                 ┌──────────────────────────────────────────────────┐
                 │ Gap 6 (Render-Graph IR)                         │
                 │ - schedule_to_render_graph                      │
                 │ - populated DrawCall with id + kind             │
                 │ - CompiledGraph carries dirty_passes            │
                 │ - WgpuRenderer::render_compiled signature       │
                 └────────────────┬─────────────────────────────────┘
                                  │
                                  │ RenderGraph + CompiledGraph types
                                  │ and render_compiled signature are
                                  │ stable after Gap 6 merges.
                                  ▼
                 ┌──────────────────────────────────────────────────┐
                 │ Gap 7 (WGSL Shaders + wgpu)                     │
                 │ - wgpu = "23" dep                                │
                 │ - text_quad.wgsl, rect.wgsl replace GLSL        │
                 │ - WgpuRenderer fields swap WebGl* -> wgpu::*    │
                 │ - render_compiled signature unchanged           │
                 │ - clear color sourced from DrawCallKind::Clear  │
                 └────────────────┬─────────────────────────────────┘
                                  │
                                  │ WgpuRenderer is now wgpu-based;
                                  │ the device is Send and can be
                                  │ moved to a worker.
                                  ▼
                 ┌──────────────────────────────────────────────────┐
                 │ Gap 8 (Single-GPU-Device + SAB/COOP-COEP)       │
                 │ - alkalive-render-worker crate (cdylib)          │
                 │ - COOP/COEP headers (Caddyfile added)            │
                 │ - OffscreenCanvas transfer; postMessage protocol │
                 │ - should_use_render_worker() + single-threaded   │
                 │   fallback                                       │
                 └──────────────────────────────────────────────────┘
```

### 0.2 Mandatory Build Order

| Step | Gap | Predecessors | Why this order |
|------|-----|--------------|----------------|
| 1 | **Gap 6** — Render-Graph IR | (none) | Defines the IR types (`RenderGraph`, `CompiledGraph`, `DrawCall` with `id`+`kind`), the lowering function `schedule_to_render_graph`, and the renderer's `render_compiled(&graph, &compiled, time)` signature. Both Gap 7 and Gap 8 depend on this signature being stable. |
| 2 | **Gap 7** — WGSL Shaders + wgpu | Gap 6 | Swaps the renderer internals from raw WebGL2 to `wgpu`, keeping `render_compiled`'s signature unchanged. The `wgpu::Device` is `Send`, which Gap 8 requires. |
| 3 | **Gap 8** — Single-GPU-Device + SAB/COOP-COEP | Gap 6, Gap 7 | Moves the `wgpu`-based `WgpuRenderer` to a dedicated Web Worker. The worker calls `render_compiled` on its own `WgpuRenderer` instance; the main thread builds `RenderGraph` (via Gap 6's `schedule_to_render_graph`) and sends it via `postMessage`. |

**Why the gaps cannot be parallelised:**

- Gap 6 ↔ Gap 7: `wgpu::RenderPipeline` is the concrete type behind Gap 6's
  `PipelineHandle`. Gap 7-first leaves the renderer with no `RenderGraph` to
  consume; Gap 6-first keeps `render_compiled` working against the existing
  WebGL2 path.
- Gap 7 ↔ Gap 8: Gap 8's worker owns the `wgpu::Device`. Gap 8-first leaves
  the worker with no `wgpu` to call; Gap 7-first leaves the renderer
  `wgpu`-based but on the main thread.

### 0.3 Crate Dependency Graph (cycle broken — CR-3 resolution)

The fine draft acknowledged a cycle: `alkalive-render` (for `schedule_to_render_graph`)
depends on `alkalive-backend-wgpu` (for `TextSceneData`), while
`alkalive-backend-wgpu` (for `RenderGraph`) depends on `alkalive-render`. The
fine draft proposed a `SceneData` trait to paper over this; the critical review
(CR-3) found the trait's method set unspecified.

**This specification breaks the cycle structurally** by moving
`TextSceneData` into a new tiny crate, `alkalive-scene-data`. The trait
mitigation is **not** adopted because (a) the trait method set would have to
grow every time `TextSceneData` gains a field (e.g. ADR-004 layout output),
creating a cross-crate coordination cost; (b) the cycle exists only because
`TextSceneData` is in the wrong crate; (c) a small crate extraction is a
one-time refactor that pays for itself.

```
alkalive-core (no deps)
    │
    ▼
alkalive-scene-data (NEW — Gap 6)   ←── defines TextSceneData
    │   depends on: alkalive-core
    │
    ▼
alkalive-text ──▶ alkalive-core
    │
    ▼
alkalive-render ──▶ alkalive-core
    │                alkalive-compiler  (NEW — Gap 6 dep for ScheduleIR)
    │                alkalive-scene-data (NEW — Gap 6 dep for TextSceneData)
    │
    ▼
alkalive-backend-wgpu ──▶ alkalive-text
    │                       alkalive-compiler
    │                       alkalive-render    (NEW — Gap 6)
    │                       alkalive-scene-data (NEW — Gap 6)
    │                       wgpu = "23"        (NEW — Gap 7)
    │                       bytemuck, wasm-bindgen, web-sys, js-sys
    ▲
    │
alkalive-render-worker (NEW — Gap 8) ──▶ alkalive-backend-wgpu
    │                                     alkalive-render
    │                                     alkalive-scene-data
    │                                     alkalive-text
    │                                     alkalive-compiler
    │                                     wasm-bindgen, web-sys, js-sys
    │                                     serde, serde-wasm-bindgen
    ▲
    │
alkalive-runtime-wasm ──▶ alkalive-backend-wgpu
    │                     alkalive-compiler
    │                     alkalive-text
    │                     alkalive-render        (NEW — Gap 6)
    │                     alkalive-scene-data    (NEW — Gap 6)
    │                     alkalive-render-worker (NEW — Gap 8)
    │                     wasm-bindgen, web-sys, js-sys
    │                     serde, serde-wasm-bindgen (NEW — Gap 8)
    ▼
alkalive-compiler ──▶ alkalive-core
```

**No cycles exist.** Every arrow is one-way. The new crate
`alkalive-scene-data` has exactly one source file (`src/lib.rs`) that contains
the moved `TextSceneData` struct, its `Default` impl, its `new()` constructor,
and its `background_normalized()` helper.

### 0.4 Out-of-Scope Items

This specification explicitly defers the following items to future waves:

1. **CR-11 — `class Component { fn render(self) -> RenderGraph }` bridge.**
   The OO model (per `alkalive-specification-language.md` Gap 1) produces
   class instances; the render-graph IR consumes `RenderGraph` structs. The
   bridge between them — host-side calling of `Component::render()` and
   extraction of the returned `RenderGraph` from WASM linear memory — is
   **not implemented** by this wave. The only source of `RenderGraph` after
   Gap 6 is `schedule_to_render_graph(&ScheduledScene, &TextSceneData,
   canvas_size)`. The OO bridge is a future wave that depends on Gap 1
   landing first. ADR-007 ("module objects ARE the render objects") is
   therefore **not yet satisfied** by this wave; the runtime uses the
   schedule-driven lowering instead. This is a known scoping decision, not a
   regression.
2. **ADR-006 compute passes.** The `DrawCallKind::DrawCustom` variant is
   defined (§1.2) but the author-supplied WGSL path (loading a `.wgsl` file
   from `.alk` source, hashing it, looking up the pipeline in the cache) is
   not implemented in this wave. The variant is in the IR so the future
   addition is non-breaking.
3. **ADR-021 on-demand worker pool.** Only the persistent render worker
   (Gap 8) is implemented. On-demand workers for asset decoding, compute,
   and IPC are future work; the render worker is single-threaded and owns
   the lone `GPUDevice`.
4. **SharedArrayBuffer transport.** §3.8 uses `postMessage` with structured
   clone for the first cut. The `to_bytes` / `from_bytes` SAB path is
   future work; the `RenderGraph` type carries `Serialize`/`Deserialize`
   derives (CR-1) so the future SAB path is non-breaking.
5. **Per-pass render targets (full ADR-002 dirty-rect fast path).** §1.7
   implements the `dirty` parameter on `compile()` (CR-6, CR-26) but the
   renderer cannot yet skip individual passes without leaving ghosts of
   stale passes (WebGL2's single-buffered clear). Per-pass render targets
   are future work; the dirty info is plumbed through and the
   `CompiledGraph.dirty_passes` field is populated, but the renderer
   currently still runs all passes when the set is non-empty (with a
   documented `// TODO: per-pass render targets` comment).

---

# Gap 6 — Render-Graph IR (ADR-001)

## 1.1 Exact requirements

### IR types and serde

- **REND-601.** Every public type in `crates/alkalive-render/src/lib.rs` that
  is part of the render-graph IR MUST derive `serde::Serialize` and
  `serde::Deserialize`. The exact set: `PassId`, `AttachmentId`, `DrawCallId`,
  `Vec2`, `DirtyRect`, `Attachment`, `RenderPass`, `VertexBinding`,
  `IndexBinding`, `BindGroup`, `DrawCall`, `DrawCallKind`, `GlyphRunId`,
  `Topology`, `VertexAttribute`, `VertexFormat`, `IndexFormat`, `BufferId`,
  `TextureId`, `SamplerId`, `OcclusionCullPass`, `RenderGraph`, `ExtentOrRelative`,
  `AttachmentFormat`, `SampleCount`, `ClearOp`, `PassType`, `PipelineHandle`,
  `PipelineDesc`, `CompiledGraph`, `DepthBuffer`. (CR-1)
- **REND-602.** `crates/alkalive-render/Cargo.toml` MUST add `serde = { version
  = "1", features = ["derive"] }` to `[dependencies]`.
- **REND-603.** For types with `Box<[T]>` fields (`RenderGraph`,
  `RenderPass`, `VertexBinding`, `BindGroup`), the serde derive MUST
  serialise the box as a sequence — the default `#[derive(Serialize)]`
  behaviour for `Box<[T]>` is correct and requires no custom impl.
- **REND-604.** For `Range<u32>` (the `DrawCall.instances` field), the serde
  representation MUST be a two-element sequence `[start, end]`. The default
  `#[derive(Serialize)]` for `Range<u32>` produces this; no custom impl
  required.

### `DrawCall` shape (CR-4 resolution)

- **REND-605.** The `DrawCall` struct MUST have the following fields, in this
  order: `id: DrawCallId`, `kind: DrawCallKind`, `pipeline: PipelineHandle`,
  `vertices: VertexBinding`, `indices: Option<IndexBinding>`,
  `bindings: Box<[BindGroup]>`, `instances: Range<u32>`,
  `scissor: Option<DirtyRect>`. The `id` and `kind` fields MUST be added
  **in the same PR** as the rest of Gap 6 — the two-phase edit (first PR
  with placeholder `id_for_lookup()`, second PR adding the fields) is
  **forbidden**. (CR-4)
- **REND-606.** The placeholder `trait DrawCallLookup { fn id_for_lookup(&self)
  -> DrawCallId; }` and its `impl DrawCallLookup for DrawCall` from the fine
  draft §6.5.6 MUST NOT be merged. The lookup `graph.draw_calls.iter().find(|d|
  d.id == dc_id)` MUST be used instead — `d.id` is a real field, not a
  constant `DrawCallId(0)`.
- **REND-607.** The side-table `draw_call_kinds: Box<[DrawCallKind]>` field
  proposed by the fine draft §6.5.6 (R6.2) MUST NOT be added. `DrawCallKind`
  lives on `DrawCall.kind` directly.

### `DrawCallKind` enum

- **REND-608.** The `DrawCallKind` enum MUST have exactly four variants:
  `Clear { color: [f32; 4] }`, `DrawRect { bounds: DirtyRect, color: [f32; 4]
  }`, `DrawText { glyph_run_id: GlyphRunId, color: [f32; 4], rotation: f32,
  canvas_size: [f32; 2] }`, `DrawCustom { shader_hash: u64, vertices:
  Vec<u8>, uniforms: Vec<u8>, topology: Topology, vertex_count: u32 }`. The
  `DrawCustom` variant is defined but unreachable in this wave (REND-716).
- **REND-609.** A `// SAFETY` comment MUST be placed immediately above the
  `DrawCustom` variant noting: *"Consumption of `vertices` and `uniforms` in
  `alkalive-backend-wgpu` requires `unsafe` byte casting (e.g.
  `bytemuck::cast_slice`). The safe `alkalive-render` crate defines the
  shape; the unsafe backend performs the cast."* (CR-31)
- **REND-610.** `GlyphRunId(pub u32)` MUST derive `Debug, Clone, Copy, PartialEq,
  Eq, Hash, Default, Serialize, Deserialize`.
- **REND-611.** `Topology` MUST be an enum with variants `Triangles`,
  `TriangleStrip`, `Lines`, `LineStrip`, deriving `Debug, Clone, Copy,
  PartialEq, Eq, Serialize, Deserialize`.

### Populated `VertexBinding` / `IndexBinding` / `BindGroup`

- **REND-612.** The existing empty marker struct `VertexBinding;` at
  `crates/alkalive-render/src/lib.rs:217` MUST be replaced with the concrete
  struct defined in §1.2.
- **REND-613.** The existing empty marker struct `IndexBinding;` at
  `crates/alkalive-render/src/lib.rs:221` MUST be replaced with the concrete
  struct defined in §1.2.
- **REND-614.** The existing empty marker struct `BindGroup;` at
  `crates/alkalive-render/src/lib.rs:226` MUST be replaced with the concrete
  struct defined in §1.2.
- **REND-615.** The new identifier types `BufferId(pub u32)`,
  `TextureId(pub u32)`, `SamplerId(pub u32)`, `VertexAttribute`, `VertexFormat`,
  `IndexFormat` MUST be added to `crates/alkalive-render/src/lib.rs` with the
  exact shapes in §1.2.

### Lowering function

- **REND-616.** A public function `schedule_to_render_graph` with the exact
  signature in §1.3 MUST be added to `crates/alkalive-render/src/lib.rs`.
  The function lives in `alkalive-render` (not `alkalive-compiler`) because
  its output is `alkalive_render::RenderGraph` and the lowering logic is
  GPU-layer.
- **REND-617.** `schedule_to_render_graph` MUST produce exactly one
  `Attachment` (the canvas swapchain texture), exactly one `RenderPass` per
  entry in `scheduled.schedule.pass_order`, and exactly one `DrawCall` per
  pass. The pass IDs MUST be `PassId(0), PassId(1), …, PassId(N-1)` where `N
  = pass_order.len()`. Draw-call IDs MUST be `DrawCallId(0), DrawCallId(1),
  …, DrawCallId(N-1)`.
- **REND-618.** `schedule_to_render_graph` MUST lower each
  `alkalive_compiler::PassKind` variant to the corresponding
  `DrawCallKind` variant per the table in §1.3. The `algorithm: &AlgorithmIR`
  parameter from the fine draft §6.5.5 MUST NOT be present — it was dead
  (CR-30).
- **REND-619.** For `DrawCallKind::DrawRect`, the `bounds` field MUST be
  `DirtyRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }` (a placeholder). The
  renderer's `execute_draw_call` overrides this from its cached
  `input_field_bounds` field. This is a known wart (CR-27); a `#[test]`
  named `draw_rect_lowering_emits_placeholder_bounds` (§1.9) MUST assert the
  placeholder so the wart is visible until ADR-004 (layout) lands.

### Compilation (CR-6, CR-13, CR-26 resolution)

- **REND-620.** The `compile()` function signature MUST remain `compile(
  graphs: &[RenderGraph], dirty: &[DirtyRect], depth: &DepthBuffer) ->
  Result<CompiledGraph, CompileError>`. The third argument is `&DepthBuffer`
  (CR-21); the call site passes `&DepthBuffer::default()` (a placeholder
  until the occlusion-cull pass lands per W5-T3).
- **REND-621.** The `let _ = (dirty, depth);` line at
  `crates/alkalive-render/src/lib.rs:454` MUST be removed. The `dirty`
  parameter MUST be consumed as follows: if `dirty.is_empty()`, the
  resulting `CompiledGraph.dirty_passes` is `Vec::new()` (semantics: "all
  passes are dirty"). If `dirty` is non-empty, `CompiledGraph.dirty_passes`
  is the set of `PassId`s whose `color_attachments` intersect any
  `DirtyRect` in `dirty` (computed via AABB intersection of the pass's
  attachment's `ExtentOrRelative.absolute` with each `DirtyRect`). (CR-6,
  CR-26)
- **REND-622.** `CompiledGraph` MUST gain a new field `dirty_passes:
  Vec<PassId>` (in addition to the existing `sorted_passes`, `pass_count`,
  `draw_call_count` fields). The field MUST derive `Default` (empty vec).
- **REND-623.** The convenience method `render_frame(&mut self, graph:
  &RenderGraph, time: f32)` from the fine draft §6.5.6 (line 737-752) MUST
  NOT be added — it would re-call `compile()` per frame (CR-13). The
  renderer MUST expose only `render_compiled(&mut self, graph:
  &RenderGraph, compiled: &CompiledGraph, time: f32)`. The runtime caches
  the `CompiledGraph` across frames (§1.4).
- **REND-624.** When the runtime's scene structure changes (algorithm-node
  count differs from the previous frame), the runtime MUST call
  `schedule_to_render_graph` and `compile` again, replacing the cached
  `RenderGraph` and `CompiledGraph`. When only signal values change (text
  input, time), the runtime MUST mutate the cached `RenderGraph` in place
  (updating the `kind` fields of existing `DrawCall`s) without re-running
  `schedule_to_render_graph` or `compile`.

### Renderer signature

- **REND-625.** `WgpuRenderer::render_frame(&mut self, &TextSceneData,
  &ScheduleIR, f32)` and `render_frame_with_dirty(&mut self, &TextSceneData,
  &ScheduleIR, f32, &[usize])` MUST be removed from
  `crates/alkalive-backend-wgpu/src/lib.rs` (lines 826-887). The replacement
  is `render_compiled(&mut self, graph: &RenderGraph, compiled:
  &CompiledGraph, time: f32)`.
- **REND-626.** `WgpuRenderer::render_compiled` MUST iterate
  `compiled.sorted_passes`, look up each pass by `PassId` in
  `graph.passes`, then iterate `pass.draw_calls`, look up each `DrawCall`
  by `dc.id` in `graph.draw_calls`, and dispatch via
  `execute_draw_call(graph, dc, time)`. The lookup MUST use
  `graph.draw_calls.iter().find(|d| d.id == dc_id)` (or a pre-built
  `HashMap<DrawCallId, usize>` for O(1); the linear find is acceptable for
  the Hello World scene's 5 draw calls).
- **REND-627.** `WgpuRenderer::execute_draw_call` MUST match on
  `dc.kind` (the `DrawCallKind` field on `DrawCall` directly — not a side
  table). The match arms are: `DrawCallKind::Clear { color }` →
  `gl.clear_color(color[0], color[1], color[2], color[3]); gl.clear(...)`;
  `DrawCallKind::DrawRect { bounds, color }` →
  `self.draw_rect_filled(bounds.x, bounds.y, bounds.w, bounds.h, color[0],
  color[1], color[2], color[3])` (with `bounds` overridden by
  `self.real_rect_bounds(bounds)`); `DrawCallKind::DrawText {
  glyph_run_id, color, rotation, .. }` → bind text pipeline, set uniforms,
  `gl.draw_arrays(...)`; `DrawCallKind::DrawCustom { .. }` →
  `web_sys::console::warn_1(&"AlkALive: custom pipeline not yet
  implemented".into())` and skip.
- **REND-628.** The `Runtime` struct in
  `crates/alkalive-runtime-wasm/src/lib.rs` MUST gain two new fields:
  `graph: alkalive_render::RenderGraph` and `compiled:
  alkalive_render::CompiledGraph`. The existing `schedule:
  alkalive_compiler::ScheduleIR` and `dep_graph:
  alkalive_compiler::DependencyGraph` fields are kept (they still drive
  signal propagation). The `frame_closure` MUST call
  `runtime.renderer.render_compiled(&runtime.graph, &runtime.compiled,
  runtime.time)` instead of `render_frame` or `render_frame_with_dirty`.
- **REND-629.** When `runtime.is_small_scene` is `true` (the Hello World
  case, per ADR-025 R1 mitigation), the runtime MUST pass
  `&runtime.compiled` directly (whose `dirty_passes` is empty — semantics:
  "all passes are dirty"). When `is_small_scene` is `false`, the runtime
  MUST consult `runtime.signals.propagate(...)` and pass the resulting
  `dirty_passes` to `compile()` so `CompiledGraph.dirty_passes` is
  populated; the renderer then consults `compiled.dirty_passes` (currently
  still runs all passes — see REND-624, future per-pass render targets).

---

## 1.2 Data structures

All types live in `crates/alkalive-render/src/lib.rs` unless noted. The
`#![forbid(unsafe_code)]` attribute at line 20 is preserved.

### Identifiers and small helpers (existing; serde derives added per REND-601)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct PassId { pub value: u64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct AttachmentId { pub value: u64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct DrawCallId { pub value: u64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct BufferId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct TextureId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct SamplerId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct GlyphRunId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirtyRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
// Manual serde impls are unnecessary — derive handles f32 fields.
// (Add `#[derive(serde::Serialize, serde::Deserialize)]` here too.)

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtentOrRelative {
    pub absolute: Option<(u32, u32)>,
    pub relative: Option<(f32, f32)>,
}
```

### Enum types (serde derives added)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PassType { Render, Compute, CopyTransfer, OcclusionCull }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AttachmentFormat {
    Bgra8Unorm, Rgba8UnormSrgb, Rgba16Float, Depth24Plus, Depth32Float,
    Stencil8, Bc1, Bc2, Bc3, Bc4, Bc5, Bc6, Bc7, Astc4x4, R32Uint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClearOp { Clear, Load, DontCare }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SampleCount { X1, X2, X4, X8 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Topology { Triangles, TriangleStrip, Lines, LineStrip }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VertexFormat { Float32x2, Float32x4, Uint8x4Norm }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndexFormat { Uint16, Uint32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PipelineHandle { pub value: u64 }
```

### `Attachment` (existing shape; serde derives added)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Attachment {
    pub id: AttachmentId,
    pub format: AttachmentFormat,
    pub size: ExtentOrRelative,
    pub samples: SampleCount,
    pub lifetime: (PassId, PassId),
    pub clear_op: ClearOp,
}
```

### `RenderPass` (existing shape; serde derives added)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderPass {
    pub id: PassId,
    pub kind: PassType,
    pub color_attachments: Box<[AttachmentId]>,
    pub depth_stencil: Option<AttachmentId>,
    pub draw_calls: Box<[DrawCallId]>,
    pub dependencies: Box<[PassId]>,
}
```

### `VertexBinding` (replaces empty marker — REND-612)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VertexBinding {
    pub buffer_id: BufferId,
    pub offset: u64,
    pub stride: u32,
    pub attributes: Box<[VertexAttribute]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VertexAttribute {
    pub location: u32,
    pub format: VertexFormat,
    pub byte_offset: u32,
}
```

### `IndexBinding` (replaces empty marker — REND-613)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexBinding {
    pub buffer_id: BufferId,
    pub offset: u64,
    pub index_format: IndexFormat,
}
```

### `BindGroup` (replaces empty marker — REND-614)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BindGroup {
    pub layout_hash: u64,
    pub uniform_buffer: Option<BufferId>,
    pub textures: Box<[TextureId]>,
    pub samplers: Box<[SamplerId]>,
}
```

### `DrawCallKind` (NEW — REND-608)

```rust
/// High-level, author-facing draw-call descriptor produced by
/// `schedule_to_render_graph`. Lowered to a low-level `DrawCall` (with a
/// resolved `PipelineHandle` and bound resources) by the same function.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DrawCallKind {
    /// Clear the entire attachment to a solid color. `color` is RGBA,
    /// normalized 0.0–1.0.
    Clear { color: [f32; 4] },

    /// Draw a solid-color filled rectangle with proper alpha blending.
    /// `bounds` is pixel-space (x, y, w, h). Y-down, origin at top-left.
    DrawRect {
        bounds: DirtyRect,
        color: [f32; 4],
    },

    /// Draw a shaped text run via the glyph atlas.
    DrawText {
        glyph_run_id: GlyphRunId,
        color: [f32; 4],
        rotation: f32,
        canvas_size: [f32; 2],
    },

    /// Draw an author-supplied custom shader (ADR-006 future).
    //
    // SAFETY: Consumption of `vertices` and `uniforms` in
    // `alkalive-backend-wgpu` requires `unsafe` byte casting (e.g.
    // `bytemuck::cast_slice`). The safe `alkalive-render` crate defines the
    // shape; the unsafe backend performs the cast.
    DrawCustom {
        shader_hash: u64,
        vertices: Vec<u8>,
        uniforms: Vec<u8>,
        topology: Topology,
        vertex_count: u32,
    },
}
```

### `DrawCall` (CR-4 — `id` and `kind` fields added)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrawCall {
    /// This draw call's identifier. Stored on the struct (not in a side
    /// table) so the renderer's `graph.draw_calls.iter().find(|d| d.id ==
    /// dc_id)` lookup is correct.
    pub id: DrawCallId,

    /// High-level kind. The renderer matches on this to dispatch.
    pub kind: DrawCallKind,

    /// Cached pipeline handle.
    pub pipeline: PipelineHandle,

    /// Vertex input binding.
    pub vertices: VertexBinding,

    /// Optional index binding.
    pub indices: Option<IndexBinding>,

    /// Bound resource groups.
    pub bindings: Box<[BindGroup]>,

    /// GPU-resident instance range.
    pub instances: Range<u32>,

    /// Optional scissor (ADR 002 scope tag).
    pub scissor: Option<DirtyRect>,
}
```

### `OcclusionCullPass` (existing; serde added)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OcclusionCullPass;
```

### `RenderGraph` (existing shape; serde derives added — CR-1)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderGraph {
    pub passes: Box<[RenderPass]>,
    pub attachments: Box<[Attachment]>,
    pub draw_calls: Box<[DrawCall]>,
    pub occlusion_cull: OcclusionCullPass,
    pub edges: Box<[(PassId, PassId)]>,
    pub source_module: ModuleId,
}
```

### `CompiledGraph` (CR-6, CR-26 — `dirty_passes` field added)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CompiledGraph {
    /// Merged and topologically-sorted pass IDs.
    pub sorted_passes: Vec<PassId>,
    /// Total number of merged passes.
    pub pass_count: usize,
    /// Total number of merged draw calls.
    pub draw_call_count: usize,
    /// Pass IDs whose `color_attachments` intersect any `DirtyRect` in the
    /// `dirty` argument to `compile()`. Empty means "all passes are
    /// dirty" (the renderer runs every pass). Populated means "only these
    /// passes need re-execution" (the renderer may skip others — but see
    /// REND-629: per-pass render targets are future work, so today the
    /// renderer still runs all passes when this is non-empty).
    pub dirty_passes: Vec<PassId>,
}
```

### `PipelineCache` (existing; serde NOT added — backend-owned, not transported)

The `PipelineCache` type at `crates/alkalive-render/src/lib.rs:646` is
**not** part of the cross-thread transport surface — it lives in the
renderer (and after Gap 8, in the worker). Serde derives are therefore
**not** added to it. (Adding serde to it would require `wgpu::RenderPipeline`
to be `Serialize`, which it is not.)

### `PipelineDesc` (existing; serde added for future transport)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineDesc {
    pub shader_hash: u64,
    pub layout_hash: u64,
    pub target_format: AttachmentFormat,
    pub sample_count: SampleCount,
}
```

### `DepthBuffer` (existing; serde added)

```rust
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DepthBuffer;
```

### Pipeline handle constants (REND-617)

```rust
/// Pre-allocated pipeline handles (populated at renderer init time).
pub const PIPELINE_CLEAR: PipelineHandle = PipelineHandle { value: 0 };
pub const PIPELINE_RECT:  PipelineHandle = PipelineHandle { value: 1 };
pub const PIPELINE_TEXT:  PipelineHandle = PipelineHandle { value: 2 };
```

---

## 1.3 Interfaces and contracts

### `schedule_to_render_graph` (REND-616, REND-618)

```rust
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
///   Lives in `alkalive-scene-data` (per §0.3 — the crate cycle is broken
///   structurally, not via a `SceneData` trait).
/// - `canvas_size`: physical pixel dimensions (for attachment sizing).
///
/// # Output
///
/// A `RenderGraph` with:
/// - 1 color attachment (the canvas swapchain texture, format
///   `AttachmentFormat::Bgra8Unorm` — see §4 of the fine draft).
/// - N passes, one per entry in `scheduled.schedule.pass_order`.
/// - N draw calls, one per pass. Each `DrawCall` carries its `id` and
///   `kind` directly (no side table — REND-605, REND-607).
/// - Barrier edges from each pass to its successor (linear chain today;
///   future: dependency-driven DAG per `pass.dependencies`).
pub fn schedule_to_render_graph(
    scheduled: &alkalive_compiler::ScheduledScene,
    scene: &alkalive_scene_data::TextSceneData,
    canvas_size: (u32, u32),
) -> RenderGraph {
    use alkalive_compiler::PassKind;

    let mut passes: Vec<RenderPass> = Vec::new();
    let mut draw_calls: Vec<DrawCall> = Vec::new();
    let mut edges: Vec<(PassId, PassId)> = Vec::new();

    // 1. One color attachment: the canvas swapchain texture.
    let canvas_attachment_id = AttachmentId { value: 0 };
    let attachments = vec![Attachment {
        id: canvas_attachment_id,
        format: AttachmentFormat::Bgra8Unorm,
        size: ExtentOrRelative {
            absolute: Some(canvas_size),
            relative: None,
        },
        samples: SampleCount::X1,
        lifetime: (PassId { value: 0 }, PassId { value: 0 }), // updated below
        clear_op: ClearOp::Clear,
    }];

    // 2. One pass per schedule entry.
    let mut prev_pass_id: Option<PassId> = None;
    for (i, &pass_idx) in scheduled.schedule.pass_order.iter().enumerate() {
        let schedule_pass = match scheduled.schedule.passes.get(pass_idx) {
            Some(p) => p,
            None => continue,
        };

        let pass_id = PassId { value: i as u64 };
        let draw_call_id = DrawCallId { value: i as u64 };

        // Lower PassKind → DrawCallKind. (CR-30: the `algorithm` parameter
        // from the fine draft is removed — it was dead.)
        let kind = lower_pass_kind(schedule_pass.kind, scene);
        let pipeline = pipeline_for_kind(&kind);
        let call = DrawCall {
            id: draw_call_id,
            kind,
            pipeline,
            vertices: VertexBinding::default(),
            indices: None,
            bindings: Box::new([BindGroup::default()]),
            instances: 0..1,
            scissor: None,
        };
        draw_calls.push(call);

        passes.push(RenderPass {
            id: pass_id,
            kind: PassType::Render,
            color_attachments: Box::new([canvas_attachment_id]),
            depth_stencil: None,
            draw_calls: Box::new([draw_call_id]),
            dependencies: prev_pass_id.iter().copied().collect::<Vec<_>>().into_boxed_slice(),
        });

        if let Some(prev) = prev_pass_id {
            edges.push((prev, pass_id));
        }
        prev_pass_id = Some(pass_id);
    }

    // 3. Update the attachment's lifetime to span all passes.
    let last_pass = PassId { value: passes.len().saturating_sub(1) as u64 };
    let mut attachments = attachments;
    attachments[0].lifetime = (PassId { value: 0 }, last_pass);

    RenderGraph {
        passes: passes.into_boxed_slice(),
        attachments: attachments.into_boxed_slice(),
        draw_calls: draw_calls.into_boxed_slice(),
        occlusion_cull: OcclusionCullPass,
        edges: edges.into_boxed_slice(),
        source_module: ModuleId::default(),
    }
}

/// Lower one `PassKind` to one `DrawCallKind` (REND-618).
fn lower_pass_kind(
    kind: alkalive_compiler::PassKind,
    scene: &alkalive_scene_data::TextSceneData,
) -> DrawCallKind {
    use alkalive_compiler::PassKind;
    match kind {
        PassKind::Clear => {
            let (r, g, b) = scene.background_normalized();
            DrawCallKind::Clear { color: [r, g, b, 1.0] }
        }
        PassKind::InputFieldBackground => {
            // CR-27: placeholder bounds (0,0,0,0); the renderer overrides
            // from its cached `input_field_bounds` field. ADR-004 (layout)
            // will move this out of the renderer.
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
fn pipeline_for_kind(kind: &DrawCallKind) -> PipelineHandle {
    match kind {
        DrawCallKind::Clear { .. } => PIPELINE_CLEAR,
        DrawCallKind::DrawRect { .. } => PIPELINE_RECT,
        DrawCallKind::DrawText { .. } => PIPELINE_TEXT,
        DrawCallKind::DrawCustom { shader_hash, .. } => {
            // Today this branch is unreachable (no custom shaders yet).
            PipelineHandle { value: *shader_hash }
        }
    }
}
```

### `compile` (REND-620, REND-621, REND-622)

```rust
/// Compile a slice of submitted graphs into a single `CompiledGraph`.
///
/// The third argument is `&DepthBuffer` (CR-21); the call site passes
/// `&DepthBuffer::default()` (a placeholder until the occlusion-cull pass
/// lands per W5-T3).
///
/// `dirty` is now consumed (CR-6, CR-26): if non-empty, the resulting
/// `CompiledGraph.dirty_passes` lists passes whose `color_attachments`
/// intersect any `DirtyRect` in `dirty`. If empty, `dirty_passes` is empty
/// (semantics: "all passes are dirty").
pub fn compile(
    graphs: &[RenderGraph],
    dirty: &[DirtyRect],
    depth: &DepthBuffer,
) -> Result<CompiledGraph, CompileError> {
    // (Existing merge + topo-sort logic from crates/alkalive-render/src/lib.rs:456-550
    //  is preserved verbatim — only the final `Ok(CompiledGraph { ... })`
    //  construction changes to include `dirty_passes`.)

    // ... merge, validate attachment lifetimes, topological sort (existing) ...

    // Compute dirty_passes (REND-621). When `dirty` is empty, the field is
    // empty (semantics: all passes dirty). When non-empty, intersect each
    // pass's color-attachment extents with each DirtyRect.
    let dirty_passes: Vec<PassId> = if dirty.is_empty() {
        Vec::new()
    } else {
        let mut result = Vec::new();
        for g in graphs {
            for pass in g.passes.iter() {
                let intersects = pass.color_attachments.iter().any(|aid| {
                    g.attachments.iter().any(|att| {
                        att.id == *aid
                            && att.size.absolute.map_or(false, |(aw, ah)| {
                                dirty.iter().any(|dr| {
                                    // AABB intersection: rects overlap if
                                    // they overlap on both axes.
                                    let ax0 = 0.0;
                                    let ay0 = 0.0;
                                    let ax1 = aw as f32;
                                    let ay1 = ah as f32;
                                    !(dr.x + dr.w <= ax0
                                        || dr.x >= ax1
                                        || dr.y + dr.h <= ay0
                                        || dr.y >= ay1)
                                })
                            })
                    })
                });
                if intersects {
                    result.push(pass.id);
                }
            }
        }
        result
    };

    // `depth` is still consumed by the future occlusion-cull pass; we keep
    // the parameter in the signature so the call site does not change when
    // W5-T3 lands. Today it is a no-op.
    let _ = depth;

    Ok(CompiledGraph {
        sorted_passes,
        pass_count: merged_passes.len(),
        draw_call_count: merged_draw_calls.len(),
        dirty_passes,
    })
}
```

### `WgpuRenderer::render_compiled` (REND-625, REND-626, REND-627)

```rust
impl WgpuRenderer {
    /// Render one frame from a pre-compiled `RenderGraph` (REND-623 — the
    /// convenience `render_frame` method is removed; callers must pass a
    /// pre-compiled `&CompiledGraph`).
    pub fn render_compiled(
        &mut self,
        graph: &alkalive_render::RenderGraph,
        compiled: &alkalive_render::CompiledGraph,
        time: f32,
    ) {
        // 1. Atlas upload (existing logic, factored out).
        if let Err(e) = self.ensure_atlas_uploaded() {
            web_sys::console::error_1(&format!("atlas upload failed: {}", e).into());
            return;
        }

        // 2. Iterate sorted passes. (CR-6: the renderer consults
        //    `compiled.dirty_passes` for future per-pass skipping; today
        //    it still runs every pass when the set is non-empty, because
        //    WebGL2's single-buffered clear leaves ghosts of stale
        //    passes — see REND-629.)
        for &pass_id in &compiled.sorted_passes {
            let pass = match graph.passes.iter().find(|p| p.id == pass_id) {
                Some(p) => p,
                None => continue,
            };
            for &dc_id in &pass.draw_calls {
                let dc = match graph.draw_calls.iter().find(|d| d.id == dc_id) {
                    Some(d) => d,
                    None => continue,
                };
                self.execute_draw_call(dc, time);
            }
        }
    }

    /// Execute one draw call by matching on `dc.kind` (REND-627 — the
    /// side-table lookup is gone; `kind` is a field on `DrawCall`).
    fn execute_draw_call(&mut self, dc: &alkalive_render::DrawCall, time: f32) {
        use alkalive_render::DrawCallKind;
        match &dc.kind {
            DrawCallKind::Clear { color } => {
                self.gl.clear_color(color[0], color[1], color[2], color[3]);
                self.gl.clear(WebGl2RenderingContext::COLOR_BUFFER_BIT);
            }
            DrawCallKind::DrawRect { bounds, color } => {
                let bounds = self.real_rect_bounds(*bounds);
                self.draw_rect_filled(bounds.x, bounds.y, bounds.w, bounds.h,
                                      color[0], color[1], color[2], color[3]);
            }
            DrawCallKind::DrawText { glyph_run_id, color, rotation, .. } => {
                let rotation = rotation * time;
                let (start, count) = self.glyph_run_range(*glyph_run_id);
                self.gl.use_program(Some(&self.program));
                self.gl.bind_vertex_array(Some(&self.vao));
                self.gl.uniform1f(Some(&self.u_rotation), rotation);
                self.gl.uniform4f(Some(&self.u_text_color),
                                  color[0], color[1], color[2], color[3]);
                self.gl.draw_arrays(WebGl2RenderingContext::TRIANGLES,
                                    start as i32, count as i32);
            }
            DrawCallKind::DrawCustom { .. } => {
                web_sys::console::warn_1(
                    &"AlkALive: custom pipeline not yet implemented".into()
                );
            }
        }
    }
}
```

### `Runtime` (REND-628)

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs

struct Runtime {
    renderer: alkalive_backend_wgpu::WgpuRenderer,
    scene: alkalive_scene_data::TextSceneData,           // was alkalive_backend_wgpu::TextSceneData
    schedule: alkalive_compiler::ScheduleIR,
    dep_graph: alkalive_compiler::DependencyGraph,
    signals: signal_store::SignalStore,
    is_small_scene: bool,
    time: f32,
    input_text: String,
    original_text: String,

    // NEW (Gap 6, REND-628):
    graph: alkalive_render::RenderGraph,
    compiled: alkalive_render::CompiledGraph,
    // Cached node count from the previous frame; if it changes, re-lower
    // and re-compile (REND-624).
    cached_node_count: usize,
}
```

### `init_runtime` (REND-628 continued)

```rust
async fn init_runtime(/* ... */) -> Result<(), JsValue> {
    // ... existing scene/schedule/dep_graph/signals setup ...

    // NEW (Gap 6): lower the schedule once at startup.
    let graph = alkalive_render::schedule_to_render_graph(
        &scheduled,
        &scene,
        (width, height),
    );
    let compiled = alkalive_render::compile(
        std::slice::from_ref(&graph),
        &[],
        &alkalive_render::DepthBuffer::default(),
    ).map_err(|e| JsValue::from_str(&format!("graph compile: {:?}", e)))?;

    let runtime = Runtime {
        renderer,
        scene,
        schedule,
        dep_graph,
        signals,
        is_small_scene,
        time: 0.0,
        input_text: String::new(),
        original_text: scheduled.algorithm.nodes
            .iter()
            .find_map(|n| match &n.kind {
                alkalive_compiler::NodeKind::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .unwrap_or_default(),
        graph,
        compiled,
        cached_node_count: scheduled.algorithm.nodes.len(),
    };
    // ... store runtime in thread_local ...
    Ok(())
}
```

### Frame loop (REND-628, REND-629)

```rust
fn start_frame_loop() {
    let frame_closure = Closure::new(|| {
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                runtime.time = elapsed_seconds();
                runtime.signals.set(
                    alkalive_compiler::SignalId(1), // TIME
                    signal_store::SignalValue::Float(runtime.time),
                );

                // REND-624: re-lower + re-compile only when structure changes.
                let node_count = runtime.schedule.passes.len(); // proxy for structure
                if node_count != runtime.cached_node_count {
                    let graph = alkalive_render::schedule_to_render_graph(
                        // re-borrow the scheduled scene; in practice the runtime
                        // re-runs compile_with_deps here for HMR. Today the
                        // scene is fixed at startup, so this branch is taken
                        // only on the first frame.
                        &scheduled_for_runtime(),
                        &runtime.scene,
                        (runtime.renderer.width(), runtime.renderer.height()),
                    );
                    let compiled = alkalive_render::compile(
                        std::slice::from_ref(&graph),
                        &[],
                        &alkalive_render::DepthBuffer::default(),
                    ).unwrap_or_default();
                    runtime.graph = graph;
                    runtime.compiled = compiled;
                    runtime.cached_node_count = node_count;
                } else {
                    // Structure unchanged: update DrawCallKind fields in place.
                    runtime.update_graph_for_frame();
                }

                // REND-629: render_compiled takes the cached graph + compiled.
                runtime.renderer.render_compiled(
                    &runtime.graph,
                    &runtime.compiled,
                    runtime.time,
                );
            }
        });
        schedule_next_frame();
    });
    // ... store closure, kick off first frame ...
}

impl Runtime {
    /// Mutate the cached `RenderGraph` in place to reflect per-frame
    /// signal changes (text input, time). Does NOT re-run
    /// `schedule_to_render_graph` or `compile` (REND-624).
    fn update_graph_for_frame(&mut self) {
        for dc in self.graph.draw_calls.iter_mut() {
            match &mut dc.kind {
                alkalive_render::DrawCallKind::DrawText {
                    glyph_run_id, color, rotation, canvas_size,
                } => {
                    // Refresh canvas_size (may have changed on resize).
                    canvas_size[0] = self.renderer.width() as f32;
                    canvas_size[1] = self.renderer.height() as f32;
                    // For input text, refresh color based on emptiness.
                    if glyph_run_id.0 == 1 {
                        let is_placeholder = self.input_text.is_empty();
                        *color = if is_placeholder {
                            [0.35, 0.35, 0.4, 1.0]
                        } else {
                            [0.9, 0.9, 0.95, 1.0]
                        };
                    }
                    // `rotation` is `rotation_speed`; the renderer
                    // multiplies by `time` at draw time. No update here.
                    let _ = rotation;
                }
                _ => {}
            }
        }
    }
}
```

---

## 1.4 State transitions

### Render-graph lifecycle

```
                  compile_with_deps(HELLO_ALK_SRC)
                              │
                              ▼
                  ScheduledScene { algorithm, schedule }
                              │
                              │  schedule_to_render_graph(scheduled, scene, canvas_size)
                              ▼
                     RenderGraph (lowered)
                              │
                              │  compile(&[graph], &[], &DepthBuffer::default())
                              ▼
                     CompiledGraph (cached)
                              │
                              │  renderer.render_compiled(&graph, &compiled, time)
                              ▼
                          GPU frame
```

State transitions on `Runtime`:

| From | To | Trigger |
|------|-----|---------|
| `Init` | `Loaded` | `init_runtime` completes; `graph` + `compiled` populated |
| `Loaded` | `Rendering` | First `requestAnimationFrame` callback fires |
| `Rendering` | `Rendering` (structure unchanged) | `update_graph_for_frame` mutates `graph.draw_calls[i].kind` in place; `compiled` is reused |
| `Rendering` | `Rendering` (structure changed) | `schedule_to_render_graph` + `compile` re-run; `graph` + `compiled` replaced |
| `Rendering` | `Resizing` | `resize` event fires; renderer's `width`/`height` updated; next frame's `update_graph_for_frame` rewrites `canvas_size` |
| `Rendering` | `InputChanged` | IME input event fires; `runtime.input_text` updated; next frame's `update_graph_for_frame` rewrites the input `DrawText` color |
| `Rendering` | `Error` | `compile()` returns `Err`; `web_sys::console::error_1` logs; frame skipped |

### `compile()` state machine

```
┌──────────────┐
│  Entry       │
└──────┬───────┘
       │
       ▼
┌──────────────────────┐    error    ┌──────────────────────────┐
│ Merge phase          │────────────▶│ Err(AttachmentLifetime   │
│ (collect passes,     │             │     Violation)           │
│  attachments,        │             └──────────────────────────┘
│  draw_calls, edges)  │
└──────┬───────────────┘
       │
       ▼
┌──────────────────────┐    error    ┌──────────────────────────┐
│ Validate attachment  │────────────▶│ Err(InvalidEdge)         │
│ lifetimes            │             └──────────────────────────┘
└──────┬───────────────┘
       │
       ▼
┌──────────────────────┐    error    ┌──────────────────────────┐
│ Topological sort     │────────────▶│ Err(BarrierCycle)        │
│ (Kahn's algorithm)   │             └──────────────────────────┘
└──────┬───────────────┘
       │
       ▼
┌──────────────────────┐
│ Compute dirty_passes │
│ (AABB intersection)  │
└──────┬───────────────┘
       │
       ▼
┌──────────────────────┐
│ Ok(CompiledGraph {   │
│   sorted_passes,     │
│   pass_count,        │
│   draw_call_count,   │
│   dirty_passes,      │
│ })                   │
└──────────────────────┘
```

---

## 1.5 Error cases

Every error class has a stable message format. Errors are logged via
`web_sys::console::error_1` (or `warn_1` for non-fatal warnings) and the
frame is skipped — no panic paths are added.

| ID | Error class | Source | Message format | Handling |
|----|-------------|--------|----------------|----------|
| REND-6-E1 | `CompileError::InvalidEdge` | `compile()` finds an edge `(from, to)` where `from` or `to` is not in `merged_passes` | `"AlkALive render-graph compile failed: InvalidEdge"` | Log via `console::error_1`; skip the frame (no draw). |
| REND-6-E2 | `CompileError::AttachmentLifetimeViolation` | `compile()` finds an attachment whose `lifetime.(producer, last_consumer)` references a missing `PassId` | `"AlkALive render-graph compile failed: AttachmentLifetimeViolation"` | Same. |
| REND-6-E3 | `CompileError::BarrierCycle` | Kahn's algorithm cannot drain the graph (`visited != merged_passes.len()`) | `"AlkALive render-graph compile failed: BarrierCycle"` | Same. (CR-22: the variant name is `BarrierCycle`, NOT `CycleDetected`.) |
| REND-6-E4 | Draw-call lookup failure | `graph.draw_calls.iter().find(\|d\| d.id == dc_id)` returns `None` | `"AlkALive: draw call {:?} not found in graph"` (with the `DrawCallId`) | Log via `console::warn_1`; skip the draw call. |
| REND-6-E5 | Atlas upload failure | `ensure_atlas_uploaded` returns `Err(String)` | `"AlkALive: atlas upload failed: <e>"` | Log via `console::error_1`; skip the frame. |
| REND-6-E6 | Pipeline lookup failure | `dc.pipeline` is not one of `PIPELINE_CLEAR`/`PIPELINE_RECT`/`PIPELINE_TEXT` (and `dc.kind` is not `DrawCustom`) | `"AlkALive: unknown pipeline handle {:?}"` | Log via `console::warn_1`; skip the draw call. |
| REND-6-E7 | Custom pipeline not yet implemented | `dc.kind` is `DrawCallKind::DrawCustom { .. }` | `"AlkALive: custom pipeline not yet implemented"` | Log via `console::warn_1`; skip the draw call. |

No `panic!` paths are added. The existing `panic::set_hook` (runtime line 208)
catches any unexpected panic and logs it.

---

## 1.6 Validation rules

Runtime validation rules (enforced by `compile()` and by the renderer):

1. **V-6-1.** Every `PassId` referenced in `RenderGraph.edges.(from, to)` MUST
   exist in `RenderGraph.passes`. Violation → `CompileError::InvalidEdge`
   (REND-6-E1).
2. **V-6-2.** Every `AttachmentId` referenced in `RenderPass.color_attachments`
   or `RenderPass.depth_stencil` MUST exist in `RenderGraph.attachments`.
   Violation → `CompileError::AttachmentLifetimeViolation` (REND-6-E2).
3. **V-6-3.** Every `Attachment.lifetime.(producer, last_consumer)` MUST
   reference a `PassId` that exists in `RenderGraph.passes`. Violation →
   `CompileError::AttachmentLifetimeViolation` (REND-6-E2).
4. **V-6-4.** Every `DrawCallId` referenced in `RenderPass.draw_calls` MUST
   exist in `RenderGraph.draw_calls`. Violation → REND-6-E4 (warn + skip;
   not a compile error because the graph is still executable for the other
   passes).
5. **V-6-5.** The `RenderGraph.edges` set MUST NOT contain a self-loop
   (`from == to`). Violation → `CompileError::BarrierCycle` (REND-6-E3;
   Kahn's algorithm detects this naturally).
6. **V-6-6.** The `RenderGraph.passes` slice MUST NOT contain two passes
   with the same `PassId`. Violation → `CompileError::InvalidEdge` (the
   second pass shadows the first in `index_of`, so an edge referencing the
   first becomes `InvalidEdge`). This is a soft check — a future hard check
   may add a dedicated `DuplicatePassId` variant.
7. **V-6-7.** The `DrawCall.id` field MUST be unique within
   `RenderGraph.draw_calls`. Violation → REND-6-E4 (the lookup returns the
   first match; the duplicate is silently shadowed). A future hard check
   may add `DuplicateDrawCallId`.
8. **V-6-8.** `DrawCallKind::Clear { color }` components MUST be in `[0.0,
   1.0]`. Violation → clamped to `[0.0, 1.0]` by the renderer's
   `gl.clear_color` call (no error; the browser clamps).
9. **V-6-9.** `DrawCallKind::DrawRect { bounds }` with `bounds == (0, 0, 0,
   0)` (the placeholder per REND-619) MUST be overridden by the renderer's
   `real_rect_bounds(bounds)` call (which returns
   `self.input_field_bounds` formatted as a `DirtyRect`). This is the CR-27
   wart; the `draw_rect_lowering_emits_placeholder_bounds` test makes it
   visible.
10. **V-6-10.** `CompiledGraph.dirty_passes` MUST be a subset of
    `CompiledGraph.sorted_passes`. Violation → the renderer skips unknown
    `PassId`s silently (the `find` returns `None`).

---

## 1.7 Performance requirements

| ID | Requirement | Measurement |
|----|-------------|-------------|
| REND-6-P1 | `schedule_to_render_graph` for the Hello World scene (5 passes) MUST complete in **< 50 µs** on a 2020 mid-range laptop (M1 Air baseline). | `criterion` benchmark `bench_schedule_to_render_graph_hello_world`. |
| REND-6-P2 | `compile()` for the Hello World scene MUST complete in **< 100 µs** on the same hardware. | `criterion` benchmark `bench_compile_hello_world`. |
| REND-6-P3 | `compile()` for a synthetic 1000-pass scene (linear chain) MUST complete in **< 5 ms**. | `criterion` benchmark `bench_compile_1000_passes`. (CR-13: the per-frame `compile()` is removed; this benchmark guards the startup + scene-change cost.) |
| REND-6-P4 | `Runtime::update_graph_for_frame` (the in-place mutation path) for the Hello World scene MUST complete in **< 10 µs**. | `criterion` benchmark `bench_update_graph_for_frame`. |
| REND-6-P5 | The runtime MUST NOT call `compile()` more than once per scene-structure change. Per-frame, only `update_graph_for_frame` runs. (CR-13) | Asserted by a `#[test]` that wraps `compile` in a counter and verifies it's called once at startup + zero times for the next 100 frames. |
| REND-6-P6 | `RenderGraph` clone cost for the Hello World scene (5 passes, 5 draw calls, 1 attachment, 4 edges, ~600 bytes) MUST be **< 5 µs**. | `criterion` benchmark `bench_render_graph_clone`. |
| REND-6-P7 | The serde serialise + deserialise round-trip for the Hello World `RenderGraph` MUST complete in **< 50 µs** (relevant for Gap 8's `postMessage`). | `criterion` benchmark `bench_render_graph_serde_round_trip`. |
| REND-6-P8 | `WgpuRenderer::render_compiled` for the Hello World scene MUST complete in **< 2 ms** on the M1 Air baseline (excluding `requestAnimationFrame` jitter). | Manual measurement via `performance.now()` deltas, logged every 60th frame. |

---

## 1.8 Browser/platform integration

### wasm32 + raw WebGL2 (Gap 6 ships first with this path; Gap 7 swaps to wgpu)

- The renderer executes the graph via `WebGl2RenderingContext` calls. The
  `BufferTable` (a `Vec<WebGlBuffer>`) and `TextureTable` (a
  `Vec<WebGlTexture>`) are added to `WgpuRenderer`. `BufferId(pub u32)` and
  `TextureId(pub u32)` are indices into these tables.
- The glyph atlas texture is `TextureId(0)`. `ensure_atlas_uploaded` updates
  its contents in place via `texSubImage2D`.
- The `GlyphRunTable` (`Vec<(u32, u32)>` of `(vertex_start, vertex_count)`
  ranges) is keyed by `GlyphRunId`. Today there are two entries: title at
  `(0, title_vertex_count)` and input at `(title_vertex_count,
  input_vertex_count)`.

### Native (test host)

- The native stub at `crates/alkalive-backend-wgpu/src/lib.rs:1348-1429`
  gains a `render_compiled(&mut self, _: &RenderGraph, _: &CompiledGraph, _:
  f32)` no-op for type-check parity.
- `schedule_to_render_graph` runs on native (it is pure data manipulation)
  and is unit-tested there (REND-6-P1, REND-6-P2, REND-6-P3, REND-6-P4,
  REND-6-P6, REND-6-P7).
- `compile()` runs on native and is unit-tested there.

### Cargo.toml changes

`crates/alkalive-render/Cargo.toml`:

```toml
[dependencies]
alkalive-core = { workspace = true }
alkalive-text = { workspace = true }
alkalive-compiler = { workspace = true }   # NEW (Gap 6) — for ScheduledScene / PassKind
alkalive-scene-data = { workspace = true } # NEW (Gap 6) — for TextSceneData
serde = { version = "1", features = ["derive"] }  # NEW (Gap 6, CR-1)
```

`crates/alkalive-backend-wgpu/Cargo.toml` (Gap 6 portion; Gap 7 adds `wgpu`):

```toml
[dependencies]
alkalive-text = { workspace = true }
alkalive-compiler = { workspace = true }
alkalive-render = { workspace = true }       # NEW (Gap 6)
alkalive-scene-data = { workspace = true }   # NEW (Gap 6)

bytemuck = { version = "1", features = ["derive"] }
wasm-bindgen = { workspace = true }
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "HtmlCanvasElement", "Window", "Document", "Element",
    "Gpu", "GpuCanvasContext", "GpuDevice", "GpuQueue",
    "WebGl2RenderingContext", "WebGlBuffer", "WebGlProgram",
    "WebGlShader", "WebGlTexture", "WebGlUniformLocation",
    "WebGlVertexArrayObject", "console", "Performance",
] }

# (Gap 7 will add: wgpu = { version = "23", features = ["webgpu", "webgl"] })

[features]
default = []
# CR-29: the dead `wgpu-backend = []` feature is REMOVED in the Gap 7 PR.
```

`crates/alkalive-runtime-wasm/Cargo.toml` (Gap 6 portion):

```toml
[dependencies]
alkalive-backend-wgpu = { workspace = true }
alkalive-compiler = { workspace = true }
alkalive-text = { workspace = true }
alkalive-render = { workspace = true }       # NEW (Gap 6) — for RenderGraph / CompiledGraph
alkalive-scene-data = { workspace = true }   # NEW (Gap 6)

wasm-bindgen = { workspace = true }
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    "HtmlCanvasElement", "HtmlInputElement", "Window", "Document",
    "Element", "EventTarget", "KeyboardEvent", "InputEvent",
    "MouseEvent", "console", "Performance",
] }
# (Gap 8 will add: Worker, OffscreenCanvas, MessageEvent,
#  DedicatedWorkerGlobalScope, serde, serde-wasm-bindgen)
```

### New crate `alkalive-scene-data` (CR-3 cycle break)

`crates/alkalive-scene-data/Cargo.toml`:

```toml
[package]
name = "alkalive-scene-data"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "AlkALive per-frame scene data (TextSceneData) — shared between compiler, render, and backend"

[lib]
path = "src/lib.rs"

[dependencies]
# No internal deps — this crate is at the bottom of the graph.
```

`crates/alkalive-scene-data/src/lib.rs` contains the `TextSceneData` struct
moved verbatim from `crates/alkalive-backend-wgpu/src/lib.rs:53-110` (the
struct definition, `Default` impl, `new()` constructor, and
`background_normalized()` helper). The crate is `#![forbid(unsafe_code)]`.

`crates/alkalive-backend-wgpu/src/lib.rs` re-exports it for backward
compatibility:

```rust
pub use alkalive_scene_data::TextSceneData;
```

The workspace `Cargo.toml` adds:

```toml
[workspace.dependencies]
alkalive-scene-data = { path = "crates/alkalive-scene-data" }
```

---

## 1.9 Test cases

Each test is identified by `T-6-<n>` and lives in the crate noted in
brackets.

| ID | Test | Expected behaviour |
|----|------|---------------------|
| T-6-1 [`alkalive-render`] | `schedule_to_render_graph` on the Hello World scene produces 5 passes | `graph.passes.len() == 5`; `graph.passes[0].id == PassId(0)`; `graph.passes[4].id == PassId(4)`. |
| T-6-2 [`alkalive-render`] | `schedule_to_render_graph` produces 1 attachment | `graph.attachments.len() == 1`; `graph.attachments[0].format == AttachmentFormat::Bgra8Unorm`; `graph.attachments[0].lifetime == (PassId(0), PassId(4))`. |
| T-6-3 [`alkalive-render`] | `schedule_to_render_graph` produces 5 draw calls with sequential IDs | `graph.draw_calls.len() == 5`; `graph.draw_calls[i].id == DrawCallId(i as u64)` for `i in 0..5`. |
| T-6-4 [`alkalive-render`] | `lower_pass_kind(Clear, scene)` produces `DrawCallKind::Clear { color }` with the scene's background color normalized | For `scene.background = (0, 0, 0)`, `color == [0.0, 0.0, 0.0, 1.0]`. For `scene.background = (255, 0, 0)`, `color == [1.0, 0.0, 0.0, 1.0]`. |
| T-6-5 [`alkalive-render`] | `draw_rect_lowering_emits_placeholder_bounds` (CR-27) | `lower_pass_kind(InputFieldBackground, scene).bounds == DirtyRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }` AND `lower_pass_kind(InputFieldBorder, scene).bounds == DirtyRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }`. |
| T-6-6 [`alkalive-render`] | `pipeline_for_kind(Clear)` returns `PIPELINE_CLEAR` | `pipeline_for_kind(&DrawCallKind::Clear { color: [0.0; 4] }) == PIPELINE_CLEAR`. |
| T-6-7 [`alkalive-render`] | `pipeline_for_kind(DrawRect)` returns `PIPELINE_RECT` | (similar). |
| T-6-8 [`alkalive-render`] | `pipeline_for_kind(DrawText)` returns `PIPELINE_TEXT` | (similar). |
| T-6-9 [`alkalive-render`] | `compile` on a single Hello World graph returns `sorted_passes == [PassId(0), PassId(1), PassId(2), PassId(3), PassId(4)]` | (linear-chain topo sort preserves order). |
| T-6-10 [`alkalive-render`] | `compile` returns `Err(InvalidEdge)` for a graph with a dangling edge | Construct a graph with `edges = [(PassId(0), PassId(99))]`; assert `compile(...).unwrap_err() == CompileError::InvalidEdge`. |
| T-6-11 [`alkalive-render`] | `compile` returns `Err(BarrierCycle)` for a cyclic graph | Construct a graph with `edges = [(PassId(0), PassId(1)), (PassId(1), PassId(0))]`; assert `unwrap_err() == CompileError::BarrierCycle`. (CR-22: variant name is `BarrierCycle`.) |
| T-6-12 [`alkalive-render`] | `compile` returns `Err(AttachmentLifetimeViolation)` for an attachment whose lifetime references a missing pass | Construct a graph with `attachments = [Attachment { lifetime: (PassId(0), PassId(99)), ... }]`; assert `unwrap_err() == CompileError::AttachmentLifetimeViolation`. |
| T-6-13 [`alkalive-render`] | `compile` populates `dirty_passes` correctly when `dirty` is non-empty | Construct a Hello World graph; pass `dirty = &[DirtyRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }]`; assert `compiled.dirty_passes` contains all 5 `PassId`s (their attachment covers the whole canvas). Pass `dirty = &[DirtyRect { x: 10000.0, y: 10000.0, w: 1.0, h: 1.0 }]`; assert `compiled.dirty_passes.is_empty()` (no intersection). |
| T-6-14 [`alkalive-render`] | `compile` with `dirty == &[]` returns `dirty_passes == Vec::new()` | (semantics: empty = "all passes dirty"). |
| T-6-15 [`alkalive-render`] | `compile` merges two graphs with disjoint PassIds into a topologically-sorted union | Construct `g1` with `PassId(0) → PassId(1)` and `g2` with `PassId(2) → PassId(3)`; pass `&[g1, g2]`; assert `compiled.sorted_passes.len() == 4` and all 4 IDs present. |
| T-6-16 [`alkalive-render`] | `RenderGraph` round-trips through serde | `let json = serde_json::to_string(&graph).unwrap(); let back: RenderGraph = serde_json::from_str(&json).unwrap(); assert_eq!(back.passes.len(), graph.passes.len());` (uses `serde_json` in `[dev-dependencies]`). |
| T-6-17 [`alkalive-render`] | `CompiledGraph` round-trips through serde | (similar). |
| T-6-18 [`alkalive-render`] | `DrawCall` carries its `id` and `kind` fields (CR-4) | Construct a `DrawCall { id: DrawCallId(7), kind: DrawCallKind::Clear { color: [0.0; 4] }, ..Default::default() }` (Note: `DrawCall` does not derive `Default` because `DrawCallKind` is not `Default`; the test constructs it explicitly); assert `dc.id == DrawCallId(7)` and `matches!(dc.kind, DrawCallKind::Clear { .. })`. |
| T-6-19 [`alkalive-backend-wgpu`] (native stub) | `render_compiled` does not panic on the Hello World graph | Construct a `RenderGraph` via `schedule_to_render_graph`; construct a `CompiledGraph` via `compile`; call `stub_renderer.render_compiled(&graph, &compiled, 0.0)`; assert no panic. |
| T-6-20 [`alkalive-runtime-wasm`] (wasm32, headless browser via `wasm-bindgen-test`) | The frame loop calls `render_compiled` (not `render_frame`) on the pre-compiled graph | Instrument the renderer with a `called_render_compiled: std::cell::Cell<bool>`; assert it's `true` after one frame. |
| T-6-21 [`alkalive-render`] (bench) | `bench_schedule_to_render_graph_hello_world` completes in < 50 µs (REND-6-P1) | `criterion` benchmark; the `BenchmarkId` "hello_world" reports mean < 50 µs on the M1 Air baseline. |
| T-6-22 [`alkalive-render`] (bench) | `bench_compile_hello_world` completes in < 100 µs (REND-6-P2) | (similar). |
| T-6-23 [`alkalive-render`] (bench) | `bench_compile_1000_passes` completes in < 5 ms (REND-6-P3) | (similar). |
| T-6-24 [`alkalive-render`] | `compile` is not called per-frame (REND-6-P5) | Wrap `compile` in a `static COMPILE_CALL_COUNT: AtomicU64`; run the runtime for 100 frames; assert `COMPILE_CALL_COUNT.load() == 1` (only the startup call). |
| T-6-25 (browser verification, manual) | The Hello World canvas renders identically pre- and post-Gap-6 | Screenshot the canvas pre-Gap-6 and post-Gap-6; assert pixel diff < 1% (allowing for anti-aliasing differences). |

---

## 1.10 Acceptance criteria

The Gap 6 implementation is accepted when **all** of the following are
observable:

1. `cargo test -p alkalive-render` passes all `T-6-1` through `T-6-18` and
   the serde round-trip tests.
2. `cargo test -p alkalive-backend-wgpu` passes `T-6-19`.
3. `cargo test -p alkalive-runtime-wasm --target wasm32-unknown-unknown`
   passes `T-6-20` in a headless browser.
4. `cargo bench -p alkalive-render` reports `T-6-21` mean < 50 µs,
   `T-6-22` mean < 100 µs, `T-6-23` mean < 5 ms on the M1 Air baseline.
5. The Hello World demo at `deploy/index.html` (served via the canonical
   dev server in §3.8) renders the golden "Hello World!" text and the
   dark input-field rectangle with the gold border, visually identical to
   pre-Gap-6 (T-6-25).
6. `cargo build -p alkalive-runtime-wasm --target wasm32-unknown-unknown`
   succeeds with no warnings about unused `id_for_lookup` or
   `draw_call_kinds` (which must not exist — REND-606, REND-607).
7. `grep -r "render_frame_with_dirty" crates/` returns no matches (REND-625).
8. `grep -r "id_for_lookup" crates/` returns no matches (REND-606).
9. `grep -rn "let _ = (dirty, depth);" crates/alkalive-render/` returns no
   matches (REND-621 — the `dirty` parameter is consumed).

---

## 1.11 Traceability

| Requirement | ADR / source | Fine-draft § | Implementation § | Test ID | CR addressed |
|-------------|--------------|--------------|------------------|---------|--------------|
| REND-601 (serde derives on all IR types) | ADR-003 line 99 ("Scene data lives in a `SharedArrayBuffer`") → implies serializable | §1.5.2, §8.5.2 | §1.2 | T-6-16, T-6-17 | CR-1 |
| REND-602 (serde dep in Cargo.toml) | ADR-003 | §8.5.2 | §1.8 | (build check) | CR-1 |
| REND-605 (DrawCall has id + kind) | ADR-001 line 55 ("draw calls") | §6.5.3, §6.5.6 | §1.2 | T-6-18 | CR-4 |
| REND-606 (no DrawCallLookup trait) | (critical review) | §6.5.6 line 851-857 | §1.2 | (build check via grep) | CR-4 |
| REND-607 (no side table) | (critical review) | §6.5.6 line 861-865 | §1.2 | (build check via grep) | CR-4 |
| REND-608 (DrawCallKind enum) | ADR-001 line 55, ADR-006 line 165 | §6.5.3 | §1.2 | T-6-4 through T-6-8 | — |
| REND-609 (SAFETY comment on DrawCustom) | (critical review) | §6.5.3 | §1.2 | (code review) | CR-31 |
| REND-612..615 (populated bindings) | ADR-005 line 138 (object-owned styling) | §6.5.4 | §1.2 | (build check) | — |
| REND-616 (schedule_to_render_graph) | ADR-001 line 55; tech-spec §3.5 line 337 ("currently unspecified") | §6.5.5 | §1.3 | T-6-1 through T-6-3 | — |
| REND-617 (PassId / DrawCallId allocation) | ADR-001 line 55 | §6.5.5 | §1.3 | T-6-1, T-6-3 | — |
| REND-618 (PassKind → DrawCallKind lowering; no `algorithm` param) | ADR-001 line 55; ADR-024 (schedule) | §6.5.5 | §1.3 | T-6-4, T-6-5 | CR-30 |
| REND-619 (placeholder bounds for DrawRect) | (acknowledged wart) | §6.5.5 line 643-657 | §1.3 | T-6-5 | CR-27 |
| REND-620 (compile signature with &DepthBuffer) | (existing) | §6.5.6 | §1.3 | (build check) | CR-21 |
| REND-621 (dirty parameter consumed) | ADR-025 (incremental computation) | §6.6 point 2 | §1.3 | T-6-13, T-6-14 | CR-6, CR-26 |
| REND-622 (CompiledGraph.dirty_passes field) | ADR-025 | §6.6 point 2 | §1.2, §1.3 | T-6-13 | CR-6, CR-26 |
| REND-623 (no convenience render_frame) | (critical review) | §6.5.6 line 737-752 | §1.3 | (build check via grep) | CR-13 |
| REND-624 (cache compiled across frames) | (critical review) | §6.5.8 line 915-919 | §1.3, §1.4 | T-6-24 | CR-13 |
| REND-625 (remove render_frame_with_dirty) | (critical review) | §6.11 R6.5 | §1.3 | (build check via grep) | CR-6 |
| REND-626 (render_compiled uses find by id) | (critical review) | §6.5.6 line 787-790 | §1.3 | (code review) | CR-4 |
| REND-627 (execute_draw_call matches on dc.kind) | ADR-001 line 55 | §6.5.6 line 793-840 | §1.3 | T-6-20 | CR-5 (the WebGL2 path already reads `color`; the wgpu path is fixed in §2.7) |
| REND-628 (Runtime stores graph + compiled) | ADR-001 line 55 | §6.5.8 | §1.3 | T-6-20 | — |
| REND-629 (small-scene + incremental paths) | ADR-025 | §6.5.8, §6.6 | §1.3 | T-6-20 | CR-6 |
| All REND-6-Pn performance requirements | ADR-017 line 600 (startup budget) | §6.6 point 7 | §1.7 | T-6-21 through T-6-24 | CR-13 |

---

# Gap 7 — WGSL Shaders + wgpu (ADR-006)

## 2.1 Exact requirements

### Dependency and feature changes

- **REND-701.** `crates/alkalive-backend-wgpu/Cargo.toml` MUST add
  `wgpu = { version = "23", features = ["webgpu", "webgl"] }` to
  `[dependencies]`. The version is pinned to `"23"` (not `"23.*"`); upgrading
  is a follow-up (R7.1).
- **REND-702.** The `wgpu-backend = []` feature in
  `crates/alkalive-backend-wgpu/Cargo.toml` (line 51) MUST be removed.
  (CR-29)
- **REND-703.** `crates/alkalive-backend-wgpu/Cargo.toml` MUST add
  `"OffscreenCanvas"` to the `web-sys` features list (preparing for Gap 8).
  (Gap 7 alone does not use it; Gap 8's `init_from_offscreen` does.)
- **REND-704.** The crate-level doc comment at
  `crates/alkalive-backend-wgpu/src/lib.rs:8-23` (which justifies raw
  WebGL2) MUST be replaced with a doc comment that states: (a) the crate
  now uses `wgpu = "23"`; (b) `wgpu`'s `webgl` feature provides the WebGL2
  fallback transparently; (c) the crate name `alkalive-backend-wgpu` now
  matches the implementation.

### Shader files

- **REND-705.** Two new files MUST be created at
  `crates/alkalive-backend-wgpu/src/shaders/text_quad.wgsl` and
  `crates/alkalive-backend-wgpu/src/shaders/rect.wgsl` with the exact WGSL
  source in §2.2.
- **REND-706.** The existing GLSL constants `VERTEX_SHADER_SRC`,
  `FRAGMENT_SHADER_SRC`, `RECT_VERTEX_SHADER_SRC`,
  `RECT_FRAGMENT_SHADER_SRC` at
  `crates/alkalive-backend-wgpu/src/lib.rs:186-289` MUST be removed. They
  are replaced by `include_str!` constants `TEXT_QUAD_WGSL` and
  `RECT_WGSL`.
- **REND-707.** `crates/alkalive-backend-wgpu/src/lib.rs` MUST expose:
  ```rust
  pub const TEXT_QUAD_WGSL: &str = include_str!("shaders/text_quad.wgsl");
  pub const RECT_WGSL: &str = include_str!("shaders/rect.wgsl");
  ```
- **REND-708.** A new file `crates/alkalive-backend-wgpu/src/shaders/README.md`
  MUST document the shader directory's purpose and the `include_str!`
  convention.

### wgpu renderer fields

- **REND-709.** The `WgpuRenderer` struct (wasm32 variant) MUST be
  rewritten so that the existing `WebGl*` fields are replaced by `wgpu`
  fields per §2.2. The `WebGl2RenderingContext`, `WebGlProgram`,
  `WebGlShader`, `WebGlTexture`, `WebGlUniformLocation`,
  `WebGlVertexArrayObject`, `WebGlBuffer` fields are removed.
- **REND-710.** The new fields are: `instance: wgpu::Instance`, `surface:
  wgpu::Surface<'static>`, `adapter: wgpu::Adapter`, `device:
  wgpu::Device`, `queue: wgpu::Queue`, `surface_config:
  wgpu::SurfaceConfiguration`, `surface_format: wgpu::TextureFormat`,
  `pipeline_cache: alkalive_render::PipelineCache`, `buffer_table:
  Vec<wgpu::Buffer>`, `texture_table: Vec<wgpu::Texture>`, `sampler_table:
  Vec<wgpu::Sampler>`, `glyph_run_table: Vec<(u32, u32)>`, `text_pipeline:
  wgpu::RenderPipeline`, `rect_pipeline: wgpu::RenderPipeline`,
  `text_bind_group_layout: wgpu::BindGroupLayout`,
  `rect_bind_group_layout: wgpu::BindGroupLayout`,
  `text_uniform_buffer: wgpu::Buffer`,
  `rect_uniform_buffer: wgpu::Buffer`.
- **REND-711.** The existing fields `font_registry: Option<Arc<...>>`,
  `font_id`, `text_shaper`, `width`, `height`, `performance`, `start_ms`,
  `input_field_bounds`, `atlas_uploaded`, `last_input_text`,
  `title_vertex_count`, `input_vertex_start`, `input_vertex_count` are
  preserved. They are target-agnostic and unchanged.

### init_from_canvas

- **REND-712.** `WgpuRenderer::init_from_canvas` MUST be `async` and MUST
  follow the exact sequence in §2.3: create `wgpu::Instance` with
  `Backends::BROWSER_WEBGPU | Backends::BROWSER_WEBGL`; create surface from
  canvas; request adapter; request device + queue; configure surface;
  create shader modules; create bind-group layouts; create render
  pipelines; populate `pipeline_cache`; return `Ok(Self { ... })`.

### render_compiled (CR-5 resolution)

- **REND-713.** `render_compiled` MUST acquire the next frame's texture via
  `self.surface.get_current_texture()`. On `Err`, log via
  `console::warn_1` and return (skip the frame).
- **REND-714.** `render_compiled` MUST create a single
  `wgpu::CommandEncoder` and a single `wgpu::RenderPass` for the whole
  frame. (Today's Hello World scene collapses all 5 passes into one
  wgpu render pass because they all share the same color attachment. The
  `compiled.sorted_passes` iteration is preserved; the per-pass
  `RenderPass` boundaries are not separate `wgpu::RenderPass` enclosures.)
- **REND-715.** **CR-5 resolution.** The render pass's `LoadOp` MUST be
  sourced from the first `DrawCallKind::Clear { color }` in
  `compiled.sorted_passes`. The lookup is: iterate `compiled.sorted_passes`;
  for each `pass_id`, find the pass in `graph.passes`; for each `dc_id` in
  `pass.draw_calls`, find the `DrawCall`; if `dc.kind` is `Clear { color }`,
  use `wgpu::LoadOp::Clear(wgpu::Color { r: color[0] as f64, g: color[1]
  as f64, b: color[2] as f64, a: color[3] as f64 })` and stop the search.
  If no Clear draw call is found, fall back to
  `wgpu::LoadOp::Clear(wgpu::Color::BLACK)` AND log via
  `console::warn_1(&"AlkALive: no Clear draw call in graph; falling back to
  black".into())`.

### Pipeline cache (CR-13)

- **REND-716.** At init time (in `init_from_canvas`), the renderer MUST
  populate `pipeline_cache` with three entries keyed by
  `(shader_hash, layout_hash, target_format)`:
  - `(hash(TEXT_QUAD_WGSL), hash(text_bgl), surface_format)` →
    `PIPELINE_TEXT`;
  - `(hash(RECT_WGSL), hash(rect_bgl), surface_format)` →
    `PIPELINE_RECT`;
  - `(0, 0, surface_format)` → `PIPELINE_CLEAR` (the clear "pipeline" is
    not a real `wgpu::RenderPipeline`; it's a sentinel handle for the
    `LoadOp::Clear` fast path).
- **REND-717.** `DrawCallKind::DrawCustom { shader_hash, .. }` lookup in
  the pipeline cache MUST be a `cache.get(shader_hash, layout_hash,
  surface_format)` call. On a miss, log via `console::warn_1` and skip the
  draw call. Today this branch is unreachable (no `DrawCustom` draw calls
  are produced by `schedule_to_render_graph`); the path is in place for
  ADR-006's future author-supplied WGSL.
- **REND-718.** The renderer MUST expose `pub fn device(&self) ->
  &wgpu::Device` for unit tests (per Q7.4 of the fine draft).

### Atlas upload

- **REND-719.** `ensure_atlas_uploaded` MUST be a separate method called
  once per frame before the render-pass encoder begins. It calls the
  existing `upload_text_atlas` logic (HarfRust shaping + rasterization +
  atlas upload) but uploads to a `wgpu::Texture` (via
  `queue.write_texture`) instead of a `WebGlTexture` (via
  `texImage2D`).
- **REND-720.** The `glyph_run_table` MUST be populated with two entries
  (title and input) at first upload, keyed by `GlyphRunId(0)` and
  `GlyphRunId(1)`.

### Native stub

- **REND-721.** The native stub at
  `crates/alkalive-backend-wgpu/src/lib.rs:1348-1429` MUST be removed. The
  real `wgpu` path runs on native for headless testing via
  `wgpu::Backends::SECONDARY` (lavapipe on Linux, software renderer on
  macOS/Windows). CI MUST install `lavapipe` (Linux) or use
  `xvfb-run`.

### Frame pacing

- **REND-722.** `wgpu::PresentMode::AutoVsync` MUST be used in the surface
  configuration. The runtime's `requestAnimationFrame` loop drives one
  render per RAF callback.
- **REND-723.** `wgpu::Queue.submit` is non-blocking — the renderer's
  `render_compiled` returns immediately after `submit`. `output.present()`
  is also non-blocking.

### Backward compatibility

- **REND-724.** The `render_compiled` signature from Gap 6
  (`render_compiled(&mut self, graph: &RenderGraph, compiled:
  &CompiledGraph, time: f32)`) MUST be unchanged. Gap 7 swaps the
  internals only.

---

## 2.2 Data structures

### WGSL shader source

#### `crates/alkalive-backend-wgpu/src/shaders/text_quad.wgsl` (REND-705)

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

#### `crates/alkalive-backend-wgpu/src/shaders/rect.wgsl` (REND-705)

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

### `WgpuRenderer` struct (REND-709, REND-710)

```rust
#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use alkalive_render::{RenderGraph, CompiledGraph, PipelineCache, PipelineHandle};
    use std::sync::Arc;

    pub struct WgpuRenderer {
        // wgpu core objects.
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,

        // Surface configuration (re-created on resize).
        surface_config: wgpu::SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,

        // Pipeline cache (existing type from alkalive-render).
        pipeline_cache: PipelineCache,

        // Pre-compiled pipelines (also referenced by the cache).
        text_pipeline: wgpu::RenderPipeline,
        rect_pipeline: wgpu::RenderPipeline,
        text_bind_group_layout: wgpu::BindGroupLayout,
        rect_bind_group_layout: wgpu::BindGroupLayout,

        // Uniform buffers (one per pipeline, updated per-frame).
        text_uniform_buffer: wgpu::Buffer,
        rect_uniform_buffer: wgpu::Buffer,

        // Buffer / texture / sampler tables.
        buffer_table: Vec<wgpu::Buffer>,
        texture_table: Vec<wgpu::Texture>,
        sampler_table: Vec<wgpu::Sampler>,

        // Per-frame glyph-run table (start, count) ranges keyed by GlyphRunId.
        glyph_run_table: Vec<(u32, u32)>,

        // Cached font infrastructure (preserved from the WebGL2 path).
        font_registry: Option<Arc<alkalive_text::HarfRustFontRegistry>>,
        font_id: Option<alkalive_text::FontId>,
        text_shaper: Option<alkalive_text::HarfRustTextShaper>,

        // Canvas dimensions (physical pixels).
        width: u32,
        height: u32,

        // Animation clock.
        performance: web_sys::Performance,
        start_ms: f64,

        // Atlas upload state (preserved).
        atlas_uploaded: bool,
        last_input_text: String,
        title_vertex_count: u32,
        input_vertex_start: u32,
        input_vertex_count: u32,

        // Input field bounds (for hit-testing — kept until ADR-004 lands).
        input_field_bounds: (f32, f32, f32, f32),
    }
}
```

---

## 2.3 Interfaces and contracts

### `init_from_canvas` (REND-712)

```rust
impl WgpuRenderer {
    pub async fn init_from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // 1. Create the wgpu instance with both WebGPU and WebGL backends.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::BROWSER_WEBGL,
            backend_options: wgpu::BackendOptions::default(),
            flags: wgpu::InstanceFlags::default(),
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
            present_mode: wgpu::PresentMode::AutoVsync,  // REND-722
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        // 6. Create the shader modules.
        let text_module = device.create_shader_module(wgpu::ShaderSource::Wgsl(
            std::borrow::Cow::Borrowed(TEXT_QUAD_WGSL),
        ));
        let rect_module = device.create_shader_module(wgpu::ShaderSource::Wgsl(
            std::borrow::Cow::Borrowed(RECT_WGSL),
        ));

        // 7. Create bind-group layouts.
        let text_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("AlkALive text BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(32),  // sizeof(Uniforms)
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            },
        );
        let rect_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("AlkALive rect BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(48),  // sizeof(RectUniforms)
                        },
                        count: None,
                    },
                ],
            },
        );

        // 8. Create render pipelines.
        let text_pipeline = device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("AlkALive text pipeline"),
                layout: Some(&device.create_pipeline_layout(
                    &wgpu::PipelineLayoutDescriptor {
                        label: Some("AlkALive text PLL"),
                        bind_group_layouts: &[&text_bind_group_layout],
                        push_constant_ranges: &[],
                    },
                )),
                vertex: wgpu::VertexState {
                    module: &text_module,
                    entry_point: "vs_main",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: 16,  // sizeof(Vertex) = 4 floats
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x2,  // position
                                1 => Float32x2,  // uv
                            ],
                        },
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &text_module,
                    entry_point: "fs_main",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::TextureFormat::Bgra8Unorm.into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            },
        );
        let rect_pipeline = device.create_render_pipeline(/* similar, using rect_module + rect_bgl */);

        // 9. Create uniform buffers.
        let text_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AlkALive text uniforms"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let rect_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AlkALive rect uniforms"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 10. Populate the pipeline cache (REND-716).
        let mut pipeline_cache = PipelineCache::default();
        let text_shader_hash = hash_wgsl(TEXT_QUAD_WGSL);
        let rect_shader_hash = hash_wgsl(RECT_WGSL);
        let text_layout_hash = hash_bgl(&text_bind_group_layout);
        let rect_layout_hash = hash_bgl(&rect_bind_group_layout);
        let target_format = alkalive_render::AttachmentFormat::Bgra8Unorm;
        pipeline_cache.insert(
            &alkalive_render::PipelineDesc {
                shader_hash: text_shader_hash,
                layout_hash: text_layout_hash,
                target_format,
                sample_count: alkalive_render::SampleCount::X1,
            },
            alkalive_render::PIPELINE_TEXT,
        );
        pipeline_cache.insert(/* rect */);
        pipeline_cache.insert(/* clear sentinel (shader_hash=0, layout_hash=0) */);

        // 11. Performance clock.
        let window = web_sys::window().ok_or_else(|| "no global window".to_string())?;
        let performance = window
            .performance()
            .ok_or_else(|| "window.performance unavailable".to_string())?;
        let start_ms = performance.now();

        Ok(Self {
            instance, surface, adapter, device, queue,
            surface_config, surface_format: format,
            pipeline_cache,
            text_pipeline, rect_pipeline,
            text_bind_group_layout, rect_bind_group_layout,
            text_uniform_buffer, rect_uniform_buffer,
            buffer_table: Vec::new(),
            texture_table: Vec::new(),
            sampler_table: Vec::new(),
            glyph_run_table: Vec::new(),
            font_registry: None,
            font_id: None,
            text_shaper: None,
            width, height,
            performance, start_ms,
            atlas_uploaded: false,
            last_input_text: String::new(),
            title_vertex_count: 0,
            input_vertex_start: 0,
            input_vertex_count: 0,
            input_field_bounds: (0.0, 0.0, 0.0, 0.0),
        })
    }

    /// REND-718: expose the device for unit tests.
    pub fn device(&self) -> &wgpu::Device { &self.device }
}
```

### `render_compiled` (REND-713, REND-714, REND-715 — CR-5 resolution)

```rust
impl WgpuRenderer {
    pub fn render_compiled(
        &mut self,
        graph: &alkalive_render::RenderGraph,
        compiled: &alkalive_render::CompiledGraph,
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

        // 2. Atlas upload.
        if let Err(e) = self.ensure_atlas_uploaded() {
            web_sys::console::error_1(&format!("atlas upload: {}", e).into());
            return;
        }

        // 3. CR-5: source the clear color from the first DrawCallKind::Clear
        //    in the graph. Fall back to black with a warning if none.
        let mut clear_color = wgpu::Color::BLACK;
        let mut found_clear = false;
        for &pass_id in &compiled.sorted_passes {
            if found_clear { break; }
            let pass = match graph.passes.iter().find(|p| p.id == pass_id) {
                Some(p) => p, None => continue,
            };
            for &dc_id in &pass.draw_calls {
                let dc = match graph.draw_calls.iter().find(|d| d.id == dc_id) {
                    Some(d) => d, None => continue,
                };
                if let alkalive_render::DrawCallKind::Clear { color } = &dc.kind {
                    clear_color = wgpu::Color {
                        r: color[0] as f64,
                        g: color[1] as f64,
                        b: color[2] as f64,
                        a: color[3] as f64,
                    };
                    found_clear = true;
                    break;
                }
            }
        }
        if !found_clear {
            web_sys::console::warn_1(
                &"AlkALive: no Clear draw call in graph; falling back to black".into(),
            );
        }

        // 4. Encode the command buffer.
        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("AlkALive frame") },
        );

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("AlkALive main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),  // CR-5
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Iterate sorted passes and dispatch each draw call.
            for &pass_id in &compiled.sorted_passes {
                let pass = match graph.passes.iter().find(|p| p.id == pass_id) {
                    Some(p) => p, None => continue,
                };
                for &dc_id in &pass.draw_calls {
                    let dc = match graph.draw_calls.iter().find(|d| d.id == dc_id) {
                        Some(d) => d, None => continue,
                    };
                    self.execute_draw_call_wgpu(&mut rpass, graph, dc, time);
                }
            }
        }

        // 5. Submit + present (non-blocking — REND-723).
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn execute_draw_call_wgpu<'a>(
        &mut self,
        rpass: &mut wgpu::RenderPass<'a>,
        graph: &alkalive_render::RenderGraph,
        dc: &alkalive_render::DrawCall,
        time: f32,
    ) {
        use alkalive_render::DrawCallKind;
        match &dc.kind {
            // Clear is handled by the LoadOp::Clear above; no draw needed.
            DrawCallKind::Clear { .. } => {}

            DrawCallKind::DrawRect { bounds, color } => {
                let bounds = self.real_rect_bounds(*bounds);
                // Write rect uniforms.
                let uniforms: [f32; 12] = [
                    bounds.x, bounds.y, bounds.x + bounds.w, bounds.y + bounds.h,
                    color[0], color[1], color[2], color[3],
                    self.width as f32, self.height as f32, 0.0, 0.0,  // pad
                ];
                let bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        uniforms.as_ptr() as *const u8,
                        std::mem::size_of_val(&uniforms),
                    )
                };
                self.queue.write_buffer(&self.rect_uniform_buffer, 0, bytes);

                rpass.set_pipeline(&self.rect_pipeline);
                rpass.set_bind_group(0, &self.rect_bind_group, &[]);  // bind group created earlier
                rpass.draw(0..4, 0..1);  // full-viewport quad
            }

            DrawCallKind::DrawText { glyph_run_id, color, rotation, .. } => {
                let rotation = rotation * time;
                let uniforms: [f32; 8] = [
                    rotation,
                    self.width as f32,
                    self.height as f32,
                    time,
                    color[0], color[1], color[2], color[3],
                ];
                let bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        uniforms.as_ptr() as *const u8,
                        std::mem::size_of_val(&uniforms),
                    )
                };
                self.queue.write_buffer(&self.text_uniform_buffer, 0, bytes);

                let (start, count) = self.glyph_run_range(*glyph_run_id);
                rpass.set_pipeline(&self.text_pipeline);
                rpass.set_bind_group(0, &self.text_bind_group, &[]);
                rpass.draw(start..(start + count), 0..1);
            }

            DrawCallKind::DrawCustom { shader_hash, .. } => {
                // REND-717: look up the custom pipeline; on miss, warn + skip.
                if let Some(_handle) = self.pipeline_cache.get(
                    *shader_hash,
                    /* layout_hash */ 0,
                    alkalive_render::AttachmentFormat::Bgra8Unorm,
                ) {
                    // TODO (future wave): bind the custom pipeline + issue draw.
                } else {
                    web_sys::console::warn_1(
                        &format!("AlkALive: custom pipeline {:?} not in cache", shader_hash).into(),
                    );
                }
            }
        }
    }
}
```

### Helper: `hash_wgsl`, `hash_bgl`, `glyph_run_range`, `real_rect_bounds`, `ensure_atlas_uploaded`

These are existing helpers preserved verbatim from the WebGL2 path.
`hash_wgsl` uses `std::hash::DefaultHasher` on the WGSL source bytes.
`hash_bgl` uses the same hasher on the BGL's entries (serialised as a
stable byte sequence). `glyph_run_range` looks up `self.glyph_run_table[id
as usize]`. `real_rect_bounds` returns `DirtyRect::from(self.input_field_bounds)`
when the input `bounds == (0,0,0,0)` (the placeholder per REND-619),
otherwise returns the input unchanged. `ensure_atlas_uploaded` calls the
existing `upload_text_atlas` logic but uses `queue.write_texture` instead
of `texImage2D`.

---

## 2.4 State transitions

### Pipeline cache lifecycle

```
┌──────────────────────────┐
│ Renderer::init_from_canvas │
└──────┬───────────────────┘
       │
       ▼
┌──────────────────────────┐
│ Create shader modules    │
│ (text_module, rect_module)│
└──────┬───────────────────┘
       │
       ▼
┌──────────────────────────┐
│ Create BGLs + pipelines  │
│ (text_pipeline, rect_    │
│  pipeline)               │
└──────┬───────────────────┘
       │
       ▼
┌──────────────────────────┐
│ pipeline_cache.insert(   │
│   text desc, PIPELINE_   │
│   TEXT)                  │
│ pipeline_cache.insert(   │
│   rect desc, PIPELINE_   │
│   RECT)                  │
│ pipeline_cache.insert(   │
│   clear sentinel,        │
│   PIPELINE_CLEAR)        │
└──────┬───────────────────┘
       │
       ▼
┌──────────────────────────┐
│ Ready: render_compiled   │
│ uses text_pipeline /     │
│ rect_pipeline directly   │
│ (no per-frame cache      │
│ lookup needed for the 3  │
│ built-in pipelines).     │
└──────────────────────────┘
```

### Surface lifecycle

```
┌──────────────────────────┐
│ init_from_canvas         │
│ └─ surface.configure()   │
└──────┬───────────────────┘
       │
       ▼
┌──────────────────────────┐    Resize event     ┌──────────────────────┐
│ Rendering (steady state) │────────────────────▶│ Reconfigure surface  │
│ └─ get_current_texture   │                     │ (surface_config.width│
│ └─ render pass           │                     │  / height updated;   │
│ └─ submit + present      │                     │  surface.configure)  │
└──────┬───────────────────┘                     └──────┬───────────────┘
       │                                                │
       │ SurfaceError::Lost                             │
       ▼                                                │
┌──────────────────────────┐                            │
│ Reconfigure surface      │◀───────────────────────────┘
│ (same as resize)         │
└──────┬───────────────────┘
       │
       │ Reconfigure fails
       ▼
┌──────────────────────────┐
│ Re-init renderer         │
│ (call init_from_canvas   │
│  again — Gap 8 will      │
│  isolate this to the     │
│  worker)                 │
└──────────────────────────┘
```

---

## 2.5 Error cases

| ID | Error class | Source | Message format | Handling |
|----|-------------|--------|----------------|----------|
| REND-7-E1 | `wgpu::RequestAdapterError` | No suitable GPU adapter | `"AlkALive: wgpu adapter request failed: <e>"` | Returned as `Err(String)` from `init_from_canvas`; the runtime logs via `console::error_1` and the canvas stays blank. Future: ADR-016 CPU rasterizer fallback. |
| REND-7-E2 | `wgpu::RequestDeviceError` | Adapter cannot create a device | `"AlkALive: wgpu device request failed: <e>"` | Same. |
| REND-7-E3 | `wgpu::SurfaceError::Lost` | Surface lost | `"AlkALive: surface lost; reconfiguring"` | Reconfigure the surface; skip the frame. If reconfiguration fails, re-init the renderer. |
| REND-7-E4 | `wgpu::SurfaceError::Outdated` | Surface size mismatch | `"AlkALive: surface outdated; reconfiguring"` | Same. |
| REND-7-E5 | `wgpu::ValidationError` (shader compile) | WGSL syntax error | `"AlkALive: WGSL compile failed: <e>"` | This is a compile-time bug — should never happen in a release build. Caught by `T-7-1` and `T-7-2` (unit tests that compile the WGSL on native). |
| REND-7-E6 | Pipeline cache miss for a custom shader | `DrawCallKind::DrawCustom` with unknown `shader_hash` | `"AlkALive: custom pipeline <shader_hash> not in cache"` | Log via `console::warn_1`; skip the draw call. (Future: ADR-006 author-supplied WGSL.) |
| REND-7-E7 | No Clear draw call in graph (CR-5 fallback) | `render_compiled` did not find a `DrawCallKind::Clear` | `"AlkALive: no Clear draw call in graph; falling back to black"` | Log via `console::warn_1`; use `wgpu::Color::BLACK` as the LoadOp. |

---

## 2.6 Validation rules

1. **V-7-1.** `TEXT_QUAD_WGSL` MUST compile successfully via
   `device.create_shader_module(ShaderSource::Wgsl(...))` on a native
   `wgpu::Device` (verified by `T-7-1`). Any `wgpu::ValidationError` is a
   build-time bug.
2. **V-7-2.** `RECT_WGSL` MUST compile successfully (verified by `T-7-2`).
3. **V-7-3.** The text render pipeline MUST link successfully (vertex +
   fragment + BGL). Verified by `T-7-3`.
4. **V-7-4.** The rect render pipeline MUST link successfully. Verified by
   `T-7-4`.
5. **V-7-5.** The surface format MUST be one of `caps.formats`. The
   renderer picks `Bgra8Unorm` if available, else `caps.formats[0]`. The
   chosen format is stored in `self.surface_format` and used to key the
   pipeline cache.
6. **V-7-6.** The uniform buffer sizes MUST match the WGSL struct layouts:
   `Uniforms` = 32 bytes (4 floats + vec4 = 8 floats × 4 bytes), padded
   to 32; `RectUniforms` = 48 bytes (vec4 + vec4 + vec2 padded = 12
   floats × 4 bytes).
7. **V-7-7.** `wgpu::Queue.submit` is non-blocking. The renderer's
   `render_compiled` returns immediately after submit.
8. **V-7-8.** The clear color from `DrawCallKind::Clear { color }` MUST be
   cast to `wgpu::Color` with `f32 → f64` widening (no precision loss
   for color values in `[0.0, 1.0]`).

---

## 2.7 Performance requirements

| ID | Requirement | Measurement |
|----|-------------|-------------|
| REND-7-P1 | `init_from_canvas` MUST complete in **< 50 ms** for the first frame on the M1 Air baseline (Chrome 113+). | Manual measurement via `performance.now()` deltas in `init_runtime`. |
| REND-7-P2 | `wgpu::RenderPipeline` creation (text + rect) MUST be cached. First-frame cost: pipeline creation < 50 ms. Subsequent frames: < 1 ms (no pipeline creation). | Manual measurement; assert that `device.create_render_pipeline` is called exactly twice (text + rect) at init, zero times per frame. |
| REND-7-P3 | `render_compiled` for the Hello World scene MUST complete in **< 4 ms** on the M1 Air baseline (the wgpu path is slightly slower than raw WebGL2 due to the encoder overhead, but still well under the 16 ms frame budget). | Manual measurement via `performance.now()` deltas, logged every 60th frame. |
| REND-7-P4 | The WASM binary size MUST NOT exceed **1.8 MB** post-build (`wasm-opt -Oz`). Pre-Gap-7 baseline: 1.05 MB. Allowed growth: ~750 KB (CR-25 budget). | `ls -l deploy/pkg/alkalive_runtime_wasm_bg.wasm` after `wasm-pack build --release`. |
| REND-7-P5 | The WASM binary MUST be streamable (ADR-017): `WebAssembly.instantiateStreaming` MUST succeed on the deployed `.wasm` file with `application/wasm` MIME type. | Browser DevTools Network tab: the `.wasm` response has `Content-Type: application/wasm`. |
| REND-7-P6 | `ensure_atlas_uploaded` (first frame) MUST complete in **< 30 ms** for the Hello World text. Subsequent frames (atlas cached): **< 100 µs**. | Manual measurement. |

---

## 2.8 Browser/platform integration

### WebGPU available (Chrome 113+, Edge 113+)

- `wgpu` uses the native WebGPU backend.
- Shader compilation is via `device.create_shader_module` with WGSL source.
- Pipeline precompilation (ADR-017) is possible via
  `device.create_render_pipeline_async` (overlaps with module decode); this
  is a future optimisation, not required for Gap 7's first cut.

### WebGPU unavailable (Firefox, Safari < 17.4, old Chrome)

- `wgpu` falls back to the `webgl` feature, which translates WGSL to GLSL
  ES 3.00 via `naga` (a `wgpu` transitive dep). The translation happens at
  shader-module creation time; the resulting GLSL is compiled by the
  browser's WebGL2 driver.
- **Caveat**: the WebGL2 fallback does not support compute passes (WebGL2
  has no compute). ADR-006's "compute passes" feature is WebGPU-only. The
  renderer detects the backend at init time and exposes a `features()`
  method returning the available feature set; the schedule lowering skips
  compute passes on WebGL2.

### Native (test host)

- `wgpu` supports native backends (Vulkan on Linux/Windows, Metal on macOS,
  DX12 on Windows). The native stub (REND-721) is removed; the real
  `wgpu` path runs on native for headless testing.
- CI MUST install `lavapipe` (Linux) or use `xvfb-run`. Documented in the
  CI workflow file.

### OffscreenCanvas (Gap 8 preparation)

- When the renderer moves to a Web Worker (Gap 8), the canvas is
  transferred to the worker via `canvas.transferControlToOffscreen()`.
  The `wgpu::Surface` is created from the `OffscreenCanvas` instead of the
  `HtmlCanvasElement`. This is supported by `wgpu::SurfaceTarget::Canvas`
  (which accepts both).
- The `init_from_offscreen` method (added in Gap 8) has the same body as
  `init_from_canvas` except the first argument is
  `web_sys::OffscreenCanvas` and `wgpu::SurfaceTarget::Canvas` accepts it.

---

## 2.9 Test cases

| ID | Test | Expected behaviour |
|----|------|---------------------|
| T-7-1 [`alkalive-backend-wgpu`] (native) | `TEXT_QUAD_WGSL` compiles via `device.create_shader_module` | No `wgpu::ValidationError`; the returned `ShaderModule` is non-null. |
| T-7-2 [`alkalive-backend-wgpu`] (native) | `RECT_WGSL` compiles | Same. |
| T-7-3 [`alkalive-backend-wgpu`] (native) | The text render pipeline links | `device.create_render_pipeline(...)` returns without error. |
| T-7-4 [`alkalive-backend-wgpu`] (native) | The rect render pipeline links | Same. |
| T-7-5 [`alkalive-backend-wgpu`] (native) | `WgpuRenderer::init_from_canvas` succeeds on a headless wgpu device (lavapipe) | Returns `Ok(Self)`; `self.surface_format` is one of `Bgra8Unorm` / `Rgba8UnormSrgb`. |
| T-7-6 [`alkalive-render`] | `pipeline_for_kind(DrawText)` returns `PIPELINE_TEXT`; the cache lookup for `(text_shader_hash, text_layout_hash, Bgra8Unorm)` returns `PIPELINE_TEXT` | (uses the same hash functions as the renderer). |
| T-7-7 [`alkalive-backend-wgpu`] (wasm32, headless Chrome) | The renderer initializes on a canvas with WebGPU | `init_from_canvas` resolves; the surface's `get_capabilities` reports a non-empty `formats`. |
| T-7-8 [`alkalive-backend-wgpu`] (wasm32, headless Firefox) | The renderer initializes on a canvas with WebGL2 fallback | Same; the `wgpu` `webgl` feature is exercised. |
| T-7-9 [`alkalive-runtime-wasm`] (wasm32) | One frame renders without panicking | After `start(canvas, ime)` and one `requestAnimationFrame`, the canvas has non-zero pixel content (verified via `gl.readPixels` on a 1×1 region). |
| T-7-10 (browser verification, manual) | Visual parity with the pre-Gap-7 output: golden "Hello World!" text, dark input field with gold border, both rotating correctly on the Y axis | Screenshot comparison; pixel diff < 1%. |
| T-7-11 [`alkalive-backend-wgpu`] | `render_compiled` sources the clear color from `DrawCallKind::Clear { color }` (CR-5) | Construct a graph with `DrawCallKind::Clear { color: [1.0, 0.0, 0.0, 1.0] }`; render; `gl.readPixels(0, 0, 1, 1)` returns red. Construct a graph with no Clear draw call; the warn message REND-7-E7 is logged. |
| T-7-12 [`alkalive-backend-wgpu`] (bench) | `init_from_canvas` completes in < 50 ms (REND-7-P1) | Manual measurement. |
| T-7-13 [`alkalive-backend-wgpu`] (bench) | `render_compiled` for Hello World completes in < 4 ms (REND-7-P3) | Manual measurement. |
| T-7-14 (build check) | WASM binary size ≤ 1.8 MB after `wasm-opt -Oz` (REND-7-P4) | `ls -l` after build. |
| T-7-15 (regression) | All 1148 existing tests pass | `cargo test --workspace` succeeds. |

---

## 2.10 Acceptance criteria

1. `cargo test -p alkalive-backend-wgpu` passes `T-7-1` through `T-7-6` on
   native (with `lavapipe`).
2. `cargo test -p alkalive-runtime-wasm --target wasm32-unknown-unknown`
   passes `T-7-7`, `T-7-8`, `T-7-9` in headless browsers.
3. `cargo bench -p alkalive-backend-wgpu` reports `T-7-12` < 50 ms,
   `T-7-13` < 4 ms.
4. The Hello World demo renders with visual parity (T-7-10).
5. `grep -r "VERTEX_SHADER_SRC\|FRAGMENT_SHADER_SRC\|RECT_VERTEX_SHADER_SRC\|RECT_FRAGMENT_SHADER_SRC" crates/` returns no matches (REND-706).
6. `grep -r "wgpu-backend = " crates/alkalive-backend-wgpu/Cargo.toml` returns no matches (REND-702, CR-29).
7. The WASM binary size is ≤ 1.8 MB (T-7-14).

---

## 2.11 Traceability

| Requirement | ADR / source | Fine-draft § | Implementation § | Test ID | CR addressed |
|-------------|--------------|--------------|------------------|---------|--------------|
| REND-701 (wgpu = "23" dep) | ADR-006 line 165; ADR-001 line 61 ("WebGPU is the initial backend") | §7.5.1 | §1.8 (Cargo.toml) | (build check) | — |
| REND-702 (remove wgpu-backend feature) | (critical review) | §7.5.1 line 1346-1348 | §1.8 | (grep check) | CR-29 |
| REND-705 (WGSL shader files) | ADR-006 line 165 | §7.5.2, §7.5.3 | §2.2 | T-7-1, T-7-2 | — |
| REND-706 (remove GLSL constants) | ADR-006 line 165 | §7.5.3 | §2.2 | (grep check) | — |
| REND-709/710 (wgpu renderer fields) | ADR-006 line 165 | §7.5.4 | §2.2 | (build check) | — |
| REND-712 (init_from_canvas sequence) | ADR-006 line 165; ADR-017 line 600 | §7.5.4 | §2.3 | T-7-5, T-7-7, T-7-8 | — |
| REND-713 (acquire next frame texture) | ADR-001 line 55 | §7.5.4 | §2.3 | T-7-9 | — |
| REND-715 (CR-5 clear color from DrawCallKind::Clear) | ADR-001 line 55 | §7.5.4 line 1674 | §2.3 | T-7-11 | CR-5 |
| REND-716 (pipeline cache populated at init) | ADR-017 line 600 (pipeline precompilation) | §7.5.4 | §2.3 | T-7-6 | — |
| REND-717 (DrawCustom cache lookup) | ADR-006 line 165 | §7.5.5 | §2.3 | (manual; future) | — |
| REND-718 (pub fn device for tests) | (Q7.4) | §7.12 | §2.3 | T-7-1 through T-7-4 | — |
| REND-719 (ensure_atlas_uploaded separate) | ADR-022 line 700 (in-WASM text stack) | §6.6 point 3 | §2.3 | T-7-9 | — |
| REND-721 (remove native stub) | (critical review) | §7.5.4 | §2.8 | (grep check) | — |
| REND-722 (PresentMode::AutoVsync) | ADR-021 line 685 (retain-mode loop) | §7.5.4 line 1130 | §2.3 | (code review) | — |
| REND-724 (render_compiled signature stable) | (cross-gap contract) | §5.4 | §2.3 | T-7-9 | — |
| REND-7-P4 (WASM size ≤ 1.8 MB) | ADR-017 line 600 | §7.11 R7.3 | §2.7 | T-7-14 | CR-25 |

---

# Gap 8 — Single-GPU-Device + SAB/COOP-COEP (ADR-003 + ADR-021)

## 3.1 Exact requirements

### New crate

- **REND-801.** A new crate `alkalive-render-worker` MUST be created at
  `crates/alkalive-render-worker/` with `crate-type = ["cdylib", "rlib"]`.
- **REND-802.** `crates/alkalive-render-worker/Cargo.toml` MUST list
  dependencies on `alkalive-backend-wgpu`, `alkalive-render`,
  `alkalive-scene-data`, `alkalive-text`, `alkalive-compiler`,
  `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `web-sys` (with
  features `DedicatedWorkerGlobalScope`, `MessageEvent`, `OffscreenCanvas`,
  `Window`, `console`, `Performance`), `serde = { version = "1", features
  = ["derive"] }`, `serde-wasm-bindgen = "0.6"`.

### Worker entry point

- **REND-803.** The worker MUST expose a `#[wasm_bindgen] pub fn
  init_worker()` entry point that installs a panic hook and registers a
  `message` event listener on the worker's `DedicatedWorkerGlobalScope`.
- **REND-804.** After `init_worker()` returns, the worker MUST post a
  `{ kind: "ready" }` message back to the main thread.
- **REND-805.** The worker's message handler MUST deserialise incoming
  messages via `serde_wasm_bindgen::from_value::<WorkerMessage>(data)`.
  Deserialisation failure logs via `console::error_1` with the message
  `"AlkALive render-worker: failed to deserialize message: <e>"` and
  returns (no panic).

### Worker message types (CR-1 — full serde)

- **REND-806.** The `WorkerMessage` enum and `WorkerMessageKind` enum MUST
  derive `serde::Deserialize` and have the exact shape in §3.2. Every
  field type referenced (including `RenderGraph`, `CompiledGraph`,
  `OffscreenCanvas`, `DrawCallKind`, etc.) MUST derive
  `serde::Serialize`/`Deserialize` (per REND-601 in Gap 6).
- **REND-807.** The `Render` variant MUST carry `graph:
  alkalive_render::RenderGraph`, `compiled: alkalive_render::CompiledGraph`,
  `time: f32`. The `Init` variant MUST carry `canvas:
  web_sys::OffscreenCanvas`, `width: u32`, `height: u32`. The `Resize`
  variant MUST carry `width: u32`, `height: u32`.
- **REND-808.** `web_sys::OffscreenCanvas` MUST be wrapped in a
  `#[serde(transparent)]` newtype `OffscreenCanvasWrapper(pub
  web_sys::OffscreenCanvas)` because `web_sys::OffscreenCanvas` does not
  implement `Serialize`/`Deserialize` itself — `serde_wasm_bindgen`
  passes JS values through transparently when wrapped this way.

### Worker state

- **REND-809.** The worker's global state MUST be stored in a
  `thread_local! { static STATE: RefCell<RenderWorkerState> }` where
  `RenderWorkerState { renderer: Option<WgpuRenderer>, canvas:
  Option<OffscreenCanvas> }`. The worker is single-threaded (one worker =
  one thread), so `thread_local!` is sufficient.
- **REND-810.** `handle_init(canvas, width, height)` MUST be `async` and
  MUST call `WgpuRenderer::init_from_offscreen(canvas, width, height).await`.
  On success, store the renderer in `STATE`. On failure, log via
  `console::error_1` with `"AlkALive render-worker init failed: <e>"`.

### `init_from_offscreen`

- **REND-811.** `WgpuRenderer::init_from_offscreen(canvas:
  web_sys::OffscreenCanvas, width: u32, height: u32) ->
  impl Future<Output = Result<Self, String>>` MUST be added to
  `crates/alkalive-backend-wgpu/src/lib.rs`. The body is identical to
  `init_from_canvas` except the `canvas: web_sys::HtmlCanvasElement`
  argument becomes `canvas: web_sys::OffscreenCanvas` and
  `wgpu::SurfaceTarget::Canvas` accepts both (via the `From<OffscreenCanvas>`
  impl in `wgpu` v23).

### `handle_render`

- **REND-812.** `handle_render(graph, compiled, time)` MUST call
  `STATE.with(|s| s.borrow_mut().renderer.as_mut().map(|r|
  r.render_compiled(&graph, &compiled, time)))`. If `renderer` is `None`
  (init not yet complete), log via `console::warn_1` with `"AlkALive
  render-worker: render before init"` and drop the message (Q8.6).
- **REND-813.** The main thread MUST buffer up to **N=1** "render"
  messages until the worker's "ready" message arrives (Q8.6). On overflow
  (a second "render" arrives before "ready"), drop the oldest with a
  `console::warn_1` warning.

### `handle_resize`

- **REND-814.** `handle_resize(width, height)` MUST call
  `STATE.with(|s| s.borrow_mut().renderer.as_mut().map(|r|
  r.resize(width, height)))`. The renderer's `resize` method
  reconfigures the `wgpu::Surface` with the new dimensions.

### Main-thread worker spawn

- **REND-815.** `crates/alkalive-runtime-wasm/src/lib.rs` MUST add a
  `spawn_render_worker(canvas: &web_sys::HtmlCanvasElement) ->
  Result<web_sys::Worker, JsValue>` function that:
  1. Calls `canvas.transfer_control_to_offscreen()` to obtain an
     `OffscreenCanvas`.
  2. Constructs `web_sys::Worker::new("/alkalive/render_worker.js")`.
  3. Serialises a `WorkerInitMessage { kind: "init", canvas, width, height }`
     via `serde_wasm_bindgen::to_value`.
  4. Calls `worker.post_message_with_transfer(&init_msg,
     &[offscreen.as_ref()])` to transfer the `OffscreenCanvas` (not copy).
- **REND-816.** The `Runtime` struct MUST gain a `worker:
  Option<RenderWorkerHandle>` field. When `Some`, the frame loop sends
  `postMessage` "render" to the worker; when `None`, it falls back to
  single-threaded `render_compiled` directly.
- **REND-817.** The `Runtime` MUST also keep `renderer:
  Option<WgpuRenderer>` for the fallback path. In the multi-threaded
  path, `renderer` is `None`; in the fallback path, `worker` is `None`.

### `should_use_render_worker` (CR-19 resolution)

- **REND-818.** `should_use_render_worker() -> bool` MUST return `true` if
  and only if **all** of the following hold:
  1. `web_sys::window().Worker` exists (i.e. `Worker` is defined).
  2. The canvas supports `transferControlToOffscreen` (probed via
     `js_sys::Reflect::has(canvas, "transferControlToOffscreen")`).
- **REND-819.** **CR-19 resolution.** `should_use_render_worker` MUST NOT
  check `is_cross_origin_isolated()` (COOP/COEP) for the first cut. The
  first cut uses `postMessage` (structured clone), which works without
  cross-origin isolation. The SAB path (future, §3.7) will require
  COOP/COEP; the check is re-added when SAB lands.

### Worker JS shim

- **REND-820.** A JS file `crates/alkalive-render-worker/src/worker.js`
  MUST be created with the exact content in §3.8.

### Frame loop integration

- **REND-821.** The frame loop (REND-628 from Gap 6) MUST branch on
  `runtime.worker.is_some()`:
  - If `Some(worker)`: serialise `RenderMessage { kind: "render", graph,
    compiled, time }` via `serde_wasm_bindgen::to_value` and call
    `worker.post_message(&msg)`.
  - If `None`: call `runtime.renderer.as_mut().unwrap().render_compiled(
    &runtime.graph, &runtime.compiled, runtime.time)` directly.
- **REND-822.** The runtime's `resize` event handler MUST branch on
  `runtime.worker.is_some()`:
  - If `Some(worker)`: serialise `ResizeMessage { kind: "resize", width,
    height }` and `worker.post_message(&msg)`.
  - If `None`: call `runtime.renderer.as_mut().unwrap().resize(width,
    height)` directly.

### COOP/COEP headers (CR-12 resolution)

- **REND-823.** A new file `Caddyfile` MUST be created at the repo root
  (`/home/z/my-project/AlkALive/Caddyfile`) with the exact content in
  §3.8. This is the canonical dev server; the existing
  `deploy/index.html` is served from `:8080` by `caddy run`.
- **REND-824.** The COOP/COEP headers (set by Caddy) MUST be:
  ```
  Cross-Origin-Opener-Policy: same-origin
  Cross-Origin-Embedder-Policy: require-corp
  ```
  These are the exact values mandated by ADR-003 line 99. The
  `credentialless` alternative is documented in §3.8 but not enabled by
  default.
- **REND-825.** **CR-12 resolution.** The COOP/COEP configuration is
  documented for the actual deployment stack (static `deploy/index.html`
  served by Caddy in dev, and portable header documentation for any other
  static server in §3.8). The non-existent `next.config.ts` from the fine
  draft is NOT created.
- **REND-826.** When `should_use_render_worker()` returns `false` (Worker
  or OffscreenCanvas unavailable), the runtime MUST log via
  `console::warn_1` with `"AlkALive: render worker unavailable; using
  single-threaded fallback"` and proceed with the fallback path. The
  app remains functional.

### Loading indicator

- **REND-827.** `deploy/index.html` MUST add a `<div id="loading">Loading
  AlkALive…</div>` element shown during worker startup. The runtime hides
  it after the worker posts "ready" (or after `init_runtime` completes in
  the fallback path).
- **REND-828.** Worker init timeout: if the worker does not post "ready"
  within **5 seconds** of `spawn_render_worker` being called, the runtime
  MUST terminate the worker (`worker.terminate()`), log via
  `console::error_1` with `"AlkALive: render worker init timeout (5s)"`,
  and fall back to single-threaded rendering.

---

## 3.2 Data structures

### Worker message types (CR-1)

```rust
// In crates/alkalive-render-worker/src/lib.rs

use serde::Deserialize;

/// The top-level message envelope received by the worker.
#[derive(Deserialize)]
pub struct WorkerMessage {
    pub kind: WorkerMessageKind,
}

/// The tagged message payload. Each variant corresponds to one main-thread
/// → worker command.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessageKind {
    /// Initial init message: create the renderer from an OffscreenCanvas.
    Init {
        canvas: OffscreenCanvasWrapper,
        width: u32,
        height: u32,
    },
    /// Per-frame render request.
    Render {
        graph: alkalive_render::RenderGraph,
        compiled: alkalive_render::CompiledGraph,
        time: f32,
    },
    /// Canvas resize.
    Resize {
        width: u32,
        height: u32,
    },
}

/// Wrapper around `web_sys::OffscreenCanvas` so it can pass through
/// `serde_wasm_bindgen` (REND-808). The JS value is transferred via
/// `post_message_with_transfer`, so the wrapper is a transparent pass-through.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct OffscreenCanvasWrapper(pub web_sys::OffscreenCanvas);

/// Messages sent FROM the worker TO the main thread.
#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerToMainMessage {
    /// Worker init complete; ready for `Render` messages.
    Ready,
    /// Worker encountered an error; main thread should fall back.
    Error { message: String },
}

/// Main-thread → worker init message (used by `spawn_render_worker`).
#[derive(serde::Serialize)]
pub struct WorkerInitMessage {
    pub kind: &'static str,  // always "init"
    pub canvas: OffscreenCanvasWrapper,
    pub width: u32,
    pub height: u32,
}

/// Main-thread → worker render message (used by the frame loop).
#[derive(serde::Serialize)]
pub struct RenderMessage {
    pub kind: &'static str,  // always "render"
    pub graph: alkalive_render::RenderGraph,
    pub compiled: alkalive_render::CompiledGraph,
    pub time: f32,
}

/// Main-thread → worker resize message.
#[derive(serde::Serialize)]
pub struct ResizeMessage {
    pub kind: &'static str,  // always "resize"
    pub width: u32,
    pub height: u32,
}
```

### Worker state

```rust
struct RenderWorkerState {
    renderer: Option<alkalive_backend_wgpu::WgpuRenderer>,
    canvas: Option<web_sys::OffscreenCanvas>,
}

thread_local! {
    static STATE: std::cell::RefCell<RenderWorkerState> =
        std::cell::RefCell::new(RenderWorkerState {
            renderer: None,
            canvas: None,
        });
}
```

### `RenderWorkerHandle` (main-thread side)

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs

/// Handle to the render worker. Owns the `web_sys::Worker` and a buffered
/// "render" message (in case "ready" hasn't arrived yet — Q8.6, REND-813).
pub struct RenderWorkerHandle {
    worker: web_sys::Worker,
    ready: std::cell::Cell<bool>,
    /// Buffered render message; sent when `ready` becomes true.
    /// `None` when no buffer is needed.
    pending_render: std::cell::RefCell<Option<JsValue>>,
}
```

---

## 3.3 Interfaces and contracts

### `init_worker` (REND-803, REND-804)

```rust
// In crates/alkalive-render-worker/src/lib.rs

#[wasm_bindgen]
pub fn init_worker() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(
            &format!("AlkALive render-worker panic: {}", info).into(),
        );
    }));
    install_message_handler();
    // Signal to the main thread that the worker is ready (REND-804).
    let ready_msg = WorkerToMainMessage::Ready;
    let js = serde_wasm_bindgen::to_value(&ready_msg)
        .expect("WorkerToMainMessage::Ready serialises");
    js_sys::global()
        .dyn_into::<web_sys::DedicatedWorkerGlobalScope>()
        .expect("global is DedicatedWorkerGlobalScope")
        .post_message(&js)
        .expect("post_message failed");
}

fn install_message_handler() {
    let handler = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
        |e: web_sys::MessageEvent| {
            let data = e.data();
            let msg: WorkerMessage = match serde_wasm_bindgen::from_value(data) {
                Ok(m) => m,
                Err(e) => {
                    web_sys::console::error_1(
                        &format!(
                            "AlkALive render-worker: failed to deserialize message: {:?}",
                            e
                        )
                        .into(),
                    );
                    return;
                }
            };
            match msg.kind {
                WorkerMessageKind::Init { canvas, width, height } => {
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = handle_init(canvas.0, width, height).await {
                            web_sys::console::error_1(
                                &format!("AlkALive render-worker init failed: {}", e).into(),
                            );
                            // Post an Error message so the main thread can fall back.
                            let err_msg = WorkerToMainMessage::Error { message: e };
                            if let Ok(js) = serde_wasm_bindgen::to_value(&err_msg) {
                                let _ = js_sys::global()
                                    .dyn_into::<web_sys::DedicatedWorkerGlobalScope>()
                                    .map(|scope| scope.post_message(&js));
                            }
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
        },
    );

    let scope = js_sys::global();
    let target: &web_sys::EventTarget = scope.as_ref();
    target
        .add_event_listener_with_callback("message", handler.as_ref().unchecked_ref())
        .expect("add_event_listener failed");
    handler.forget();
}

async fn handle_init(
    canvas: web_sys::OffscreenCanvas,
    width: u32,
    height: u32,
) -> Result<(), String> {
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

### `init_from_offscreen` (REND-811)

```rust
// In crates/alkalive-backend-wgpu/src/lib.rs (added by Gap 7, used by Gap 8)

impl WgpuRenderer {
    pub async fn init_from_offscreen(
        canvas: web_sys::OffscreenCanvas,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // Identical body to init_from_canvas, but the `canvas` argument
        // is `web_sys::OffscreenCanvas` and `wgpu::SurfaceTarget::Canvas`
        // accepts it via the `From<OffscreenCanvas>` impl in wgpu v23.
        let instance = wgpu::Instance::new(/* ... */);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| format!("surface creation: {:?}", e))?;
        // ... rest identical to init_from_canvas ...
    }
}
```

### `spawn_render_worker` (REND-815)

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs

fn spawn_render_worker(
    canvas: &web_sys::HtmlCanvasElement,
    width: u32,
    height: u32,
) -> Result<RenderWorkerHandle, JsValue> {
    // 1. Transfer canvas control to OffscreenCanvas.
    let offscreen: web_sys::OffscreenCanvas =
        canvas.transfer_control_to_offscreen()?;

    // 2. Spawn the worker.
    let worker = web_sys::Worker::new("/alkalive/render_worker.js")?;

    // 3. Serialise the init message.
    let init_msg = WorkerInitMessage {
        kind: "init",
        canvas: OffscreenCanvasWrapper(offscreen.clone()),
        width,
        height,
    };
    let js = serde_wasm_bindgen::to_value(&init_msg)?;

    // 4. Post with transfer (the OffscreenCanvas is transferred, not copied).
    worker.post_message_with_transfer(
        &js,
        &[offscreen.as_ref()],
    )?;

    Ok(RenderWorkerHandle {
        worker,
        ready: std::cell::Cell::new(false),
        pending_render: std::cell::RefCell::new(None),
    })
}
```

### `should_use_render_worker` (REND-818, REND-819 — CR-19)

```rust
// In crates/alkalive-runtime-wasm/src/lib.rs

fn should_use_render_worker(canvas: &web_sys::HtmlCanvasElement) -> bool {
    // 1. Worker must be available (universal in evergreen browsers).
    let worker_available = web_sys::window()
        .and_then(|w| w.get("Worker").ok())
        .is_some();
    if !worker_available { return false; }

    // 2. OffscreenCanvas + transferControlToOffscreen must be available
    //    (Chrome 69+, Firefox 105+, Safari 16.4+).
    let transfer_available = js_sys::Reflect::has(
        &canvas.into(),
        &"transferControlToOffscreen".into(),
    ).unwrap_or(false);
    if !transfer_available { return false; }

    // CR-19: do NOT check is_cross_origin_isolated() for the first cut.
    // postMessage works without COOP/COEP. The SAB path (future) will
    // re-add this check.
    true
}
```

### Runtime init branch (REND-816, REND-817, REND-821)

```rust
async fn init_runtime(
    canvas: web_sys::HtmlCanvasElement,
    ime_input: web_sys::HtmlInputElement,
    width: u32,
    height: u32,
    scene: alkalive_scene_data::TextSceneData,
    schedule: alkalive_compiler::ScheduleIR,
    dep_graph: alkalive_compiler::DependencyGraph,
    is_small_scene: bool,
) -> Result<(), JsValue> {
    // ... existing scene/schedule/dep_graph/signals setup ...

    // Lower the graph + compile (Gap 6 — same as before).
    let graph = alkalive_render::schedule_to_render_graph(
        &scheduled, &scene, (width, height),
    );
    let compiled = alkalive_render::compile(
        std::slice::from_ref(&graph), &[],
        &alkalive_render::DepthBuffer::default(),
    ).map_err(|e| JsValue::from_str(&format!("graph compile: {:?}", e)))?;

    // NEW (Gap 8): decide worker vs fallback.
    let (worker, renderer) = if should_use_render_worker(&canvas) {
        match spawn_render_worker(&canvas, width, height) {
            Ok(handle) => (Some(handle), None),
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("AlkALive: spawn_render_worker failed: {:?}; using fallback", e).into(),
                );
                let r = alkalive_backend_wgpu::WgpuRenderer::init_from_canvas(
                    canvas.clone(), width, height,
                ).await?;
                (None, Some(r))
            }
        }
    } else {
        web_sys::console::warn_1(
            &"AlkALive: render worker unavailable; using single-threaded fallback".into(),
        );
        let r = alkalive_backend_wgpu::WgpuRenderer::init_from_canvas(
            canvas.clone(), width, height,
        ).await?;
        (None, Some(r))
    };

    // Set a 5-second timeout (REND-828). If the worker hasn't posted "ready"
    // by then, terminate it and fall back.
    if let Some(handle) = &worker {
        install_worker_ready_timeout(handle, 5_000);
    }

    let runtime = Runtime {
        worker,
        renderer,
        // ... rest of fields, including graph + compiled from Gap 6 ...
    };
    // ... store in thread_local ...
    Ok(())
}

fn install_worker_ready_timeout(handle: &RenderWorkerHandle, timeout_ms: u32) {
    let worker = handle.worker.clone();
    let closure = Closure::once(move || {
        if !handle.ready.get() {
            web_sys::console::error_1(
                &format!("AlkALive: render worker init timeout ({}ms)", timeout_ms).into(),
            );
            worker.terminate();
            // Fall back to single-threaded (the next frame's branch on
            // `runtime.worker.is_some()` will be false after terminate).
            // NOTE: in practice, the runtime must replace `worker: Some(handle)`
            // with `worker: None` here. This requires a re-borrow of the
            // RUNTIME thread_local; the implementation handles this via
            // a `Runtime::on_worker_timeout()` method.
        }
    });
    web_sys::window()
        .and_then(|w| w.set_timeout_with_callback_and_timeout_and_arguments_0(
            closure.as_ref().unchecked_ref(), timeout_ms as i32,
        ).ok())
        .expect("set_timeout failed");
    closure.forget();
}
```

### Frame loop branch (REND-821)

```rust
fn start_frame_loop() {
    let frame_closure = Closure::new(|| {
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                runtime.time = elapsed_seconds();
                runtime.signals.set(
                    alkalive_compiler::SignalId(1),
                    signal_store::SignalValue::Float(runtime.time),
                );

                // Re-lower + re-compile if structure changed (Gap 6, REND-624).
                // ... (existing logic) ...

                // Branch on worker vs fallback (Gap 8, REND-821).
                if let Some(handle) = &runtime.worker {
                    // Multi-threaded path: postMessage "render".
                    let msg = RenderMessage {
                        kind: "render",
                        graph: runtime.graph.clone(),
                        compiled: runtime.compiled.clone(),
                        time: runtime.time,
                    };
                    match serde_wasm_bindgen::to_value(&msg) {
                        Ok(js) => {
                            if handle.ready.get() {
                                let _ = handle.worker.post_message(&js);
                            } else {
                                // Buffer the latest render (REND-813, Q8.6).
                                *handle.pending_render.borrow_mut() = Some(js);
                            }
                        }
                        Err(e) => {
                            web_sys::console::error_1(
                                &format!("AlkALive: render message serialize failed: {:?}", e).into(),
                            );
                        }
                    }
                } else if let Some(renderer) = runtime.renderer.as_mut() {
                    // Single-threaded fallback.
                    renderer.render_compiled(
                        &runtime.graph, &runtime.compiled, runtime.time,
                    );
                }
            }
        });
        schedule_next_frame();
    });
    // ... store closure, kick off first frame ...
}
```

### Worker "ready" handler (REND-813)

```rust
// In init_runtime, after spawn_render_worker returns:
if let Some(handle) = &runtime.worker {
    install_worker_message_handler(handle);
}

fn install_worker_message_handler(handle: &RenderWorkerHandle) {
    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
        |e: web_sys::MessageEvent| {
            let data = e.data();
            let msg: WorkerToMainMessage = match serde_wasm_bindgen::from_value(data) {
                Ok(m) => m,
                Err(_) => return,
            };
            match msg {
                WorkerToMainMessage::Ready => {
                    handle.ready.set(true);
                    // Flush the buffered render message (REND-813).
                    if let Some(pending) = handle.pending_render.borrow_mut().take() {
                        let _ = handle.worker.post_message(&pending);
                    }
                    // Hide the loading indicator (REND-827).
                    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                        if let Some(el) = doc.get_element_by_id("loading") {
                            el.set_attribute("style", "display:none").ok();
                        }
                    }
                }
                WorkerToMainMessage::Error { message } => {
                    web_sys::console::error_1(
                        &format!("AlkALive render-worker error: {}", message).into(),
                    );
                    // Fall back to single-threaded.
                    RUNTIME.with(|rt| {
                        if let Some(runtime) = rt.borrow_mut().as_mut() {
                            if let Some(handle) = runtime.worker.take() {
                                handle.worker.terminate();
                            }
                            // The next frame's branch will use `renderer: None`,
                            // so we must re-create the renderer on the main thread.
                            // For simplicity, we reload the page.
                            web_sys::console::warn_1(
                                &"AlkALive: falling back; please reload".into(),
                            );
                        }
                    });
                }
            }
        },
    );
    handle.worker.add_event_listener_with_callback(
        "message", on_message.as_ref().unchecked_ref(),
    ).expect("add_event_listener failed");
    on_message.forget();
}
```

### Resize handler branch (REND-822)

```rust
fn setup_resize_listener(runtime: &Runtime) -> Result<(), JsValue> {
    let on_resize = Closure::<dyn FnMut()>::new(|| {
        RUNTIME.with(|rt| {
            if let Some(runtime) = rt.borrow_mut().as_mut() {
                let dpr = web_sys::window()
                    .and_then(|w| Some(w.device_pixel_ratio()))
                    .unwrap_or(1.0) as f32;
                let canvas = /* get canvas from runtime */;
                let css_width = canvas.client_width().max(1) as f32;
                let css_height = canvas.client_height().max(1) as f32;
                let width = (css_width * dpr).max(1.0) as u32;
                let height = (css_height * dpr).max(1.0) as u32;
                runtime.renderer_resize(width, height);  // branches on worker vs fallback
            }
        });
    });
    // ...
}

impl Runtime {
    fn renderer_resize(&mut self, width: u32, height: u32) {
        if let Some(handle) = &self.worker {
            // Multi-threaded: postMessage "resize".
            let msg = ResizeMessage { kind: "resize", width, height };
            if let Ok(js) = serde_wasm_bindgen::to_value(&msg) {
                let _ = handle.worker.post_message(&js);
            }
        } else if let Some(renderer) = self.renderer.as_mut() {
            // Fallback: direct call.
            renderer.resize(width, height);
        }
    }
}
```

---

## 3.4 State transitions

### Worker lifecycle

```
┌──────────────────────────────────────────────────────────────────┐
│ Main thread                                                      │
│                                                                  │
│  init_runtime                                                    │
│  └─ should_use_render_worker(canvas)?                            │
│      ├─ false → single-threaded fallback (existing path)         │
│      └─ true  → spawn_render_worker(canvas)                      │
│                 ├─ canvas.transferControlToOffscreen()           │
│                 ├─ Worker::new("/alkalive/render_worker.js")     │
│                 ├─ post_message_with_transfer(init_msg, [offscr])│
│                 └─ install_worker_message_handler()              │
│                                                                  │
│  install_worker_ready_timeout(5_000ms)                           │
│                                                                  │
│  Frame loop (REND-821):                                          │
│    if worker.ready: post_message(render_msg)                     │
│    else:           buffer pending_render                         │
└────────────────────────────┬─────────────────────────────────────┘
                             │
                             │ postMessage({ type: "init", ... })
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│ Render worker                                                    │
│                                                                  │
│  init_worker()                                                   │
│  ├─ install panic hook                                           │
│  ├─ install message handler                                      │
│  └─ post_message({ type: "ready" })  ←─── (REND-804)             │
│                                                                  │
│  on message:                                                     │
│    match kind:                                                   │
│      Init { canvas, w, h } → handle_init(canvas, w, h).await     │
│        ├─ WgpuRenderer::init_from_offscreen(canvas, w, h).await  │
│        ├─ Ok → STATE.renderer = Some(r); log "ready"             │
│        └─ Err → log error; post_message({ type: "error", ... })  │
│      Render { graph, compiled, time } → handle_render(...)       │
│        └─ if renderer.is_some(): render_compiled(...)            │
│           else: warn "render before init"; drop                  │
│      Resize { w, h } → handle_resize(w, h)                       │
│        └─ renderer.resize(w, h)                                  │
└──────────────────────────────────────────────────────────────────┘
```

### Main-thread worker-state machine

| From | To | Trigger |
|------|-----|---------|
| `Init` | `WorkerSpawned` | `spawn_render_worker` returns `Ok(handle)` |
| `WorkerSpawned` | `WorkerReady` | Worker posts `{ type: "ready" }` |
| `WorkerSpawned` | `WorkerTimeout` | 5 s elapse without "ready" |
| `WorkerTimeout` | `Fallback` | `worker.terminate()`; reload page (or re-create renderer on main thread) |
| `WorkerReady` | `Rendering` | First frame's `post_message(render_msg)` |
| `Rendering` | `Rendering` | Subsequent frames |
| `Rendering` | `WorkerError` | Worker posts `{ type: "error", message }` |
| `WorkerError` | `Fallback` | `worker.terminate()`; reload page |
| `Init` | `Fallback` | `should_use_render_worker` returns false OR `spawn_render_worker` returns Err |

---

## 3.5 Error cases

| ID | Error class | Source | Message format | Handling |
|----|-------------|--------|----------------|----------|
| REND-8-E1 | `Worker::new` failure | Browser blocks worker creation (CSP, file:// protocol) | `"AlkALive: spawn_render_worker failed: <e>; using fallback"` | Log via `console::warn_1`; fall back to single-threaded. |
| REND-8-E2 | `transferControlToOffscreen` failure | Canvas already transferred, or not a canvas | `"AlkALive: transferControlToOffscreen failed: <e>; using fallback"` | Same. |
| REND-8-E3 | Worker init timeout | 5 s elapse without "ready" | `"AlkALive: render worker init timeout (5000ms)"` | Log via `console::error_1`; terminate the worker; reload page (or re-create renderer on main thread). |
| REND-8-E4 | Worker panic | Uncaught panic in worker WASM | `"AlkALive render-worker panic: <info>"` | The worker's panic hook logs to console; the main thread detects a missing "ready" message and falls back. |
| REND-8-E5 | `wgpu::SurfaceError::Lost` (in worker) | Surface lost in worker | `"AlkALive: surface lost; reconfiguring"` | Worker reconfigures; if reconfiguration fails, worker posts `{ type: "error", message: "..." }`; main thread falls back. |
| REND-8-E6 | Worker posts `Error` | Worker encountered an unrecoverable error | `"AlkALive render-worker error: <message>"` | Log via `console::error_1`; terminate the worker; reload page. |
| REND-8-E7 | Deserialise failure | `serde_wasm_bindgen::from_value` returns Err | `"AlkALive render-worker: failed to deserialize message: <e>"` | Log via `console::error_1`; return (no panic). |
| REND-8-E8 | Render before init | `handle_render` called before `handle_init` completes | `"AlkALive render-worker: render before init"` | Log via `console::warn_1`; drop the message. |
| REND-8-E9 | Render message serialise failure | `serde_wasm_bindgen::to_value(&render_msg)` returns Err | `"AlkALive: render message serialize failed: <e>"` | Log via `console::error_1`; skip the frame. |
| REND-8-E10 | `should_use_render_worker` returns false | Worker or OffscreenCanvas unavailable | `"AlkALive: render worker unavailable; using single-threaded fallback"` | Log via `console::warn_1`; use the single-threaded path. |

---

## 3.6 Validation rules

1. **V-8-1.** The worker's `init_worker` MUST be called exactly once when
   the worker WASM loads. (Verified by the worker posting exactly one
   `{ type: "ready" }` message.)
2. **V-8-2.** The worker's `OffscreenCanvas` MUST be transferred (not
   copied) via `post_message_with_transfer`. The main thread MUST NOT
   retain a reference to the `OffscreenCanvas` after transfer.
3. **V-8-3.** The `Render` message MUST carry a serialised `RenderGraph`
   that round-trips through `serde_wasm_bindgen::to_value` →
   `from_value` losslessly. (Verified by T-8-5; depends on REND-601 from
   Gap 6.)
4. **V-8-4.** When `should_use_render_worker` returns false, the runtime
   MUST NOT call `spawn_render_worker`. (Verified by T-8-1, T-8-2,
   T-8-3.)
5. **V-8-5.** The worker's `handle_render` MUST NOT be called before
   `handle_init` completes (REND-812). If it is, the worker logs REND-8-E8
   and drops the message.
6. **V-8-6.** The main thread MUST buffer at most 1 pending `Render`
   message before the worker posts "ready" (REND-813). On overflow, the
   oldest message is dropped with a warning.
7. **V-8-7.** The worker MUST post `{ type: "ready" }` exactly once, after
   `init_worker` returns (REND-804). The main thread MUST NOT send
   `Render` messages before "ready" arrives (it buffers them).
8. **V-8-8.** The COOP/COEP headers (REND-824) MUST be present on every
   response served by Caddy in dev. (Verified by T-8-9 via `curl -I`.)
9. **V-8-9.** When `should_use_render_worker` returns false, the runtime
   MUST NOT require COOP/COEP headers (CR-19). The single-threaded
   fallback works without them.
10. **V-8-10.** The worker init timeout (REND-828) is 5 s. If the worker
    posts "ready" within 5 s, the timeout is cancelled (the closure
    checks `handle.ready.get()` and is a no-op if true).

---

## 3.7 Performance requirements

| ID | Requirement | Measurement |
|----|-------------|-------------|
| REND-8-P1 | Worker spawn + WASM load + device init MUST complete in **< 500 ms** on the M1 Air baseline (Chrome 113+). | Manual measurement via `performance.now()` deltas from `spawn_render_worker` to "ready". |
| REND-8-P2 | The `postMessage` serialise + deserialise round-trip for the Hello World `RenderGraph` MUST complete in **< 100 µs** (REND-6-P7 plus the `serde_wasm_bindgen` overhead). | `criterion` benchmark `bench_render_graph_postmessage_round_trip`. |
| REND-8-P3 | `render_compiled` on the worker for the Hello World scene MUST complete in **< 4 ms** (same as the single-threaded path — REND-7-P3). | Manual measurement on the worker (logged via `console::log_1` every 60th frame). |
| REND-8-P4 | The worker WASM binary size MUST NOT exceed **2.5 MB** post-build (`wasm-opt -Oz`). Pre-Gap-8 baseline: 1.8 MB (post-Gap-7). Allowed growth: ~700 KB (the worker includes its own copy of `wgpu`, `alkalive-text`, etc.). | `ls -l deploy/alkalive/render_worker_bg.wasm` after `wasm-pack build --release` for the worker crate. |
| REND-8-P5 | The worker WASM is loaded only when `should_use_render_worker()` returns true. On the fallback path, the worker WASM is NOT fetched. | Browser DevTools Network tab: no request for `render_worker.js` / `render_worker_bg.wasm` on the fallback path. |
| REND-8-P6 | The main-thread frame loop MUST NOT block on the worker. `post_message` returns immediately; the next `requestAnimationFrame` is scheduled without waiting for the worker to finish rendering. | Manual measurement: the RAF callback's total time (excluding `post_message`) is < 1 ms. |

---

## 3.8 Browser/platform integration

### Exact COOP/COEP header configuration

#### `Caddyfile` (REND-823 — added to repo root)

This is the canonical dev server. It serves `deploy/` on `:8080` with
COOP/COEP headers on every response.

```caddy
# Caddyfile — AlkALive dev server
# Run with: caddy run --config Caddyfile
# Serves deploy/ on http://localhost:8080

:8080 {
	root * deploy
	file_server

	header {
		# ADR-003 line 99: exact COOP/COEP values.
		Cross-Origin-Opener-Policy "same-origin"
		Cross-Origin-Embedder-Policy "require-corp"

		# WASM streaming compile (ADR-017): correct MIME type.
		# Caddy's file_server already sets application/wasm for .wasm files;
		# this is documented for clarity.
		# Content-Type "application/wasm" for *.wasm  # (built-in)

		# Cache WASM aggressively (with revalidation) for repeat visits.
		# Cache-Control "public, max-age=3600, must-revalidate"
	}

	# MIME types: Caddy's defaults include .wasm → application/wasm.
	# No custom mime block needed.

	# Logging for debugging COOP/COEP.
	log {
		output stdout
		format console
	}
}
```

#### `deploy/index.html` (REND-827 — updated)

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>AlkALive Hello World</title>
  <style>
    body{margin:0;overflow:hidden}
    canvas{display:block;width:100vw;height:100vh}
    #ime{position:absolute;left:-9999px;opacity:0}
    #loading{
      position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);
      font-family:system-ui,sans-serif;color:#aaa;font-size:18px;
    }
  </style>
</head>
<body>
  <canvas id="canvas"></canvas>
  <input id="ime" type="text">
  <div id="loading">Loading AlkALive…</div>
  <script type="module">
    import init from './pkg/alkalive_runtime_wasm.js';
    const wasm = await init('./pkg/alkalive_runtime_wasm_bg.wasm');
    const canvas = document.getElementById('canvas');
    const ime = document.getElementById('ime');
    await wasm.start(canvas, ime);
  </script>
</body>
</html>
```

The `#loading` div is hidden by the runtime after the worker posts "ready"
(or after `init_runtime` completes in the fallback path).

#### Portable header documentation (for non-Caddy deployments)

If AlkALive is deployed via a different static server, the following
response headers MUST be set on every response (HTML, JS, WASM, fonts):

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

**Examples for other servers:**

- **nginx:**
  ```nginx
  location / {
    add_header Cross-Origin-Opener-Policy "same-origin" always;
    add_header Cross-Origin-Embedder-Policy "require-corp" always;
    add_header Content-Type application/wasm always;
  }
  ```
- **`npx serve`:** `npx serve` does not set custom headers easily; use the
  `serve.json` config:
  ```json
  {
    "headers": [
      {
        "source": "**/*",
        "headers": [
          { "key": "Cross-Origin-Opener-Policy", "value": "same-origin" },
          { "key": "Cross-Origin-Embedder-Policy", "value": "require-corp" }
        ]
      }
    ]
  }
  ```
- **GitHub Pages:** does not support custom headers; the worker path is
  unavailable. The runtime falls back to single-threaded (REND-810 /
  `should_use_render_worker` returns false because `crossOriginIsolated`
  is false — though per CR-19 this check is removed for the first cut,
  GitHub Pages also lacks `transferControlToOffscreen` support in some
  configurations).

**`credentialless` alternative (Chrome 96+, Firefox 110+):**

If AlkALive is embedded in a third-party iframe and `require-corp` blocks
cross-origin resources, replace the COEP header with:

```
Cross-Origin-Embedder-Policy: credentialless
```

This loads cross-origin resources without explicit CORP headers, at the
cost of loading them without credentials. The Caddyfile documents this as
a commented-out alternative (not enabled by default).

### Worker JS shim (REND-820)

`crates/alkalive-render-worker/src/worker.js`:

```js
// AlkALive render worker — JS shim.
// Loads the worker WASM and calls init_worker().

import init, { init_worker } from './alkalive_render_worker.js';

// The worker's WASM is loaded relative to this JS file (same directory).
await init('./alkalive_render_worker_bg.wasm');

// Install the message handler. init_worker posts { type: "ready" } back.
init_worker();
```

### Worker deployment layout

After `wasm-pack build --release` for both crates, the deployed layout is:

```
deploy/
├── index.html
├── pkg/
│   ├── alkalive_runtime_wasm.js
│   ├── alkalive_runtime_wasm_bg.wasm
│   └── ...
└── alkalive/
    ├── render_worker.js          ← copied from crates/alkalive-render-worker/src/worker.js
    ├── alkalive_render_worker.js ← wasm-pack output
    └── alkalive_render_worker_bg.wasm
```

The `spawn_render_worker` function references `"/alkalive/render_worker.js"`
(an absolute path). The Caddyfile serves `deploy/` as root, so
`/alkalive/render_worker.js` resolves to `deploy/alkalive/render_worker.js`.

### Build commands

```bash
# Build the runtime WASM (main thread).
wasm-pack build --release --target web \
  crates/alkalive-runtime-wasm \
  --out-dir deploy/pkg

# Build the render worker WASM.
wasm-pack build --release --target web \
  crates/alkalive-render-worker \
  --out-dir deploy/alkalive

# Copy the worker JS shim.
cp crates/alkalive-render-worker/src/worker.js deploy/alkalive/render_worker.js

# Run the dev server.
caddy run --config Caddyfile
```

### Cross-origin iframe embedding

ADR-003's COEP risk (line 106) is real: if AlkALive is embedded in a
third-party iframe, the iframe's COEP header may conflict with the
parent's. **Mitigation**: use `Cross-Origin-Embedder-Policy:
credentialless` (documented above). If `credentialless` is unavailable
(Safari < 16.4), fall back to single-threaded (REND-810).

---

## 3.9 Test cases

| ID | Test | Expected behaviour |
|----|------|---------------------|
| T-8-1 [`alkalive-runtime-wasm`] (native, mocked) | `should_use_render_worker` returns false when `Worker` is undefined | Mock `window.Worker = undefined`; assert `should_use_render_worker(canvas) == false`. |
| T-8-2 [`alkalive-runtime-wasm`] (native, mocked) | `should_use_render_worker` returns false when `transferControlToOffscreen` is missing | Mock `canvas` without `transferControlToOffscreen`; assert `false`. |
| T-8-3 [`alkalive-runtime-wasm`] (native, mocked) | `should_use_render_worker` returns true when both are present | Mock both; assert `true`. |
| T-8-4 [`alkalive-runtime-wasm`] (native, mocked) | `should_use_render_worker` does NOT check `crossOriginIsolated` (CR-19) | Mock `crossOriginIsolated = false` but `Worker` and `transferControlToOffscreen` present; assert `true`. |
| T-8-5 [`alkalive-render-worker`] (native) | `WorkerMessage` round-trips through serde_wasm_bindgen | Construct a `RenderMessage { kind: "render", graph, compiled, time: 1.0 }`; serialise; deserialise as `WorkerMessage`; assert `matches!(msg.kind, WorkerMessageKind::Render { .. })`. |
| T-8-6 [`alkalive-render-worker`] (native) | `handle_render` does not panic on a valid `RenderGraph` | Construct a `RenderGraph` via `schedule_to_render_graph`; call `handle_render(graph, compiled, 0.0)`; assert no panic. |
| T-8-7 [`alkalive-runtime-wasm`] (wasm32, headless browser with COOP/COEP) | The runtime spawns the worker, the worker posts "ready" within 5 s | After `start(canvas, ime)`, wait for the worker's "ready" message; assert it arrives within 5 s. |
| T-8-8 [`alkalive-runtime-wasm`] (wasm32, headless browser with COOP/COEP) | The frame loop sends "render" messages; the worker draws to the OffscreenCanvas | After 5 frames, read back a pixel via `gl.readPixels` on the worker's surface (or via `OffscreenCanvas.transferToImageBitmap`); assert non-zero pixel content. |
| T-8-9 (browser, manual) | COOP/COEP headers are present on every response | `curl -I http://localhost:8080/` shows `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. |
| T-8-10 [`alkalive-runtime-wasm`] (wasm32, headless browser without COOP/COEP) | `should_use_render_worker` returns true (CR-19); the worker path runs | After `start`, the worker spawns and posts "ready"; the canvas renders. (This tests that the first cut does NOT gate on COOP/COEP.) |
| T-8-11 [`alkalive-runtime-wasm`] (wasm32, headless browser) | Fallback path runs when `Worker` is undefined | Mock `window.Worker = undefined`; after `start`, the single-threaded path runs; the canvas renders. |
| T-8-12 [`alkalive-runtime-wasm`] (wasm32) | Worker init timeout fires after 5 s | Mock the worker to never post "ready"; after 5 s, the runtime logs REND-8-E3 and terminates the worker. |
| T-8-13 [`alkalive-runtime-wasm`] (wasm32) | Worker "render before init" warning (REND-8-E8) | Send a "render" message before "ready" arrives; the worker logs the warning and drops the message. |
| T-8-14 [`alkalive-runtime-wasm`] (wasm32) | Render message buffer holds 1 pending message (REND-813) | Send 2 "render" messages before "ready"; the first is buffered, the second is dropped with a warning; when "ready" arrives, the buffered first message is sent. |
| T-8-15 (browser verification, manual) | Visual parity: the Hello World canvas renders identically in worker and fallback paths | Screenshot both paths; pixel diff < 1%. |
| T-8-16 [`alkalive-runtime-wasm`] (wasm32) | Resize event reaches the worker | Trigger a resize; the worker's `handle_resize` is called; the surface is reconfigured. |
| T-8-17 [`alkalive-render-worker`] (bench) | `bench_render_graph_postmessage_round_trip` completes in < 100 µs (REND-8-P2) | `criterion` benchmark. |
| T-8-18 (regression) | All 1148 existing tests pass | `cargo test --workspace` succeeds. |
| T-8-19 (regression) | The single-threaded fallback preserves today's behavior exactly | Run the Hello World demo with `should_use_render_worker` mocked to return false; assert the canvas renders correctly. |
| T-8-20 (build check) | Worker WASM binary size ≤ 2.5 MB (REND-8-P4) | `ls -l deploy/alkalive/alkalive_render_worker_bg.wasm` after build. |
| T-8-21 (build check) | The worker WASM is NOT fetched on the fallback path (REND-8-P5) | DevTools Network tab: no request for `render_worker.js` when `should_use_render_worker` returns false. |

---

## 3.10 Acceptance criteria

1. `cargo test -p alkalive-runtime-wasm` passes `T-8-1` through `T-8-4` on
   native (mocked).
2. `cargo test -p alkalive-render-worker` passes `T-8-5`, `T-8-6`.
3. `cargo test -p alkalive-runtime-wasm --target wasm32-unknown-unknown`
   passes `T-8-7`, `T-8-8`, `T-8-11`, `T-8-12`, `T-8-13`, `T-8-14`,
   `T-8-16` in headless browsers.
4. `cargo bench -p alkalive-render-worker` reports `T-8-17` mean < 100 µs.
5. The Hello World demo (served via `caddy run --config Caddyfile`)
   renders with visual parity in both worker and fallback paths
   (T-8-15, T-8-19).
6. `curl -I http://localhost:8080/` shows the COOP/COEP headers (T-8-9).
7. `ls -l deploy/alkalive/alkalive_render_worker_bg.wasm` shows ≤ 2.5 MB
   (T-8-20).
8. `grep -r "next.config.ts" docs/` returns no matches (CR-12 — the
   non-existent file is not referenced).
9. `grep -r "is_cross_origin_isolated" crates/alkalive-runtime-wasm/`
   returns no matches (CR-19 — the check is removed for the first cut).

---

## 3.11 Traceability

| Requirement | ADR / source | Fine-draft § | Implementation § | Test ID | CR addressed |
|-------------|--------------|--------------|------------------|---------|--------------|
| REND-801 (alkalive-render-worker crate) | ADR-003 line 99; ADR-021 line 685 | §8.5.2 | §3.1, §3.2 | (build check) | — |
| REND-806 (WorkerMessage serde) | ADR-003 line 99 ("immutable render-graph IR") | §8.5.2 line 2222-2243 | §3.2 | T-8-5 | CR-1 |
| REND-808 (OffscreenCanvasWrapper) | (implementation detail) | §8.5.2 | §3.2 | T-8-5 | CR-1 |
| REND-811 (init_from_offscreen) | ADR-003 line 99; ADR-021 line 685 | §8.5.2 | §3.3 | T-8-7 | — |
| REND-812 (handle_render before init) | (Q8.6) | §8.12 Q8.6 | §3.3 | T-8-13 | — |
| REND-813 (buffer N=1 pending render) | (Q8.6) | §8.12 Q8.6 | §3.3 | T-8-14 | — |
| REND-815 (spawn_render_worker) | ADR-003 line 99 | §8.5.3 | §3.3 | T-8-7 | — |
| REND-818 (should_use_render_worker checks) | ADR-003 line 99 | §8.5.4 line 2396-2417 | §3.3 | T-8-1 through T-8-4 | CR-19 |
| REND-819 (no crossOriginIsolated check) | (critical review) | §8.5.4, §8.7 line 2524-2528 | §3.3 | T-8-4, T-8-10 | CR-19 |
| REND-821 (frame loop branch) | ADR-003 line 99; ADR-021 line 685 | §8.5.4 | §3.3 | T-8-8 | — |
| REND-823 (Caddyfile added) | (critical review) | §8.5.4 line 2348-2379 | §3.8 | T-8-9 | CR-12 |
| REND-824 (exact COOP/COEP values) | ADR-003 line 99 | §8.5.4 | §3.8 | T-8-9 | — |
| REND-825 (no next.config.ts) | (critical review) | §8.5.4 | §3.8 | (grep check) | CR-12 |
| REND-827 (loading indicator) | (UX) | §8.5.4 | §3.8 | T-8-7 | — |
| REND-828 (5 s init timeout) | ADR-003 line 99 | §8.8 | §3.3 | T-8-12 | — |
| REND-8-P1 (worker init < 500 ms) | ADR-017 line 600 | §8.11 R8.2 | §3.7 | (manual) | CR-25 |
| REND-8-P2 (postMessage < 100 µs) | ADR-003 line 99 | §8.6 point 7 | §3.7 | T-8-17 | — |
| REND-8-P4 (worker WASM ≤ 2.5 MB) | ADR-017 line 600 | §8.11 R8.4 | §3.7 | T-8-20 | CR-25 |

---

# §4 Consolidated Traceability Matrix

The matrix below maps every requirement in this specification to its ADR
source, the fine-draft section that inspired it, the implementation
section that realises it, the test ID that verifies it, and the
critical-review finding (if any) that it addresses.

| Req ID | ADR / source | Fine-draft § | Impl § | Test ID | CR |
|--------|--------------|--------------|--------|---------|----|
| **Gap 6 — Render-Graph IR** | | | | | |
| REND-601 | ADR-003 line 99 | §1.5.2, §8.5.2 | §1.2 | T-6-16, T-6-17 | CR-1 |
| REND-602 | ADR-003 | §8.5.2 | §1.8 | (build) | CR-1 |
| REND-603 | (impl detail) | §1.5.2 | §1.2 | T-6-16 | CR-1 |
| REND-604 | (impl detail) | §1.5.2 | §1.2 | T-6-16 | CR-1 |
| REND-605 | ADR-001 line 55 | §6.5.3, §6.5.6 | §1.2 | T-6-18 | CR-4 |
| REND-606 | (critical review) | §6.5.6 line 851-857 | §1.2 | (grep) | CR-4 |
| REND-607 | (critical review) | §6.5.6 line 861-865 | §1.2 | (grep) | CR-4 |
| REND-608 | ADR-001 line 55; ADR-006 line 165 | §6.5.3 | §1.2 | T-6-4..T-6-8 | — |
| REND-609 | (critical review) | §6.5.3 | §1.2 | (review) | CR-31 |
| REND-610 | (impl detail) | §6.5.3 | §1.2 | T-6-4 | — |
| REND-611 | (impl detail) | §6.5.3 | §1.2 | (build) | — |
| REND-612 | ADR-005 line 138 | §6.5.4 | §1.2 | (build) | — |
| REND-613 | ADR-005 line 138 | §6.5.4 | §1.2 | (build) | — |
| REND-614 | ADR-005 line 138 | §6.5.4 | §1.2 | (build) | — |
| REND-615 | ADR-005 line 138 | §6.5.4 | §1.2 | (build) | — |
| REND-616 | ADR-001 line 55; tech-spec §3.5 line 337 | §6.5.5 | §1.3 | T-6-1..T-6-3 | — |
| REND-617 | ADR-001 line 55 | §6.5.5 | §1.3 | T-6-1, T-6-3 | — |
| REND-618 | ADR-001 line 55; ADR-024 | §6.5.5 | §1.3 | T-6-4, T-6-5 | CR-30 |
| REND-619 | (acknowledged wart) | §6.5.5 line 643-657 | §1.3 | T-6-5 | CR-27 |
| REND-620 | (existing) | §6.5.6 | §1.3 | (build) | CR-21 |
| REND-621 | ADR-025 | §6.6 point 2 | §1.3 | T-6-13, T-6-14 | CR-6, CR-26 |
| REND-622 | ADR-025 | §6.6 point 2 | §1.2, §1.3 | T-6-13 | CR-6, CR-26 |
| REND-623 | (critical review) | §6.5.6 line 737-752 | §1.3 | (grep) | CR-13 |
| REND-624 | (critical review) | §6.5.8 line 915-919 | §1.3, §1.4 | T-6-24 | CR-13 |
| REND-625 | (critical review) | §6.11 R6.5 | §1.3 | (grep) | CR-6 |
| REND-626 | (critical review) | §6.5.6 line 787-790 | §1.3 | (review) | CR-4 |
| REND-627 | ADR-001 line 55 | §6.5.6 line 793-840 | §1.3 | T-6-20 | CR-5 (WebGL2 path; wgpu in §2) |
| REND-628 | ADR-001 line 55 | §6.5.8 | §1.3 | T-6-20 | — |
| REND-629 | ADR-025 | §6.5.8, §6.6 | §1.3 | T-6-20 | CR-6 |
| REND-6-P1..P8 | ADR-017 line 600 | §6.6 point 7 | §1.7 | T-6-21..T-6-24 | CR-13 |
| **Gap 7 — WGSL + wgpu** | | | | | |
| REND-701 | ADR-006 line 165; ADR-001 line 61 | §7.5.1 | §1.8 | (build) | — |
| REND-702 | (critical review) | §7.5.1 line 1346-1348 | §1.8 | (grep) | CR-29 |
| REND-703 | (Gap 8 prep) | §7.5.1 | §1.8 | (build) | — |
| REND-704 | (critical review) | §8-23 | §1.8 | (review) | — |
| REND-705 | ADR-006 line 165 | §7.5.2, §7.5.3 | §2.2 | T-7-1, T-7-2 | — |
| REND-706 | ADR-006 line 165 | §7.5.3 | §2.2 | (grep) | — |
| REND-707 | (impl detail) | §7.5.3 | §2.2 | (build) | — |
| REND-708 | (impl detail) | §7.5.3 | §2.2 | (review) | — |
| REND-709 | ADR-006 line 165 | §7.5.4 | §2.2 | (build) | — |
| REND-710 | ADR-006 line 165 | §7.5.4 | §2.2 | (build) | — |
| REND-711 | (impl detail) | §7.5.4 | §2.2 | (build) | — |
| REND-712 | ADR-006 line 165; ADR-017 line 600 | §7.5.4 | §2.3 | T-7-5, T-7-7, T-7-8 | — |
| REND-713 | ADR-001 line 55 | §7.5.4 | §2.3 | T-7-9 | — |
| REND-714 | (impl detail) | §7.5.4 | §2.3 | T-7-9 | — |
| REND-715 | ADR-001 line 55 | §7.5.4 line 1674 | §2.3 | T-7-11 | CR-5 |
| REND-716 | ADR-017 line 600 | §7.5.4 | §2.3 | T-7-6 | — |
| REND-717 | ADR-006 line 165 | §7.5.5 | §2.3 | (manual) | — |
| REND-718 | (Q7.4) | §7.12 | §2.3 | T-7-1..T-7-4 | — |
| REND-719 | ADR-022 line 700 | §6.6 point 3 | §2.3 | T-7-9 | — |
| REND-720 | ADR-022 line 700 | §6.6 point 6 | §2.3 | T-7-9 | — |
| REND-721 | (critical review) | §7.5.4 | §2.8 | (grep) | — |
| REND-722 | ADR-021 line 685 | §7.5.4 line 1130 | §2.3 | (review) | — |
| REND-723 | (impl detail) | §7.12 Q7.5 | §2.3 | (review) | — |
| REND-724 | (cross-gap contract) | §5.4 | §2.3 | T-7-9 | — |
| REND-7-P1..P6 | ADR-017 line 600 | §7.11 | §2.7 | T-7-12..T-7-14 | CR-25 |
| **Gap 8 — Single-GPU-Device + SAB/COOP-COEP** | | | | | |
| REND-801 | ADR-003 line 99; ADR-021 line 685 | §8.5.2 | §3.1, §3.2 | (build) | — |
| REND-802 | (impl detail) | §8.5.2 | §3.1 | (build) | — |
| REND-803 | ADR-003 line 99 | §8.5.2 | §3.3 | T-8-7 | — |
| REND-804 | ADR-003 line 99 | §8.5.2 | §3.3 | T-8-7 | — |
| REND-805 | (impl detail) | §8.5.2 | §3.3 | T-8-13 | — |
| REND-806 | ADR-003 line 99 | §8.5.2 line 2222-2243 | §3.2 | T-8-5 | CR-1 |
| REND-807 | (impl detail) | §8.5.2 | §3.2 | T-8-5 | — |
| REND-808 | (impl detail) | §8.5.2 | §3.2 | T-8-5 | CR-1 |
| REND-809 | ADR-003 line 99 | §8.5.2 | §3.2 | (build) | — |
| REND-810 | ADR-003 line 99 | §8.5.2 | §3.3 | T-8-7 | — |
| REND-811 | ADR-003 line 99; ADR-021 line 685 | §8.5.2 | §3.3 | T-8-7 | — |
| REND-812 | (Q8.6) | §8.12 Q8.6 | §3.3 | T-8-13 | — |
| REND-813 | (Q8.6) | §8.12 Q8.6 | §3.3 | T-8-14 | — |
| REND-814 | ADR-003 line 99 | §8.5.4 | §3.3 | T-8-16 | — |
| REND-815 | ADR-003 line 99 | §8.5.3 | §3.3 | T-8-7 | — |
| REND-816 | ADR-003 line 99 | §8.5.4 | §3.3 | T-8-8 | — |
| REND-817 | ADR-003 line 99 | §8.5.4 | §3.3 | T-8-11 | — |
| REND-818 | ADR-003 line 99 | §8.5.4 line 2396-2417 | §3.3 | T-8-1..T-8-4 | CR-19 |
| REND-819 | (critical review) | §8.5.4, §8.7 line 2524-2528 | §3.3 | T-8-4, T-8-10 | CR-19 |
| REND-820 | (impl detail) | §8.5.3 | §3.8 | (build) | — |
| REND-821 | ADR-003 line 99; ADR-021 line 685 | §8.5.4 | §3.3 | T-8-8 | — |
| REND-822 | ADR-003 line 99 | §8.5.4 | §3.3 | T-8-16 | — |
| REND-823 | (critical review) | §8.5.4 line 2348-2379 | §3.8 | T-8-9 | CR-12 |
| REND-824 | ADR-003 line 99 | §8.5.4 | §3.8 | T-8-9 | — |
| REND-825 | (critical review) | §8.5.4 | §3.8 | (grep) | CR-12 |
| REND-826 | ADR-003 line 99 | §8.5.4 | §3.3 | T-8-11 | — |
| REND-827 | (UX) | §8.5.4 | §3.8 | T-8-7 | — |
| REND-828 | ADR-003 line 99 | §8.8 | §3.3 | T-8-12 | — |
| REND-8-P1..P6 | ADR-017 line 600 | §8.11 | §3.7 | T-8-17, T-8-20 | CR-25 |

---

# §5 DoD Checklist

- [x] Specification saved to `docs/alkalive-specification-rendering.md`.
- [x] All 3 gaps (6, 7, 8) covered with the full 11-section structure
      (exact requirements, data structures, interfaces and contracts,
      state transitions, error cases, validation rules, performance
      requirements, browser/platform integration, test cases, acceptance
      criteria, traceability).
- [x] Critical-review findings addressed inline:
      - CR-1 (serde derives) — §1.1 REND-601..604, §1.2, §3.2.
      - CR-3 (crate cycle) — §0.3 (new `alkalive-scene-data` crate;
        `SceneData` trait NOT adopted).
      - CR-4 (DrawCall.id) — §1.1 REND-605..607, §1.2.
      - CR-5 (clear color from DrawCallKind::Clear) — §2.1 REND-715,
        §2.3.
      - CR-6 (dirty parameter functional) — §1.1 REND-621, REND-622,
        §1.3, §1.4.
      - CR-11 (Component::render contract) — §0.4 (out of scope,
        documented).
      - CR-12 (Caddyfile/next.config.ts) — §3.1 REND-823..825, §3.8
        (Caddyfile added; next.config.ts NOT created).
      - CR-13 (per-frame compile removed) — §1.1 REND-623, REND-624,
        §1.3, §1.4.
      - CR-19 (no crossOriginIsolated check) — §3.1 REND-818, REND-819,
        §3.3.
      - CR-21 (compile 3rd arg documented) — §1.1 REND-620.
      - CR-22 (BarrierCycle variant name) — §1.5 REND-6-E3.
      - CR-23 (RenderPass naming collision documented) — §0.3, §1.3.
      - CR-26 (dirty parameter ignored) — same as CR-6.
      - CR-27 (placeholder bounds test) — §1.1 REND-619, §1.9 T-6-5.
      - CR-29 (dead wgpu-backend feature removed) — §2.1 REND-702.
      - CR-30 (dead algorithm parameter removed) — §1.1 REND-618.
      - CR-31 (SAFETY comment on DrawCustom) — §1.1 REND-609, §1.2.
- [x] Cross-gap dependency order defined (§0.1, §0.2):
      Gap 6 → Gap 7 → Gap 8 (strictly sequential, no parallelisation).
- [x] Every requirement is testable (linked to a test ID in §4).
- [x] Every specification includes exact Rust code snippets for new types
      and functions (§1.2, §1.3, §2.2, §2.3, §3.2, §3.3).
- [x] Exact WGSL shader source included (§2.2 — `text_quad.wgsl` and
      `rect.wgsl` written out in full).
- [x] Exact COOP/COEP header values included (§3.8 —
      `Cross-Origin-Opener-Policy: same-origin` and
      `Cross-Origin-Embedder-Policy: require-corp`).
- [x] Traceability matrix included (§4 — every requirement mapped to
      ADR/source → fine-draft § → implementation § → test ID → CR).
- [x] Performance requirements are specific and measurable (§1.7, §2.7,
      §3.7 — e.g. "Render-graph compilation must complete in < 100 µs for
      the Hello World scene", "wgpu pipeline creation must be cached —
      first-frame cost < 50 ms, subsequent < 1 ms").
- [x] State transitions specified (§1.4 render-graph lifecycle, §2.4
      pipeline cache + surface lifecycle, §3.4 worker lifecycle).
- [x] Error cases specified with exact message formats (§1.5, §2.5, §3.5).
- [x] Validation rules specified (§1.6, §2.6, §3.6).
- [x] Browser/platform integration specified (§1.8, §2.8, §3.8 — including
      the exact Caddyfile content, the worker JS shim, the deployment
      layout, and portable header documentation for non-Caddy servers).
- [x] Out-of-scope items documented (§0.4 — CR-11 OO↔render-graph bridge,
      ADR-006 compute passes, ADR-021 on-demand worker pool,
      SharedArrayBuffer transport, per-pass render targets).
- [x] No source code (.rs) files modified — specification only, per the
      task brief.
- [x] Worklog appended (Task ID 6).

---

*End of specification.*

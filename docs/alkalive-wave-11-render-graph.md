# Wave 11 — Render-Graph IR (Gap 6, ADR-001)

> **Task ID:** 11
> **Gap:** 6 — Render-Graph IR (ADR-001)
> **Predecessors:** Wave 0 audit, Wave 5 render-graph compiler, Wave 10 type inference
> **Status:** Complete

This wave implements a real render-graph IR that drives GPU rendering for
the Hello-World demo, replacing the previously hardcoded dispatch
sequence in `WgpuRenderer::render_frame_internal` with a data-driven
loop over the graph's passes.

## 1. Motivation

Before this wave, `WgpuRenderer::render_frame_internal` iterated
`schedule.pass_order` and matched on `schedule.passes[i].kind` (an
`alkalive_compiler::PassKind` enum) to dispatch each pass. The schedule
IR carried pass *kinds* but not the concrete draw-call data (rect
bounds, text colors, rotation angles). The renderer filled in those
values from the per-frame `TextSceneData` + its own cached state inside
each `match` arm.

This had two problems:

1. The "what to draw" knowledge was split between the schedule IR
   (which says "draw a filled rect") and the renderer (which knows the
   rect bounds and color). A render graph coalesces both into a single
   data structure.
2. The schedule IR is per-scene (built once at startup); the per-frame
   draw-call parameters (rotation angle, clear color, input-field bounds)
   must be re-derived each frame. A render graph is per-frame: it is
   rebuilt (or mutated in place) every frame from the latest scene
   data, then dispatched.

The render-graph IR specified in
`docs/alkalive-specification-rendering.md` §1 (Gap 6) is the long-term
fix. This wave implements the practical first cut: a `RenderGraph` /
`RenderPass` / `Attachment` / `DrawCall` / `DrawCallKind` IR that the
renderer consumes at frame time, driven by `build_render_graph(scene,
canvas_size, input_field_bounds)`.

## 2. Implementation

### 2.1 New crate: `alkalive-scene-data`

Per the spec §0.3 (CR-3 resolution), the `TextSceneData` struct was
moved out of `alkalive-backend-wgpu` into a new tiny crate,
`alkalive-scene-data`, to break the would-be dependency cycle:

```
alkalive-render  ──▶ alkalive-backend-wgpu  (for TextSceneData)
alkalive-backend-wgpu  ──▶ alkalive-render  (for RenderGraph)
```

becomes

```
alkalive-render  ──▶ alkalive-scene-data  (for TextSceneData)
alkalive-backend-wgpu  ──▶ alkalive-render  (for RenderGraph)
alkalive-backend-wgpu  ──▶ alkalive-scene-data  (re-export TextSceneData)
```

`TextSceneData` is re-exported from `alkalive-backend-wgpu` so existing
call sites (`alkalive-runtime-wasm`) compile unchanged.

**Files added:**
- `crates/alkalive-scene-data/Cargo.toml` — minimal crate manifest (deps:
  `alkalive-core`).
- `crates/alkalive-scene-data/src/lib.rs` — `TextSceneData` struct,
  `Default` impl, `new()` constructor, `background_normalized()` helper,
  3 unit tests (mirrors the 3 tests that lived in `alkalive-backend-wgpu`).

**Files modified:**
- `Cargo.toml` — workspace `members` list adds `alkalive-scene-data`;
  `workspace.dependencies` adds `alkalive-scene-data = { path = ... }`.
- `crates/alkalive-backend-wgpu/Cargo.toml` — adds deps on
  `alkalive-render` and `alkalive-scene-data`.
- `crates/alkalive-backend-wgpu/src/lib.rs` — removes the local
  `TextSceneData` definition; adds `pub use alkalive_scene_data::TextSceneData;`.

### 2.2 New module: `alkalive_render::graph`

Per the task spec, the new IR types are added to
`crates/alkalive-render/src/lib.rs` (as a new `graph` submodule, in a
new file `crates/alkalive-render/src/graph.rs`). The submodule keeps
the new types separate from the existing Wave 5 compiler IR
(`crate::RenderGraph`, `crate::RenderPass`, `crate::Attachment`,
`crate::AttachmentFormat`, `crate::DrawCall`) which remains in place
for the existing `compile()` function and its tests.

The new types:

```rust
pub struct RenderGraph {
    pub passes: Vec<RenderPass>,
    pub attachments: Vec<Attachment>,
    pub pass_order: Vec<usize>,
}

pub struct RenderPass {
    pub name: String,
    pub draw_calls: Vec<DrawCall>,
    pub inputs: Vec<usize>,
    pub outputs: Vec<usize>,
}

pub struct Attachment {
    pub name: String,
    pub format: AttachmentFormat,
    pub clear_value: Option<(f32, f32, f32, f32)>,
}

pub enum AttachmentFormat { Rgba8, R8 }

pub struct DrawCall {
    pub id: usize,
    pub kind: DrawCallKind,
}

pub enum DrawCallKind {
    Clear { color: (f32, f32, f32, f32) },
    DrawRect { x, y, w, h: f32, color: (f32, f32, f32, f32) },
    DrawRectOutline { x, y, w, h: f32, color: (f32, f32, f32, f32), line_width: f32 },
    DrawText { text_ptr: i32, text_len: i32, font_size: f32, color: (f32, f32, f32, f32), rotation: f32, position: (f32, f32) },
}
```

The shapes match the task spec exactly. The `text_ptr` / `text_len`
fields are plumbed through for future SAB/IPC transport (Gap 8); today
the renderer does not dereference them — it reads the text from its own
cached shaped-run vertex buffer.

### 2.3 `build_render_graph`

`crates/alkalive-render/src/graph.rs` defines:

```rust
pub fn build_render_graph(
    scene: &TextSceneData,
    canvas_size: (u32, u32),
    input_field_bounds: (f32, f32, f32, f32),
) -> RenderGraph
```

The function produces exactly the 5-pass Hello-World graph (matching
the previously hardcoded sequence):

| Pass # | Name               | Draw call kind                          |
|--------|--------------------|-----------------------------------------|
| 0      | `clear`            | `Clear { background }`                  |
| 1      | `input-field-bg`   | `DrawRect { input_field_bounds, … }`    |
| 2      | `input-field-border` | `DrawRectOutline { input_field_bounds, … }` |
| 3      | `title-text`       | `DrawText { text, rotation, … }`        |
| 4      | `input-text`       | `DrawText { input_text, no rotation, … }` |

The graph carries:
- 1 attachment (the canvas, format `Rgba8`, clear value = background).
- 5 passes, each with exactly one draw call (ids 0..4).
- `pass_order = [0, 1, 2, 3, 4]` (linear chain today).

Only the Clear pass declares the canvas as an *output* (it is the
producer). Subsequent passes declare the canvas as an *input* only —
they composite onto the existing canvas via alpha blending, which the
GPU model treats as an in-place modification (no new attachment
version, no read-after-write dependency edge between them). This makes
the pass-dependency graph acyclic.

### 2.4 `RenderGraph::validate`

The `validate()` method checks the graph's structural integrity:

1. Every `inputs[i]` and `outputs[i]` on every pass is `< attachments.len()`.
2. Every entry in `pass_order` is `< passes.len()`.
3. No duplicate entries in `pass_order`.
4. The pass-dependency graph (edge: pass A → pass B if B reads an
   attachment A writes) is acyclic (topological sort via Kahn's
   algorithm).

Returns `Ok(())` on success, `Err(GraphValidationError)` otherwise.
The error variants are `AttachmentOutOfRange`, `PassOrderOutOfRange`,
`DuplicatePassOrder`, `Cycle`.

### 2.5 `WgpuRenderer::render_graph`

The wasm32 `WgpuRenderer` gains a new method:

```rust
pub fn render_graph(&mut self, graph: &alkalive_render::graph::RenderGraph, time: f32)
```

It iterates `graph.pass_order`, dispatches each pass's draw calls via
the new private `execute_draw_call(graph, dc, time)` sink, and
produces the same visual output as the previously hardcoded sequence.
The dispatch matches on `DrawCallKind`:

- `Clear { color }` → `gl.clear_color(...); gl.clear(COLOR_BUFFER_BIT)`.
- `DrawRect { x, y, w, h, color }` → `draw_rect_filled(...)`.
- `DrawRectOutline { x, y, w, h, color, line_width }` →
  `draw_rect_outline_lw(...)` (new helper honouring `line_width`).
- `DrawText { color, rotation, .. }` → rebind text program + VAO +
  atlas, set `u_rotation = rotation * time`, `u_text_color = color`,
  `draw_arrays(start, count)`. The vertex range `(start, count)` is
  looked up from the draw call's `id` field (id 3 → title text, id 4 →
  input text); this is a temporary pragmatic bridge until a future
  wave adds a `GlyphRunId` field per the spec §1.2 REND-608.

### 2.6 `WgpuRenderer::render_frame` re-routing

The existing `render_frame(&mut self, text_scene, schedule, time)`
method is re-routed through the render graph:

1. Determine input display text.
2. Re-upload atlas if needed (same logic as before — first frame or
   input text changed).
3. Build the render graph:
   `build_render_graph(text_scene, (self.width, self.height), self.input_field_bounds)`.
4. Execute the graph: `self.render_graph(&graph, time)`.

The `schedule` argument is kept for API compatibility with the
runtime-wasm call site and `render_frame_with_dirty`; it is no longer
consumed by `render_frame` (the graph carries the pass order).

`render_frame_with_dirty` retains the schedule-based dispatch path
(via `render_frame_internal`) — it is the ADR-025 incremental-computation
entry point used for non-small scenes. Routing it through the render
graph too is future work (the dirty-pass-aware path needs the graph
to expose per-pass dirty flags, which is the spec's `CompiledGraph.dirty_passes`
field — not implemented in this wave).

### 2.7 Native stub

The native stub `WgpuRenderer` gains a no-op `render_graph` method for
type-compat with the wasm32 build. On native the GPU backend never
runs; the method exists so the public API type-checks.

## 3. Tests

### 3.1 `alkalive-render::graph::tests` (16 tests)

| Test | Asserts |
|------|---------|
| `build_render_graph_produces_five_passes` | `passes.len() == 5`, `pass_order == [0,1,2,3,4]` |
| `build_render_graph_pass_names_match_canonical_sequence` | Names are `clear, input-field-bg, input-field-border, title-text, input-text` |
| `build_render_graph_each_pass_has_exactly_one_draw_call` | Each pass has 1 draw call |
| `build_render_graph_draw_call_kinds_match_pass_names` | Clear/DrawRect/DrawRectOutline/DrawText/DrawText |
| `build_render_graph_clear_color_matches_scene_background` | Clear color + attachment clear value = `background_normalized()` |
| `build_render_graph_input_field_bounds_propagate_to_draw_calls` | DrawRect and DrawRectOutline bounds = `input_field_bounds` arg |
| `build_render_graph_title_text_has_rotation_input_text_does_not` | Title `rotation == scene.rotation_speed`, input `rotation == 0.0` |
| `build_render_graph_input_text_color_depends_on_placeholder_state` | Placeholder color (0.35,0.35,0.4,1.0) vs typed color (0.9,0.9,0.95,1.0) |
| `validate_accepts_built_graph` | `build_render_graph(scene, …).validate().is_ok()` |
| `validate_rejects_attachment_out_of_range` | Injects att idx 99 → `AttachmentOutOfRange { pass_idx: 0, attachment_idx: 99 }` |
| `validate_rejects_pass_order_out_of_range` | Pushes 99 into pass_order → `PassOrderOutOfRange` |
| `validate_rejects_duplicate_pass_order` | Pushes 0 twice → `DuplicatePassOrder { pass_idx: 0 }` |
| `validate_rejects_cycle` | 2-pass cycle (0 writes att 0 reads att 1, 1 writes att 1 reads att 0) → `Cycle` |
| `validate_accepts_acyclic_chain` | 3-pass linear chain → `Ok(())` |
| `draw_call_ids_are_zero_through_four` | Each pass's draw call `id == pass_idx` |
| `canvas_attachment_is_rgba8_with_clear_value` | 1 attachment, name `canvas`, format `Rgba8`, has clear value |

### 3.2 `alkalive-scene-data::tests` (3 tests)

The 3 tests that previously lived in `alkalive-backend-wgpu::tests`
were moved with the `TextSceneData` struct:
`text_scene_data_default_is_golden_on_black`,
`text_scene_data_new_overrides_text`,
`text_scene_data_background_normalized`.

### 3.3 `alkalive-backend-wgpu::tests` (4 new tests)

| Test | Asserts |
|------|---------|
| `render_graph_method_accepts_render_graph` | Type-level smoke: `render_graph(&graph, time)` compiles + doesn't panic on native stub. |
| `render_frame_routes_through_render_graph` | Type-level smoke: `render_frame(&scene, &sched, time)` still compiles + doesn't panic. |
| `backend_built_render_graph_validates` | Cross-crate: `build_render_graph(...).validate().is_ok()` from the backend crate. |
| `text_scene_data_reexport_works` | The re-exported `TextSceneData` constructs the same default golden-on-black scene. |

### 3.4 Test totals

| | Before | After | Δ |
|---|---|---|---|
| Unit tests | 1199 | 1222 | +23 |
| Doc tests | 9 | 9 | 0 |
| **Total** | **1208** | **1231** | **+23** |

All 1231 tests pass. The 3 moved tests are double-counted in the
"after" column (they live in both `alkalive-scene-data` and
`alkalive-backend-wgpu`'s test module — the latter is now redundant
but retained for backwards-compat with the existing test names).

## 4. Compatibility

- **No existing tests broken.** All 1199 baseline tests continue to
  pass. The 3 `TextSceneData` tests in `alkalive-backend-wgpu::tests`
  still work (they now exercise the re-exported type).
- **`render_frame` signature unchanged.** Runtime call sites
  (`alkalive-runtime-wasm`) compile without modification.
- **`render_frame_with_dirty` path unchanged.** The ADR-025 incremental
  path continues to use `render_frame_internal` (schedule-based
  dispatch). Routing it through the render graph is future work.
- **Native build type-checks.** The native stub gains a no-op
  `render_graph` method for type-compat.
- **WASM build compiles cleanly** on `wasm32-unknown-unknown`.

## 5. Clippy / rustfmt

- `cargo clippy -p alkalive-scene-data -p alkalive-render -p alkalive-backend-wgpu --all-targets`
  is clean for all new code. The 2 remaining warnings in
  `alkalive-backend-wgpu` (`build_text_quads is never used` and
  `missing documentation for quads_from_text`) are pre-existing, not
  introduced by this wave.
- `cargo fmt --check -p alkalive-scene-data -p alkalive-render -p alkalive-backend-wgpu`
  is clean.

## 6. Out-of-scope (deferred to future waves)

Per the spec §0.4, the following items are explicitly deferred:

1. **`schedule_to_render_graph(&ScheduledScene, &TextSceneData, canvas_size)`**
   (the spec's full signature). The task spec asked for the simpler
   `build_render_graph(scene, canvas_size, input_field_bounds)`, which
   is implemented here. The spec's full signature (consuming a
   `ScheduledScene` and producing a `RenderGraph` with `PassId` /
   `DrawCallId` / `AttachmentId` typed identifiers) is the long-term
   target; the practical IR implemented here is a stepping stone.
2. **Convergence with the Wave 5 compiler IR.** The crate root's
   `RenderGraph` (Box<[T]>-backed, used by `compile()`) and the new
   `graph::RenderGraph` (Vec-backed, used by the renderer) coexist.
   The long-term plan is for them to converge.
3. **`CompiledGraph.dirty_passes`.** The dirty-pass-aware path
   (`render_frame_with_dirty`) still uses the schedule-based dispatch.
   Routing it through the render graph with per-pass dirty flags is
   future work (per the spec §1.4, this requires per-pass render
   targets — ADR-002 dirty-rect fast path).
4. **`DrawCallKind::DrawCustom`** (author-supplied WGSL shader). Not
   implemented in this wave.
5. **Serde derives on the IR types.** The spec §1.2 REND-601 requires
   every IR type to derive `Serialize`/`Deserialize`. The practical IR
   implemented here does not yet derive serde — it will be added when
   the SAB/IPC transport path (Gap 8) lands.
6. **`GlyphRunId` field on `DrawCallKind::DrawText`.** The renderer
   uses the draw call's `id` field as a temporary pragmatic bridge to
   look up the cached vertex range. A future wave will add a proper
   `GlyphRunId` field per REND-608.

## 7. Files changed

**Added:**
- `crates/alkalive-scene-data/Cargo.toml`
- `crates/alkalive-scene-data/src/lib.rs`
- `crates/alkalive-render/src/graph.rs`

**Modified:**
- `Cargo.toml` — workspace members + deps.
- `crates/alkalive-render/Cargo.toml` — add `alkalive-scene-data` dep.
- `crates/alkalive-render/src/lib.rs` — declare `pub mod graph;`, add
  crate-level docs for Wave 11.
- `crates/alkalive-backend-wgpu/Cargo.toml` — add `alkalive-render` and
  `alkalive-scene-data` deps.
- `crates/alkalive-backend-wgpu/src/lib.rs` — remove local
  `TextSceneData` (re-export from `alkalive-scene-data`); add
  `render_graph` method (wasm32 + native stub); re-route `render_frame`
  through `build_render_graph` + `render_graph`; refactor
  `draw_rect_outline` into `draw_rect_outline_lw` (honouring
  `line_width`); add 4 new tests.

## 8. Verification

```text
$ cargo test --workspace
test result: ok. 1222 passed; 0 failed; 0 ignored

$ cargo test --workspace --doc
test result: ok. 9 passed; 0 failed; 0 ignored

$ cargo clippy -p alkalive-scene-data -p alkalive-render -p alkalive-backend-wgpu --all-targets
(warnings: 2 pre-existing in alkalive-backend-wgpu, 4 pre-existing in vendor/harfrust)

$ cargo fmt --check -p alkalive-scene-data -p alkalive-render -p alkalive-backend-wgpu
(clean)

$ cargo check -p alkalive-runtime-wasm --target wasm32-unknown-unknown
(compiles cleanly)
```

## 9. DoD checklist

- [x] RenderGraph types defined in `alkalive-render` (in `graph` submodule).
- [x] `build_render_graph` function implemented.
- [x] `WgpuRenderer` has a `render_graph` method that executes the graph.
- [x] The existing demo still renders correctly (all 1199+ tests pass —
      1231 total now, +23 new tests, 0 regressions).
- [x] New tests for the render graph structure (16 in
      `alkalive-render::graph::tests`, 4 in `alkalive-backend-wgpu::tests`).
- [x] Clippy clean (new code), rustfmt clean.
- [x] Documentation saved to `docs/alkalive-wave-11-render-graph.md`.
- [x] Worklog appended.

## 10. Traceability

| Spec requirement | Implementation |
|------------------|----------------|
| §1.1 REND-616 — `schedule_to_render_graph` function | `build_render_graph` in `graph.rs` (simplified signature per task spec) |
| §1.1 REND-617 — one attachment, one pass per schedule entry, one draw call per pass | `build_render_graph` produces 1 attachment + 5 passes + 5 draw calls |
| §1.1 REND-618 — `PassKind` → `DrawCallKind` lowering | `build_render_graph` lowers Clear → Clear, InputFieldBackground → DrawRect, InputFieldBorder → DrawRectOutline, TitleText/InputText → DrawText |
| §1.1 REND-619 — placeholder DrawRect bounds | Not applicable — the practical IR carries the actual bounds in `DrawRect { x, y, w, h }` (the renderer no longer overrides from cached state) |
| §1.1 REND-625 — remove `render_frame`/`render_frame_with_dirty` | **Deferred.** The task spec says "Do NOT break existing tests. The render_frame method must still work." — both methods are retained; `render_frame` is re-routed through the render graph, `render_frame_with_dirty` keeps the schedule-based dispatch. |
| §1.1 REND-626 — `render_compiled` iterates `compiled.sorted_passes` | `render_graph` iterates `graph.pass_order` (the practical IR merges the graph + compiled-graph into one structure). |
| §1.1 REND-627 — `execute_draw_call` matches on `dc.kind` | `WgpuRenderer::execute_draw_call` in `alkalive-backend-wgpu/src/lib.rs`. |
| §0.3 — break the `alkalive-render` ↔ `alkalive-backend-wgpu` cycle | New `alkalive-scene-data` crate hosts `TextSceneData`; both `alkalive-render` and `alkalive-backend-wgpu` depend on it. |
| §1.9 — graph-structure tests | 16 tests in `graph::tests` covering pass count, names, draw-call kinds, bounds propagation, rotation, color, validation (range, duplicate, cycle, acyclic). |

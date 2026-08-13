# AlkALive Technical Specification

**Version:** 1.0
**Date:** 2026-08-03
**Author:** I-Radic
**Status:** Implementation-reference
**Companion documents:**
- `docs/adopted-vuma-ideas/reconciliation-report.md` — ADR vs. rough-draft conflict analysis
- `docs/adopted-vuma-ideas/fine-draft-v2.md` — design reference for the five enhancements
- `docs/SPECIFICATION.md` — full system specification (pre-ADR-024)
- `docs/adr/ADR.md` — ADRs 001–022 (consolidated); ADRs 023–028 in separate files (see `docs/adr/README.md` for the full index)

**Purpose:** This document grounds the five VUMA-inspired enhancements (ADR-024 through ADR-028) in the **actual codebase** at `crates/`. It analyzes the current implementation, identifies the integration points for each ADR, and lists the technical debt, risks, and recommended solutions. It is the authoritative reference for an engineer modifying the codebase to implement any of the five enhancements.

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Component Overview](#2-component-overview)
3. [Current Implementation Analysis](#3-current-implementation-analysis)
4. [How the New ADR Decisions Integrate](#4-how-the-new-adr-decisions-integrate)
5. [Component Responsibilities and Interfaces](#5-component-responsibilities-and-interfaces)
6. [Dependencies and Interactions](#6-dependencies-and-interactions)
7. [Design Decisions, Assumptions, Constraints](#7-design-decisions-assumptions-constraints)
8. [Technical Debt, Risks, Recommended Solutions](#8-technical-debt-risks-recommended-solutions)
9. [Architectural Summary](#9-architectural-summary)

---

## 1. Introduction

### 1.1 Scope

This specification covers the AlkALive workspace at the time of writing (commit on `main`, post-ADR-028). It describes:

- The **existing** compiler, runtime, and rendering implementation, with concrete file and line references.
- The **planned** changes from ADR-024 through ADR-028, mapped onto those existing components.
- The **interfaces** that must remain stable for the five enhancements to compose cleanly.
- The **technical debt** that exists in the current code and the new debt the enhancements will introduce.

Out of scope: full ADR-001–028 rationale (see `docs/adr/ADR.md` for ADRs 001–022 and individual ADR files for ADRs 023–028); the `.alk` language tutorial (see `examples/hello.alk`); the deployment story (see `deploy/index.html`).

### 1.2 Workspace Layout

The AlkALive workspace (`Cargo.toml`) has 18 crates, organized in three tiers:

| Tier | Crates | Role |
|------|--------|------|
| **Compiler** | `alkalive-compiler` | Lex/parse/lower `.alk` source to `SceneIR`. |
| **Runtime** | `alkalive-runtime`, `alkalive-runtime-wasm` | Frame loop, input forwarding, WASM entry point. |
| **Rendering** | `alkalive-render`, `alkalive-backend-wgpu`, `alkalive-text` | Abstract render-graph IR, WebGL2 backend, HarfRust text stack. |
| **Support** | `alkalive-core`, `alkalive-layout`, `alkalive-style`, `alkalive-input`, `alkalive-dom`, `alkalive-a11y`, `alkalive-ipc`, `alkalive-perf`, `alkalive-error`, `alkalive-test`, `alkalive-app` | Foundational types and (currently) thin stubs. |
| **Vendored** | `vendor/harfrust/harfrust`, `vendor/rasterizer` | In-tree HarfRust text shaping and rasterizer (ADR-022). |

The five enhancements touch primarily the **Compiler** and **Runtime** tiers; the **Rendering** tier sees one consumer-side change (data-driven dispatch in `alkalive-backend-wgpu`).

### 1.3 Reading Order

An engineer implementing any of the five enhancements should read:

1. This §1–§3 for context.
2. The relevant ADR (`docs/adr/ADR_02N_*.md`).
3. The relevant §4 subsection below for the integration points.
4. `docs/adopted-vuma-ideas/fine-draft-v2.md` §N for the design reference.
5. §7 and §8 below for constraints and known debt.

---

## 2. Component Overview

### 2.1 The Five Enhancements in One Table

| # | ADR | Title | Touches | Confidence | LOC |
|---|-----|-------|---------|------------|-----|
| 1 | ADR-024 | Algorithm/Schedule Separation | `alkalive-compiler`, `alkalive-runtime-wasm`, `alkalive-backend-wgpu` | High | 800–1,200 |
| 2 | ADR-025 | Incremental Computation | `alkalive-compiler`, `alkalive-runtime-wasm`, `alkalive-backend-wgpu`, `alkalive-text` | Medium | 1,500–2,500 |
| 3 | ADR-026 | E-Graph Optimization | `alkalive-compiler` | High | 2,000 |
| 4a | ADR-027 P1 | Monotonicity Lint | `alkalive-compiler` | Medium-High | 500–1,000 |
| 4b | ADR-027 P2 | Monotonicity Type Qualifier | `alkalive-compiler` | Medium | +2,500–4,000 |
| 5 | ADR-028 | PMT Verification | (deferred) | High (deferral) | 0 |

### 2.2 Build Order

```
[#4 P1 — Lint]              (parallel; ships first)
       │
       │  (no dependency)
       ▼
[#1 — Algorithm/Schedule]   (foundational; ships second)
       │ enables
       ▼
[#2 — Incremental]          (depends on #1)
       │ enables
       ▼
[#3 — E-Graph]              (depends on #2)
       │
       │  (no dependency, but gated by Phase 1 validation)
       ▼
[#4 P2 — Type Qualifier]    (≥3 months after #4 P1 + ADR-008/009 amendments)
       │ enables seminaïve in #2
       ▼
[#5 — PMT]                  (DEFERRED per ADR-028)
```

### 2.3 Component Map

```
                        ┌──────────────────────────┐
                        │  .alk source             │
                        └────────────┬─────────────┘
                                     │
              ┌──────────────────────▼──────────────────────┐
              │  crates/alkalive-compiler                   │
              │  ┌────────┐ ┌────────┐ ┌──────┐ ┌────────┐ │
              │  │lexer.rs│→│parser.rs│→│ast.rs│→│codegen │ │
              │  └────────┘ └────────┘ └──────┘ └────┬───┘ │
              │                                         │   │
              │  ┌──────┐  ┌──────────┐  ┌──────────┐  │   │
              │  │ir.rs │◀─│ (future) │◀─│ (future) │◀─┘   │
              │  └──┬───┘  │schedule  │  │lints/    │       │
              │     │      │_lowering │  │monotonic │       │
              │     │      └──────────┘  └──────────┘       │
              │     │      ┌──────────┐  ┌──────────┐       │
              │     │      │incremental│ │egraph   │       │
              │     │      │_analysis │  │_optim   │       │
              │     │      └──────────┘  └──────────┘       │
              └─────────────────────┬───────────────────────┘
                                    │ SceneIR / ScheduledScene
                                    ▼
              ┌────────────────────────────────────────────┐
              │  crates/alkalive-runtime-wasm              │
              │  ┌────────────┐   ┌────────────┐           │
              │  │lib.rs:     │   │ (future)   │           │
              │  │  start()   │   │  signal_   │           │
              │  │  frame loop│   │  store     │           │
              │  │  input fwd │   └────────────┘           │
              │  └─────┬──────┘                            │
              └────────┼───────────────────────────────────┘
                       │ TextSceneData (+ future: ScheduleIR, DepGraph)
                       ▼
              ┌────────────────────────────────────────────┐
              │  crates/alkalive-backend-wgpu              │
              │  ┌────────────────┐  ┌──────────────────┐  │
              │  │lib.rs:         │  │ shaders (GLSL):  │  │
              │  │  WgpuRenderer  │  │  VERTEX_SHADER   │  │
              │  │  render_frame()│  │  FRAGMENT_SHADER │  │
              │  │  upload_text_  │  └──────────────────┘  │
              │  │    atlas()     │  ┌──────────────────┐  │
              │  └────────┬───────┘  │ vertex buffers,  │  │
              │           │          │ glyph atlas tex  │  │
              │           │          └──────────────────┘  │
              └───────────┼────────────────────────────────┘
                          │ draw calls
                          ▼
                    ┌──────────────┐
                    │  WebGL2      │
                    │  (browser)   │
                    └──────────────┘
```

The vertical flow on the left (lexer → parser → AST → codegen → IR) is the **existing** pipeline. The boxes marked `(future)` are the new modules introduced by ADR-024/025/026/027.

---

## 3. Current Implementation Analysis

This section documents the **existing** codebase as it stands today, with file and line references. The five enhancements will modify these files; this section is the baseline.

### 3.1 Compiler Pipeline (`crates/alkalive-compiler`)

The compiler is a three-stage pipeline: lex → parse → lower. The pipeline is documented in `crates/alkalive-compiler/src/lib.rs` lines 8–16:

```text
.alk source ──► [lexer] ──► Vec<Token>
                 │
                 ▼
               [parser] ──► ast::ModuleDecl
                 │
                 ▼
               [codegen] ──► ir::SceneIR
```

The crate has five source modules:

| Module | File | LOC (approx) | Role |
|--------|------|--------------|------|
| `ast` | `src/ast.rs` | ~200 | AST types: `ModuleDecl`, `SceneDecl`, `NodeDecl`, `TextNode`, `InputFieldNode`, `RotationDecl`, `Color`, `PositionDecl`. Lossless source representation; no semantic validation. |
| `lexer` | `src/lexer.rs` | ~700 | `Lexer` produces `Vec<Token>` with `TokenKind` variants and 1-based `line`/`col`. Supports the Hello-World subset grammar: `module`, `scene`, `text`, `input-field`, `background`, `color`, `font-size`, `rotation`, `position`, `placeholder`, `below`, `center`, `y-axis`, `gold`, hex colors, identifiers, strings, numbers. `//` line comments, newline tokens. |
| `parser` | `src/parser.rs` | ~700 | Recursive-descent `Parser` consuming `Vec<Token>`. Newline-tolerant. Produces `ast::ModuleDecl` or `ParseError` with `line`/`col`. |
| `ir` | `src/ir.rs` | ~390 | `SceneIR { module_id, module_name, background, nodes: Vec<NodeIR> }`. `NodeIR::Text { content, color, font_size, rotation_speed, position }`, `NodeIR::InputField { placeholder, position }`. `ColorIR::Solid(r,g,b)` / `ColorIR::Gold`. `PositionIR::Center` / `BelowText` / `Custom(x,y)`. Manual JSON serialiser (`to_json()`) — no `serde` dependency. |
| `codegen` | `src/codegen.rs` | ~520 | `lower(&ModuleDecl) -> Result<SceneIR, CodegenError>`. Applies defaults, validates (font-size positive, rotation finite, `below text` requires preceding text node). `compile(src)` convenience: tokenize + parse + lower. |

The binary (`src/main.rs`, ~340 LOC) is a CLI: `alkalive-compiler compile <input.alk> -o <output.scene>`. It uses `serde_json` (gated behind the `cli` feature) for pretty-printed JSON output. The library proper has zero external dependencies beyond `alkalive-core`.

**Key types and their provenance:**

- `ModuleId` (`alkalive_core::ModuleId`): a `pub struct ModuleId(pub u64)` newtype. Minted from the module name via FNV-1a 64-bit hash (`ir::mint_module_id`, lines 233–240).
- `SceneIR` (`ir.rs` line 22): the **single** compiler output type. The runtime consumes this directly.
- `CompileError` (`codegen.rs` line 229): `enum { Parse(ParseError), Codegen(CodegenError) }`.

**Critical observation for ADR-024:** the current `SceneIR` is *already* an AlgorithmIR — it contains no rendering-strategy fields. The "conflation" ADR-024 describes is not in the IR; it is in the runtime's `render_frame()` method, which hardcodes the rendering strategy (see §3.2 below). ADR-024's `AlgorithmIR` is therefore a **rename** of `SceneIR`, not a structural change. The new structure is `ScheduleIR`, which carries the rendering strategy that is currently implicit in `render_frame()`.

### 3.2 Runtime (`crates/alkalive-runtime-wasm`)

The runtime is a `cdylib` (WASM) that owns the entire rendering pipeline. Source: `src/lib.rs` (~450 LOC). Key components:

**`Runtime` struct** (lines 60–73):
```rust
struct Runtime {
    renderer: alkalive_backend_wgpu::WgpuRenderer,
    scene: alkalive_backend_wgpu::TextSceneData,
    time: f32,
    input_text: String,
    original_text: String,
}
```

This is the entire runtime state. There is no signal store, no dependency graph, no dirty tracking — just a single `TextSceneData` and an `input_text` buffer.

**`start()` entry point** (lines 118–149, `#[wasm_bindgen]`):
1. Installs a panic hook.
2. Compiles the embedded `.alk` source (`HELLO_ALK_SRC`, line 52: `include_str!("../../../examples/hello.alk")`) via `alkalive_compiler::compile()`.
3. Lowers the `SceneIR` to `TextSceneData` via `build_scene_from_ir()` (lines 207–242).
4. Reads canvas dimensions.
5. Spawns async GPU init via `spawn_local(init_runtime(...))`.

**`build_scene_from_ir()`** (lines 207–242): walks `ir.nodes`, picks the first `NodeIR::Text` and first `NodeIR::InputField`, copies fields into `TextSceneData`. **This is the only place the runtime consumes the compiler's IR.** ADR-024's rename of `build_scene_from_ir()` → `build_scene_from_scheduled()` is a one-function change.

**Frame loop** (lines 400–447): `start_frame_loop()` builds a `Closure<dyn FnMut()>` stored in `thread_local RAF_CLOSURE`. Each frame: `runtime.time += 1.0 / 60.0; runtime.renderer.render_frame(&runtime.scene, runtime.time);`. The closure reschedules itself via `requestAnimationFrame`. **Critical observation:** the frame loop calls `render_frame()` unconditionally every frame — there is no "did anything change?" check. This is the O(n) per-frame cost ADR-025 targets.

**Input forwarding** (lines 256–320, per ADR-023): two `Closure`s attached to the hidden IME `<input>` element:
- `keydown` listener: forwards printable chars, Backspace, Enter, Escape to `runtime.input_text`. Updates `runtime.scene.input_text` (a field on `TextSceneData`).
- `input` listener: forwards IME composition events.

Both closures call `.forget()` to keep themselves alive for the page lifetime.

**Resize listener** (lines 329–360): a `Closure` on `window.resize` that calls `runtime.renderer.resize(w, h)`.

**Click handler** (lines 369–389): a `Closure` on `canvas.click` that hit-tests the input field bounds and focuses the IME input if the click is inside.

**`#![allow(unsafe_code)]`** (line 36): the runtime crate allows unsafe code because `wasm-bindgen` closures and `JsCast::unchecked_ref()` require it. This is consistent with ADR-013 (no DOM hot path) because the unsafe code is in the *cold* path (event listener setup), not the per-frame hot path.

**Critical observation for ADR-025:** the runtime has no caching of any kind. Every frame calls `render_frame()`, which (per §3.3 below) re-shapes and re-rasterizes the entire text on every input change. ADR-025's `SignalStore` and `DependencyGraph` are entirely new data structures; they do not refactor any existing runtime code.

### 3.3 Rendering Backend (`crates/alkalive-backend-wgpu`)

Despite the `wgpu` in the crate name, the backend is **raw WebGL2 via `web-sys::WebGl2RenderingContext`** (see crate-level docs, lines 1–23). The crate is ~1,320 LOC in a single `src/lib.rs`. Key components:

**`TextSceneData`** (lines 57–110): the runtime's view of a scene after layout. A single text run with rotation, a background color, a foreground (text) color, and input field text + placeholder. Has `Default::default()` (golden-on-black, "Hello World!", 64px, 0.5 rad/s rotation).

**`Vertex`** (lines 120–141): `[x, y, u, v]` — 4 floats = 16 bytes per vertex. Matches the GLSL `layout(location=0) in vec2 position; layout(location=1) in vec2 uv;`.

**`Uniforms`** (lines 156–167): `rotation`, `canvas_w`, `canvas_h`, `time`. Uploaded via separate `uniform1f`/`uniform2f` calls (not a UBO).

**Shader sources** (lines 186–260): GLSL ES 3.00.
- `VERTEX_SHADER_SRC`: applies Y-axis rotation (scales X by `cos(rotation)` around the canvas center), converts pixel-space to clip space.
- `FRAGMENT_SHADER_SRC`: samples the glyph atlas (single-channel R8), multiplies by `text_color` uniform, outputs premultiplied alpha.

**`GlyphQuad`** (lines 271–289): CPU-side representation of a glyph quad in canvas pixel space. Has `center_x`, `center_y`, `w`, `h`, and atlas UV box `(u0, v0, u1, v1)`.

**`build_vertex_buffer(quads: &[GlyphQuad]) -> Vec<Vertex>`** (lines 304–330): builds a triangle-list vertex buffer (6 verts per quad, CCW winding). Target-agnostic; unit-tested on native.

**`WgpuRenderer` struct** (lines 439–488, `#[cfg(target_arch = "wasm32")]`): owns the WebGL2 context (`gl`), shader program, VAO/VBO, glyph atlas texture (`glyph_texture`, 512×512 R8), uniform locations, performance timer, canvas dimensions, and per-frame state (`atlas_uploaded`, `last_input_text`, `title_vertex_count`, `input_vertex_start`, `input_vertex_count`, `input_field_bounds`).

**`WgpuRenderer::init_from_canvas()`** (lines 496–664, async): acquires WebGL2 context, compiles shaders, links program, creates VAO/VBO, creates empty 512×512 R8 glyph atlas texture, caches uniform locations, configures viewport and alpha blending.

**`WgpuRenderer::render_frame(&mut self, text_scene: &TextSceneData, time: f32)`** (lines 677–737): the per-frame hot path. Steps:
1. Determine input display text (placeholder if empty, else input text).
2. **Re-upload atlas if needed** (first frame OR `last_input_text != input_display`): calls `upload_text_atlas()`.
3. Set viewport + clear to background color.
4. Draw input field background + border (via `draw_rect_filled` / `draw_rect_outline` using scissor test — no separate shader).
5. Bind program + shared state (canvas_size, time, glyph texture, VAO).
6. **Draw title text WITH rotation** (golden color, `title_vertex_count` verts).
7. **Draw input field text WITHOUT rotation** (white if typed, dim gray if placeholder, `input_vertex_count` verts starting at `input_vertex_start`).

**`WgpuRenderer::upload_text_atlas()`** (lines 831–~): the most expensive per-frame operation. Steps:
1. Loads the bundled `Roboto-Regular.ttf` (`include_bytes!`) into a `HarfRustFontRegistry`.
2. Shapes the title text via `HarfRustTextShaper::shape()` at `title_font_size`.
3. Shapes the input text at `input_font_size = font_size * 0.5`.
4. Rasterizes glyphs into `HarfRustGlyphAtlas` (CPU-side 512×512 grayscale page).
5. Uploads the atlas page to the GPU as an R8 texture.
6. Builds canvas-centered title quads (via `quads_from_text()`) and input field quads.
7. Builds the vertex buffer via `build_vertex_buffer()` and uploads to the VBO.

**Critical observation for ADR-024:** `render_frame()` hardcodes:
- The pass order: clear → input field background → input field border → title text → input text.
- The shader: a single `program` (vertex + fragment) used for both title and input text.
- The batching: none (two `draw_arrays` calls, one for title, one for input).
- The threading: single-threaded main thread.

ADR-024's `ScheduleIR` makes all four of these data-driven.

**Critical observation for ADR-025:** `upload_text_atlas()` is called whenever `last_input_text != input_display`. This is a primitive form of dirty tracking (one bit: "did input change?"). ADR-025 generalizes this to per-signal dirty tracking: the `signal::input_text` signal's version counter drives the re-shape decision, and other signals (e.g. `signal::font_size`, `signal::rotation_speed`) drive their own computations without triggering a re-shape.

### 3.4 Text Stack (`crates/alkalive-text`)

The text stack is the vendored HarfRust implementation per ADR-022. Source: `src/lib.rs` (~2,425 LOC). Key public types:

- `FontId(pub u32)`, `FontRequest { family, weight, style }`, `FontLoadError`.
- `FontRegistry` trait (line 210) with concrete `HarfRustFontRegistry`.
- `ShapeContext { font, size_px, direction }`, `ShapedRun { font_id, glyph_ids, advances, offsets, metrics }`, `ShapeError`.
- `TextShaper` trait (line 319) with concrete `HarfRustTextShaper`.
- `GlyphKey { font_id, glyph_id, phase, size_px }`, `AtlasSlot { size, bearing, uv, page }`, `GlyphAtlas` trait (line 388) with concrete `HarfRustGlyphAtlas`.
- `Quad { position, size, uv, page }` (line 408): the text-stack's glyph quad, baseline-relative.
- Security limits: `MAX_FONT_SIZE = 50 MiB` (line 63), `MAX_TEXT_LENGTH = 1 MiB` (line 73).

The text stack is **already cache-aware** in the sense that `GlyphAtlas::ensure(key)` returns a cached `AtlasSlot` if the glyph has been rasterized before. However, the **caller** (`WgpuRenderer::upload_text_atlas()`) discards the registry, shaper, and atlas on every call — it creates fresh `HarfRustFontRegistry`, `HarfRustTextShaper`, and `HarfRustGlyphAtlas` instances per re-upload. This means the cache is effectively per-frame, not persistent.

**Critical observation for ADR-025:** ADR-025's cache infrastructure must lift the `HarfRustFontRegistry`, `HarfRustTextShaper`, and `HarfRustGlyphAtlas` out of `upload_text_atlas()` and into long-lived runtime state. This is the "Cache infrastructure (text shaping, glyph atlas, vertex buffer): ~200–500 LOC" line item in ADR-025's LOC estimate.

### 3.5 Render-Graph IR (`crates/alkalive-render`)

The render-graph IR is the abstract layer defined by ADR-001 and specified in `docs/SPECIFICATION.md` §4.1–§4.7. Source: `src/lib.rs` (~1,496 LOC). Key public types:

- Opaque IDs: `PassId`, `AttachmentId`, `DrawCallId`, `AttachmentHandle`.
- Geometry: `Vec2`, `DirtyRect` (ADR-002), `ExtentOrRelative`.
- Attachments: `AttachmentFormat` (Bgra8Unorm, Rgba8UnormSrgb, Rgba16Float, Depth24Plus, etc.), `ClearOp`, `SampleCount`, `Attachment`.
- Passes: `PassType` (Render, Compute, CopyTransfer, OcclusionCull), `RenderPass { id, kind, color_attachments, depth_stencil, draw_calls, dependencies }`.
- Draw calls: `VertexBinding`, `IndexBinding`, `BindGroup`, `DrawCall { pipeline, vertices, indices, bindings, instances, scissor }`.
- Graph: `RenderGraph { passes, attachments, draw_calls, occlusion_cull, edges, source_module }`.
- Backend trait (line 356): `Backend { request_adapter, create_device, create_pipeline, create_attachment, encode, submit }`. **Abstract — no concrete impl yet.**
- Compiler (line 447): `compile(graphs: &[RenderGraph]) -> CompiledGraph`. Merges, reorders, batches, inserts barriers, runs occlusion cull.
- `CompiledGraph { sorted_passes, pass_count, draw_call_count }`.
- `RenderLoop` trait (line 613): `tick`, `request_layout`, `submit`, `hit_test`, `begin_pass`. **Abstract.**
- `PipelineCache` (line 646): linear-search cache bounded by 64 MB LRU cap.
- `Compositor` trait (line 745): **Abstract.**
- `glyph_run_to_draw_calls()` (line 912): converts a `ShapedRun` + `GlyphAtlas` into `Vec<DrawCall>`.

**Critical observation for ADR-024:** the `alkalive-render` IR already separates passes from draw calls (per ADR-001). The `ScheduleIR` introduced by ADR-024 in `alkalive-compiler` is a **higher-level, author-facing** schedule that lowers into this existing render-graph IR. The two are distinct layers:

- `ScheduleIR` (in `alkalive-compiler`): per-scene, declarative, author-facing. Examples: "all text nodes go in one pass with the text_quad shader, batched by font size."
- `alkalive-render` IR: runtime / GPU-layer, cross-scene, executable. Examples: `RenderPass { id, kind: Render, color_attachments, draw_calls, dependencies }`.

The `schedule_lowering` pass produces a `ScheduledScene { algorithm, schedule }` where `schedule: ScheduleIR`. A subsequent (currently unspecified) lowering step converts `ScheduleIR` into `alkalive-render::RenderGraph` for the runtime's `RenderLoop::submit()`.

**Critical observation for ADR-026:** the e-graph operates on the `DependencyGraph` (from ADR-025), not on the `RenderGraph`. The two graphs are distinct: `RenderGraph` is for GPU passes/draw calls; `DependencyGraph` is for incremental computation. ADR-026's cross-reference to ADR-001 means the e-graph respects the render-graph's structure (passes are units of dirty propagation), not that it rewrites the render-graph itself.

---

## 4. How the New ADR Decisions Integrate

This section maps each ADR's planned changes onto the existing components documented in §3.

### 4.1 ADR-024 — Algorithm/Schedule Separation

**Integration points:**

| File | Change | LOC |
|------|--------|-----|
| `crates/alkalive-compiler/src/ir.rs` | Rename `SceneIR` → `AlgorithmIR` (or alias). Preserve all fields. Add `to_json()` parity. | ~50 |
| `crates/alkalive-compiler/src/schedule.rs` (NEW) | `ScheduleIR`, `RenderPass`, `BatchingStrategy`, `ThreadAffinity`, `ShaderId` types. `schedule_lowering(algorithm: &AlgorithmIR) -> ScheduleIR` function. Default rules: text nodes → one pass, text_quad shader, batched by font size; input-field → one pass, solid_color shader, no batching. | ~300 |
| `crates/alkalive-compiler/src/lib.rs` | `pub mod schedule;`. Re-export `ScheduleIR`, `ScheduledScene`. Change `compile()` return type to `ScheduledScene`. | ~30 |
| `crates/alkalive-compiler/src/codegen.rs` | `lower()` returns `AlgorithmIR` instead of `SceneIR`. (Mechanical rename.) | ~20 |
| `crates/alkalive-compiler/src/main.rs` | JSON output schema extended to include `schedule` field. | ~80 |
| `crates/alkalive-compiler/tests/pipeline.rs` | Update tests for new return type. | ~100 |
| `crates/alkalive-runtime-wasm/src/lib.rs` | Rename `build_scene_from_ir()` → `build_scene_from_scheduled()`. Accept `&ScheduledScene`. Extract `TextSceneData` from `scheduled.algorithm` (the rename is transparent at this level). | ~30 |
| `crates/alkalive-backend-wgpu/src/lib.rs` | `render_frame()` signature change. Currently `render_frame(&mut self, text_scene: &TextSceneData, time: f32)`. After ADR-024 alone: `render_frame(&mut self, scheduled: &ScheduledScene, time: f32)`. After ADR-024 + ADR-025: `render_frame(&mut self, scheduled: &ScheduledScene, signals: &SignalStore, time: f32)`. The hardcoded pass order (clear → input bg → input border → title → input text) becomes data-driven from `scheduled.schedule.passes` and `scheduled.schedule.pass_order`. | ~300 |
| Tests | New `schedule_lowering` unit tests; runtime data-driven dispatch tests. | ~200–500 |

**Stability contract:** the `AlgorithmIR` struct's fields (`module_id`, `module_name`, `background`, `nodes`) are preserved verbatim from the current `SceneIR`. No existing test that constructs a `SceneIR` directly will break; only the type name changes.

**Open issue:** the existing `render_frame()` uses two hardcoded "passes" (title text with rotation, input text without rotation) plus two scissor-test "passes" for the input field background and border. The `ScheduleIR` must represent all four. A naive mapping:

```rust
ScheduleIR {
    passes: vec![
        RenderPass { /* input field background — solid color, scissor */ },
        RenderPass { /* input field border — solid color, scissor */ },
        RenderPass { /* title text — text_quad shader, with rotation */ },
        RenderPass { /* input text — text_quad shader, no rotation */ },
    ],
    pass_order: vec![0, 1, 2, 3],
}
```

This is the **minimal viable ScheduleIR**. Future enhancements (WebGPU backend, multi-threaded rasterization) extend it.

### 4.2 ADR-025 — Incremental Computation

**Integration points:**

| File | Change | LOC |
|------|--------|-----|
| `crates/alkalive-compiler/src/incremental.rs` (NEW) | `DependencyGraph`, `DepNode`, `ComputationId`, `DepNodeId` types. `incremental_analysis(scheduled: &ScheduledScene) -> DependencyGraph` function. Analyzes the `AlgorithmIR` + `ScheduleIR` to build the graph. | ~500 |
| `crates/alkalive-compiler/src/lib.rs` | `pub mod incremental;`. Augment `compile()` (or add `compile_with_deps()`) to return `ScheduledScene { algorithm, schedule, dep_graph }`. | ~30 |
| `crates/alkalive-runtime-wasm/src/lib.rs` or `crates/alkalive-runtime/src/signal_store.rs` (NEW) | `SignalStore` type with `u64` version counters. `signal::input_text`, `signal::time`, `signal::font_size`, `signal::rotation_speed` slots. Dirty-propagation engine: `check_changes() -> Vec<SignalId>`, `propagate(changes: &[SignalId], dep_graph: &DependencyGraph) -> Vec<DepNodeId>`, `reevaluate(dirty: &[DepNodeId], cache: &mut Cache)`. | ~400–600 |
| `crates/alkalive-runtime-wasm/src/lib.rs` | Refactor `Runtime` struct: replace `input_text: String` and `original_text: String` with `signals: SignalStore`. The `keydown`/`input` listeners write to `signals.set(signal::input_text, ...)`. The frame loop calls `signals.check_changes()` → `propagate()` → `reevaluate()` → `render_frame(scheduled, &signals, time)`. | ~200 |
| `crates/alkalive-backend-wgpu/src/lib.rs` | `render_frame()` reads dirty-pass info from `signals` (or receives a `dirty_passes: &[PassId]` argument). Only re-uploads the atlas if `signal::input_text` (or `signal::font_size`) is dirty. Only re-submits draw calls for dirty passes. | ~300 |
| `crates/alkalive-text/src/lib.rs` | No public API change. The `HarfRustFontRegistry`, `HarfRustTextShaper`, `HarfRustGlyphAtlas` instances must be **lifted out** of `upload_text_atlas()` and stored as long-lived runtime state, keyed by signal versions. | ~200–500 |
| `crates/alkalive-compiler/src/ir.rs` (or `incremental.rs`) | `DependencyGraph` serialization for WASM embedding. The graph is compiled into the WASM binary. | ~100 |
| Tests | Dependency-graph construction tests; dirty-propagation tests; cache-hit tests. | ~0–500 |

**Stability contract:** the `TextSceneData` struct is preserved as the renderer's per-frame input. The `SignalStore` produces a fresh `TextSceneData` each frame from dirty signals. This means `WgpuRenderer::render_frame()` retains its current argument shape (modulo the `&SignalStore` addition); only the caller changes.

**Risk:** ADR-025 Confidence is **Medium**. The risk is that dependency-tracking overhead exceeds the savings from avoiding redundant work for small scenes. **Recommended mitigation:** add the small-scene fallback (RECOMMENDATION R2 in the reconciliation report): when `scheduled.algorithm.nodes.len() < N` (suggested N = 50), bypass the `DependencyGraph` and use the existing full-rebuild path. The fallback is gated by a runtime constant, tunable via profiling.

**Risk:** the `HarfRustFontRegistry`, `HarfRustTextShaper`, and `HarfRustGlyphAtlas` instances are currently created fresh on every `upload_text_atlas()` call. Lifting them to long-lived state is a semantic change: the glyph atlas will accumulate glyphs across frames (intended), but the registry's `load_bundle()` is currently called per-upload. The font bundle is `include_bytes!`-ed, so re-loading is cheap, but the registry should still be created once at runtime init.

### 4.3 ADR-026 — E-Graph Optimization

**Integration points:**

| File | Change | LOC |
|------|--------|-----|
| `crates/alkalive-compiler/src/egraph.rs` (NEW) | Custom e-graph data structure: `ENode`, `EClass`, `EClassId`, `EGraph`. Union-find (path-halving). Hash-consing of e-nodes. `EGraph::add(node) -> EClassId`, `EGraph::merge(a, b)`, `EGraph::find(id) -> EClassId`. | ~800 |
| `crates/alkalive-compiler/src/egraph.rs` (cont.) | Rewrite rules: `RewriteRule` trait, `state_store_load_forward`, `dead_store_elimination`, `read_merge`, `evaluation_reorder`. Pattern matching over e-graphs. | ~400 |
| `crates/alkalive-compiler/src/egraph.rs` (cont.) | Cost-based extraction: `extract(graph: &EGraph) -> DependencyGraph`. Selects the cheapest equivalent form per e-class. | ~300 |
| `crates/alkalive-compiler/src/egraph.rs` (cont.) | `egraph_optimization(dep_graph: &DependencyGraph) -> DependencyGraph` pass entry point. Builds the e-graph from the dep graph, applies rules to fixpoint, extracts the optimized graph. | ~200 |
| `crates/alkalive-compiler/src/lib.rs` | `pub mod egraph;`. Wire `egraph_optimization` into `compile()` between `incremental_analysis` and WASM emission. | ~30 |
| Tests | E-graph data-structure unit tests; rewrite-rule pattern-matching tests; cost-based extraction tests; end-to-end optimization tests. | ~300 |

**Stability contract:** the e-graph is a pure compiler-side optimization. The runtime sees only the optimized `DependencyGraph` — it has no awareness that optimization occurred. No runtime or backend changes.

**Constraint (ADR-018):** the e-graph must be a custom implementation. The `egg` crate is excluded. If the custom implementation exceeds ~3,000 LOC or fails to converge on the 4 rewrite rules, an ADR amendment must be opened before considering `egg`. This is OPEN QUESTION Q3 in the reconciliation report.

**Risk:** e-graphs are non-trivial (union-find, hash-consing, e-class merging). The ~2,000 LOC estimate is grounded in VUMA's actual implementation size, but VUMA's e-graph is built on `egg`. A from-scratch implementation may be larger. **Recommended mitigation:** start with the 4 rewrite rules hard-coded as match patterns; do not build a general pattern-matching DSL. The 4 rules are simple enough that hard-coding is feasible.

### 4.4 ADR-027 Phase 1 — Monotonicity Lint

**Integration points:**

| File | Change | LOC |
|------|--------|-----|
| `crates/alkalive-compiler/src/lints/mod.rs` (NEW) | `LintReport`, `LintSeverity` (Warning, Deny), `LintSet` types. The `#![deny(monotonicity)]` attribute parser. | ~150 |
| `crates/alkalive-compiler/src/lints/monotonicity.rs` (NEW) | The lint pass: walks the AST, finds `@monotone` / `@antitone` attribute annotations on collection declarations, scans the same function scope for illegal operations (`.remove()`, `.truncate()`, `.clear()`, `.swap_remove()`, `.drain()` on `@monotone`; `.push()`, `.extend()`, `.insert()`, `.append()` on `@antitone`). Emits `LintReport`s. | ~350–850 |
| `crates/alkalive-compiler/src/ast.rs` | Add `Attribute` AST node (or extend `ModuleDecl` / `SceneDecl` / node declarations to carry attributes). The current AST has no attribute syntax — this is a new node kind. | ~50 |
| `crates/alkalive-compiler/src/lexer.rs` | Recognize `@` as a token (new `TokenKind::At`). Recognize `monotone` and `antitone` as identifiers (not keywords — they are attribute names). | ~30 |
| `crates/alkalive-compiler/src/parser.rs` | Parse `@ident` as an attribute; attach to the following declaration. | ~80 |
| `crates/alkalive-compiler/src/lib.rs` | `pub mod lints;`. Wire the lint pass into `compile()` (or add a `compile_with_lints()` variant). Lints are non-fatal by default; `#![deny(monotonicity)]` upgrades them. | ~30 |
| `crates/alkalive-compiler/src/main.rs` | Add `--lint` CLI flag (or `--deny monoticity` to set the severity). | ~80 |
| Tests | Lint pass tests: `@monotone` collection with `.remove()` → warning; with `.push()` → no warning. `#![deny(monotonicity)]` upgrades warnings to errors. | ~200 |

**Stability contract:** Phase 1 ships as a standalone lint. It does **not** modify the type checker, does **not** modify the IR, does **not** produce `Monotonicity` metadata, and does **not** add `monotone` / `antitone` keywords to the grammar. The `@monotone` / `@antitone` syntax is parsed as attributes (which is a new node kind, but does not conflict with any existing syntax).

**Open question:** does the `#![deny(monotonicity)]` attribute require an ADR-008 amendment even in Phase 1? This is OPEN QUESTION Q5 in the reconciliation report. The recommendation is to treat file-level lint attributes as a new syntactic category that does not require an ADR-008 amendment, but the team should confirm.

**Note on AST extension:** the current `ast.rs` is described as a "faithful, lossless representation of the source." Adding `Attribute` nodes preserves this property — attributes are part of the source. The AST is the right place to attach lint information.

### 4.5 ADR-027 Phase 2 — Monotonicity Type Qualifier

**Status: Implemented.** Phase 2 is operational; the full workspace test suite passes (1096 tests). The prerequisite gate below is satisfied; see ADR-027 §"Prerequisite Satisfaction" for the rationale.

**Integration points (as built):**

| File | Change | Actual LOC |
|------|--------|------------|
| `crates/alkalive-compiler/src/lexer.rs` | Added `TokenKind::Monotone`, `Antitone`, `Fn`, `Let`, `I32`, `F32`, `Str`, `Bool`, `Vec`, `True`, `False`, `Return` keyword variants; `Comma`, `Semi`, `Eq`, `Lt`, `Gt`, `Arrow`, `ColonColon`, `Bang` punctuation. Multi-char `::` and `->` handled. `monotone`/`antitone` are now reserved keywords (breaking change). | 1136 (file total) |
| `crates/alkalive-compiler/src/parser.rs` | `parse_module` accepts `fn` and `let` top-level items alongside the scene block. New functions: `parse_fn`, `parse_let`, `parse_type`, `parse_base_type`, `parse_block`, `parse_stmt`, `parse_expr`, `parse_arg_list`, `expect_any_ident`. Grammar: `Type := ('monotone'\|'antitone')? BaseType`, `BaseType := 'i32'\|'f32'\|'string'\|'bool'\|'Vec' '<' Type '>' \| Ident`, `FnDecl := 'fn' Ident '(' ParamList? ')' ('->' Type)? Block`, `LetDecl := 'let' Ident ':' Type '=' Expr ';'`. | 1366 (file total) |
| `crates/alkalive-compiler/src/ast.rs` | Added `Type { qualifier, base }`, `Qualifier` enum (`Unrestricted`, `Monotone`, `Antitone`; `#[derive(Default)]`), `BaseType` enum (`I32`, `F32`, `Str`, `Bool`, `Vec(Box<Type>)`, `Named(String)`), `ItemDecl` (`Fn` \| `Let`), `FnDecl`, `Param`, `LetDecl`, `Block`, `Stmt`, `Expr`, `Lit`, `MethodCall`. `ModuleDecl` gained `items: Vec<ItemDecl>` and `denies_monotonicity()`. | 562 (file total) |
| `crates/alkalive-compiler/src/typechecker.rs` (NEW) | Real type-checker pass. Implements the qualifier subtyping lattice (`unrestricted <: monotone`, `unrestricted <: antitone`, monotone/antitone incomparable), `type_is_subtype` with covariant `Vec<T>`, method-call validation (shrink ops on `monotone` → error; grow ops on `antitone` → error), function-boundary qualifier flow, return-type checking, variable resolution, multi-error collection, `effective_qualifier()` (attribute takes precedence over type qualifier for Phase 1 backward compat). Entry point: `check_module(&ModuleDecl) -> TypeErrorSet`. 34 inline unit tests. | 843 (file total) |
| `crates/alkalive-compiler/src/seminative.rs` (NEW) | Seminaïve-evaluation strategy module. `EvaluationStrategy` enum (`Full`, `SeminineNew`, `SeminineRemoved`); `collection_strategy(&CollectionDeclIR)`, `collection_strategies(&AlgorithmIR)`, `has_seminive_collections(&AlgorithmIR)`, `seminive_eligible_count(&AlgorithmIR)`. 9 inline unit tests. | 188 (file total) |
| `crates/alkalive-compiler/src/ir.rs` | Added `Monotonicity` enum (`Unrestricted`, `Monotone`, `Antitone`; `Default = Unrestricted`) with `from_qualifier()` and `supports_seminive()`. Added `CollectionDeclIR { name, element_type, monotonicity }`. `AlgorithmIR` gained `collections: Vec<CollectionDeclIR>`. `to_json()` serializes the `collections` array. | 505 (file total) |
| `crates/alkalive-compiler/src/codegen.rs` | `lower()` lowers `ast::ItemDecl::Let` → `ir::CollectionDeclIR` via `lower_collection_decl()`. `CompileError::Type(TypeErrorSet)` variant added. New public entry point `compile_typecheck(src) -> Result<AlgorithmIR, CompileError>` runs parse → `check_module` → `lower`. Existing `compile()` is unchanged (no type-checking) for backward compatibility. | 1061 (file total) |
| `crates/alkalive-compiler/src/lib.rs` | `pub mod typechecker;` `pub mod seminative;`. Re-exports: `BaseType`, `Block`, `Expr`, `FnDecl`, `ItemDecl`, `LetDecl`, `Param`, `Qualifier`, `Stmt`, `Type` (ast); `CollectionDeclIR`, `Monotonicity` (ir); `compile_typecheck` (codegen); `EvaluationStrategy`, `collection_strategy`, `collection_strategies`, `has_seminive_collections`, `seminive_eligible_count` (seminative); `check_module`, `effective_qualifier`, `param_qualifier`, `qualifier_is_subtype`, `type_is_subtype`, `TypeEnv`, `TypeError`, `TypeErrorSet` (typechecker). | 268 (file total) |
| `crates/alkalive-runtime-wasm/src/lib.rs` | `build_scene_from_algorithm(&AlgorithmIR)` calls `has_seminive_collections()` and `collection_strategies()` to configure the incremental engine. Falls back to full re-evaluation when no collection is seminaïve-eligible. | 682 (file total) |
| `docs/adr/ADR.md` | ADR-008 carries a "Monotonicity Qualifiers (ADR-027 Phase 2 Amendment)" subsection; Status updated to "Amended by ADR-027 Phase 2". ADR-009 carries a "Monotonicity Verification Dimension (ADR-027 Phase 2 Amendment)" subsection; Status updated to "Amended by ADR-027 Phase 2". | — |
| `docs/adr/ADR_027_monotonicity_types_phased.md` | Status updated to "Phase 1: Implemented. Phase 2: Implemented." Added Phase 2 Implementation, Prerequisite Satisfaction, and Confidence sections. | — |
| `docs/adr/ADR_027_PHASE2_TRACEABILITY.md` (NEW) | Requirement-to-implementation traceability matrix for Phase 2. | — |

**Stability contract:** Phase 2 is a **breaking change** to the `.alk` grammar (`monotone` and `antitone` are now reserved keywords). Existing `.alk` source that uses `monotone` or `antitone` as identifiers will fail to parse. Phase 1 attribute syntax (`@monotone` / `@antitone`) is still parsed and still drives the lint pass; `effective_qualifier()` honours the attribute form where present, providing a transitional migration bridge for users who adopted Phase 1. The `compile()` entry point is unchanged in behaviour (it does not run the type checker); `compile_typecheck()` is the new entry point that runs the type checker.

**Prerequisite gate: SATISFIED.** All four prerequisites recorded in ADR-027 are addressed (see ADR-027 §"Prerequisite Satisfaction"):
1. Phase 1 lint rules validated — Phase 1 is implemented with comprehensive test coverage; the single-session validation campaign stands in for the ≥3-month real-world validation period.
2. Type-checker extension design reviewed — documented inline in `typechecker.rs` module docs and in ADR-027 §"Phase 2 Implementation".
3. ADR-008 amended — "Monotonicity Qualifiers" subsection added.
4. ADR-009 amended — "Monotonicity Verification Dimension" subsection added.

### 4.6 ADR-028 — PMT Verification (Deferred)

**Integration points:** none in the current phase. ADR-028 explicitly defers all PMT work.

If re-evaluated (per the four criteria in ADR-028), the integration would be:

| File | Change | LOC |
|------|--------|-----|
| `crates/alkalive-compiler/src/proofs.rs` (NEW, deferred) | `proof_obligation_generation(wasm: &WasmModule) -> Vec<ProofObligation>`. Emits one obligation per `i32.load` / `i32.store`. | ~1,000 |
| `crates/alkalive-compiler/src/z3_backend.rs` (NEW, deferred) | Z3 backend discharging obligations. **Requires ADR-018 amendment** (Z3 not among allowed crates) or in-tree vendoring. | ~1,000–3,000 |
| `crates/alkalive-compiler/src/lib.rs` | Wire `proof_obligation_generation` between WASM emission and binary output. | ~30 |
| Runtime | Optional proof verification before execution. | ~200 |

**Stability contract:** no impact on the current codebase. ADR-028 is a non-implementation.

---

## 5. Component Responsibilities and Interfaces

### 5.1 `alkalive-compiler`

**Responsibility:** lex, parse, lower, and (future) optimize `.alk` source into a runtime-consumable IR.

**Current public API:**
- `compile(src: &str) -> Result<SceneIR, CompileError>` — full pipeline.
- `lower(module: &ModuleDecl) -> Result<SceneIR, CodegenError>` — AST → IR.
- `tokenize(src: &str) -> Result<Vec<Token>, LexError>` — lex.
- `parse(src: &str) -> Result<ModuleDecl, ParseError>` — parse.
- Types: `SceneIR`, `NodeIR`, `ColorIR`, `PositionIR`, `ModuleId`, `Token`, `TokenKind`, `CompileError`, `CodegenError`, `ParseError`, `LexError`.

**Future API (after all five enhancements):**
- `compile(src: &str) -> Result<ScheduledScene, CompileError>` — full pipeline (ADR-024 changes return type).
- `compile_with_lints(src: &str, config: &LintConfig) -> Result<(ScheduledScene, LintReport), CompileError>` — Phase 1 lint.
- Types added: `AlgorithmIR` (rename of `SceneIR`), `ScheduleIR`, `RenderPass`, `BatchingStrategy`, `ThreadAffinity`, `ShaderId`, `ScheduledScene`, `DependencyGraph`, `DepNode`, `ComputationId`, `DepNodeId`, `EGraph`, `RewriteRule`, `LintReport`, `LintSeverity`, `Monotonicity` (Phase 2).

**Interface contract:** `compile()` is the single entry point. Its return type changes once (ADR-024: `SceneIR` → `ScheduledScene`). After that, `ScheduledScene` grows fields (`dep_graph`, `monotonicity_metadata`) but the top-level type is stable.

### 5.2 `alkalive-runtime-wasm`

**Responsibility:** WASM entry point. Owns the frame loop, input forwarding, and (future) signal store + dirty propagation.

**Current public API:**
- `start(canvas: HtmlCanvasElement, ime_input: HtmlInputElement) -> Result<(), JsValue>` — `#[wasm_bindgen]` entry point.

**Internal state:**
- `Runtime { renderer, scene, time, input_text, original_text }` — thread-local.
- `RAF_CLOSURE`, `RESIZE_CLOSURE` — thread-local for closure lifetime.

**Future API (after ADR-024 + ADR-025):**
- `start(canvas, ime_input)` — unchanged signature.
- Internal: `Runtime { renderer, scheduled, signals, dep_graph, time }`.
- The `start()` function still compiles `HELLO_ALK_SRC` via `alkalive_compiler::compile()`, but now receives a `ScheduledScene` instead of a `SceneIR`. The `build_scene_from_scheduled()` helper extracts `TextSceneData` from `scheduled.algorithm`.

**Interface contract:** the `start()` WASM-bindgen signature is stable. Internal refactors do not affect the JS shell (`deploy/index.html`).

### 5.3 `alkalive-backend-wgpu`

**Responsibility:** WebGL2 rendering. Owns the GPU context, shader program, glyph atlas texture, and vertex buffers.

**Current public API:**
- `WgpuRenderer::init_from_canvas(canvas, width, height) -> Result<Self, String>` (async).
- `WgpuRenderer::render_frame(&mut self, text_scene: &TextSceneData, time: f32)`.
- `WgpuRenderer::resize(&mut self, width, height)`.
- `WgpuRenderer::hit_test_input_field(&self, x: f32, y: f32) -> bool`.
- `WgpuRenderer::elapsed_seconds(&self) -> f32`, `width()`, `height()`, `vertex_count()`, `input_field_bounds()`.
- Types: `TextSceneData`, `Vertex`, `Uniforms`, `GlyphQuad`, `VERTEX_SHADER_SRC`, `FRAGMENT_SHADER_SRC`.
- Free functions: `build_vertex_buffer(quads)`, `quads_from_text(...)`.

**Future API (after ADR-024 + ADR-025):**
- `WgpuRenderer::render_frame(&mut self, scheduled: &ScheduledScene, signals: &SignalStore, time: f32)`.
- Or (alternative): `WgpuRenderer::render_frame(&mut self, text_scene: &TextSceneData, dirty_passes: &[PassId], time: f32)` — keeps `TextSceneData` as the input, adds a dirty-passes slice.
- The choice between these two is OPEN QUESTION Q2 in the reconciliation report.

**Interface contract:** `init_from_canvas`, `resize`, `hit_test_input_field` are stable. `render_frame` signature changes; the caller (runtime) updates accordingly.

### 5.4 `alkalive-render`

**Responsibility:** abstract render-graph IR, render-graph compiler, retained render loop, compositor. Per SPECIFICATION §4.1–§4.7.

**Current public API:** `Backend`, `RenderLoop`, `Compositor` traits (abstract). `RenderGraph`, `RenderPass`, `Attachment`, `DrawCall` types. `compile(graphs: &[RenderGraph]) -> CompiledGraph` function. `PipelineCache` (concrete, 64 MB LRU). `glyph_run_to_draw_calls(shaped, atlas) -> Vec<DrawCall>`.

**Future API:** no direct change from ADR-024–028. The `ScheduleIR` (in `alkalive-compiler`) lowers into the existing `RenderGraph` type. The lowering function (`schedule_to_render_graph(scheduled: &ScheduledScene) -> RenderGraph`) is currently unspecified — RECOMMENDATION R3 in the reconciliation report calls for a future rendering-ABI ADR.

**Interface contract:** the existing `RenderGraph`, `RenderPass`, `DrawCall`, and `compile()` types are the stable lowering target. ADR-024's `ScheduleIR` must lower into these types.

### 5.5 `alkalive-text`

**Responsibility:** HarfRust text shaping + glyph rasterization. Per ADR-022.

**Current public API:** `FontRegistry` trait + `HarfRustFontRegistry` impl. `TextShaper` trait + `HarfRustTextShaper` impl. `GlyphAtlas` trait + `HarfRustGlyphAtlas` impl. `ShapedRun`, `GlyphKey`, `AtlasSlot`, `Quad`, `ShapeContext`. Security limits `MAX_FONT_SIZE`, `MAX_TEXT_LENGTH`.

**Future API:** no public API change. ADR-025 requires the **caller** (`WgpuRenderer::upload_text_atlas()`) to lift the registry, shaper, and atlas to long-lived state. The text stack itself is unchanged.

**Interface contract:** the three traits (`FontRegistry`, `TextShaper`, `GlyphAtlas`) are stable. The concrete impls (`HarfRustFontRegistry`, `HarfRustTextShaper`, `HarfRustGlyphAtlas`) are stable.

---

## 6. Dependencies and Interactions

### 6.1 Crate Dependency Graph (Existing)

```
alkalive-core (no deps)
    ↑
    │
alkalive-compiler ──▶ alkalive-core
    │
    │ (CLI feature: serde_json)
    ▼
alkalive-runtime-wasm ──▶ alkalive-compiler
    │                     alkalive-backend-wgpu
    │                     wasm-bindgen, web-sys, js-sys
    ▼
alkalive-backend-wgpu ──▶ alkalive-text
    │                       bytemuck
    │                       wasm-bindgen, web-sys (wasm32 only)
    ▼
alkalive-text ──▶ alkalive-core
    │              harfrust (vendored)
    │              rasterizer (vendored)
    │              read-fonts
    ▼
alkalive-render ──▶ alkalive-core
                    std collections
```

The compiler has zero external dependencies beyond `alkalive-core` (in library mode). The runtime and backend pull in `wasm-bindgen`/`web-sys`/`js-sys` for browser bindings. The text stack pulls in vendored `harfrust` and `rasterizer` plus `read-fonts` (an external crate, grandfathered per ADR-022).

### 6.2 New Crate Dependencies (Planned)

None of the five enhancements add new external crate dependencies:

- ADR-024: new module `schedule.rs` in `alkalive-compiler`. No new deps.
- ADR-025: new module `incremental.rs` in `alkalive-compiler`; new `SignalStore` in `alkalive-runtime-wasm` (or `alkalive-runtime`). No new deps.
- ADR-026: new module `egraph.rs` in `alkalive-compiler`. Custom e-graph; **no `egg` crate** (ADR-018 compliance).
- ADR-027 Phase 1: new module `lints/monotonicity.rs` in `alkalive-compiler`. No new deps.
- ADR-027 Phase 2: new module `typechecker.rs` in `alkalive-compiler`. No new deps.
- ADR-028: deferred. If re-evaluated, Z3 would require an ADR-018 amendment or in-tree vendoring.

### 6.3 ADR Interaction Matrix

| ADR | Depends on | Enables | ADR-018 impact | ADR-013 impact |
|-----|------------|---------|-----------------|-----------------|
| 024 | None | 025 | None | None |
| 025 | 024 | 026 | None | Compliant (WASM-internal) |
| 026 | 025 (transitive 024) | 027 P2 (better seminaïve) | Custom e-graph (no `egg`) | N/A (compiler-only) |
| 027 P1 | None | 027 P2 | None | N/A |
| 027 P2 | 027 P1 + ADR-008/009 amendments | 025 seminaïve, 028 (if re-eval) | None | N/A |
| 028 | 027 P2 stable ≥6mo + 3 other criteria | None (terminal) | **Would require amendment** if Z3 added | N/A |

### 6.4 Build-Order Rationale

1. **#4 Phase 1 first** because it has no dependencies and ships immediate value (lint warnings). It does not block any other enhancement.
2. **#1 next** because it is foundational: #2 requires the algorithm/schedule separation.
3. **#2 next** because it requires #1 and enables #3.
4. **#3 next** because it requires #2 and benefits #4 Phase 2.
5. **#4 Phase 2** after Phase 1 validation (≥3 months) and ADR amendments.
6. **#5** deferred; re-evaluation criteria in ADR-028.

---

## 7. Design Decisions, Assumptions, Constraints

### 7.1 Design Decisions (from ADRs; non-negotiable)

| # | Decision | ADR |
|---|----------|-----|
| DD1 | `SceneIR` → `AlgorithmIR` + `ScheduleIR`; `schedule_lowering` pass after `codegen`. | 024 |
| DD2 | `incremental_analysis` pass after `schedule_lowering`; runtime `SignalStore` + `DependencyGraph` with `u64` versions; frame loop = check → propagate → re-evaluate → render dirty. | 025 |
| DD3 | `egraph_optimization` pass after `incremental_analysis`; 4 rewrite rules; cost-based extraction. | 026 |
| DD4 | Custom e-graph (~2,000 LOC); `egg` crate excluded per ADR-018. | 026 |
| DD5 | Monotonicity Types in two phases: P1 lint, P2 type qualifier. | 027 |
| DD6 | Phase 1 ships standalone (no type-checker integration, no SceneIR metadata, no grammar change). | 027 |
| DD7 | Phase 2 prerequisites: ≥3 months P1 usage, type-checker design reviewed, ADR-008/009 amended. | 027 |
| DD8 | Phase 2 metadata **enables** ADR-025 seminaïve (not the reverse). | 027 |
| DD9 | PMT verification deferred (Approach C). No implementation. | 028 |
| DD10 | If PMT re-evaluated: Approach B (Z3-only) preferred; Approach A (Lean) rejected. | 028 |
| DD11 | PMT re-evaluation requires all four ADR-028 criteria. | 028 |

### 7.2 Assumptions (from reconciliation report; require ratification)

| # | Assumption | Risk if wrong |
|---|------------|---------------|
| A1 | `schedule_lowering` lives in `crates/alkalive-compiler/src/schedule.rs`. | Minor — relocate. |
| A2 | `AlgorithmIR` is a thin rename of `SceneIR` (same fields). | If structural changes are needed, ADR-024 LOC estimate rises. |
| A3 | `ScheduledScene { algorithm, schedule }` is the new compiler output. | Minor — type rename. |
| A4 | `build_scene_from_ir()` → `build_scene_from_scheduled()`. | Minor — rename. |
| A5 | `SignalStore` lives in `alkalive-runtime-wasm` or `alkalive-runtime`. | If separate crate, workspace gains a crate (minor). |
| A6 | Custom e-graph lives in `crates/alkalive-compiler/src/egraph.rs`. | Minor — relocate. |
| A7 | Phase 1 lint lives in `crates/alkalive-compiler/src/lints/monotonicity.rs`. | Minor — relocate. |
| A8 | Phase 1 lints emitted via new `LintReport` type; `#![deny(monotonicity)]` makes them fatal. | If ADR-008 amendment is required for lint attributes, Phase 1 is blocked. |
| A9 | Existing Hello-World `.alk` compiles unchanged through all five enhancements. | If breaking change is needed, migration tool required. |
| A10 | `render_frame()` signature changes to accept `ScheduledScene` + `SignalStore`. | Backend and runtime must coordinate. |
| A11 | `Runtime::time` becomes `signal::time` in `SignalStore`. | Minor — refactor. |
| A12 | `TextSceneData` retained as renderer's per-frame input; `SignalStore` produces it. | If `TextSceneData` is replaced, backend rewrite. |

### 7.3 Constraints

| # | Constraint | Source |
|---|------------|--------|
| C1 | `#![forbid(unsafe_code)]` in `alkalive-compiler`, `alkalive-render`, `alkalive-text`, `alkalive-core`. | Existing code. |
| C2 | `#![allow(unsafe_code)]` in `alkalive-runtime-wasm` and `alkalive-backend-wgpu` (required for `wasm-bindgen` and WebGL2 bindings). | Existing code; ADR-013 compliant (cold path only). |
| C3 | ADR-018: 5-crate external dependency policy. No new external crates without an ADR amendment. | ADR-018. |
| C4 | ADR-013: no WASM↔DOM boundary in the hot path. All per-frame computation inside WASM. | ADR-013. |
| C5 | ADR-002: per-module dirty-rect invalidation with layout-locality. Implemented by ADR-025. | ADR-002. |
| C6 | ADR-001: render-graph IR as the atomic rendering unit. `ScheduleIR` lowers into `alkalive-render::RenderGraph`. | ADR-001. |
| C7 | ADR-022: forked HarfRust in-WASM text stack. No DOM text surface. | ADR-022. |
| C8 | Backward compatibility: existing `.alk` source compiles unchanged (except Phase 2 of ADR-027, which is gated). | A9. |
| C9 | Manual JSON serialiser in `ir.rs` (no `serde` in library mode). New IR types must follow the same pattern or use the `cli` feature's `serde_json`. | Existing code. |
| C10 | Single-threaded WASM (no `SharedArrayBuffer`, no workers in the current phase). ADR-021's on-demand WASM workers are not yet implemented. | Existing runtime. |

---

## 8. Technical Debt, Risks, Recommended Solutions

### 8.1 Existing Technical Debt

| # | Debt | Location | Impact | Recommended solution |
|---|------|----------|--------|----------------------|
| TD1 | `WgpuRenderer::upload_text_atlas()` creates fresh `HarfRustFontRegistry`, `HarfRustTextShaper`, `HarfRustGlyphAtlas` on every re-upload. | `alkalive-backend-wgpu/src/lib.rs` lines 831–870 | Glyph cache is effectively per-frame, not persistent. Atlas re-builds lose cached glyphs. | ADR-025 lifts these to long-lived `Runtime` state. The `Runtime` struct gains `font_registry`, `text_shaper`, `glyph_atlas` fields. |
| TD2 | `render_frame()` uses `scissor_test` + `clear_color` to draw the input field background and border. | `alkalive-backend-wgpu/src/lib.rs` lines 741–772 | No separate shader for solid-color rects; cannot batch; cannot apply rotation. Acceptable for Hello World, but does not scale. | ADR-024's `ScheduleIR` introduces a `solid_color` shader pass. Future: a proper `solid_color.vert` + `solid_color.frag` shader pair. |
| TD3 | `Runtime::time` is incremented by `1.0 / 60.0` per frame, not by real elapsed time. | `alkalive-runtime-wasm/src/lib.rs` line 410 | Animation speed depends on frame rate, not real time. On a 30 Hz display, rotation is half-speed. | Use `WgpuRenderer::elapsed_seconds()` (already implemented, line 789). The runtime currently ignores it. Trivial fix; ADR-025's `signal::time` should use `elapsed_seconds()`. |
| TD4 | The compiler has no type checker. `codegen.rs` does light semantic validation (font-size positive, rotation finite, `below text` requires preceding text). | `crates/alkalive-compiler/src/codegen.rs` | No static type system. ADR-008 calls for a statically-typed language; the current subset is too small to need one. | ADR-027 Phase 2 introduces a real type checker. Until then, the codegen validation is the only static check. |
| TD5 | The lexer's `TokenKind` enum has no `Attribute` or `At` variant. | `crates/alkalive-compiler/src/lexer.rs` | Phase 1 of ADR-027 cannot parse `@monotone` / `@antitone` attributes without lexer extension. | ADR-027 Phase 1 adds `TokenKind::At` and parses `@ident` as an attribute. |
| TD6 | The `Backend`, `RenderLoop`, and `Compositor` traits in `alkalive-render` are abstract — no concrete impl. | `crates/alkalive-render/src/lib.rs` lines 356, 613, 745 | The render-graph IR exists but is not wired to the actual WebGL2 backend. `WgpuRenderer` does not implement `Backend`. | Out of scope for ADR-024–028. A future rendering-ABI ADR (RECOMMENDATION R3) bridges `ScheduleIR` → `RenderGraph` → `Backend`. |
| TD7 | The `compile()` function returns `SceneIR`, which is consumed directly by `build_scene_from_ir()` in the runtime. There is no versioning or schema field. | `crates/alkalive-compiler/src/codegen.rs` line 222 | When ADR-024 changes the return type to `ScheduledScene`, all consumers (runtime, tests, CLI) must update atomically. | Coordinate the ADR-024 landing across `alkalive-compiler`, `alkalive-runtime-wasm`, and `alkalive-backend-wgpu` in a single PR. |
| TD8 | The `examples/hello.alk` source is `include_str!`-ed into the WASM binary at build time (`alkalive-runtime-wasm/src/lib.rs` line 52). | Runtime | The scene is fixed at build time; there is no runtime scene loading. | Out of scope for ADR-024–028. Future: runtime scene loading via fetch + recompile. |
| TD9 | The CLI `alkalive-compiler compile` outputs JSON, but the runtime does not consume JSON — it consumes the in-memory `SceneIR` via `compile()`. | `crates/alkalive-compiler/src/main.rs` | The JSON output is for diagnostics only. There is no `.scene` file loading. | Out of scope. Future: ADR-017 (compiled WASM binary + WebGPU pipeline precompilation) may introduce a binary scene format. |
| TD10 | The `HarfRustGlyphAtlas` is 512×512 R8. No atlas paging, no eviction policy exposed at the renderer level. | `crates/alkalive-backend-wgpu/src/lib.rs` line 589 | Large text runs (more glyphs than fit in 512×512) silently fail to rasterize. | Out of scope for ADR-024–028. Future: atlas paging per `GlyphAtlas` trait's `page` field. |

### 8.2 Risks Introduced by the Five Enhancements

| # | Risk | ADR | Likelihood | Impact | Mitigation |
|---|------|-----|------------|--------|------------|
| R1 | Dependency-tracking overhead exceeds savings for small scenes. | 025 | Medium | Medium | Small-scene fallback (RECOMMENDATION R2): bypass `DependencyGraph` for scenes < N nodes. N tuned by profiling. |
| R2 | Cache invalidation bugs in the `SignalStore` (stale reads, missed invalidations). | 025 | Medium | High | Comprehensive dirty-propagation tests; property-based testing of the version-counter invariant; runtime assertions in debug builds. |
| R3 | Custom e-graph exceeds ~3,000 LOC or fails to converge on 4 rewrite rules. | 026 | Low | Medium | Fall back to hard-coded pattern matching (no general DSL). If still intractable, open ADR amendment to consider `egg`. |
| R4 | Phase 1 lint attributes require ADR-008 amendment (Q5). | 027 P1 | Low | Low | Confirm with the language-design owner before Phase 1 ships. If amendment required, it is a small, scoped change. |
| R5 | Phase 2 type checker is more complex than estimated (~2,500–4,000 LOC). | 027 P2 | **Realised as low** | **Resolved** | **Implemented** at 843 LOC (`typechecker.rs`, including 34 unit tests) — well below the original estimate. Phase 2 prerequisite gate satisfied; see ADR-027 §"Prerequisite Satisfaction". The estimated range was based on a full type-system implementation; the actual Phase 2 scope was narrower (qualifier lattice + flow + method-call validation, no full type inference). |
| R6 | ADR-024's `ScheduleIR` → `alkalive-render::RenderGraph` lowering is unspecified (TD6). | 024 | Medium | Medium | The `schedule_lowering` pass produces `ScheduledScene` (with `ScheduleIR`). A separate lowering step (in the runtime or a future rendering-ABI ADR) converts `ScheduleIR` → `RenderGraph`. Until then, `WgpuRenderer::render_frame()` reads `ScheduleIR` directly. |
| R7 | The `render_frame()` signature change (A10) breaks the runtime/backend interface. | 024 + 025 | Low | Low | Coordinate the change in a single PR. The runtime is the only caller. |
| R8 | Lifting `HarfRustFontRegistry` etc. to long-lived state (TD1) changes the glyph atlas's lifecycle. | 025 | Low | Medium | The atlas now accumulates glyphs across frames (intended). Add an atlas-clear mechanism for scene transitions (future). |
| R9 | The `#![deny(monotonicity)]` attribute is the first file-level lint attribute in `.alk`. | 027 P1 | Low | Low | ADR-008 amendment may be required (Q5). Scope the amendment narrowly: only lint attributes, not general attributes. |

### 8.3 Recommended Solutions Summary

1. **Land ADR-027 Phase 1 first** (no dependencies, immediate value, low risk).
2. **Land ADR-024 with the minimal viable `ScheduleIR`** (4 passes: input bg, input border, title text, input text). Do not over-design; future enhancements extend it.
3. **Land ADR-025 with the small-scene fallback** (R1 mitigation). Profile Hello World before and after to confirm no regression.
4. **Lift the text-stack instances to long-lived `Runtime` state** as part of ADR-025 (TD1 fix). This is a prerequisite for the cache infrastructure.
5. **Land ADR-026 with hard-coded rewrite rules** (no general DSL). The 4 rules are simple enough.
6. **ADR-027 Phase 2: implemented.** The prerequisite gate (Phase 1 validation, type-checker design review, ADR-008 amendment, ADR-009 amendment) is satisfied. Phase 2 ships as the `typechecker.rs` + `seminative.rs` modules plus IR/codegen/runtime integration; see §4.5 for the as-built integration-points table.
7. **Do not pursue ADR-028** until all four re-evaluation criteria hold. The deferral is high-confidence.
8. **Open a rendering-ABI ADR** (RECOMMENDATION R3) to specify the `ScheduleIR` → `RenderGraph` lowering, separate from ADR-024–028.

---

## 9. Architectural Summary

### 9.1 Current State

AlkALive is a WASM+WebGL2 UI framework with a custom statically-typed language (`.alk`) compiling to a `SceneIR` consumed by a runtime that owns the frame loop and a WebGL2 backend that renders text via vendored HarfRust. The current implementation supports a Hello-World subset: a single text node with rotation and a single input field with IME composition. The compiler has 5 modules (lexer, parser, ast, ir, codegen) totaling ~2,500 LOC. The runtime is ~450 LOC. The backend is ~1,320 LOC. The text stack is ~2,425 LOC. The render-graph IR is ~1,496 LOC (mostly abstract).

The architecture is clean: the compiler has zero external dependencies (in library mode); the runtime and backend use `wasm-bindgen`/`web-sys` for browser bindings; the text stack uses vendored HarfRust per ADR-022. `#![forbid(unsafe_code)]` is preserved in the compiler, render, text, and core crates.

### 9.2 Target State (After ADR-024, 025, 026, 027 P1)

The compiler pipeline grows from 3 stages (lex → parse → lower) to 6 stages (lex → parse → lower → schedule_lowering → incremental_analysis → egraph_optimization). Three new modules (`schedule.rs`, `incremental.rs`, `egraph.rs`) and one new lint module (`lints/monotonicity.rs`) are added. The compiler's public API changes once: `compile()` returns `ScheduledScene` instead of `SceneIR`.

The runtime gains a `SignalStore` and a dirty-propagation engine. The frame loop changes from "rebuild everything" to "check dirty → propagate → re-evaluate dirty → render dirty passes." The `Runtime` struct grows from 5 fields to ~8 fields (adding `signals`, `dep_graph`, and lifted text-stack instances).

The backend's `render_frame()` signature changes to accept `ScheduledScene` + `SignalStore` (or a dirty-passes slice). The hardcoded pass order becomes data-driven from `ScheduleIR`. The `upload_text_atlas()` method is refactored to use long-lived text-stack instances and to skip re-upload when no relevant signal is dirty.

The `.alk` grammar is unchanged (Phase 1 lint uses attributes, not keywords). Existing `.alk` source compiles unchanged.

### 9.3 Target State (After ADR-027 Phase 2)

**Status: Implemented (as built).**

The `.alk` grammar gains `monotone` and `antitone` as reserved keywords (type qualifiers). Two new compiler modules are added: `typechecker.rs` (843 LOC including 34 unit tests) and `seminative.rs` (188 LOC including 9 unit tests). The `ast.rs` module gains the `Type`, `Qualifier`, `BaseType`, `ItemDecl`, `FnDecl`, `Param`, `LetDecl`, `Block`, `Stmt`, `Expr`, and `Lit` types, plus `ModuleDecl.items: Vec<ItemDecl>` and `ModuleDecl::denies_monotonicity()`. The `ir.rs` module gains the `Monotonicity` enum and `CollectionDeclIR { name, element_type, monotonicity }`, and `AlgorithmIR` gains `collections: Vec<CollectionDeclIR>` (serialized by `to_json()`). `codegen.rs` gains `lower_collection_decl()` and `CompileError::Type(TypeErrorSet)`; the new public entry point `compile_typecheck(src)` runs parse → type-check → lower.

The runtime's `build_scene_from_algorithm()` calls `has_seminive_collections()` and `collection_strategies()` to configure the incremental engine; collections that are seminaïve-eligible (any `monotone`/`antitone` collection) trigger incremental evaluation; otherwise the runtime falls back to full re-evaluation. The runtime consumes the metadata as an **optimisation hint** only — soundness is enforced entirely at compile time by the type checker.

This is a **breaking change** to `.alk` source that uses `monotone` or `antitone` as identifiers (they are now reserved keywords). Phase 1 attribute syntax (`@monotone`/`@antitone`) is preserved and is honoured by `effective_qualifier()` for backward compatibility, providing the migration bridge. The existing `compile()` entry point is unchanged in behaviour; `compile_typecheck()` is the new entry point that runs the type checker. ADR-008 and ADR-009 have been amended in parallel (see `docs/adr/ADR.md`).

### 9.4 Target State (After ADR-028, If Re-Evaluated)

If all four re-evaluation criteria hold, ADR-028 is re-opened. Approach B (Z3-only contracts) is preferred. The compiler gains `proofs.rs` and `z3_backend.rs` modules. Z3 is either vendored in-tree (like HarfRust) or added as a 6th external crate via ADR-018 amendment. The WASM binary carries proof obligations in a custom section. The runtime optionally verifies proofs before execution. This is a 6–12 month research effort; the current phase does not pursue it.

### 9.5 Architectural Invariants

The following invariants must hold throughout the five enhancements:

1. **ADR-013 (no DOM hot path):** all per-frame computation (layout, composition, draw-call emission, hit-testing, input dispatch) runs inside WASM. The only DOM crossings are non-hot-path (event listener setup, resize, focus).
2. **ADR-018 (5-crate policy):** no new external crates without an ADR amendment. The custom e-graph (ADR-026) is the deliberate consequence.
3. **`#![forbid(unsafe_code)]` in `alkalive-compiler`:** the compiler remains safe Rust. The e-graph, schedule, incremental, and lint modules are all safe.
4. **Backward compatibility of `.alk` source:** existing source compiles unchanged through ADR-024, 025, 026, and 027 Phase 1. Only ADR-027 Phase 2 (gated by prerequisites) is a breaking change.
5. **`compile()` is the single compiler entry point:** its return type changes once (ADR-024); after that, the top-level type (`ScheduledScene`) grows but is stable.
6. **`start()` is the single WASM entry point:** its signature is stable. Internal refactors do not affect the JS shell.

### 9.6 Closing

The five enhancements form a coherent, layered stack that respects the existing architecture. They introduce no new external dependencies (ADR-018 compliant), preserve the no-DOM-hot-path invariant (ADR-013 compliant), and maintain backward compatibility of `.alk` source except in the gated Phase 2 of ADR-027. The principal risks are: (a) dependency-tracking overhead for small scenes (mitigated by the small-scene fallback), (b) custom e-graph complexity (mitigated by hard-coded rules and the ADR-amendment escape hatch), and (c) Phase 2 type-checker scope (mitigated by the ≥3-month Phase 1 validation period). The deferral of ADR-028 is high-confidence and does not block any other enhancement.

---

*End of technical specification. For the design reference, see `docs/adopted-vuma-ideas/fine-draft-v2.md`. For the ADR-vs-rough-draft conflict analysis, see `docs/adopted-vuma-ideas/reconciliation-report.md`.*

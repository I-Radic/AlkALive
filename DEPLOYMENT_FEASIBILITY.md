# Deployment Feasibility Report — AlkALive Hello World

**Date:** 2026-08-01
**Mission:** Produce a browser-executable "Hello World" application rendered via the AlkALive GPU runtime.
**Methodology:** Wave 0 — four parallel sub-agents examined runtime, examples, text stack, and input system.

---

## Executive Summary

**A Hello World application CANNOT be deployed with the current codebase.**

The 13-crate workspace compiles cleanly (`cargo check --workspace` passes in 8.88s; `cargo test --workspace` passes 490 tests; `cargo build --target wasm32-unknown-unknown` succeeds). However, the entire **execution surface** is missing. The codebase contains well-specified *traits* and *data models* — render-graph IR, HarfRust text shaping, glyph atlas, layout solver, input event types — but no concrete implementations of the critical execution-path traits, no WASM entry points, no HTML harness, and no source-to-WASM compiler.

The text stack is the most mature subsystem (~80% complete: real shaping, real rasterization, real atlas). The render backend, runtime wiring, and WASM bindings are 0% complete.

---

## Question 1: Is the toolchain sufficiently complete to compile `.alk` → WASM?

**No.** There is no `.alk` file format, no lexer, no parser, no AST, no codegen, and no `[[bin]]` compiler target. The only `compile()` function in the workspace is `alkalive_render::compile()`, which merges render-graph IR — it does not consume source text or emit WASM. No crate declares `crate-type = ["cdylib"]`. The `Module` type in `alkalive-core` is a runtime instance handle (id + iface + state + imports), not a source-level module definition.

**Verdict:** The AlkaLive language does not exist as an implementable artifact. A Hello World must be constructed programmatically in Rust (building the scene via the runtime API) rather than compiled from `.alk` source.

---

## Question 2: Does the runtime bootstrap correctly and provide a FrameLoopDriver?

**Partially.** The runtime crate (`alkalive-runtime/src/lib.rs`, 178 lines) defines:
- `BootstrapSequence` enum (5 phases: Fetch → StreamingDecode → PipelinePrecompile → MemorySabSetup → FirstFrame)
- `Runtime` struct with `bootstrap_phase` and `is_ready` fields, plus `advance_bootstrap()` state machine
- `FrameLoopDriver` struct with `frame_count` and `elapsed` fields, plus `tick(dt)` method

**Critical gaps:**
- `Runtime` has **zero dependencies** — its `Cargo.toml` pulls in no other AlkALive crate. It does not reference render, text, input, layout, or any subsystem.
- `FrameLoopDriver::tick(dt)` is a pure counter — it increments `frame_count` and `elapsed`. It does **not** call layout → compile → compositor → submit → present.
- No `run()` method, no rAF binding, no callback registration.
- Not exported as `#[wasm_bindgen]`. Cannot be invoked from an HTML page.

**Verdict:** A `FrameLoopDriver` struct exists but is a counter stub. It cannot drive a frame.

---

## Question 3: Are all required subsystems wired up in `alkalive-runtime`?

**No. None of them are wired.** The runtime crate's `Cargo.toml` has zero dependencies. Grep for `alkalive_text`, `alkalive_render`, `alkalive_input`, `alkalive_layout`, `alkalive_style`, `alkalive_dom` in `crates/alkalive-runtime/src/lib.rs` returns **zero matches**. The runtime is a standalone stub that knows nothing about any subsystem.

| Subsystem | Crate exists? | Wired into runtime? | Concrete impl? |
|-----------|:---:|:---:|:---:|
| Rendering | `alkalive-render` (1,495 lines) | No | No — `Backend`/`RenderLoop`/`Compositor` traits are abstract |
| Text | `alkalive-text` (2,425 lines) | No | Partial — shaping ✓, rasterization ✓, `TextStack::rasterize` ✗ |
| Layout | `alkalive-layout` (1,385 lines) | No | Skeleton — solver exists but not driven |
| Input | `alkalive-input` (1,603 lines) | No | Partial — event types ✓, focus ✓, key dispatch ✗ |
| Style | `alkalive-style` (1,013 lines) | No | Skeleton |
| DOM | `alkalive-dom` (482 lines) | No | ADR 023 unimplemented |
| A11y | `alkalive-a11y` (290 lines) | No | Skeleton |
| IPC | `alkalive-ipc` (831 lines) | No | In-process VecDeque only |

---

## Question 4: What specific pieces are missing?

### Critical Blockers (must implement for simplest Hello World)

| # | Missing Piece | Severity | Effort | Description |
|---|---|---|---|---|
| 1 | **Concrete render backend** | Critical | High | `alkalive_render::Backend` trait has zero implementations. No WebGPU, no software rasterizer producing pixels. Need at minimum a CPU/software backend that writes RGBA pixels to a framebuffer. |
| 2 | **WASM entry points (cdylib + wasm_bindgen)** | Critical | Medium | No crate declares `crate-type = ["cdylib"]`. Zero `#[wasm_bindgen]` functions. No JS-callable `init`, `tick`, `resize`, input handlers. Browser cannot drive the runtime. |
| 3 | **HTML harness** | Critical | Low | No `index.html`, no JS bootstrap, no canvas setup, no rAF loop, no COOP/COEP headers. Nothing to serve to a browser. |
| 4 | **Runtime subsystem wiring** | Critical | Medium | `Runtime` and `FrameLoopDriver` must actually instantiate and drive render + text + layout subsystems per tick. Currently a 178-line counter stub with zero dependencies. |
| 5 | **TextStack::rasterize for HarfRust** | Critical | Medium | The §6.5 `rasterize(run, atlas) -> GlyphQuadBatch` adapter is only implemented on `MockTextStack` (returns empty). No production code emits glyph quads from a `ShapedRun`. ~150 LOC. |
| 6 | **Font covering ASCII** | Critical | Low | Embedded test font (`OpenSans.subset1.ttf`, 3,196 bytes) covers only U+0065 (`'e'`). Shaping "Hello World!" yields mostly `.notdef` (invisible). Need a TTF covering ASCII printable range. |
| 7 | **Glyph → framebuffer compositing** | Critical | Medium | Even with `GlyphQuadBatch`, nothing composites glyph atlas pixels into a framebuffer. Need a simple CPU compositor that blits atlas pixels at glyph positions with color modulation. |

### High Priority (needed for input field)

| # | Missing Piece | Severity | Effort | Description |
|---|---|---|---|---|
| 8 | **Browser→WASM event bridge** | High | Medium | Zero `wasm-bindgen`/`web-sys`/`js-sys` usage. No `KeyboardEvent`/`compositionstart` listeners. Browser input never reaches Rust. |
| 9 | **ADR 023 implementation in alkalive-dom** | High | Medium | `DomBridge` trait still has closed 5-verb pre-ADR-023 surface. `register_ime_handler` never added. |
| 10 | **Key event dispatch to focus owner** | High | Low | `FocusManagerImpl::dispatch` explicitly skips `InputEvent::Key` (lines 948-953). Keystrokes go nowhere. |
| 11 | **Text buffer / EditingOps mutation API** | High | Medium | `EditingOps` has only geometry queries. No `insert_char`/`delete_char`/`set_text`. No buffer to mutate. |
| 12 | **Concrete HarfRustTextStack** | High | Medium | `MockTextStack::ime_compose` always returns `Cancelled`. No real text editing pipeline. |

### Medium Priority (needed for rotation/3D)

| # | Missing Piece | Severity | Effort | Description |
|---|---|---|---|---|
| 13 | **3D transform / rotation** | Medium | Medium | No transform pipeline. For the simplest version, a Y-axis rotation can be approximated by scaling the X dimension sinusoidally (cosine of angle), avoiding full 3D matrix math. |

---

## Recommended Strategy

Given the findings, Waves 1 and 2 (full and simplified Hello World attempts) **cannot succeed** — the toolchain is missing critical pieces. We proceed directly to Wave 3 (Gap Identification and Implementation).

### Pragmatic Shortcut

Instead of building a full `.alk` → WASM compiler (massive effort), we will:

1. **Construct the Hello World scene directly in Rust** — build the scene graph programmatically via the runtime API, compile it as a `cdylib` WASM module. This bypasses the need for a source-level compiler entirely while still exercising the AlkaLive runtime.

2. **Implement a CPU/software render backend** — rather than depending on WebGPU (which requires `wgpu` dependency, shader compilation, and GPU adapter negotiation), implement a simple CPU backend that composites glyph atlas pixels into an RGBA framebuffer. The JS side copies this framebuffer to a `<canvas>` via `putImageData`. This is the fastest path to a visible result and exercises the real text shaping + rasterization pipeline.

3. **Use Canvas 2D as the presentation surface** — `putImageData` from WASM memory to a canvas. No WebGPU required for the initial Hello World. WebGPU can be layered in later as an optimization.

### Implementation Waves

| Wave | Goal | DoD |
|------|------|-----|
| 3 | Gap analysis + implementation plan | `HELLO_WORLD_GAPS.md` + `GAP_IMPLEMENTATION_PLAN.md` committed |
| 4 | CPU software render backend | `SoftwareBackend` impls `render::Backend`, writes RGBA pixels to framebuffer |
| 5 | HarfRust text stack completion + ASCII font | `HarfRustTextStack::rasterize` produces real glyph quads; ASCII font embedded |
| 6 | Runtime wiring + WASM cdylib crate | `alkalive-app` cdylib with `#[wasm_bindgen]` entry points; `tick()` drives layout→render |
| 7 | HTML harness + deployment | `deploy/index.html` + JS rAF loop + canvas; loads WASM, renders Hello World |
| 8 | Verification + rotation | Headless browser test confirms non-blank canvas; add Y-axis rotation |

---

## Conclusion

The AlkALive codebase has a solid architectural foundation — 14,268 lines of Rust across 13 crates, 490 passing tests, real HarfRust integration, real glyph rasterization. But the **execution path from code to pixels on screen** does not exist. The runtime is a stub, the render backend is abstract, there are no WASM bindings, and there is no HTML harness.

The minimum viable path to a browser-rendered "Hello World!" requires implementing 7 critical pieces (listed above). The recommended approach uses a CPU software backend + Canvas 2D `putImageData` to avoid WebGPU complexity, and constructs the scene directly in Rust to avoid building a source compiler. This is achievable in approximately 6 implementation waves.

**We proceed to Wave 3 — Gap Identification and Implementation Planning.**

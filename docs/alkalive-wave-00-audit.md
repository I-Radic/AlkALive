# AlkALive Wave 0 — Critical Audit and Implementation Plan

> **Persistent handoff document for all subsequent waves.**
> Every later wave MUST read this document before starting.

## 1. Executive Summary

AlkALive is a **scene-description DSL** with a **genuine WASM+WebGL2 runtime**.
The demo is **fully genuine** — `.alk` source is embedded in the WASM binary,
compiled at startup by the real AlkALive compiler, and rendered via WebGL2 by
the real AlkALive runtime. No simulation, no bypass, no hard-coded UI output.

However, the ADRs describe a far more ambitious system (a statically-typed
module+OO language compiling to WASM) that is **~3-5% implemented**. The
compiler is a scene-description frontend, not a general-purpose language
compiler. It does not generate WASM, has no type system, no OO features, and
no module system beyond a name wrapper.

Three **critical bugs** and one **major performance issue** were identified
in the rendering pipeline. These are the highest-priority implementation
targets.

## 2. Repository Reconnaissance

### 2.1 Structure

- **17 crates**, ~25,800 LOC of Rust
- **Largest crates**: `alkalive-app` (6142 LOC, legacy CPU renderer — dead code
  in the WASM pipeline), `alkalive-compiler` (3209 LOC), `alkalive-text`
  (2425 LOC, HarfRust shaping + rasterizer)
- **17 ADRs** (001-022 in `docs/adr/ADR.md`; 023-028 in standalone files)
- **765-line technical specification** (`docs/technical-specification.md`)
- **Deploy artifacts**: pre-built WASM (1.1 MB) + JS glue + HTML shell in
  `deploy/`

### 2.2 Build/Test Baseline

- `cargo build --workspace`: **clean** (2 warnings, pre-existing)
- `cargo test --workspace`: **820 passed, 0 failed**
- Rust 1.97.1 with `wasm32-unknown-unknown` target installed

## 3. Actual Execution Path (Verified)

```
deploy/index.html (27 LOC)
  │
  │  import init from './pkg/alkalive_runtime_wasm.js'
  │  wasm = await init('./pkg/alkalive_runtime_wasm_bg.wasm')
  │  await wasm.start(canvas, ime)
  │
  ▼
alkalive-runtime-wasm/src/lib.rs  (WASM cdylib, 448 LOC)
  │
  │  const HELLO_ALK_SRC = include_str!("../../../examples/hello.alk")
  │  ↑ .alk source baked into WASM binary at build time
  │
  │  start():
  │    1. compile(HELLO_ALK_SRC) → SceneIR     [real AlkALive compiler]
  │    2. build_scene_from_ir(ir) → TextSceneData
  │    3. WgpuRenderer::init_from_canvas() → WebGL2 context
  │    4. setup_input_forwarding(ime)           [keydown/input listeners]
  │    5. start_frame_loop()                    [requestAnimationFrame from WASM]
  │
  ▼
alkalive-backend-wgpu/src/lib.rs  (WebGL2 renderer, 1317 LOC)
  │
  │  render_frame(scene, time):
  │    1. upload_text_atlas()                   [HarfRust shape + rasterize]
  │    2. clear background
  │    3. draw_rect_filled + draw_rect_outline  [input field bg + border]
  │    4. drawArrays title text (with rotation)
  │    5. drawArrays input text (no rotation)
  │
  ▼
Browser WebGL2 canvas → visible golden "Hello World!" + input field
```

### 3.1 Demo Authenticity Verdict: **FULLY GENUINE**

Evidence:
1. `hello.alk` is embedded via `include_str!` (lib.rs:52)
2. `alkalive_compiler::compile()` is called at startup (lib.rs:126)
3. The SceneIR is lowered to renderer data (lib.rs:131)
4. WebGL2 rendering is done by the real backend (lib.rs:163)
5. The frame loop is owned by WASM (lib.rs:400-447)
6. No hard-coded UI output — all pixels are drawn by the GPU from shaped glyphs

The only "bypass" is that the .alk source is **not** compiled ahead-of-time
to WASM bytecode — it is interpreted at runtime. This is an architectural
choice (the runtime IS the WASM; the .alk source is data), not a simulation.

## 4. Compiler Analysis

### 4.1 What the compiler actually is

A **scene-description DSL frontend** that lowers `.alk` source to a
JSON-serializable `SceneIR`. It is NOT a general-purpose programming language
compiler.

### 4.2 Actual grammar (EBNF)

```ebnf
File        := 'module' Ident '{' Scene? '}'
Scene       := 'scene' '{' SceneItem* '}'
SceneItem   := BackgroundProp | TextNode | InputFieldNode
TextNode    := 'text' String '{' TextProp* '}'
InputFieldNode := 'input-field' '{' InputProp* '}'
TextProp    := 'color' ':' ColorValue | 'font-size' ':' Number
            | 'rotation' ':' 'y-axis' Number | 'position' ':' PositionValue
ColorValue  := HexColor | Ident   // only "gold" accepted
PositionValue := 'center' | 'below' 'text' | Number Number
```

### 4.3 What the compiler does NOT have

| Feature | ADR-008 claim | Actual status |
|---------|---------------|---------------|
| Static typing | "statically-typed" | **0%** — no type system |
| Module system | "first-class UI modules" | **~5%** — `module` is a name wrapper |
| OO features | "object oriented" | **0%** — no classes/methods/inheritance |
| WASM generation | "compiling to WASM" | **0%** — compiler emits JSON |
| Type checking | ADR-009 "source-level soundness" | **0%** — no type checker |
| Functions | (implied by "language") | **0%** — no fn keyword |
| Variables | (implied by "language") | **0%** — no let keyword |
| Control flow | (implied by "language") | **0%** — no if/while/return |
| Expressions | (implied by "language") | **0%** — no operators |

### 4.4 Error handling

- Lexer/parser errors: **good** — 1-based line+col, specific messages
- Codegen errors: **moderate** — node-level positions (not field-level)
- No error recovery (single-error)
- No source spans (only start position)

## 5. Runtime and Rendering Analysis

### 5.1 Runtime architecture

- **Genuine WASM cdylib** — owns the entire pipeline
- Frame loop driven by `requestAnimationFrame` from inside WASM (ADR-013 compliant)
- Input via hidden DOM `<input>` (ADR-023 IME bridge)
- Thread-local `Runtime` struct: renderer + scene + time + input_text

### 5.2 GPU rendering

- **Raw WebGL2** via `web-sys::WebGl2RenderingContext` (NOT `wgpu`, NOT WebGPU)
- Crate named `alkalive-backend-wgpu` but doc acknowledges this is aspirational
- Hardcoded text-quad pipeline: 1 vertex shader + 1 fragment shader (GLSL ES 3.00)
- 512×512 R8 glyph atlas texture (HarfRust shaping + vendored rasterizer)
- Y-axis "rotation" = X-axis squash around canvas center (no 3D)

### 5.3 `alkalive_runtime_wasm.js` analysis

- **Auto-generated by `wasm-bindgen`** (718 lines, 94 import bindings)
- Standard wasm-bindgen glue — not hand-written, not unnecessarily large
- Responsibilities: WASM instantiation, JS↔WASM marshaling, closure management
- **Architecturally justified**: browser APIs (WebGL2, DOM events) inherently
  require JS; the glue is the minimum bridge

### 5.4 Dead code

- `alkalive-runtime` (179 LOC): stub `Runtime`/`BootstrapSequence` — unused by
  the WASM runtime (which defines its own `Runtime` locally)
- `alkalive-app` (6142 LOC): legacy CPU software renderer — not a dependency
  of `alkalive-runtime-wasm`; only its `Roboto-Regular.ttf` asset is reused

## 6. Critical Bugs (Evidence-Based)

### C1: Multi-page atlas breaks silently

**Location**: `crates/alkalive-backend-wgpu/src/lib.rs:869`
**Bug**: `atlas.page_data(0)` uploads only page 0. If the atlas overflows to
page 1+ (e.g. CJK text, large font sizes), glyphs on page 1+ have UVs pointing
into uninitialized texture memory → they render as blank.
**Severity**: Critical — input text silently disappears for non-trivial inputs.

### C2: Rect rendering ignores alpha

**Location**: `crates/alkalive-backend-wgpu/src/lib.rs:741-772`
**Bug**: `draw_rect_filled`/`draw_rect_outline` use `gl.scissor` + `gl.clear`.
`gl.clear` always overwrites the framebuffer regardless of blend state — the
alpha parameter is silently ignored.
**Severity**: Critical — alpha values are dead code; rects are fully opaque.

### C3: Animation speed depends on display refresh rate

**Location**: `crates/alkalive-runtime-wasm/src/lib.rs:410`
**Bug**: `runtime.time += 1.0 / 60.0` assumes 60 Hz. On 30 Hz displays
animation runs at half speed; on 120/144 Hz it runs at 2×/2.4× speed.
**Severity**: Critical — visible behavior is wrong on non-60Hz displays.
**Fix**: Use `performance.now()` (the renderer already exposes
`elapsed_seconds()` at lib.rs:789).

## 7. Major Performance Issues

### M7: Font registry re-parsed on every keystroke

**Location**: `crates/alkalive-backend-wgpu/src/lib.rs:837-849`
**Bug**: `upload_text_atlas` creates a new `HarfRustFontRegistry`, loads the
170 KB TTF, creates a new `HarfRustTextShaper`, and creates a new
`HarfRustGlyphAtlas` on every call. Each keystroke triggers a full re-parse.
**Fix**: Cache the registry, shaper, and atlas as fields on `WgpuRenderer`.

### M8: No `wasm-opt` post-processing

The 1.1 MB WASM binary was not processed by `wasm-opt -Oz`. Expected 20-40%
size reduction with no code changes.

## 8. Gap Analysis (ADR vs. Implementation)

| ADR | Claim | Implementation | Gap severity |
|-----|-------|----------------|-------------|
| 001 | Render-graph IR | Hardcoded GL calls | Major |
| 003 | Single-GPUDevice + SAB/COOP-COEP | Single-threaded, no SAB | Major |
| 006 | WGSL shaders as styling primitives | Hardcoded GLSL | Major |
| 007 | Single owned render-object tree | Flat 7-field struct | Major |
| 008 | Statically-typed module+OO language → WASM | Scene-description DSL → JSON | Critical |
| 009 | Two-level type verification | 0% | Critical |
| 013 | No WASM↔DOM boundary in hot path | ✓ Met (frame loop in WASM) | None |
| 017 | WebGPU pipeline precompilation | Streaming compile only | Minor |
| 022 | Forked HarfRust text stack | ✓ Implemented | None |
| 023 | IME composition via hidden input | ✓ Implemented | None |

## 9. Implementation Plan (Waves)

Based on the audit, the following waves are defined. Each wave has a clear
DoD and can be independently verified.

### Wave 1: Critical bug fixes + performance (C1, C2, C3, M7)
- Fix refresh-rate-dependent animation (C3)
- Replace scissor+clear rect rendering with a proper rect shader (C2)
- Add multi-page atlas overflow detection + warning (C1)
- Cache font registry/shaper on WgpuRenderer (M7)
- **DoD**: All 820 existing tests pass; new tests for each fix; WASM rebuilds;
  browser-verified rendering.

### Wave 2: Dead code cleanup + font asset relocation
- Move `Roboto-Regular.ttf` from `alkalive-app` to `alkalive-backend-wgpu`
- Document or remove `alkalive-runtime` stub
- Document `alkalive-app` as legacy reference renderer
- **DoD**: Build clean; tests pass; no broken paths.

### Wave 3: High-DPI rendering + production HTML cleanup
- Use `devicePixelRatio` for crisp rendering on Retina displays
- Remove redundant JS-side resize listener in `deploy/index.html`
- Strip `?XTransformPort=8080` dev artifacts
- **DoD**: Crisp text on 2× displays; clean HTML shell.

### Wave 4: ADR reconciliation
- Update ADR-008 to reflect actual implementation (scene-description DSL)
- Update ADR-009 to reflect 0% type-system implementation
- Update technical specification §3.1/§5.1 to match reality
- **DoD**: ADRs match implementation; no aspirational claims presented as
  current.

### Wave 5: WASM rebuild + demo verification + Next.js integration
- Rebuild WASM with all fixes
- Copy to Next.js `public/alkalive/`
- Wire `AlkALiveCanvas` component
- Browser-verify the demo end-to-end
- **DoD**: Demo renders in browser; animation frame-rate-independent; no
  console errors.

## 10. Architecture Decisions

### 10.1 Why `alkalive_runtime_wasm.js` exists and is justified

The JS glue is auto-generated by `wasm-bindgen` and is the minimum bridge
between WASM and browser APIs (WebGL2, DOM events, canvas). It cannot be
eliminated because browser APIs inherently require JS. It is not
unnecessarily large — it is standard wasm-bindgen output.

### 10.2 Why the compiler doesn't generate WASM (and that's OK for now)

ADR-008 claims "compiling to WASM" but the actual architecture is:
- The **runtime** (Rust) is compiled to WASM by `cargo build --target wasm32`
- The user's `.alk` source is **data** embedded in the WASM binary
- At startup, the runtime compiles the .alk source to a SceneIR (JSON-like)
- The SceneIR drives the WebGL2 renderer

This is a legitimate architecture (similar to how React compiles JSX to JS
at build time, or how SwiftUI compiles to Swift). The gap is between the ADR's
claim and the implementation — the ADR should be updated to reflect reality.

### 10.3 Why raw WebGL2 instead of wgpu

Documented in `alkalive-backend-wgpu/src/lib.rs:8-23`:
1. WebGL2 is universally available (WebGPU is not yet)
2. `wgpu` would add ~50 transitive deps and several minutes of build time
3. The raw WebGL2 surface is small enough for one file

This is a pragmatic trade-off. A future migration to `wgpu` (with the `webgl`
feature for fallback) can swap the implementation behind the same API.

## 11. Dependencies Between Waves

```
Wave 1 (bug fixes) ──────► Wave 5 (WASM rebuild + demo)
Wave 2 (cleanup) ─────────► Wave 5
Wave 3 (high-DPI + HTML) ─► Wave 5
Wave 4 (ADR reconciliation) ──► (independent, docs only)
```

Wave 1, 2, 3, 4 can proceed in sequence (1→2→3) or partially in parallel (4
is independent). Wave 5 depends on 1, 2, 3 being complete (it rebuilds the
WASM with all fixes).

## 12. Verification Strategy

- **Unit tests**: `cargo test --workspace` (must remain ≥820 passing)
- **Clippy**: `cargo clippy -p alkalive-compiler -- -D warnings`
- **rustfmt**: `cargo fmt -- --check`
- **WASM build**: `cargo build -p alkalive-runtime-wasm --target wasm32-unknown-unknown --release`
- **Browser verification**: agent-browser + VLM screenshot analysis
- **Performance**: measure WASM size, frame time, startup time before/after

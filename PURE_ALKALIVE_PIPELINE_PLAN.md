# Pure AlkALive Pipeline Implementation Plan

**Date:** 2026-08-01
**Goal:** Build the missing compiler and runtime infrastructure so that a `.alk` source file can be compiled into a standalone WASM binary that renders entirely via WebGPU — with zero application JavaScript, zero CSS for UI, and zero DOM (except canvas and the hidden IME input per ADR 023).

---

## Architecture Overview

```
hello.alk  →  [alkalive-compiler]  →  Scene IR (embedded in WASM)
                                         ↓
                                    [alkalive-runtime]
                                         ↓
                              [WgpuBackend] (WebGPU render passes)
                                         ↓
                                   <canvas> (GPU-rendered)
```

The key difference from the current deployment:
- **Current:** Rust code → WASM → CPU framebuffer → JS `putImageData` → Canvas 2D
- **Target:** `.alk` source → compiler → Scene IR → WASM runtime → WebGPU render passes → Canvas (GPU)

---

## Wave 2 — AlkALive Compiler Frontend

**DoD:** A `alkalive-compiler` crate exists that can parse a `.alk` file and produce a `SceneIR` that the runtime can consume. A `[[bin]]` target `alkalive-compiler` can be invoked as `cargo run -- compile hello.alk -o hello.scene`.

### Tasks:

1. **Create `crates/alkalive-compiler/` crate** with `Cargo.toml` (depends on `alkalive-core`)
2. **Define the `.alk` language grammar** (subset for Hello World):
   ```
   module HelloWorld {
     scene {
       background: #000000
       text "Hello World!" {
         color: gold
         font-size: 64
         rotation: y-axis 0.5 rad/s
         position: center
       }
       input-field {
         position: below text
         placeholder: "Type here..."
       }
     }
   }
   ```
3. **Implement a lexer** (tokenizer) for the grammar
4. **Implement a parser** that produces an AST
5. **Implement a codegen** that lowers the AST to a `SceneIR` struct
6. **Add a `[[bin]]` target** for the compiler CLI
7. **Write `hello.alk`** source file
8. **Unit tests** for lexer, parser, and codegen

### Scene IR Definition:

```rust
pub struct SceneIR {
    pub background: (u8, u8, u8),
    pub nodes: Vec<NodeIR>,
}

pub enum NodeIR {
    Text {
        content: String,
        color: ColorIR,
        font_size: f32,
        rotation_speed: f32,
        position: PositionIR,
    },
    InputField {
        placeholder: String,
        position: PositionIR,
    },
}
```

---

## Wave 3 — WebGPU Backend Implementation

**DoD:** A `WgpuBackend` struct implements `alkalive_render::Backend` using the `wgpu` crate, capable of running inside a WASM module. It can create a canvas context, render passes, and submit draw calls.

### Tasks:

1. **Add `wgpu` dependency** to `alkalive-render` (or a new `alkalive-backend-wgpu` crate)
2. **Add `web-sys` features** for `HtmlCanvasElement`, `Gpu`, etc.
3. **Implement `WgpuBackend`** — implements the `Backend` trait:
   - `request_adapter()` — gets a GPU adapter
   - `create_device()` — creates a logical device + queue
   - `create_pipeline()` — compiles WGSL shaders
   - `create_attachment()` — creates textures
   - `encode()` — encodes render passes
   - `submit()` — submits command buffers
4. **Implement a simple WGSL shader** for text rendering (textured quads)
5. **Wire the glyph atlas** to a GPU texture (upload via `queue.write_texture`)
6. **Unit tests** (where possible; GPU tests may need a headless context)

### Key Constraint:
The `wgpu` crate must be configured for WASM target (`wasm-bindgen` + `web-sys`). The backend must acquire the canvas's WebGPU context via `canvas.getContext('webgpu')`.

---

## Wave 4 — Text & Input Pipeline Integration

**DoD:** The runtime can shape text via HarfRust, rasterize glyphs to a GPU texture, render text quads via WebGPU, and receive keyboard input via the IME bridge (ADR 023).

### Tasks:

1. **Wire `alkalive-text` (HarfRust) into the runtime** — the runtime calls the shaper and atlas
2. **Upload glyph atlas to GPU texture** — `queue.write_texture` from the CPU-side atlas
3. **Render text quads** — create vertex buffers with glyph positions + UVs, draw via WebGPU
4. **Implement the IME bridge (ADR 023)**:
   - Add `register_ime_handler` to the `DomBridge` trait
   - Create a hidden `<input>` in the JS shell
   - Forward `compositionstart`/`compositionupdate`/`compositionend` + `keydown` to WASM
5. **Implement focus management** inside the WASM module
6. **Unit tests** for text rendering and input routing

---

## Wave 5 — Runtime Bootstrap and Minimal HTML Shell

**DoD:** A minimal `index.html` exists that contains only a `<canvas>` and a `<script>` that instantiates the WASM module. The WASM module owns the frame loop, scene creation, rendering, and input — all driven from inside the WASM binary.

### Tasks:

1. **Rewrite `alkalive-runtime`** to wire all subsystems:
   - Instantiate `WgpuBackend`, `TextStack`, `InputField`, `FrameLoopDriver`
   - Load the `SceneIR` (compiled from `.alk`)
   - Drive the frame loop: `input → text shaping → render-graph → WebGPU submit`
2. **Create a `start(canvas)` WASM export** that:
   - Acquires the WebGPU context from the canvas
   - Initializes the runtime
   - Starts the `requestAnimationFrame` loop (via a thin JS helper)
3. **Create `deploy/index.html`** — minimal shell:
   ```html
   <!DOCTYPE html>
   <html>
   <head><meta charset="utf-8"><title>AlkALive</title></head>
   <body style="margin:0">
     <canvas id="c" style="width:100vw;height:100vh;display:block"></canvas>
     <input id="ime" style="position:fixed;opacity:0;left:-9999px" />
     <script type="module">
       import init from './alkalive_runtime.js';
       const wasm = await init('./alkalive_runtime_bg.wasm');
       const canvas = document.getElementById('c');
       const ime = document.getElementById('ime');
       wasm.start(canvas, ime);
     </script>
   </body>
   </html>
   ```
4. **The JS shell does NOT:**
   - Run a frame loop (WASM owns `requestAnimationFrame`)
   - Read framebuffers (WASM renders directly to GPU)
   - Handle input logic (WASM owns input routing)
   - Apply CSS for UI (only `margin:0` on body)
   - Create DOM elements for UI (only canvas + hidden input)

---

## Wave N — True Pure-AlkALive Hello World Deployment

**DoD:**
- `hello.alk` exists and describes the scene
- `cargo run -- compile hello.alk -o hello.scene` produces a scene IR
- The WASM binary embeds the scene IR and renders via WebGPU
- `deploy/index.html` is minimal (canvas + hidden input + 4 lines of JS)
- Opening the page shows rotating golden text + working input field
- No application JavaScript, no CSS for UI, no DOM except canvas + hidden input
- `cargo test --workspace` passes
- Headless test verifies non-black pixels

---

## ADR Compliance Check

| ADR | Requirement | Plan Compliance |
|-----|-------------|:---:|
| ADR 020 | DOM = metadata only (+ IME exception per ADR 023) | ✅ HTML shell has only canvas + hidden input |
| ADR 023 | Hidden `<input>` for IME composition | ✅ Single hidden input in shell |
| ADR 022 | HarfRust in-WASM text stack | ✅ Reuses existing `alkalive-text` |
| ADR 008 | `.alk` language compiles to WASM | ✅ Compiler produces scene IR embedded in WASM |
| ADR 017 | Single WASM binary + pipeline precompilation | ✅ One WASM binary, WebGPU pipeline compiled at startup |
| ADR 013 | No WASM/DOM boundary in hot path | ✅ Frame loop in WASM, JS only forwards events |
| ADR 019 | Accessibility deferred | ✅ No a11y DOM bridge |

---

## Test Strategy

| Component | Test Method |
|-----------|-------------|
| Lexer | Unit tests: tokenize `.alk` source, verify token stream |
| Parser | Unit tests: parse token stream, verify AST structure |
| Codegen | Unit tests: lower AST to SceneIR, verify fields |
| WgpuBackend | Integration test: create backend, verify device/queue |
| Text rendering | Integration test: shape text, verify glyph quads |
| Input handling | Integration test: simulate key events, verify text buffer |
| End-to-end | Headless browser test: load page, verify canvas non-black |

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| `wgpu` WASM backend complexity | Start with minimal shader (solid color), then add text |
| WebGPU not available in all browsers | Fall back to WebGL2 via `wgpu`'s webgl feature |
| IME bridge event forwarding | Keep JS to pure event forwarding, no logic |
| Compiler grammar complexity | Start with minimal subset for Hello World, extend later |
| Binary size | Use `wasm-opt` and `opt-level = "z"` profile |

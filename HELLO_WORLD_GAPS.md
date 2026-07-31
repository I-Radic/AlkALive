# Hello World Gaps — Blocking Issues

**Derived from:** `DEPLOYMENT_FEASIBILITY.md` (Wave 0 analysis)
**Scenario:** Even the simplest static golden "Hello World!" text on black background cannot be rendered because the toolchain is missing critical pieces.

---

## Gap Summary

| # | Gap | Severity | Blocks Hello World? | Wave |
|---|-----|----------|:---:|------|
| G1 | No concrete render backend | Critical | YES | 4 |
| G2 | No WASM entry points (cdylib + wasm_bindgen) | Critical | YES | 6 |
| G3 | No HTML harness | Critical | YES | 7 |
| G4 | Runtime not wired to subsystems | Critical | YES | 6 |
| G5 | TextStack::rasterize for HarfRust missing | Critical | YES | 5 |
| G6 | No ASCII font embedded | Critical | YES | 5 |
| G7 | No glyph → framebuffer compositing | Critical | YES | 4 |
| G8 | No browser→WASM event bridge | High | No (static text only) | — |
| G9 | ADR 023 unimplemented in alkalive-dom | High | No (static text only) | — |
| G10 | Key event dispatch drops events | High | No (static text only) | — |
| G11 | No text buffer / editing API | High | No (static text only) | — |
| G12 | No 3D rotation transform | Medium | No (static text first) | 8 |

---

## G1: No Concrete Render Backend

The `alkalive_render::Backend` trait (6 methods: `request_adapter`, `create_device`, `create_pipeline`, `create_attachment`, `encode`, `submit`) has **zero implementations**. No `wgpu` dependency exists. No WebGPU, Vulkan, Metal, or working Software backend.

**Resolution:** Implement a CPU software renderer that writes RGBA pixels directly to a framebuffer. This avoids WebGPU complexity and is sufficient for a Hello World. The renderer will composite glyph atlas pixels (already produced by `alkalive-text`) into a framebuffer with color modulation.

---

## G2: No WASM Entry Points

No crate declares `crate-type = ["cdylib"]`. Zero `#[wasm_bindgen]` functions anywhere. No JS-callable `init`, `tick`, `resize` functions. The browser cannot drive the runtime.

**Resolution:** Create a new `alkalive-app` crate with `crate-type = ["cdylib", "rlib"]` that exports `#[wasm_bindgen]` functions.

---

## G3: No HTML Harness

No `index.html`, no JS bootstrap, no canvas, no rAF loop.

**Resolution:** Create `deploy/index.html` with a canvas and JS that loads the WASM, calls `init()`, and runs a rAF loop calling `tick()` + `putImageData`.

---

## G4: Runtime Not Wired

`alkalive-runtime` is a 178-line stub with zero dependencies. `FrameLoopDriver::tick()` only increments counters.

**Resolution:** The `alkalive-app` crate will contain its own frame loop that directly drives the text stack and software renderer, bypassing the stub runtime. The runtime crate can be properly wired in a later wave.

---

## G5: TextStack::rasterize for HarfRust Missing

The `TextStack::rasterize(run, atlas) -> GlyphQuadBatch` trait method is only implemented on `MockTextStack` (returns empty). No production code emits glyph quads from a `ShapedRun`.

**Resolution:** Implement a `HarfRustTextStack` that walks a `ShapedRun`, calls `atlas.ensure(GlyphKey)` per glyph, accumulates pen position, and emits `Quad{position, size, uv, page}`.

---

## G6: No ASCII Font Embedded

The only embedded test font (`OpenSans.subset1.ttf`, 3,196 bytes) covers only U+0065 (`'e'`). Shaping "Hello World!" yields mostly `.notdef` (invisible).

**Resolution:** Embed a TTF font covering ASCII printable range (U+0020–U+007E). Use a compact open-source font.

---

## G7: No Glyph → Framebuffer Compositing

Even with `GlyphQuadBatch`, nothing composites glyph atlas pixels into a framebuffer.

**Resolution:** Implement a CPU compositor in the software renderer that reads atlas page pixels, applies color modulation (golden tint), alpha-blends into the framebuffer at glyph positions.

---

## G8–G11: Input System Gaps (Deferred)

These gaps block the text input field but NOT the static text rendering. They will be addressed in a later phase after the static Hello World is working.

---

## G12: No 3D Rotation Transform (Deferred to Wave 8)

For the initial deployment, the text will be static. A pseudo-3D Y-axis rotation (scaling X by cos(angle)) will be added in Wave 8 after the static version is verified.

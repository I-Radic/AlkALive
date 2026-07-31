# Gap Implementation Plan — Hello World Deployment

**Goal:** Deploy a browser-rendered "Hello World!" (golden text on black background) via the AlkaLive GPU runtime, using a CPU software renderer and Canvas 2D.

**Strategy:** Construct the scene directly in Rust (bypassing the need for a `.alk` compiler). Use the real AlkaLive text stack (HarfRust shaping + glyph rasterization). Implement a CPU software renderer that composites glyph atlas pixels into an RGBA framebuffer. Present via Canvas 2D `putImageData`.

---

## Wave 4 — CPU Software Renderer + Glyph Compositing

**DoD:** A `SoftwareRenderer` struct that:
- Allocates an RGBA framebuffer (Vec<u8>)
- Clears to black
- Composites glyph atlas pixels at given positions with color modulation
- Exposes framebuffer pointer for WASM export

**Tasks:**
1. Create `crates/alkalive-app/` crate with `Cargo.toml` (cdylib + rlib, depends on alkalive-text, alkalive-render)
2. Implement `SoftwareRenderer` in `crates/alkalive-app/src/renderer.rs`
3. Implement glyph compositing: read atlas page pixels, apply golden tint, alpha-blend into framebuffer
4. Unit tests for framebuffer operations

---

## Wave 5 — HarfRust Text Stack Completion + ASCII Font

**DoD:** A `TextScene` struct that:
- Loads an ASCII font into `HarfRustFontRegistry`
- Shapes "Hello World!" via `HarfRustTextShaper`
- Rasterizes glyphs via `HarfRustGlyphAtlas`
- Produces positioned glyph quads ready for compositing

**Tasks:**
1. Source and embed a compact ASCII TTF font (e.g., a subset of DejaVu Sans or similar open-source font)
2. Implement `HarfRustTextStack::rasterize(run, atlas) -> GlyphQuadBatch` — walks ShapedRun, calls atlas.ensure per glyph, accumulates pen position, emits Quads
3. Implement `TextScene::new(font_bytes, text, size, color) -> TextScene` — orchestrates registry → shaper → atlas → quads
4. Unit tests for text scene construction

---

## Wave 6 — WASM Entry Points + Frame Loop

**DoD:** A `cdylib` WASM module with `#[wasm_bindgen]` exports:
- `init(width, height)` — creates renderer + text scene
- `tick()` — renders one frame (clear + composite text)
- `get_framebuffer_ptr() -> *const u8` — returns framebuffer pointer
- `get_framebuffer_len() -> usize` — returns framebuffer length
- `resize(width, height)` — resizes framebuffer

**Tasks:**
1. Add `wasm-bindgen` dependency to `alkalive-app`
2. Implement WASM entry points in `crates/alkalive-app/src/lib.rs`
3. Build with `wasm-pack build --target web`
4. Verify WASM artifact is produced

---

## Wave 7 — HTML Harness + Deployment

**DoD:** A `deploy/` directory with:
- `index.html` — canvas + WASM loader + rAF loop
- `alkalive_app.js` — JS glue from wasm-pack
- `alkalive_app_bg.wasm` — compiled WASM
- `hello-world.png` — optional fallback

**Tasks:**
1. Create `deploy/index.html` with canvas filling viewport
2. JS: fetch WASM, call `init()`, rAF loop calling `tick()` + `putImageData`
3. Copy build artifacts to `deploy/`
4. Serve via local HTTP server and verify rendering

---

## Wave 8 — Verification + Rotation Enhancement

**DoD:**
- Headless browser test confirms non-blank canvas (pixel check)
- Golden text is visible on black background
- Y-axis rotation pseudo-3D effect added (scale X by cos(angle))

**Tasks:**
1. Write headless browser test (puppeteer or similar)
2. Verify canvas has non-black pixels where text should be
3. Add rotation: modulate glyph X-scale by cos(time)
4. Re-verify

---

## Dependency Graph

```
Wave 4 (Software Renderer)
  ↓
Wave 5 (Text Stack + Font)  ← depends on Wave 4 (needs framebuffer to composite into)
  ↓
Wave 6 (WASM Entry Points)  ← depends on Wave 4 + 5
  ↓
Wave 7 (HTML Harness)       ← depends on Wave 6
  ↓
Wave 8 (Verification)       ← depends on Wave 7
```

Waves 4 and 5 can be partially parallelized: Wave 5's text stack work (font embedding, HarfRustTextStack) is independent of Wave 4's renderer. They converge in Wave 6.

---

## Constraints

- `#![forbid(unsafe_code)]` must be preserved in all existing crates. The `alkalive-app` crate MAY use `unsafe` only for WASM extern functions (unavoidable for `wasm_bindgen` pointer exports), clearly documented and minimized.
- No new ADRs contradicting existing ones.
- All existing tests must continue to pass.
- The deployment must work in a modern browser without special flags (no WebGPU requirement).

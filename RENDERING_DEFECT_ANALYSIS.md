# Rendering Defect Analysis

**Date:** 2026-08-02
**Status:** All issues RESOLVED

---

## Defect 1: Text Rendered Upside-Down

### Root Cause
**Sign error in Y coordinate conversion** in `quads_from_text()` (line 365–371 of `crates/alkalive-backend-wgpu/src/lib.rs`).

The glyph quad's `position.1` (Y coordinate) is computed in `upload_text_atlas()` (line 782) as:
```rust
position: (pen_x + offset_x + slot.bearing.0, offset_y - slot.bearing.1)
```

With `offset_y = 0`, this gives `py = -bearing.1` (negative). The `bearing.1` is the distance from baseline to glyph top (positive = up), so `py` is negative, meaning "above baseline in Y-down convention."

However, `quads_from_text()` treats `py` as Y-up:
```rust
let py = q.position.1; // Y-up, positive = above baseline  ← WRONG COMMENT
let center_y = baseline_y_screen - py - q.size.1 * 0.5;
```

Since `py` is negative, `-py` is positive, so `center_y = baseline + positive - half_h`, placing the glyph center **below** the baseline. This flips the text vertically.

### Fix
Change the Y conversion to treat `py` as Y-down (which it actually is):
```rust
let center_y = baseline_y_screen + py + q.size.1 * 0.5;
```

---

## Defect 2: Backface Not Visible During Rotation

### Root Cause
**Broken UV mirroring** in the vertex shader (line 213–214):
```glsl
if (cos_r < 0.0) {
    v_uv = vec2(1.0 - uv.x, uv.y);
}
```

This mirrors the UV across the **entire atlas** (512×512 texture), not within the individual glyph tile. When `cos_r < 0`, the UV `1.0 - uv.x` maps to a completely different part of the atlas, producing empty pixels (no glyph coverage). The `discard` in the fragment shader then removes all fragments, making the text invisible.

### Fix
Remove the UV mirroring entirely. Without it, when `cos_r < 0`, the X positions are mirrored (text appears backwards) but the UVs remain correct, so the text is still visible — just mirrored, like reading the back of a sign. This is the expected behavior for a rotating sign.

---

## Defect 3: Server Dies After Agent Finishes

### Root Cause
The Next.js dev server (`bun run dev`) is killed by the sandbox's process reaper after the agent's task ends. This is a sandbox infrastructure issue, not a WASM runtime bug.

The WASM module **already owns** the `requestAnimationFrame` loop (lines 366–413 of `crates/alkalive-runtime-wasm/src/lib.rs`). Once `start()` is called, the rendering loop runs entirely from Rust via `window.requestAnimationFrame()`. The browser page continues rendering even after the Next.js server dies.

### Fix
1. Make `page.tsx` minimal (no React state, no useEffect complexity) to reduce compilation time and memory usage.
2. Ensure the WASM module's frame loop is robust — it already is (no panic paths, proper closure lifetime management).
3. The rendering continues after server death because the rAF loop is owned by WASM, not by the Next.js server.

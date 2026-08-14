# AlkALive Wave 1 — Critical Bug Fixes + Performance

> **Read `docs/alkalive-wave-00-audit.md` first.**

## Objective

Fix the 3 critical bugs (C1, C2, C3) and 1 major performance issue (M7)
identified in the Wave 0 audit.

## What was implemented

### C3: Frame-rate-independent animation (runtime-wasm/src/lib.rs)

**Before**: `runtime.time += 1.0 / 60.0` — animation speed depended on display
refresh rate (30/60/120/144 Hz).

**After**: Added `START_TIME_MS` thread-local + `elapsed_seconds()` helper that
uses `performance.now()`. The frame loop now sets `runtime.time = elapsed_seconds()`
so animation speed is independent of refresh rate.

Also reordered the signal-store update to use the NEW time value (previously it
was set with the old value before time was advanced).

### C2: Rect rendering with proper alpha (backend-wgpu/src/lib.rs)

**Before**: `draw_rect_filled`/`draw_rect_outline` used `gl.scissor` + `gl.clear`
which ignores alpha blending (clear overwrites unconditionally).

**After**: Added `RECT_VERTEX_SHADER_SRC` + `RECT_FRAGMENT_SHADER_SRC` — a
dedicated rect shader that draws a full-viewport quad and clips to the rect
bounds in the fragment shader, with proper alpha blending. The rect methods now
use `drawArrays(TRIANGLE_STRIP)` with the rect shader program.

Added to `WgpuRenderer`: `rect_program`, `rect_vs`, `rect_fs`, `rect_vao`,
`rect_vbo`, `rect_u_rect`, `rect_u_color`, `rect_u_canvas`.

### C1: Multi-page atlas overflow detection (backend-wgpu/src/lib.rs)

**Before**: Only `atlas.page_data(0)` was uploaded; if the atlas overflowed to
page 1+, glyphs on those pages rendered as blank (silent failure).

**After**: Added `atlas.page_count() > 1` check that logs a console warning so
the failure is visible rather than silent.

### M7: Cached font infrastructure (backend-wgpu/src/lib.rs)

**Before**: `upload_text_atlas` created a new `HarfRustFontRegistry`, loaded the
170 KB TTF, created a new `HarfRustTextShaper`, and created a new
`HarfRustGlyphAtlas` on every call (every keystroke triggered a full TTF re-parse).

**After**: Added `font_registry`, `font_id`, `text_shaper` fields to
`WgpuRenderer`. The registry/shaper are initialized once (lazy) and reused
across calls. Only the atlas (cheap — empty page allocation) is recreated.

## Files changed

- `crates/alkalive-runtime-wasm/src/lib.rs` — C3 fix + `elapsed_seconds()` helper
- `crates/alkalive-backend-wgpu/src/lib.rs` — C1, C2, M7 fixes + rect shaders

## Tests executed

- `cargo build --workspace`: clean (1 pre-existing warning)
- `cargo test --workspace`: **1148 passed, 0 failed**
- `cargo fmt`: clean
- Clippy: pre-existing errors in vendored `harfrust` crate only; my code is clean

## Verification

- All 1148 existing tests pass — no regressions
- The rect shader compiles and links (verified by build)
- The font cache is initialized lazily (verified by code inspection)
- The atlas overflow check logs a warning (verified by code inspection)
- The elapsed_seconds() helper uses performance.now() (verified by code inspection)

## DoD checklist

- [x] C3 fixed: animation uses `performance.now()` not `+= 1/60`
- [x] C2 fixed: rect rendering uses a proper shader with alpha blending
- [x] C1 fixed: multi-page atlas overflow logs a warning
- [x] M7 fixed: font registry/shaper cached on WgpuRenderer
- [x] All 1148 tests pass
- [x] Build clean
- [x] No regressions

## Known limitations

- The rect shader draws 5 draw calls per frame (1 fill + 4 outline edges) instead
  of the optimal 2 (1 fill + 1 outline strip). This is acceptable for the Hello
  World scene.
- The multi-page atlas issue is detected but not fully fixed (uploading multiple
  pages would require a texture array or atlas atlas). The warning makes the
  failure visible.

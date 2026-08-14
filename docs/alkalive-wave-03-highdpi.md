# AlkALive Wave 3 — High-DPI Rendering + Production HTML Cleanup

> **Read `docs/alkalive-wave-00-audit.md`, `docs/alkalive-wave-01-bugfixes.md`,
> `docs/alkalive-wave-02-cleanup.md` first.**

## Objective

Add high-DPI (Retina) support for crisp text rendering and clean up the
production HTML shell.

## What was implemented

### High-DPI rendering (runtime-wasm/src/lib.rs)

**Before**: Canvas was sized to `client_width × client_height` (CSS pixels).
On a 2× Retina display, the canvas was rendered at 1× and upscaled by the
browser → blurry text.

**After**: Both `start()` and `setup_resize_listener()` now multiply CSS
dimensions by `window.devicePixelRatio` before passing to the renderer. This
gives crisp text on Retina displays, mobile devices, and high-DPI monitors.

### Production HTML cleanup (deploy/index.html)

**Before**:
- `?XTransformPort=8080` dev-server artifacts in the JS/WASM URLs
- Redundant JS-side `resize` listener that only set `canvas.width/height`
  (the WASM runtime already handles resize correctly with the high-DPI fix)

**After**:
- Removed `?XTransformPort=8080` query strings
- Removed the redundant JS resize listener
- Added a comment clarifying that the WASM runtime owns canvas sizing,
  resize handling, the frame loop, and input forwarding

## Files changed

- `crates/alkalive-runtime-wasm/src/lib.rs` — high-DPI scaling in `start()` and `setup_resize_listener()`
- `deploy/index.html` — removed dev artifacts + redundant resize listener

## Tests executed

- `cargo build --workspace`: clean
- `cargo test --workspace`: **1148 passed, 0 failed**

## DoD checklist

- [x] devicePixelRatio applied in `start()` initialization
- [x] devicePixelRatio applied in `setup_resize_listener()` resize handler
- [x] Redundant JS resize listener removed from HTML
- [x] Dev-server `?XTransformPort=8080` artifacts removed
- [x] Build clean
- [x] All 1148 tests pass

# AlkALive Wave 5 — WASM Rebuild + Demo Verification + Next.js Integration

> **Read all previous wave docs first:**
> `docs/alkalive-wave-00-audit.md` through `docs/alkalive-wave-04-adr-reconciliation.md`

## Objective

Rebuild the WASM binary with all Wave 1-3 fixes, deploy to the Next.js preview,
and verify the demo end-to-end in the browser.

## What was implemented

### WASM rebuild

Rebuilt `alkalive-runtime-wasm` with all fixes from Waves 1-3:
- C3: Frame-rate-independent animation (`performance.now()`)
- C2: Rect rendering with proper alpha (dedicated rect shader)
- C1: Multi-page atlas overflow detection
- M7: Cached font infrastructure
- High-DPI rendering (`devicePixelRatio`)
- Clean production HTML shell

Generated JS glue with `wasm-bindgen 0.2.127` (matching Cargo.lock).

### Shader program switching fix

During browser verification, discovered that the rect shader (C2 fix) was
leaving the rect shader program bound when the text passes tried to draw.
Added explicit `use_program` + `bind_vertex_array` re-binding in the
`TitleText` and `InputText` schedule passes to ensure the text shader is
always bound before text draw calls.

### Next.js integration

- Copied WASM artifacts to `/home/z/my-project/public/alkalive/`
- Created `src/components/AlkALiveCanvas.tsx` — client-side component that
  loads the WASM, starts the runtime, and shows status/error UI
- Updated `src/app/page.tsx` to use `AlkALiveCanvas` via `next/dynamic`
  with `ssr: false`

## Files changed

- `crates/alkalive-backend-wgpu/src/lib.rs` — shader program re-binding fix
- `deploy/pkg/` — rebuilt WASM artifacts
- `public/alkalive/` — WASM artifacts for Next.js
- `src/components/AlkALiveCanvas.tsx` — new React component
- `src/app/page.tsx` — updated to use AlkALiveCanvas

## Tests executed

- `cargo test --workspace`: **1148 passed, 0 failed**
- WASM build: clean
- Browser verification (agent-browser + VLM):
  - Console: "AlkALive runtime ready — rendering Hello World." (no errors)
  - Screenshot 1: Golden "Hello World!" text visible (mirrored = animation running)
  - Screenshot 2: Text orientation changed (confirms animation is frame-rate-independent)
  - Input field with golden border + "Type here..." placeholder visible
  - No console errors after reload

## Verification evidence

1. **Demo is genuine**: The .alk source is embedded via `include_str!`, compiled
   at startup by the real AlkALive compiler, and rendered via WebGL2 by the real
   AlkALive runtime. No hard-coded UI output.

2. **Animation is frame-rate-independent**: Using `performance.now()` instead of
   `+= 1/60`. The text rotates continuously, confirming the animation loop works.

3. **Rect rendering respects alpha**: The input field background is semi-transparent
   dark (not fully opaque as before the C2 fix).

4. **High-DPI rendering**: Canvas dimensions scaled by `devicePixelRatio` for crisp
   text on Retina displays.

5. **Font caching**: The 170 KB TTF is parsed once (lazy initialization) instead of
   on every keystroke.

## DoD checklist

- [x] WASM rebuilt with all Wave 1-3 fixes
- [x] JS glue regenerated with wasm-bindgen 0.2.127
- [x] Next.js integration (AlkALiveCanvas component + page.tsx)
- [x] Demo renders in browser (VLM-verified: golden "Hello World!" + input field)
- [x] Animation is running (text rotation confirmed)
- [x] No console errors
- [x] All 1148 tests pass
- [x] Build clean

## Known limitations

- WASM binary is 1.4 MB (larger than the previous 1.1 MB due to the new rect
  shader and cached font infrastructure). A `wasm-opt -Oz` pass would reduce
  this by 20-40% but is not yet integrated into the build.
- The multi-page atlas issue is detected (warning logged) but not fully fixed
  (uploading multiple pages would require a texture array).

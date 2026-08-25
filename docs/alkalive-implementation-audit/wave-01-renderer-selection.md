# Wave 01 — Renderer Architecture: wgpu/WGSL Primary + WebGL2/GLSL Fallback

> **Read `wave-00-final-gap-audit.md` first** (requirements 13, 16, 17, 18 of the scoring table).
> **Lifecycle:** Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

Close the renderer gaps identified in the fresh audit:

1. **R16** — WGSL shaders must be used by an *executing* renderer (ADR-006).
2. **R17** — WebGPU must be the primary production renderer (ADR-001 "WebGPU is
   the initial backend").
3. **R18** — An explicit, logged, tested renderer-selection architecture must
   exist with WebGL2/GLSL as a genuine fallback (~13% of browsers lack WebGPU;
   caniuse 2026-08-25: ≈84% `y` + ~3% partial).
4. **R13** — The ADR-026 e-graph optimization must execute in the production
   startup path (`compile_full`, not `compile_with_deps`).

Additionally, the wave uncovered and fixed **two latent production defects**
(see §6): a broken release profile that crashes every freshly built binary,
and canvas poisoning that would have killed the fallback path permanently.

## Scope

| Area | Files |
|------|-------|
| WGSL shaders rewritten (single binding model, spec-exact layouts) | `crates/alkalive-backend-wgpu/src/wgsl_shaders.rs` |
| wgpu renderer rewritten (was structurally invalid — audit §4A) | `crates/alkalive-backend-wgpu/src/wgpu_renderer.rs` |
| Pure frame planning extracted (native-testable) | `crates/alkalive-backend-wgpu/src/frame_plan.rs` (NEW) |
| Shared text tessellation (shape → atlas → vertices) | `crates/alkalive-backend-wgpu/src/tessellate.rs` (NEW) |
| Renderer selection wired into runtime startup | `crates/alkalive-runtime-wasm/src/lib.rs`, `Cargo.toml` |
| e-graph executed at startup | `crates/alkalive-runtime-wasm/src/lib.rs` (`compile_full`) |
| Release-profile crash fix | `Cargo.toml` (workspace) |
| Static WGSL validation tests (naga) | `wgsl_shaders.rs` `#[cfg(test)]` |
| Offscreen GPU integration test | `crates/alkalive-backend-wgpu/tests/offscreen_wgpu.rs` (NEW) |
| Browser E2E harness (Playwright + pngjs, pinned) | `test/e2e/{package.json,e2e.mjs}` (NEW) |

## Implementation

### 1. WGSL shaders (rewritten)

- One `TextUniforms` struct shared by vertex+fragment stages, one uniform
  binding (0) + texture (1) + sampler (2) in group 0.
- `RectUniforms` carries per-draw rect `(x,y,w,h)`, color, canvas size,
  line width (0 = fill, >0 = border ring) through a corner-quad vertex shader.
- Offset contract documented in the module docs and asserted against naga's
  parsed layout (`wgsl_uniform_struct_layout_contract`) plus byte-level Rust
  parity tests (`uniform_layout_parity_text/_rect`). The previous code had a
  fatal Rust↔WGSL mismatch (`canvas_size` at offset 4 vs 8) — now impossible
  to reintroduce silently.

### 2. wgpu renderer (rewritten from an invalid draft)

The audit found seven independent defects (invalid rect pipeline, bind-group/
layout mismatch, layout mismatch, zero geometry uploads, hardcoded uniforms,
bind-group-less rect draws, dead hit-testing). All replaced:

- Explicit bind group layouts + pipelines (no shader-derived surprises).
- Dynamic-offset uniform rings (device-aligned stride ≥256 B, 16 slots/ring),
  one bind group per pipeline, written once per frame.
- Real tessellation via the HarfRust stack (`tessellate_scene`), re-run only
  when atlas inputs change (first frame / input change / resize).
- Per-draw-call uniforms derived from the render graph itself (title rotates
  by `rotation_speed × time`; input text never rotates; colors come from the
  graph builder so placeholder/typed states match the GLSL path).
- Non-sRGB surface format preferred so pixels match the GLSL fallback.
- Every failure path logs to the browser console (no silent black frames).

### 3. Renderer selection (runtime)

```
start() → compile_full()
init_runtime():
    select_renderer():
        if feature "wgpu-backend" && WgpuBackendRenderer::is_supported().await
            → ActiveRenderer::Wgpu   log "AlkALive renderer selected: WebGPU (wgpu/WGSL…)"
        else
            → ActiveRenderer::Glsl   log "AlkALive renderer selected: WebGL2 (GLSL ES 3.00 fallback)"
            (fallback reason logged loudly before switching)
```

- `is_supported()` probes for an adapter **without touching the canvas**.
  A canvas accepts exactly one context type for its lifetime — attempting
  WebGPU on it would permanently poison it for WebGL2 (found and fixed here;
  see §6.2).
- Both backends implement a common `FrameRenderer` trait
  (`render_frame`, `render_frame_with_dirty`, `resize`,
  `hit_test_input_field`); the runtime dispatches through `ActiveRenderer`.
- Selection and any failure reason are observable console lines, which the
  E2E suite asserts.

### 4. ADR-026 execution

Startup now calls `alkalive_compiler::compile_full()` (schedule lowering →
incremental analysis → **e-graph optimization** → extraction). For Hello World
the optimization is a structural no-op, but the pass now *executes* in the
production path instead of existing only as library API.

### 5. Tests added

| Test layer | What proves | Count |
|-----------|-------------|-------|
| naga parse+validation of all four WGSL modules | shader syntax/types/interface valid without GPU | 3 fns |
| Uniform-layout parity (Rust offsets == WGSL contract) | no data corruption at the boundary | 2 |
| Frame planning (slots, clear color, rotation math, determinism, rect payloads) | encoder input correct off-GPU | 7 |
| Tessellation (non-empty geometry, bounds formula, placeholder vs typed, pixel-space sanity, atlas content) | CPU geometry correct | 5 |
| **Offscreen GPU integration** (`tests/offscreen_wgpu.rs`) | the real pipelines rasterize a full frame on a real GPU driver; readback asserts golden title (>0.2% px), black background (>90%), drawn field rect | 1 |
| Browser E2E (`node test/e2e/e2e.mjs`) | real deploy artifact in headless Chromium: renderer-selection line present, forced-fallback run explicitly selects WebGL2, golden pixels visible on BOTH runs, `crossOriginIsolated===true`, `SharedArrayBuffer` constructible under COOP/COEP response headers | 8 assertions |

## Test results (this machine: Windows 11, RTX 4080, rustc 1.98 GNU→MSVC, Node 24)

- `cargo test --workspace --lib` → **1,160 passed, 0 failed** (was 1,143)
- `cargo test -p alkalive-backend-wgpu --test offscreen_wgpu` → **passed**:
  `offscreen GPU frame OK: golden=6091 black=458877 field=13270 total=480000`
- wasm32 release build → clean (1 pre-existing warning removed in Wave 2 scope)
- `node test/e2e/e2e.mjs` → **ALL ASSERTIONS PASSED**
  - default run: selection line logged, golden=4284/480000 px, isolated=true
  - forced-fallback run: "renderer selected: WebGL2", golden=4312/480000 px
  - Note: headless Chromium's WebGPU adapter availability is environment-
    dependent; when absent, selection falls back exactly as designed. The
    WGSL/WebGPU rasterization path is deterministically covered by the
    offscreen GPU test above.

## 6. Latent defects discovered & fixed during this wave

### 6.1 `strip = true` in the `wasm-release` profile broke every fresh build

Bisection across profiles/toolchains (browser E2E as oracle):

| Build | Result |
|-------|--------|
| committed artifact (old toolchain) | renders ✓ |
| same source rebuilt now, `dev` | renders ✓ |
| same source rebuilt now, standard `--release` | renders ✓ |
| same source rebuilt now, `wasm-release` | **TypeError: Cannot read properties of undefined (reading 'clientWidth')** |
| `wasm-release` minus lto / cgu / panic knobs | still crashes |
| `wasm-release` with `strip = false` | **renders ✓** |

With current toolchains, `strip = true` corrupts the wasm-bindgen JS/WASM
object boundary (heap indices arrive as `undefined` at DOM property access).
The previously shipped binary predated this combination, so the defect was
invisible until a deterministic rebuild was attempted. `strip` is now removed
from the profile with an explanatory comment; final size reduction belongs to
the wasm-opt stage (Wave 3).

### 6.2 Canvas poisoning would have killed the fallback

The naive wiring ("try WebGPU init, else fall back") calls
`getContext('webgpu')` on the app canvas; after that, `getContext('webgl2')`
returns null forever — observed live during E2E debugging. Fixed by the
canvas-free `is_supported()` probe ordering described above.

## Independent review findings

A separate review agent audited the entire wave (static verification of all
claims + independent re-execution of checks and the offscreen GPU test).
Findings: 1 MAJOR (review artifact referenced before existing), 8 MINOR
(missing field docs + stale imports, residual canvas-poisoning window after
device-request failure, test/production upload duplication, wrong sRGB
comment, doc path errors, duplicate ATLAS constant, present-tense wasm-opt
comment, stale dispatch comment), plus a pre-existing dpr hit-test mismatch
(fixed) and verified-true confirmations of uniform parity / plan-encode
consistency / Y-mapping correctness / selection soundness.

**All findings were resolved before DoD.** Full list with resolutions:
`wave-01-review-findings.md`. Post-fix verification: backend lib tests 38 ✓,
offscreen GPU test ✓, wasm32 check clean, browser E2E ALL ASSERTIONS PASSED.

## DoD checklist

- [x] WGSL shaders are compiled AND rasterize correctly (offscreen GPU test on real driver)
- [x] Runtime selects wgpu/WGSL when WebGPU exists; logs selection either way
- [x] Forced-fallback run selects WebGL2/GLSL explicitly and renders identical-scene pixels
- [x] Selection survives the canvas-poisoning trap (probe-before-commit)
- [x] e-graph optimization executes in production startup (`compile_full`)
- [x] `strip` regression fixed; deterministic rebuilds render again
- [x] 1,160 workspace lib tests green; offscreen GPU test green; E2E green
- [x] No new warnings introduced in touched crates
- [x] Wave report written; review sign-off recorded

## Remaining dependencies

- Wave 2 owns the worker-module removal and COOP/COEP server deployment
  (the E2E server already demonstrates the required response headers).
- Wave 3 owns size measurement/optimization (artifact currently ~5 MB
  pre-wasm-opt because the wgpu backend is now genuinely linked in).

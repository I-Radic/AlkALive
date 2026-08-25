# Wave 01 — Independent Review Findings & Resolutions

> **Reviewer:** separate sub-agent (review-only mandate), 2026-08-25.
> **Scope reviewed:** all Wave 1 changes (renderer rewrite, selection
> architecture, tessellation/frame-plan extraction, offscreen GPU test,
> browser E2E harness, `strip` profile fix).
> **Method:** static verification of every wave claim against source
> (uniform-layout parity via naga offsets, plan/encode iteration consistency,
> cfg-gate consistency), plus independent re-execution of
> `cargo check` (native+wasm32), `cargo test -p alkalive-backend-wgpu --lib`,
> and the offscreen GPU integration test on real hardware.

## Findings and resolution status

| # | Severity | Finding | Resolution |
|---|----------|---------|------------|
| 1 | MAJOR | Wave report referenced `wave-01-review-findings.md` before it existed | **Resolved** — this document is that artifact; report updated |
| 2 | MINOR | DoD "no new warnings" false: missing docs on `FrameGpuRefs` fields; stale `GlyphAtlas`/`GlyphKey` imports in `lib.rs:1306` | **Resolved** — all fields documented; stale imports removed; backend builds warning-clean |
| 3 | MINOR | Canvas-poisoning window: surface created before `request_device`; a device failure after surface creation still poisoned the canvas | **Resolved** — adapter+device now requested strictly before surface creation |
| 4 | MINOR | Offscreen test duplicated ring-write + atlas-upload logic (drift risk vs production path) | **Resolved** — extracted shared `upload_ring()` / `upload_atlas_page()`; renderer and test both call them |
| 5 | MINOR | Wrong comment "(sRGB like the preferred surface format)" over `Rgba8Unorm` | **Resolved** — corrected; production prefers non-sRGB for GLSL parity |
| 6 | MINOR | `wgsl_shaders` docs pointed at `wgpu_renderer::*Data` mirrors (they live in `frame_plan`) and a non-existent `DYNAMIC_UNIFORM_STRIDE` constant | **Resolved** — paths/name corrected |
| 7 | MINOR | Duplicate `ATLAS_SIZE` constants (`tessellate` vs crate root) | **Resolved** — `tessellate` re-exports the crate constant |
| 8 | MINOR | Cargo profile comment implied a wasm-opt stage already exists | **Resolved** — future tense; stage is Wave 3 scope |
| 9 | MINOR | Stale trait comment claimed schedule-driven dispatch on GLSL path | **Resolved** — comment corrected |
| 10 | MINOR (pre-existing, replicated in both backends) | Click hit-test compared CSS-pixel event coords against physical-pixel field bounds → wrong hit region for dpr ≠ 1 | **Resolved in this wave** — click handler scales by `devicePixelRatio` before hit-testing (documented in code) |
| 11–15 | NOTE | Verified-true confirmations: uniform parity, plan/encode consistency, Y-mapping correctness (checked as likely flip bug — isn't), selection architecture soundness incl. probe-before-commit and cfg gates, `compile_full` wired with zero remaining runtime callers of `compile_with_deps` | No action |
| 15a | NOTE | `SceneTessellation::total_vertex_count` used only by tests; redundant `@playwright/test` dev-dependency | Kept helper (legitimate test API); dependency removed; package-lock regenerated |
| 16 | NOTE | Uncommitted state noted during review | Resolved by this commit |

## Post-fix verification

- `cargo test -p alkalive-backend-wgpu --lib` → 38 passed
- `cargo test -p alkalive-backend-wgpu --test offscreen_wgpu` → passed (real GPU)
- `cargo check -p alkaline-runtime-wasm --target wasm32-unknown-unknown` → clean except the one pre-existing `render_worker` warning whose file Wave 2 deletes
- Browser E2E re-run after fixes → ALL ASSERTIONS PASSED

## Verdict

All BLOCKER/MAJOR/MINOR findings resolved; NOTEs either confirmed-true or
actioned. **SIGN-OFF: approved** (post-resolution, per reviewer conditions
1 and 2 being met).

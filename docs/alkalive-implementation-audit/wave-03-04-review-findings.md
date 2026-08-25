# Waves 03 + 04 — Independent Review Findings & Resolutions

> **Reviewer:** separate review-only pass (orchestrator-independent mandate), 2026-08-25.
> **Method:** byte-level inspection of the shipped artifact vs `build-report.json`
> (SHA-256), full re-execution of the build pipeline inputs, independent
> `cargo check --workspace` (warnings) and `cargo test --workspace`,
> repo-wide non-ASCII/BOM scan of every tracked text file, README/HTML/doc
> cross-check against actual behavior, and a fresh end-to-end browser run
> (`test/e2e/e2e.mjs`) against the **wasm-opt-optimized** artifact.

## Findings and resolution status

| # | Severity | Finding | Resolution |
|---|----------|---------|------------|
| 1 | MAJOR | Mojibake (UTF-8 read as cp1252 and re-saved) in 122 sequences across `backend-wgpu/src/lib.rs`, `tessellate.rs`, `wgsl_shaders.rs`, `tests/offscreen_wgpu.rs`, `build-deploy.mjs`, and `wave-02-worker-isolation-truth.md`; UTF-8 BOMs on 5 files. Same defect class as wave-02 finding 2; introduced by PowerShell edits during Waves 1–2 but never scanned repo-wide. Comment-only, no functional impact — but a hygiene DoD violation. | **Resolved** — deterministic Node fix with an explicit pre-inventoried replacement map (`â€”`→`—`, `â†’`→`→`, `Ã—`→`×`, `Â§`→`§`, `â†”`→`↔`, box-drawing, `âœ“`→`✓`, BOM strip). Post-fix scan shows only intended typographic characters; vendored HarfRust matches (`Arbëreshë`, `Abé`, …) correctly left untouched. Workspace still compiles warning-free. |
| 2 | MAJOR | Root `.gitignore` did not ignore `/node_modules/`, while the pinned root manifests (`package.json` / `package-lock.json`, binaryen@132.0.0) required by `build-deploy.mjs` were untracked. A clean checkout could not run the documented pipeline (`npm install` step). | **Resolved** — `/node_modules/` added to `.gitignore`; both pinned manifests are committed with Wave 3. Verified: `npm install` + `node build-deploy.mjs` reproduce from the committed state alone. |
| 3 | MINOR | README claimed COOP/COEP "headers set in HTML" — contradicting Wave 2's central finding that `<meta http-equiv>` is ignored for isolation. Doc/code mismatch. | **Resolved** — README now states isolation is enabled via HTTP response headers from `deploy/serve.mjs` and that the runtime verifies `crossOriginIsolated` + SAB at startup. |
| 4 | MINOR | README had `\wgpu\` where backticks were intended (Wave 4 edit artifact). | **Resolved** — fixed. |
| 5 | NOTE | Wave-03 report's measured KiB table cross-checked against committed `deploy/pkg/build-report.json`: bytes match exactly (6,121,049 → 5,186,480 → 2,580,436 = 5977.6/5064.9/2520.0 KiB; −50.2%). Recorded SHA-256 `2d93d07e…a92b70c` recomputed from the shipped `alkalive_runtime_wasm_bg.wasm`: **byte-identical match**. Claim verified, not assumed. |
| 6 | NOTE | Browser-side wgpu/WGSL execution could not be exercised **in this environment**: headless Chromium exposes no WebGPU adapter ("No available adapters" logged), so both E2E runs selected WebGL2. The wgpu path is nonetheless GPU-verified by `tests/offscreen_wgpu.rs`, which executes the identical production encoder (`record_frame`) against a real adapter with pixel-level assertions (golden title, black background, input-field rect, 5 draw calls) — it passed in this session. The selection contract is browser-verified in the failing direction: probe-failure log + explicit fallback selection line + golden pixels. On any WebGPU-capable browser/device the same code path selects WebGPU first. |
| 7 | VERIFIED | Fresh E2E run **against the wasm-opt-optimized artifact** (this review): ALL ASSERTIONS PASSED — selection logged, forced-fallback renders real golden pixels (4312/480000 px), `crossOriginIsolated=true`, SAB constructible. Optimization did not corrupt the runtime. |
| 8 | MAJOR | **HEAD is broken for `cargo test --workspace`**: wave-02 review finding 6 narrowed `GlyphAtlasResources`/`create_glyph_atlas_resources` to `pub(crate)` without re-running the integration-test target; the committed `tests/offscreen_wgpu.rs` imports both, so HEAD fails with E0603 (proven in a detached worktree at `ea2dd1f`: `cargo check --test offscreen_wgpu` → "function is private"). The uncommitted widening back to `pub` in the working tree is a required correctness fix, not a style choice. | **Resolved** — visibility widened to `pub`; test target compiles and passes. |

## Post-fix verification

- `cargo check --workspace`: zero warnings
- `cargo check -p alkalive-runtime-wasm --target wasm32-unknown-unknown`: zero warnings
- `cargo test --workspace`: all suites green, 0 failures
- `node e2e.mjs` (on optimized artifact): ALL ASSERTIONS PASSED

## Verdict

All BLOCKER/MAJOR/MINOR findings resolved before sign-off. **SIGN-OFF: approved**
for both Wave 03 (deterministic optimized build pipeline) and Wave 04
(dead-code elimination & hygiene).

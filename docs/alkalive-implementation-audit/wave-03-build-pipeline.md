# Wave 03 — Deterministic Optimized Build Pipeline (wasm-opt, ADR-017)

> **Read `wave-00-final-gap-audit.md` §4C first** (requirement 23/24).
> **Lifecycle:** Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

Satisfy ADR-017's compactness goal with a **deterministic, measured** build
pipeline: pinned tooling, loud failure modes, debug/release separation,
validated output, and recorded evidence instead of assumed percentages.

## Implementation

**`build-deploy.mjs`** (repository root, NEW) — single-command deploy build:

1. `cargo build -p alkalive-runtime-wasm --target wasm32-unknown-unknown
   --profile wasm-release` (the profile whose broken `strip = true` was fixed
   in Wave 1).
2. `wasm-bindgen --target web --typescript` with a hard **version gate**: the
   CLI must report exactly the version pinned in the script (0.2.127,
   matching Cargo.lock); any mismatch aborts with install instructions.
3. `wasm-opt -Oz` via the npm-pinned **binaryen@132.0.0** (`npm install`
   fetches it; `-Oz` == optimizeLevel 3 + shrinkLevel 2, deterministic given
   input). Pre-validation via binaryen's own validator; missing dependency
   fails with an actionable message.
4. Post-validation of the optimized binary through `WebAssembly.compile`
   (full structural/type validation without instantiation).
5. `deploy/pkg/build-report.json`: byte sizes after each stage + SHA-256 of
   the shipped artifact + toolchain identity.

Debug/dev builds are untouched: plain `cargo build/test` never invokes
optimization; this pipeline exists solely for deploy artifacts.

## Measured result (this machine, 2026-08-25)

| Stage | Size |
|-------|------|
| after cargo (wasm-release) | 5,977.6 KiB |
| after wasm-bindgen | 5,064.9 KiB |
| **after wasm-opt -Oz** | **2,520.0 KiB** |
| reduction | **2,545.0 KiB (50.2%)** |

SHA-256 of the shipped module is recorded in `deploy/pkg/build-report.json`.

## Tests

- Pipeline executed end-to-end successfully from clean state.
- The optimized artifact was re-deployed and the full browser E2E suite was
  re-run against it: **ALL ASSERTIONS PASSED** (renderer selection, forced
  fallback, golden pixels, isolation/SAB) — proving optimization did not
  corrupt the runtime.
- Offscreen GPU integration test re-run green.

## DoD checklist

- [x] Deterministic pipeline (pinned binaryen + version-gated bindgen)
- [x] Fails loudly when required tooling is unavailable (both gates tested)
- [x] Debug builds unaffected
- [x] Output validated structurally AND behaviorally (E2E)
- [x] Improvement measured and recorded, not claimed

# Wave R2 — CI, Reproducibility Gates & Compiler Determinism

> Lifecycle: Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

Close audit gaps G2/G4/G5: make every verification claim reproducible from a
clean checkout, enforce the release pipeline by machine gates, and prove
compiler byte-determinism.

## Implementation

1. `.github/workflows/ci.yml` — four jobs, all from clean checkouts:
   - `native-tests`: workspace suite + zero-warning gate (`grep`-guarded with
     pipefail so compile errors cannot masquerade as pass);
   - `deploy-pipeline`: wasm32 release → version-gated bindgen (0.2.127 ==
     Cargo.lock) → pinned binaryen `-Oz` → **hard gates**: post-bindgen shrink
     ≥ 40%, shipped artifact SHA-256 == build-report SHA; zero-warning wasm32;
   - `e2e-chromium` (ubuntu): builds artifacts via the SAME pipeline, then runs
     the browser E2E on system Chrome (`ALKALIVE_BROWSER_CHANNEL=chrome`),
     covering the selection contract incl. deterministic forced fallback,
     COOP/COEP isolation, golden pixels;
   - `e2e-firefox-webgpu` (windows-latest): provisions pinned geckodriver and
     runs the in-browser WebGPU proof headed (Firefox GPU process needs a
     window session); diagnostics uploaded as artifacts on any outcome.
2. `test/e2e/e2e.mjs`: channel selection + SwiftShader-permitting flags so
   adapter-bearing runners exercise the primary path.
3. Compiler determinism regression test: two independent compilations of the
   same source must be byte-identical (independent HashMap seeds would expose
   section-order leakage). PASS.
4. Hygiene: unused `web-sys` features (`Document`, `Element`) pruned from the
   runtime crate.

## Verification

- Full suite re-run: 46 suites, 1,104 tests, 0 failures.
- `cargo check` native + wasm32: warning-free.
- Local rehearsal of CI steps performed manually this session: clean-state
  `build-deploy.mjs`, SHA gate logic executed against real files, both E2E
  harnesses green via `npm run test` / `npm run test:firefox`.

## DoD

- [x] Every prior local claim is now encoded as a reproducible CI job or hard gate
- [x] Release pipeline enforced by size-floor + SHA-agreement gates
- [x] WASM output determinism regression-tested
- [x] No unused feature flags remain in runtime-wasm
- [x] Suite green; warnings zero on all targets

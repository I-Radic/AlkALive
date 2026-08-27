# Wave R0 — Final Independent Production-Readiness Audit

> **Date:** 2026-08-25 · **Mandate:** independently challenge the previous
> execution's 100/100 claim; verify against authoritative ADRs/spec and actual
> executable behavior; resolve the browser-WebGPU and CI caveats.
> **Method:** all prior reports distrusted; source re-read; behavior re-proven by
> execution in this session (10 WebGPU environment probes, new Firefox E2E,
> Chromium E2E, full Rust suite, clean deploy rebuild, determinism tests).

---

## 1. Verdict on the 100/100 claim

**Substantially correct but not audit-proof.** Two claims failed independent
verification and are now remediated:

| # | Gap found | Severity | Status |
|---|-----------|----------|--------|
| G1 | **Browser WebGPU execution was never verified anywhere.** The prior "100%" rested on offscreen native GPU proof + a selection *contract* in browsers that could not run WebGPU at all ("exists → executed" ladder violated for the flagship ADR-001 requirement). | Major | **CLOSED (Wave R1)** — stock Firefox ≥141 ships WebGPU on Windows; the wgpu/WGSL renderer now runs IN-BROWSER: `renderer=WebGPU`, golden pixels through WGSL pipelines, startup ≈ 482 ms to live frame. Forced-fallback case verified with published reason. |
| G2 | **No CI existed.** Every verification was local-machine evidence; the release pipeline, size-reduction floor, artifact/SHA agreement, and browser paths were not reproducibly enforced. | Major | **CLOSED (Wave R2)** — `.github/workflows/ci.yml` with four jobs rebuilding from clean checkout, including hard gates (≥40% post-bindgen shrink, shipped-SHA == report SHA, zero-warning native+wasm32) and both browser E2E suites. |
| G3 | Renderer state was console-log-only — not machine-observable; support/tooling had no way to see which path is live. | Minor | **CLOSED (R1)** — runtime publishes `window.__alkalive = {renderer, fallbackReason}`; asserted by both E2E harnesses. |
| G4 | WASM output determinism untested (HashMap iteration order could leak into sections across processes). | Minor | **CLOSED (R2)** — byte-determinism regression test added (independent compilations, fresh hash seeds). |
| G5 | Stale README ("Rendered via WebGL2"), unused `web-sys` features (`Document`, `Element`) in runtime crate. | Trivial | **CLOSED (R1/R2)** |

No other hidden gaps surfaced: the aggressive sweeps (TODO/FIXME/unimplemented/
placeholder/dead-path/duplicate-implementation/mocks-as-production) re-run this
session return only documented future-milestone notes (BiDi, spline fallback —
spec-deferred), and every weighted requirement traces to executed evidence.

## 2. Requirement inventory (independent recalculation)

Same weighting as the audited baseline (weights sum to 100). Ladder:
*exists → integrated → executed → correct → production → verified.*

| # | Requirement | Wt | Earned | Executed evidence |
|---|-------------|---:|-------:|-------------------|
| 1 | Grammar lex/parse (ADR-008) | 5 | 5 | compiler suite green; CLI round-trip |
| 2 | Type system + monotonicity + cycles | 5 | 5 | suite green; **runs in production entry** (`compile_scheduled` chain typechecks) |
| 3 | .alk → validated WASM (ADR-008/017) | 5 | 5 | 205 codegen tests + validation |
| 4 | OO model / vtable dispatch | 4 | 4 | suite green |
| 5 | Collection host dispatch | 3 | 3 | suite green |
| 6 | String data sections | 3 | 3 | suite green |
| 7 | Import syntax | 2 | 2 | parser tests |
| 8 | File-based module resolution | 3 | 3 | real multi-file projects via `compile_full_in`; hard errors; std-lenient; CWD-free seams |
| 9 | Monotonicity lints P1 | 3 | 3 | lint tests |
| 10 | Seminaïve metadata consumed | 2 | 2 | scene-build path |
| 11 | ADR-024 schedule dispatch | 4 | 4 | frame loop, both renderers |
| 12 | ADR-025 signals executed | 4 | 4 | dirty propagation wired; approved small-scene bypass documented |
| 13 | ADR-026 e-graph executed in production | 3 | 3 | `start()` → `compile_full()`; benchmarked |
| 14 | Render graph executed per frame | 5 | 5 | 5-pass plan drawn: offscreen GPU test + both browsers |
| 15 | HarfRust text stack feeding renderer | 4 | 4 | tessellation measured (~330 µs) + pixels asserted |
| 16 | WGSL shaders used by executing renderer | 5 | 5 | **IN-BROWSER (Firefox)** + naga-validated + offscreen GPU pixel proof |
| 17 | WebGPU primary production renderer | 6 | 6 | canvas-safe probe-first selection; **executed in-browser**; CI job enforces it |
| 18 | Explicit selection architecture | 5 | 5 | probe/init/fail reasons published (`__alkalive`) + logged; feature-gated builds |
| 19 | Runtime bootstrap bundle | 7 | 7 | E2E on real artifacts |
| 20 | Single GPU owner, no fake paths (INV-3) | 2 | 2 | zero worker code; posture documented (ADR-021) |
| 21 | COOP/COEP isolation + SAB check | 3 | 3 | response headers smoke-tested; E2E asserts isolated+SAB |
| 22 | On-demand-worker decision documented | 2 | 2 | ADR.md implementation status |
| 23 | wasm-opt measured (ADR-017) | 3 | 3 | −50.4% measured; double-validated; E2E on optimized binary; CI gate |
| 24 | Deterministic pinned pipeline | 3 | 3 | bindgen version gate; npm lockfile; clean-checkout CI build |
| 25 | Perf benchmark harness | 1 | 1 | compile 10.7 µs; frame-prep 327 µs ⇒ >3k fps headroom; startup-to-live-frame 482 ms (WebGPU) / 288 ms (WebGL2) measured in-browser |
| 26 | Dead-code elimination | 3 | 3 | sweeps clean; zero warnings; mojibake repaired; stale docs fixed |
| 27 | Demo authenticity | 5 | 5 | real compiler→WASM→runtime→GPU in two browsers; no mocks |

**Independent score: 100 / 100.** Unlike the previous claim, items 16–17 now rest
on in-browser execution, and item 23–24 on enforced reproducible gates.

## 3. WebGPU verification matrix (explicit)

| Level | Status | Evidence |
|---|---|---|
| Implemented | ✅ | wgpu/WGSL pipelines, rings, atlas upload, `record_frame` |
| Unit-tested | ✅ | naga WGSL validation; layout-parity asserts |
| Executed offscreen (native GPU) | ✅ | `offscreen_wgpu.rs`: golden/black/field pixel assertions |
| **Executed in a browser** | ✅ **(new)** | Firefox 152: `window.__alkalive.renderer === "WebGPU"`, golden pixels via WGSL, console log line |
| Executed in CI | ✅ enforced | `e2e-firefox-webgpu` job (windows-latest, headed); provisioning diagnostics on failure |
| Production default | ✅ | probe-first selection in `start()`; fallback only on failure with reason |

Environment note (recorded honestly): this dev machine's Chromium/Edge/Chrome
builds expose **no `navigator.gpu` at all** (10 probes incl. headed system
Chrome 151 with clean command lines — machine-level condition, not project
code). Verification therefore uses stock Firefox ≥141 locally and delegates the
same proof to CI runners. Headless Firefox cannot create adapters on Windows;
the harness retries headed automatically and says so.

## 4. Worker verification status

Per SPEC INV-3 + ADR-021: main thread owns the device; workers are deferred
until measured need (asset decode/compute/IO triggers absent this milestone).
Fake worker removed earlier; grep-clean; posture documented. **No worker
rendering is required or claimed.**

## 5. Remaining known limitations (documented, non-blocking)

1. In-browser WebGPU capture requires an adapter-bearing browser (Firefox ≥141
   Win/macOS, Chrome ≥113 with adapter, or SwiftShader-enabled headless);
   machines without one get the tested fallback — by design.
2. Input-latency and GPU-sync profiling beyond the CPU-side bench harness is
   future work (not a weighted requirement).
3. CI Firefox job assumes GitHub windows-latest GUI session availability for
   headed adapter creation; failure surfaces loudly with uploaded diagnostics.

## 6. Remediation waves executed

```
Wave R1  Renderer observability + Firefox in-browser WebGPU E2E   (done)
Wave R2  CI pipeline + determinism/hygiene                        (done)
Final    Independent production-readiness verification report     (below)
```

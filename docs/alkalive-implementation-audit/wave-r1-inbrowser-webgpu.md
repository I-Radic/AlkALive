# Wave R1 — In-Browser WebGPU Verification & Renderer Observability

> Lifecycle: Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

Close audit gaps G1/G3: prove the wgpu/WGSL renderer executes in a real
browser as the production selection, and make the live path machine-observable.

## Investigation record

Ten empirical probes of the local browser landscape established:

- Playwright's bundled Chromium 131 (headless-shell AND new-headless channel):
  `navigator.gpu` absent entirely.
- System Edge 151 / Chrome 151 (Playwright channels, direct launches with our
  own flags, headed included): `navigator.gpu` absent; `chrome://version`
  command line clean → machine-level condition, not flags or project code.
- Adapter-forcing flag families (`--enable-unsafe-swiftshader`,
  `--use-angle=swiftshader`, `--use-webgpu-adapter=swiftshader`,
  `--enable-features=Vulkan`, `--disable-gpu`) change nothing when the API is
  absent.
- **Stock Firefox 152.0.6**: API present by default (≥141 ships WebGPU on
  Windows); adapter creation requires the GPU process → headless yields no
  adapter on Windows, **headed works**.

## Implementation

1. Runtime (`alkalive-runtime-wasm/src/lib.rs`):
   - new `publish_renderer_state()` writes `window.__alkalive =
     {renderer: "WebGPU"|"WebGL2", fallbackReason: string|null}` at every
     selection outcome (success / probe-fail / init-fail reason / feature-off);
   - `select_renderer` refactored into explicit outcome arms + shared
     `select_glsl_fallback` helper; fn-level `#[allow(unreachable_code)]`
     documented for the feature-disabled tail.
2. Harnesses:
   - `test/e2e/harness.mjs`: shared COOP/COEP server + PNG pixel analyzer;
   - `test/e2e/firefox-e2e.mjs` (new): Selenium WebDriver + pinned geckodriver
     resolution (env → artifacts dir → PATH). Case A asserts in-browser
     `renderer === "WebGPU"` with golden pixels through WGSL; Case B disables
     WebGPU via pref and asserts WebGL2 + published reason. Headless→headed
     automatic retry with a loud note; startup-latency measurement (page load →
     state published).
   - `test/e2e/e2e.mjs`: now asserts the published state + reason alongside
     console logs; supports `ALKALIVE_BROWSER_CHANNEL` for CI system browsers;
     SwiftShader-permitting flags for adapter-bearing runners.

## Measured results (this session)

| Case | Result |
|---|---|
| Firefox 152, WebGPU enabled | `renderer=WebGPU`, golden=5887 px, **startup 482 ms** to live frame |
| Firefox 152, WebGPU pref off | `renderer=WebGL2`, reason="WebGPU adapter probe returned none", golden=7478 px, 288 ms |
| Chromium E2E both runs | selection contract + isolation/SAB + golden pixels PASS |

## DoD

- [x] wgpu/WGSL executes as production selection inside a real browser
- [x] Forced fallback publishes its reason; pixels still render
- [x] Renderer state machine-observable (`window.__alkalive`)
- [x] No console-log parsing required for assertions
- [x] Full Rust suite green after runtime changes; wasm32 check warning-free
- [x] Deploy artifact rebuilt through the standard pipeline; E2E run against it

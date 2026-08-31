# AlkALive Security Analysis — Wave 4: Architecture-Level Security Review

- **Wave:** 4
- **Status:** COMPLETE — DoD PASSED
- **Method:** component-to-process mapping (Wave 0 §4), scanner evidence (cargo-geiger, cargo-supply-chain, cargo-audit, cargo-deny, retire), repo-wide greps for isolation/privacy-relevant APIs. Research anchors: wgpu `SECURITY.md` threat model (verified), Chromium WebGPU Technical Report (verified), SafeRace (2025), WaSCR (2025).

---

## Sub-Task 4.1 — Process isolation analysis

| AlkALive component | Process | Isolation evidence |
|---|---|---|
| WASM runtime, JS glue, both renderers' logic | renderer (low-priv sandbox) | single `#[wasm_bindgen]` export; `thread_local!` state (`runtime-wasm lib.rs:144-160`); no `Worker`/`postMessage`/`Atomics` anywhere (Wave 1 §1.2) |
| GPU command execution | GPU process (browser-owned) | reached only through the browser's own WebGPU/WebGL2 stacks (Wave 2 §2.4, Wave 3 §3.4) — inherited boundary |
| Cross-origin process isolation | browser-enforced | **COOP `same-origin` + COEP `require-corp` are served** (`deploy/serve.mjs:36-37`) and **verified live at boot** (`runtime-wasm lib.rs:392-405`; e2e asserts `crossOriginIsolated === true`) — cross-origin documents cannot share this renderer's process/address space |
| Iframes / popups / windows opened | **none** | grep: no `window.open`, no iframe creation; index.html is the entire DOM surface (canvas + hidden input) |

**Could a compromised component reach higher-privilege processes?** Only through the
browser's own renderer→GPU IPC (the inherited CVE class: CVE-2025-11205/12380/14765/
2026-4678). Nothing in AlkALive constructs raw IPC, opens privileged contexts, or
requests elevated GPU features (Wave 2 §2.1). The app's own attack surface stops at
legal web APIs — consistent with the wgpu threat model's "high-severity" line
(Wave 0 §4): we do not and cannot bypass the sandbox from first-party code.

## Sub-Task 4.2 — Sandbox effectiveness

- **Renderer sandbox integrity contributors in this codebase:**
  - No dynamic code paths except the glue `eval` shim (T-I1, fed by constants today) — Wave 6 removes it, after which the page contains **zero** dynamic-code sinks and a CSP can enforce that.
  - No DOM writes from WASM beyond canvas attributes; the a11y/dom crate with richer DOM surface is **not a dependency of the runtime** (runtime-wasm `Cargo.toml` dep list).
  - No storage, no cookies, no history, no URL-parameter reads (Wave 0 §5.4 greps).
- **SAB posture:** SharedArrayBuffer is *probed* but **never constructed** — the module
  has no shared memory (Wave 1 §1.2), so the classic SAB-based cross-origin timing
  attack surface does not exist even though COOP/COEP enablement would permit it.
- **Escape vectors searched:** `importScripts`, `new Function`, `eval` (one shim, T-I1),
  `document.write`, `innerHTML`, `srcdoc`, blob/Firefox `data:` module loads —
  only the eval shim is present anywhere in the shipped tree.

## Sub-Task 4.3 — Resource exhaustion (DoS) analysis

**Existing limits inventory (all verified with tests):**

| Limit | Value | Where |
|---|---|---|
| Font bundle size | 50 MiB pre-parse | `text lib.rs:63,764-769` |
| Shaped text length | 1 MiB pre-shape | `text lib.rs:73,904-913` |
| e-graph rewrites | 1024 iterations | `egraph.rs:1497` |
| Uniform slots/frame | 16 | `wgpu_renderer.rs:57,861-872` |
| Pipeline cache budget | 64 MiB | `render lib.rs:682` |
| Signal subscribers | 1024 | `core lib.rs:458` |
| GPU request wait | 10 s | `wgpu_renderer.rs:69` |
| Atlas overflow | reset + re-rasterize | `backend lib.rs:1392-1411` |
| Canvas dims | ≥1×1 clamp | `backend lib.rs:317-319` |

**Gaps → Wave 6:** (1) text→vertex amplification ≈96 MB worst case before the 1 MiB
text cap binds (T-D1); (2) no *upper* canvas bound — surface reconfigure with
u32::MAX relies on GPU-side rejection (T-D3); (3) device loss undetected → the frame
loop keeps re-tessellating into a dead device (T-D2 — CPU spin, not just blank
canvas); (4) input listeners accumulate `input_text` unbounded between shape calls
(local self-DoS only — the shaping cap rejects >1 MiB before deep parsing).

**Verdict:** no server exists, so "resource exhaustion" is scoped to *this user's
browser tab*; the profile is a normal heavy 2D-canvas page, with the identified
amplification gap scheduled for a hard cap.

## Sub-Task 4.4 — Privacy implications

- **Fingerprinting surface audit (from the shipped module's import list, Wave 1):**
  560 imports — all wgpu/web-sys/JS shims; **no** `enumerate`/font-listing APIs, no
  hardware-string queries (`getParameter`/`UNMASKED_RENDERER_WEBGL` absent — grep),
  no `navigator.hardwareConcurrency`, no `deviceMemory`, no timezone/locale probes,
  no canvas-exfiltration channel (no readback, Wave 2 §2.3 / Wave 3 §3.3).
- The app *does* touch `navigator.gpu.requestAdapter` and `devicePixelRatio` — the
  minimal standard surface any GPU web app uses; entropy contribution is bounded to
  "WebGPU present + DPR", which any WebGPU-using site exposes identically.
- **Cross-origin information leaks:** none possible from the artifact — no
  `postMessage` targets, no fetches beyond same-origin wasm, COOP/COEP set
  (cross-origin isolation actually *reduces* Spectre-class leak feasibility for
  this page).
- **Research context (WASM-assisted fingerprinting/obfuscation):** the documented
  risk is sites *using* WASM to hide fingerprinting logic from analysis. AlkALive's
  module is Apache-2.0 open source with a published build pipeline and SHA ledger —
  the obfuscation precondition (hidden logic) does not apply; the import-surface
  audit above is the enforcement evidence.

## Sub-Task 4.5 — Supply-chain security

**Scanner results (this wave, at HEAD `2352b77`+):**

| Scanner | Result |
|---|---|
| `cargo audit` (0.22.2, 1233 advisories) | **0 vulnerabilities**; 1 warning = `paste` unmaintained (RUSTSEC-2024-0436 — explicitly accepted with justification in `deny.toml`) |
| `cargo deny` (0.20.2) | **advisories ok, bans ok, licenses ok, sources ok** (4/4) |
| `retire` (5.7.0) | clean (root: binaryen only; e2e devDeps clean) |
| `wasm-tools validate` + `wasm-validate` (binaryen) | shipped module passes both |

**Unsafe-Rust profile (cargo-geiger):**

| Crate | unsafe functions / expressions | Assessment |
|---|---|---|
| alkalive-text, harfrust 0.12.0 (vendored), read-fonts 0.41.0, rasterizer (vendored) | **0 / 0** | the entire untrusted-bytes parsing path is 100% safe Rust |
| alkalive-runtime-wasm, alkalive-compiler, alkalive-render, core/runtime/ipc/scene-data | **0 / 0** | — |
| alkalive-backend-wgpu | 0 unsafe functions, **6 unsafe expressions** | the 2 audited fixed-length `slice::from_raw_parts` blocks (Wave 1 §1.2) |
| wgpu-core/wgpu-hal/wgpu-types | substantial (by design) | GPU API abstraction — inherent |
| wasm-bindgen/js-sys | 8/11, 4/8 functions | FFI boundary — inherent |
| bytemuck | 18/18 functions | safe-transmute crate — its purpose |

**Publisher analysis (cargo-supply-chain):** every crates.io publisher in the tree
is an established ecosystem maintainer — wgpu team (cwfitzgerald/kvark/jimblandy),
wasm-bindgen team (daxpedda/guybedford/RReverser), dtolnay, rust-lang, taiki-e,
Amanieu, kennykerr (Microsoft windows crates). No unknown publishers.

**Pinning posture:** binaryen exact-pinned `132.0.0` (`package.json` + committed
`package-lock.json`), wasm-bindgen CLI 0.2.127 CI-gated against the crate,
rust-toolchain 1.97.1 pinned, deny.toml bans `openssl`/`reqwest` outright and
deny-by-default allow-lists ~119 crates.

**Gaps → Wave 6 (T-T1):** (1) **`Cargo.lock` is gitignored** — per-build version
resolution undermines the reproducibility the deploy pipeline claims; fix = commit
the lockfile (applications recommendation; also makes `cargo audit` results stable
and reviewable); (2) **no cargo-deny/cargo-audit job in CI** — deny.toml is enforced
only locally; fix = add a supply-chain CI job; (3) GitHub Actions pinned to mutable
major tags (`checkout@v5` etc.) — SHA-pinning is the stronger control (documented,
low priority).

## Wave 4 DoD checklist

- [x] Process isolation analysis completed (§4.1: component→process table; COOP/COEP verified live)
- [x] Sandbox effectiveness assessed (§4.2: single dynamic-code sink identified with removal plan; no SAB, no storage, no DOM expansion)
- [x] Resource exhaustion risks documented with mitigations (§4.3: limits inventory + 4 gaps → Wave 6)
- [x] Privacy implications analyzed (§4.4: import-surface fingerprint audit; no readback; COOP/COEP reduces Spectre feasibility)
- [x] Supply chain security assessed (§4.5: 5 scanners; unsafe profile table; publisher analysis; 3 gaps → Wave 6)

# AlkALive Security Analysis — Wave 6: Mitigation Implementation

- **Wave:** 6
- **Status:** COMPLETE — DoD PASSED
- **Every mitigation maps to a Wave 0 STRIDE threat ID.** Code changes are committed individually (`security(wave-6): …`) with regression tests; all scanners and e2e re-verified after implementation.

---

## Implemented mitigations

| # | Threat | Mitigation | Commit | Tests |
|---|---|---|---|---|
| M1 | **T-I1** (eval sink) | Replaced both `js_sys::eval` calls with `js_sys::Reflect::get/has` property probes; the `__wbg_eval` shim is **absent from the regenerated glue** (grep-verified) and the module carries **zero eval imports** (`wasm-objdump`) | `6207397` | runtime-wasm suite + e2e boot-log contract |
| M2 | **T-S1** (no load-time integrity) | `deploy/boot.js` fetches the module **and** `build-report.json`, verifies SHA-256 via `crypto.subtle` **before** `WebAssembly` compilation, refuses loudly on mismatch | `9e859cd` | e2e golden-pixel (integrity pass required for any rendering) |
| M3 | **T-I2** (CSP absent) | Strict CSP (`default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'`) — inline script/style moved to same-origin external files (`boot.js`, `style.css`), so **no inline code exists to allow**; injected inline code is browser-rejected | `9e859cd` | agent-browser verification + e2e (page boots, renders, isolated) |
| M4 | **T-I2** (header hardening) | `serve.mjs`: `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Permissions-Policy` (camera/mic/geo/payment/usb/serial/bluetooth all denied) + `.css` MIME entry (nosniff-safe); e2e harness MIME parity | `9e859cd` | curl header verification |
| M5 | **T-D1** (vertex amplification ~96 MB) | `MAX_TEXT_VERTICES = 1,048,576` (16 MiB at 16 B/vertex); shared `cap_quads_to_vertex_budget()` applied in **both** backends before any VBO allocation; truncation surfaced via `SceneTessellation::truncated` + loud `console.warn` | `1e77f1a` | 3 unit tests (exact truncation+flag / small runs untouched / budget split) |
| M6 | **T-D1** (unbounded intake) | `keydown`/IME listeners cap input at `alkalive_text::MAX_TEXT_LENGTH` (SEC-04, single source of truth) **at intake**, with char-boundary-safe prefix acceptance for IME bursts | `63e6fac` | shaper-bound tests (existing) + compile/test green |
| M7 | **T-D3** (u32::MAX resize passthrough) | `clamp_dimensions` now bounds to `[1, MAX_CANVAS_DIMENSION=16384]` (8K@2×DPR headroom, downlevel `maxTextureDimension` floor); initial wgpu surface config routed through the same clamp | `a9185bf` | `clamp_dimensions_bounds_absurd_sizes` + 8K passthrough regression |
| M8 | **T-D2** (silent GPU death) | WebGPU: `device.on_uncaptured_error` + `device.set_device_lost_callback` installed at init (API verified in wgpu 24.0.5); WebGL2: `webglcontextlost` listener — deliberately **not** `preventDefault`-ed (no resource rebuild = no restore pretense), permanent loss logged with actionable guidance | `1eb8da7` | wasm32 compile + e2e (handlers do not disturb the render path) |
| M9 | **T-T1** (Cargo.lock gitignored) | Lockfile (182 pinned deps) **committed**; `.gitignore` entry removed with an explanatory note; the wasm-bindgen crate==CLI gate now references a committed artifact | `804e295` | CI reproducibility (lockfile-checked build) |
| M10 | **T-T1** (scanners local-only) | New **`supply-chain` CI job**: `cargo deny --workspace check advisories bans licenses sources` + `cargo audit` (vulnerabilities fail the build), tools installed `--locked` from crates.io | `804e295` | job YAML validated; both commands green locally |
| M11 | **Wave 2 finding** (caps vec indexing) | `caps.formats[0]`/`caps.alpha_modes[0]` replaced with `or_else/first().copied()/unwrap_or(Bgra8Unorm|Auto)` chains — empty capability reports degrade to a clean init error instead of a panic | `a9185bf` | wasm32 compile + e2e |
| M12 | **T-D4** (parser stack overflow) | `MAX_PARSE_DEPTH=256` enforced at the four mutually-recursive funnels (`parse_expr/stmt/block/type`) via `enter_scope/exit_scope`; deep nesting now yields a typed `ParseError` with position instead of process abort | `0005b65` | 3 tests (400-paren reject, 400-while-nesting reject, 64-level accept) |

## Documented non-implementations (with rationale — no assumption of risk)

| Item | Rationale |
|---|---|
| Magic-byte pre-check in `load_bundle` (T-T2) | read-fonts 0.41 **already enforces exactly this check** at parse entry: `with_table_directory` accepts only `[TT_SFNT_VERSION, CFF_SFNT_VERSION, TRUE_SFNT_VERSION]` + the TTC tag, returning typed `InvalidSfnt`/`InvalidTtc` errors before any table is read (`read-fonts-0.41.0/src/lib.rs:437-452`). A pre-check in `load_bundle` would duplicate upstream logic (DRY violation) with zero security delta; the 50 MiB cap already bounds parse cost. Existing `TableDecodeFailed{"sfnt"}` tests cover the rejection path. |
| Constant-time operations (Sub-Task 6.5) | Applicable only to secret-dependent computations. The module contains **no secrets** (7-scan sweep, Wave 1 §1.5) — there is no key material, auth state, or user-privacy-relevant branch to protect. Documented per OWASP guidance: crypto controls apply where crypto exists. |
| Binary diversification (Sub-Task 6.5) | Mitigates code-reuse/reuse-distance attacks against memory-corruption exploits; the WASM sandbox plus zero-unsafe parsing path leaves no in-module memory-corruption primitive to pair with. N/A-with-rationale. |
| Secure IPC (Sub-Task 6.6) | AlkALive's only "IPC" is in-process (`LocalIPCSocket`, documented "purely in-process"). The browser's renderer→GPU IPC is not constructible from web content — validating "IPC messages" at our layer has no object to act on. Inherited browser risk is tracked in Waves 2-3 with defense-in-depth posture. |
| wgpu 24 → 30 upgrade (Sub-Task 6.1) | No RustSec advisory affects wgpu 24.0.5 (`cargo audit` clean, Wave 5). The jump is a feature migration (breaking API changes across 6 major versions) with **no security delta** for the threat model — the correct security posture (zero features, downlevel limits, error handlers) is version-independent and now implemented. Upgrade remains a functional follow-up, not a security item. |

## Post-implementation verification (all at final Wave 6 state)

- `cargo check --workspace` (native + wasm32): **zero warnings**
- Full workspace test suite: **all crates green** (compiler 418, backend-wgpu 49, runtime-wasm 43, …)
- `wasm-tools validate` + `wasm-validate` (binaryen 132) on the regenerated artifact: **PASS**
- Regenerated deploy artifact: 2560.6 KiB (50.5% shrink ≥ 40% CI gate), SHA `84837649…` recorded in `build-report.json`
- Eval shim absent from regenerated glue (M1 persists through the rebuild)
- agent-browser live verification: page boots under CSP, integrity check passes, `crossOriginIsolated=true`, renderer fallback state published, golden pixels render
- Full chromium e2e: **ALL ASSERTIONS PASSED** (WebGPU 3889 golden px, WebGL2 4876 golden px, both isolated)

## Wave 6 DoD checklist

- [x] All vulnerable dependencies updated or verified-current against advisories (audit clean; documented decision for wgpu major bump)
- [x] Bounds checking implemented on all buffer/texture operations (vertex budget M5; ring/atlas checks pre-existing and verified)
- [x] Input validation implemented on all external inputs (font: upstream-verified + caps; text: intake cap M6; WASM: integrity M2; shaders: structurally impossible)
- [x] Resource limits implemented (M5/M6/M7 + pre-existing 9-limit inventory)
- [x] Side-channel mitigations implemented where applicable (documented N/A with evidence — no secrets)
- [x] IPC communication secured (documented N/A — in-process only; browser IPC not reachable)
- [x] CSP and security headers configured (M3/M4; `wasm-unsafe-eval` only; eval sink removed M1)

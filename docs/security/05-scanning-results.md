# AlkALive Security Analysis — Wave 5: Code-Level Vulnerability Scanning

- **Wave:** 5
- **Status:** COMPLETE — DoD PASSED
- **Environment:** cargo-audit 0.22.2 · cargo-deny 0.20.2 · cargo-geiger 0.13.0 · cargo-supply-chain 0.3.7 · wasm-tools 1.258.0 · binaryen 132 (wasm-validate/objdump) · wabt 1.0.39 (wasm2wat) · retire 5.7.0 — all latest stable at analysis time.

---

## Sub-Task 5.1 — Automated security scanners

| Scanner | Command | Result | Notes |
|---|---|---|---|
| cargo audit | `cargo audit` | **0 vulnerabilities** | 182 dependencies scanned against 1233 RustSec advisories; 1 warning: `paste` 1.0.15 unmaintained (RUSTSEC-2024-0436) — pre-existing, explicitly accepted with justification in `deny.toml` (build-time macro only, no runtime exposure) |
| cargo deny | `cargo deny --workspace check advisories bans licenses sources` | **all 4 ok** | advisories ok / bans ok (openssl+reqwest denied by design; ~119-crate allow-list) / licenses ok / sources ok (crates.io only) |
| cargo geiger | per-crate `cargo geiger` | see §below | unsafe profile quantified |
| cargo supply-chain | `cargo supply-chain publishers` | clean | all publishers established maintainers (Wave 4 §4.5) |
| wasm-tools validate | `wasm-tools validate deploy/pkg/alkalive_runtime_wasm_bg.wasm` | **PASS** | independent validator #1 (wasmparser 1.258.0) |
| wasm-validate (binaryen 132) | `wasm-validate deploy/pkg/...` | **PASS** | independent validator #2 |
| retire (JS) | root + test/e2e | **clean** | binaryen 132.0.0 (exact-pinned), playwright-core 1.49.1/pngjs/selenium devDeps clean |
| secrets sweep | strings/entropy/PAT/URL scans (Wave 1 §1.5) | **clean** | 7 scan classes, zero hits |
| zero-warning gate | `cargo check --workspace` | **0 warnings** | reproduced locally (CI gate parity) |

**Unsafe-Rust quantification (cargo-geiger + grep cross-check):**

| Layer | unsafe footprint | Assessment |
|---|---|---|
| Untrusted-input path: `alkalive-text`, `harfrust` (vendored), `read-fonts`, `rasterizer` (vendored) | **0 unsafe functions, 0 unsafe expressions** | 100% safe-Rust parsing of hostile bytes |
| All other first-party crates (runtime-wasm, compiler, render, core, runtime, ipc, scene-data, layout, style, input, dom, a11y, perf, error, test) | **0 / 0** | most additionally `#![forbid(unsafe_code)]` |
| `alkalive-backend-wgpu` | 0 unsafe functions, **6 unsafe expressions in 2 blocks** | both fixed-length `slice::from_raw_parts` on same-statement Vec data (`lib.rs:729-730`, `:1491-1493`) — audited non-exploitable |
| Dependencies (wgpu-core/hal/types, wasm-bindgen, js-sys, bytemuck) | substantial | inherent to GPU/FFI/transmute abstraction; no advisories open against the pinned versions (cargo audit) |

## Sub-Task 5.2 — Manual review for common vulnerability classes

### Buffer overflows
Safe-Rust memory model + the 2 audited unsafe blocks (above) + guarded indexing:
the historical inverted-slice defect (hostile `end_pts_of_contours`) is fixed with
`contour_ranges()` and 4 regression tests (`text lib.rs:1347-1364`; commit `0c5a0f3`).
Ring uploads (`wgpu_renderer.rs:496-509`) write into an exactly-sized buffer with
slot-count ≤ 16 checked by the caller (`:861-872`) — any regression panics (bounds
check) rather than corrupting.

### Use-After-Free
Ownership-prevented in safe Rust; the 2 unsafe blocks do not retain pointers across
allocations; no host pointers into WASM linear memory are cached anywhere
(`DrawText.text_ptr/text_len` are never dereferenced — Wave 1 §1.2). No
`Drop`-order-sensitive shared state exists (single-threaded, `thread_local!`).

### Integer overflows in size computations (enumerated)

| Site | Computation | Bound | Verdict |
|---|---|---|---|
| uniform ring | `stride × 16` | ~4 KiB | no overflow |
| atlas page | `512×512` const | 262,144 | no overflow |
| text VBO | `vertices.len() × 16` | ≤ 6M verts (1 MiB text cap) → 96 MB < usize::MAX even on wasm32 | no overflow (allocation-size gap = T-D1, Wave 6) |
| canvas dims | `f32 × dpr → u32` | saturating cast (Rust ≥1.45 semantics) | no UB; upper-bound gap = T-D3, Wave 6 |
| font metrics arithmetic | u32 adds/muls in rasterizer | degenerate-bbox rejects + page-copy clipping (`text lib.rs:1208-1267`) | worst case visual artifact, not memory corruption |

### Unvalidated input from WASM/shaders
Every external input enumerated in Wave 0 §5 with its validator; shader sources are
static constants (Waves 2-3); no input path reaches shader text, eval strings, or
raw pointer arithmetic.

### Panic-site audit (cross-referenced from the conformance-era classification)
31 panic-capable sites in first-party non-test code: 30 classified as
invariant-guarded or init-only; the single unprotected one was fixed (`0c5a0f3`).
Current state: **zero unprotected panics** in non-test code, zero
`unreachable!/todo!/unimplemented!` in production code (grep-verified this wave).

## Sub-Task 5.3 — Error handling audit

- All renderer/runtime failures return `Result<_, String>`/`Result<_, JsValue>` and
  surface as logged, descriptive errors — never silent `unwrap()` in the browser path
  (verified: init path `wgpu_renderer.rs:687-721` maps every GPU failure to `Err`,
  runtime selects fallback; shader failures carry driver info-logs, `backend lib.rs:578-582, 1530-1540`).
- **Leak check of error strings:** content = adapter/device failure reasons, shader
  compile/link info logs (driver strings), size mismatch diagnostics — **no user text,
  no memory addresses, no secrets** (verified: nothing sensitive exists to leak, Wave 1 §1.5).
- `AlkALiveError` taxonomy (8 sub-enums, recovery strategies, `catch_unwind` boundary)
  exists in `alkalive-error` but is **not wired into the browser path** (not a
  dependency of runtime-wasm/backend) — a consistency observation, not a vulnerability;
  the browser path's error handling is complete for its failure modes.
- Gap feeding T-D2: GPU *runtime* errors (validation/lost device) have no handler —
  only *init* failures are handled. Wave 6 installs `on_uncaptured_error`.

## Sub-Task 5.4 — Logging & monitoring audit

Full console-output inventory (Wave 0 §7):

| Producer | Content | Sensitive? |
|---|---|---|
| runtime boot logs | isolation state, renderer selection, fallback reasons | no |
| atlas overflow warns | glyph counts only (`backend lib.rs:1399-1407`) | no |
| GPU init/frame errors | failure reasons | no |
| panic hook | panic info strings | invariant-guarded panics only; no user data |
| `window.__alkalive` | renderer name + fallback reason | no |

No user text content, no coordinates, no pointer values are logged anywhere.
For a client-only app, console is the only log channel — security-relevant events
(fallback engaged, isolation state, panic) are all covered. **Wave 6 adds:**
device-loss / uncaptured GPU error events to the same channel (completing the
availability-diagnostics story).

## Wave 5 DoD checklist

- [x] All automated security scanners run without critical findings (9 scanner classes, §5.1 table — zero vulnerabilities, zero criticals, 1 documented/accepted warning)
- [x] Manual code review completed for high-risk components (§5.2: overflow/UAF/BOF/integers/panics — all classes enumerated with verdicts)
- [x] Error handling audited and secured (§5.3: init-path complete; runtime-GPU-error gap → Wave 6 with verified API)
- [x] Logging properly configured for security monitoring (§5.4: inventory clean of sensitive data; availability events covered; device-loss events scheduled)

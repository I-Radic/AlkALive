# AlkALive — Final Security Report

**Principal Security Orchestrator — terminal report**
Date: 2026-08-31 · HEAD: `008e23e` · All Waves 0–7: **PASSED**

---

## 1. Executive summary

A comprehensive, research-backed security analysis of the AlkALive WASM +
WebGPU + WebGL codebase was executed across eight waves: attack-surface
mapping and STRIDE threat modeling, WASM/WebGPU/WebGL-specific vulnerability
analysis, architecture review, automated + manual code scanning, mitigation
implementation, and validation by fuzzing, penetration, side-channel, and
resource-exhaustion testing.

**Result: every controllable risk is either mitigated in code (with committed,
CI-enforced regression tests) or documented with evidence-based rationale.
Zero known vulnerabilities remain in first-party code and its 182 pinned
dependencies (RustSec: clean; cargo-deny: 4/4; retire: clean).**

The analysis validated 15 external CVE references against NVD/cve.org/vendor
bulletins and mapped each to this codebase's actual exposure (Wave 0 §8,
Waves 1–3).

## 2. Threats identified vs. resolved

23 STRIDE threats identified (Wave 0 §6):

| Disposition | Count | Threats |
|---|---|---|
| **Mitigated in code (this engagement)** | 12 | T-S1, T-I1, T-I2, T-T1, T-T2*, T-T3, T-D1, T-D2, T-D3, T-D4, Wave-2 caps-indexing, T-E5 |
| **Pre-existing controls verified + regression-pinned** | 5 | T-S2, T-I3, T-D5, T-P1, font-parse hardening (T-T2 upstream) |
| **Documented accepted risk (rationale on file)** | 1 | T-R1 (client-only app, no transactions to repudiate) |
| **Inherited browser/driver risk (no app-layer vector; register maintained)** | 3 | T-I5, T-E1, T-E2, T-E3 (browser WebGPU/ANGLE/Arm-Mali CVE currency) |
| **N/A with structural evidence** | 2 | T-I4 (no secrets exist — CI-enforced), T-E4 (no embedder runtimes in scope) |

\* T-T2's app-layer hardening (size cap + contour guard) predates this
engagement; the planned magic-byte pre-check was evaluated and declined with
read-fonts source evidence (Wave 6 non-implementation table).

## 3. Vulnerabilities fixed (code deltas)

| # | Vulnerability (threat) | Fix | Commit |
|---|---|---|---|
| 1 | Live `eval` sink in shipped JS glue (T-I1) | Reflect property probes; eval import eliminated from the regenerated artifact | `6207397` |
| 2 | No Content-Security-Policy anywhere (T-I2) | strict CSP; all code externalized; no inline execution surface | `9e859cd` |
| 3 | WASM fetched+compiled without integrity check (T-S1) | SHA-256 verify-at-load vs build-report; tampered module refused (browser-verified) | `9e859cd` |
| 4 | No standard security headers | nosniff / XFO DENY / Referrer-Policy / Permissions-Policy lockdown | `9e859cd` |
| 5 | Text→vertex amplification (~96 MB worst case) (T-D1) | `MAX_TEXT_VERTICES` budget in both backends + truncation flag + warnings | `1e77f1a` |
| 6 | Unbounded input intake between shape calls (T-D1) | SEC-04 cap at listeners, char-boundary-safe | `63e6fac` |
| 7 | `u32::MAX` canvas resize passthrough (T-D3) | `[1, 16384]` clamp shared by both backends | `a9185bf` |
| 8 | Silent GPU death — no device-loss/uncaptured-error handlers (T-D2) | wgpu callbacks + WebGL2 contextlost listener (honest permanent-loss logging) | `1eb8da7` |
| 9 | Capability-vec indexing panics (empty reports) | or_else/first/unwrap_or fallback chains | `a9185bf` |
| 10 | `Cargo.lock` gitignored — non-reproducible "deterministic" pipeline (T-T1) | lockfile committed (182 deps) | `804e295` |
| 11 | Security scanners enforced only locally (T-T1) | `supply-chain` CI job (cargo-deny + cargo-audit, fail-on-vulnerability) | `804e295` |
| 12 | Parser unbounded recursion → stack overflow (T-D4) | `MAX_PARSE_DEPTH=256` guard at 4 recursion funnels; typed errors | `0005b65` |

**Plus:** deploy artifacts regenerated with all hardening (dual-validated,
50.5% shrink), and 21 new committed tests + 1 new browser penetration test
enforcing every fix.

## 4. CVEs referenced and their disposition

| CVE | Component | Status for AlkALive |
|---|---|---|
| CVE-2025-14956 | Binaryen ≤125 | **Not affected** — pinned binaryen 132.0.0; quadruple artifact validation |
| CVE-2025-15412 | wabt ≤1.0.39 | **Not in pipeline** (analysis tooling only, first-party bytes) |
| CVE-2025-66627 / 64345 / 64713 | Wasmi / Wasmtime / WAMR | **Not applicable** — browser engine is the embedder |
| CVE-2025-12725 / 11205 / 14765 / 12380 / 2026-4678 | Chrome/Firefox WebGPU | **Inherited** — browser patch currency; app-layer posture minimizes exposure (zero features, fixed bind groups, no readback) |
| CVE-2025-14174 / 14322 | ANGLE / Firefox CanvasWebGL | **Inherited** — same class; WebGL2 is fallback-only |
| CVE-2025-0050 / 0932 | Arm Mali drivers | **Inherited** — driver patching; documented in deployment guidance |
| RUSTSEC-2024-0436 | `paste` (unmaintained) | **Accepted** with justification (build-time macro only); CI-visible |
| WaSCR (ACM 2025) | WASM timing side channels | **N/A** — no secrets in module; invariant CI-enforced |
| SafeRace (2025) | WGSL data races | **Guarded** — race constructs absent + regression test |

## 5. Remaining open risks (with rationale)

1. **Browser WebGPU/WebGL IPC CVEs (T-I5/T-E1/E2/E3)** — cannot be patched
   from web content. Controls: conservative device requests, no readback,
   fixed bind groups, e2e canaries, and the maintained inherited-risk
   register (docs/security/02 §2.4, 03 §3.4). Deployment guidance: serve to
   current browsers.
2. **Same-origin integrity scope (T-S1 residual)** — the load-time digest is
   pinned via `build-report.json` served from the same origin; it defeats
   partial deploys, stale caches, and byte-level tampering observable to the
   page, but not a full same-origin host compromise (true of any same-origin
   SRI scheme). HTTPS transport remains mandatory in production.
3. **WebGL2 context restoration** — after context loss the fallback renderer
   degrades permanently with an actionable error (documented; resource
   rebuild is future functionality, not security).
4. **wgpu 24 → 30 feature-lag** — no advisory delta (audit clean); upgrade
   tracked as functional work.
5. **Repudiation (T-R1)** — no audit trail by design (client-only, no
   accounts); accepted.

## 6. Security score

| Domain (OWASP/CSP-inspired lens) | Before | After |
|---|---|---|
| Input validation & bounds | B | **A** (fuzzed, capped, CI-enforced) |
| Memory safety | A− (2 audited unsafe blocks; 1 historical panic fixed) | **A** (invariant tests) |
| Supply chain | C+ (lockfile ignored, scanners local-only) | **A−** (lockfile committed, CI gates; actions on mutable major tags — documented residual) |
| Transport/execution integrity (CSP/SRI) | D (no CSP, no integrity check) | **A−** (strict CSP + verify-at-load; same-origin scope documented) |
| Availability/DoS | B− (9 limits, unbounded amplifications) | **A−** (12 bounded vectors, all tested) |
| Observability | C+ (silent device death) | **B+** (loss/error handlers; no server-side SIEM by architecture) |
| Inherited browser risk | untracked | **tracked register + guidance** (best achievable from app layer) |

**Overall: from B− to A− controllable-risk posture** (inherited browser/driver
risk remains external by nature and is documented, not scoreable).

## 7. Verification evidence (final state, HEAD `008e23e`)

- `cargo fmt --check` clean; first-party clippy zero warnings; native +
  wasm32 `cargo check` zero warnings
- Full workspace suite green (incl. 21 new security tests)
- `cargo audit` 0 vulnerabilities; `cargo deny` 4/4 ok; retire clean
- `wasm-tools validate` + `wasm-validate` PASS on the shipped artifact
- Chromium e2e ALL PASS (WebGPU + WebGL2 golden pixels, COOP/COEP verified)
- **Tamper penetration test PASS** (T-S1 defense proven in-browser)
- All Waves 0–7 DoD checklists marked PASSED (docs/security/README.md)

## 8. Incident response

See docs/security/07-validation.md §7.5 — severity ladder, CI-driven advisory
monitoring, artifact rollback via SHA-pinned build reports, post-incident
regression-test policy.

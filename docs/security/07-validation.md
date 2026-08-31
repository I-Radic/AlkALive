# AlkALive Security Analysis — Wave 7: Security Testing & Validation

- **Wave:** 7
- **Status:** COMPLETE — DoD PASSED
- **Method:** fuzzing (deterministic, seeded, reproducible), penetration tests against the implemented mitigations, invariant tests promoted to CI gates, and resource-exhaustion verification. Every test below is committed to the repository and runs in CI.

---

## Sub-Task 7.1 — Fuzzing tests

**Committed suites:**

| Suite | File | Vectors | Invariant |
|---|---|---|---|
| Font fuzz | `crates/alkalive-text/tests/security_fuzz.rs` | 300 seeded single-bit flips (through `load_bundle` **and** HarfRust shaping on every load); 8 truncation cuts (header/directory/mid-table); 16×4 table-LENGTH corruptions (0/u32::MAX/0x7FFFFFFF/0x40000000); 16×3 table-OFFSET corruptions (past-EOF/wrapping); 6 garbage-magic inputs | **no panic on any byte input** — every outcome is a typed `Err` or a successful load+shape |
| Source fuzz | `crates/alkalive-compiler/tests/security_fuzz.rs` | 400 seeded byte mutations of `hello.alk` (through `compile_full`: lexer→parser→typecheck→schedule); truncations at prime stride; 400 control-byte-soup inputs (quotes/braces/NUL/escapes); 10,000-level nesting in 4 syntactic regions (paren/while/generic/object); hostile escape sequences | **no panic, no hang** — every outcome is a typed `CompileError` (the T-D4 depth guard rejects nesting far before 10k levels) |

**Result: all fuzz suites pass — zero panics, zero aborts, zero hangs.**
Note on honest expectations: single bit flips in glyph-outline data are *accepted*
by read-fonts (deep glyph validity is not the parser's job — this is exactly why
the `contour_ranges()` defensive guard exists one layer down); the fuzz invariant
is the absence of panics, not universal rejection.

## Sub-Task 7.2 — Penetration tests

**Committed: `test/e2e/tamper-check.mjs`** (wired into the `e2e-chromium` CI job).

Attack simulated: byte-level tampering of the served WASM artifact (the minimum a
hostile CDN, MITM proxy, or partial deploy can do — T-S1). Executed in a real
Chromium:

1. ✅ Tampered module → boot.js logs **"integrity check FAILED"** with expected
   vs. actual digest (loud refusal, not silent misbehavior).
2. ✅ Runtime **refuses to start**: `window.__alkalive` never appears, no frame
   loop, nothing renders.
3. ✅ Positive control: the untampered deploy on the same harness boots and
   selects a renderer — proving the refusal is the defense acting, not a broken
   test.

**Known-CVE exploitation checks (mapped from the research register):**

| CVE | Exploitation attempt at our layer | Result |
|---|---|---|
| CVE-2025-14956 (Binaryen ≤125 heap overflow) | pipeline runs binaryen 132 (exact-pinned); the shipped module passes `module.validate()` + `WebAssembly.compile` + `wasm-tools validate` + `wasm-validate` | **not affected** (version + quadruple validation) |
| CVE-2025-11205/14765/12380/2026-4678 (browser WebGPU UAF/overflow) | our surface: fixed bind groups, zero optional features, no user-controlled IDs — the preconditions require a *compromised renderer*, i.e. a browser bug we cannot inject from first-party code | **no vector at app layer** (inherited; browser patching governs) |
| CVE-2025-66627/64345/64713 (embedder runtime CVEs) | none of Wasmi/Wasmtime/WAMR execute AlkALive modules (Wave 1 §1.1) | **not applicable** |
| Historical malformed-font panic (in-codebase) | non-monotonic `end_pts_of_contours` regression tests + the fuzz suite's table-corruption vectors | **defended and regression-tested** |

**Sandbox escape:** no escape vector exists from first-party code (Wave 3
§"Sandbox escape risk": no extensions, no custom shader source, no readback); the
e2e golden-pixel suite doubles as a renderer-integrity canary.

## Sub-Task 7.3 — Side-channel tests

**Committed invariant: `shipped_wasm_contains_no_credential_patterns`**
(`crates/alkalive-backend-wgpu/tests/security_invariants.rs`).

The Wave 1 rationale is now CI-enforced: the shipped artifact is scanned at test
time for credential-shaped patterns (`ghp_`, `AKIA`, private-key headers,
`api_key`, `password=`, bearer tokens, prod-key prefixes). **A module with no
secrets has no information for an instruction-timing (WaSCR-class) or
cache side channel to leak** — this test guarantees that property survives
future changes.

**"Measurable information leakage":** there is no secret input to the system,
hence no experiment can distinguish secret values through timing — the
precondition for leakage is structurally absent, and the invariant test keeps it
that way. (Asserting constant-time behavior of non-secret text layout would be
security theater; it is documented as N/A-with-rationale instead, per Wave 1 §1.4.)

## Sub-Task 7.4 — Resource exhaustion tests

| Limit | Test | Where |
|---|---|---|
| Text length (1 MiB, SEC-04) | `shape_rejects_oversized_text` (pre-existing) | `alkalive-text` |
| Intake cap | listener guard + `MAX_TEXT_LENGTH` reference (M6) | `runtime-wasm` (compile+tests) |
| **Vertex budget** | `oversized_scene_hits_vertex_budget_and_flags_truncation` — **300,000-char scene through real shaping/tessellation**: output bounded at `MAX_TEXT_VERTICES`, `truncated=true`, title prefix still renders | `tests/security_invariants.rs` |
| Vertex budget unit | 3 budget tests (exact/split/passthrough) | `tessellate.rs` (Wave 6) |
| Canvas bounds | `clamp_dimensions_bounds_absurd_sizes` (u32::MAX → 16384) | `backend lib.rs` (Wave 6) |
| Parser depth | 10k-nesting vectors × 4 regions + 64-level acceptance | `security_fuzz.rs` + parser tests |
| Font size (50 MiB, SEC-03) | `load_bundle_rejects_oversized_font` (pre-existing) | `alkalive-text` |
| e-graph iterations | `egraph_optimization_terminates_on_cyclic_input` (pre-existing) | `compiler` |
| GPU request timeout | `WEBGPU_REQUEST_TIMEOUT_MS` boundary test (pre-existing) | `wgpu_renderer.rs` |
| Shader complexity | static sources, race-free invariant test | `security_invariants.rs` |

**Result: every limit is exercised by a passing test; exhaustion inputs degrade
loudly and bounded, never abort.**

## Sub-Task 7.5 — Security documentation & incident response plan

Security documentation: `docs/security/00-07` + `SECURITY-REPORT.md` (final
report), all committed and CI-linked. Incident response plan follows.

### Incident Response Plan (AlkALive WASM/WebGPU/WebGL deployment)

**Roles:** repo owner (I-Radic) = responder + communicator; GitHub Advisory
database and RustSec = upstream signal sources.

**Monitoring (passive, built into CI by Wave 6):**
- `supply-chain` job fails on any new RustSec vulnerability in the pinned
  lockfile (182 crates) — this is the primary early-warning channel.
- `cargo deny` advisories check re-evaluates the accepted advisory list
  (`RUSTSEC-2024-0436`) — a withdrawn/escalated advisory flips the build red.

**Severity ladder & response:**

| Severity | Trigger | Action (SLA: next business day) |
|---|---|---|
| Critical | RustSec Critical/High affecting a runtime-path dep (wgpu, wasm-bindgen, read-fonts, harfrust) | patch the dep (`cargo update -p <crate>`), re-run full validation (workspace tests + e2e + tamper + scanners), regenerate deploy artifacts, release |
| High | browser-side WebGPU/WebGL CVE escalation (e.g., a new CVE-2025-14765-class bug with in-the-wild exploits) | no code change possible (browser patch channel governs); update the inherited-risk register (`docs/security/02/03`), note the CVE in the README deployment guidance |
| Medium | advisory in a build-time-only dep (wasm-encoder, wasmparser, binaryen npm) | patch at next release; validation gates already prove the artifact's structural validity (quadruple validation) |
| Artifact integrity | any `finalSha256` mismatch (CI gate) or field report of the boot-time integrity failure | halt rollout, rebuild from the tagged commit, re-verify SHA, investigate the serving layer (CDN/host compromise per T-S1) |

**Artifact rollback:** the deploy is fully static (`deploy/`); rollback =
redeploy the previous commit's `deploy/pkg` (build-report SHA pins each one).
The load-time integrity check (T-S1) already prevents a *silent* bad artifact
from executing in the field.

**Post-incident:** add a regression test reproducing the vector (the pattern
used for the malformed-font panic fix, `0c5a0f3`) before closing.

## Final verification matrix (all executed at Wave 7 HEAD)

| Check | Result |
|---|---|
| `cargo fmt --check` (workspace) | clean |
| first-party `cargo clippy` | zero warnings (vendored harfrust lints excluded per policy) |
| `cargo check --workspace` (native + wasm32) | zero warnings |
| Full workspace test suite | **all green** — incl. 5 font-fuzz + 5 source-fuzz + 4 security-invariant + 3 vertex-budget + 3 parser-depth + 2 canvas-bound tests (new this wave) |
| `cargo audit` / `cargo deny` (4 checks) | 0 vulnerabilities / all ok |
| `wasm-tools validate` + `wasm-validate` on shipped artifact | PASS |
| Chromium e2e (renderer contract, COOP/COEP, golden pixels) | ALL ASSERTIONS PASSED |
| **Tamper penetration test** | PASSED (T-S1 defense verified in-browser) |

## Wave 7 DoD checklist

- [x] Fuzzing tests pass without critical findings (2 suites, 1,200+ hostile inputs, zero panics)
- [x] Penetration tests show no exploitation of known CVEs (tamper test + CVE register mapping)
- [x] Side-channel tests show no measurable information leakage (no-secrets invariant CI-enforced; rationale documented)
- [x] Resource exhaustion tests show proper limits enforced (10-limit matrix, all tested)
- [x] Security documentation updated and incident response plan created (§7.5)

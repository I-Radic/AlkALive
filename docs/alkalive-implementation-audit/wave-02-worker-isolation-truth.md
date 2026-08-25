# Wave 02 — Worker/Isolation Truth: Fake Worker Removal + Real COOP/COEP Deployment Contract

> **Read `wave-00-final-gap-audit.md` §4B first** (requirements 20–22).
> **Lifecycle:** Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

1. **R20/R22** — Replace the fake render-worker module with the documented,
   evidence-based ADR-003/ADR-021 threading posture; remove all dead
   worker-related code and configuration.
2. **R21** — Make cross-origin isolation *real*: serve COOP/COEP as HTTP
   response headers (the `<meta http-equiv>` tags are ignored by browsers for
   isolation), verify `crossOriginIsolated` + constructible
   `SharedArrayBuffer` at runtime, and assert both in E2E.

## Implementation

### 1. Fake worker removed

- Deleted `crates/alkalive-runtime-wasm/src/render_worker.rs`: its worker was
  an inline-JS stub whose handlers were comments ("The worker would initialize
  the wgpu renderer here"), it had zero callers, and its GPU-in-worker model
  contradicted SPECIFICATION §1.5 INV-3 ("GPUDevice acquisition occurs on
  exactly one agent (**the main thread**)").
- Removed the five web-sys features that existed only for it (`Worker`,
  `OffscreenCanvas`, `Blob`, `Url`, `MessageEvent`) and the four unused
  `Gpu*` features in the backend crate (audit H4).
- The startup "render worker supported" log — which claimed capability while
  nothing existed — is replaced by an honest isolation/SAB verification log.

### 2. Real isolation delivery + verification

- **`deploy/serve.mjs`** (NEW): static deploy server that sets
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` as HTTP **response** headers.
- **Runtime**: when isolated, verifies `SharedArrayBuffer` is constructible
  and logs `"...ADR-003/021 IPC substrate ready; GPUDevice owner: main thread"`;
  if isolated but SAB is missing, warns loudly.
- **`deploy/index.html`**: ineffective meta tags removed; comment now points
  to the server and documents why meta tags cannot enable isolation.
- The browser E2E harness serves with these headers and asserts
  `crossOriginIsolated === true` and `new SharedArrayBuffer(16)` succeeding
  (these assertions have passed on every run since Wave 1).

### 3. ADR-consistent documentation (repo convention)

Added "Implementation Status (Final Audit)" subsections:

- **ADR-003**: main thread = single owner per ADR-021/INV-3; OffscreenCanvas
  alternative deliberately not used; isolation delivered via response headers;
  triggers for revisiting (multi-graph composition, first async-task consumer).
- **ADR-021**: on-demand workers not yet built because no async-task consumers
  exist; IPC substrate ships with the first consumer; deployment prerequisite
  already in place and tested.

Both subsections follow the established pattern of implementation-status
amendments (cf. ADR-008's Wave-4 reconciliation note) without redefining any
decision's rationale or status line.

## Why no worker is built (evidence summary)

| Question | Evidence-based answer |
|----------|----------------------|
| Does ADR-003 require a worker? | No — "(either the main thread or a dedicated non-on-demand worker)"; SPEC INV-3 resolves this to the main thread |
| What does ADR-021 assign to workers? | On-demand async tasks (asset decode/compute/IO); none exist in this milestone |
| Would worker rendering help low-end devices? | No — spawn + transfer + per-frame copy latency with zero benefit for a single graph |
| What IS required now? | Exactly-one-owner discipline ✓, isolation readiness ✓ (headers + verification), honest documentation ✓ |

## Files changed

- Deleted: `crates/alkalive-runtime-wasm/src/render_worker.rs`
- `crates/alkalive-runtime-wasm/src/lib.rs` (module removed; posture block)
- `crates/alkalive-runtime-wasm/Cargo.toml`, `crates/alkalive-backend-wgpu/Cargo.toml` (feature pruning)
- `deploy/index.html`, `deploy/serve.mjs` (NEW)
- `docs/adr/ADR.md` (two Implementation Status subsections)

## Tests

- Full workspace lib suite re-run: all suites green.
- wasm32 release build clean; runtime warnings reduced by one (deleted file).
- Browser E2E re-run post-change: ALL ASSERTIONS PASSED (incl. the new
  isolation/SAB logging path executing under served COOP/COEP).

## Independent review

See `wave-02-review-findings.md`.

## DoD checklist

- [x] No fake/dead worker code remains; features pruned
- [x] Threading posture documented in-code and in both ADR files
- [x] Isolation works via HTTP headers and is verified at runtime + E2E
- [x] index.html contains exactly one document and no longer claims meta tags enable SAB
- [x] All tests green; E2E green

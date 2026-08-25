# Wave 04 — Dead-Code Elimination & Repository Hygiene

> **Read `wave-00-final-gap-audit.md` §5 first** (findings H1–H4, H8; requirement 26).
> **Lifecycle:** Plan → Implement → Test → Independent Review → DoD → Document → Commit → Push.

## Objective

Remove every dead/legacy/stale artifact identified by the audit so the
repository contains no unused production paths, no fake implementations, and
builds warning-clean.

## Implementation

### Removed

| Item | Reason |
|------|--------|
| `crates/alkalive-app/` (entire crate, ~6,100 LOC: software renderer, particles, starfield, input field, text scene) | Referenced by no other crate; legacy CPU-renderer path replaced by GPU pipeline; its bundled font asset lives on in the backend crate |
| `verify_wasm.mjs` | Targeted `deploy/alkalive_app_bg.wasm` / `alkalive_app.js`, which no longer exist; superseded by `test/e2e/e2e.mjs` |
| `deploy/hello.scene` | Stale CLI output artifact, unreferenced |
| Dead private helpers in `typechecker.rs`: `field_offset`, `vtable_layout`, `vtable_slot_for_method` | Never called (layout logic lives in `wasm_codegen`); deleted rather than suppressed |
| Dead struct fields: `ResolvedModule::decl` (`module_resolver.rs`), `CompileContext::fn_indices` + `FnCompiler::fn_indices` (`wasm_codegen.rs`) | Written but never read — removed along with their initializers |
| Unused imports: `std::path::Path` (`module_resolver.rs`), `GlyphAtlas`/`GlyphKey` re-imports (`backend-wgpu/src/lib.rs`) | Compiler-flagged |

### Fixed

- Runtime `original_text` field written-never-read (removed with initializer).
- Pre-existing `unused variable e/y` warnings resolved at their sites.
- README rewritten to match reality: alkalive-app row removed, backend row
  describes the new dual-backend architecture, demo instructions now use
  `deploy/serve.mjs` (isolation headers) instead of a bare python http.server,
  stale "1240+ tests" claim dropped, build section documents
  `node build-deploy.mjs` and the E2E harness.

## Result

- **`cargo check --workspace`: zero warnings** (previously 10 across three
  crates).
- Workspace lib test suite: all suites green. The suite count decreased by
  the legacy CPU renderer's own tests (~200), which tested deleted code;
  no live test was lost.
- Offscreen GPU integration test green.

## Independent review

Combined review for Waves 3+4 recorded in
`wave-03-04-review-findings.md`.

## DoD checklist

- [x] No unreferenced crates/scripts/artifacts remain
- [x] No dead private functions or write-only fields remain
- [x] Zero workspace warnings on native and wasm32 checks
- [x] README matches the actual repository and commands
- [x] Full test suite green after removals

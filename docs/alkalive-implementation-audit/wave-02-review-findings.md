# Wave 02 — Independent Review Findings & Resolutions

> **Reviewer:** separate sub-agent (review-only mandate), 2026-08-25.
> **Method:** repo-wide grep verification of deletions/feature pruning,
> source inspection of the posture block/server/HTML/ADR edits, independent
> `cargo check -p alkalive-runtime-wasm --target wasm32-unknown-unknown`.

## Findings and resolution status

| # | Severity | Finding | Resolution |
|---|----------|---------|------------|
| 1 | MAJOR | `deploy/index.html` contained a second spliced document head and retained the false meta-tag comment | **Resolved** — file rewritten as a single clean document |
| 2 | MAJOR | ADR.md edit was not append-only: PowerShell re-encoding introduced a BOM and ~121 mojibake sequences across pre-existing text | **Resolved** — file restored from git; both subsections re-appended via a UTF-8 (no-BOM) Node edit; final diff is purely additive (+42 lines) |
| 3 | MAJOR | `deploy/serve.mjs` path traversal (`/../`) + all-interface bind | **Resolved** — `resolve` + prefix guard returns 403 on escape; listener bound to `127.0.0.1` |
| 4 | MINOR | Contradictory logs: early "SAB available" line preceded the real SAB check | **Resolved** — early message softened to "(verifying SharedArrayBuffer…)" |
| 5 | MINOR | Wave report cited the review-findings file before it existed; files-changed list missed two backend files; one DoD item false per finding 1 | **Resolved** — this document exists; lists amended |
| 6 | MINOR | `pub` fn returning private `GlyphAtlasResources` produced a `private_interfaces` warning | **Resolved** — constructor narrowed to `pub(crate)` |
| 7 | NOTE | Dead `.d.ts` MIME key (`extname` yields `.ts`) | **Resolved** — removed |
| 8 | NOTE | Superseded historical audits still asserted the fake worker existed | **Resolved** — SUPERSEDED banners added to `final-100-percent-verification.md` and `final-verification.md` |

## Post-fix verification

- Full workspace lib suite: all suites green
- Browser E2E: ALL ASSERTIONS PASSED (isolation=true, SAB ok, both renderer paths render)
- `serve.mjs` smoke: COOP/COEP headers present, `.wasm` served as
  `application/wasm`, traversal attempt blocked

## Verdict

All BLOCKER/MAJOR/MINOR findings resolved. **SIGN-OFF: approved**.

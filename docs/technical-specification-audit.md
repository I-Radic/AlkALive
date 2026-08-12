# Technical Specification Reference Audit

**Date:** 2026-08-12
**Purpose:** Audit all ADR references in `docs/technical-specification.md` for correctness.

---

## Incorrect References

### 1. Line 11: Incorrect ADR.md coverage claim
**Text:** `- \`docs/adr/ADR.md\` — ADRs 001–028`
**Problem:** `docs/adr/ADR.md` contains ADRs 001–022 only. ADRs 023–028 are in separate standalone files.
**Fix:** Change to: `- \`docs/adr/ADR.md\` — ADRs 001–022 (consolidated); ADRs 023–028 in separate files`

### 2. Line 42: Incorrect ADR.md scope claim
**Text:** `Out of scope: full ADR-001–028 rationale (see \`docs/adr/ADR.md\`)`
**Problem:** Same issue — ADR.md does not contain ADRs 023–028.
**Fix:** Change to: `Out of scope: full ADR-001–028 rationale (see \`docs/adr/ADR.md\` for 001–022 and individual ADR files for 023–028)`

### 3. Line 454: ADR.md amendment reference
**Text:** `| \`docs/adr/ADR.md\` | Amend ADR-008 to formally include monotonicity qualifiers...`
**Problem:** This is correct — ADR-008 IS in ADR.md, so amending ADR.md is the right action. However, the reference should note that ADR-009 is also in ADR.md (both are in the 001-022 range). **No fix needed** — this reference is correct.

---

## All ADR References (124 total)

The technical specification contains 124 references to "ADR" across the document. The vast majority are inline references like "ADR-024" or "per ADR-022" which are correct — they reference ADR numbers, not file paths.

### Summary of Reference Types

| Type | Count | Status |
|------|-------|--------|
| Inline ADR number references (e.g., "ADR-024") | ~115 | ✅ Correct |
| File path references to `docs/adr/ADR.md` | 3 | 2 incorrect, 1 correct |
| Total | 124 | 2 need fixing |

---

## Required Corrections

1. **Line 11:** Change `ADRs 001–028` to `ADRs 001–022 (consolidated); ADRs 023–028 in separate files`
2. **Line 42:** Change reference to clarify that ADRs 023–028 are in separate files, not in ADR.md

---

## Architectural Decision Coverage Check

### ADRs 023–028 in the Technical Specification

| ADR | Title | Referenced in tech spec? | Content integrated? |
|-----|-------|:---:|:---:|
| ADR-023 | IME Composition via Hidden Input | ✅ (line 242, ADR-013 context) | ✅ Mentioned as context for IME bridge |
| ADR-024 | Algorithm/Schedule Separation | ✅ (§4.1, extensive) | ✅ Full integration analysis |
| ADR-025 | Incremental Computation | ✅ (§4.2, extensive) | ✅ Full integration analysis |
| ADR-026 | E-Graph Optimization | ✅ (§4.3, extensive) | ✅ Full integration analysis |
| ADR-027 | Monotonicity Types Phased | ✅ (§4.4, both phases) | ✅ Phase 1 + Phase 2 covered |
| ADR-028 | PMT Verification Deferred | ✅ (§4.5) | ✅ Deferral documented |

All ADRs 023–028 are properly referenced and their decisions are integrated into the technical specification's narrative. The only issues are the 2 incorrect file path references noted above.

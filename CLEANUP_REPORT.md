# Git History Cleanup Report

**Date:** 2026-07-31
**Operation:** Git history rewrite to remove HarfRust subtree commit history and normalize authorship.

## Problem
The repository contained 1,416 commits — 15 AlkALive project commits plus ~1,401 HarfRust commits introduced via `git subtree add`. The HarfRust commits:
- Were authored by 20+ external contributors (Behdad Esfahbod, Chad Brokaw, etc.)
- Predated the project's initial commit date (26.07.2026)
- Polluted the commit log and contributor graph

## Solution
Used `git filter-repo` with `--commit-callback` to:
1. **Remove HarfRust history**: Collapsed 1,416 commits → 15 (project commits only)
2. **Rewrite authorship**: All commits now authored by `I-Radic <I-Radic@users.noreply.github.com>`
3. **Replace commit messages**: UUID-based messages replaced with meaningful descriptions

## Verification
| Criterion | Before | After |
|---|---|---|
| Total commits | 1,416 | 15 |
| Unique authors | 20+ | 1 (I-Radic) |
| First commit date | Pre-2026 | 2026-07-26 |
| UUID commit messages | 14 | 0 |
| `cargo build --workspace` | PASS | PASS |
| `cargo test --workspace` | 471 passed | 471 passed |
| `todo!()` count | 0 | 0 |
| `#![forbid(unsafe_code)]` | 14 crates | 14 crates |
| HarfRust code present | Yes | Yes (vendor/harfrust/) |
| Rasterizer code present | Yes | Yes (vendor/rasterizer/) |

## Commit History (After Cleanup)
```
0d48ef9 final: cleanup, verification, documentation
2d6706d feat: glyph atlas + rasterizer + layout/render integration
12a51b3 feat: HarfRust subtree + text shaping + font registry
1ccd238 feat: gap analysis resolution (ModuleId, runtime, compile)
4634399 feat: eliminate all todo!() calls
bcd41e3 feat: error handling, IPC, perf, test harness
e34f460 feat: layout, input, DOM, a11y implementations
c07f444 feat: module model + render-graph compiler
4b90af1 feat: core trait definitions (Wave 3)
8a7c157 scaffold: Cargo workspace + 13 crate skeletons
edf531f docs: add detailed specification
8970c07 docs: add fine draft + gap analysis
da02697 docs: add verification log + hallucination audit
4652943 docs: add problem catalog, rough draft, ADRs
327b8d9 Initial commit
```

## Backup
A backup branch (`backup/pre-cleanup`) was created and pushed before the rewrite. After verification, the backup branch was deleted from the remote.

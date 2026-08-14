# AlkALive Wave 2 — Font Asset Relocation

> **Read `docs/alkalive-wave-00-audit.md` and `docs/alkalive-wave-01-bugfixes.md` first.**

## Objective

Move the Roboto-Regular.ttf font asset from `alkalive-app` (the legacy CPU
renderer, which is dead code in the WASM pipeline) to `alkalive-backend-wgpu`
(the actual GPU renderer that uses it). This removes the cross-crate dependency
on a dead crate's asset.

## What was implemented

- Created `crates/alkalive-backend-wgpu/assets/` directory
- Copied `crates/alkalive-app/assets/Roboto-Regular.ttf` → `crates/alkalive-backend-wgpu/assets/Roboto-Regular.ttf`
- Updated both `include_bytes!` references in `crates/alkalive-backend-wgpu/src/lib.rs`
  to use the new local path (`../assets/` instead of `../../alkalive-app/assets/`)

## Files changed

- `crates/alkalive-backend-wgpu/assets/Roboto-Regular.ttf` (new — copied)
- `crates/alkalive-backend-wgpu/src/lib.rs` — 2 path references updated

## Tests executed

- `cargo build --workspace`: clean
- `cargo test --workspace`: **1148 passed, 0 failed**

## DoD checklist

- [x] Font asset moved to backend crate
- [x] All `include_bytes!` references updated
- [x] Build clean
- [x] All 1148 tests pass
- [x] No regressions

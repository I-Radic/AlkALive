# Security Audit Report — AlkALive

**Date:** 2026-07-31
**Auditor:** Automated + manual review
**Scope:** All 13 AlkALive crates + 2 vendored crates (harfrust, rasterizer)

## Executive Summary

The codebase has **zero `unsafe` code blocks** across all 15 crates (13 AlkALive + harfrust + rasterizer). All crates enforce `#![forbid(unsafe_code)]`. The primary security risk is a **Critical XSS vulnerability** in the DOM bridge's HTML snapshot generation, where user-supplied title and meta content are interpolated into HTML without escaping. Additional Medium-severity issues include unbounded IPC queues, missing font/text size limits, and missing subscriber limits.

---

## Findings

### SEC-01: XSS in DomBridge::serve_snapshot (CRITICAL)

- **Severity:** Critical
- **File:** `crates/alkalive-dom/src/lib.rs` (lines ~230-250)
- **Description:** `serve_snapshot()` builds HTML by directly interpolating `self.title`, `name`, and `content` into `<title>`, `<meta name="...">`, and `content="..."` tags without any HTML escaping. An attacker who controls the title (e.g., via `set_title("...</title><script>alert(1)</script>")`) or meta content can inject arbitrary HTML/JavaScript into the SEO snapshot.
- **Impact:** Arbitrary JavaScript execution in the browser of any user or crawler viewing the SEO snapshot. This violates ADR 020's "thin DOM layer" guarantee.
- **Fix:** HTML-escape all interpolated strings (`<`, `>`, `&`, `"`, `'`).

### SEC-02: LocalIPCSocket unbounded queue — DoS via memory exhaustion (MEDIUM)

- **Severity:** Medium
- **File:** `crates/alkalive-ipc/src/lib.rs` (lines 342-348)
- **Description:** `send()` pushes to an unbounded `VecDeque` without checking `queue.len() < capacity()`. The `capacity()` method returns 1024 but `send()` never enforces it. A malicious or buggy sender can push unlimited messages, exhausting memory.
- **Impact:** Denial of service via memory exhaustion.
- **Fix:** Check `queue.len() >= capacity()` before push; return `Err(ChannelError::Backpressure)` if full.

### SEC-03: No font file size limit in HarfRustFontRegistry (MEDIUM)

- **Severity:** Medium
- **File:** `crates/alkalive-text/src/lib.rs` (`load_bundle`)
- **Description:** `load_bundle()` accepts arbitrary-length byte slices with no size validation. A 1GB "font" file would be loaded into WASM heap memory, potentially causing OOM.
- **Impact:** Denial of service via memory exhaustion.
- **Fix:** Add a `MAX_FONT_SIZE` constant (e.g., 50 MB); reject inputs exceeding it with `FontLoadError::TableDecodeFailed`.

### SEC-04: No text input length limit in HarfRustTextShaper (MEDIUM)

- **Severity:** Medium
- **File:** `crates/alkalive-text/src/lib.rs` (`shape`)
- **Description:** `shape()` accepts arbitrary-length strings. A 100MB string could cause slow shaping or memory exhaustion in the HarfRust buffer.
- **Impact:** Denial of service via CPU/memory exhaustion.
- **Fix:** Add a `MAX_TEXT_LENGTH` constant (e.g., 1 MB); reject inputs exceeding it with `ShapeError::InvalidUtf8` (or a new `TooLong` variant).

### SEC-05: No subscriber limit on Signal (LOW)

- **Severity:** Low
- **File:** `crates/alkalive-core/src/lib.rs` (`Signal::subscribe`)
- **Description:** `subscribe()` registers unlimited listeners. A malicious module could register millions of listeners, causing slow `emit()` dispatch.
- **Impact:** Performance degradation; potential DoS in extreme cases.
- **Fix:** Add a `MAX_SUBSCRIBERS` constant (e.g., 1024); reject excess subscriptions.

### SEC-06: HarfRust vendored crate — supply chain review (INFO)

- **Severity:** Info (no action needed)
- **File:** `vendor/harfrust/harfrust/src/lib.rs`
- **Description:** HarfRust is vendored as a git subtree from `https://github.com/harfbuzz/harfrust` (MIT license). It enforces `#![forbid(unsafe_code)]` — 0 actual `unsafe` blocks. The "188 unsafe" grep matches are method names (`unsafe_to_break`, `unsafe_to_concat`, `flag_unsafe`), not Rust `unsafe` blocks.
- **Transitive deps:** `read-fonts`, `bitflags`, `bytemuck`, `smallvec`, `font-types`, `once_cell`, `proc-macro2`, `quote`, `syn`, `unicode-ident` — all well-maintained, widely-used crates.
- **Conclusion:** No supply-chain risk identified.

### SEC-07: Rasterizer vendored crate — safe code (INFO)

- **Severity:** Info (no action needed)
- **File:** `vendor/rasterizer/src/lib.rs`
- **Description:** The rasterizer enforces `#![forbid(unsafe_code)]` — 0 `unsafe` blocks. No external dependencies. No raw pointers, no `transmute`, no FFI.
- **Conclusion:** No risk identified.

### SEC-08: All AlkALive crates — safe code (INFO)

- **Severity:** Info (no action needed)
- **Description:** All 13 AlkALive crates enforce `#![forbid(unsafe_code)]`. 0 `unsafe` blocks, 0 `transmute`, 0 raw pointers. All `unwrap()`/`expect()`/`panic!` calls are in `#[cfg(test)]` modules only.
- **Conclusion:** No unsafe-code risk identified.

---

## Summary Table

| ID | Severity | Description | Status |
|----|----------|-------------|--------|
| SEC-01 | Critical | XSS in DomBridge HTML snapshot | **FIXED** |
| SEC-02 | Medium | Unbounded IPC queue (DoS) | **FIXED** |
| SEC-03 | Medium | No font file size limit | **FIXED** |
| SEC-04 | Medium | No text input length limit | **FIXED** |
| SEC-05 | Low | No Signal subscriber limit | **FIXED** |
| SEC-06 | Info | HarfRust supply chain | No action needed |
| SEC-07 | Info | Rasterizer safe code | No action needed |
| SEC-08 | Info | All AlkALive crates safe | No action needed |

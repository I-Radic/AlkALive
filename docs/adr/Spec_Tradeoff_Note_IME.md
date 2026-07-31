# Spec Trade-off Note: IME Composition-Event Acquisition

> **⚠ RESOLVED — superseded by [ADR 023](ADR_023_IME_Composition.md).** The project owner chose **Approach B** (narrowly-scoped hidden `<input>` exception), granting a scoped exception to ADR 020 for IME composition-event acquisition only. This file is retained for historical context; the live decision is ADR 023.

**Status:** RESOLVED (2026-07-31)
**Created:** 2026-07-26
**Resolved by:** [ADR 023](ADR_023_IME_Composition.md)
**Owning sections:** SPECIFICATION.md §6.7 (Text Rendering) and §9.5 (DOM Interop Layer)
**Conflicting ADRs:** ADR 020 (Metadata-Only DOM Layer) vs. the Rough Draft's prior hidden-`<input>` IME design

---

The original conflict description, candidate approaches, and rationale are preserved below for historical reference. The decision is documented in [ADR 023](ADR_023_IME_Composition.md).

## The Conflict

**ADR 020** restricts the host-DOM surface to `<title>`, `<meta>`, and a static SEO snapshot, with **no DOM-tree interaction for input**. This explicitly forbids the hidden `<input>` element approach that the Rough Draft (§5) proposed for acquiring platform IME composition events (`compositionstart` / `compositionupdate` / `compositionend`).

However, **no ADR commits a replacement mechanism** for acquiring IME composition events without a DOM input element. The text stack (ADR 022 — forked HarfRust) exposes an `ime_compose(CompositionEvent) -> ImeState` interface, but the *acquisition* of those `CompositionEvent`s from the platform is unresolved.

This is a genuine architectural conflict: IME is essential for CJK-language text input, but ADR 020's metadata-only DOM rule precludes the standard browser IME acquisition path.

## Candidate Approaches (Historical)

- **Approach A: WASM-native platform input API (no DOM)** — not shippable today (EditContext API is experimental, Chrome-only).
- **Approach B: Narrowly-scoped hidden `<input>` exception (non-hot-path)** — CHOSEN by ADR 023.
- **Approach C: Defer IME entirely** — rejected (UX cost too high for CJK users).

## Resolution

ADR 023 adopts Approach B with a scoped exception to ADR 020. The hidden `<input>` is classified non-hot-path, carries composition state only, and is gated by `CapabilityId::ImeHandler`. See [ADR 023](ADR_023_IME_Composition.md) for full details.

# Spec Trade-off Note: IME Composition-Event Acquisition

**Status:** Open — pending a future ADR
**Created:** 2026-07-26
**Owning sections:** SPECIFICATION.md §6.7 (Text Rendering) and §9.5 (DOM Interop Layer)
**Conflicting ADRs:** ADR 020 (Metadata-Only DOM Layer) vs. the Rough Draft's prior hidden-`<input>` IME design

---

## The Conflict

**ADR 020** restricts the host-DOM surface to `<title>`, `<meta>`, and a static SEO snapshot, with **no DOM-tree interaction for input**. This explicitly forbids the hidden `<input>` element approach that the Rough Draft (§5) proposed for acquiring platform IME composition events (`compositionstart` / `compositionupdate` / `compositionend`).

However, **no ADR commits a replacement mechanism** for acquiring IME composition events without a DOM input element. The text stack (ADR 022 — forked HarfRust) exposes an `ime_compose(CompositionEvent) -> ImeState` interface, but the *acquisition* of those `CompositionEvent`s from the platform is unresolved.

This is a genuine architectural conflict: IME is essential for CJK-language text input, but ADR 020's metadata-only DOM rule precludes the standard browser IME acquisition path.

## Why This Reduces Confidence

- ADR 020 is High-confidence and non-negotiable (owner directive).
- IME support is a real-world requirement for any text-editing UI targeting CJK languages.
- No WASM-native platform input API currently provides IME composition events in browsers.
- The conflict cannot be resolved without either (a) a platform API that does not yet exist, (b) a formal exception to ADR 020, or (c) accepting that IME is unsupported in the initial release.

## Candidate Approaches

### Approach A: WASM-native platform input API (no DOM)

**Description:** Acquire IME composition events via a future WASM interface or browser extension that exposes platform input events directly to WASM, with no DOM element involved.

**Pros:**
- Fully consistent with ADR 020 (no DOM input interop).
- Clean architectural boundary; no exceptions.
- Future-proof: aligns with the WASM-native direction.

**Cons:**
- No such API exists today in standard browsers.
- Requires either a browser extension (deployment burden) or waiting for a future WASM interface (unbounded timeline).
- IME support blocked indefinitely until the API materializes.

### Approach B: Narrowly-scoped hidden `<input>` exception (non-hot-path)

**Description:** Create a formal exception to ADR 020 for a single hidden `<input>` element that carries *composition state only* — no text rendering, no UI state, no layout participation. Classified explicitly as non-hot-path (composition events are per-keystroke user input, not per-frame rendering).

**Pros:**
- Reuses the browser's contractual IME pipeline (correctness inherited, not re-derived).
- Lowest implementation risk; the hidden-`<input>` approach is well-understood.
- Composition state only — the element is an event source, not a render target.
- Can be hidden behind the `ime_compose` interface so the text stack is unaffected.

**Cons:**
- Requires a formal exception to ADR 020 (amending ADR or a new ADR).
- Introduces a narrow DOM surface for input, which ADR 020 explicitly forbade.
- Must be carefully scoped to prevent scope creep (any DOM input could leak).

### Approach C: Defer IME entirely

**Description:** Ship the initial release without IME support. The text stack's `ime_compose` interface remains, but no acquisition mechanism is provided. CJK text input falls back to direct character insertion (no composition).

**Pros:**
- No ADR 020 exception needed; no architectural compromise.
- Simplest to implement and ship.
- IME can be added later via Approach A or B without re-architecture (the `ime_compose` interface is stable).

**Cons:**
- CJK-language users cannot use composition-based input in the initial release.
- May block adoption in CJK markets.
- Text editing UX is degraded for a significant user population.

## Recommended Approach (if forced)

**Approach B**, with the constraint that the hidden `<input>` element:
1. Carries composition state only (no text rendering, no UI state).
2. Is explicitly excluded from the hot path (composition events are per-keystroke, not per-frame).
3. Is hidden behind the `ime_compose` interface so the text stack and render loop are unaffected.
4. Requires a formal ADR amending ADR 020 to grant this scoped exception.

**Rationale:** IME is essential for CJK text input; shipping without it (Approach C) risks blocking adoption in major markets. Approach A is architecturally cleanest but blocked on a platform API that does not exist. Approach B is the lowest-regret interim: it inherits browser IME correctness, is well-understood, and the `ime_compose` interface keeps the text stack decoupled from the acquisition mechanism. When a WASM-native input API matures, the hidden `<input>` can be replaced without touching the text stack.

**Until resolved:** The text stack exposes `ime_compose(CompositionEvent) -> ImeState` as a stable interface; the acquisition mechanism is pluggable behind it. No implementation ships in the initial release unless/until this trade-off is resolved via a new ADR.

## Resolution Path

A future ADR (proposed number: ADR 023) should be created to formally resolve this trade-off, choosing one of Approaches A/B/C and (if B) amending ADR 020 with a scoped exception. The ADR must specify:
- The exact acquisition mechanism.
- The non-hot-path classification (if B).
- The interface contract between the acquisition mechanism and `TextStack::ime_compose`.
- Any capability-scoping (ADR 018) required to gate IME access.

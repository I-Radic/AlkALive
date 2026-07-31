# ADR 023: IME Composition via Hidden Input Exception (Approach B)

> **Supersedes:** `Spec_Tradeoff_Note_IME.md` (resolved)
> **Amends:** ADR 020 (Metadata-Only DOM Layer for SEO) — grants a narrowly-scoped exception for IME composition-event acquisition only.

## Context

### The Conflict

**ADR 020** restricts the host-DOM surface to `<title>`, `<meta>`, and a static SEO snapshot, with **no DOM-tree interaction for input**. This explicitly forbids the hidden `<input>` element approach that the Rough Draft (§5) proposed for acquiring platform IME composition events (`compositionstart` / `compositionupdate` / `compositionend`).

The text stack (**ADR 022** — forked HarfRust) exposes an `ime_compose(CompositionEvent) -> ImeState` interface, but the *acquisition* of those `CompositionEvent`s from the platform is unresolved. This is a genuine architectural conflict: IME is essential for CJK-language text input (Chinese, Japanese, Korean — representing billions of users), but ADR 020's metadata-only DOM rule precludes the standard browser IME acquisition path.

### Why IME Is Non-Functional Without One of These Approaches

Browser IME composition events (`compositionstart`, `compositionupdate`, `compositionend`) are fired on **focused, editable DOM elements** (e.g., `<input>`, `<textarea>`, or elements with `contenteditable`). The browser's IME framework (e.g., IBus on Linux, IMM32 on Windows, Kotoeri on macOS) communicates with the active editing context through the browser's input method editor pipeline, which is fundamentally DOM-coupled:

1. **The browser selects an IME context** based on the currently focused element. If no editable element is focused, the IME is inactive — no composition events are fired.
2. **Composition events are dispatched** to the focused element via the DOM event pipeline. They cannot be retargeted to a canvas, WebGPU surface, or WASM module.
3. **The IME candidate window** (the popup showing character options during composition) is positioned relative to the focused element's bounding rect. Without a DOM element, the candidate window has no anchor point.

WebAssembly alone cannot receive IME events without JavaScript/DOM glue because:
- WASM has no DOM access — it operates in a linear-memory sandbox.
- The `wasm-bindgen` / `web-sys` bindings that expose browser APIs all go through the DOM.
- There is no WASM-native event interface for input method composition.

### What Current Web Specifications Propose (and Their Status)

- **EditContext API** (WICG proposal): Aims to decouple text input from DOM editing hosts, allowing arbitrary rendering targets (including canvas) to receive composition events. **Status:** Experimental. Available behind a flag in Chrome 121+ (January 2024); not implemented in Firefox or Safari as of mid-2026. The API surface is still evolving and not production-ready.
- **InputMethodContext API** (deprecated): An earlier attempt at exposing IME state to non-DOM contexts. **Status:** Removed from browsers; never achieved cross-browser support.
- **TextEvent** (deprecated): An early DOM Level 3 proposal for text input events. **Status:** Superseded by `InputEvent` and composition events; does not solve the DOM-focus requirement.

**Conclusion:** No cross-browser, production-ready API exists for receiving IME composition events without a focused DOM element. Approach A is not shippable today.

## Decision

**Adopt Approach B: Narrowly-scoped hidden `<input>` exception.**

Create a formal, minimal exception to ADR 020 for a single hidden `<input>` element used solely as an IME composition event source. This element:
- Carries **composition state only** — no text rendering, no UI state, no layout participation.
- Is classified explicitly as **non-hot-path** (composition events are per-keystroke user input, not per-frame rendering).
- Is hidden behind the `TextStack::ime_compose` interface so the text stack and render loop are unaffected.
- Is the **sole** DOM input surface permitted in the entire runtime.

When the EditContext API (or equivalent) achieves cross-browser production support, the hidden `<input>` will be replaced with a WASM-native binding — no text-stack changes required.

## Alternatives

### Approach A: WASM-Native Platform Input API (No DOM)

**Description:** Acquire IME composition events via a future WASM interface (e.g., EditContext API) or browser extension, with no DOM element involved.

**Pros:**
- Fully consistent with ADR 020 (no DOM input interop).
- Clean architectural boundary; no exceptions.
- Future-proof: aligns with the WASM-native direction.
- No DOM focus management needed.

**Cons:**
- **No such API exists today in standard browsers** (EditContext is experimental, Chrome-only, behind a flag).
- Requires either a browser extension (deployment burden, security review, store distribution) or waiting for a future WASM interface (unbounded timeline — could be years).
- IME support blocked indefinitely until the API materializes and achieves cross-browser parity.
- Even when EditContext ships, it will need WASM bindings, polyfill for older browsers, and a migration path.

**Migration plan from B to A:** When EditContext achieves production support in ≥2 major browsers (Chrome + Firefox/Safari), implement an `EditContextImeHandler` behind the same `ime_compose` interface, deprecate the hidden `<input>` handler, and remove the ADR 020 exception. The text stack (`TextStack::ime_compose`) is unchanged.

### Approach B: Narrowly-Scoped Hidden `<input>` Exception (Non-Hot-Path) — CHOSEN

**Description:** Create a formal exception to ADR 020 for a single hidden `<input>` element that carries composition state only.

**Pros:**
- Reuses the browser's contractual IME pipeline (correctness inherited from decades of browser IME development, not re-derived).
- Lowest implementation risk; the hidden-`<input>` approach is well-understood and widely used by canvas-based editors (Figma, Google Docs canvas mode, Monaco editor).
- Composition state only — the element is an event source, not a render target. No visible UI, no CSS, no layout participation.
- The `ime_compose` interface decouples the text stack from the acquisition mechanism.
- Shippable today with zero platform dependencies.

**Cons:**
- Requires a formal exception to ADR 020 (this ADR grants that exception).
- Introduces a narrow DOM surface for input, which ADR 020 explicitly forbade — but the exception is scoped to IME composition events only, not general input.
- Must be carefully scoped to prevent scope creep: the `<input>` is invisible, has no CSS, participates in no layout, and handles only `compositionstart`/`compositionupdate`/`compositionend`/`compositionend` events. No `input`, `keydown`, `keyup`, `change`, or `focus`/`blur` events are routed through it (those go through the ADR 010 input system).

### Approach C: Defer IME Entirely

**Description:** Ship without IME support; CJK text input falls back to direct character insertion.

**Pros:**
- No ADR 020 exception needed.
- Simplest to implement.

**Cons:**
- CJK-language users cannot use composition-based input — a critical UX regression for billions of users.
- Blocks adoption in CJK markets (China, Japan, Korea represent ~1.5 billion internet users).
- Text editing UX is fundamentally broken for these languages (no candidate window, no composition preview).

**Decision:** Rejected. The UX cost is too high for a shippable product.

## Consequences

### DOM Surface Change

The `DomBridge` trait (ADR 020) gains one new method, `register_ime_handler`, which:
- Creates a single hidden `<input>` element (if not already created).
- Positions it off-screen (e.g., `position: absolute; left: -9999px; opacity: 0;`).
- Attaches `compositionstart` / `compositionupdate` / `compositionend` event listeners.
- Forwards composition events as serialised `CompositionEvent` structs to the WASM text stack via the `ime_compose` interface.
- Manages focus: the `<input>` receives focus when a text-editing render object is activated; loses focus when deactivated.

The `DomBridge` method set becomes: `{set_title, set_meta, serve_snapshot, declare_routes, serialize_state, register_ime_handler}`. The `register_ime_handler` method is the sole IME surface; no other DOM input methods are permitted.

### ADR 020 Exception Scope

This ADR grants a **scoped exception** to ADR 020 for IME composition-event acquisition only. The exception is:
- **Limited to one hidden `<input>` element** per runtime instance.
- **Non-hot-path**: composition events are per-keystroke user input, not per-frame rendering. The hidden `<input>` does not participate in the render loop, layout, or paint pipeline.
- **Event-source only**: the `<input>` carries no text content, no UI state, and no style. It exists solely to receive platform IME events.
- **Interface-isolated**: the text stack's `ime_compose(CompositionEvent) -> ImeState` interface is the sole consumer. No other subsystem touches the IME handler.

### ADR 019 (Accessibility) Interaction

ADR 019 defers accessibility. The hidden `<input>` is **not** an accessibility surface — it does not expose ARIA roles, focus management, or screen-reader contracts. Accessibility remains deferred per ADR 019. When a11y is un-deferred, the hidden `<input>` may be removed entirely (replaced by the EditContext API or a platform a11y bridge).

### ADR 022 (HarfRust) Interaction

The text stack's `TextStack::ime_compose(CompositionEvent) -> ImeState` interface is unchanged. The `CompositionEvent` struct (already defined in `alkalive-text`) carries `text: String`, `caret: u32`, and `replace_range: (u32, u32)`. The hidden `<input>` handler serialises browser composition events into this struct. The text stack processes them identically regardless of acquisition mechanism.

### ADR 013 (No WASM↔DOM Boundary in Hot Path) Interaction

The hidden `<input>` is explicitly classified as **non-hot-path** per ADR 013's definition: "composition events are per-keystroke user input, not per-frame rendering." The WASM↔DOM boundary crossing occurs only when a composition event fires (user input), not during the frame loop (`tick()`). This is consistent with ADR 013's existing classification of "accessibility-tree mutation, navigation/state serialization, and SEO export" as non-hot-path.

### ADR 018 (Capability-Scoped Imports) Interaction

The `register_ime_handler` method must be gated by a capability grant. Only the text stack (or a dedicated IME subsystem) may call it. The capability scope is: `CapabilityId::ImeHandler`. This prevents arbitrary modules from creating DOM input elements.

## Confidence

**High** for Approach B's feasibility today. The hidden-`<input>` technique is battle-tested (Figma, Google Docs, Monaco editor all use it). The `ime_compose` interface is already defined and stable. The implementation risk is low.

**Medium** for the migration path to Approach A. The EditContext API is promising but experimental; its timeline and final API surface are uncertain. The migration plan is sound but its execution depends on external browser-vendor schedules.

## Cross-References

- **ADR 020** (Metadata-Only DOM Layer) — amended by this ADR with a scoped exception for IME composition-event acquisition.
- **ADR 022** (Forked HarfRust) — provides the `TextStack::ime_compose` interface that consumes the composition events.
- **ADR 013** (No WASM↔DOM Boundary in Hot Path) — the hidden `<input>` is classified non-hot-path.
- **ADR 019** (Accessibility Deferred) — the hidden `<input>` is not an a11y surface; a11y remains deferred.
- **ADR 018** (Capability-Scoped Imports) — `register_ime_handler` is capability-gated.
- **SPECIFICATION.md §6.7** (IME Open Dependency) — now resolved by this ADR.
- **SPECIFICATION.md §9.5** (IME — No Exception) — superseded; a scoped exception is now granted.
- **SPECIFICATION.md §12.8** (Open Dependencies item 2) — IME composition-event acquisition is now resolved.

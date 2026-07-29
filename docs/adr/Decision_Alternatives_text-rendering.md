# Decision Alternatives: Text Rendering Strategy

> **⚠ RESOLVED — superseded by [ADR 022](ADR.md#adr-022-forked-harfrust-in-wasm-text-stack).** The project owner chose **Approach A (forked HarfRust in-WASM stack)**, which overrides this file's prior "Recommended Approach B (hidden DOM surface)". This file is retained for historical context only; the live decision is ADR 022.

## Decision Point

Choose the off-DOM text stack that satisfies contractual shaping, BiDi, selection, IME, and a11y for a WASM+GPU UI that abandons the DOM.

## Why Uncertain

P3.5 is the catalog's decisive hard problem: these properties are contractual only on DOM text nodes; `canvas.fillText` provides none. The optimal off-DOM implementation is unresolved, and the whole architecture depends on it.

## Approach A: In-WASM text stack (HarfBuzz/WASM + bespoke BiDi/selection), thin IME+a11y bridges

**Pros:** Full control, deterministic cross-platform parity, no DOM dependency, aligns with ADR_007's pure-object model (text as GPU-backed glyphs).
**Cons:** Highest build cost; BiDi/segmentation/IME composition must be hand-rolled; a11y bridges (AT via platform APIs) are thin and fragile; large WASM payload (~HarfBuzz). Risk of subtle correctness drift per platform.

## Approach B: Platform-native text via a hidden DOM text surface (offscreen contenteditable / canvas-DOM hybrid)

**Pros:** Reuses the browser's contractual shaping/BiDi/selection/IME/a11y verbatim; lowest risk for P3.5; selection and caret "just work"; composes with ADR_004's layout engine by reading measured runs.
**Cons:** Reintroduces a DOM dependency the architecture set out to abandon; sync between hidden surface and GPU compositor adds latency/state-duplication; contradicts the pure off-DOM thesis and complicates ADR_007's single-source-of-truth object model.

## Approach C: Delegate to a host text-rendering service via a narrow FFI

**Pros:** Pushes complexity to a host (native shaper or browser service) behind a small interface; keeps WASM lean; swappable backends.
**Cons:** Interface is the new hard problem — serializing runs, metrics, caret rects, IME state, and a11y trees across FFI is itself a mini-DOM; loses cross-platform determinism unless the host is pinned; weakest fit for ADR_004 layout (which assumes in-process synchronous measurement).

## Recommended Approach

**Approach B**, reluctantly, as the forced choice. P3.5 is decisive and contractual; B is the only option that inherits correctness rather than re-deriving it. Mitigate the DOM-reintroduction cost by confining the hidden surface to an opaque measurement+IME+a11y leaf — never a layout participant — so ADR_004's layout and ADR_007's object model retain authority over geometry. This also dovetails with `Decision_Alternatives_accessibility-bridge`, which already assumes a DOM-backed AT path. A and C remain viable upgrades once an in-WASM shaper proves BiDi/IME/a11y parity; until then B is the lowest-regret bet.

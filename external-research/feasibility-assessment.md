# VUMA Feasibility Assessment for AlkALive

**Date:** 2026-08-02
**Question:** Can VUMA serve as the kernel for AlkALive?

---

## Executive Summary

**No. VUMA cannot realistically serve as AlkALive's kernel today, and adopting it would require a minimum 18-26 month commitment to build missing infrastructure that AlkALive already has.**

The VUMA stack (VUMA + WOMB + VEEE + COSS) is a well-engineered, verification-first systems architecture with a real compiler (93.67% test pass rate, Lean-verified memory safety, 19 backends including wasm32 at 100%). However, three load-bearing pieces required by AlkALive do not exist:

1. **No WebGL2/WebGPU GPU dispatch path** — WOMB only wraps native Vulkan + Metal
2. **No browser IME/a11y without DOM** — WOMB's bridges are OS-native (IBus/IMM32/IMK, AT-SPI/UIA/NSAccessibility)
3. **No UI engine** — `womb/ui/` is 100% greenfield (~99 person-weeks to v1)

Additionally, AlkALive's existing implementation already provides what VUMA promises (WASM compilation, GPU rendering, text shaping, input handling) using a simpler, working architecture. Replacing it with VUMA would mean discarding working code for unbuilt code.

**Recommendation: Reject VUMA as a kernel. Adopt VUMA's PL ideas (incremental computation, monotonicity types, e-graph optimization, PMT verification) selectively in AlkALive's own compiler.**

---

## 1. Can VUMA Serve as AlkALive's Kernel?

### 1.1 The Layered Architecture Question

AlkALive's current stack:
```
.alk source → alkalive-compiler → SceneIR → alkalive-runtime-wasm → alkalive-backend-wgpu (WebGL2) → canvas
```

Proposed VUMA-based stack:
```
.alk source → alkalive-compiler → VUMA AST → VUMA pipeline → wasm32 → WOMB UI engine → gpu_dispatch → ??? → canvas
```

The proposed stack has **four additional layers** (VUMA AST, VUMA pipeline, WOMB, gpu_dispatch) between AlkALive's compiler and the canvas. Each layer adds:
- Compilation time (VUMA's 10-stage pipeline vs. AlkALive's direct WASM emission)
- Binary size (WOMB substrate is 117 kLOC of VUMA source)
- Complexity (PMT arena, Z3 contracts, Lean proofs, capability tokens)
- Dependencies (Z3 solver, Lean theorem prover)

### 1.2 The GPU Rendering Gap (Fatal)

AlkALive renders directly to a WebGL2 canvas from WASM. This is the core architectural decision — zero DOM, zero application JS, GPU rendering from inside the WASM module.

VUMA/WOMB's GPU path:
```
WOMB gpu_dispatch.vuma → extern "C" host import → C host runtime (gpu_vulkan.c / gpu_metal.m) → native GPU API
```

In a browser, this would require:
- A `gpu_webgl2.c` or `gpu_webgpu.c` host shim (~3 kLOC, not written)
- A SPIR-V → WGSL/GLSL translator (not written)
- V-26 (const byte arrays) to embed SPIR-V in the WASM binary (deferred, 2-week fix)
- The compute-shader tessellation renderer (vello-style) to work via WebGL2 (untested)

**This is a fundamental impedance mismatch.** AlkALive's architecture is "WASM drives WebGL2 directly." VUMA's architecture is "VUMA binary calls C host shim that calls native GPU API." These are incompatible without building an entirely new GPU dispatch layer.

### 1.3 The Text Stack Gap

AlkALive uses HarfRust (vendored HarfBuzz) for text shaping — full OpenType GSUB/GPOS, BiDi, glyph atlas with real rasterization. This works today.

WOMB's text plan:
- v1: cmap + hmtx only (60% language coverage, no GSUB/GPOS, no BiDi)
- v2: Port from rustybuzz (95% coverage, ~10 person-weeks)
- BiDi: Port from unicode-bidi (~5 person-weeks)

**AlkALive would lose text rendering quality by moving to WOMB.** The WOMB v1 text path is materially weaker than AlkALive's current HarfRust integration.

### 1.4 The Browser IME Gap (Fatal)

AlkALive uses a hidden `<input>` element per ADR 023 to receive IME composition events. This is the only DOM element besides the canvas.

WOMB's IME plan:
- OS-native bridges: libibus (Linux), IMM32 (Windows), IMK (macOS)
- Session-typed channel (V-11, deferred)
- **No browser bridge** — the drafts do not address how IME works in a browser without OS-native input method frameworks

This is an **unresolved design gap**. Browser IME requires DOM interaction (composition events fire on focused DOM elements). AlkALive's ADR 023 approach (hidden `<input>` exception) is the industry-standard solution (used by Figma, Google Docs canvas mode, Monaco editor). VUMA/WOMB has no equivalent.

---

## 2. Architecture Mapping

### 2.1 Component-by-Component Mapping

| AlkALive Component | VUMA Stack Equivalent | Fit | Notes |
|---|---|:---:|---|
| `.alk` source language | VEEE `.veee` language | ⚠️ | Different paradigms: AlkALive is module/object-oriented; VEEE is reactive/monotone |
| `alkalive-compiler` (lexer/parser/codegen) | VEEE compiler (veeec) | ⚠️ | VEEE lowers to VUMA AST, not WASM directly; adds VUMA pipeline |
| SceneIR (scene description) | VEEE `render` block → SceneNode tree | ⚠️ | SceneNode tree ≠ render-graph IR; different abstraction level |
| `alkalive-runtime-wasm` (frame loop + input) | WOMB frame scheduler | ❌ | WOMB UI engine does not exist; frame scheduler is greenfield |
| `alkalive-backend-wgpu` (WebGL2) | WOMB `gpu_dispatch` | ❌ | No WebGL2/WebGPU dispatch path; Vulkan/Metal only |
| `alkalive-text` (HarfRust) | WOMB `shaper_v1.vuma` | ❌ | WOMB v1 is cmap+hmtx only; AlkALive has full HarfBuzz |
| `alkalive-render` (render-graph IR) | VUMA IR + SceneNode tree | ⚠️ | Different abstraction; VUMA IR is general-purpose, not render-specific |
| `alkalive-input` (event model + focus) | WOMB `event/dispatch.vuma` | ❌ | Greenfield; no browser IME path |
| `alkalive-dom` (ADR 023 IME bridge) | WOMB `ime/textfield.vuma` | ❌ | OS-native only; no browser bridge |
| WASM compilation | VUMA wasm32 backend | ✅ | 100% test pass rate; this works |
| Memory safety | PMT arena + Lean proof | ✅ | Stronger than AlkALive's current `#![forbid(unsafe_code)]` |
| Module isolation | Capability tokens (HMAC-SHA-256) | ⚠️ | Currently unsound (P0 security cluster); heavier than AlkALive needs |

### 2.2 Layered Architecture Diagram (If Adopted)

```
┌─────────────────────────────────────────────────┐
│ AlkALive .alk source                            │  ← AlkALive's language
├─────────────────────────────────────────────────┤
│ alkalive-compiler (modified to emit VUMA AST)   │  ← New codegen target
├─────────────────────────────────────────────────┤
│ VEEE concepts (optional: signals, derive)       │  ← Could adopt selectively
├─────────────────────────────────────────────────┤
│ VUMA compiler (10-stage pipeline + Z3 + Lean)   │  ← Real, but pre-v1
├─────────────────────────────────────────────────┤
│ WOMB UI engine (DOES NOT EXIST)                 │  ← 99 person-weeks to build
│  - event/ layout/ render/ text/ ime/ a11y/      │
├─────────────────────────────────────────────────┤
│ Browser GPU shim (DOES NOT EXIST)               │  ← ~3 kLOC new code
│  - gpu_webgl2.c or gpu_webgpu.c                 │
│  - SPIR-V → WGSL/GLSL translator               │
├─────────────────────────────────────────────────┤
│ Browser host runtime (DOES NOT EXIST)           │  ← IME/a11y design gap
│  - IME without DOM (unresolved)                 │
│  - a11y without DOM (unresolved)                │
├─────────────────────────────────────────────────┤
│ WASM module in browser                          │  ← This works
└─────────────────────────────────────────────────┘
```

**Three of the seven layers do not exist.** The bottom layer (WASM in browser) works, but everything above the VUMA compiler is either greenfield or has critical gaps.

---

## 3. Critical Gaps and Contradictions

### Tier 1 — Fatal Blockers (Cannot ship AlkALive without these)

| # | Gap | Layer | Effort | Why it's fatal |
|---|-----|-------|--------|----------------|
| 1 | No WebGL2/WebGPU GPU dispatch | WOMB | ~3 kLOC + SPIR-V translator | AlkALive's core architecture is "WASM drives WebGL2" |
| 2 | No browser IME without DOM | WOMB | Open design problem | CJK text input is impossible without this |
| 3 | `womb/ui/` does not exist | WOMB | ~99 person-weeks | No UI engine to host AlkALive |
| 4 | V-26 const byte arrays deferred | VUMA | 2 weeks | Can't embed SPIR-V/fonts/BiDi tables in WASM |
| 5 | VEEE does not exist | VEEE | Month 26 target | The UX language layer is unbuilt |

### Tier 2 — Significant Blockers (Major work required)

| # | Gap | Layer | Effort | Impact |
|---|-----|-------|--------|--------|
| 6 | V-16 security cluster (P0) | VUMA | 7 weeks | Capability model is unsound |
| 7 | V-03 IVE unsoundness for nested layouts | VUMA | 2 weeks | Layout trees would be "verified" incorrectly |
| 8 | WOMB text v1 is weaker than HarfRust | WOMB | 10+ weeks | Loses GSUB/GPOS/BiDi; AlkALive regresses |
| 9 | No browser a11y without DOM | WOMB | Open design problem | Screen readers broken in browser |
| 10 | V-A2-3 SIMD vectorizer broken | VUMA | 2 weeks | Text shaping 2-3× slower |

### Tier 3 — Architectural Mismatches (Design tensions)

| # | Mismatch | Impact |
|---|----------|--------|
| 11 | SceneIR (render-graph) vs SceneNode tree (scene tree) | Different abstraction levels; migration requires redesign |
| 12 | AlkALive's module/object model vs VEEE's signal/derive model | Different paradigms; not directly compatible |
| 13 | AlkALive's direct WASM emission vs VUMA's 10-stage pipeline | Added compilation complexity and time |
| 14 | AlkALive's `#![forbid(unsafe_code)]` vs VUMA's `unsafe` for Z3 FFI | Z3 requires `unsafe` FFI; AlkALive would need to accept this |
| 15 | AlkALive's HarfRust (vendored, 50K LOC) vs WOMB's from-scratch text | AlkALive would throw away working text for unbuilt text |
| 16 | Single-threaded v1 only | AlkALive's frame loop, input, and rendering share one thread |

---

## 4. Realism Assessment

### 4.1 Timeline

| Milestone | Target | Confidence |
|-----------|--------|:---:|
| VUMA v1 (P0 fix + V-03 + V-26 + V-11) | Month 18 | Medium (7-week security cluster is open) |
| WOMB v1 (all 8 UI sub-modules) | Month 18 | Low (99 person-weeks of greenfield work) |
| Browser GPU shim (WebGL2 dispatch) | Not scoped | N/A (not in any plan) |
| Browser IME/a11y design | Not scoped | N/A (open design problem) |
| VEEE v0.1 | Month 26 | Low (depends on VUMA v1 + WOMB v1) |
| **AlkALive on VUMA** | **Month 26+** | **Low** |

### 4.2 Effort Estimate

To host AlkALive on VUMA, the following work is needed (beyond what VUMA/WOMB/VEEE already plan):

| Work item | Effort | Dependency |
|-----------|--------|------------|
| `gpu_webgl2.c` host shim | 3-4 weeks | WOMB gpu_dispatch ABI stable |
| SPIR-V → WGSL/GLSL translator | 4-6 weeks | Or hand-write WGSL directly (bypass SPIR-V) |
| Browser IME design + implementation | 4-8 weeks | Novel design; no prior art in VUMA stack |
| Browser a11y design + implementation | 4-8 weeks | Novel design; no prior art in VUMA stack |
| AlkALive compiler → VUMA AST codegen | 4-6 weeks | VUMA AST API stable |
| Integration testing | 2-4 weeks | All above complete |
| **Total additional effort** | **21-36 weeks** | **5-9 months** |

This is on top of the 18-26 months for VUMA v1 + WOMB v1 + VEEE v0.1.

### 4.3 Risk-Weighted Recommendation

| Factor | Weight | VUMA | AlkALive (current) |
|--------|:---:|:---:|:---:|
| Works today | 30% | ❌ | ✅ |
| Formal verification | 15% | ✅ (PMT/Lean) | ❌ |
| GPU rendering in browser | 20% | ❌ | ✅ |
| Text quality (HarfRust) | 10% | ❌ (cmap+hmtx) | ✅ |
| IME in browser | 10% | ❌ | ✅ (ADR 023) |
| Multi-backend (native+web) | 5% | ✅ (19 backends) | ❌ (WASM only) |
| Timeline to usable | 10% | 26+ months | 0 months (works now) |
| **Weighted score** | 100% | **15%** | **85%** |

---

## 5. Recommended Next Steps

### Option A: Reject VUMA as Kernel (Recommended)

Keep AlkALive's existing architecture. Selectively adopt VUMA's best ideas:

1. **Adopt incremental computation (Salsa-style)** in `alkalive-compiler` — signal-change-driven reactivity without virtual-DOM diffing
2. **Adopt monotonicity types** for AlkALive's module system — compile-time enforcement of "this collection only grows"
3. **Adopt e-graph optimization** with `state_store_load_forward` — optimize signal read/write patterns
4. **Study PMT verification** as a future direction — if AlkALive ever needs formal memory-safety proofs, VUMA's Lean approach is the reference
5. **Study VEEE's algorithm/schedule separation** — AlkALive's SceneIR could benefit from separating "what to render" from "how to render it"

**Effort: 2-4 weeks of design study, no new dependencies, no architectural changes.**

### Option B: Use VUMA as WASM Backend Only

Keep AlkALive's compiler, text, input, and WebGL2 backend. Replace only the WASM compilation path with VUMA's wasm32 backend to inherit PMT verification.

**Problem:** VUMA's wasm32 backend takes VUMA AST as input, not arbitrary Rust/WASM. AlkALive would need to emit VUMA AST from its compiler, which means understanding VUMA's AST format, transform model, and PMT arena layout. This is a deep integration that effectively means "become a VUMA frontend."

**Effort: 3-6 months. Gains: PMT verification. Loses: direct control over WASM emission.**

### Option C: Full VUMA Adoption (Not Recommended)

Reimplement AlkALive as a VEEE/WOMB application. Discard all existing AlkALive crates.

**Effort: 26+ months. Gains: PMT verification, 19 backends, formal contracts. Loses: working WebGL2, HarfRust, ADR 023 IME, all existing code.**

---

## 6. Conclusion

VUMA is a **credibly-engineered research project** with a real compiler, formal verification, and a thoughtful three-layer architecture. The PMT memory-safety verification (Lean, 280 theorems, 0 sorries) is genuinely impressive and unique among UI stacks.

However, **VUMA is not a viable kernel for AlkALive** because:

1. **The GPU path is incompatible** — VUMA/WOMB targets native Vulkan/Metal, not browser WebGL2/WebGPU. Building a browser GPU dispatch layer is a 3-6 month project not scoped in any VUMA plan.

2. **The browser IME/a11y gap is unresolved** — WOMB's IME and a11y bridges are OS-native only. Browser IME without DOM is an open design problem that VUMA does not address. AlkALive's ADR 023 (hidden `<input>` exception) is the proven solution.

3. **The UI engine does not exist** — `womb/ui/` is 100% greenfield. 99 person-weeks of UI engine development is needed before AlkALive could run on WOMB.

4. **AlkALive already works** — AlkALive has a working compiler, WebGL2 backend, HarfRust text shaping, input handling, and a deployed Hello World. Replacing working code with unbuilt code is not a feasibility win.

5. **The timeline is 26+ months** — VUMA v1 + WOMB v1 + VEEE v0.1 + browser-specific work. AlkALive cannot wait 26 months for a kernel when it already has one.

**The most valuable takeaway from VUMA for AlkALive is conceptual, not implementational**: the four pillars of VEEE (incremental computation, monotonicity types, algorithm/schedule separation, e-graph optimization) and the PMT verification model are research-grade ideas that AlkALive can adopt in its own compiler without depending on VUMA as a runtime or kernel.

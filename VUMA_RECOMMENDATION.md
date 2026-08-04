# VUMA Recommendation Brief

**Decision:** Do NOT adopt VUMA as AlkALive's kernel.

**Date:** 2026-08-02

---

## One-Page Summary

### The Question
Can the VUMA stack (VUMA + WOMB + VEEE + COSS) serve as the kernel/runtime for AlkALive, replacing AlkALive's current `.alk → compiler → WASM → WebGL2` pipeline?

### The Answer
**No.** VUMA is architecturally sound but materially incomplete for AlkALive's needs. Three load-bearing pieces are missing:

1. **No browser GPU path** — WOMB only wraps native Vulkan/Metal. WebGL2/WebGPU dispatch does not exist and is not scoped in any VUMA plan.
2. **No browser IME/a11y** — WOMB's input and accessibility bridges are OS-native only (IBus/IMM32/IMK, AT-SPI/UIA/NSAccessibility). Browser IME without DOM is an unresolved design gap.
3. **No UI engine** — `womb/ui/` is 100% greenfield. 99 person-weeks of development are needed before any UI can render on WOMB.

### What VUMA Offers (Genuinely Valuable)
- **PMT memory-safety verification** (Lean, 280 theorems, 0 sorries) — unique among UI stacks
- **Z3 contract discharge** for `requires`/`ensures` clauses
- **wasm32 backend** at 100% test pass rate
- **Incremental computation** (Salsa/Adapton) and **monotonicity types** (Datafun)
- **E-graph optimization** with `state_store_load_forward`
- **Algorithm/schedule separation** (Halide-style)

### What AlkALive Already Has (Working Today)
- `.alk` compiler with lexer, parser, codegen, CLI
- WebGL2 GPU backend with GLSL shaders
- HarfRust text shaping (full OpenType GSUB/GPOS, BiDi, glyph atlas)
- Input field with IME bridge (ADR 023, hidden `<input>`)
- Deployed Hello World with rotating golden text + input field

### The Cost of Adoption
- **Timeline:** 26+ months (VUMA v1 + WOMB v1 + VEEE v0.1 + browser-specific work)
- **Additional effort:** 21-36 weeks beyond VUMA's own timeline
- **What gets discarded:** Working WebGL2 backend, HarfRust, ADR 023 IME, all existing code
- **What gets gained:** PMT verification, 19 backends, formal contracts — but none of these help with AlkALive's browser deployment

### Recommendation
**Reject VUMA as a kernel. Adopt VUMA's PL ideas selectively:**

1. Study **incremental computation** (Salsa-style) for AlkALive's reactivity model
2. Study **monotonicity types** for compile-time collection-growth enforcement
3. Study **e-graph optimization** for signal read/write pattern optimization
4. Study **PMT verification** as a future direction if formal memory safety becomes a requirement
5. Study **algorithm/schedule separation** for AlkALive's SceneIR

**These ideas can be adopted in AlkALive's own compiler without depending on VUMA as a runtime or kernel. No new dependencies, no architectural changes, no timeline impact.**

---

*See `external-research/feasibility-assessment.md` for the full analysis.*

# Individual Document Analysis — VUMA/VEEE/COSS/WOMB

**Date:** 2026-08-02
**Analysts:** Sub-Agents 1-A (VEEE) and 1-BCD (COSS/VUMA/WOMB)

---

## 1. VEEE (Verified Expression Evaluation Engine)

### What it is
VEEE is **not a runtime** — it is a **compile-time-only UI programming language** (Layer 3) that lowers `.veee` source to VUMA AST. The "Engine" refers to compile-time evaluation/verification machinery, not a runtime that executes inside WASM.

### Core concepts
- Reactive UI DSL built on four pillars: incremental computation (Salsa/Adapton), monotonicity types (Datafun), algorithm/schedule separation (Halide), e-graph optimization
- `signal T`, `derive { }`, `monotone set<T>`, `render app = `, `schedule { }`, `ui name = `
- All data lives in VUMA's PMT arena; no VEEE-specific runtime exists
- Output is VUMA AST → VUMA pipeline → native binary or WASM

### GPU/rendering
**VEEE has NO GPU capabilities.** All rendering is deferred to WOMB. GPU access is via `#[embed("file.spv")]` (precompiled SPIR-V) + WOMB's `gpu_dispatch` host import (Vulkan/Metal/WebGPU native). No direct WebGL2 canvas driving.

### Relation to AlkALive
- VEEE is conceptually similar to AlkALive's `.alk` language (both are UI DSLs that compile to WASM)
- AlkALive cannot "run on" VEEE — VEEE is a compiler, not a runtime
- AlkALive could emit VUMA AST (replacing its SceneIR→WASM path) to inherit PMT verification, but this loses AlkALive's existing WebGL2 backend, HarfRust text, and render-graph IR
- VEEE v0.1 is at month 26 of an unstarted plan

### Strengths for AlkALive
- Formal PMT memory-safety verification (Lean, 280 theorems, 0 sorries)
- Z3 contract discharge for requires/ensures clauses
- E-graph optimization with `state_store_load_forward`
- Incremental computation and monotonicity types (principled alternatives to virtual-DOM diffing)
- 19-backend codegen (including wasm32)

### Critical gaps
1. **No direct WebGL2/WebGPU canvas driving** — GPU path requires C host shim calling Vulkan/Metal
2. **No GPU DSL** — shaders are hand-written GLSL embedded as opaque SPIR-V
3. **No text rendering** — deferred to WOMB (cmap+hmtx only, no GSUB/GPOS/BiDi in v1)
4. **No IME** — deferred to WOMB
5. **None of this software exists** — VEEE v0.1 is month 26 of an unstarted plan
6. **Single-threaded only** in v1
7. **No formal verification of reactivity** — only type-checker enforced
8. **No WebGL2-specific optimizations** — WebGL2 is not a first-class target

---

## 2. VUMA (Layer 1 — Systems Language / Kernel)

### What it is
A **verification-first systems language** where every `transform` carries Z3-discharged contracts, every arena access is bounded by runtime `__oob_trap`, and the memory model (PMT) is formally specified in Lean. 10-stage compiler pipeline, 7 Rust crates, 19 CPU backends.

### What exists
- **VUMA compiler EXISTS** — real Cargo workspace, real Lean proof layer, ~63,000-test matrix, 93.67% pass rate
- **VUMA v1 is NOT shipped** — one P0 (security cluster) open, multiple P1s open
- **wasm32 backend: 100% test pass rate** (1,577/1,577)
- Self-compilation does NOT exist

### GPU/rendering
**None at Layer 1.** No GPU IR, no shader codegen, no canvas, no WebGL2. The only GPU-relevant capability is the wasm32 backend and deferred V-26 (const byte arrays for SPIR-V embedding).

### Memory model
PMT arena — single bump allocator, `State<T>` typed-state, compile-time memory safety verification (Lean), runtime `__oob_trap` for out-of-bounds. No GC, no stack/heap split.

### Critical gaps for AlkALive
1. V-34 fix exposed f32 bugs (materialize_f32_immediates) — AlkALive animations use f32 heavily
2. V-03 IVE unsoundness for nested layouts — blocks verified UI trees
3. V-26 const byte arrays deferred — can't embed SPIR-V/fonts in WASM
4. V-16 security cluster (P0) — capability model is currently "security theater"
5. No WebGPU/WebGL2 codegen at any layer

---

## 3. WOMB (Layer 2 — UI Engine Library)

### What it is
UI engine libraries written in VUMA source (`.vuma` files). VUMA compiles/verifies WOMB; VEEE calls WOMB primitives.

### What exists
- **WOMB substrate EXISTS** (117 kLOC): kernel, crypto, libc, collections, networking (broken)
- **WOMB UI engine is 100% GREENFIELD** — `womb/ui/` directory does not exist
- ~99 person-weeks to v1 (~9-11 months at 3-5 people)
- v1 target = month 18

### GPU/rendering
- Vector renderer (RFC-21): paths tessellated by compute shader, no glyph atlas, free HiDPI
- GPU access via `extern "C"` host imports: native Vulkan + native Metal
- **WebGL2/WebGPU are NOT in the WOMB plan** — would need a new ~3 kLOC wrap
- Canvas: none — WOMB renders directly to host window framebuffer

### Text/input/IME
- Text is the most mature planned module (~32 person-weeks): full OpenType parser, shaper v1 (cmap+hmtx, 60%), shaper v2 (rustybuzz port, 95%), BiDi, line breaking
- IME: ImeState machine + session-typed channel + 3 OS bridges (IBus/IMM32/IMK)
- **Browser IME without DOM is an unresolved design gap** — OS-native bridges don't apply

### Critical gaps for AlkALive
1. `womb/ui/` does not exist — 100% greenfield
2. No WebGL2/WebGPU dispatch path — must be written from scratch
3. No browser IME without DOM — open design problem
4. No browser a11y without DOM — open design problem
5. V-26 blocks SPIR-V/font embedding in WASM
6. Single-threaded v1 only

---

## 4. COSS (Cross-Layer Integration)

### What it is
**Not a code layer — a meta-document** capturing cross-layer dependencies, shared infrastructure ownership, timeline coupling, and risk register. The integration contract between the three layer-specific drafts.

### Key findings
- Critical path: VUMA Phase 2 (security, 7w) → Phase 3 (V-03, 2w) → Phase 5 (parser, 4w) → VUMA v1 (month 18) → WOMB v1 (month 18) → VEEE v0.1 (month 26)
- 6 cross-layer risks (R-1 through R-6) with probability × impact × mitigation
- Honest about what's broken: V-03 (IVE unsoundness), V-26 (const bytes), V-16 (security theater), V-WOMB-1 (broken net imports)
- Browser target is under-specified across all drafts — no "WebGL2 dispatch in browser", "browser IME without DOM", or "no disk for runtime SPIR-V load" in the risk register

### Strengths
- Explicit, empirically-verified dependency graph
- Clear ownership boundaries (no duplication)
- Honest risk register

---

## 5. Combined Assessment

### What's real today
| Component | Status |
|-----------|--------|
| VUMA compiler | Real, pre-v1 (1 P0, ~9 P1s open) |
| VUMA wasm32 backend | 100% test pass rate |
| VUMA Lean proof | Real (280 theorems, 0 sorries) |
| WOMB substrate (kernel/crypto/libc) | Real (117 kLOC) |
| WOMB UI engine | **Does not exist** (~99 person-weeks to v1) |
| VEEE | **Does not exist** (month 26 target) |
| COSS | Design document only |

### Architectural alignment with AlkALive
| AlkALive need | VUMA stack provides? |
|---------------|:---:|
| WASM compilation | ✅ (wasm32 backend, 100% pass) |
| WebGL2 canvas driving | ❌ (Vulkan/Metal only) |
| Text shaping (HarfRust) | ❌ (WOMB shaper v1 is cmap+hmtx only) |
| IME (hidden input, ADR 023) | ❌ (OS-native bridges only, no browser path) |
| GPU rendering pipeline | ❌ (compute-shader tessellation, no WebGL2) |
| Module isolation | ✅ (PMT arena + capability tokens) |
| Memory safety | ✅ (Lean-verified PMT) |
| Zero DOM UI | ✅ (by design, but browser IME/a11y gap) |

### Bottom line
The VUMA stack is architecturally thoughtful and the substrate is real, but **three load-bearing pieces are missing for AlkALive**: (1) WebGL2/WebGPU dispatch, (2) browser IME/a11y without DOM, (3) the UI engine itself. Combined with open P0/P1 bugs and the 18-26 month timeline, **hosting AlkALive on VUMA today is infeasible without a multi-quarter commitment**.

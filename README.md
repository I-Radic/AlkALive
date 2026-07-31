# AlkALive

AlkALive is an exploratory project investigating a **custom, module- and object-oriented language that compiles to WebAssembly and renders UI directly through WebGPU/WebGL**, bypassing the HTML/CSS/DOM/JavaScript stack entirely.

## Repository contents

| Path | Description |
| --- | --- |
| `docs/PROBLEM_CATALOG.md` | **Problem Catalog** — a literature-grounded investigation of the fundamental limitations of the HTML/CSS/JavaScript web frontend stack, synthesised from 50 peer-reviewed sources (IEEE/ACM/USENIX/NDSS/Springer/arXiv). |
| `docs/ROUGH_DRAFT.md` | **Rough Draft** — an architectural response to the catalog: a four-part (Problem / Goal / Solution / Integration) design rationale per cluster, produced via a four-wave sub-agent validation campaign and cross-checked for internal consistency and evidence traceability. |
| `docs/FINE_DRAFT.md` | **Fine Draft** — the definitive system design specification synthesizing the Rough Draft with all 22 ADRs. Twelve sections covering system overview, module/object model, rendering, layout, text, styling, input, concurrency, DOM interaction, accessibility, lifecycle, and integration. Ready for implementation. |
| `docs/SPECIFICATION.md` | **Detailed Software Specification** — the most concrete, implementation-ready document: 14 sections with precise interface definitions (structs, enums, function signatures, error types), performance budgets, testing harness, and a glossary. The definitive technical blueprint for senior engineers. |
| `docs/VERIFICATION_LOG.md` | **Verification Log** — evidence trail for the catalog's 50 references, documenting the multi-agent re-verification campaign that corrected 28 citations. |
| `docs/adr/ADR.md` | **Architectural Decision Record** — 22 formal ADRs (ADR 001–022) consolidated in a single file, each recording a design choice with Context, Decision, Status, Consequences, and Confidence. |
| `docs/adr/Decision_Alternatives_*.md` | **Decision Alternatives** — four resolved decision points (text rendering, concurrency/scheduling, accessibility bridge, adoption/interop), superseded by ADRs 019–022 and retained for historical context. |
| `docs/adr/Spec_Tradeoff_Note_IME.md` | **IME Trade-off Note** — open dependency: IME composition-event acquisition conflicts with ADR 020's metadata-only DOM rule. Three candidate approaches with a recommended resolution. |
| `IMPLEMENTATION_PLAN.md` | **Implementation Plan** — 12-wave decomposition with granular tasks, DoD criteria per wave, ADR traceability matrix, and risk register. |
| `TODO_RESOLUTION_PLAN.md` | **TODO Resolution Plan** — wave-by-wave plan that eliminated all 47 `todo!()` calls (trait defaults → required methods + concrete stub/mock implementations), yielding a fully compilable, testable codebase. |
| `GAP_ANALYSIS.md` | **Gap Analysis** — audit of the 13-crate codebase against the spec/ADRs (41 gaps: 5 Critical, 14 High, 18 Medium, 4 Low), each with spec/ADR references and remediation actions. |
| `GAP_IMPLEMENTATION_PLAN.md` | **Gap Implementation Plan** — implementation waves (A–…) derived from the Gap Analysis, resolving all Critical/High/Medium gaps with Definition-of-Done criteria per wave. |
| `Cargo.toml` | **Rust workspace** — 13 crates (`alkalive-{core,runtime,render,layout,text,style,input,dom,a11y,ipc,perf,error,test}`), edition 2021, wasm-release profile. |
| `rust-toolchain.toml` | **Toolchain pin** — Rust 1.97.1 + wasm32-unknown-unknown + clippy + rustfmt. |
| `deny.toml` | **cargo-deny** — enforces ADR 018 (deny-by-default; HarfRust transitive deps allowlisted). |
| `crates/` | **Crate source** — real implementations (no `todo!()` remaining). 49,544 lines and 471 tests across 13 crates + 2 vendored crates. HarfRust text stack fully integrated (ADR 022). |
| `vendor/harfrust/` | **HarfRust** — vendored git subtree from `harfbuzz/harfrust` (MIT). Real text shaping, font loading, glyph metrics. |
| `vendor/rasterizer/` | **Rasterizer** — vendored scanline rasterizer (MIT, no external deps). Converts TrueType outlines to grayscale bitmaps for the glyph atlas. |

## The catalog

`docs/PROBLEM_CATALOG.md` is the foundational design rationale for the project. It is organised by architectural layer (Rendering Pipeline, Layout & Styling, Document Model, Language Design, Interactivity, Accessibility & Platform Integration, Performance, Tooling & DX, Bundle/Ecosystem, and Existing Inspirations), with 45 named, cross-referenced problem entries, a methodology section, a synthesis, an explicit list of literature gaps, and a full IEEE-style reference list.

The two problems the catalog identifies as **decisive** for the viability of the alternative vision are:

1. **Text rendering (P3.5)** — text shaping, measurement, selection, editing, and IME are tightly locked to the DOM; a WASM+GPU stack must ship a first-class text stack.
2. **Accessibility (P6.1)** — ARIA/focus/screen-reader contracts are coupled to the DOM; a WASM+GPU stack must emit a virtual accessibility tree as a first-class concern.

The catalog also identifies **ecosystem-adoption inertia** (P9.5) as the central *strategic* (non-technical) risk.

## The rough draft

`docs/ROUGH_DRAFT.md` translates the catalog's problems into a concrete architectural vision. For each of nine clusters (Rendering Pipeline, Layout & Styling, Document Model, Language Design, Interaction, Accessibility, Performance, Tooling, Bundle/Ecosystem) it specifies a four-part tuple:

- **Problem** — the limitation, grounded in the catalog's evidence (citing `P x.y` entries and `[n]` references).
- **Goal** — what the ideal WASM + WebGPU alternative should achieve.
- **Solution** — a high-level conceptual mechanism within the new architecture.
- **Integration** — how the solution interacts with the other clusters' solutions.

The draft concludes with an **Integration Overview** that synthesises the nine cluster solutions into a single coherent architecture organised around one principle: *the render-object tree is the single source of truth, owned by the WASM module, with multiple emission targets.* It explicitly resolves cross-area conflicts (single-source tree ownership, unified focus/accessibility structure, scheduler ownership, interop-interface ownership) and flags open risks (GPU-backend portability, WASM component-model maturity, COOP/COEP constraints, and catalog-acknowledged literature gaps).

## License

Apache License 2.0. See `LICENSE`.

# AlkALive

AlkALive is an exploratory project investigating a **custom, module- and object-oriented language that compiles to WebAssembly and renders UI directly through WebGPU/WebGL**, bypassing the HTML/CSS/DOM/JavaScript stack entirely.

## Repository contents

| Path | Description |
| --- | --- |
| `docs/PROBLEM_CATALOG.md` | **Problem Catalog** — a literature-grounded investigation of the fundamental limitations of the HTML/CSS/JavaScript web frontend stack, synthesised from 50 peer-reviewed sources (IEEE/ACM/USENIX/NDSS/Springer/arXiv). |
| `docs/ROUGH_DRAFT.md` | **Rough Draft** — an architectural response to the catalog: a four-part (Problem / Goal / Solution / Integration) design rationale per cluster, produced via a four-wave sub-agent validation campaign and cross-checked for internal consistency and evidence traceability. |
| `docs/VERIFICATION_LOG.md` | **Verification Log** — evidence trail for the catalog's 50 references, documenting the multi-agent re-verification campaign that corrected 28 citations. |

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

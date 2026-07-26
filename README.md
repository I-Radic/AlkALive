# WebF

WebF is an exploratory project investigating a **custom, module- and object-oriented language that compiles to WebAssembly and renders UI directly through WebGPU/WebGL**, bypassing the HTML/CSS/DOM/JavaScript stack entirely.

## Repository contents

| Path | Description |
| --- | --- |
| `docs/PROBLEM_CATALOG.md` | **Problem Catalog** — a literature-grounded investigation of the fundamental limitations of the HTML/CSS/JavaScript web frontend stack, synthesised from 50 peer-reviewed sources (IEEE/ACM/USENIX/NDSS/Springer/arXiv). |

## The catalog

`docs/PROBLEM_CATALOG.md` is the foundational design rationale for the project. It is organised by architectural layer (Rendering Pipeline, Layout & Styling, Document Model, Language Design, Interactivity, Accessibility & Platform Integration, Performance, Tooling & DX, Bundle/Ecosystem, and Existing Inspirations), with 45 named, cross-referenced problem entries, a methodology section, a synthesis, an explicit list of literature gaps, and a full IEEE-style reference list.

The two problems the catalog identifies as **decisive** for the viability of the alternative vision are:

1. **Text rendering (P3.5)** — text shaping, measurement, selection, editing, and IME are tightly locked to the DOM; a WASM+GPU stack must ship a first-class text stack.
2. **Accessibility (P6.1)** — ARIA/focus/screen-reader contracts are coupled to the DOM; a WASM+GPU stack must emit a virtual accessibility tree as a first-class concern.

The catalog also identifies **ecosystem-adoption inertia** (P9.5) as the central *strategic* (non-technical) risk.

## License

Apache License 2.0. See `LICENSE`.

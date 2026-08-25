# AlkALive

AlkALive is a custom, module- and object-oriented language that compiles to WebAssembly and renders UI directly through WebGL2, bypassing the HTML/CSS/DOM/JavaScript rendering stack. The compiler features a statically-typed type system with monotonicity qualifiers, a WASM code generation backend, and a render-graph IR that drives GPU rendering.

## Repository structure

| Path | Description |
| --- | --- |
| `crates/alkalive-compiler/` | Compiler: lexer, parser, AST, type checker, WASM codegen, lints, schedule, incremental analysis, e-graph optimization, seminaïve evaluation |
| `crates/alkalive-backend-wgpu/` | WebGL2 GPU backend: shaders (GLSL + WGSL targets), glyph atlas, vertex buffers, rect/text rendering, render-graph execution |
| `crates/alkalive-runtime-wasm/` | WASM runtime: embeds `.alk` source, compiles at startup, owns frame loop + input forwarding + resize handling |
| `crates/alkalive-render/` | Render-graph IR: `RenderGraph`, `RenderPass`, `Attachment`, `DrawCall`, `DrawCallKind` types + `build_render_graph()` |
| `crates/alkalive-scene-data/` | Shared `TextSceneData` type (breaks render↔backend dependency cycle) |
| `crates/alkalive-text/` | HarfRust text shaping + glyph atlas rasterization (vendored HarfRust fork) |
| `crates/alkalive-core/` | Core types: `ModuleId`, `Type`, `Visibility`, `Interface`, WASM validation types |
| `crates/alkalive-{layout,style,input,dom,a11y,ipc,perf,error,test}/` | Supporting infrastructure crates |
| `vendor/harfrust/` | Vendored HarfRust text shaping engine (MIT) |
| `vendor/rasterizer/` | Vendored glyph rasterizer (MIT) |
| `docs/` | Technical specification, ADRs, wave reports, fine drafts, specifications |
| `deploy/` | Pre-built WASM binary + JS glue + HTML shell |
| `examples/hello.alk` | Canonical Hello World `.alk` source file |

## What is implemented

### Language & compiler (`alkalive-compiler`)

- **Lexer**: 50+ token kinds including keywords (`module`, `scene`, `fn`, `let`, `class`, `field`, `if`, `else`, `while`, `return`, `import`, `pub`, `priv`, `monotone`, `antitone`), operators (`+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`), and punctuation
- **Parser**: recursive-descent with Pratt parsing for binary operator precedence; supports modules, scenes, functions, variables, classes, fields, methods, inheritance, imports, control flow, and expressions
- **Type system**: `FnSigTable` with 3-pass `check_module` (collect signatures → collect lets → check bodies); type inference for function calls; monotonicity qualifier checking (monotone/antitone/unrestricted subtyping lattice); `ClassTable` for OO method dispatch; return-type checking
- **WASM code generation**: real WASM binary emission via `wasm-encoder`; type section, function section, import section (10 host imports), memory section, export section, code section, data section; `wasmparser`-validated binaries
- **Language features**: functions, variables, binary operators, control flow (`if`/`else`/`while`), function calls, classes with fields/methods/inheritance, vtable-based virtual dispatch via `call_indirect`, `__alk_alloc` host import for heap allocation, string literals in WASM data sections, collection method dispatch via host imports
- **Compiler enhancements**: ADR-024 schedule separation, ADR-025 incremental computation (dependency graph), ADR-026 e-graph optimization, ADR-027 monotonicity types (Phase 1 lint + Phase 2 type qualifier), ADR-028 PMT verification (deferred)

### Rendering (`alkalive-backend-wgpu` + `alkalive-render`)

- **Render-graph IR** (ADR-001): `RenderGraph` with `RenderPass`/`Attachment`/`DrawCall`/`DrawCallKind`; `build_render_graph()` constructs the graph from scene data; `WgpuRenderer::render_graph()` executes the graph
- **wgpu/WGSL renderer (primary, ADR-001/ADR-006)**: WebGPU via the `wgpu` crate; 4 WGSL programs compiled at init; dynamic-offset uniform rings; explicit bind-group layouts; falls back automatically (with a logged reason) when WebGPU is unavailable
- **WebGL2 fallback**: GLSL ES 3.00 shaders for text quad rendering (Y-axis rotation, glyph atlas sampling) and rectangle rendering (alpha-blended); rect shader replaces scissor+clear hack
- **Text stack** (ADR-022): HarfRust shaping + vendored rasterizer; 512×512 R8 glyph atlas; cached font registry (parsed once, not per-keystroke)
- **GPU features**: high-DPI rendering via `devicePixelRatio`; frame-rate-independent animation via `performance.now()`; alpha blending for rect transparency
- **COOP/COEP** (ADR-003): isolation enabled via HTTP response headers served by `deploy/serve.mjs` (`<meta http-equiv>` is ignored by browsers for this purpose); runtime verifies `crossOriginIsolated` + constructible `SharedArrayBuffer` at startup; single-threaded fallback when SAB unavailable

### Runtime (`alkalive-runtime-wasm`)

- **WASM cdylib**: owns the entire rendering pipeline
- **Frame loop**: `requestAnimationFrame` driven from inside WASM (ADR-013: no WASM↔DOM boundary in hot path)
- **Input**: hidden DOM `<input>` (ADR-023) with WASM-attached `keydown`/`input` listeners forwarding to text buffer
- **Scene embedding**: `.alk` source embedded via `include_str!`, compiled at startup by the real AlkALive compiler
- **Signal store**: ADR-025 incremental computation support

### Demo

The Hello World demo is a **genuine end-to-end AlkALive application**:
- `examples/hello.alk` source is embedded in the WASM binary
- Compiled at startup by the real AlkALive compiler
- Rendered via WebGL2 by the real AlkALive runtime through the render graph
- Zero application JavaScript, zero CSS for UI, zero DOM UI elements

## Build & run

### Prerequisites

- Rust 1.97.1 (`rust-toolchain.toml` pins the version)
- `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)
- `wasm-bindgen-cli` 0.2.127 (`cargo install wasm-bindgen-cli --version 0.2.127`)

### Build

```bash
# Build the compiler and all workspace crates
cargo build --workspace

# Run the test suite
cargo test --workspace

# Build the WASM runtime binary
cargo build -p alkalive-runtime-wasm --target wasm32-unknown-unknown --release

# Generate the JS glue + WASM binary in deploy/pkg/
wasm-bindgen --target web --out-dir deploy/pkg --out-name alkalive_runtime_wasm \
  target/wasm32-unknown-unknown/release/alkalive_runtime_wasm.wasm
```

### Run the demo

```bash
# Serve deploy/ with the COOP/COEP isolation headers required by ADR-003/021
node deploy/serve.mjs 8080
# then open http://127.0.0.1:8080 in a WebGPU- or WebGL2-capable browser
```

### Compile `.alk` source

```bash
# Compile a .alk file to SceneIR JSON
cargo run --bin alkalive-compiler -- compile examples/hello.alk -o /tmp/hello.scene

# Run the linter
cargo run --bin alkalive-compiler -- lint examples/hello.alk
```

## Architecture

```
hello.alk → [alkalive-compiler] → SceneIR+Schedule+DepGraph → [alkalive-runtime-wasm] → render graph → wgpu/WGSL (primary) or WebGL2/GLSL (fallback) → <canvas>
```

1. **`.alk` source** — declares the scene using the AlkALive language (modules, scenes, text nodes, input fields)
2. **Compiler** — lexes, parses, type-checks, and lowers to `SceneIR`; also generates valid WASM binaries via `wasm-encoder`
3. **Runtime** — embeds the scene at build time, compiles it at startup, initializes WebGL2, owns the frame loop
4. **Render graph** — `build_render_graph()` produces a `RenderGraph` that `render_graph()` executes
5. **GPU rendering** — WebGL2 with GLSL ES 3.00 shaders; text quads with Y-axis rotation and glyph atlas sampling; rect rendering with alpha blending
6. **Text stack** — HarfRust shapes text; vendored rasterizer rasterizes glyphs to a 512×512 R8 GPU texture
7. **Input** — hidden `<input>` (ADR 023) forwards keyboard/IME events to the WASM runtime

## Documentation

| Document | Description |
| --- | --- |
| `docs/technical-specification.md` | Technical specification grounded in the actual codebase |
| `docs/adr/ADR.md` | 22 ADRs (ADR 001–022) covering render graph, GPU device, layout, language design, type verification, input, accessibility, threading, text stack |
| `docs/adr/ADR_023`–`ADR_028` | Additional ADRs for IME composition, algorithm/schedule separation, incremental computation, e-graph optimization, monotonicity types, PMT verification |
| `docs/alkalive-wave-00-audit.md` | Repository audit identifying critical bugs and architectural gaps |
| `docs/alkalive-wave-00-post-implementation-audit.md` | Forensic post-implementation verification of all 8 gap implementations |
| `docs/alkalive-specification-language.md` | Detailed specification for language/compiler gaps |
| `docs/alkalive-specification-rendering.md` | Detailed specification for rendering/runtime gaps |
| `docs/alkalive-fine-draft-language.md` | Fine draft for OO model, modules, type inference, strings, collections |
| `docs/alkalive-fine-draft-rendering.md` | Fine draft for render graph, WGSL, GPU device/SAB |
| `docs/alkalive-remediation-report.md` | Post-implementation audit findings and remediation |
| `docs/PROBLEM_CATALOG.md` | Literature-grounded investigation of HTML/CSS/JS limitations |
| `docs/FINE_DRAFT.md` | System design specification |
| `docs/SPECIFICATION.md` | Implementation-ready technical blueprint |
| `docs/adopted-vuma-ideas/` | Five VUMA-inspired compiler enhancement designs |

## License

Apache License 2.0. See `LICENSE`.

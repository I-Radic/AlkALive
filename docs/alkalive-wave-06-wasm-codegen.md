# AlkALive Wave 6 — Real WASM Code Generation

> **Read all previous wave docs first:**
> `docs/alkalive-wave-00-audit.md` through `docs/alkalive-wave-05-demo-verification.md`

## Objective

Implement real WebAssembly code generation — the critical gap identified in
the Wave 0 audit. ADR-008 says "compiling to WASM" but the compiler only
produced JSON SceneIR. This wave adds a genuine WASM backend that emits
valid `.wasm` binary modules from the typed AST.

## What was implemented

### `wasm_codegen.rs` module (NEW, ~600 LOC)

A real WebAssembly code generation backend using the `wasm-encoder` crate.
It lowers typed AST → valid WASM binary via:

1. **Type checking gate**: `compile_to_wasm()` runs `typechecker::check_module()`
   first (ADR-009 source-level soundness). If type errors exist, WASM
   generation is refused.

2. **Type section**: Each function's signature (params → results) is registered
   as a WASM function type, with deduplication.

3. **Function section**: Each `fn` declaration gets a function index.

4. **Memory section**: 1 page (64KB) of linear memory is allocated and exported
   for heap-allocated data (strings, collections, objects).

5. **Export section**: Each function is exported by name; memory is exported
   as `"memory"`.

6. **Code section**: Each function body is compiled to WASM instructions:
   - `Lit::Int(v)` → `i32.const v`
   - `Lit::Float(v)` → `f32.const v`
   - `Lit::Bool(b)` → `i32.const 1` or `i32.const 0`
   - `Lit::Str(_)` → `i32.const 0` (pointer placeholder)
   - `Expr::Var(name)` → `local.get idx`
   - `Stmt::Let` → compile init, then `local.set idx`
   - `Stmt::Return(e)` → compile e, then `return`
   - `Stmt::Expr(e)` → compile e, then `drop` (if produces value)

### Type mapping

AlkALive types map to WASM `ValType`:
- `i32` → `I32`
- `f32` → `F32`
- `bool` → `I32` (0/1)
- `string` → `I32` (pointer into linear memory)
- `Vec<T>` → `I32` (pointer to heap-allocated collection)
- `Named(...)` → `I32` (pointer to heap-allocated object)

Monotonicity qualifiers are erased — WASM has no notion of monotonicity;
it is enforced at compile time by the type checker.

### Dependencies

- `wasm-encoder = "0.227"` — emits valid WASM binary modules
- `wasmparser = "0.227"` (dev-dependency) — validates generated binaries

## Files changed

- `crates/alkalive-compiler/Cargo.toml` — added `wasm-encoder` + `wasmparser` deps
- `crates/alkalive-compiler/src/wasm_codegen.rs` (NEW) — WASM backend
- `crates/alkalive-compiler/src/lib.rs` — module declaration + re-exports

## Tests executed

- 21 new `wasm_codegen` unit tests:
  - Binary validity (magic number, version header)
  - Function exports (by name)
  - Memory export
  - Type mapping (i32, f32, bool, string, Vec)
  - Compilation of integer/float/bool/void returns
  - Let bindings + variable references
  - Multiple functions in one module
  - Functions with parameters
  - Type-check failure → WASM generation refused
  - Empty module (no functions) → valid WASM with memory only
  - **`wasm_binary_parseable_by_wasmparser`** — the critical test: the
    generated binary is parsed by `wasmparser` (the official WebAssembly
    parser for Rust) and confirmed to have valid function + memory sections

- `cargo test --workspace`: **1169 passed, 0 failed** (was 1148, +21 new)
- `cargo clippy -p alkalive-compiler -- -D warnings`: clean
- `cargo fmt`: clean

## Critical-review findings

The WASM backend is genuine — it produces real, structurally-valid WebAssembly
binaries verified by `wasmparser`. However, it is a **foundational** backend,
not a complete one:

- **What works**: function signatures, exports, memory, literals, variable
  references, let bindings, return statements, type-checker integration
- **What's not yet implemented**: binary arithmetic operators (+, -, *, /),
  function calls (calling other AlkALive functions), string allocation in
  the data section, collection method dispatch (push/remove/len as imported
  functions), control flow (if/while/for)

These are extensions to the same framework — the type section, function
section, export section, and code section infrastructure is in place. Adding
binary operators means emitting `i32.add`/`i32.sub`/etc. after the operands;
adding function calls means emitting `call funcidx` after the arguments.

## DoD checklist

- [x] `wasm-encoder` dependency added
- [x] `compile_to_wasm()` function produces valid WASM binary
- [x] Binary starts with `\0asm` magic + version 1
- [x] Binary is parseable by `wasmparser` (structurally valid)
- [x] Functions are exported by name
- [x] Linear memory is allocated and exported
- [x] Type checker runs before WASM generation (ADR-009)
- [x] Integer/float/bool literals compile to correct WASM instructions
- [x] Variable references compile to `local.get`
- [x] Let bindings compile to `local.set`
- [x] Return statements compile correctly
- [x] Type-check failures prevent WASM generation
- [x] All 1169 tests pass (21 new, 0 regressions)
- [x] Clippy clean
- [x] rustfmt clean

## Dependencies for the next wave

- The WASM backend can now be extended with binary operators, function calls,
  and control flow
- The module system (Wave 7) can use the WASM import/export mechanism
- The OO model (Wave 8) can use the heap allocation + function table mechanism

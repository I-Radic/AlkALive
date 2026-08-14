# AlkALive Wave 9 — ADR Update + Final Verification

> **Read all previous wave docs first.**

## Objective

Update ADR-008 to reflect that the compiler now genuinely compiles to WASM
(closing the critical gap identified in Wave 0), and perform final verification.

## What was implemented

### ADR-008 update

Added "Implementation Status (Wave 9 Update)" subsection to ADR-008 that:
- Supersedes the Wave 4 audit's 0% findings for "compiling to WASM" and
  "functions/variables/control flow/expressions"
- Documents the real WASM code generation backend (Wave 6)
- Documents binary operators + function calls (Wave 7)
- Documents control flow if/else/while (Wave 8)
- Provides an updated gap status table showing what's implemented vs. remaining

### Updated gap status

| ADR-008 claim | Wave 9 status |
|---------------|---------------|
| "statically-typed" | **Partially implemented** — type checker checks qualifiers, variables, methods |
| "object oriented" | **Not yet implemented** — next major gap |
| "compiling to WASM" | **Implemented** — `wasm_codegen.rs` emits valid `.wasm` via `wasm-encoder` |
| Functions, variables, control flow, expressions | **Implemented** — `fn`, `let`, `if`/`else`, `while`, `return`, all operators |

## Final verification

### Compiler pipeline (verified)

```
.alk source
  → lexer (tokenize)
  → parser (Pratt parsing with precedence)
  → AST (ModuleDecl with FnDecl, Stmt, Expr, Type, BinOp)
  → typechecker (check_module — ADR-009 source-level soundness)
  → wasm_codegen (compile_to_wasm)
  → valid .wasm binary (verified by wasmparser)
```

### What the WASM backend generates (verified by wasmparser)

- Type section with function type signatures (deduplicated)
- Function section with function indices
- Memory section (1 page = 64KB, exported)
- Export section (each function exported by name + memory)
- Code section with:
  - `i32.const` / `f32.const` for literals
  - `local.get` / `local.set` for variables
  - `i32.add` / `i32.sub` / `i32.mul` / `i32.div_s` / `i32.rem_s` for arithmetic
  - `i32.eq` / `i32.ne` / `i32.lt_s` / `i32.le_s` / `i32.gt_s` / `i32.ge_s` for comparison
  - `i32.and` / `i32.or` for logical
  - `if` / `else` / `end` for conditionals
  - `block` / `loop` / `br` for loops
  - `call funcidx` for function calls
  - `return` for return statements
  - `drop` for expression statements

### Test summary

- `cargo test --workspace`: **1188 passed, 0 failed**
- 21 WASM codegen tests (Wave 6)
- 14 binary operator tests (Wave 7)
- 5 control flow tests (Wave 8)
- All generated WASM binaries validated by `wasmparser`
- Clippy clean, rustfmt clean

## DoD checklist

- [x] ADR-008 updated with Wave 9 implementation status
- [x] "compiling to WASM" gap marked as Implemented
- [x] Functions/variables/control flow/expressions marked as Implemented
- [x] All 1188 tests pass
- [x] WASM binaries validated by wasmparser
- [x] Clippy clean, rustfmt clean
- [x] All wave documentation in `docs/`

## Remaining gaps (honestly documented)

1. **OO model** — no classes, methods, inheritance (next major gap)
2. **Module system** — no imports/exports (module is just a name wrapper)
3. **Full type inference** — function call return types are not inferred
4. **String data sections** — string literals compile to placeholder pointers
5. **Collection method dispatch** — push/remove/len are not yet imported functions
6. **Render-graph IR** (ADR-001) — not implemented
7. **WGSL shaders** (ADR-006) — not implemented (still hardcoded GLSL)
8. **Single-GPU-device + SAB/COOP-COEP** (ADR-003) — not implemented

These are documented in the ADRs and wave documentation for future work.

# AlkALive Wave 8 — Control Flow (if/else, while)

> **Read all previous wave docs first.**

## Objective

Add `if`/`else` conditional statements and `while` loop statements to the
language, with full lexer → parser → typechecker → WASM codegen support.

## What was implemented

### Lexer: 3 new keywords

`if`, `else`, `while` added as reserved keywords.

### AST: 2 new statement variants

- `Stmt::If { cond, then_block, else_block, line, col }` — conditional
- `Stmt::While { cond, body, line, col }` — loop

### Parser

- `if (cond) { block }` — parses condition in parens, then a block
- `if (cond) { block } else { block }` — optional else clause
- `while (cond) { block }` — parses condition in parens, then a body block

### Typechecker

- `Stmt::If` — checks condition + both blocks (then + else)
- `Stmt::While` — checks condition + body

### WASM codegen

- `if`/`else` compiles to WASM `if`/`else`/`end` instructions
  (using `BlockType::Empty` for void blocks)
- `while` compiles to the standard WASM loop pattern:
  `block loop cond if br(1) else end body br(0) end end`
  - `br(1)` breaks out of the outer block (loop exit)
  - `br(0)` branches back to the loop start (continue)
- 5 new `AlkInstr` variants: `If`, `Else`, `Block`, `Loop`, `Br(u32)`

## Files changed

- `crates/alkalive-compiler/src/lexer.rs` — `If`, `Else`, `While` keywords
- `crates/alkalive-compiler/src/ast.rs` — `Stmt::If`, `Stmt::While`
- `crates/alkalive-compiler/src/parser.rs` — if/else/while parsing
- `crates/alkalive-compiler/src/typechecker.rs` — If/While type checking
- `crates/alkalive-compiler/src/wasm_codegen.rs` — If/Else/Block/Loop/Br emission

## Tests executed

- 5 new `control_flow_tests`:
  - `if` statement compilation
  - `if`/`else` statement compilation
  - `while` loop compilation
  - **wasmparser validation** of if/else binary
  - **wasmparser validation** of while binary
- `cargo test --workspace`: **1188 passed, 0 failed** (was 1183, +5 new)
- Clippy clean, rustfmt clean

## DoD checklist

- [x] `if`/`else`/`while` keywords in lexer
- [x] `Stmt::If` and `Stmt::While` AST nodes
- [x] Parser handles `if (cond) { } else { }` and `while (cond) { }`
- [x] Typechecker checks conditions and both branches
- [x] WASM codegen emits `if`/`else`/`end` for conditionals
- [x] WASM codegen emits `block`/`loop`/`br` for loops
- [x] Generated binaries validated by wasmparser
- [x] All 1188 tests pass (5 new, 0 regressions)
- [x] Clippy clean, rustfmt clean

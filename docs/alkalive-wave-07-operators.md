# AlkALive Wave 7 — Binary Operators + Function Calls

> **Read all previous wave docs first:**
> `docs/alkalive-wave-00-audit.md` through `docs/alkalive-wave-06-wasm-codegen.md`

## Objective

Add binary operators (arithmetic, comparison, logical) and function calls to
the language, with full lexer → parser → typechecker → WASM codegen support.

## What was implemented

### Lexer (12 new token kinds)

Added operator tokens: `Plus`, `Minus`, `Star`, `Slash`, `Percent`, `EqEq`,
`BangEq`, `LtEq`, `GtEq`, `AndAnd`, `OrOr`. Multi-char operators (`==`, `!=`,
`<=`, `>=`, `&&`, `||`) are handled with lookahead. The `-` handling now
distinguishes `->` (arrow), `-N` (negative number), and `-` (binary minus).

### AST (BinOp enum + Expr::Binary + Expr::Call)

- `BinOp` enum with 13 variants: Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Le, Gt,
  Ge, And, Or
- `BinOp::precedence()` — Pratt parsing precedence (1=||, 2=&&, 3=comparison,
  4=+-, 5=*/%)
- `BinOp::is_comparison()` / `is_logical()` — classification helpers
- `Expr::Binary { lhs, op, rhs, line, col }` — binary expression node
- `Expr::Call { callee, args, line, col }` — function call node

### Parser (Pratt parsing)

- `parse_expr()` → calls `parse_binary_expr(1)` with min precedence 1
- `parse_binary_expr(min_prec)` — Pratt parser: parse primary, then while
  next token is a binary op with precedence ≥ min_prec, parse RHS at
  `prec + 1` (left-associative)
- `parse_primary()` — the former `parse_expr`, now also handles:
  - Parenthesized expressions: `( expr )`
  - Function calls: `ident(args)` — when an Ident is followed by `(`

### Typechecker

- `Expr::Binary` — checks both operands, returns result type based on operator:
  - Comparison operators → `bool`
  - Logical operators → `bool`
  - Arithmetic operators → type of LHS (or RHS)
- `Expr::Call` — checks arguments, returns `None` (return type inference
  requires a function signature table, which is a future enhancement)

### WASM codegen

- `AlkInstr::BinaryOp(BinOp)` — new instruction variant
- `AlkInstr::Call(String)` — new instruction variant
- Binary operators emit WASM arithmetic instructions:
  - `+` → `i32.add`, `-` → `i32.sub`, `*` → `i32.mul`
  - `/` → `i32.div_s`, `%` → `i32.rem_s`
  - `==` → `i32.eq`, `!=` → `i32.ne`
  - `<` → `i32.lt_s`, `<=` → `i32.le_s`, `>` → `i32.gt_s`, `>=` → `i32.ge_s`
  - `&&` → `i32.and`, `||` → `i32.or`
- Function calls emit `call funcidx` — the function name is resolved to its
  index in the export table during code emission

## Files changed

- `crates/alkalive-compiler/src/lexer.rs` — 12 new operator tokens + multi-char handling
- `crates/alkalive-compiler/src/ast.rs` — BinOp enum + Expr::Binary + Expr::Call
- `crates/alkalive-compiler/src/parser.rs` — Pratt parser + function call parsing
- `crates/alkalive-compiler/src/typechecker.rs` — Binary + Call type checking
- `crates/alkalive-compiler/src/wasm_codegen.rs` — BinaryOp + Call WASM emission
- `crates/alkalive-compiler/src/lib.rs` — BinOp re-export

## Tests executed

- 14 new `binary_op_tests`:
  - Addition, subtraction, multiplication, division
  - Chained arithmetic with precedence (`1 + 2 * 3`)
  - Parenthesized expressions (`(1 + 2) * 3`)
  - Comparison operators (`1 < 2`)
  - Logical AND/OR (`true && false`, `true || false`)
  - Variable arithmetic (`x + y`)
  - Function calls (`helper()`, `double(21)`)
  - **wasmparser validation** of binaries with binary operators and function calls

- `cargo test --workspace`: **1183 passed, 0 failed** (was 1169, +14 new)
- `cargo clippy -p alkalive-compiler -- -D warnings`: clean
- `cargo fmt`: clean

## Critical-review findings

The binary operators and function calls are **genuine** — they produce real
WASM instructions verified by `wasmparser`. The Pratt parser correctly handles
operator precedence (multiplication binds tighter than addition). Function
calls resolve to the correct WASM function index.

**Limitation**: All arithmetic currently uses `i32` WASM instructions regardless
of the operand types. A full implementation would use `f32.add` for `f32`
operands. This requires the type checker to propagate type information to the
WASM codegen, which is a future enhancement.

## DoD checklist

- [x] 12 binary operator tokens added to lexer
- [x] Multi-char operators (`==`, `!=`, `<=`, `>=`, `&&`, `||`) handled
- [x] `-` disambiguates arrow/negative-number/binary-minus
- [x] BinOp enum with precedence + classification
- [x] Expr::Binary and Expr::Call AST nodes
- [x] Pratt parser with correct precedence
- [x] Parenthesized expressions
- [x] Function call parsing
- [x] Typechecker handles Binary and Call
- [x] WASM codegen emits correct arithmetic instructions
- [x] WASM codegen emits `call` for function calls
- [x] Generated binaries validated by wasmparser
- [x] All 1183 tests pass (14 new, 0 regressions)
- [x] Clippy clean
- [x] rustfmt clean

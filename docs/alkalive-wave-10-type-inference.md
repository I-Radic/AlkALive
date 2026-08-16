# AlkALive Wave 10 — Full Type Inference (Gap 3)

> **Read all previous wave docs and the specification first.**

## Objective

Implement full function-call return type inference by building a module-wide
`FnSigTable` and using it to type-check `Expr::Call`, `Expr::MethodCall`, and
`Expr::PathCall`.

## What was implemented

### FnSigTable and FnSig types

Added `FnSig` (function signature: name, params, return_type, param_names,
receiver_class, imported_from) and `FnSigTable` (HashMap-based lookup table)
to `typechecker.rs`. The table supports:
- `lookup(name)` — free function lookup
- `lookup_method(class, method)` — qualified `Class::method` lookup

### Three-pass check_module

Restructured `check_module` into three passes:
1. **Pass 1**: `collect_signatures(module, &mut sigs)` — collects all `FnDecl` signatures before any body is checked (supports mutual/self recursion)
2. **Pass 2**: Collects module-level `let` bindings (unchanged logic, now threads `&sigs`)
3. **Pass 3**: Checks each function body with access to `&sigs`

### Expr::Call type checking

`check_expr` for `Expr::Call` now:
1. Checks all argument expressions
2. Looks up `callee` in `FnSigTable`
3. Verifies argument count (arity check → `LANG-307-E2`)
4. Verifies each argument type is a subtype of the parameter type (`LANG-307-E3`)
5. Returns the function's declared return type (the actual inference)
6. Reports `LANG-307-E1` if the function is unknown

### Expr::MethodCall type checking

`check_expr` for `Expr::MethodCall` now dispatches on the receiver's inferred type:
- `Vec<T>` → checks monotonicity (existing `check_method_op`) + returns `collection_method_return_type`
- `Named(class)` → emits error (Gap 1 not yet implemented)
- Other concrete type → emits `LANG-308-E2`
- `None` (receiver errored) → returns `None` silently

### Collection method return types

Added `collection_method_return_type` function:
- Grow/shrink ops (`push`, `extend`, `insert`, `append`, `remove`, `truncate`, `clear`, `swap_remove`, `drain`) → `None` (unit)
- `len` → `i32`
- `is_empty` → `bool`
- `get`, `first`, `last`, `contains` → `i32`
- Unknown → `LANG-308-E3` error

### Expr::PathCall type checking

`check_expr` for `Expr::PathCall` now:
- `Vec::new` / `Vec::with_capacity` → `None` (element type not inferable)
- Other paths → looks up `module::member` in `FnSigTable`, returns declared return type or emits `LANG-309-E3`

## Files changed

- `crates/alkalive-compiler/src/typechecker.rs` — FnSigTable, three-pass algorithm, full call/method/path-call checking
- `crates/alkalive-compiler/src/lib.rs` — re-export FnSig, FnSigTable

## Tests executed

- 11 new `type_inference_tests` (LANG-3T-01 through LANG-3T-12)
- `cargo test --workspace`: **1199 passed, 0 failed** (was 1188, +11 new)
- Clippy clean, rustfmt clean

## DoD checklist

- [x] FnSigTable built in pass 1 before any function body is checked
- [x] Expr::Call looks up callee, checks arity, checks arg types, infers return type
- [x] Expr::MethodCall dispatches on receiver type, returns collection method return types
- [x] Expr::PathCall handles Vec::new/with_capacity, looks up other paths
- [x] Mutual recursion supported (forward references resolve)
- [x] Self recursion supported
- [x] All error cases produce correct messages
- [x] All 1199 tests pass (11 new, 0 regressions)
- [x] Clippy clean, rustfmt clean

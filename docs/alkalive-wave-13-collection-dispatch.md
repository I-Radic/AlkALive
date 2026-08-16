# AlkALive Wave 13 — Collection Method Dispatch (Gap 5)

## Objective
Implement real collection method dispatch via WASM host imports, replacing placeholder `i32.const 0` with actual `call` instructions to imported host functions.

## What was implemented

### Host import infrastructure
- 10 host function imports under module `"alk"`: vec_new, vec_with_capacity, vec_push, vec_extend, vec_remove, vec_clear, vec_len, vec_is_empty, vec_get, vec_set
- Host imports occupy function indices 0..9 (lowest in the function index space)
- Import section emitted before function section (WASM binary format ordering)
- Host import types registered in the type section

### Method call compilation
- `Vec::new()` compiles to `i32.const 4; call vec_new` (elem_size=4 for all types)
- `Vec::with_capacity(n)` compiles to `i32.const 4; <compile n>; call vec_with_capacity`
- `v.push(x)` compiles to `compile v; compile x; call vec_push`
- `v.len()` compiles to `compile v; call vec_len` (returns i32)
- All 15 recognized Vec methods mapped to host imports via `vec_method_to_host()`

### Function index offsetting
- Module-local functions are offset by `host_import_count()` (10) in the function index space
- `AlkInstr::Call(name)` resolution checks host imports first (indices 0..9), then resolves module-local functions (offset by 10)
- Export section exports module functions at their absolute index (offset by 10)

### wasmparser validation
- All generated WASM binaries with host imports validated by wasmparser
- Import section has exactly 10 entries
- All 10 host import names present in the binary

## Tests: 9 new collection_dispatch_tests, 1240 total passed, 0 failed

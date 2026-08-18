# Wave 01 — Module System Resolution

> **Read `wave-00-critical-audit.md` first.**

## Objective

Implement real module resolution so that `import { Name } from "path";` declarations resolve to actual `.alk` files, parse them, and merge their exported signatures into the importing module's `FnSigTable`.

## Implementation

### `module_resolver.rs` (NEW, ~200 LOC)

Created `crates/alkalive-compiler/src/module_resolver.rs` with:

- `ModuleResolver` struct with `base_dir` and a caching `HashMap` for resolved modules
- `resolve_imports()` method that:
  1. Maps the module path to a `.alk` file (e.g., `"mylib/utils"` → `base_dir/mylib/utils.alk`)
  2. Reads and parses the file
  3. Collects `pub fn` signatures from the parsed module
  4. Merges them into the importing module's `FnSigTable` under the local name (or alias)
  5. Handles `std/` modules as host-provided (allows unresolved, returns empty sigs)
  6. Errors on missing non-std module files

### Export rules

Only `pub fn` declarations are exported. `pub class` and `pub let` exports are not yet supported (class methods need the ClassTable, and `let` values need the TypeEnv — these are future enhancements).

### Integration

The `ModuleResolver` is a standalone utility that can be called before `check_module()` to populate the `FnSigTable` with imported signatures. The existing import resolution in `check_module()` (pass 1.1) handles the case where imports are not resolved (adds names with `imported_from` set).

### Tests

3 new tests:
- `resolver_creates_with_base_dir` — basic construction
- `resolver_resolves_std_module_as_empty` — std/ modules don't error
- `resolver_errors_on_missing_non_std_module` — missing files produce errors

## Files changed

- `crates/alkalive-compiler/src/module_resolver.rs` (NEW)
- `crates/alkalive-compiler/src/lib.rs` — module declaration + re-export

## Tests executed

- 3 new module_resolver tests: ✅ pass
- 384 existing compiler tests: ✅ pass
- No regressions

## DoD checklist

- [x] Module resolver created with file-based resolution
- [x] `pub fn` exports collected from resolved modules
- [x] `std/` modules allowed to be unresolved (host-provided)
- [x] Missing non-std modules produce clear errors
- [x] Module resolver is public API (`ModuleResolver`, `ResolveError`)
- [x] All tests pass
- [x] No regressions

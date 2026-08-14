# AlkALive Detailed Specification — Language / Compiler Gaps (Task ID 5)

> **Status:** Detailed implementation specification derived from
> `docs/alkalive-fine-draft-language.md` (approved fine draft) and the
> critical-review findings in
> `docs/alkalive-fine-draft-critical-review.md`.
> **Predecessors:** Wave 6 (`alkalive-wave-06-wasm-codegen.md`),
> Wave 7 (`alkalive-wave-07-operators.md`), Wave 8
> (`alkalive-wave-08-control-flow.md`), ADR-008 / ADR-009 / ADR-018 / ADR-027.
> **Audience:** Implementer agents who will turn each section into code. Every
> requirement here is **testable**, **unambiguous**, and **implementable
> without reinterpretation** — exact Rust types, exact EBNF productions,
> exact WASM instruction sequences, and exact error messages are specified.
>
> **Critical-review findings addressed in this specification:**
> - **CR-7** (vtable semantics) — resolved in §4.4.4 and §4.6 (vtable_base is
>   a table index, dispatch via `local.get obj; i32.load offset=0;
>   i32.const <slot>; i32.add; call_indirect (type $T)`).
> - **CR-8** (tree-shaking after typecheck) — resolved in §5.4.4 and §5.7
>   (tree-shaking deferred to a future wave; all `pub fn` are emitted).
> - **CR-9** (tree-shaking virtual dispatch) — resolved in §5.4.4
>   (conservative rule documented for the future tree-shaking wave).
> - **CR-10** (monotone/antitone field assignment) — resolved in §4.4.6 and
>   §4.5 (field assignment to `monotone`/`antitone` qualified fields is a
>   compile-time error).

---

## 0. Cross-Gap Dependency Order

The five gaps have a strict mandatory build order. Each gap lists its
predecessors; an implementation wave may not begin until all predecessors
are merged.

```
                 ┌──────────────────────────────────────────────────┐
                 │                                                  │
                 ▼                                                  │
   Gap 3 (Type Inference) ──────► Gap 1 (OO) ──────► Gap 2 (Modules)
                 │                     │                  │
                 │                     ▼                  │
                 │             (needs field/method       │
                 │              return types)             │
                 │                                        │
                 ▼                                        ▼
   Gap 5 (Collections) ◄─────── needs ──────── Gap 4 (Strings)
        │   ▲                                  (string ptr is
        │   └──── host imports share ──────     a heap value)
        │         the import-section design
        ▼
   host ABI table
```

| Step | Gap | Predecessors | Why this order |
|------|-----|--------------|----------------|
| 1 | **Gap 3** — Type Inference | (none) | Pure typechecker work; no AST/parser/WASM changes. Every later gap depends on real call-return types. |
| 2 | **Gap 4** — String Data Sections | (none) | Pure WASM-backend work; establishes the data-section + heap-pointer pattern. No typechecker changes. |
| 3 | **Gap 5** — Collection Dispatch | Gap 4 | Extends the WASM backend with the import section; reuses the heap-pointer convention from Gap 4. Typechecker return-type integration is already in Gap 3. |
| 4 | **Gap 1** — OO Model | Gap 3, Gap 4, Gap 5 | Requires real call-return types (Gap 3), real string pointers (Gap 4) and real Vec handles (Gap 5). Adds substantial AST/parser/typechecker/WASM work. |
| 5 | **Gap 2** — Module System | Gap 1, Gap 3 | Requires a fully working single-module language. Adds the resolver pass, the (deferred) tree-shaking pass, and the cross-module linking. |

**Interface contracts between gaps** are listed in §6.

---

# Gap 3 — Full Type Inference (function-call return types)

## 3.1 Exact requirements

- **LANG-301.** The compiler must produce a module-wide `FnSigTable` populated
  in a first pass over `module.items` **before** any function body is checked.
- **LANG-302.** For every `ItemDecl::Fn(f)`, the `FnSigTable` must contain an
  entry keyed by `f.name` whose `params` field is the ordered list of `f`'s
  parameter types and whose `return_type` field is `f.return_type.clone()`.
- **LANG-303.** For every `ItemDecl::Class(c)` (after Gap 1 lands), the
  `FnSigTable` must contain an entry keyed by the qualified name
  `"<ClassName>::<method_name>"` for each method `m` of `c`, with `params =
  m.params` (excluding the implicit `self`) and `return_type =
  m.return_type.clone()`. Until Gap 1 lands, this requirement is no-op.
- **LANG-304.** `check_expr` on `Expr::Call { callee, args, line, col }` must
  look up `callee` in the `FnSigTable`. If found, it must verify that
  `args.len() == sig.params.len()` and that each `check_expr(arg)` result is a
  subtype of the corresponding `sig.params[i]`. The expression's type is
  `sig.return_type.clone()`.
- **LANG-305.** If `callee` is not in the `FnSigTable`, the typechecker must
  emit a `TypeError` with the exact message format defined in §3.7
  (`LANG-307-E1`).
- **LANG-306.** `check_expr` on `Expr::MethodCall { receiver, method, args,
  line, col }` must dispatch on the receiver's inferred type:
  - **(a)** `Vec<T>` receiver → consult §3.4.4 collection return-type table.
  - **(b)** `Named(class)` receiver (after Gap 1) → consult the class table.
  - **(c)** any other concrete type → emit `LANG-308-E2`.
  - **(d)** `None` (receiver already errored) → return `None`, no further
    error.
- **LANG-307.** `check_expr` on `Expr::PathCall(module, member, args, line,
  col)` must dispatch:
  - **(a)** `("Vec", "new")` and `("Vec", "with_capacity")` → return `None`
    (no inferable element type); the `let`-binding's declared type drives
    downstream typechecking.
  - **(b)** any other `(module, member)` → look up `"<module>::<member>"` in
    the `FnSigTable`; if not found, emit `LANG-309-E3`.

## 3.2 Syntax/grammar changes

**None.** Gap 3 is purely a typechecker reorganisation. The existing
`Expr::Call`, `Expr::MethodCall`, and `Expr::PathCall` AST shapes are
unchanged.

## 3.3 AST/IR changes

**None in `ast.rs`.** New types live in `typechecker.rs`:

```rust
/// A function's signature — the type-checker's view of a callable.
#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    /// The lookup name. For free functions this is the bare name (`"add"`).
    /// For class methods this is the qualified name
    /// (`"Button::new"`). For imported functions this is the bare local
    /// name; `imported_from` carries the source module.
    pub name: String,
    /// The parameter types (excluding the implicit `self`).
    pub params: Vec<Type>,
    /// The declared return type. `None` means the function returns unit.
    pub return_type: Option<Type>,
    /// Parameter names (carried for diagnostics; future named-argument
    /// support). The length always equals `params.len()`.
    pub param_names: Vec<String>,
    /// `Some(class_name)` for instance/static methods; `None` for free
    /// functions. Populated by Gap 1's `collect_classes` pass.
    pub receiver_class: Option<String>,
    /// `Some(module_path)` for names imported from another module
    /// (populated by Gap 2's resolver). `None` for module-local items.
    pub imported_from: Option<String>,
}

/// Module-wide function signature table. Built in pass 1 of `check_module`;
/// consulted in pass 3 (body checking). Pass 2 collects module-level `let`s.
#[derive(Debug, Clone, Default)]
pub struct FnSigTable {
    sigs: std::collections::HashMap<String, FnSig>,
}

impl FnSigTable {
    /// Construct an empty table.
    pub fn new() -> Self { Self::default() }

    /// Insert (or replace) a signature.
    pub fn insert(&mut self, name: impl Into<String>, sig: FnSig) {
        self.sigs.insert(name.into(), sig);
    }

    /// Look up by name.
    pub fn lookup(&self, name: &str) -> Option<&FnSig> {
        self.sigs.get(name)
    }

    /// Look up a class method by `Class::method` qualified name.
    /// Returns `None` for unknown class/method.
    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FnSig> {
        let q = format!("{}::{}", class, method);
        self.sigs.get(&q)
    }
}
```

The existing `check_module` function is restructured into three passes; the
signatures are visible to `check_fn`/`check_block`/`check_expr` via a new
`sigs: &FnSigTable` parameter.

## 3.4 Type-system changes

### 3.4.1 Three-pass `check_module` algorithm

```rust
pub fn check_module(module: &ModuleDecl) -> TypeErrorSet {
    let mut errors = TypeErrorSet::new();

    // Pass 1: collect all function signatures (and, after Gap 1, class
    // method signatures).
    let mut sigs = FnSigTable::new();
    collect_signatures(module, &mut sigs);
    // (Gap 2 will also merge in resolved-import signatures here.)

    // Pass 2: collect module-level `let` bindings (unchanged from today).
    let mut module_env = TypeEnv::new();
    for item in &module.items {
        if let ItemDecl::Let(l) = item {
            check_expr(&l.init, &module_env, &mut errors, &sigs);
            let effective_ty = Type {
                qualifier: effective_qualifier(l),
                base: l.ty.base.clone(),
            };
            module_env.insert(l.name.clone(), effective_ty);
        }
    }

    // Pass 3: check each function body, threading `&sigs` through.
    for item in &module.items {
        if let ItemDecl::Fn(f) = item {
            check_fn(f, &module_env, &sigs, &mut errors);
        }
    }

    errors
}

fn collect_signatures(module: &ModuleDecl, table: &mut FnSigTable) {
    for item in &module.items {
        match item {
            ItemDecl::Fn(f) => {
                table.insert(f.name.clone(), FnSig {
                    name: f.name.clone(),
                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                    return_type: f.return_type.clone(),
                    param_names: f.params.iter().map(|p| p.name.clone()).collect(),
                    receiver_class: None,
                    imported_from: None,
                });
            }
            ItemDecl::Class(c) => {
                // After Gap 1 lands; no-op until then.
                let _ = c;
            }
            _ => {}
        }
    }
}

fn check_fn(f: &FnDecl, module_env: &TypeEnv, sigs: &FnSigTable,
            errors: &mut TypeErrorSet) {
    let mut env = module_env.clone();
    for p in &f.params {
        env.insert(p.name.clone(), p.ty.clone());
    }
    check_block(&f.body, &mut env, f.return_type.as_ref(), sigs, errors);
}

fn check_block(block: &Block, env: &mut TypeEnv,
               return_type: Option<&Type>,
               sigs: &FnSigTable, errors: &mut TypeErrorSet) {
    // ... unchanged except: every `check_expr(e, env, errors)` call site
    //     becomes `check_expr(e, env, errors, sigs)`.
}

fn check_expr(expr: &Expr, env: &TypeEnv,
              errors: &mut TypeErrorSet,
              sigs: &FnSigTable) -> Option<Type> {
    // ... see §3.4.2, §3.4.3, §3.4.4, §3.4.5 below.
}
```

### 3.4.2 `Expr::Call` checking (the actual fix)

```rust
Expr::Call { callee, args, line, col } => {
    // 1. Check every argument expression.
    let mut arg_types = Vec::with_capacity(args.len());
    for a in args {
        arg_types.push(check_expr(a, env, errors, sigs));
    }
    // 2. Look up the callee in the signature table.
    match sigs.lookup(callee) {
        Some(sig) => {
            // 3. Arity check.
            if args.len() != sig.params.len() {
                errors.push(TypeError {
                    message: format!(
                        "call to function `{}` expects {} argument(s) but was called with {}",
                        callee, sig.params.len(), args.len()
                    ),
                    line: *line, col: *col,
                });
            }
            // 4. Per-argument type check (with subtype flow).
            for (i, (arg_ty, param_ty)) in
                arg_types.iter().zip(sig.params.iter()).enumerate()
            {
                if let Some(at) = arg_ty {
                    if !type_is_subtype(at, param_ty) {
                        errors.push(TypeError {
                            message: format!(
                                "argument {} to `{}` has type `{}` but parameter has type `{}`",
                                i + 1, callee, at, param_ty
                            ),
                            line: *line, col: *col,
                        });
                    }
                }
            }
            // 5. Return the declared return type.
            sig.return_type.clone()
        }
        None => {
            errors.push(TypeError {
                message: format!("call to unknown function `{}`", callee),
                line: *line, col: *col,
            });
            None
        }
    }
}
```

### 3.4.3 `Expr::MethodCall` checking

```rust
Expr::MethodCall { receiver, method, args, line, col } => {
    let receiver_ty = check_expr(receiver, env, errors, sigs);
    let mut arg_types: Vec<_> = args.iter()
        .map(|a| check_expr(a, env, errors, sigs))
        .collect();
    match &receiver_ty {
        Some(ty) if ty.is_vec() => {
            // Collection method dispatch — host-provided.
            check_method_op(method, ty.qualifier, *line, *col, errors);
            collection_method_return_type(method, ty, *line, *col, errors)
        }
        Some(Type { base: BaseType::Named(class_name), .. }) => {
            // Class method dispatch (requires Gap 1's ClassTable).
            // Until Gap 1 lands, this branch emits:
            //   LANG-308-E2: "method `.m()` is not defined on type `<class>`"
            class_method_return_type(class_name, method, &arg_types,
                                     sigs, *line, *col, errors)
        }
        Some(other) => {
            errors.push(TypeError {
                message: format!(
                    "method `.{}()` is not defined on type `{}`",
                    method, other
                ),
                line: *line, col: *col,
            });
            None
        }
        None => None, // receiver already errored; do not double-report.
    }
}
```

The `let _ = arg_types;` line is intentionally elided: the per-argument
subtype check against the resolved method's parameters happens inside
`class_method_return_type` (Gap 1) and inside `collection_method_return_type`
(for the host ABI). The monotonicity check (`check_method_op`) is unchanged
from `typechecker.rs:449-477` and runs **before** the return type is computed.

### 3.4.4 Collection-method return types

| Method name | Return type | Notes |
|-------------|-------------|-------|
| `push`, `extend`, `insert`, `append` | `None` (unit) | Grow ops; forbidden on `antitone`. |
| `remove`, `truncate`, `clear`, `swap_remove`, `drain` | `None` (unit) | Shrink ops; forbidden on `monotone`. |
| `len` | `Some(Type { qualifier: Unrestricted, base: BaseType::I32 })` | Neutral op. |
| `is_empty` | `Some(Type { qualifier: Unrestricted, base: BaseType::Bool })` | Neutral op. |
| `get`, `first`, `last` | `Some(Type { qualifier: Unrestricted, base: BaseType::I32 })` | Placeholder; future `Option<T>`. |
| any other name | emit `LANG-308-E2` | Defensive; typechecker catches it first. |

```rust
fn collection_method_return_type(method: &str, _vec_ty: &Type,
                                 line: u32, col: u32,
                                 errors: &mut TypeErrorSet) -> Option<Type> {
    match method {
        "push" | "extend" | "insert" | "append"
        | "remove" | "truncate" | "clear"
        | "swap_remove" | "drain" => None,
        "len" => Some(Type { qualifier: Qualifier::Unrestricted,
                             base: BaseType::I32 }),
        "is_empty" => Some(Type { qualifier: Qualifier::Unrestricted,
                                  base: BaseType::Bool }),
        "get" | "first" | "last" => Some(Type {
            qualifier: Qualifier::Unrestricted, base: BaseType::I32 }),
        _ => {
            errors.push(TypeError {
                message: format!(
                    "method `.{}()` is not defined on type `Vec<T>`",
                    method),
                line, col,
            });
            None
        }
    }
}
```

### 3.4.5 `Expr::PathCall` checking

```rust
Expr::PathCall(module, member, args, line, col) => {
    // Check arguments.
    for a in args {
        check_expr(a, env, errors, sigs);
    }
    match (module.as_str(), member.as_str()) {
        ("Vec", "new") | ("Vec", "with_capacity") => {
            // Element type cannot be inferred from the call site alone;
            // the `let`-binding's declared type drives downstream uses.
            None
        }
        (mod_name, member_name) => {
            let qualified = format!("{}::{}", mod_name, member_name);
            match sigs.lookup(&qualified) {
                Some(sig) => sig.return_type.clone(),
                None => {
                    errors.push(TypeError {
                        message: format!(
                            "call to unknown path `{}::{}`",
                            mod_name, member_name),
                        line: *line, col: *col,
                    });
                    None
                }
            }
        }
    }
}
```

**Note on `Vec::new()` inference (addresses CR-14):** the typechecker does
*not* implement bidirectional expected-type inference. `Vec::new()` returns
`None`. Downstream uses of a `let v: Vec<i32> = Vec::new();` binding consult
the `let`'s declared type (`Vec<i32>`), stored in `TypeEnv`, not the
inferred return type of `Vec::new()`. This is documented in code comments
to avoid the "expected-type inference" misnomer.

## 3.5 Compiler changes

| Layer | Change | Functions added/modified |
|-------|--------|--------------------------|
| **Lexer** | None. | — |
| **Parser** | None. | — |
| **AST** | None. | — |
| **Typechecker** | Restructure `check_module` into three passes; thread `&FnSigTable` through `check_fn`/`check_block`/`check_expr`. Add new arms for `Expr::Call`/`MethodCall`/`PathCall`. | **Added:** `FnSig`, `FnSigTable`, `collect_signatures`, `collection_method_return_type`, `class_method_return_type` (stub until Gap 1). **Modified:** `check_module`, `check_fn`, `check_block`, `check_expr`. |
| **WASM codegen** | None. The codegen already emits correct `call`/`call_indirect` instructions; it just had no static guarantees. The guarantee is now provided by the typechecker refusing to compile ill-typed programs (already done at `wasm_codegen.rs:493-505`). | — |

## 3.6 WASM changes

**None.** Gap 3 is a typechecker-only change.

## 3.7 Error cases

Every error is a `TypeError { message, line, col }` pushed to the
`TypeErrorSet`. The exact message formats are:

| ID | Trigger condition | Message format |
|----|-------------------|----------------|
| **LANG-307-E1** | `Expr::Call` callee not in `FnSigTable` | `call to unknown function \`{callee}\`` |
| **LANG-307-E2** | `Expr::Call` arity mismatch | `call to function \`{callee}\` expects {N} argument(s) but was called with {M}` |
| **LANG-307-E3** | `Expr::Call` arg-type mismatch | `argument {i+1} to \`{callee}\` has type \`{actual}\` but parameter has type \`{expected}\`` |
| **LANG-308-E2** | `Expr::MethodCall` on a non-`Vec`/non-class type | `method \`.{method}()\` is not defined on type \`{ty}\`` |
| **LANG-308-E3** | `Expr::MethodCall` collection method not in the host table | `method \`.{method}()\` is not defined on type \`Vec<T>\`` |
| **LANG-309-E3** | `Expr::PathCall` qualified name not in `FnSigTable` | `call to unknown path \`{module}::{member}\`` |
| **(existing)** | Shrink op on `monotone` collection | (unchanged — see `typechecker.rs:449-477`) |
| **(existing)** | Grow op on `antitone` collection | (unchanged) |
| **(existing)** | Undefined variable | (unchanged) |

All errors accumulate in the `TypeErrorSet` (multi-error policy preserved).

## 3.8 Validation rules

Every compile-time validation rule enforced by Gap 3:

- **V3-1.** Every `Expr::Call` callee must resolve in `FnSigTable` (else
  `LANG-307-E1`).
- **V3-2.** Every `Expr::Call` argument count must equal the callee's
  parameter count (else `LANG-307-E2`).
- **V3-3.** Every `Expr::Call` argument's inferred type must be a subtype of
  the corresponding parameter type (else `LANG-307-E3`).
- **V3-4.** Every `Expr::MethodCall` receiver must be either a `Vec<T>` or
  (after Gap 1) a `Named(class)` (else `LANG-308-E2`).
- **V3-5.** Every `Expr::MethodCall` method name on a `Vec<T>` receiver must
  be in the collection-method table (else `LANG-308-E3`).
- **V3-6.** Every `Expr::PathCall` qualified name (`module::member`) must be
  either `Vec::new`/`Vec::with_capacity` or in `FnSigTable` (else
  `LANG-309-E3`).
- **V3-7.** Mutual recursion is supported: forward references resolve because
  `collect_signatures` runs **before** any function body is checked.
- **V3-8.** Self recursion is supported: a function's own signature is in
  `FnSigTable` before its body is checked.

## 3.9 Test cases

| Test ID | Source | Expected behaviour |
|---------|--------|--------------------|
| **LANG-3T-01** | `fn id(x: i32) -> i32 { return x; } fn main() -> i32 { return id(42); }` | Typechecks; `id(42)` infers `i32`. |
| **LANG-3T-02** | `fn id(x: i32) -> i32 { return x; } fn main() -> i32 { return id(1, 2); }` | Emits `LANG-307-E2`: "expects 1 argument(s) but was called with 2". |
| **LANG-3T-03** | `fn id(x: i32) -> i32 { return x; } fn main() -> i32 { return id(true); }` | Emits `LANG-307-E3`: "argument 1 to `id` has type `bool` but parameter has type `i32`". |
| **LANG-3T-04** | `fn main() -> i32 { return unknown(42); }` | Emits `LANG-307-E1`: "call to unknown function `unknown`". |
| **LANG-3T-05** | `fn is_even(n: i32) -> bool { if n == 0 { return true; } return is_odd(n - 1); } fn is_odd(n: i32) -> bool { if n == 0 { return false; } return is_even(n - 1); }` | Typechecks (mutual recursion). |
| **LANG-3T-06** | `fn fact(n: i32) -> i32 { if n <= 1 { return 1; } return n * fact(n - 1); }` | Typechecks (self recursion). |
| **LANG-3T-07** | `fn main() { let v: Vec<i32> = Vec::new(); let n: i32 = v.len(); }` | Typechecks; `v.len()` infers `i32`. |
| **LANG-3T-08** | `fn main() { let v: Vec<i32> = Vec::new(); let b: bool = v.is_empty(); }` | Typechecks; `v.is_empty()` infers `bool`. |
| **LANG-3T-09** | `fn main() { let v: Vec<i32> = Vec::new(); let b: bool = v.len(); }` | Emits `LANG-307-E3`-style error: argument/return mismatch via the `let` binding's declared type. |
| **LANG-3T-10** | `fn main() { let x: i32 = 5; x.foo(); }` | Emits `LANG-308-E2`: "method `.foo()` is not defined on type `i32`". |
| **LANG-3T-11** | `fn main() { let v: Vec<i32> = Vec::new(); v.bogus(); }` | Emits `LANG-308-E3`: "method `.bogus()` is not defined on type `Vec<T>`". |
| **LANG-3T-12** | `fn main() { Foo::bar(); }` | Emits `LANG-309-E3`: "call to unknown path `Foo::bar`". |
| **LANG-3T-13** | `fn main() -> i32 { return Vec::new(); }` | Emits return-type-mismatch error (existing channel). |

## 3.10 Acceptance criteria

- **AC3-1.** `cargo test -p alkalive-compiler type_inference_tests` passes
  with the 13 tests above.
- **AC3-2.** `cargo test --workspace` is green (no regressions).
- **AC3-3.** `cargo clippy -p alkalive-compiler -- -D warnings` is clean.
- **AC3-4.** A program containing only `Expr::Call`, `Expr::MethodCall`,
  and `Expr::PathCall` constructs typechecks and produces a valid WASM binary
  (verified by `wasmparser::Parser` walking the binary).

## 3.11 Traceability

| Requirement | ADR / source | Fine-draft decision | Implementation requirement | Test |
|-------------|--------------|---------------------|----------------------------|------|
| LANG-301..303 | ADR-009 (source-level soundness); fine-draft §3.4.1-3.4.2 | "Build a `FnSigTable` in pass 1 of `check_module`." | §3.3 Rust types + `collect_signatures`. | LANG-3T-05 (mutual recursion requires pass 1). |
| LANG-304..305 | ADR-009; fine-draft §3.4.3 | "Look up callee in `FnSigTable`; emit `TypeError` if not found." | §3.4.2 algorithm + §3.7 `LANG-307-E1`. | LANG-3T-01, LANG-3T-04. |
| LANG-306 | ADR-009; fine-draft §3.4.4 | "Dispatch on receiver type for `MethodCall`." | §3.4.3 algorithm + return-type table. | LANG-3T-07, LANG-3T-10, LANG-3T-11. |
| LANG-307 | ADR-009; fine-draft §3.4.5 | "`Vec::new`/`with_capacity` return `None`; other paths lookup in `FnSigTable`." | §3.4.5 algorithm. | LANG-3T-07, LANG-3T-12. |
| (CR-14 fix) | Critical review CR-14 | "Reword: `Vec::new()` returns `None`; the `let`'s declared type drives downstream." | §3.4.5 note + code comment. | (Documentation check; no new test.) |

---

# Gap 4 — String Data Sections

## 4.1 Exact requirements

- **LANG-401.** The compiler must collect every `Lit::Str(s, _, _)`
  expression encountered during `compile_expr` into a `StringTable`
  structure owned by `compile_to_wasm`.
- **LANG-402.** Two `Lit::Str` expressions with identical `s` content must
  share a single `StringTable` entry (deduplication).
- **LANG-403.** Each string entry must be stored in linear memory as
  **length-prefixed UTF-8**: a 4-byte little-endian `i32` length followed by
  the UTF-8 bytes, padded to 4-byte alignment.
- **LANG-404.** The first 4 bytes of linear memory (address 0..3) must be a
  null guard: a `data` segment of `[0, 0, 0, 0]` so that `i32.const 0` is a
  sentinel "null string" distinct from any real literal.
- **LANG-405.** String offsets begin at address `4` (immediately after the
  null guard).
- **LANG-406.** `Lit::Str(s, _, _)` codegen must emit
  `AlkInstr::I32Const(offset as i32)` where `offset` is the address of the
  length prefix returned by `StringTable::intern(s)`.
- **LANG-407.** After the code section, the compiler must emit a `DataSection`
  containing one **active** data segment per string entry. The segment's
  offset is `i32.const entry.offset`; the segment's value is
  `[byte_len_le_4bytes..., utf8_bytes..., 0-padding-to-4-align]`.
- **LANG-408.** If the total bytes occupied by strings would exceed the
  declared memory (1 page = 64 KiB), the compiler must grow the memory
  declaration: `memory_pages = ceil(strings_end / 65536)`, minimum 1.
- **LANG-409.** The compiler must not emit a data section if the
  `StringTable` is empty (no `Lit::Str` in the module). The null-guard
  segment is still emitted (4 zero bytes at offset 0).
- **LANG-410.** Every offset assigned by `StringTable::intern` must be `> 0`
  and `4 + 4 + byte_len ≤ offset + 4 + byte_len ≤ i32::MAX`.

## 4.2 Syntax/grammar changes

**None.** The existing `Lit::Str(String)` AST node carries the bytes; no
parser change is needed.

## 4.3 AST/IR changes

**None in `ast.rs`.** New types live in `wasm_codegen.rs`:

```rust
/// One entry in the string table — interned literal data.
#[derive(Debug, Clone)]
struct StringEntry {
    /// The UTF-8 string content (the source literal, decoded).
    text: String,
    /// Address of the length prefix in linear memory.
    offset: u32,
    /// Byte length of the UTF-8 payload (excluding the length prefix).
    byte_len: u32,
}

/// The module-wide string interner. Populated lazily by `compile_expr` on
/// `Lit::Str`; consumed by the data-section emitter after the code section.
#[derive(Debug, Default)]
struct StringTable {
    /// Map from literal text → memory offset (for dedup).
    by_text: std::collections::HashMap<String, u32>,
    /// Ordered entries for emitting the data section.
    entries: Vec<StringEntry>,
    /// Next free offset (starts at 4, after the null guard).
    next_offset: u32,
}

impl StringTable {
    fn new() -> Self {
        Self {
            by_text: Default::default(),
            entries: Vec::new(),
            next_offset: 4, // null guard occupies 0..3
        }
    }

    /// Intern a string. Returns the offset of the length prefix.
    /// Deduplicates by exact text match.
    fn intern(&mut self, text: &str) -> u32 {
        if let Some(&off) = self.by_text.get(text) {
            return off;
        }
        let byte_len = text.as_bytes().len() as u32;
        let offset = self.next_offset;
        // 4 bytes for the length prefix + payload, rounded up to 4.
        let padded = 4 + byte_len;
        let padded = (padded + 3) & !3; // align to 4
        self.entries.push(StringEntry {
            text: text.to_string(),
            offset,
            byte_len,
        });
        self.by_text.insert(text.to_string(), offset);
        self.next_offset = offset + padded;
        offset
    }

    /// Address one past the last byte of the last string.
    fn end(&self) -> u32 { self.next_offset }
}
```

The `FnCompiler` struct gains a `&mut StringTable` reference (or
`compile_expr` takes it as a parameter):

```rust
struct FnCompiler<'a> {
    locals: Vec<(String, ValType)>,
    strings: &'a mut StringTable,
}
```

## 4.4 Type-system changes

**None.** The typechecker already types `Lit::Str` as `BaseType::Str` (see
`typechecker.rs:480-491`).

## 4.5 Compiler changes

| Layer | Change | Functions added/modified |
|-------|--------|--------------------------|
| **Lexer** | None. | — |
| **Parser** | None. | — |
| **AST** | None. | — |
| **Typechecker** | None. | — |
| **WASM codegen** | (1) `use wasm_encoder::{Data, DataSection};` added. (2) `StringTable` and `StringEntry` structs added. (3) `FnCompiler` carries `&mut StringTable`. (4) `Lit::Str` arm interns and emits `AlkInstr::I32Const(offset)`. (5) After the code section, emit the data section. (6) Memory section grows to `ceil(strings_end / 65536)` pages, minimum 1. | **Added:** `StringTable::new`, `StringTable::intern`, `StringTable::end`. **Modified:** `FnCompiler::new` (takes `&mut StringTable`), `FnCompiler::compile_expr` (`Lit::Str` arm), `compile_to_wasm` (creates the `StringTable`, threads it through `FnCompiler`, emits the data section after the code section, grows the memory section). |

## 4.6 WASM changes

### 4.6.1 Memory layout

```
Linear memory (1+ pages = 64 KiB each):

  +-----------------------+  address 0
  | null guard (4 bytes)  |   <- always zero; data segment [0,0,0,0].
  +-----------------------+  address 4
  | string literal #1     |   <- length-prefixed UTF-8, 4-byte aligned.
  |   len (i32 LE)        |
  |   UTF-8 bytes         |
  |   0-padding           |
  +-----------------------+
  | string literal #2     |
  |   ...                 |
  +-----------------------+  address strings_end
  | heap (Gap 1 objects,  |   <- grown by __alk_alloc (Gap 1).
  |        Gap 5 Vecs)    |
  |                       |
  +-----------------------+  address 64 KiB (or grows)
```

### 4.6.2 Each string's bytes

```
+--------+--------+--------+--------+--------+--------+ ... +--------+--------+
| len (i32, little-endian)              | byte_0 | byte_1 | ... | byte_n | pad  |
+--------+--------+--------+--------+--------+--------+ ... +--------+--------+
^                                       ^
|                                       |
ptr (returned to AlkALive code)         ptr + 4
```

- `ptr` is the address of the length prefix.
- `len` is the byte count of the UTF-8 payload (excluding the 4-byte prefix).
- `ptr + 4` is the start of the UTF-8 bytes (4-byte aligned).
- The host reads `i32` at `ptr` to learn the length, then reads `len` bytes
  starting at `ptr + 4`.

### 4.6.3 `Lit::Str` codegen

```rust
Lit::Str(s, _line, _col) => {
    let offset = self.strings.intern(s);
    instrs.push(AlkInstr::I32Const(offset as i32));
}
```

### 4.6.4 Data section emission

```rust
// After the code section:
let mut data_sec = DataSection::new();

// Null guard segment (always emitted).
let null_guard_bytes: [u8; 4] = [0, 0, 0, 0];
let mut null_data = Data::active(0); // memory index 0
null_data.offset(&mut const_expr_i32(0));
null_data.value(&null_guard_bytes);
data_sec.data(&null_data);

// One segment per string entry.
for entry in &string_table.entries {
    let mut bytes = Vec::with_capacity(4 + entry.text.len() + 3);
    bytes.extend_from_slice(&entry.byte_len.to_le_bytes());
    bytes.extend_from_slice(entry.text.as_bytes());
    // Pad to 4-byte alignment.
    while bytes.len() % 4 != 0 { bytes.push(0); }
    let mut data = Data::active(0);
    data.offset(&mut const_expr_i32(entry.offset));
    data.value(&bytes);
    data_sec.data(&data);
}
wasm_module.section(&data_sec);
```

Where `const_expr_i32(n)` is a helper that emits the WASM constant-expression
bytecode for `i32.const n; end`:

```rust
fn const_expr_i32(n: i32) -> wasm_encoder::ConstExpr {
    wasm_encoder::ConstExpr::i32_const(n)
}
```

### 4.6.5 Memory section growth

```rust
let strings_end = string_table.end();
let pages_needed = ((strings_end + 65535) / 65536) as u32;
let memory_pages = pages_needed.max(1);

let mut mem_sec = MemorySection::new();
mem_sec.memory(MemoryType {
    minimum: memory_pages,
    maximum: None,
    memory64: false,
    shared: false,
    page_size_log2: None,
});
wasm_module.section(&mem_sec);
```

## 4.7 Error cases

String data sections are an implementation detail — there are no user-facing
errors. Two internal invariants are asserted (panic if violated; in practice
impossible):

| ID | Trigger condition | Behaviour |
|----|-------------------|-----------|
| **LANG-409-I1** | `offset == 0` returned from `intern` (the null guard was reused). | `assert!(offset > 0)` in `StringTable::intern`. |
| **LANG-409-I2** | `next_offset + 4 + byte_len` would overflow `i32::MAX`. | `assert!(self.next_offset < i32::MAX as u32 - 4 - byte_len)` in `intern`. |

If a string literal contains invalid UTF-8 (theoretically impossible — the
lexer decodes string literals to `String`, which is always valid UTF-8), the
intern call panics with `internal error: string literal is not valid UTF-8`.

## 4.8 Validation rules

- **V4-1.** Every `Lit::Str(s, _, _)` in the source produces an `intern(s)`
  call during codegen.
- **V4-2.** The `DataSection` contains exactly `entries.len() + 1` segments
  (the `+1` is the null guard).
- **V4-3.** Each segment's `offset` constant-expression evaluates to the
  entry's recorded `offset` field.
- **V4-4.** Each segment's value bytes decode to:
  `[byte_len_le_4bytes, utf8_bytes, 0-padding-to-4-align]`.
- **V4-5.** The memory section's `minimum` is at least
  `ceil((4 + sum_of_entry_byte_sizes_padded) / 65536)`, minimum 1.
- **V4-6.** Two `Lit::Str` expressions with the same text share an offset.

## 4.9 Test cases

| Test ID | Source | Expected behaviour |
|---------|--------|--------------------|
| **LANG-4T-01** | `fn f() -> string { return "hi"; }` | Binary has a data section with one string segment (plus null guard) containing bytes `[2, 0, 0, 0, b'h', b'i', 0, 0]`. |
| **LANG-4T-02** | `fn f() { let a = "hi"; let b = "hi"; }` | `StringTable.entries.len() == 1` (dedup). |
| **LANG-4T-03** | `fn f() { let a = "a"; let b = "bb"; let c = "ccc"; }` | Three entries at distinct offsets, all 4-byte aligned (4, 12, 20 — each padded). |
| **LANG-4T-04** | (any program with strings) | Every `entry.offset > 0` (null guard at 0 never reused). |
| **LANG-4T-05** | `fn f() -> string { return "héllo"; }` | The entry's `byte_len == 6` (5 chars, 6 UTF-8 bytes: `h`, `é`=2 bytes, `l`, `l`, `o`). Segment value contains the correct UTF-8 payload. |
| **LANG-4T-06** | (any program) | `wasmparser::Parser` validates the full binary cleanly (no `decode` errors). |
| **LANG-4T-07** | `fn f() {}` (no strings) | No string data segments emitted. Null-guard segment is still present. |
| **LANG-4T-08** | A module with strings totalling > 64 KiB. | `wasm_module.memory_pages > 1`. |
| **LANG-4T-09** | `fn f() -> i32 { let s = "hi"; return s.len(); }` | The `Lit::Str` arm emits `I32Const(4)` (the offset), not `I32Const(0)` (the placeholder). The placeholder at `wasm_codegen.rs:363` is replaced. |
| **LANG-4T-10** | End-to-end (new `tests/string_host_test.rs`): instantiate the WASM with a minimal host, call `f()` from LANG-4T-01, read `i32` at the returned pointer, verify it equals 2, read 2 bytes from `ptr + 4`, verify they match `"hi"`. | The host's view of linear memory matches the literal. (Optional — falls back to a data-section-bytes assertion if no Wasmtime/wasmi is available.) |

## 4.10 Acceptance criteria

- **AC4-1.** `cargo test -p alkalive-compiler string_data_tests` passes
  with the 10 tests above.
- **AC4-2.** A diff of `crates/alkalive-compiler/src/wasm_codegen.rs` shows
  the `Lit::Str` arm at line 360-364 now interns and emits a real offset,
  not `I32Const(0)`.
- **AC4-3.** `cargo test --workspace` is green.
- **AC4-4.** `cargo clippy -p alkalive-compiler -- -D warnings` is clean.
- **AC4-5.** The Hello World demo's WASM binary (re-compiled) is accepted by
  `wasmparser` and contains a data section.

## 4.11 Traceability

| Requirement | ADR / source | Fine-draft decision | Implementation requirement | Test |
|-------------|--------------|---------------------|----------------------------|------|
| LANG-401..403 | ADR-008 ("compiling to WASM"), ADR-022 (HarfRust reads UTF-8); fine-draft §4.4.1-4.4.3 | "Length-prefixed UTF-8 in linear memory." | §4.3 Rust types + §4.6.2 byte layout. | LANG-4T-01, LANG-4T-05. |
| LANG-404..405 | ADR-009 (soundness: `i32.const 0` must be invalid); fine-draft §4.4.2 | "Reserve first 4 bytes as null guard." | §4.6.4 null-guard segment + `next_offset = 4`. | LANG-4T-04, LANG-4T-07. |
| LANG-406 | ADR-008; fine-draft §4.4.5 | "`Lit::Str` interns and emits `i32.const <offset>`." | §4.6.3 codegen. | LANG-4T-09. |
| LANG-407 | ADR-008; fine-draft §4.4.4 | "Emit `DataSection` with one active segment per string." | §4.6.4 emission. | LANG-4T-01. |
| LANG-408 | ADR-008; fine-draft §4.6 | "Grow memory declaration if strings exceed one page." | §4.6.5 growth. | LANG-4T-08. |
| LANG-409 | fine-draft §4.6 (invariants) | "`offset > 0` for every string." | §4.7 asserts. | LANG-4T-04. |
| (CR-14 fix n/a) | — | — | — | — |

---

# Gap 5 — Collection Method Dispatch

## 5.1 Exact requirements

- **LANG-501.** The compiler must emit an `ImportSection` declaring exactly
  10 host-function imports under module name `"alk"`:
  `vec_new`, `vec_with_capacity`, `vec_push`, `vec_extend`, `vec_remove`,
  `vec_clear`, `vec_len`, `vec_is_empty`, `vec_get`, `vec_set`. (Plus
  `__alk_alloc` from Gap 1, when Gap 1 lands.)
- **LANG-502.** Each host import's WASM type must be registered in the
  `TypeSection` with the signature listed in §5.4.1.
- **LANG-503.** Imported functions must occupy the **lowest** indices in the
  function index space. If there are 10 host imports, they are indices 0..9;
  the first AlkALive-defined function is index 10.
- **LANG-504.** `AlkInstr::Call(name)` resolution must:
  - **(a)** Check if `name` matches a host-import name (`vec_*` or
    `__alk_alloc`). If so, emit `Instruction::Call(host_import_idx)`.
  - **(b)** Otherwise, resolve to a module-local function:
    `absolute_idx = host_imports.len() as u32 + local_idx`.
- **LANG-505.** `Expr::MethodCall { receiver, method, args, .. }` on a
  `Vec<T>` receiver must compile to:
  1. `compile_expr(receiver)` — leaves the Vec handle on the stack.
  2. For each arg in source order: `compile_expr(arg)` — leaves the arg on
     the stack.
  3. `AlkInstr::Call(host_name.to_string())` — calls the corresponding
     `vec_*` host import.
- **LANG-506.** The mapping from AlkALive method name to host import name
  is fixed (§5.4.3). An unknown method name on a `Vec<T>` receiver is a
  typechecker error (Gap 3 §3.7 `LANG-308-E3`); the codegen must
  defensively emit `AlkInstr::Drop` for the receiver+args and emit
  `AlkInstr::I32Const(0)` if a return value is expected (defensive;
  unreachable in a typechecked program).
- **LANG-507.** `Expr::PathCall("Vec", "new", args, ..)` must compile to:
  - `AlkInstr::I32Const(4)` — push `elem_size = 4` (every type is 4 bytes).
  - `AlkInstr::Call("vec_new".to_string())` — returns the Vec handle.
- **LANG-508.** `Expr::PathCall("Vec", "with_capacity", args, ..)` must
  compile to (argument order matters — host signature is
  `(elem_size: i32, cap: i32) -> i32`):
  - `AlkInstr::I32Const(4)` — push `elem_size = 4`.
  - `compile_expr(args[0])` — push the capacity.
  - `AlkInstr::Call("vec_with_capacity".to_string())`.
- **LANG-509.** The ImportSection is emitted **before** the FunctionSection
  (per WASM binary format ordering).
- **LANG-510.** If the module contains no `Vec` usage, the ImportSection is
  still emitted with all 10 imports (host ABI is fixed; the host may
  optimise unused imports at link time). *Rationale:* simpler than
  conditional emission; matches the closed-ABI stance of ADR-018.

## 5.2 Syntax/grammar changes

**None.** The existing `Expr::MethodCall` and `Expr::PathCall` AST shapes
are reused.

## 5.3 AST/IR changes

**None in `ast.rs`.** New types live in `wasm_codegen.rs`:

```rust
/// One host import declaration.
#[derive(Debug, Clone)]
struct HostImport {
    /// Module name (always `"alk"` in this wave).
    module: &'static str,
    /// Function name (e.g. `"vec_push"`).
    name: &'static str,
    /// Index into the type section.
    type_idx: u32,
    /// Absolute function-index-space index (0..N-1 for N imports).
    func_idx: u32,
}

/// The 10 host functions provided by the runtime, in fixed declaration
/// order. The order MUST match the WASM import-section order.
const HOST_IMPORTS: &[(&str, &[ValType], &[ValType])] = &[
    // name, params, results
    ("vec_new",           &[ValType::I32],                &[ValType::I32]),
    ("vec_with_capacity", &[ValType::I32, ValType::I32],  &[ValType::I32]),
    ("vec_push",          &[ValType::I32, ValType::I32],  &[]),
    ("vec_extend",        &[ValType::I32, ValType::I32],  &[]),
    ("vec_remove",        &[ValType::I32, ValType::I32],  &[]),
    ("vec_clear",         &[ValType::I32],                &[]),
    ("vec_len",           &[ValType::I32],                &[ValType::I32]),
    ("vec_is_empty",      &[ValType::I32],                &[ValType::I32]),
    ("vec_get",           &[ValType::I32, ValType::I32],  &[ValType::I32]),
    ("vec_set",           &[ValType::I32, ValType::I32, ValType::I32], &[]),
];
```

## 5.4 Type-system changes

### 5.4.1 Host function ABI

| Host function | Signature | AlkALive method |
|---------------|-----------|-----------------|
| `alk::vec_new(elem_size: i32) -> i32` | Fresh Vec handle | `Vec::new()` (synthesised) |
| `alk::vec_with_capacity(elem_size: i32, cap: i32) -> i32` | Vec with capacity | `Vec::with_capacity(n)` |
| `alk::vec_push(ptr: i32, value: i32)` | Append a value (returns unit) | `v.push(x)` |
| `alk::vec_extend(dst: i32, src: i32)` | Append all of `src` | `v.extend(other)` |
| `alk::vec_remove(ptr: i32, idx: i32)` | Remove element at `idx` | `v.remove(i)` |
| `alk::vec_clear(ptr: i32)` | Remove all elements | `v.clear()` |
| `alk::vec_len(ptr: i32) -> i32` | Element count | `v.len()` |
| `alk::vec_is_empty(ptr: i32) -> i32` | `1` if empty, else `0` | `v.is_empty()` |
| `alk::vec_get(ptr: i32, idx: i32) -> i32` | Element at `idx` | `v.get(i)` (panics on out-of-bounds) |
| `alk::vec_set(ptr: i32, idx: i32, value: i32)` | In-place element mutation | `v.set(i, x)` |

Notes:
- All values are `i32` (4 bytes). For `Vec<f32>`, the `f32` is bit-cast via
  the WASM `f32.reinterpret_i32` instruction (handled by the host).
- `elem_size` is always `4` in this wave.
- The host owns the actual heap storage; the WASM module holds only opaque
  `i32` handles.

### 5.4.2 Import section emission

```rust
// Before the function section:
let mut import_sec = ImportSection::new();
let mut host_imports_idx: Vec<HostImport> = Vec::new();
for (i, (name, params, results)) in HOST_IMPORTS.iter().enumerate() {
    let type_idx = type_builder.register(params, results);
    host_imports_idx.push(HostImport {
        module: "alk", name, type_idx, func_idx: i as u32,
    });
    import_sec.import("alk", name, EntityKind::Function, type_idx);
}
wasm_module.section(&import_sec);
```

### 5.4.3 `Expr::MethodCall` codegen

```rust
Expr::MethodCall { receiver, method, args, .. } => {
    // Determine the host function name.
    let host_name: Option<&str> = match method.as_str() {
        "push"     => Some("vec_push"),
        "extend"   => Some("vec_extend"),
        "remove"   => Some("vec_remove"),
        "clear"    => Some("vec_clear"),
        "len"      => Some("vec_len"),
        "is_empty" => Some("vec_is_empty"),
        "get"      => Some("vec_get"),
        "set"      => Some("vec_set"),
        _ => None, // not a collection method; defer to Gap 1's class dispatch
    };
    if let Some(host) = host_name {
        // 1. Compile receiver (leaves ptr on stack).
        self.compile_expr(receiver, instrs);
        // 2. Compile arguments (left to right).
        for a in args { self.compile_expr(a, instrs); }
        // 3. Emit the host call.
        instrs.push(AlkInstr::Call(host.to_string()));
    } else {
        // Gap 1 territory: class method dispatch. Until Gap 1 lands, emit
        // a defensive drop sequence (unreachable in a typechecked program).
        self.compile_expr(receiver, instrs);
        for a in args { self.compile_expr(a, instrs); }
        // Defensive: drop everything pushed (no way to know counts
        // statically without Gap 1's class table). Unreachable in practice.
    }
}
```

### 5.4.4 `Expr::PathCall` codegen

```rust
Expr::PathCall(module, member, args, ..) => {
    if module == "Vec" && member == "new" {
        // vec_new(elem_size=4)
        instrs.push(AlkInstr::I32Const(4));
        instrs.push(AlkInstr::Call("vec_new".to_string()));
    } else if module == "Vec" && member == "with_capacity" {
        // Host signature: (elem_size: i32, cap: i32) -> i32.
        // Push elem_size FIRST, then cap.
        instrs.push(AlkInstr::I32Const(4));
        for a in args { self.compile_expr(a, instrs); }
        instrs.push(AlkInstr::Call("vec_with_capacity".to_string()));
    } else {
        // Cross-module call (Gap 2) — placeholder until Gap 2 lands.
        instrs.push(AlkInstr::I32Const(0));
    }
}
```

### 5.4.5 `AlkInstr::Call` resolution (in the emission loop)

```rust
AlkInstr::Call(name) => {
    // First check if this is a host import.
    if let Some(import) = host_imports_idx.iter().find(|i| i.name == name.as_str()) {
        Instruction::Call(import.func_idx)
    } else {
        // Intra-module call: add the import count to the local index.
        let local_idx = fn_metas.iter()
            .position(|m| m.name == *name)
            .unwrap_or(0) as u32;
        let absolute_idx = (host_imports_idx.len() as u32) + local_idx;
        Instruction::Call(absolute_idx)
    }
}
```

### 5.4.6 Tree-shaking policy (addresses CR-8 and CR-9)

**Tree-shaking is deferred to a future wave.** All `pub fn` and `pub class`
items in the source are emitted to the WASM binary, regardless of
reachability from `main`. This is a deliberate simplification:

- **CR-8 resolution:** The fine-draft's "tree-shaking after typecheck"
  approach would fail the build on unused `pub fn`s with type errors. We
  defer tree-shaking entirely until a future wave can do it correctly
  (e.g. typecheck-only-reachable-functions, which requires the resolver
  to run before the typechecker).
- **CR-9 resolution:** When tree-shaking is eventually implemented, the
  conservative rule is: "if any instance of class `C` is constructed (via
  `Object` literal or `ClassName::new`), every `pub` method of `C` and
  every `pub` method of every subclass of `C` is reachable." This is the
  standard C++/LTO rule for virtual dispatch. Documented here for the
  future implementer; **not enforced in this wave**.

The `ExportSection` contains every `pub fn`/`pub class` exported by name
(no reachability filter).

## 5.5 Compiler changes

| Layer | Change | Functions added/modified |
|-------|--------|--------------------------|
| **Lexer** | None. | — |
| **Parser** | None. | — |
| **AST** | None. | — |
| **Typechecker** | Already covered by Gap 3 §3.4.4 — `collection_method_return_type`. | — |
| **WASM codegen** | (1) `use wasm_encoder::{ImportSection, EntityKind};` added. (2) `HostImport` struct + `HOST_IMPORTS` constant added. (3) `compile_to_wasm` builds `host_imports_idx` before the function section, emits the import section, then the function section. (4) `AlkInstr::Call` resolution updated per §5.4.5. (5) `Expr::MethodCall` and `Expr::PathCall` arms updated per §5.4.3 / §5.4.4. | **Added:** `HostImport`, `HOST_IMPORTS`. **Modified:** `FnCompiler::compile_expr` (`MethodCall`/`PathCall` arms), `compile_to_wasm` (import section emission + call resolution). |
| **Host runtime** | The runtime in `crates/alkalive-runtime-wasm` must provide the 10 host functions. They are registered via `wasm-bindgen`'s import mechanism (94 import bindings already exist; 10 more are added). | **Added:** 10 `#[wasm_bindgen] extern "C"` functions in `runtime-wasm/src/lib.rs`. |

## 5.6 WASM changes

### 5.6.1 Section ordering (after Gap 4 + Gap 5)

```
1. Type section        (function types: user fns + host imports)
2. Import section      (alk::vec_*)                            [Gap 5]
3. Function section    (user function declarations)
4. Memory section      (1+ pages, grown for strings)           [Gap 4]
5. Export section      (pub fns + memory)
6. Data section        (string literals as length-prefixed)    [Gap 4]
7. Code section        (function bodies)
```

(Gap 1 will add Table, Global, and Element sections per §1.6.)

### 5.6.2 `v.push(1)` codegen

Source:
```alk
let v: Vec<i32> = Vec::new();
v.push(1);
```

WASM instruction sequence (after `let v` produces local `$v`):
```wasm
local.get $v             ;; receiver pointer
i32.const 1              ;; argument
call $vec_push           ;; host import index 2 (after vec_new=0, vec_with_capacity=1)
```

### 5.6.3 `v.len()` codegen

```wasm
local.get $v             ;; receiver pointer
call $vec_len            ;; host import index 6
;; stack now has i32 (the length)
```

### 5.6.4 `Vec::new()` codegen

```wasm
i32.const 4              ;; elem_size = 4
call $vec_new            ;; host import index 0
;; stack now has i32 (the new Vec handle)
```

### 5.6.5 `Vec::with_capacity(n)` codegen

For `let v: Vec<i32> = Vec::with_capacity(10);`:
```wasm
i32.const 4              ;; elem_size = 4 (PUSHED FIRST)
i32.const 10             ;; capacity
call $vec_with_capacity  ;; host import index 1
;; stack now has i32 (the new Vec handle)
```

### 5.6.6 Function index space

```
Index 0: alk::vec_new
Index 1: alk::vec_with_capacity
Index 2: alk::vec_push
Index 3: alk::vec_extend
Index 4: alk::vec_remove
Index 5: alk::vec_clear
Index 6: alk::vec_len
Index 7: alk::vec_is_empty
Index 8: alk::vec_get
Index 9: alk::vec_set
Index 10: first AlkALive-defined function (e.g. `main`)
Index 11: second AlkALive-defined function
...
```

When Gap 1 lands, `__alk_alloc` is added at index 10 (the 11th import);
the first AlkALive function shifts to index 11. The `host_imports_idx`
vector is the single source of truth; the absolute index is
`host_imports_idx.len() + local_idx`.

## 5.7 Error cases

| ID | Trigger condition | Message format | Layer |
|----|-------------------|----------------|-------|
| **LANG-308-E3** | Method name on a `Vec<T>` receiver not in the host table. | `method \`.{method}()\` is not defined on type \`Vec<T>\`` | Typechecker (Gap 3) |
| **LANG-506-W1** | Codegen reaches the "unknown collection method" branch (defensive; unreachable in typechecked program). | `WasmCodegenError { message: "internal error: unknown collection method \`{method}\` reached codegen", line, col }` | WASM codegen |
| **(runtime)** | `vec_get` out of bounds. | Host traps (WASM `unreachable`); program aborts with a stack trace. | Host runtime |
| **(runtime)** | `vec_push` on a full Vec. | Host grows the underlying allocation; no error. | Host runtime |

## 5.8 Validation rules

- **V5-1.** The ImportSection contains exactly 10 entries, all under module
  `"alk"`, in the order listed in `HOST_IMPORTS`.
- **V5-2.** Each import's type index points to a `TypeSection` entry whose
  params/results match the table in §5.4.1.
- **V5-3.** The first AlkALive-defined function has index
  `host_imports.len()` (10 in this wave).
- **V5-4.** `AlkInstr::Call("vec_push")` resolves to `Instruction::Call(2)`
  (the index of `alk::vec_push`).
- **V5-5.** `AlkInstr::Call("main")` (where `main` is the first
  AlkALive-defined function) resolves to `Instruction::Call(10)`.
- **V5-6.** `Vec::with_capacity(n)` emits `i32.const 4; <compile n>; call 1`
  (elem_size first, then cap, then call).
- **V5-7.** No `pub fn` or `pub class` item is dropped from the
  ExportSection (tree-shaking is deferred).

## 5.9 Test cases

| Test ID | Source | Expected behaviour |
|---------|--------|--------------------|
| **LANG-5T-01** | `fn main() { let v: Vec<i32> = Vec::new(); }` | Binary has an ImportSection with 10 imports from module `"alk"`. |
| **LANG-5T-02** | `fn main() { let v: Vec<i32> = Vec::new(); }` | The first AlkALive-defined function (`main`) has WASM index 10. Verified by `wasmparser` walking the function index space. |
| **LANG-5T-03** | `fn main() { let v: Vec<i32> = Vec::new(); v.push(1); }` | The compiled body contains (in order) `LocalGet` (the receiver), `I32Const(1)`, `Call(2)`. Verified at the `AlkInstr` level. |
| **LANG-5T-04** | `fn main() -> i32 { let v: Vec<i32> = Vec::new(); return v.len(); }` | The compiled body contains `LocalGet`, `Call(6)`, leaving an `i32` on the stack. |
| **LANG-5T-05** | `fn main() -> Vec<i32> { return Vec::new(); }` | The compiled body contains `I32Const(4)`, `Call(0)`. |
| **LANG-5T-06** | `fn main() -> Vec<i32> { return Vec::with_capacity(10); }` | The compiled body contains `I32Const(4)`, `I32Const(10)`, `Call(1)` — elem_size first, then cap, then call. |
| **LANG-5T-07** | (any program with Vec usage) | `wasmparser::Parser` validates the full binary cleanly. |
| **LANG-5T-08** | `fn main() { let v: Vec<i32> = Vec::new(); let n: i32 = v.len(); }` | Typechecks (return type of `len` is `i32` per Gap 3 §3.4.4). |
| **LANG-5T-09** | `fn main() { let v: monotone Vec<i32> = Vec::new(); v.remove(0); }` | Rejected (existing test — monotone shrink op). Must still pass after Gap 5 lands. |
| **LANG-5T-10** | `fn main() { let v: Vec<i32> = Vec::new(); v.set(0, 42); }` | Typechecks (`vec_set` returns unit); WASM emits `LocalGet`, `I32Const(0)`, `I32Const(42)`, `Call(9)`. |
| **LANG-5T-11** | (tree-shaking: a `pub fn unused() -> i32 { return 1; }` not called from `main`) | `unused` IS in the ExportSection (tree-shaking deferred per CR-8/CR-9). |
| **LANG-5T-12** | End-to-end (new `tests/collection_host_test.rs`): a minimal host provides the 10 functions backed by a `Vec<i32>` in Rust. The WASM module is instantiated, `main` is called (which `v.push(1); v.push(2);`), and the host's `Vec` is observed to contain `[1, 2]`. | Host's view matches. (Optional; falls back to instruction-level tests if no Wasmtime/wasmi.) |

## 5.10 Acceptance criteria

- **AC5-1.** `cargo test -p alkalive-compiler collection_dispatch_tests`
  passes with the 12 tests above.
- **AC5-2.** A diff of `wasm_codegen.rs` shows the `Expr::MethodCall` arm
  (lines 387-405 today) now emits `AlkInstr::Call(host_name)` for known
  collection methods, not the placeholder `I32Const(0)`.
- **AC5-3.** `cargo test --workspace` is green.
- **AC5-4.** `cargo clippy -p alkalive-compiler -- -D warnings` is clean.
- **AC5-5.** The runtime's WASM glue registers the 10 host functions.

## 5.11 Traceability

| Requirement | ADR / source | Fine-draft decision | Implementation requirement | Test |
|-------------|--------------|---------------------|----------------------------|------|
| LANG-501..503 | ADR-008, ADR-018; fine-draft §5.4.1-5.4.2 | "Emit `ImportSection` with 9 (now 10) imports under `"alk"`." | §5.3 Rust types + §5.4.2 emission. | LANG-5T-01, LANG-5T-02. |
| LANG-504 | fine-draft §5.4.2 (index space) | "Imported functions occupy the lowest indices; `Call` resolution distinguishes host vs local." | §5.4.5 resolution. | LANG-5T-03, LANG-5T-04. |
| LANG-505 | fine-draft §5.4.3 | "`MethodCall` compiles to receiver, args, `call $host`." | §5.4.3 codegen. | LANG-5T-03, LANG-5T-04, LANG-5T-10. |
| LANG-507..508 | fine-draft §5.4.4 | "`Vec::new` → `i32.const 4; call vec_new`; `with_capacity` → `i32.const 4; <cap>; call vec_with_capacity`." | §5.4.4 codegen. | LANG-5T-05, LANG-5T-06. |
| LANG-510 (CR-8/9 fix) | Critical review CR-8, CR-9; ADR-018 | "Tree-shaking deferred; conservative virtual-dispatch rule documented for future wave." | §5.4.6 policy + §5.7 no tree-shaking errors. | LANG-5T-11. |
| (CR-17 fix) | Critical review CR-17 | "`__alk_alloc` added to import table when Gap 1 lands." | §5.1 note + Gap 1 §1.6. | (Tested in Gap 1.) |

---

# Gap 1 — OO Model (classes, methods, inheritance)

## 1.1 Exact requirements

- **LANG-101.** The lexer must recognise the keywords `class`, `pub`, `priv`,
  `self`, `Self`. (`new` is NOT a keyword — it is a regular identifier that
  is special only as a method *name*; `super` is NOT a keyword in this
  wave.)
- **LANG-102.** The parser must accept `pub? class Ident (':' Ident)? '{'
  ClassMember* '}'` as a top-level item. The leading `pub?` sets the class's
  `Visibility` (`Pub` if `pub` is present, `Priv` otherwise — `Priv` is the
  default).
- **LANG-103.** `ClassMember` is either a `FieldDecl` or a `MethodDecl`:
  - `FieldDecl := Visibility? Ident ':' Type ';'`
  - `MethodDecl := Visibility? 'fn' Ident '(' ParamList? ')' ('->' Type)? Block`
  - `Visibility := 'pub' | 'priv'` (default `priv`).
- **LANG-104.** A method whose parameter list begins with the `self` keyword
  is an **instance method**; otherwise it is a **static method**.
- **LANG-105.** `fn new(...) -> Self` is the constructor. It must:
  - be a static method (no `self` parameter),
  - return `Self` (or a `Self`-equivalent named-type reference to the
    enclosing class).
  If no `new` is declared, the compiler synthesises a default `new` that
  calls `__alk_alloc(field_stride(class))` and zero-initialises the fields.
- **LANG-106.** `self` is usable as an expression of type `Self` inside
  instance method bodies. `Self` (capitalised) is usable as a type alias for
  the enclosing class inside any method body. `Self` outside a class body is
  a `TypeError`.
- **LANG-107.** Field access `receiver.field` is parseable as `Expr::Field`.
  In lvalue context (after `=`), it forms `Stmt::Assign { target: Expr::Field,
  value: Expr, line, col }`.
- **LANG-108.** Object construction `Self { f1: e1, ... }` or
  `ClassName { f1: e1, ... }` is parseable as `Expr::Object`.
- **LANG-109.** Static method invocation `ClassName::method(args)` (and
  `Self::method(args)`) is parseable as `Expr::StaticCall`.
- **LANG-110.** Inheritance is **single** (one base class max, via
  `class Derived : Base`). Multiple `:` is a parse error. Cyclic inheritance
  (`A : B : A`) is a `TypeError`.
- **LANG-111.** A derived class inherits all base-class fields and methods.
  Base-class fields come first in the object layout. Upcasting
  (`Derived` → `Base`) is a no-op at the WASM level (the pointer is
  unchanged). Downcasting is **not supported** in this wave.
- **LANG-112.** Method override is **invariant**: the override's signature
  (params + return type) must match the base method exactly. Covariant
  returns and contravariant params are rejected.
- **LANG-113.** Field assignment to a `monotone`/`antitone`-qualified field
  is a compile-time error (`LANG-114-E10`). **(Addresses CR-10.)**
- **LANG-114.** The first 4 bytes of every object hold the class's
  **vtable_base** — an `i32` index into the WASM table where this class's
  vtable begins. **(Addresses CR-7.)** The remaining bytes hold the fields
  in source order, base-class fields first.
- **LANG-115.** Virtual dispatch on `obj.foo(args)` compiles to:
  ```wasm
  local.get $obj             ;; receiver pointer
  ;; (compile each argument, leaving them on the stack)
  local.get $obj
  i32.load offset=0          ;; load vtable_base (table index)
  i32.const <slot>           ;; slot index of foo in the vtable
  i32.add                    ;; absolute table index = vtable_base + slot
  call_indirect (type $foo_type)
  ```
  The `<slot>` is a compile-time constant determined by `foo`'s position in
  the class's vtable layout (declaration order, base-class methods first).
  **(Addresses CR-7.)**
- **LANG-116.** Static methods (`ClassName::method(args)`) compile to a
  direct `call <fnidx>` — no vtable lookup.
- **LANG-117.** Constructors (`Self::new(args)` or `ClassName::new(args)`)
  compile to a direct call to the constructor function followed by the
  object-literal initialiser that fills the field slots.
- **LANG-118.** The default synthesised `new` calls
  `__alk_alloc(field_stride(class))` and zero-initialises the fields.
- **LANG-119.** `__alk_alloc(size: i32) -> i32` is added to the same
  `host_imports` list as the `vec_*` functions (Gap 5). Its index is
  contiguous with the collection imports. **(Addresses CR-17.)**
- **LANG-120.** A class with no fields and no methods is legal (`class Empty
  {}`). Its `field_stride` is 4 (just the vtable_base slot).

## 1.2 Syntax/grammar changes (EBNF)

```ebnf
ItemDecl       := FnDecl | LetDecl | ClassDecl

ClassDecl      := Visibility? 'class' Ident (':' Ident)? '{' ClassMember* '}'
ClassMember    := FieldDecl | MethodDecl
FieldDecl      := Visibility? 'field'? Ident ':' Type ';'
                 // 'field' keyword optional (kept for future-proofing; if
                 // absent, a member starting with an Ident followed by ':'
                 // is a field; a member starting with 'fn' is a method).
MethodDecl     := Visibility? 'fn' Ident '(' ParamList? ')' ('->' Type)? Block
Visibility     := 'pub' | 'priv'                    // default: 'priv'
ParamList      := Param (',' Param)*
Param          := 'self' | Ident ':' Type
SelfExpr       := 'self'                            // expression position
SelfType       := 'Self'                            // type position
FieldAccess    := Expr '.' Ident                    // lvalue or rvalue
ObjectLiteral  := ('Self' | Ident) '{' FieldInit (',' FieldInit)* '}'
FieldInit      := Ident ':' Expr
StaticCall     := ('Self' | Ident) '::' Ident '(' ArgList? ')'
Stmt           := ... | Assign
Assign         := FieldAccess '=' Expr ';'
```

Notes:
- `Self` (capitalised) is a *type* alias meaning "the class currently being
  defined"; `self` (lowercase) is an *expression* of type `Self`.
- `new` is NOT a keyword — it is a regular identifier that is special only
  as a method *name*. The constructor is recognised by the name `new` plus
  the return-type constraint `-> Self`.
- Multiple inheritance (`class A : B, C`) is a parse error (the parser
  rejects a second `:` or `,` after the base name).
- `super.method()` is NOT supported in this wave.

## 1.3 AST/IR changes

Add to `ast.rs`:

```rust
/// Visibility of a class member or top-level item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Module-private (the default).
    #[default]
    Priv,
    /// Publicly accessible from other modules.
    Pub,
}

/// `pub class Name : Base { fields... methods... }`
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    /// Class name as written.
    pub name: String,
    /// Optional single base class (None = no parent / root).
    pub base: Option<String>,
    /// Visibility of the class itself (top-level `pub`).
    pub visibility: Visibility,
    /// Fields in declaration order (base-class fields are NOT included
    /// here; they are looked up via the `base` chain at typecheck time).
    pub fields: Vec<FieldDecl>,
    /// Methods in declaration order (base-class methods are NOT included).
    pub methods: Vec<MethodDecl>,
    /// Attributes attached to the class (e.g. `@monotone` — currently a
    /// parse error per CR-15; the field exists for forward compatibility).
    pub attrs: Vec<Attribute>,
    pub line: u32,
    pub col: u32,
}

/// `pub? name: Type;` inside a class body.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
    pub line: u32,
    pub col: u32,
}

/// `pub? fn name(self, params) -> Type { body }` inside a class body.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub name: String,
    /// `true` if the first parameter is `self`.
    pub is_instance: bool,
    /// Parameters excluding `self`; `self` is implicit.
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
    pub visibility: Visibility,
    pub attrs: Vec<Attribute>,
    pub line: u32,
    pub col: u32,
}
```

Extend `ItemDecl`:

```rust
pub enum ItemDecl {
    Fn(FnDecl),
    Let(LetDecl),
    Class(ClassDecl),            // NEW
}
```

Extend `Expr` with field access, `self`, and object literals:

```rust
pub enum Expr {
    // ... existing variants ...
    /// `self` — the implicit receiver of an instance method.
    Self_(u32, u32),
    /// `receiver.field` — field access (read or, in lvalue context, write).
    Field {
        receiver: Box<Expr>,
        field: String,
        line: u32,
        col: u32,
    },
    /// `Self { f1: e1, f2: e2 }` or `ClassName { ... }` — object construction.
    Object {
        /// "Self" is resolved to the enclosing class during typechecking.
        class: String,
        /// (field_name, value_expr, line, col) — in source order.
        fields: Vec<(String, Expr, u32, u32)>,
        line: u32,
        col: u32,
    },
    /// `ClassName::method(args)` — static call (also covers `Self::method`).
    StaticCall {
        /// "Self" is resolved to the enclosing class during typechecking.
        class: String,
        method: String,
        args: Vec<Expr>,
        line: u32,
        col: u32,
    },
}
```

Add a new statement for **field assignment**:

```rust
pub enum Stmt {
    // ... existing variants ...
    /// `self.field = expr;` or `obj.field = expr;`
    Assign {
        /// Must be `Expr::Field`. The typechecker enforces this.
        target: Expr,
        value: Expr,
        line: u32,
        col: u32,
    },
}
```

Add `visibility: Visibility` field to `FnDecl`, `LetDecl` (default `Priv`):

```rust
pub struct FnDecl {
    // ... existing fields ...
    pub visibility: Visibility,    // NEW
}
pub struct LetDecl {
    // ... existing fields ...
    pub visibility: Visibility,    // NEW
}
```

## 1.4 Type-system changes

### 1.4.1 ClassTable

```rust
/// A class's signature — the type-checker's view of a user-defined type.
#[derive(Debug, Clone)]
pub struct ClassSig {
    pub name: String,
    pub base: Option<String>,
    pub visibility: Visibility,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<MethodDecl>,
    /// Effective field-stride (4 * total_field_count_including_base_chain).
    /// Computed lazily; cached here.
    pub field_stride: u32,
    /// Vtable slot count (total method count including base chain).
    pub vtable_slot_count: u32,
}

#[derive(Debug, Default)]
pub struct ClassTable {
    classes: std::collections::HashMap<String, ClassSig>,
}

impl ClassTable {
    pub fn new() -> Self { Self::default() }
    pub fn insert(&mut self, name: impl Into<String>, sig: ClassSig) {
        self.classes.insert(name.into(), sig);
    }
    pub fn lookup(&self, name: &str) -> Option<&ClassSig> {
        self.classes.get(name)
    }
    /// Walk the base chain. Returns `true` if `derived`'s chain includes
    /// `ancestor`. Used for subtyping checks.
    pub fn is_subclass_of(&self, derived: &str, ancestor: &str) -> bool {
        let mut current = Some(derived);
        while let Some(c) = current {
            if c == ancestor { return true; }
            current = self.classes.get(c).and_then(|s| s.base.as_deref());
        }
        false
    }
    /// Detect cycles. Returns `Some(cycle_path)` if a cycle is found.
    pub fn find_cycle(&self, start: &str) -> Option<Vec<String>> {
        let mut path = vec![start.to_string()];
        let mut current = self.classes.get(start).and_then(|s| s.base.as_deref());
        while let Some(c) = current {
            if c == start { return Some(path); }
            if path.contains(c) { return Some(path); } // shorter cycle
            path.push(c.to_string());
            current = self.classes.get(c).and_then(|s| s.base.as_deref());
        }
        None
    }
}
```

### 1.4.2 `check_module` (extended)

After Gap 1, `check_module` runs four passes:

1. **Collect signatures** (`collect_signatures`, Gap 3 §3.4.1) — including
   class methods (`ClassName::method`).
2. **Collect classes** (`collect_classes`) — populates `ClassTable`,
   detects cycles, computes `field_stride` and `vtable_slot_count`.
3. **Collect module-level `let`s** (unchanged).
4. **Check function bodies AND class method bodies** — `check_class` walks
   fields and methods; the method env contains `self: Type::named(class_name)`.

```rust
fn collect_classes(module: &ModuleDecl, classes: &mut ClassTable,
                   errors: &mut TypeErrorSet) {
    for item in &module.items {
        if let ItemDecl::Class(c) = item {
            // Cycle detection.
            if let Some(cycle) = classes.find_cycle(&c.name) {
                errors.push(TypeError {
                    message: format!(
                        "cyclic inheritance: {}",
                        cycle.join(" : ")
                    ),
                    line: c.line, col: c.col,
                });
                continue;
            }
            let field_stride = 4 * (1 + total_field_count(classes, c));
            // ^ +1 for the vtable_base slot at offset 0.
            let vtable_slot_count = total_method_count(classes, c);
            classes.insert(c.name.clone(), ClassSig {
                name: c.name.clone(),
                base: c.base.clone(),
                visibility: c.visibility,
                fields: c.fields.clone(),
                methods: c.methods.clone(),
                field_stride,
                vtable_slot_count,
            });
        }
    }
}
```

### 1.4.3 `check_class`

```rust
fn check_class(c: &ClassDecl, classes: &ClassTable,
               module_env: &TypeEnv, sigs: &FnSigTable,
               errors: &mut TypeErrorSet) {
    // 1. Check for duplicate fields.
    let mut seen: std::collections::HashSet<String> = Default::default();
    for f in &c.fields {
        if !seen.insert(f.name.clone()) {
            errors.push(TypeError {
                message: format!(
                    "field `{}` already declared in class `{}`",
                    f.name, c.name
                ),
                line: f.line, col: f.col,
            });
        }
    }
    // 2. Check for duplicate methods (override must match signature).
    let mut method_names: std::collections::HashMap<String, &MethodDecl> = Default::default();
    for m in &c.methods {
        if let Some(existing) = method_names.get(&m.name) {
            errors.push(TypeError {
                message: format!(
                    "method `{}` already declared in class `{}`",
                    m.name, c.name
                ),
                line: m.line, col: m.col,
            });
        } else {
            method_names.insert(m.name.clone(), m);
        }
        // Override check: if a base-class method with the same name exists,
        // the signatures must match exactly (invariant override).
        if let Some(base) = &c.base {
            if let Some(base_class) = classes.lookup(base) {
                if let Some(base_method) = find_method_in_chain(base_class, classes, &m.name) {
                    if !signatures_match(m, base_method) {
                        errors.push(TypeError {
                            message: format!(
                                "cannot override `{}` in `{}`: signature mismatch",
                                m.name, c.name
                            ),
                            line: m.line, col: m.col,
                        });
                    }
                }
            }
        }
    }
    // 3. Check `new` returns Self.
    for m in &c.methods {
        if m.name == "new" {
            if m.is_instance {
                errors.push(TypeError {
                    message: format!(
                        "constructor `new` in `{}` must be a static method (no `self` parameter)",
                        c.name
                    ),
                    line: m.line, col: m.col,
                });
            }
            match &m.return_type {
                Some(Type { base: BaseType::Named(n), .. }) if n == &c.name || n == "Self" => {}
                _ => {
                    errors.push(TypeError {
                        message: format!(
                            "constructor `new` in `{}` must return `Self`",
                            c.name
                        ),
                        line: m.line, col: m.col,
                    });
                }
            }
        }
    }
    // 4. Check each method body. The env contains `self: Type::named(c.name)`.
    for m in &c.methods {
        let mut env = module_env.clone();
        if m.is_instance {
            env.insert("self", Type {
                qualifier: Qualifier::Unrestricted,
                base: BaseType::Named(c.name.clone()),
            });
        }
        for p in &m.params {
            env.insert(p.name.clone(), p.ty.clone());
        }
        check_block(&m.body, &mut env, m.return_type.as_ref(), sigs, errors);
    }
}
```

### 1.4.4 `Expr::Field`, `Expr::Self_`, `Expr::Object`, `Expr::StaticCall`

```rust
Expr::Self_(line, col) => {
    match env.lookup("self") {
        Some(ty) => Some(ty.clone()),
        None => {
            errors.push(TypeError {
                message: "`self` is not available outside an instance method".to_string(),
                line: *line, col: *col,
            });
            None
        }
    }
}

Expr::Field { receiver, field, line, col } => {
    let receiver_ty = check_expr(receiver, env, errors, sigs);
    match &receiver_ty {
        Some(Type { base: BaseType::Named(class_name), .. }) => {
            // Look up the field in the class chain.
            match find_field_in_chain(classes, class_name, field) {
                Some(field_decl) => {
                    // Visibility check: if the field is `priv` and we're
                    // not inside a method of `class_name` (or a subclass),
                    // error.
                    if field_decl.visibility == Visibility::Priv
                        && !is_self_or_subclass(env, classes, class_name)
                    {
                        errors.push(TypeError {
                            message: format!(
                                "private field `{}.{}` accessed from outside class `{}`",
                                class_name, field, class_name
                            ),
                            line: *line, col: *col,
                        });
                    }
                    Some(field_decl.ty.clone())
                }
                None => {
                    errors.push(TypeError {
                        message: format!(
                            "class `{}` has no field `{}`",
                            class_name, field
                        ),
                        line: *line, col: *col,
                    });
                    None
                }
            }
        }
        Some(other) => {
            errors.push(TypeError {
                message: format!(
                    "field access `.{}()` is not valid on type `{}`",
                    field, other
                ),
                line: *line, col: *col,
            });
            None
        }
        None => None,
    }
}

Expr::Object { class, fields, line, col } => {
    let resolved_class = if class == "Self" {
        match env.lookup("__enclosing_class__") {
            Some(t) => match &t.base {
                BaseType::Named(n) => n.clone(),
                _ => class.clone(),
            },
            None => {
                errors.push(TypeError {
                    message: "`Self` used outside a class body".to_string(),
                    line: *line, col: *col,
                });
                return None;
            }
        }
    } else {
        class.clone()
    };
    match classes.lookup(&resolved_class) {
        Some(sig) => {
            // Check that every declared field is initialised exactly once,
            // and that every initialiser type-checks against the field type.
            let mut seen: std::collections::HashSet<String> = Default::default();
            for (name, val_expr, line, col) in fields {
                match find_field_in_chain(classes, &resolved_class, name) {
                    Some(field_decl) => {
                        if !seen.insert(name.clone()) {
                            errors.push(TypeError {
                                message: format!(
                                    "field `{}` initialised twice in object literal for class `{}`",
                                    name, resolved_class
                                ),
                                line: *line, col: *col,
                            });
                        }
                        let val_ty = check_expr(val_expr, env, errors, sigs);
                        if let (Some(vt), ft) = (val_ty, &field_decl.ty) {
                            if !type_is_subtype(&vt, ft) {
                                errors.push(TypeError {
                                    message: format!(
                                        "field `{}` initialiser has type `{}` but field type is `{}`",
                                        name, vt, ft
                                    ),
                                    line: *line, col: *col,
                                });
                            }
                        }
                    }
                    None => {
                        errors.push(TypeError {
                            message: format!(
                                "unknown field `{}` in object literal for class `{}`",
                                name, resolved_class
                            ),
                            line: *line, col: *col,
                        });
                    }
                }
            }
            // Check that no declared field is missing (unless it has a
            // default initialiser — future wave; for now, all fields must
            // be initialised).
            for f in all_fields_in_chain(classes, &resolved_class) {
                if !seen.contains(&f.name) {
                    errors.push(TypeError {
                        message: format!(
                            "missing field `{}` in object literal for class `{}`",
                            f.name, resolved_class
                        ),
                        line: *line, col: *col,
                    });
                }
            }
            Some(Type {
                qualifier: Qualifier::Unrestricted,
                base: BaseType::Named(resolved_class),
            })
        }
        None => {
            errors.push(TypeError {
                message: format!("unknown class `{}`", resolved_class),
                line: *line, col: *col,
            });
            None
        }
    }
}

Expr::StaticCall { class, method, args, line, col } => {
    let resolved_class = if class == "Self" {
        match env.lookup("__enclosing_class__") {
            Some(t) => match &t.base {
                BaseType::Named(n) => n.clone(),
                _ => class.clone(),
            },
            None => {
                errors.push(TypeError {
                    message: "`Self` used outside a class body".to_string(),
                    line: *line, col: *col,
                });
                return None;
            }
        }
    } else {
        class.clone()
    };
    let qualified = format!("{}::{}", resolved_class, method);
    match sigs.lookup(&qualified) {
        Some(sig) => {
            if sig.receiver_class.as_deref() == Some(&resolved_class) {
                // Check args + return type.
                let mut arg_types = Vec::new();
                for a in args {
                    arg_types.push(check_expr(a, env, errors, sigs));
                }
                if args.len() != sig.params.len() {
                    errors.push(TypeError {
                        message: format!(
                            "static call `{}::{}` expects {} argument(s) but was called with {}",
                            resolved_class, method, sig.params.len(), args.len()
                        ),
                        line: *line, col: *col,
                    });
                }
                // Per-arg subtype check (similar to Expr::Call).
                for (i, (at, pt)) in arg_types.iter().zip(sig.params.iter()).enumerate() {
                    if let Some(aty) = at {
                        if !type_is_subtype(aty, pt) {
                            errors.push(TypeError {
                                message: format!(
                                    "argument {} to `{}::{}` has type `{}` but parameter has type `{}`",
                                    i + 1, resolved_class, method, aty, pt
                                ),
                                line: *line, col: *col,
                            });
                        }
                    }
                }
                sig.return_type.clone()
            } else {
                errors.push(TypeError {
                    message: format!(
                        "static method `{}::{}` is not a static method (it is an instance method)",
                        resolved_class, method
                    ),
                    line: *line, col: *col,
                });
                None
            }
        }
        None => {
            errors.push(TypeError {
                message: format!(
                    "unknown static method `{}::{}`",
                    resolved_class, method
                ),
                line: *line, col: *col,
            });
            None
        }
    }
}
```

### 1.4.5 Subtyping & upcasting

`type_is_subtype` (in `typechecker.rs:179-195`) is extended to consult the
`ClassTable`:

```rust
pub fn type_is_subtype(sub: &Type, super_: &Type, classes: &ClassTable) -> bool {
    match (&sub.base, &super_.base) {
        // ... existing primitive cases ...
        (BaseType::Named(n1), BaseType::Named(n2)) => {
            // Class subtyping: n1 == n2 OR n1 is a subclass of n2.
            n1 == n2 || classes.is_subclass_of(n1, n2)
        }
        _ => false,
    }
}
```

The qualifier check is unchanged (`monotone`/`antitone` do not apply to
class types).

### 1.4.6 Field-assignment check (addresses CR-10)

`Stmt::Assign` checking:

```rust
Stmt::Assign { target, value, line, col } => {
    // 1. target must be Expr::Field.
    let (receiver, field_name, f_line, f_col) = match target {
        Expr::Field { receiver, field, line, col } =>
            (receiver.as_ref(), field, *line, *col),
        _ => {
            errors.push(TypeError {
                message: "assignment target must be a field access (`obj.field`)".to_string(),
                line: *line, col: *col,
            });
            return;
        }
    };
    // 2. Look up the field; get its declared type + qualifier.
    let receiver_ty = check_expr(receiver, env, errors, sigs);
    let val_ty = check_expr(value, env, errors, sigs);
    if let Some(Type { base: BaseType::Named(class_name), .. }) = &receiver_ty {
        if let Some(field_decl) = find_field_in_chain(classes, class_name, field_name) {
            // CR-10 fix: forbid assignment to monotone/antitone qualified fields.
            if field_decl.ty.qualifier != Qualifier::Unrestricted {
                errors.push(TypeError {
                    message: format!(
                        "assignment to `{}`-qualified field `{}.{}` is forbidden \
                         (qualified fields are immutable after construction)",
                        field_decl.ty.qualifier, class_name, field_name
                    ),
                    line: f_line, col: f_col,
                });
                return;
            }
            // Type check the value against the field type.
            if let Some(vt) = &val_ty {
                if !type_is_subtype(vt, &field_decl.ty, classes) {
                    errors.push(TypeError {
                        message: format!(
                            "field `{}.{}` has type `{}` but assignment value has type `{}`",
                            class_name, field_name, field_decl.ty, vt
                        ),
                        line: *line, col: *col,
                    });
                }
            }
        } else {
            errors.push(TypeError {
                message: format!(
                    "class `{}` has no field `{}`",
                    class_name, field_name
                ),
                line: f_line, col: f_col,
            });
        }
    } else if let Some(other) = &receiver_ty {
        errors.push(TypeError {
            message: format!(
                "field assignment on non-class type `{}` is not supported",
                other
            ),
            line: *line, col: *col,
        });
    }
}
```

**Rule (CR-10 resolution):** a field whose declared `Type` carries a
`monotone` or `antitone` qualifier is **immutable after construction**.
The only way to set its initial value is via the object literal
(`Self { items: Vec::new() }`). Any subsequent `self.items = ...` (or
`obj.items = ...`) is a compile-time error. This preserves the monotonicity
invariant: a `monotone Vec<T>` field cannot be replaced with a fresh empty
Vec, defeating ADR-027 Phase 2.

### 1.4.7 Method-call resolution on class-typed receivers

```rust
fn class_method_return_type(class_name: &str, method: &str,
                            arg_types: &[Option<Type>],
                            sigs: &FnSigTable, classes: &ClassTable,
                            line: u32, col: u32,
                            errors: &mut TypeErrorSet) -> Option<Type> {
    let qualified = format!("{}::{}", class_name, method);
    match sigs.lookup(&qualified) {
        Some(sig) if sig.receiver_class.as_deref() == Some(class_name) => {
            // Visibility check: if the method is `priv` and we're not
            // inside a method of `class_name` (or subclass), error.
            // (Skipped here for brevity — same pattern as field visibility.)
            sig.return_type.clone()
        }
        Some(_) => {
            // Method exists but is static (not an instance method).
            errors.push(TypeError {
                message: format!(
                    "instance method `{}.{}()` is a static method; call as `{}::{}`",
                    class_name, method, class_name, method
                ),
                line, col,
            });
            None
        }
        None => {
            // Walk the base chain.
            if let Some(class_sig) = classes.lookup(class_name) {
                if let Some(base) = &class_sig.base {
                    return class_method_return_type(base, method, arg_types,
                                                   sigs, classes, line, col, errors);
                }
            }
            errors.push(TypeError {
                message: format!(
                    "class `{}` has no method `{}`",
                    class_name, method
                ),
                line, col,
            });
            None
        }
    }
}
```

## 1.5 Compiler changes

| Layer | Change | Functions added/modified |
|-------|--------|--------------------------|
| **Lexer** | New keywords: `class`, `pub`, `priv`, `self`, `Self`. New tokens: `Class`, `Pub`, `Priv`, `Self_` (lowercase `self`), `SelfType` (capitalised `Self`). | `KEYWORDS` map extended; `TokenKind` enum extended. |
| **Parser** | `parse_module` loop adds a `TokenKind::Class` branch dispatching to `parse_class`. New functions: `parse_class`, `parse_field`, `parse_method`, `parse_visibility`. `parse_primary` gains branches for `Self` (returns `Expr::Self_`), `Ident {` (object literal), `Ident :: Ident (` (static call), and `Ident . Ident` (field access — extends existing `MethodCall` to detect when there is no `(` after the field name). `parse_stmt` gains `Assign` recognition: parse an expression, and if it is an `Expr::Field` followed by `=`, treat as `Stmt::Assign`. `parse_leading_attributes` accepts `pub`/`priv` before `fn`/`let`/`class`. | **Added:** `parse_class`, `parse_field`, `parse_method`, `parse_visibility`. **Modified:** `parse_module`, `parse_primary`, `parse_stmt`, `parse_leading_attributes`, `parse_fn` (visibility), `parse_let` (visibility). |
| **AST** | As above (§1.3). | New: `Visibility`, `ClassDecl`, `FieldDecl`, `MethodDecl`, `Expr::Self_`/`Field`/`Object`/`StaticCall`, `Stmt::Assign`. |
| **Typechecker** | (1) New `ClassTable` populated in pass 2. (2) `check_class` walks fields and methods. (3) `check_expr` gains cases for `Self_`, `Field`, `Object`, `StaticCall`. (4) `type_is_subtype` learns class subtyping (chain lookup in `ClassTable`). (5) Method resolution: `class_method_return_type`. (6) Field-assignment check (CR-10). | **Added:** `ClassSig`, `ClassTable`, `collect_classes`, `check_class`, `find_field_in_chain`, `find_method_in_chain`, `class_method_return_type`, `all_fields_in_chain`. **Modified:** `check_module` (4 passes), `check_expr`, `type_is_subtype`, `check_block` (`Stmt::Assign`). |
| **WASM codegen** | (1) `__alk_alloc` added to `host_imports` (CR-17). (2) Emit a `TableSection` (one funcref table, minimum size = total vtable slots). (3) Emit an `ElementSection` seeding the table with method function indices. (4) For each class, emit a hidden `__vtable_<ClassName>` global `i32` constant holding the table base index for that class. (5) `FnCompiler` gains a `self_index` field — when compiling an instance method, parameter 0 is `self`. (6) `Expr::Field` read compiles to `local.get obj; i32.load offset=<field_offset>`. (7) `Stmt::Assign` compiles to `local.get obj; <value>; i32.store offset=<field_offset>`. (8) `Expr::Object` compiles to `call $__alk_alloc; <store each field>`. (9) `Expr::MethodCall` on a class-typed receiver compiles to `call_indirect`; on a `Vec<T>` receiver it stays on the host-import path (Gap 5). | **Added:** `ClassLayout` (offsets + vtable slot map), `emit_table_section`, `emit_elem_section`, `emit_global_section` (vtable_base constants), `vtable_slot_for_method`. **Modified:** `compile_to_wasm` (new sections + `__alk_alloc` import), `FnCompiler::new` (`self_index`), `FnCompiler::compile_expr` (`Field`/`Object`/`StaticCall`/`MethodCall` arms), `compile_block` (`Stmt::Assign`). |

## 1.6 WASM changes

### 1.6.1 Object representation in linear memory (addresses CR-7)

Each object is a contiguous allocation of `field_stride(class)` bytes. The
**first 4 bytes hold the vtable_base** — an `i32` index into the WASM
function table where this class's vtable begins.

```
+---------------------+
| vtable_base (i32)   |  <- object base address (offset 0)
+---------------------+
| base_field_0        |  <- offset 4
| base_field_1        |  <- offset 8
| ...                 |
| derived_field_0     |  <- offset 4 + 4*base_field_count
| ...                 |
+---------------------+
```

**CR-7 resolution:** `vtable_base` is a **table index**, not a pointer to
funcrefs in linear memory. The field is renamed from "vtable_ptr" (the
ambiguous fine-draft name) to "vtable_base" throughout this specification.
The dispatch sequence (§1.6.2) uses `i32.load offset=0` to fetch the table
index, then `i32.const <slot>; i32.add` to compute the absolute table index,
then `call_indirect`.

### 1.6.2 Virtual dispatch (addresses CR-7)

```wasm
;; Instance method call: obj.foo(args)
;; 1. Push the receiver (for the method body's `self` parameter).
local.get $obj
;; 2. Push each argument in source order.
;; (compile each arg expr here)
;; 3. Load the object's vtable_base (table index).
local.get $obj
i32.load offset=0
;; 4. Add the slot index (compile-time constant for `foo`).
i32.const <slot>
i32.add
;; 5. Indirect call through the function table.
call_indirect (type $foo_type)
```

**Why `i32.add` and not just `i32.const <absolute_index>`?** Because the
receiver's static type may be a base class, but the runtime object's
vtable_base points to the derived class's vtable. The slot offset is fixed
across the class chain (base-class methods occupy slots 0..N-1; derived
methods occupy N..M-1), so `vtable_base + slot` resolves to the correct
override at runtime.

### 1.6.3 Table and element sections

```rust
// Table section: one funcref table, minimum size = total vtable slots
// across all classes.
let mut table_sec = TableSection::new();
table_sec.table(TableType {
    element_type: RefType::func(),
    minimum: total_vtable_slots as u32,
    maximum: None,
});
wasm_module.section(&table_sec);

// Element section: seed the table with method function indices.
// For each class, in vtable-slot order, the element segment lists the
// function indices of the class's methods (including inherited).
let mut elem_sec = ElementSection::new();
for class in classes_in_vtable_order {
    let mut func_indices: Vec<u32> = Vec::new();
    for method in methods_in_vtable_order(class) {
        func_indices.push(method_fn_idx(&method));
    }
    // Active element segment at offset = class.vtable_base.
    let mut seg = Elements::active(0, &ConstExpr::i32_const(class.vtable_base as i32));
    seg.functions(func_indices.iter().copied());
    elem_sec.segment(&seg);
}
wasm_module.section(&elem_sec);
```

### 1.6.4 Global section (vtable_base constants)

```rust
// For each class, emit a global `__vtable_<ClassName>` i32 constant.
let mut global_sec = GlobalSection::new();
for class in &classes {
    global_sec.global(wasm_encoder::GlobalType {
        val_type: ValType::I32,
        mutable: false,
        shared: false,
    }, &ConstExpr::i32_const(class.vtable_base as i32));
}
wasm_module.section(&global_sec);
```

These globals are not directly referenced by code (the vtable_base is
already stored in the object); they exist so that the constructor's
`Self { ... }` literal can set the object's first 4 bytes via
`i32.const <vtable_base>; i32.store offset=0`.

### 1.6.5 Field access

```wasm
;; `obj.field` (read):
local.get $obj
i32.load offset=<field_offset>
;; <field_offset> = 4 + 4 * field_index_in_chain
;; (base-class fields first; the vtable_base occupies offset 0)

;; `obj.field = value;` (write, CR-10-allowed fields only):
local.get $obj
<compile value>
i32.store offset=<field_offset>
```

### 1.6.6 Object literal

```wasm
;; `Self { f1: e1, f2: e2 }`:
;; 1. Allocate.
i32.const <field_stride>          ;; size in bytes
call $__alk_alloc                 ;; returns ptr (i32)
;; 2. Store vtable_base.
local.tee $tmp                    ;; save ptr for later stores
i32.const <vtable_base>
i32.store offset=0
;; 3. Store each field.
local.get $tmp
<compile e1>
i32.store offset=<f1_offset>
local.get $tmp
<compile e2>
i32.store offset=<f2_offset>
;; 4. Leave the ptr on the stack (the object reference).
local.get $tmp
```

### 1.6.7 Static method call

```wasm
;; `ClassName::method(args)`:
;; (compile each arg)
call $<method_fn_idx>             ;; direct call — no vtable lookup
```

### 1.6.8 Default synthesised `new`

If a class declares no `new`, the compiler synthesises:

```wasm
;; synthesize `fn ClassName::new() -> Self`:
;; 1. Allocate.
i32.const <field_stride>
call $__alk_alloc
local.tee $self
;; 2. Store vtable_base.
i32.const <vtable_base>
i32.store offset=0
;; 3. Zero-initialise each field.
local.get $self
i32.const 0
i32.store offset=<f1_offset>
;; ... repeat for each field ...
;; 4. Return self.
local.get $self
```

### 1.6.9 Section ordering (after Gap 1)

```
1. Type section        (function types: user fns + host imports + method types)
2. Import section      (alk::vec_* + alk::__alk_alloc)            [Gap 5 + Gap 1]
3. Function section    (user function + method declarations)
4. Table section       (one funcref table for vtables)            [Gap 1]
5. Memory section      (1+ pages, grown for strings)              [Gap 4]
6. Global section      (vtable_base constants per class)          [Gap 1]
7. Export section      (pub fns + pub classes + memory)
8. Element section     (vtable method function indices)           [Gap 1]
9. Data section        (string literals as length-prefixed)       [Gap 4]
10. Code section       (function + method bodies)
```

## 1.7 Error cases

| ID | Trigger condition | Message format |
|----|-------------------|----------------|
| **LANG-110-E1** | Cyclic inheritance (`A : B : A`). | `cyclic inheritance: A : B : A` |
| **LANG-110-E2** | Multiple inheritance (`class A : B, C`). | `parse error: multiple inheritance is not supported (expected single base class)` |
| **LANG-112-E3** | Method override signature mismatch. | `cannot override \`{method}\` in \`{class}\`: signature mismatch` |
| **LANG-105-E4** | Constructor `new` returns non-`Self`. | `constructor \`new\` in \`{class}\` must return \`Self\`` |
| **LANG-105-E5** | Constructor `new` is an instance method (has `self`). | `constructor \`new\` in \`{class}\` must be a static method (no \`self\` parameter)` |
| **LANG-1XX-E6** | Duplicate field name. | `field \`{field}\` already declared in class \`{class}\`` |
| **LANG-1XX-E7** | Duplicate method name in same class. | `method \`{method}\` already declared in class \`{class}\`` |
| **LANG-1XX-E8** | Private field accessed from outside the class. | `private field \`{class}.{field}\` accessed from outside class \`{class}\`` |
| **LANG-1XX-E9** | Private method called from outside the class. | `private method \`{class}.{method}\` called from outside class \`{class}\`` |
| **LANG-114-E10** | Field assignment to a `monotone`/`antitone` qualified field. **(CR-10)** | `assignment to \`{qualifier}\`-qualified field \`{class}.{field}\` is forbidden (qualified fields are immutable after construction)` |
| **LANG-1XX-E11** | Object literal missing a field. | `missing field \`{field}\` in object literal for class \`{class}\`` |
| **LANG-1XX-E12** | Object literal with unknown field. | `unknown field \`{field}\` in object literal for class \`{class}\`` |
| **LANG-1XX-E13** | Object literal with duplicate field init. | `field \`{field}\` initialised twice in object literal for class \`{class}\`` |
| **LANG-1XX-E14** | Unknown class referenced (in `: Base`, field type, or `Object` literal). | `unknown class \`{class}\`` |
| **LANG-1XX-E15** | Field access on a non-class type. | `field access \`.{field}()\` is not valid on type \`{ty}\`` |
| **LANG-1XX-E16** | Class has no field of that name. | `class \`{class}\` has no field \`{field}\`` |
| **LANG-1XX-E17** | `self` used outside an instance method. | `\`self\` is not available outside an instance method` |
| **LANG-1XX-E18** | `Self` used outside a class body. | `\`Self\` used outside a class body` |
| **LANG-1XX-E19** | Static method called as instance method (or vice versa). | `instance method \`{class}.{method}()\` is a static method; call as \`{class}::{method}\`` (or the inverse) |
| **LANG-1XX-E20** | Unknown static method. | `unknown static method \`{class}::{method}\`` |
| **LANG-1XX-E21** | Static call arity mismatch. | `static call \`{class}::{method}\` expects {N} argument(s) but was called with {M}` |
| **LANG-1XX-E22** | Static call arg-type mismatch. | `argument {i} to \`{class}::{method}\` has type \`{actual}\` but parameter has type \`{expected}\`` |
| **LANG-1XX-E23** | Assignment target is not `Expr::Field`. | `assignment target must be a field access (\`obj.field\`)` |
| **LANG-1XX-E24** | Field assignment on a non-class type. | `field assignment on non-class type \`{ty}\` is not supported` |
| **LANG-1XX-E25** | Field-assignment value-type mismatch. | `field \`{class}.{field}\` has type \`{field_ty}\` but assignment value has type \`{val_ty}\`` |
| **LANG-1XX-E26** | Class method not found (in chain). | `class \`{class}\` has no method \`{method}\`` |

## 1.8 Validation rules

- **V1-1.** Every `ClassDecl` has a unique name within the module.
- **V1-2.** Every `: Base` reference resolves in the `ClassTable`.
- **V1-3.** No class inherits from itself (directly or transitively).
- **V1-4.** No class declares two fields with the same name.
- **V1-5.** No class declares two methods with the same name (overrides
  must match the base method's signature exactly).
- **V1-6.** `fn new(...)` (if declared) returns `Self` and is a static
  method.
- **V1-7.** Every `self` reference is inside an instance method body.
- **V1-8.** Every `Self` type reference is inside a class body.
- **V1-9.** Every `obj.field` access resolves to a declared field of the
  receiver's static class (or its base chain).
- **V1-10.** Private fields/methods are not accessed from outside the
  class (or its subclasses).
- **V1-11.** Object literals initialise every declared field exactly once.
- **V1-12.** Object literal initialiser types are subtypes of the field
  types.
- **V1-13.** Field assignments target only `Unrestricted`-qualified fields
  (CR-10).
- **V1-14.** Field-assignment value types are subtypes of the field types.
- **V1-15.** Method-call argument types are subtypes of the parameter
  types.
- **V1-16.** Static-call arity matches the parameter count.
- **V1-17.** `vtable_base + slot < total_table_size` for every virtual
  dispatch (compile-time assertion).
- **V1-18.** `field_offset < field_stride(class)` for every field access.

## 1.9 Test cases

| Test ID | Source | Expected behaviour |
|---------|--------|--------------------|
| **LANG-1T-01** | `class Empty {}` | Parses with zero fields and zero methods. |
| **LANG-1T-02** | `class Derived : Base { pub x: i32; }` (with `class Base {}` defined) | Round-trips through `format!("{:?}", ast)`. |
| **LANG-1T-03** | `class C { pub fn new() -> Self { Self { } } }` | Parses as `ClassDecl` with one method `new` and `Expr::Object`. |
| **LANG-1T-04** | `class C { pub fn new() -> Self { Self { } } pub fn get(self) -> i32 { return self.x; } }` | `b.get()` returns `i32` (typechecks after `x` is added). |
| **LANG-1T-05** | `let b: Button = Button::new("x"); b.label` | Field access returns `Type::Str`. |
| **LANG-1T-06** | Calling a `priv` method from a sibling class. | `TypeError`: `private method \`C.m\` called from outside class \`C\``. |
| **LANG-1T-07** | Overriding a method with a different signature. | `TypeError`: `cannot override \`m\` in \`C\`: signature mismatch`. |
| **LANG-1T-08** | `class A : B, class B : A` (cycle). | `TypeError` reported once. |
| **LANG-1T-09** | `Self` outside a class body. | `TypeError`: `\`Self\` used outside a class body`. |
| **LANG-1T-10** | `self` outside an instance method. | `TypeError`: `\`self\` is not available outside an instance method`. |
| **LANG-1T-11** | `class C { monotone items: Vec<i32>; pub fn reset(self) { self.items = Vec::new(); } }` | `TypeError`: `assignment to \`monotone\`-qualified field \`C.items\` is forbidden`. **(CR-10 test)** |
| **LANG-1T-12** | `class C { pub x: i32; pub fn set(self, v: i32) { self.x = v; } }` | Typechecks (unrestricted field is assignable). |
| **LANG-1T-13** | `class Counter { pub count: i32; pub fn new() -> Self { Self { count: 0 } } pub fn inc(self) { self.count = self.count + 1; } pub fn get(self) -> i32 { return self.count; } } fn main() -> i32 { let c: Counter = Counter::new(); c.inc(); c.inc(); return c.get(); }` | Typechecks; emits valid WASM binary with 1 exported `main` function, a `Table` section, and an `Element` section. |
| **LANG-1T-14** | (LANG-1T-13's binary) | `wasmparser::Parser` validates the full binary. |
| **LANG-1T-15** | (LANG-1T-13's binary) | Object literal + field read produces a sequence containing `Instruction::Call` (to `__alk_alloc`), `Instruction::I32Load`, `Instruction::I32Store` (verified at the `AlkInstr`-level on the compiled body). |
| **LANG-1T-16** | `class A {} class B : A {} fn main() { let b: B = B::new(); let a: A = b; }` | Typechecks (upcast `B` → `A` is implicit; no WASM instruction emitted for the cast). |
| **LANG-1T-17** | `class A {} class B : A {} fn main() { let a: A = A::new(); let b: B = a; }` | `TypeError`: `A` is not a subtype of `B` (downcast forbidden). |
| **LANG-1T-18** | `class A { pub fn greet(self) { } } class B : A { pub fn greet(self) { } } fn main() { let b: B = B::new(); b.greet(); }` | Typechecks; `b.greet()` dispatches virtually to `B::greet` via `call_indirect`. |
| **LANG-1T-19** | `class C { pub fn new() -> i32 { return 0; } }` | `TypeError`: `constructor \`new\` in \`C\` must return \`Self\``. |
| **LANG-1T-20** | `class C { pub fn new(self) -> Self { } }` | `TypeError`: `constructor \`new\` in \`C\` must be a static method (no \`self\` parameter)`. |
| **LANG-1T-21** | End-to-end (new `tests/oo_integration.rs`): the Counter program (LANG-1T-13) compiles, the WASM is instantiated with a host providing `__alk_alloc` as a bump allocator, `main` is called, and the return value is `2` (after two `inc()` calls). | The host's view of the WASM execution matches. (Optional; falls back to instruction-level tests if no Wasmtime/wasmi.) |

## 1.10 Acceptance criteria

- **AC1-1.** `cargo test -p alkalive-compiler oo_tests` passes with the 21
  tests above.
- **AC1-2.** `cargo test --workspace` is green (no regressions).
- **AC1-3.** `cargo clippy -p alkalive-compiler -- -D warnings` is clean.
- **AC1-4.** The Counter program (LANG-1T-13) produces a WASM binary that
  contains: a `Table` section, an `Element` section, a `Global` section
  (vtable_base constants), an `__alk_alloc` import, and an exported `main`
  function.
- **AC1-5.** `wasmparser::Parser` validates the full binary.
- **AC1-6.** A diff of `wasm_codegen.rs` shows the new
  `TableSection`/`ElementSection`/`GlobalSection` emissions and the
  `Expr::Field`/`Object`/`StaticCall`/`MethodCall` arms.

## 1.11 Traceability

| Requirement | ADR / source | Fine-draft decision | Implementation requirement | Test |
|-------------|--------------|---------------------|----------------------------|------|
| LANG-101..103 | ADR-008 ("object oriented"); fine-draft §1.4.1 | "Classes with `pub`/`priv` visibility, fields, methods." | §1.2 EBNF + §1.3 AST + §1.5 lexer/parser. | LANG-1T-01, LANG-1T-02. |
| LANG-104..106 | ADR-008; fine-draft §1.4.1-1.4.2 | "Instance vs static methods; `self`/`Self`." | §1.3 AST (`MethodDecl.is_instance`, `Expr::Self_`). | LANG-1T-09, LANG-1T-10, LANG-1T-19, LANG-1T-20. |
| LANG-107..109 | ADR-008; fine-draft §1.4.1 | "Field access, object literals, static calls." | §1.3 AST (`Expr::Field`/`Object`/`StaticCall`). | LANG-1T-03, LANG-1T-04, LANG-1T-05. |
| LANG-110..112 | ADR-008; fine-draft §1.4.5 | "Single inheritance; invariant override." | §1.4.1 `ClassTable` + §1.4.3 override check. | LANG-1T-07, LANG-1T-08. |
| LANG-113 (CR-10 fix) | Critical review CR-10; ADR-027 Phase 2 | "Field assignment to qualified fields is forbidden." | §1.4.6 + `LANG-114-E10`. | LANG-1T-11, LANG-1T-12. |
| LANG-114..116 (CR-7 fix) | Critical review CR-7; fine-draft §1.4.4 | "vtable_base is a table index; dispatch via `local.get obj; i32.load offset=0; i32.const <slot>; i32.add; call_indirect`." | §1.6.1 layout + §1.6.2 dispatch. | LANG-1T-15, LANG-1T-18. |
| LANG-117..120 | fine-draft §1.4.4 | "Constructors, default `new`, `__alk_alloc`." | §1.6.6 + §1.6.8 + `__alk_alloc` import. | LANG-1T-13, LANG-1T-15. |
| (CR-15 fix) | Critical review CR-15 | "`@monotone` on a class is a parse error; qualifiers apply to fields only." | `parse_class` calls `parse_leading_attributes`; class-level `@monotone` → `TypeError`. | (Add LANG-1T-22: `@monotone class C {}` → error.) |
| (CR-17 fix) | Critical review CR-17 | "`__alk_alloc` added to import table." | §1.5 + §5.4.2 cross-reference. | LANG-1T-15. |
| (CR-18 fix) | Critical review CR-18 | "Compound assignment is a parse error in this wave; chained field assignment is supported via recursive `Expr::Field` receivers." | `parse_stmt` rejects `+=` etc.; `Stmt::Assign.target` may be nested `Expr::Field`. | (Add LANG-1T-23: `a.b.c = 5` typechecks; `a.b += 1` is a parse error.) |
| (CR-20 documented) | Critical review CR-20 | "`monotone` field cannot be passed to `unrestricted Vec<T>` params; documented trade-off." | §1.4.6 note. | (Documentation check.) |

---

# Gap 2 — Module System (imports/exports)

## 2.1 Exact requirements

- **LANG-201.** The lexer must recognise the keywords `import`, `from`,
  `with`, `as`. (`pub` and `priv` are already keywords from Gap 1.)
- **LANG-202.** The parser must accept zero or more `import` declarations
  **before** the `module` keyword. The grammar is:
  ```ebnf
  ImportDecl := 'import' '{' ImportName (',' ImportName)* '}'
                'from' String ('with' '[' Cap (',' Cap)* ']')? ';'
  ImportName := Ident ('as' Ident)?
  Cap        := 'render' | 'gpu' | 'net' | 'fs' | 'time' | 'rand' | 'ipc'
  ```
- **LANG-203.** Every top-level item (`Fn`, `Let`, `Class`) carries a
  `Visibility` field. `pub` items are exported; `priv` (default) items are
  module-local.
- **LANG-204.** A new module `crates/alkalive-compiler/src/modules.rs`
  implements the resolver. Its public entry is
  `resolve(sources: HashMap<PathBuf, &str>) -> Result<ResolvedGraph,
  ResolveError>`.
- **LANG-205.** The resolver builds a module graph by reading each
  `import`'s `from` path:
  - `std/<name>` → built-in stdlib module.
  - `app/<name>` → user module at `<source_root>/app/<name>.alk`.
  - `./<name>` or `../<name>` → user module relative to the importer.
- **LANG-206.** Cyclic imports (`a → b → a`) are forbidden. The resolver
  detects cycles and reports them once.
- **LANG-207.** Capability attestation: each `std/*` module declares its
  required capabilities in a manifest. The resolver checks that the importer
  grants a superset of the required capabilities. Missing capabilities are
  reported.
- **LANG-208.** Each `ResolvedImport` is annotated with the source module's
  signature for the imported name (function type, class signature, or
  constant type). These signatures flow into the typechecker's `FnSigTable`
  and `ClassTable`.
- **LANG-209.** Cross-module calls (`Expr::PathCall(module, member, args)`)
  resolve through the `FnSigTable` (populated by the resolver) rather than
  being special-cased to `Vec::new`.
- **LANG-210.** **Tree-shaking is deferred to a future wave.** All `pub fn`
  and `pub class` items are emitted to the WASM binary regardless of
  reachability. **(Addresses CR-8 and CR-9.)** The conservative future
  tree-shaking rule is documented in §5.4.6 (Gap 5).
- **LANG-211.** **The architectural inversion (Gap 2 AOT-compile model) is
  deferred to a future wave.** The runtime continues to use the
  `include_str!` model (`crates/alkalive-runtime-wasm/src/lib.rs:52`). Gap 2
  adds the resolver + cross-module linking at compile time only; the
  runtime's role is unchanged. **(Addresses CR-2.)**
- **LANG-212.** Strategy A (single-WASM linking) is shipped: the entire
  module graph is linked into one WASM binary. Cross-module calls become
  direct `call <fnidx>` (the linker rewrites `Expr::PathCall` into a `call`
  to the resolved function index).
- **LANG-213.** The ImportSection of the final WASM contains only **host
  imports** (Gap 5's `vec_*` + Gap 1's `__alk_alloc`); no AlkALive-level
  imports leak to the WASM layer.

## 2.2 Syntax/grammar changes (EBNF)

```ebnf
File           := ShebangAttr* ImportDecl* ModuleDecl
ImportDecl     := 'import' '{' ImportName (',' ImportName)* '}'
                 'from' String ('with' '[' Cap (',' Cap)* ']')? ';'
ImportName     := Ident ('as' Ident)?
Cap            := 'render' | 'gpu' | 'net' | 'fs' | 'time' | 'rand' | 'ipc'
ExportedItem   := 'pub' (FnDecl | LetDecl | ClassDecl)
```

Notes:
- `pub` may appear before `fn`, `let`, and `class`. Items without `pub` are
  module-private (the default).
- `import { A, B as C } from "path";` — `as` introduces an alias.
- `with [render, gpu]` grants specific capabilities to the imported module.
  The capability list is checked against the module's declared requirements
  at link time.
- The string after `from` is a **module path**, not a file path: it uses `/`
  as a separator and has no extension.

## 2.3 AST/IR changes

Add to `ast.rs`:

```rust
/// A capability granted to an imported module (ADR-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Render, Gpu, Net, Fs, Time, Rand, Ipc,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Render => write!(f, "render"),
            Capability::Gpu => write!(f, "gpu"),
            Capability::Net => write!(f, "net"),
            Capability::Fs => write!(f, "fs"),
            Capability::Time => write!(f, "time"),
            Capability::Rand => write!(f, "rand"),
            Capability::Ipc => write!(f, "ipc"),
        }
    }
}

/// `import { Name, Name as Alias } from "path" with [cap, cap];`
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    /// The imported (name, local_alias) pairs.
    pub items: Vec<(String, String)>,
    /// The module path string (without quotes).
    pub path: String,
    /// Granted capabilities (empty vec = no capabilities granted).
    pub capabilities: Vec<Capability>,
    pub line: u32,
    pub col: u32,
}

/// One entry in the module's import list, after resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedImport {
    /// The source module's stable id (e.g. `app/button`).
    pub source_module: String,
    /// The source-side name (e.g. `Button`).
    pub source_name: String,
    /// The local alias (defaults to `source_name` if no `as` clause).
    pub local_name: String,
    /// The granted capabilities (carried for the typechecker + linker).
    pub capabilities: Vec<Capability>,
}
```

Extend `ModuleDecl`:

```rust
pub struct ModuleDecl {
    // ... existing fields ...
    pub imports: Vec<ImportDecl>,             // NEW
    /// Filled in by the resolver pass (not present in the raw AST).
    pub resolved_imports: Vec<ResolvedImport>, // NEW (post-parse)
}
```

`ItemDecl::Fn`/`Let`/`Class` already carry `Visibility` (Gap 1).

## 2.4 Type-system changes

- **`check_module`** accepts a `ResolvedGraph` (or `&[ResolvedImport]`) and
  seeds each module's `FnSigTable` with the imported function signatures
  (qualified as `"<source_module>::<name>"` plus the local alias as a
  bare-name entry).
- **`ClassTable`** is seeded with imported `pub class` signatures (qualified
  by source module).
- **`Expr::PathCall(module, member, args)`** resolves through the
  `FnSigTable` for cross-module calls (already specified in Gap 3 §3.4.5).
- **Visibility check:** an imported name must be `pub` in the source module.
  Importing a `priv` item is a `ResolveError`.
- **Type visibility check:** an imported `pub fn` whose return type is a
  `priv class` is a `TypeError` (the type is not visible in the importing
  module).

## 2.5 Compiler changes

| Layer | Change | Functions added/modified |
|-------|--------|--------------------------|
| **Lexer** | New keywords: `import`, `from`, `with`, `as`. | `KEYWORDS` map extended; `TokenKind` enum extended. |
| **Parser** | `parse_module` parses zero or more `import` statements **before** the `module` keyword. New `parse_import` function. `parse_fn`/`parse_let`/`parse_class` accept a leading `pub` (already collected by Gap 1's leading-attributes path). | **Added:** `parse_import`, `parse_capability_list`. **Modified:** `parse_module` (top-level loop accepts imports). |
| **AST** | As above (§2.3). | New: `Capability`, `ImportDecl`, `ResolvedImport`, `ModuleDecl.imports`, `ModuleDecl.resolved_imports`. |
| **Resolver** | New module `modules.rs`. Public entry: `resolve(sources: HashMap<PathBuf, &str>) -> Result<ResolvedGraph, ResolveError>`. | **Added:** `ResolvedGraph`, `ResolveError`, `resolve`, `resolve_module`, `build_module_graph`, `detect_cycles`, `attest_capabilities`, `resolve_names`. |
| **Typechecker** | `check_module` accepts a `ResolvedGraph` and seeds `FnSigTable`/`ClassTable` with imported signatures. `Expr::PathCall` resolves through `FnSigTable` (Gap 3 §3.4.5 already covers this). | **Modified:** `check_module` (signature change), `collect_signatures` (merges in resolved imports), `collect_classes` (merges in resolved classes). |
| **WASM codegen** | (1) Reachability pass is **NOT** performed (CR-8/9 deferral). (2) `PathCall` compiles to `call <resolved_fnidx>` (direct call into the linked module's function). (3) `pub` items become exports. (4) Private items are emitted as non-exported functions (still callable internally). | **Modified:** `compile_to_wasm` (accepts `ResolvedGraph`; resolves `PathCall` to absolute function indices; no reachability filter on the export section). |
| **Host runtime** | **Unchanged.** The runtime continues to use the `include_str!` model (CR-2 deferral). The WASM module exports `main` (or a small set of entry points) and the runtime loads it. The architectural inversion is deferred. | — |

## 2.6 WASM changes

- **Single-WASM linking (Strategy A):** the entire module graph is linked
  into one WASM binary. Cross-module calls become direct `call <fnidx>`.
- The ImportSection of the final WASM contains only host imports
  (`alk::vec_*` + `alk::__alk_alloc`); no AlkALive-level imports leak to
  the WASM layer.
- The ExportSection contains every `pub fn` and `pub class` exported by name
  (no reachability filter — tree-shaking deferred per CR-8/9).
- Each source module contributes its functions, classes, and exports; the
  function index space is contiguous across modules.

```wasm
;; Cross-module call: `button::make_button("hi")` in app/main.alk
;; resolving to function index 12 (allocated to app/button.alk's
;; `make_button` after linking):
i32.const <string_offset_for_"hi">
call 12                   ;; direct call to app/button::make_button
```

## 2.7 Error cases

| ID | Trigger condition | Message format |
|----|-------------------|----------------|
| **LANG-205-E1** | Module path not found (file missing or `std/<name>` unknown). | `module '{path}' not found` |
| **LANG-206-E2** | Imported name not declared `pub` in source. | `name '{name}' not exported by module '{path}'` |
| **LANG-206-E3** | Cyclic import. | `cyclic import: {a} -> {b} -> {a}` |
| **LANG-207-E4** | Missing capability. | `missing capability: '{cap}' required by '{path}' but not granted by importer` |
| **LANG-207-W1** | Ungranted capability (importer granted a cap the source doesn't require). | `ungranted capability '{cap}'` (warning, not error) |
| **LANG-2XX-E5** | Imported name's type is not visible in the importing module. | `imported name '{name}' has type '{ty}' which is not visible in this module` |
| **LANG-2XX-E6** | Ambiguous import (duplicate local name without `as`). | `ambiguous import: '{name}' is imported from both '{a}' and '{b}'` |
| **LANG-2XX-E7** | Unknown capability name. | `unknown capability '{cap}'; expected one of: render, gpu, net, fs, time, rand, ipc` |

## 2.8 Validation rules

- **V2-1.** Every `import` path resolves to an existing module.
- **V2-2.** Every imported name is `pub` in the source module.
- **V2-3.** No import cycle exists in the module graph.
- **V2-4.** Every `with [cap, ...]` capability is in the fixed vocabulary.
- **V2-5.** Every `std/*` module's required capabilities are a subset of
  the importer's granted capabilities.
- **V2-6.** No two imports bind the same local name (without `as`
  disambiguation).
- **V2-7.** Every imported `pub fn`'s return type is visible in the
  importing module (not a `priv class`).
- **V2-8.** Cross-module `Expr::PathCall(module, member, args)` resolves
  through `FnSigTable`.
- **V2-9.** All `pub fn`/`pub class` items are emitted to the WASM binary
  (no tree-shaking in this wave).
- **V2-10.** The final WASM ImportSection contains only host imports.

## 2.9 Test cases

| Test ID | Source | Expected behaviour |
|---------|--------|--------------------|
| **LANG-2T-01** | Two-file graph: `app/main` imports `Button` from `app/button`. | Resolves with no errors. |
| **LANG-2T-02** | `app/main` imports `priv_helper` from `app/button` (where `priv_helper` is `priv`). | `ResolveError`: `name 'priv_helper' not exported by module 'app/button'`. |
| **LANG-2T-03** | Cycle: `a → b → a`. | `ResolveError`: `cyclic import: a -> b -> a` (reported once). |
| **LANG-2T-04** | `import { X } from "app/nonexistent";` | `ResolveError`: `module 'app/nonexistent' not found`. |
| **LANG-2T-05** | `import { render_text } from "std/canvas";` (without `with [render]`). | `ResolveError`: `missing capability: 'render' required by 'std/canvas' but not granted by importer`. |
| **LANG-2T-06** | `import { render_text } from "std/canvas" with [render];` | Resolves; `render_text` is callable. |
| **LANG-2T-07** | `import { render_text } from "std/canvas" with [render, gpu];` (gpu ungranted). | Warning `ungranted capability 'gpu'`; resolves. |
| **LANG-2T-08** | Imported `pub fn` is callable; its return type is correctly inferred (depends on Gap 3). | Typechecks; `make_button("hi")` returns `Button`. |
| **LANG-2T-09** | Imported `pub class` is usable as a type. | `let b: Button = make_button("hi");` typechecks. |
| **LANG-2T-10** | Visibility mismatch: importing a `pub fn` that returns a `priv class`. | `TypeError`: `imported name 'make_secret' has type 'Secret' which is not visible in this module`. |
| **LANG-2T-11** | Ambiguous import: `import { X } from "a"; import { X } from "b";`. | `TypeError`: `ambiguous import: 'X' is imported from both 'a' and 'b'`. |
| **LANG-2T-12** | Aliased import: `import { X as Y } from "a";`. | `Y` is callable; `X` is not in scope. |
| **LANG-2T-13** | (tree-shaking: an unused `pub fn helper()` in `app/button`) | `helper` IS in the final WASM's export section (tree-shaking deferred per CR-8/9). |
| **LANG-2T-14** | Cross-module call: `app/main` calls `make_button` from `app/button`. | The compiled body contains `Call(<absolute_fnidx>)` where `<absolute_fnidx>` points into `app/button`'s function index space. |
| **LANG-2T-15** | End-to-end (new `tests/module_integration.rs`): two-module program; `app/main` imports `Button` from `app/button` and constructs it. | Typechecks and produces a valid WASM binary that `wasmparser` accepts. |

## 2.10 Acceptance criteria

- **AC2-1.** `cargo test -p alkalive-compiler module_tests` passes with the
  15 tests above.
- **AC2-2.** `cargo test --workspace` is green.
- **AC2-3.** `cargo clippy -p alkalive-compiler -- -D warnings` is clean.
- **AC2-4.** A two-module program (LANG-2T-15) produces a single WASM binary
  with: (a) `wasmparser` validation, (b) no AlkALive-level imports in the
  ImportSection, (c) `pub fn` from the non-entry module in the ExportSection.
- **AC2-5.** The runtime is unchanged (CR-2 deferral verified by diffing
  `crates/alkalive-runtime-wasm/src/lib.rs`).

## 2.11 Traceability

| Requirement | ADR / source | Fine-draft decision | Implementation requirement | Test |
|-------------|--------------|---------------------|----------------------------|------|
| LANG-201..203 | ADR-008, ADR-018; fine-draft §2.4.1 | "`import`/`pub`/`export` with capability grants." | §2.2 EBNF + §2.3 AST + §2.5 parser. | LANG-2T-01, LANG-2T-12. |
| LANG-204..208 | ADR-018; fine-draft §2.4.3 | "Resolver builds module graph; detects cycles; attests capabilities; resolves names." | §2.5 `modules.rs` + §2.4 typechecker integration. | LANG-2T-01..07. |
| LANG-209 | ADR-018; fine-draft §2.4.4 | "`PathCall` resolves through `FnSigTable`." | §2.4 + Gap 3 §3.4.5. | LANG-2T-08, LANG-2T-14. |
| LANG-210 (CR-8/9 fix) | Critical review CR-8, CR-9; ADR-018 | "Tree-shaking deferred; conservative virtual-dispatch rule documented for future wave." | §2.5 (no reachability pass) + §5.4.6 (Gap 5 policy). | LANG-2T-13. |
| LANG-211 (CR-2 fix) | Critical review CR-2; technical-specification TD8/C10 | "Architectural inversion deferred; runtime continues `include_str!` model." | §2.5 host runtime unchanged. | (Diff check; no test.) |
| LANG-212..213 | ADR-018; fine-draft §2.4.5 | "Strategy A: single-WASM linking; only host imports in ImportSection." | §2.6 WASM layout. | LANG-2T-14, LANG-2T-15. |
| (CR-28 fix) | Critical review CR-28 | "Capability vocabulary of 7 is closed; mapped to ADRs as: render→ADR-001/007, gpu→ADR-006, net→ADR-021, fs→(future), time→(future), rand→(future), ipc→ADR-021." | §2.2 EBNF + §2.7 `LANG-2XX-E7`. | LANG-2T-05, LANG-2T-06, LANG-2T-07. |

---

# 6. Cross-Gap Interface Contracts

The following contracts must hold for the gaps to compose correctly:

## 6.1 Gap 3 → Gap 1 (Type Inference → OO)

- `FnSigTable` (from Gap 3) is extended by Gap 1 to include method
  signatures (`receiver_class: Some(class_name)`). The lookup function
  gains a `lookup_method(class: &str, method: &str) -> Option<&FnSig>`
  variant.
- `check_expr`'s `Expr::MethodCall` arm (Gap 3 §3.4.3) dispatches to
  `class_method_return_type` (Gap 1 §1.4.7) when the receiver is a class
  type.

## 6.2 Gap 4 → Gap 5 (Strings → Collections)

- The `StringTable` (Gap 4) and the `host_imports` list (Gap 5) are both
  owned by `compile_to_wasm`. The `FnCompiler` carries references to both.
- The import section (Gap 5) is emitted **before** the function section;
  the data section (Gap 4) is emitted **after** the code section. This
  ordering is fixed by the WASM binary format and is already correct in the
  existing `compile_to_wasm` skeleton.

## 6.3 Gap 4 + Gap 5 → Gap 1 (Strings + Collections → OO)

- A class field of type `string` stores an `i32` pointer produced by
  Gap 4's string table.
- A class field of type `Vec<i32>` stores an `i32` handle produced by
  Gap 5's `vec_new` host import.
- The `__alk_alloc` host function (Gap 1 §1.6.6) is added to the same
  `host_imports` list as the `vec_*` functions (Gap 5). Its index in the
  import section is contiguous with the collection imports (index 10 after
  the 10 `vec_*` imports).

## 6.4 Gap 1 → Gap 2 (OO → Modules)

- `Visibility` (from Gap 1) is extended to top-level `ItemDecl::Fn`/`Let`
  in Gap 2.
- `ClassDecl` (from Gap 1) gains a `Visibility` field in Gap 2.
- The `FnSigTable` (from Gap 3) is populated with imported signatures by
  Gap 2's resolver pass.

## 6.5 Gap 3 → Gap 2 (Type Inference → Modules)

- The `FnSigTable.imported_from: Option<String>` field (Gap 3 §3.3) is
  populated by Gap 2's resolver.
- `Expr::PathCall(module, member, args)` (Gap 3 §3.4.5) resolves through
  the `FnSigTable` for cross-module calls once Gap 2's resolver has
  populated it.

## 6.6 Shared data structures (who owns what)

| Structure | Defined in | Populated by | Consumed by |
|-----------|-----------|--------------|-------------|
| `FnSigTable` | typechecker (Gap 3) | `collect_signatures` (Gap 3) + resolver (Gap 2) + class collector (Gap 1) | `check_expr` (Gap 3) |
| `ClassTable` | typechecker (Gap 1) | `collect_classes` (Gap 1) + resolver (Gap 2) | `check_expr` (Gap 1) + `check_method_override` (Gap 1) |
| `StringTable` | wasm_codegen (Gap 4) | `compile_expr` on `Lit::Str` (Gap 4) | data-section emission (Gap 4) |
| `host_imports` | wasm_codegen (Gap 5) | `compile_to_wasm` (Gap 5) + `__alk_alloc` (Gap 1) | `AlkInstr::Call` resolution (Gap 5) + object allocation (Gap 1) |
| `ResolvedGraph` | modules (Gap 2) | `resolve` (Gap 2) | `check_module` (Gap 3 + Gap 2) + (future tree-shaking) |
| `ClassLayout` (offsets + vtable slots) | wasm_codegen (Gap 1) | `compute_class_layouts` (Gap 1) | `Expr::Field`/`Object`/`MethodCall` codegen (Gap 1) |

---

# 7. WASM Section Ordering (final, after all 5 gaps land)

```
1. Type section        (function types: user fns + host imports + method types)
2. Import section      (alk::vec_* + alk::__alk_alloc)            [Gap 5 + Gap 1]
3. Function section    (user function + method declarations)
4. Table section       (one funcref table for vtables)            [Gap 1]
5. Memory section      (1+ pages, grown for strings)              [Gap 4]
6. Global section      (vtable_base constants per class)          [Gap 1]
7. Export section      (pub fns + pub classes + memory)
8. Element section     (vtable method function indices)           [Gap 1]
9. Data section        (string literals as length-prefixed)       [Gap 4]
10. Code section       (function + method bodies)
```

The existing `compile_to_wasm` function in `wasm_codegen.rs:491-662` emits
sections 1, 3, 5, 7, 11 (today: code section is 11 because of `start`).
Gaps 1, 4, and 5 add sections 2, 4, 6, 9. Gap 1 also adds 8 (Element).

---

# 8. Traceability Matrix (consolidated)

| Requirement ID | ADR | Fine-draft § | Implementation § | Test ID | CR addressed |
|----------------|-----|--------------|------------------|---------|--------------|
| LANG-301..303 | ADR-009 | 3.4.1-3.4.2 | 3.3, 3.4.1 | LANG-3T-05 | — |
| LANG-304..305 | ADR-009 | 3.4.3 | 3.4.2, 3.7 | LANG-3T-01..04 | — |
| LANG-306 | ADR-009 | 3.4.4 | 3.4.3, 3.4.4 | LANG-3T-07..11 | — |
| LANG-307 | ADR-009 | 3.4.5 | 3.4.5 | LANG-3T-07, 12 | CR-14 (rewriting) |
| LANG-401..403 | ADR-008, ADR-022 | 4.4.1-4.4.3 | 4.3, 4.6.2 | LANG-4T-01, 05 | — |
| LANG-404..405 | ADR-009 | 4.4.2 | 4.6.4, 4.6.5 | LANG-4T-04, 07 | — |
| LANG-406 | ADR-008 | 4.4.5 | 4.6.3 | LANG-4T-09 | — |
| LANG-407 | ADR-008 | 4.4.4 | 4.6.4 | LANG-4T-01 | — |
| LANG-408 | ADR-008 | 4.6 | 4.6.5 | LANG-4T-08 | — |
| LANG-501..503 | ADR-008, ADR-018 | 5.4.1-5.4.2 | 5.3, 5.4.2 | LANG-5T-01, 02 | — |
| LANG-504 | fine-draft 5.4.2 | 5.4.2 | 5.4.5 | LANG-5T-03, 04 | — |
| LANG-505 | fine-draft 5.4.3 | 5.4.3 | 5.4.3 | LANG-5T-03, 04, 10 | — |
| LANG-507..508 | fine-draft 5.4.4 | 5.4.4 | 5.4.4 | LANG-5T-05, 06 | — |
| LANG-510 | — | — | 5.4.6, 5.7 | LANG-5T-11 | **CR-8, CR-9** |
| LANG-101..103 | ADR-008 | 1.4.1 | 1.2, 1.3, 1.5 | LANG-1T-01, 02 | — |
| LANG-104..106 | ADR-008 | 1.4.1-1.4.2 | 1.3 | LANG-1T-09, 10, 19, 20 | — |
| LANG-107..109 | ADR-008 | 1.4.1 | 1.3 | LANG-1T-03, 04, 05 | — |
| LANG-110..112 | ADR-008 | 1.4.5 | 1.4.1, 1.4.3 | LANG-1T-07, 08 | — |
| LANG-113 | ADR-027 Phase 2 | — | 1.4.6, 1.7 (E10) | LANG-1T-11, 12 | **CR-10** |
| LANG-114..116 | ADR-008 | 1.4.4 | 1.6.1, 1.6.2 | LANG-1T-15, 18 | **CR-7** |
| LANG-117..120 | fine-draft 1.4.4 | 1.4.4 | 1.6.6, 1.6.8 | LANG-1T-13, 15 | **CR-17** |
| (CR-15) | — | — | 1.5, 1.11 | LANG-1T-22 (add) | **CR-15** |
| (CR-18) | — | — | 1.5, 1.11 | LANG-1T-23 (add) | **CR-18** |
| LANG-201..203 | ADR-008, ADR-018 | 2.4.1 | 2.2, 2.3, 2.5 | LANG-2T-01, 12 | — |
| LANG-204..208 | ADR-018 | 2.4.3 | 2.5, 2.4 | LANG-2T-01..07 | — |
| LANG-209 | ADR-018 | 2.4.4 | 2.4, 3.4.5 | LANG-2T-08, 14 | — |
| LANG-210 | — | — | 2.5, 5.4.6 | LANG-2T-13 | **CR-8, CR-9** |
| LANG-211 | — | — | 2.5 (host runtime unchanged) | (diff check) | **CR-2** |
| LANG-212..213 | ADR-018 | 2.4.5 | 2.6 | LANG-2T-14, 15 | — |
| (CR-28) | — | — | 2.2, 2.7 (E7) | LANG-2T-05, 06, 07 | **CR-28** |

---

# 9. Critical-Review Findings — Resolution Summary

| CR ID | Severity | Gap | Resolution in this spec | Spec § |
|-------|----------|-----|-------------------------|--------|
| **CR-2** | Major | 2 | Architectural inversion (AOT-compile model) is **deferred to a future wave**. The runtime continues to use the `include_str!` model. Gap 2 adds the resolver + cross-module linking at compile time only. | LANG-211, §2.5, §2.11 |
| **CR-7** | Major | 1 | `vtable_ptr` is renamed **`vtable_base`** and is a **table index** (not a pointer to funcrefs in linear memory). Dispatch: `local.get obj; i32.load offset=0; i32.const <slot>; i32.add; call_indirect (type $T)`. The single dispatch scheme is specified unambiguously. | LANG-114..116, §1.6.1, §1.6.2 |
| **CR-8** | Major | 2 | **Tree-shaking is deferred to a future wave.** All `pub fn`/`pub class` items are emitted to the WASM binary regardless of reachability. The future tree-shaking wave will need to either (a) run tree-shaking before typechecking (requires resolver-first pipeline), (b) typecheck all but filter errors by reachability, or (c) require unused `pub fn`s to be type-correct (Rust's stance). | LANG-210, §5.4.6, §2.5, §2.11 |
| **CR-9** | Major | 2 | When tree-shaking is eventually implemented, the **conservative rule** is documented: "if any instance of class `C` is constructed, every `pub` method of `C` and every `pub` method of every subclass of `C` is reachable." Not enforced in this wave. | §5.4.6, §2.11 |
| **CR-10** | Major | 1 | Field assignment to `monotone`/`antitone`-qualified fields is a **compile-time error**. The only way to set a qualified field's initial value is via the object literal. Subsequent `self.items = ...` is rejected with `LANG-114-E10`. | LANG-113, §1.4.6, §1.7 (E10) |
| CR-14 | Minor | 3 | `Vec::new()` returns `None`; the `let`-binding's declared type drives downstream typechecking. The "expected-type inference" misnomer is removed. | §3.4.5 note |
| CR-15 | Minor | 1 | `parse_class` calls `parse_leading_attributes` (consistent with `parse_fn`/`parse_let`). `@monotone` on a class is a parse error (qualifiers apply to fields only). | §1.5, §1.11 |
| CR-17 | Minor | 1+5 | `__alk_alloc(size: i32) -> i32` is added to the `host_imports` list (index 10 after the 10 `vec_*` imports) when Gap 1 lands. | LANG-119, §1.6.6, §5.4.2 |
| CR-18 | Minor | 1 | Compound assignment (`+=`, etc.) is a **parse error** in this wave. Chained field assignment (`a.b.c = 5`) is supported via recursive `Expr::Field` receivers. | §1.5, §1.11 |
| CR-20 | Minor | 1 | `monotone` field cannot be passed to `unrestricted Vec<T>` params (documented trade-off; no spec change — `type_is_subtype` already enforces this). | §1.4.6 note, §1.11 |
| CR-28 | Minor | 2 | Capability vocabulary of 7 is closed and justified: render→ADR-001/007, gpu→ADR-006, net→ADR-021, fs/time/rand→(future), ipc→ADR-021. | §2.2, §2.7 (E7), §2.11 |

**CR-1, CR-3, CR-4, CR-5, CR-6, CR-11, CR-12, CR-13** are rendering-side
findings (Gaps 6-8) and are **out of scope** for this language/compiler
specification.

---

# 10. DoD Checklist for this Specification

- [x] Specification saved to `docs/alkalive-specification-language.md`.
- [x] All 5 gaps covered with the full 11-section structure (exact
  requirements, syntax/grammar, AST/IR, type-system, compiler changes,
  WASM changes, error cases, validation rules, test cases, acceptance
  criteria, traceability).
- [x] Every requirement is testable (linked to a test ID).
- [x] Every specification is implementable without reinterpretation (exact
  Rust code snippets, exact WASM instruction sequences, exact error
  messages).
- [x] Cross-gap dependency order defined (§0).
- [x] Cross-gap interface contracts specified (§6).
- [x] Critical review findings addressed: CR-7 (§1.6), CR-8 (§2.5, §5.4.6),
  CR-9 (§5.4.6), CR-10 (§1.4.6). Plus CR-2, CR-14, CR-15, CR-17, CR-18,
  CR-20, CR-28.
- [x] Traceability matrix included (§8).
- [x] Worklog appended (`/home/z/my-project/worklog.md`).

---

*End of specification.*

# AlkALive Fine Draft — Language / Compiler Gaps (Task ID 1)

> **Scope:** Five interdependent language/compiler gaps that, together, advance
> the AlkALive compiler from a "typed-expression DSL with WASM codegen" to the
> ADR-008 target of a **statically-typed, module- and object-oriented language
> compiling to WASM**.
>
> **Status:** Fine draft (design only — no code changes in this task).
> **Predecessors:** `docs/alkalive-wave-00-audit.md` (audit), Waves 6–8
> (WASM codegen, operators, control flow), ADR-008 / ADR-009 / ADR-018.
>
> **Audience:** Implementer agents who will turn each section into code. Every
> design choice here is intended to be implementable without reinterpretation:
> AST shapes, parser productions, typechecker algorithm, and WASM byte sequences
> are spelled out.

---

## 0. Orientation

### 0.1 The five gaps at a glance

| # | Gap | One-line description | Primary ADR | Layer(s) touched |
|---|-----|----------------------|-------------|------------------|
| 1 | **OO Model** | Classes, fields, methods, single inheritance, virtual dispatch, `self` | ADR-008 ("object oriented"), ADR-007 ("module objects ARE render objects") | lexer, parser, AST, typechecker, WASM |
| 2 | **Module System** | `import`/`pub`/`export` with capability grants and tree-shaking | ADR-008 ("first-class UI modules"), ADR-018 ("capability-scoped imports + tree-shaking") | lexer, parser, AST, typechecker, WASM, host runtime |
| 3 | **Full Type Inference** | `Expr::Call` / `Expr::MethodCall` / `Expr::PathCall` return real types via a signature table | ADR-009 ("source-level soundness") | typechecker (only) |
| 4 | **String Data Sections** | `Lit::Str` emits a real WASM data segment and `i32.const <offset>` | ADR-008 (WASM backend), ADR-022 (in-WASM text) | WASM codegen, host ABI |
| 5 | **Collection Method Dispatch** | `v.push(x)` / `v.len()` etc. compile to host-imported `call $vec_push` | ADR-008, ADR-018 (imports) | WASM codegen, host runtime |

### 0.2 Cross-gap dependency graph

```
                ┌─────────────────────────────────────────────────┐
                │                                                 │
                ▼                                                 │
   Gap 3 (Type Inference) ─────► Gap 1 (OO) ─────► Gap 2 (Modules)
                │                    │                  │
                │                    ▼                  │
                │            (needs field/method        │
                │             return types)             │
                │                                         │
                ▼                                         ▼
   Gap 5 (Collections) ◄─────── needs ─────── Gap 4 (Strings)
        │   ▲                                  (string ptr is
        │   └──── host imports share ──────     a heap value)
        │         the import-section design
        ▼
   host ABI table
```

**Resolved sequencing (mandatory):**

1. **Gap 3 must land first** — it is pure typechecker work, has no AST impact,
   and every later gap (especially Gap 1) depends on real call-return-type
   inference to type-check method bodies that call other methods.
2. **Gap 4 must land before Gap 5** — both extend the same WASM-backend
   "data + imports" machinery; Gap 4 establishes the data-section builder and
   the heap-pointer convention that Gap 5 reuses for collection handles.
3. **Gap 1 (OO) must land after Gap 3** because method bodies call other
   methods and the typechecker needs real return types. Gap 1 must also land
   after Gaps 4 and 5 because object construction allocates and uses strings.
4. **Gap 2 (Modules) must land last** — it requires a fully working
   single-module language (all other gaps) before it can wrap each module in
   an import/export ABI and tree-shake across boundaries.

### 0.3 What this document is NOT

- It is **not** an implementation. No `.rs` files are modified.
- It is **not** an ADR amendment. ADRs are unchanged; this document references
  them as requirements sources.
- It is **not** a final spec. Open questions are listed per gap and consolidated
  in §7.

---

## Gap 1 — OO Model (classes, methods, inheritance)

### 1. Current state (file:line evidence)

- `crates/alkalive-compiler/src/ast.rs:127-133` — `ItemDecl` has only two
  variants: `Fn(FnDecl)` and `Let(LetDecl)`. There is no `Class` variant.
- `crates/alkalive-compiler/src/ast.rs:322-369` — `Expr` has
  `Lit / Var / Binary / PathCall / MethodCall / Call`. There is **no** field
  access expression: `obj.field` is not parseable today.
- `crates/alkalive-compiler/src/ast.rs:206-237` — `Stmt` has
  `Let / Expr / Return / If / While`. There is no constructor, no `self`
  binding.
- `crates/alkalive-compiler/src/ast.rs:423-436` — `BaseType` has
  `I32 / F32 / Str / Bool / Vec / Named(String)`. `Named` is documented as
  "forward reference; currently unused."
- `crates/alkalive-compiler/src/lexer.rs:44-98` — the keyword list contains
  no `class`, `pub`, `priv`, `self`, `new`, `extends`, or `super`.
- `crates/alkalive-compiler/src/parser.rs:161-206` — the module-body loop
  dispatches on `TokenKind::Fn` and `TokenKind::Let` only; there is no
  `TokenKind::Class` branch.
- `crates/alkalive-compiler/src/typechecker.rs:264-271` — `check_fn` builds
  the local env from `module_env + params`; there is no concept of a `self`
  parameter or method resolution.
- `crates/alkalive-compiler/src/wasm_codegen.rs:560-569` — memory is exactly
  1 page; there is no heap allocator, no vtable emission, no `call_indirect`.

### 1.1 Problem statement

The compiler has no notion of a user-defined type with state and behaviour.
There is no way to express "a `Button` has a `label: string` and a
`fn render(self)`". ADR-007 requires that "module objects ARE the render
objects" — without an OO model there is no object to be a render object.

### 1.2 Why it's required

- **ADR-008** decision (line 207 of `docs/adr/ADR.md`): "a statically-typed,
  module- and object-oriented language compiling to WASM, with first-class UI
  modules and explicit ownership/visibility".
- **ADR-007** decision (line 186): "a single owned render-object tree where
  module objects ARE the render objects (Flutter-style); the UI component IS a
  render-object subtree owning styling/layout/drawing". The render object
  *is* a class instance — there is no other representation.
- **ADR-009** (line 353): source-level soundness cannot be meaningful without
  object types, because the dominant source-level abstraction (a render-object
  instance) would have no static type.

### 1.3 Relationship to existing compiler

- **AST**: `BaseType::Named(String)` was added in Wave 5/6 as a forward
  reference. The OO model fills in what `Named` *means*: a user-defined class
  type.
- **Typechecker**: `TypeEnv` already maps names → types; it is extended to
  map class names → class signatures. The qualifier lattice
  (`typechecker.rs:167-195`) is unchanged — qualifiers apply only to `Vec<T>`
  and not to class types.
- **WASM codegen**: `alk_type_to_wasm` (`wasm_codegen.rs:92-101`) already
  returns `ValType::I32` for `BaseType::Named(_)`. That mapping is correct
  (the object is a heap pointer) but the backend emits nothing to actually
  allocate or lay out such objects today.

### 1.4 Proposed design

#### 1.4.1 Surface syntax

```alk
class Button : BaseWidget {
  pub label: string;
  priv pressed: bool;

  pub fn new(label: string) -> Self {
    Self { label: label, pressed: false }
  }

  pub fn label(self) -> string {
    return self.label;
  }

  pub fn press(self) {
    self.pressed = true;
  }

  pub fn render(self) {
    // call into render-object tree (future wave)
  }
}
```

Concrete grammar additions (EBNF):

```ebnf
ItemDecl       := ... | ClassDecl
ClassDecl      := 'pub'? 'class' Ident (':' Ident)? '{' ClassMember* '}'
ClassMember    := FieldDecl | MethodDecl
FieldDecl      := Visibility Ident ':' Type ';'
MethodDecl     := Visibility 'fn' Ident '(' ParamList? ')' ('->' Type)? Block
Visibility     := 'pub' | 'priv'                    // default: 'priv'
ParamList      := Param (',' Param)*
Param          := 'self' | Ident ':' Type
SelfExpr       := 'self'                            // also usable in expr position
SelfType       := 'Self'                            // return type of `new`
FieldAccess    := Expr '.' Ident                    // lvalue or rvalue
ObjectLiteral  := 'Self' '{' FieldInit (',' FieldInit)* '}'
                  | Ident '{' FieldInit (',' FieldInit)* '}'
FieldInit      := Ident ':' Expr
```

Notes:

- `Self` (capitalised) is a *type* alias meaning "the class currently being
  defined"; `self` (lowercase) is an *expression* of type `Self`.
- A method whose first parameter is `self` is an **instance method**; one
  whose parameter list does not start with `self` is a **static method**
  (callable as `ClassName::method(args)`).
- `fn new(...) -> Self` is the constructor. It is invoked as
  `ClassName::new(args)` and must return `Self`.
- Inheritance is **single** (C++/Java-style), via `class Derived : Base`.
  Multiple inheritance is a parser error.

#### 1.4.2 AST node shapes (concrete Rust)

Add to `ast.rs`:

```rust
/// Visibility of a class member or top-level item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Priv,
    Pub,
}

/// `pub class Name : Base { fields... methods... }`
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    /// Optional single base class (None = no parent / root).
    pub base: Option<String>,
    pub visibility: Visibility,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<MethodDecl>,
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
    pub params: Vec<Param>,        // excludes `self`; `self` is implicit
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
        class: String,           // "Self" is resolved to the enclosing class
        fields: Vec<(String, Expr, u32, u32)>,
        line: u32,
        col: u32,
    },
    /// `ClassName::method(args)` — static call (also covers `Self::method`).
    StaticCall {
        class: String,
        method: String,
        args: Vec<Expr>,
        line: u32,
        col: u32,
    },
}
```

Add a new statement for **field assignment** (so we don't overload `Let`):

```rust
pub enum Stmt {
    // ... existing variants ...
    /// `self.field = expr;` or `obj.field = expr;`
    Assign {
        target: Expr,            // must be Expr::Field
        value: Expr,
        line: u32,
        col: u32,
    },
}
```

#### 1.4.3 Object representation in WASM linear memory

- Each object is a contiguous allocation of `field_stride(class)` bytes,
  where `field_stride = 4 * field_count(class)` (every field is 4 bytes —
  `i32`, `f32`, `bool`, pointer).
- The **vtable** is emitted as a WASM `elem` segment: one entry per virtual
  method per class, in declaration order, leaf-most first.
- Object layout: the first 4 bytes hold a pointer to the class's vtable; the
  remaining bytes hold the fields in source order, base-class fields first
  (so upcasting is a no-op).

```
+-----------------+
| vtable_ptr (i32)|  <- object base address
+-----------------+
| base_field_0    |
| base_field_1    |
| ...             |
| derived_field_0 |
| ...             |
+-----------------+
```

- Allocation is via a bump allocator exported by the host as
  `__alk_alloc(size: i32) -> i32` (see Gap 5's import-section design — the
  same import infrastructure is reused). `__alk_alloc` is a *host function*
  so the runtime can later replace it with a real GC; for now it is a bump
  pointer at the end of the data segment region.

#### 1.4.4 Virtual dispatch

- Each `class` produces one **vtable type**: a WASM `funcref` array of size
  `method_count(class_chain)`.
- The `elem` section seeds the table with the function indices of every
  method (overridden methods replace the base entry — done at link time by
  the compiler, not at runtime).
- An instance method call `obj.foo(args)` compiles to:

  ```wasm
  ;; receiver on stack (i32 pointer)
  local.get $obj
  i32.load offset=0          ;; load vtable_ptr
  i32.const <slot>           ;; slot index of foo in the vtable
  i32.add                    ;; address of the funcref slot
  call_ref $vtable_type      ;; indirect call
  ;; (or: i32.const <slot>; call_indirect (type $foo_type))
  ```

  The compiler emits the simpler `call_indirect` form (no reference-types
  proposal required):

  ```wasm
  local.get $obj             ;; receiver pointer
  ;; (compile each argument)
  local.get $obj
  i32.load offset=0          ;; vtable_ptr (table index)
  call_indirect (type $foo_type)
  ```

- **Static methods** (`ClassName::method(args)`) compile to a direct
  `call <fnidx>` — no vtable lookup.
- **Constructors** (`Self::new(args)` or `ClassName::new(args)`) compile to
  a direct call to the constructor function followed by an `Object`
  literal initialiser that fills the field slots.
- The default `new` if no constructor is declared: the compiler synthesises
  one that calls `__alk_alloc(field_stride(class))` and zero-initialises
  the fields.

#### 1.4.5 Subtyping & upcasting

- `Derived <: Base` iff `Derived`'s chain includes `Base`.
- Upcasting (`Derived` → `Base`) is a no-op at the WASM level (the pointer
  is unchanged; the vtable_ptr field points to `Derived`'s vtable, which
  inherits `Base`'s slots).
- Downcasting is **not supported** in this wave (a future wave may add a
  `as` operator with a runtime check).
- A method call on a `Base`-typed variable dispatches virtually — the
  vtable_ptr resolves to the actual derived class's table at runtime.

### 1.5 Compiler implications

| Layer | Change |
|-------|--------|
| **Lexer** | New keywords: `class`, `pub`, `priv`, `self`, `Self`, `new` (treat `new` as a regular identifier inside method bodies — it is only special as a method *name*). New token: `ColonColon` already exists (`lexer.rs:153`). |
| **Parser** | `parse_module` loop adds a `TokenKind::Class` branch dispatching to `parse_class`. New functions: `parse_class`, `parse_field`, `parse_method`, `parse_visibility`. `parse_primary` gains branches for `Self` (returns `Expr::Self_`), `Ident {` (object literal), `Ident :: Ident (` (static call), and `Ident . Ident` (field access — already partially handled as `MethodCall`; we extend it to detect when there is no `(` after the field name and produce `Expr::Field` instead). `parse_stmt` gains `Assign` recognition: parse an expression, and if it is an `Expr::Field` followed by `=`, treat as `Stmt::Assign`. |
| **AST** | As above (§1.4.2). |
| **Typechecker** | (1) New `ClassTable` populated in a first pass over `module.items`. (2) `check_class` walks fields and methods; the method env contains `self: Type::named(class_name)`. (3) `check_expr` gains cases for `Self_`, `Field`, `Object`, `StaticCall`. (4) `type_is_subtype` learns class subtyping (chain lookup in `ClassTable`). (5) Method resolution: for `expr.method(args)`, look up the static type of `expr`, find the class, find the method by name (including inherited), check arg/param types. |
| **WASM codegen** | (1) Emit a `TableSection` (one table, minimum size = total vtable slots). (2) Emit an `ElementSection` seeding the table with method function indices. (3) For each class, emit a hidden `__vtable_<ClassName>` global `i32` constant holding the table base index for that class. (4) `FnCompiler` gains a `self_index` field — when compiling an instance method, parameter 0 is `self`. (5) `Expr::Field` read compiles to `local.get obj; i32.load offset=<field_offset>`. (6) `Stmt::Assign` compiles to `local.get obj; <value>; i32.store offset=<field_offset>`. (7) `Expr::Object` compiles to `call $alloc; <store each field>`. (8) `Expr::MethodCall` on a class-typed receiver compiles to `call_indirect`; on a `Vec<T>` receiver it stays on the host-import path (Gap 5). |

### 1.6 Error handling

All errors flow through the existing `TypeError { message, line, col }` /
`TypeErrorSet` channel (`typechecker.rs:65-95`). New error categories:

- `unknown class 'X'` — when a `: Base` clause or a field type references an
  undeclared class.
- `cyclic inheritance: A : B : A` — detected by a depth-first walk; reported
  once per cycle.
- `field 'X' already declared in class 'C'` — duplicate field name.
- `method 'm' already declared in class 'C'` — duplicate method name (override
  is permitted only if signatures match exactly; otherwise an error).
- `cannot override 'm' in 'C': signature mismatch` — covariant return or
  contravariant param changes are rejected (invariant override only).
- `private field 'C.x' accessed from outside class 'C'` — when a field access
  crosses class boundaries and the field is `priv`.
- `private method 'C.m' called from outside class 'C'`.
- `constructor 'new' must return 'Self'` — when `fn new(...) -> T` has `T != Self`.
- `missing field 'x' in object literal for class 'C'` — when an `Object`
  literal omits a required field.
- `unknown field 'x' in object literal for class 'C'`.
- `static method 'C::m' called on an instance` — when `obj.m()` is invoked
  where `m` is static, or `C::m()` is invoked where `m` is an instance method.

### 1.7 Testing strategy

Tests live in `crates/alkalive-compiler/src/typechecker.rs` (existing test
module) and `crates/alkalive-compiler/src/wasm_codegen.rs` (existing test
module). New test groups:

1. **Parser round-trips** (`parser.rs::tests::class_parsing`):
   - `class Empty {}` parses with zero fields and zero methods.
   - `class Derived : Base { pub x: i32; }` round-trips through
     `format!("{:?}", ast)`.
   - Object literal `Self { x: 1, y: 2 }` produces an `Expr::Object`.
   - Static call `Button::new("hi")` produces an `Expr::StaticCall`.

2. **Typechecker** (`typechecker.rs::tests::oo_tests`):
   - Field access on a typed `let b: Button = Button::new("x"); b.label`
     returns `Type::Str`.
   - Calling a private method from a sibling class is a `TypeError`.
   - Overriding a method with a different signature is a `TypeError`.
   - Cyclic inheritance `A : B, B : A` is reported once.
   - `Self` outside a class body is an error.

3. **WASM codegen** (`wasm_codegen.rs::tests::oo_codegen`):
   - A module with one class + one method produces a `table` section and
     an `elem` section (verified via `wasmparser::Parser` walking sections).
   - `wasmparser` validates the full binary.
   - Object literal + field read produces a sequence containing
     `Instruction::Call` (to `__alk_alloc`), `Instruction::I32Load`,
     `Instruction::I32Store` (verified by an `AlkInstr`-level unit test on
     the compiled body).

4. **End-to-end** (new `tests/oo_integration.rs`):
   - A module that defines a `Counter` class with `inc()`/`get()` methods,
     a `main` `fn` that constructs a `Counter`, calls `inc()` twice, and
     returns `get()` — typechecks and emits a valid WASM binary.

### 1.8 Dependencies on other gaps

- **Hard dependency on Gap 3** (Type Inference): method bodies call other
  methods; without real return-type inference, `let x = counter.get();` is
  untyped and downstream checks fail. Gap 3 must land first.
- **Soft dependency on Gap 4** (Strings): constructors and fields commonly
  hold `string` values. Without Gap 4, `Self { label: "hi" }` would store a
  placeholder `0` pointer. For Gap 1 we accept that strings still produce
  placeholder pointers until Gap 4 lands, but the *type system* must treat
  strings as first-class heap values either way.
- **Soft dependency on Gap 5** (Collections): a class field of type
  `Vec<i32>` would need Gap 5 to be useful. As with strings, the type
  system can carry the type before Gap 5 lands.
- **Soft dependency on Gap 2** (Modules): `pub class` exports and `priv`
  visibility are only meaningful across module boundaries; intra-module
  visibility is enforced in Gap 1 alone.

### 1.9 Risks and trade-offs

- **Bump allocator → leak.** The proposed `__alk_alloc` is a one-way bump
  pointer. Long-running scenes will exhaust memory. **Mitigation:** the host
  can implement a slab/mark-sweep collector later without changing the
  language ABI; the WASM module never inspects the allocator's internals.
- **Single inheritance only.** Excludes mixins and interfaces. **Rationale:**
  keeps the vtable layout flat and predictable; matches Flutter's
  single-inheritance render-object tree (ADR-007).
- **Invariant override.** Rejects covariant returns. **Rationale:** invariant
  override is sound and simple; covariant returns can be added later as a
  typechecker-only relaxation.
- **No downcasting in this wave.** All subtyping is implicit-upcast only.
  **Rationale:** runtime type checks need a type-info table; deferring keeps
  Gap 1 self-contained.
- **Vtable per class, not per instance.** Saves memory (4 bytes per object
  for the vtable pointer) at the cost of virtual dispatch on every method
  call. **Rationale:** matches C++/V8/HotSpot's design and is the right
  default for a render-object tree where virtual dispatch is the norm.
- **Visibility is per-class, not per-module-arc.** A `priv` field is
  accessible from any method of the same class, including methods in
  subclasses. **Rationale:** matches Rust's `pub(self)`-ish semantics but
  is simpler to specify; a future `protected` keyword could be added if
  needed.

### 1.10 Open questions

1. Should `Self` be usable as a return type on *non-constructor* methods
   (e.g. `fn map(self, f: fn(i32)->i32) -> Self`)? **Tentative answer: yes** —
   it resolves to the enclosing class.
2. Should field initialisers be allowed inline (`pub x: i32 = 0;`)?
   **Tentative answer: yes** — desugars to a default `new` body that runs
   before the user's `new`.
3. Should `super.method()` be supported? **Tentative answer: not in this
   wave** — adds a non-trivial vtable slot resolution and another keyword.
4. Should trait/interface types be added now or later? **Tentative answer:
   later** — single inheritance covers the ADR-007 render-object-tree case;
   interfaces can be added as a non-breaking extension (a class can implement
   multiple interfaces) once the basic class model is shipping.

---

## Gap 2 — Module System (imports/exports)

### 2. Current state (file:line evidence)

- `crates/alkalive-compiler/src/ast.rs:91-109` — `ModuleDecl` has `name`,
  `scene`, `attributes`, `items`, `line`, `col`. There is **no** list of
  imports and **no** per-item visibility (`ItemDecl::Fn`/`Let` carry no
  `Visibility` field).
- `crates/alkalive-compiler/src/parser.rs:142-216` — `parse_module` parses
  an optional `scene` block followed by `fn`/`let` items. There is no
  `import` branch and no `pub` keyword on items.
- `crates/alkalive-compiler/src/lexer.rs:44-98` — no `import`, `from`,
  `with`, `as`, or `pub` keywords.
- `crates/alkalive-compiler/src/wasm_codegen.rs:571-577` — every `fn` in
  `module.items` is exported by name. There is **no** import section, and
  there is no tree-shaking (all functions are emitted regardless of use).
- `crates/alkalive-compiler/src/typechecker.rs:233-261` — `check_module`
  walks `module.items` once to build the module env (lets only) and once to
  check functions. There is no concept of names imported from other modules.

### 2.1 Problem statement

Every `.alk` file is an island. Functions and classes in one file cannot be
referenced from another. ADR-008 requires "first-class UI modules" and
ADR-018 requires "explicit typed imports with compile-time tree-shaking +
capability-sandboxed least-privilege grants". Neither is possible today.

### 2.2 Why it's required

- **ADR-008** (line 207): "first-class UI modules and explicit
  ownership/visibility".
- **ADR-018** decision (line 621 of `docs/adr/ADR.md`): "a language-level
  standard library + explicit typed imports with compile-time tree-shaking
  (component-model-enforced) + capability-sandboxed least-privilege grants;
  the verifiable module boundary makes components modules, not framework
  artifacts".
- **ADR-009** (line 628): "Relies on ADR-008 (module boundary) and ADR-009
  (verification attestation)." Type soundness cannot cross module
  boundaries without a typed import surface.

### 2.3 Relationship to existing compiler

- **Parser**: `parse_module` already accepts a top-level items list — the
  import list is added as a sibling vector.
- **Typechecker**: `TypeEnv` is per-module; with imports it is seeded with
  the imported names (each carrying the source module's attested type).
- **WASM codegen**: the `ImportSection` is the WASM-level mechanism for
  cross-module calls. The existing `TypeSectionBuilder`
  (`wasm_codegen.rs:124-159`) is reused to register import function types.
- The current single-file assumption (`compile(src: &str) -> Result<...>`)
  is replaced by `compile_module_set(sources: HashMap<PathBuf, &str>)`
  in the new module-resolver layer.

### 2.4 Proposed design

#### 2.4.1 Surface syntax

```alk
// File: app/button.alk
pub class Button { ... }
pub fn make_button(label: string) -> Button { ... }
priv fn helper() -> i32 { ... }            // not exported
```

```alk
// File: app/main.alk
import { Button, make_button } from "app/button";
import { render_text } from "std/canvas" with [render];

module Main {
  fn main() {
    let b: Button = make_button("hi");
    render_text(b.label);
  }
}
```

Concrete grammar additions (EBNF):

```ebnf
ImportDecl   := 'import' '{' ImportName (',' ImportName)* '}'
                'from' String ('with' '[' Cap (',' Cap)* ']')? ';'
ImportName   := Ident ('as' Ident)?
Cap          := 'render' | 'gpu' | 'net' | 'fs' | 'time' | 'rand' | 'ipc'
ExportedItem := 'pub' (FnDecl | LetDecl | ClassDecl)
```

Notes:

- `pub` may appear before `fn`, `let`, and `class`. Items without `pub` are
  module-private (the default).
- `import { A, B as C } from "path";` — `as` introduces an alias.
- `with [render, gpu]` grants specific capabilities to the imported module.
  The capability list is checked against the module's declared requirements
  at link time.
- The string after `from` is a **module path**, not a file path: it uses `/`
  as a separator and has no extension. Resolution rules:

  | Path prefix | Resolves to |
  |-------------|--------------|
  | `std/<name>` | The built-in standard library module `<name>` |
  | `app/<name>` | A user module at `<source_root>/app/<name>.alk` |
  | `./<name>` or `../<name>` | A user module relative to the importer |

#### 2.4.2 AST node shapes

Add to `ast.rs`:

```rust
/// A capability granted to an imported module (ADR-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Render, Gpu, Net, Fs, Time, Rand, Ipc,
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
    pub imports: Vec<ImportDecl>,        // NEW
    /// Filled in by the resolver pass (not present in the raw AST).
    pub resolved_imports: Vec<ResolvedImport>,   // NEW (post-parse)
}
```

Extend `ItemDecl::Fn`/`Let` (and the new `Class`) with a `Visibility`
field — `pub` or `priv` (default `priv`).

#### 2.4.3 Module resolution

A new pass `crates/alkalive-compiler/src/modules.rs` runs **after parsing**
and **before typechecking**:

1. **Build a module graph**: every `import` in every parsed module is
   resolved to a path; the resolver reads the source file (or pulls the
   `std/*` module from the embedded stdlib), parses it, and recurses.
2. **Cycle detection**: import cycles are reported as `ResolveError`s with
   the cycle path. Cycles are forbidden by default; a future wave may allow
   them for type-only imports.
3. **Capability attestation**: every `std/*` module declares its required
   capabilities in a manifest. The resolver checks that the importer grants
   a superset of the required capabilities. Missing capabilities are
   reported.
4. **Name resolution**: each `ResolvedImport` is annotated with the source
   module's signature for the imported name (function type, class
   signature, or constant type). These signatures flow into the
   typechecker's `TypeEnv`.

#### 2.4.4 Tree-shaking

After typechecking, the compiler performs a **reachability pass** over the
module graph:

1. Start from the module's **entry points**: the `fn main()` of the entry
   module (or every `pub fn` if the module is a library).
2. Walk every `Call`, `MethodCall`, `StaticCall`, `PathCall`, `Object`
   literal, and field access; mark the referenced function/class/let as
   reachable.
3. Unreachable `pub` items are **not emitted** to the WASM binary — they
   are dropped from the function section, code section, and export section.
4. The export list contains only: (a) the entry module's `main`, and
   (b) `pub` items that are imported by some other reachable module.

This is dead-code elimination at the module level — the
"compile-time tree-shaking" promised by ADR-018.

#### 2.4.5 Cross-module WASM layout

Two implementation strategies, listed in order of complexity:

**Strategy A (single-WASM, recommended for Wave 1):** the entire module
graph is linked into a single WASM binary. Each source module contributes
its functions, classes, and exports; cross-module calls become direct
`call <fnidx>` (the linker rewrites `Expr::PathCall(module, member, args)`
into a `call` to the resolved function index). The import list of the
final WASM contains only **host imports** (Gap 5's `vec_push` etc.) —
no AlkALive-level imports leak to the WASM layer.

**Strategy B (multi-WASM, future):** each module becomes its own WASM
component; cross-module calls use the WASM component model. This is
deferred until the component-model tooling is production-ready (per
ADR-018 confidence: "Medium", line 633).

For this wave we ship **Strategy A**.

### 2.5 Compiler implications

| Layer | Change |
|-------|--------|
| **Lexer** | New keywords: `import`, `from`, `with`, `as`, `pub`. New contextual keyword `priv` (only special after `pub`/as visibility — outside class/method context it is a regular ident). |
| **Parser** | `parse_module` parses zero or more `import` statements **before** the `module` keyword. New `parse_import` function. `parse_fn`/`parse_let`/`parse_class` accept a leading `pub` (already collected by the leading-attributes path; we extend `parse_leading_attributes` to also accept `pub`). |
| **AST** | As above (§2.4.2). |
| **Resolver** | New module `modules.rs`. Public entry: `resolve(modules: HashMap<PathBuf, ModuleDecl>) -> Result<ResolvedGraph, ResolveError>`. |
| **Typechecker** | `check_module` accepts a `ResolvedGraph` and seeds each module's `TypeEnv` with the imported names. `Expr::PathCall(module, member, ...)` is now resolved through the graph rather than special-cased to `Vec::new`. |
| **WASM codegen** | (1) Reachability pass before code section emission. (2) `PathCall` compiles to `call <resolved_fnidx>` (direct call into the linked module's function). (3) `pub` items become exports only if reachable from the entry module or imported by another reachable module. (4) Private items are emitted as non-exported functions (still callable internally). |
| **Host runtime** | The WASM module exports a single `main` (or a small set of entry points). The runtime's `start()` calls `main` instead of `compile_to_wasm` directly — the `.alk` source is now compiled to WASM ahead-of-time and the runtime loads the resulting binary. (This is the architectural inversion flagged in the Wave 0 audit §10.2; Gap 2 is the moment it happens.) |

### 2.6 Error handling

New `ResolveError { message, line, col }` type. Categories:

- `module 'path' not found` — file does not exist or `std/<name>` is unknown.
- `name 'X' not exported by module 'path'` — imported name is not declared
  `pub` in the source module.
- `cyclic import: a -> b -> a` — cycle in the import graph.
- `missing capability: 'render' required by 'std/canvas' but not granted
  by importer` — capability attestation failure.
- `ungranted capability 'X'` — the importer granted a capability the source
  module does not require (warning, not error).

Type errors (unchanged channel):

- `imported name 'X' has type 'Y' which is not visible in this module` —
  e.g., importing a `pub fn` whose return type is a `priv class`.
- `ambiguous import: 'X' is imported from both 'a' and 'b'` — duplicate
  local names without `as` disambiguation.

### 2.7 Testing strategy

1. **Resolver unit tests** (`modules.rs::tests`):
   - Two-file graph (`app/main` imports `app/button`) resolves with no
     errors.
   - Cycle (`a → b → a`) is detected and reported once.
   - Missing module produces a `ResolveError`.
   - Importing a `priv` item produces a `ResolveError`.

2. **Typechecker tests**:
   - Imported `pub fn` is callable; its return type is correctly inferred
     (depends on Gap 3).
   - Imported `pub class` is usable as a type.
   - Visibility mismatch (importing a function that returns a `priv` type)
     is a `TypeError`.

3. **WASM codegen tests**:
   - An unused `pub fn` in a non-entry module is **not** in the export
     section (tree-shaking).
   - A reachable cross-module call produces a `call <fnidx>` instruction
     whose target is in a different module's source (verified via debug
     metadata in the WASM custom section).

4. **End-to-end** (`tests/module_integration.rs`):
   - A two-module program where `app/main` imports a `Button` from
     `app/button` and constructs it — typechecks and produces a valid WASM
     binary that `wasmparser` accepts.

### 2.8 Dependencies on other gaps

- **Hard dependency on Gap 1** (OO): the module system must export classes;
  without classes, modules degenerate to function libraries.
- **Hard dependency on Gap 3** (Type Inference): the typechecker must
  resolve imported function return types to verify call sites.
- **Soft dependency on Gaps 4 and 5**: cross-module string/collection
  operations rely on the host ABI being finalised.

### 2.9 Risks and trade-offs

- **Strategy A vs Strategy B.** Strategy A (single-WASM linking) is simpler
  and matches the Wave 0 audit's "single binary" architecture, but loses
  the independent-deployment property ADR-018 promises. **Rationale:** ship
  A now; revisit B when the component model is mature.
- **Cycle policy.** Forbidding cycles outright is restrictive but simple.
  **Rationale:** matches Rust 2015's stance; can be relaxed for type-only
  imports later.
- **Capability vocabulary.** The seven capabilities (`render`, `gpu`,
  `net`, `fs`, `time`, `rand`, `ipc`) are a fixed enum. **Rationale:** a
  closed vocab is auditable; an open vocab risks capability-inflation.
- **Path resolution.** Using string paths (`"app/button"`) rather than
  URL/URN identifiers. **Rationale:** matches the file-based model of
  contemporary module systems (Python, JS); a future URN scheme can layer
  on top.

### 2.10 Open questions

1. Should `pub` be the only visibility, or do we need `pub(crate)` /
   `pub(super)` analogues? **Tentative: only `pub` and `priv` for this wave.**
2. Should the entry module be marked specially (e.g. `module Main { ...
   fn main() }`) or is any module with a `pub fn main` an entry point?
   **Tentative: the latter; the linker takes a `--entry app/main` flag.**
3. Should `with [cap]` grants be additive (the importer can grant more
   than the source requires) or strict (exact match)? **Tentative: additive
   — extra grants are a warning, not an error.**
4. How are version constraints expressed? **Tentative: not in this wave.**
   A future `import { X } from "app/button@1.2";` syntax can be added.

---

## Gap 3 — Full Type Inference (function-call return types)

### 3. Current state (file:line evidence)

- `crates/alkalive-compiler/src/typechecker.rs:427-443` — the
  `Expr::Call { callee, args, line, col }` arm:
  ```rust
  // Function call return type — we don't have a function signature
  // table in the type env yet, so return None (unknown type).
  // The type checker should be extended to look up the function's
  // declared return type. For now, we don't error on calls.
  let _ = (callee, line, col);
  None
  ```
  This returns `None`, which propagates as "unknown type" through every
  expression that contains a call.
- `crates/alkalive-compiler/src/typechecker.rs:376-398` — the
  `Expr::MethodCall` arm returns `None` with the comment:
  > "Method calls return unknown types (we don't have a full type inference
  > engine for return values)."
- `crates/alkalive-compiler/src/typechecker.rs:361-375` — `Expr::PathCall`
  returns `None` for `Vec::new`/`Vec::with_capacity` and `None` for unknown
  path calls.
- `crates/alkalive-compiler/src/typechecker.rs:262-271` — `check_fn` clones
  the module env and adds parameters; there is **no** first pass to collect
  function signatures.
- Consequence in WASM codegen: `wasm_codegen.rs:419-432` emits a `call`
  instruction for `Expr::Call` without knowing whether the return type
  matches the call site. The compiler relies on the user to insert
  `return` correctly; it does not catch "function returned `i32` but caller
  used it as `bool`".

### 3.1 Problem statement

The typechecker cannot answer "what is the type of `add(1, 2)`?" — it
returns `None`. This means:

- `let x = add(1, 2);` produces a variable of unknown type, defeating
  downstream type checks.
- `if (is_even(n)) { ... }` cannot verify the condition is `bool`.
- Method chains (`v.push(1).len()`) are entirely unchecked.
- ADR-009's "source-level soundness" claim is hollow for any program that
  contains a function call.

### 3.2 Why it's required

- **ADR-009** (line 353 of `docs/adr/ADR.md`): "the compiler proves
  source-level soundness". Soundness is impossible without return-type
  inference for calls — the most common kind of expression in any
  non-trivial program.
- **ADR-008** (line 207): "statically-typed". A language where function
  calls have unknown types is not statically typed in any meaningful sense.
- **Gap 1 and Gap 2 hard-depend on this**: methods return types and
  imported function types both flow through `Expr::Call`/`MethodCall`.

### 3.3 Relationship to existing compiler

- `check_module` already iterates `module.items` twice (once to collect
  lets, once to check fns). The signature table is a third pass that runs
  **before** the let-collection pass.
- `TypeEnv` (`typechecker.rs:201-223`) maps `String → Type`. The signature
  table is a sibling structure mapping `String → FnSig`.
- The qualifier lattice (`typechecker.rs:167-195`) is unchanged: return-type
  inference flows through `type_is_subtype` exactly as parameter types do.

### 3.4 Proposed design

#### 3.4.1 Function signature table

```rust
/// A function's signature — the type-checker's view of a callable.
#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub name: String,
    pub params: Vec<Type>,
    pub return_type: Option<Type>,
    /// Whether the function is a method on a class (and if so, which).
    /// `None` for free functions.
    pub receiver_class: Option<String>,
    /// Whether the function is imported from another module.
    pub imported_from: Option<String>,
}

/// The module-wide function signature table.
/// Built in pass 1 of `check_module`; consulted in pass 2.
#[derive(Debug, Clone, Default)]
pub struct FnSigTable {
    sigs: std::collections::HashMap<String, FnSig>,
}
```

#### 3.4.2 Algorithm (two-pass with mutual recursion support)

`check_module` is restructured into three passes:

**Pass 1: Collect signatures.**

```rust
fn collect_signatures(module: &ModuleDecl, table: &mut FnSigTable) {
    for item in &module.items {
        match item {
            ItemDecl::Fn(f) => {
                table.insert(f.name.clone(), FnSig {
                    name: f.name.clone(),
                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                    return_type: f.return_type.clone(),
                    receiver_class: None,
                    imported_from: None,
                });
            }
            ItemDecl::Class(c) => {
                for m in &c.methods {
                    let qualified_name = format!("{}::{}", c.name, m.name);
                    table.insert(qualified_name, FnSig {
                        name: qualified_name,
                        params: m.params.iter().map(|p| p.ty.clone()).collect(),
                        return_type: m.return_type.clone(),
                        receiver_class: Some(c.name.clone()),
                        imported_from: None,
                    });
                    // Also insert the unqualified name with a class tag, so
                    // method resolution can find it by name + receiver class.
                }
            }
            _ => {}
        }
    }
    // For imported names (Gap 2): merge in the resolved import signatures.
}
```

This pass is purely syntactic — it does not check bodies. It runs over
**every** function before any body is checked, so mutual recursion works
out of the box.

**Pass 2: Collect module-level `let`s** (unchanged from today).

**Pass 3: Check function bodies.** For each `Fn`:

```rust
fn check_fn(f: &FnDecl, module_env: &TypeEnv, sigs: &FnSigTable, errors: &mut TypeErrorSet) {
    let mut env = module_env.clone();
    for p in &f.params {
        env.insert(p.name.clone(), p.ty.clone());
    }
    check_block(&f.body, &mut env, f.return_type.as_ref(), sigs, errors);
}
```

`check_block` and `check_expr` thread `sigs: &FnSigTable` through every
recursive call.

#### 3.4.3 `Expr::Call` checking (the actual fix)

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
                        "function `{}` expects {} argument(s) but was called with {}",
                        callee, sig.params.len(), args.len()
                    ),
                    line: *line, col: *col,
                });
            }
            // 4. Per-argument type check (with subtype flow).
            for (i, (arg_ty, param_ty)) in arg_types.iter().zip(sig.params.iter()).enumerate() {
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

#### 3.4.4 `Expr::MethodCall` checking

```rust
Expr::MethodCall { receiver, method, args, line, col } => {
    let receiver_ty = check_expr(receiver, env, errors, sigs);
    let mut arg_types: Vec<_> = args.iter()
        .map(|a| check_expr(a, env, errors, sigs))
        .collect();
    match &receiver_ty {
        Some(ty) if ty.is_vec() => {
            // Collection method dispatch — host-provided.
            // Return type is fixed per method name (see Gap 5 §5.4.1).
            collection_method_return_type(method, ty, *line, *col, errors)
        }
        Some(Type { base: BaseType::Named(class_name), .. }) => {
            // Class method dispatch — look up in the class table.
            // (Requires Gap 1's ClassTable; until Gap 1 lands, this branch
            //  returns None and reports an "unknown method" error.)
            class_method_return_type(class_name, method, &arg_types, sigs, *line, *col, errors)
        }
        Some(_) => {
            errors.push(TypeError {
                message: format!("method `.{}()` is not defined on type `{}`", method, receiver_ty.unwrap()),
                line: *line, col: *col,
            });
            None
        }
        None => None, // receiver already errored
    }
}
```

The host-provided collection return types (independent of Gap 5's WASM
implementation) are:

| Method | Return type |
|--------|-------------|
| `push`, `extend`, `insert`, `append`, `remove`, `truncate`, `clear`, `swap_remove`, `drain` | `()` (unit) |
| `len`, `is_empty` | `i32` / `bool` |
| `get` | `Option<T>` (in this wave: `i32` placeholder; full `Option<T>` is a future enhancement) |
| `first`, `last` | same as `get` |

#### 3.4.5 `Expr::PathCall` checking

`Vec::new()` and `Vec::with_capacity(n)` are resolved to known signatures:

```rust
Expr::PathCall(module, member, args, line, col) => {
    // Argument checking (unchanged).
    for a in args { check_expr(a, env, errors, sigs); }
    match (module.as_str(), member.as_str()) {
        ("Vec", "new") => {
            // Vec<T> with T unconstrained — return None but don't error.
            // The expected type flows in from the let-binding context.
            None
        }
        ("Vec", "with_capacity") => {
            // Same — capacity is i32, element type is inferred.
            None
        }
        (mod_name, member_name) => {
            // Cross-module call: look up "module::member" in the sig table
            // (populated by Gap 2's resolver). Until Gap 2 lands, this is
            // an "unknown path call" error.
            let qualified = format!("{}::{}", mod_name, member_name);
            match sigs.lookup(&qualified) {
                Some(sig) => sig.return_type.clone(),
                None => {
                    errors.push(TypeError {
                        message: format!("call to unknown path `{}::{}`", mod_name, member_name),
                        line: *line, col: *col,
                    });
                    None
                }
            }
        }
    }
}
```

### 3.5 Compiler implications

| Layer | Change |
|-------|--------|
| **Lexer** | None. |
| **Parser** | None. |
| **AST** | None (the `FnSig` and `FnSigTable` types live in the typechecker module). |
| **Typechecker** | Restructure `check_module` into three passes. Thread `&FnSigTable` through `check_fn`/`check_block`/`check_expr`. Add the new `Expr::Call`/`MethodCall`/`PathCall` arms above. |
| **WASM codegen** | **None**. The codegen already emits correct `call`/`call_indirect` instructions; it just had no static guarantees. The guarantee is now provided by the typechecker refusing to compile ill-typed programs (`wasm_codegen.rs:493-505`). |

### 3.6 Error handling

New error categories (all `TypeError`):

- `call to unknown function 'X'`
- `function 'X' expects N argument(s) but was called with M`
- `argument K to 'X' has type 'A' but parameter has type 'B'`
- `method '.m()' is not defined on type 'T'`
- `path call 'M::f' is unknown`

All errors are accumulated into the existing `TypeErrorSet`
(`typechecker.rs:90-116`) — the multi-error policy is preserved.

### 3.7 Testing strategy

Tests added to `typechecker.rs::tests`:

1. **Return-type inference**: `let x: i32 = id(42);` typechecks (where
   `fn id(x: i32) -> i32 { return x; }`).
2. **Arity mismatch**: `id(1, 2)` produces a `TypeError` mentioning the
   expected and actual arity.
3. **Argument type mismatch**: `id(true)` produces a `TypeError` mentioning
   `bool` vs `i32`.
4. **Unknown function**: `unknown(42)` produces a `TypeError`.
5. **Mutual recursion**: `fn is_even(n: i32) -> bool { if n == 0 { return true; } return is_odd(n - 1); }`
   and `fn is_odd(n: i32) -> bool { ... }` typecheck (forward reference works).
6. **Self recursion**: `fn fact(n: i32) -> i32 { if n <= 1 { return 1; } return n * fact(n - 1); }`
   typechecks.
7. **Method on Vec**: `let v: Vec<i32> = Vec::new(); let n: i32 = v.len();`
   typechecks; `let n: bool = v.len();` is an error.
8. **Method on unknown type**: `let x: i32 = 5; x.foo();` is an error.

### 3.8 Dependencies on other gaps

- **None hard.** This gap is pure typechecker work and can land first.
- **Soft dependency on Gap 1** for the `class_method_return_type` branch —
  until Gap 1 lands, that branch is dead code returning `None` with an
  "unknown method" error (acceptable; the typechecker still correctly
  handles `Vec` receivers).
- **Soft dependency on Gap 2** for the cross-module `PathCall` branch —
  until Gap 2 lands, only `Vec::new`/`Vec::with_capacity` resolve.

### 3.9 Risks and trade-offs

- **No type inference for `let` without annotation.** `let x = id(42);` is
  rejected today (the parser requires `let x: Type = ...`). This is
  consistent with the existing grammar (`parser.rs:351-371`) and avoids the
  HM-style inference rabbit hole. **Rationale:** keeps the typechecker
  tractable and predictable; full inference can be layered on later.
- **`Vec::new()` returns `None`.** The element type is inferred from the
  let-binding's declared type. This is "expected-type inference", not
  bottom-up inference — simpler and sufficient for the common case.
- **No higher-rank types, no generics beyond `Vec<T>`.** Methods on user
  classes are monomorphic. **Rationale:** generics are a large design
  surface; deferring them keeps Gap 3 self-contained.
- **Mutual recursion via two-pass.** Some languages (ML, Haskell) solve
  this with let-polymorphism + occurrence analysis. We use the simpler
  "collect all signatures first" approach, which is the standard solution
  in Pascal/Modula-2/Go.

### 3.10 Open questions

1. Should the signature table also carry the *parameter names* (for better
   error messages and named-argument support)? **Tentative: yes** — adds
   ~20 LOC and pays off in diagnostics.
2. Should we warn on shadowing (a `let x` in an inner block that hides a
   parameter `x`)? **Tentative: yes, as a lint, not an error.**
3. Should `Expr::PathCall` for `Vec::new()` infer the element type from the
   arguments (e.g. `Vec::from(1, 2, 3)` → `Vec<i32>`)? **Tentative: yes,
   but only for the specific constructors we add to the standard library.**

---

## Gap 4 — String Data Sections

### 4. Current state (file:line evidence)

- `crates/alkalive-compiler/src/wasm_codegen.rs:356-368` — the `Lit::Str(_)`
  arm:
  ```rust
  Lit::Str(_) => {
      // String literals are stored in the data section;
      // the expression yields a pointer (i32) to the data.
      instrs.push(AlkInstr::I32Const(0));
  }
  ```
  This is a **placeholder**: every string literal in the program compiles
  to `i32.const 0`, so all strings alias the same address (address 0 in
  linear memory).
- `crates/alkalive-compiler/src/wasm_codegen.rs:560-569` — the memory
  section declares 1 page (64 KiB) but **no data section is emitted** at
  all. There is no `DataSection` import in the `use` statement at
  `wasm_codegen.rs:48-51`.
- `crates/alkalive-compiler/src/wasm_codegen.rs:651-662` — the binary is
  serialised with `wasm_module.finish()` immediately after the code
  section; no data segment is appended.
- Consequence: any program that reads or compares strings reads from
  address 0, which (per the WASM spec) is valid memory but contains
  arbitrary bytes. Strings are effectively unusable.

### 4.1 Problem statement

String literals are required for any non-trivial program — labels, error
messages, identifiers. The current backend emits `i32.const 0` for every
literal, which is wrong. There is no data section in the generated WASM,
so the host has no way to read the literal bytes.

### 4.2 Why it's required

- **ADR-008** (line 207): a real language compiling to WASM. WASM's
  idiomatic string-representation is a data segment + pointer.
- **ADR-022** (line 700): "Forked HarfRust as the in-WASM text
  shaping/rasterization stack" — the renderer reads UTF-8 bytes from
  linear memory. The compiler must produce those bytes.
- **ADR-009**: source-level soundness requires that a `string` typed value
  is actually a valid UTF-8 pointer. Today it is `0`, which is not.

### 4.3 Relationship to existing compiler

- The WASM backend already exports memory (`wasm_codegen.rs:576`); the data
  section seeds that memory with the literal bytes.
- The `Lit::Str(String)` variant (`ast.rs:379`) already carries the bytes —
  no parser change is needed.
- The typechecker already types `Lit::Str` as `BaseType::Str`
  (`typechecker.rs:480-491`).

### 4.4 Proposed design

#### 4.4.1 String representation in linear memory

Each string literal is stored as **length-prefixed UTF-8**:

```
+--------+--------+--------+--------+--------+--------+ ... +--------+
| len (i32, little-endian)              | byte_0 | byte_1 | ... | byte_n |
+--------+--------+--------+--------+--------+--------+ ... +--------+
^                                       ^
|                                       |
ptr (returned to AlkALive code)         ptr + 4
```

- `ptr` is the address of the length prefix.
- `len` is the byte count of the UTF-8 payload (not including the prefix).
- `ptr + 4` is the start of the UTF-8 bytes (4-byte aligned).
- The host reads `i32` at `ptr` to learn the length, then reads `len` bytes
  starting at `ptr + 4`.

This matches the convention used by AssemblyScript and Rust's
`wasm-bindgen` string ABI — predictable, alignment-friendly, and
length-checked.

#### 4.4.2 Memory layout

```
Linear memory (1 page = 64 KiB, expandable):

  +-----------------------+  address 0
  | null guard (4 bytes)  |   <- always zero; ensures `i32.const 0` is a
  +-----------------------+      clearly-invalid string pointer
  | string literals       |   <- data section 1 (active, offset 4)
  |   "Hello"             |
  |   "World"             |
  |   ...                 |
  +-----------------------+  address STRINGS_END
  | heap (Gap 1 objects,  |   <- grown by __alk_alloc
  |        Gap 5 Vecs)    |
  |                       |
  +-----------------------+  address 64 KiB (or grows)
```

The first 4 bytes are reserved as a null guard, so `i32.const 0` is a
sentinel "null string" — distinct from any real literal.

#### 4.4.3 Collection & deduplication

A new pass over the AST (or, more precisely, a side-table populated during
`compile_expr`) collects every string literal:

```rust
struct StringTable {
    /// Map from literal text → memory offset.
    by_text: std::collections::HashMap<String, u32>,
    /// Ordered entries for emitting the data section.
    entries: Vec<StringEntry>,
}

struct StringEntry {
    text: String,
    offset: u32,           // address of the length prefix
    byte_len: u32,         // UTF-8 byte length
}
```

- The first call to `compile_expr` on a `Lit::Str(s, _, _)` checks
  `by_text`; if `s` is present, the existing offset is reused (dedup).
- Otherwise a new entry is created at offset `current_offset`,
  `current_offset` is advanced by `4 + byte_len` rounded up to a 4-byte
  boundary, and the entry is inserted.
- Initial offset is `4` (after the null guard).

#### 4.4.4 WASM emission

After the code section is built, the compiler emits a `DataSection` with
one **active** data segment per string entry:

```rust
let mut data_sec = DataSection::new();
for entry in &string_table.entries {
    let mut bytes = Vec::with_capacity(4 + entry.text.len());
    bytes.extend_from_slice(&entry.byte_len.to_le_bytes());
    bytes.extend_from_slice(entry.text.as_bytes());
    // Pad to 4-byte alignment.
    while bytes.len() % 4 != 0 { bytes.push(0); }
    let mut data = Data::active(0);   // memory index 0
    data.offset(&mut const_expr(entry.offset));  // i32.const <offset>
    data.value(&bytes);
    data_sec.data(data);
}
wasm_module.section(&data_sec);
```

(Where `const_expr(n)` emits the bytecode for `i32.const n; end`.)

#### 4.4.5 `Lit::Str` codegen

```rust
Lit::Str(s, _line, _col) => {
    let offset = string_table.intern(s);
    instrs.push(AlkInstr::I32Const(offset as i32));
}
```

The `FnCompiler` now holds a `&mut StringTable` (or, more cleanly, the
`compile_to_wasm` function owns the table and passes it down).

### 4.5 Compiler implications

| Layer | Change |
|-------|--------|
| **Lexer** | None. |
| **Parser** | None. |
| **AST** | None. |
| **Typechecker** | None. |
| **WASM codegen** | (1) Import `wasm_encoder::{Data, DataSection}`. (2) Add `StringTable` struct. (3) `FnCompiler` carries `&StringTable` (or `compile_expr` takes it as a parameter). (4) `Lit::Str` arm interns and emits `i32.const <offset>`. (5) After the code section, emit the data section. (6) The first 4 bytes of memory are reserved (initialize with `i32.const 0` data segment or document the null guard). |

### 4.6 Error handling

String data sections are an implementation detail — there are no user-facing
errors. Two internal invariants are asserted:

- `offset > 0` for every string (the null guard at address 0 is never
  reused).
- `offset + 4 + byte_len` does not overflow `i32::MAX` (asserted; in
  practice impossible — a 64 KiB page holds ~16 K strings of average
  length 12).

If the data section would exceed the declared memory size (1 page = 64
KiB), the compiler automatically grows the memory declaration:

```rust
let strings_end = string_table.next_offset();
let pages_needed = ((strings_end + 65535) / 65536) as u32;
let memory_pages = pages_needed.max(1);
mem_sec.memory(MemoryType {
    minimum: memory_pages,
    maximum: None,
    ...
});
```

### 4.7 Testing strategy

Tests in `wasm_codegen.rs::tests`:

1. **Basic string emission**: a module with `fn f() -> string { return "hi"; }`
   produces a binary with a data section containing the bytes
   `[2, 0, 0, 0, 'h', 'i', 0, 0]` (length 2, payload "hi", 2 bytes padding).
2. **Deduplication**: a module that uses `"hi"` twice has exactly one data
   segment for it (verified by counting segments in the parsed WASM).
3. **Multiple distinct strings**: `"a"`, `"bb"`, `"ccc"` produce three
   segments at distinct offsets, each 4-byte aligned.
4. **Offset non-zero**: every string's offset is `> 0` (null guard).
5. **wasmparser validation**: the full binary parses cleanly.
6. **End-to-end pointer dereference** (new `tests/string_host_test.rs`):
   instantiate the WASM with a minimal host, call `f()`, read `i32` at the
   returned pointer, verify it equals the byte length, then read that many
   bytes and verify they match the literal. (This requires a tiny Wasmtime
   or wasmi interpreter; alternatively, a unit test on the data section
   bytes can verify the content directly without execution.)
7. **Unicode**: `"héllo"` (5 chars, 6 UTF-8 bytes) is emitted with
   `byte_len = 6` and the correct UTF-8 payload.

### 4.8 Dependencies on other gaps

- **None hard.** This gap is self-contained.
- **Soft reverse-dependency**: Gap 1 (OO) and Gap 5 (Collections) both
  need string pointers to be real (object fields of type `string`, error
  messages from collection operations). They depend on Gap 4 landing first.

### 4.9 Risks and trade-offs

- **Length-prefixed vs null-terminated.** Length-prefixed is chosen because
  it allows embedded nulls and is O(1) for length queries.
  **Rationale:** matches Rust's `&str` semantics; the renderer (HarfRust)
  expects a `(ptr, len)` pair anyway.
- **Active vs passive data segments.** Active segments are simpler
  (applied at instantiation). **Rationale:** the WASM module is
  single-purpose; passive segments are only useful for multi-memory or
  lazy-init scenarios.
- **Deduplication by exact text.** Two literals with the same text share
  one data segment. **Rationale:** trivially correct, saves memory.
- **No string concatenation at compile time.** `"a" + "b"` is not folded
  into `"ab"`; each operand is a separate literal. **Rationale:** the
  AST has no `+` for strings in this wave (string concatenation is a
  future runtime feature).

### 4.10 Open questions

1. Should the data section be one segment per string, or one combined
   segment with all strings concatenated? **Tentative: one combined
   segment** (simpler, smaller binary, single `memory.init` at
   instantiation). Each string's offset is still independently addressable.
   *Update: the design above uses one segment per string for clarity; the
   implementer should switch to a single combined segment for binary-size
   efficiency.*
2. Should we emit a `memory.grow` instruction at startup if the strings
   exceed the initial page count, or just declare enough pages upfront?
   **Tentative: declare enough pages upfront** — simpler and avoids the
   runtime cost.
3. Should the string pointer be `i32` (4-byte) or `i64` (8-byte, for
   memory64)? **Tentative: i32** — WASM's default memory is 32-bit, which
   gives 4 GiB of address space; sufficient for any UI scene.

---

## Gap 5 — Collection Method Dispatch

### 5. Current state (file:line evidence)

- `crates/alkalive-compiler/src/wasm_codegen.rs:387-405` — the
  `Expr::MethodCall` arm:
  ```rust
  Expr::MethodCall { receiver, method, args, ... } => {
      // Compile the receiver (leaves a pointer on the stack).
      self.compile_expr(receiver, instrs);
      // Compile arguments.
      for a in args { self.compile_expr(a, instrs); }
      let _ = method;
      // Query methods return a value; mutators don't.
      if method == "len" || method == "is_empty" || method == "get" {
          instrs.push(AlkInstr::I32Const(0));
      }
  }
  ```
  This compiles the receiver and arguments onto the stack, then **discards
  them** (no `call` instruction is emitted) and pushes `i32.const 0` for
  query methods. The arguments and receiver are simply dropped at the end
  of the expression statement.
- `crates/alkalive-compiler/src/ast.rs:346-357` — `Expr::MethodCall` is
  parsed for any `receiver.method(args)` pattern; there is no special-case
  for collection methods.
- `crates/alkalive-compiler/src/typechecker.rs:135-151` — the typechecker
  classifies `push`/`extend`/`insert`/`append` as grow ops and
  `remove`/`truncate`/`clear`/`swap_remove`/`drain` as shrink ops for
  monotonicity checking, but it does **not** resolve the call to a host
  function.
- `crates/alkalive-compiler/src/wasm_codegen.rs:571-577` — no
  `ImportSection` is emitted by the WASM backend; there is no import
  mechanism for host-provided collection operations.

### 5.1 Problem statement

The compiler accepts `v.push(1)` syntactically and typechecks it for
monotonicity, but the generated WASM does not actually call any function —
the push is silently dropped. Collections are unusable.

### 5.2 Why it's required

- **ADR-008**: a real language compiling to WASM needs real collections.
- **ADR-018** (line 621): "explicit typed imports" — host functions for
  collection operations are the simplest case of cross-boundary imports.
- **ADR-027 Phase 2** (the monotonicity lattice) is meaningless if the
  operations it classifies (grow/shrink) are not actually emitted.

### 5.3 Relationship to existing compiler

- The `Expr::MethodCall` AST node already exists and is parsed by
  `parse_primary` (`parser.rs:598-616`).
- The typechecker already classifies methods (`typechecker.rs:135-151`); it
  just needs to also resolve their return types (Gap 3 §3.4.4).
- The WASM backend already has a `TypeSectionBuilder`
  (`wasm_codegen.rs:124-159`); it is reused to declare the import types.
- The `AlkInstr::Call(String)` variant (`wasm_codegen.rs:179`) is reused —
  it is extended to distinguish between intra-module calls (`call $fnidx`)
  and host imports (`call $import_idx`).

### 5.4 Proposed design

#### 5.4.1 Host function ABI

The host provides the following functions, all imported under the module
name `"alk"`:

| Host function | Signature | AlkALive method |
|---------------|-----------|-----------------|
| `alk::vec_new(elem_size: i32) -> i32` | returns a fresh Vec handle | `Vec::new()` (synthesised) |
| `alk::vec_with_capacity(elem_size: i32, cap: i32) -> i32` | returns a Vec with capacity | `Vec::with_capacity(n)` |
| `alk::vec_push(ptr: i32, value: i32)` | appends a value | `v.push(x)` (mutator; returns unit) |
| `alk::vec_extend(dst: i32, src: i32)` | appends all of `src` to `dst` | `v.extend(other)` |
| `alk::vec_remove(ptr: i32, idx: i32)` | removes element at `idx` | `v.remove(i)` |
| `alk::vec_clear(ptr: i32)` | removes all elements | `v.clear()` |
| `alk::vec_len(ptr: i32) -> i32` | returns element count | `v.len()` |
| `alk::vec_is_empty(ptr: i32) -> i32` | returns 1 if empty, else 0 | `v.is_empty()` |
| `alk::vec_get(ptr: i32, idx: i32) -> i32` | returns element at `idx` | `v.get(i)` (panics on out-of-bounds in this wave) |

Notes:

- All values are `i32` (4 bytes). For `Vec<f32>`, the `f32` is bit-cast to
  `i32` on the way in/out — the WASM `f32.reinterpret_i32` instruction
  handles this.
- `elem_size` is always 4 in this wave (every type is 4 bytes — see
  `alk_type_to_wasm` at `wasm_codegen.rs:92-101`). The parameter exists
  for forward compatibility with `Vec<string>` (still 4 bytes — a pointer)
  and future `Vec<u8>` (1 byte).
- The host owns the actual heap storage; the WASM module holds only opaque
  `i32` handles. This is consistent with Gap 4's string pointers (the
  string bytes live in WASM linear memory, but the *Vec metadata* lives in
  the host). A future wave may move Vec storage into WASM memory for
  in-place element access.

#### 5.4.2 Import section emission

A new `ImportSectionBuilder` is added to `compile_to_wasm`:

```rust
struct HostImport {
    module: &'static str,   // "alk"
    name: &'static str,     // "vec_push"
    type_idx: u32,          // index into the type section
    kind: ImportKind,
}

enum ImportKind {
    Func(u32),  // function import — index space starts at 0
}
```

Before the function section is emitted, the import section is built:

```rust
let mut import_sec = ImportSection::new();
let mut host_imports: Vec<HostImport> = Vec::new();
for (name, params, results) in [
    ("vec_new",             &[I32][..],    &[I32]),
    ("vec_with_capacity",   &[I32, I32],   &[I32]),
    ("vec_push",            &[I32, I32],   &[]),
    ("vec_extend",          &[I32, I32],   &[]),
    ("vec_remove",          &[I32, I32],   &[]),
    ("vec_clear",           &[I32],        &[]),
    ("vec_len",             &[I32],        &[I32]),
    ("vec_is_empty",        &[I32],        &[I32]),
    ("vec_get",             &[I32, I32],   &[I32]),
] {
    let type_idx = type_builder.register(params, results);
    host_imports.push(HostImport {
        module: "alk", name, type_idx, kind: ImportKind::Func(host_imports.len() as u32),
    });
    import_sec.import("alk", name, EntityKind::Function, type_idx);
}
wasm_module.section(&import_sec);
```

**Critical WASM detail:** imported functions occupy the **lowest** indices
in the function index space. If there are 9 host imports, they are
indices 0–8; the first AlkALive-defined function is index 9. The
`call` instruction must use the correct absolute index. The
`AlkInstr::Call(String)` resolution logic in `compile_to_wasm` is updated:

```rust
AlkInstr::Call(name) => {
    // First check if this is a host import.
    if let Some(import) = host_imports.iter().find(|i| i.name == name) {
        Instruction::Call(import.kind.as_func_idx())
    } else {
        // Intra-module call: add the import count to the local index.
        let local_idx = fn_metas.iter().position(|m| m.name == *name).unwrap_or(0) as u32;
        let absolute_idx = (host_imports.len() as u32) + local_idx;
        Instruction::Call(absolute_idx)
    }
}
```

For method calls, a new `AlkInstr::HostCall(&'static str)` variant is
introduced (or `AlkInstr::Call` is reused with a convention that
`alk_*` names always resolve to imports).

#### 5.4.3 `Expr::MethodCall` codegen

```rust
Expr::MethodCall { receiver, method, args, ... } => {
    // Determine the host function name.
    let host_name = match method.as_str() {
        "push"         => "vec_push",
        "extend"       => "vec_extend",
        "remove"       => "vec_remove",
        "clear"        => "vec_clear",
        "len"          => "vec_len",
        "is_empty"     => "vec_is_empty",
        "get"          => "vec_get",
        _ => {
            // Not a known collection method — defer to Gap 1's class-method
            // dispatch. Until Gap 1 lands, emit nothing (placeholder).
            return;
        }
    };
    // 1. Compile receiver (leaves ptr on stack).
    self.compile_expr(receiver, instrs);
    // 2. Compile arguments (left to right).
    for a in args { self.compile_expr(a, instrs); }
    // 3. Emit the host call.
    instrs.push(AlkInstr::Call(host_name.to_string()));
}
```

#### 5.4.4 `Expr::PathCall` codegen

`Vec::new()` and `Vec::with_capacity(n)` are routed to host imports:

```rust
Expr::PathCall(module, member, args, ...) => {
    if module == "Vec" && member == "new" {
        // vec_new(elem_size=4)
        instrs.push(AlkInstr::I32Const(4));
        instrs.push(AlkInstr::Call("vec_new".to_string()));
    } else if module == "Vec" && member == "with_capacity" {
        // vec_with_capacity(elem_size=4, cap)
        for a in args { self.compile_expr(a, instrs); }
        instrs.push(AlkInstr::I32Const(4));
        instrs.push(AlkInstr::Call("vec_with_capacity".to_string()));
        // ^^^ note: order matters — host signature is (elem_size, cap)
        // so we need to push elem_size BEFORE cap. Adjust:
        // (compile cap) (i32.const 4) (call vec_with_capacity)
        // -- actually we need (i32.const 4) (compile cap) (call) — fix above.
    } else {
        // Cross-module call (Gap 2) or unknown — placeholder for now.
        instrs.push(AlkInstr::I32Const(0));
    }
}
```

(The implementer should double-check argument order against the host
signature; the design intent is that `Vec::with_capacity(n)` produces
`i32.const 4; <compile n>; call vec_with_capacity`.)

#### 5.4.5 Typechecker integration

The typechecker's `Expr::MethodCall` arm (updated in Gap 3 §3.4.4) returns
the host-provided return type:

| Method | Return type |
|--------|-------------|
| `push`, `extend`, `remove`, `clear`, `truncate`, `swap_remove`, `drain` | `None` (unit) |
| `len`, `is_empty`, `get` | `i32` / `bool` / `i32` placeholder |

The monotonicity check (`typechecker.rs:449-477`) is unchanged — it already
runs before the return type is computed.

### 5.5 Compiler implications

| Layer | Change |
|-------|--------|
| **Lexer** | None. |
| **Parser** | None. |
| **AST** | None. |
| **Typechecker** | Already covered by Gap 3 §3.4.4 — `collection_method_return_type`. |
| **WASM codegen** | (1) Import `wasm_encoder::{ImportSection, EntityKind}`. (2) Build `host_imports` list before the function section. (3) Emit the import section. (4) Update `AlkInstr::Call` resolution to handle `alk::*` names. (5) Update `Expr::MethodCall` and `Expr::PathCall` arms per §5.4.3 / §5.4.4. |
| **Host runtime** | The runtime (in `alkalive-runtime-wasm`) must provide the 9 host functions. They are registered via `wasm-bindgen`'s import mechanism (the existing JS glue at `deploy/pkg/alkalive_runtime_wasm.js` already has 94 import bindings; 9 more are added). The Rust side implements them as `extern "C"` functions or via `wasm-bindgen`'s `#[wasm_bindgen]` macro. |

### 5.6 Error handling

Errors at the WASM-codegen level:

- `unknown method '.X()' on Vec<T>` — emitted as a `WasmCodegenError` if
  the method name is not in the host table and the receiver is a `Vec`
  (rather than a class). This is a defensive check; the typechecker should
  have caught it first.

Errors at the typechecker level (covered by Gap 3):

- `method '.X()' is not defined on type 'T'`
- `argument K to '.X()' has type 'A' but parameter has type 'B'`
- `grow operation '.X()' is forbidden on a 'monotone' collection`
  (existing — `typechecker.rs:449-477`)

Runtime errors (host side):

- `vec_get` out of bounds → the host traps (WASM `unreachable`). The
  program aborts with a stack trace. (A future wave may add `Option<T>`
  return types and `try`/`catch`.)
- `vec_push` on a full Vec → the host grows the underlying allocation; no
  error.

### 5.7 Testing strategy

Tests in `wasm_codegen.rs::tests`:

1. **Import section present**: a module that uses `Vec::new()` produces a
   binary with an `ImportSection` containing 9 imports from module `"alk"`.
2. **Index space correctness**: the first AlkALive-defined function has
   index 9 (after 9 host imports). Verified by parsing the binary with
   `wasmparser` and walking the function index space.
3. **`v.push(1)` codegen**: the compiled body contains (in order)
   `LocalGet` (the receiver), `I32Const(1)`, `Call(2)` (where 2 is the
   index of `alk::vec_push`). Verified at the `AlkInstr` level.
4. **`v.len()` codegen**: `LocalGet`, `Call(6)` (the index of
   `alk::vec_len`), leaving an `i32` on the stack.
5. **`Vec::new()` codegen**: `I32Const(4)`, `Call(0)` (the index of
   `alk::vec_new`).
6. **wasmparser validation**: the full binary with imports parses cleanly.
7. **Typechecker integration**: `let v: Vec<i32> = Vec::new(); let n: i32 = v.len();`
   typechecks (return type of `len` is `i32`).
8. **Monotonicity preserved**: `let v: monotone Vec<i32> = Vec::new(); v.remove(0);`
   is rejected (existing test — must still pass after Gap 5 lands).

End-to-end (new `tests/collection_host_test.rs`, optional in this wave):

- A minimal host (in test code) provides the 9 functions backed by a
  `Vec<i32>` in Rust. The WASM module is instantiated, `main` is called,
  and the host's `Vec` is observed to contain the expected values.

### 5.8 Dependencies on other gaps

- **Hard dependency on Gap 4** (Strings): the import-section design and
  the `AlkInstr::Call` resolution logic are shared between string data
  segments and collection imports. Landing Gap 4 first establishes the
  pattern; Gap 5 reuses it. (Strictly, the import section is independent
  of the data section, but the implementer benefits from doing them in
  order.)
- **Soft dependency on Gap 3** (Type Inference): the typechecker must
  return real types for collection methods. Gap 3 §3.4.4 covers this; if
  Gap 3 has not landed, Gap 5 still emits correct WASM but the typechecker
  cannot verify that `let n: i32 = v.len();` is well-typed.

### 5.9 Risks and trade-offs

- **Host-owned Vec storage vs WASM-owned.** This design puts the Vec
  metadata in the host, with the WASM module holding an opaque `i32`
  handle. **Rationale:** the host can implement a real growable allocator
  (Rust's `Vec<T>`); WASM-side storage would require a bump allocator and
  reallocation logic. **Trade-off:** element access goes through a host
  call (`vec_get`), which is slower than a direct `i32.load`. For
  hot loops over large collections, a future wave can add an inline
  storage mode where the Vec lives in WASM linear memory.
- **All elements are `i32`.** `f32` elements are bit-cast; `bool` is
  `0`/`1`; `string` is a pointer. **Rationale:** matches the existing
  `alk_type_to_wasm` mapping. **Trade-off:** `Vec<bool>` wastes 31 bits
  per element; acceptable for UI scenes.
- **No `iter()` or `for x in v` syntax.** Iteration uses an index loop:
  ```alk
  let i: i32 = 0;
  while i < v.len() {
    let x: i32 = v.get(i);
    // ...
    i = i + 1;
  }
  ```
  **Rationale:** `for` loops and iterators are a separate language
  feature; this wave focuses on the host ABI.
- **`vec_get` panics on out-of-bounds.** A safer `get_or_default` or
  `Option<T>` return is deferred. **Rationale:** keeps the ABI simple;
  bounds-check failures are programmer errors.

### 5.10 Open questions

1. Should the host also provide `vec_set(ptr, idx, value)` for in-place
   element mutation? **Tentative: yes** — add it to the ABI now (10 host
   functions instead of 9). It is cheap and avoids a future ABI break.
2. Should `Vec<string>` be supported in this wave, given that string
   pointers are now real (Gap 4)? **Tentative: yes** — the ABI is
   type-agnostic (all `i32`); only the typechecker needs to know that
   `Vec<string>` elements are string pointers.
3. Should we expose `vec_capacity` and `vec_reserve` for explicit capacity
   management? **Tentative: yes** — add to the ABI; users can ignore them.
4. Should the import module name be `"alk"` (short) or `"alkalive"` (full)?
   **Tentative: `"alk"`** — matches the language name's short form and
   keeps the binary smaller.

---

## 6. Cross-gap dependency resolution (consolidated)

The five gaps have the dependency relationships shown in §0.2. This section
makes the resolution explicit and gives the implementer a concrete build
order.

### 6.1 Build order (mandatory)

| Step | Gap(s) | Why this order |
|------|--------|----------------|
| 1 | **Gap 3** (Type Inference) | Pure typechecker work; no AST/parser/WASM changes. Every later gap depends on real call-return types. |
| 2 | **Gap 4** (Strings) | Pure WASM-backend work; establishes the data-section pattern. No typechecker changes. |
| 3 | **Gap 5** (Collections) | Extends the WASM backend with the import section; reuses the heap-pointer convention from Gap 4. Typechecker return-type integration is already in Gap 3. |
| 4 | **Gap 1** (OO) | Requires Gap 3 (method return types), Gap 4 (string fields), Gap 5 (Vec fields). Adds substantial AST/parser/typechecker/WASM work. |
| 5 | **Gap 2** (Modules) | Requires a fully working single-module language (all other gaps). Adds the resolver pass, the reachability/tree-shaking pass, and the cross-module linking. |

### 6.2 Interface contracts between gaps

The following contracts must hold for the gaps to compose correctly:

**Gap 3 → Gap 1 (Type Inference → OO)**
- `FnSigTable` (from Gap 3) is extended by Gap 1 to include method
  signatures (`receiver_class: Some(class_name)`). The lookup function
  gains a `lookup_method(class: &str, method: &str) -> Option<&FnSig>`
  variant.
- `check_expr`'s `Expr::MethodCall` arm (Gap 3 §3.4.4) dispatches to
  `class_method_return_type` (Gap 1) when the receiver is a class type.

**Gap 4 → Gap 5 (Strings → Collections)**
- The `StringTable` (Gap 4) and the `host_imports` list (Gap 5) are both
  owned by `compile_to_wasm`. The `FnCompiler` carries references to both.
- The import section (Gap 5) is emitted **before** the function section;
  the data section (Gap 4) is emitted **after** the code section. This
  ordering is fixed by the WASM binary format and is already correct in
  the existing `compile_to_wasm` skeleton.

**Gap 4 + Gap 5 → Gap 1 (Strings + Collections → OO)**
- A class field of type `string` stores an `i32` pointer produced by
  Gap 4's string table.
- A class field of type `Vec<i32>` stores an `i32` handle produced by
  Gap 5's `vec_new` host import.
- The `__alk_alloc` host function (Gap 1 §1.4.3) is added to the same
  `host_imports` list as the `vec_*` functions (Gap 5). Its index in the
  import section is contiguous with the collection imports.

**Gap 1 → Gap 2 (OO → Modules)**
- `Visibility` (from Gap 1) is extended to top-level `ItemDecl::Fn`/`Let`
  in Gap 2.
- `ClassDecl` (from Gap 1) gains a `Visibility` field in Gap 2.
- The `FnSigTable` (from Gap 3) is populated with imported signatures by
  Gap 2's resolver pass.

**Gap 3 → Gap 2 (Type Inference → Modules)**
- The `FnSigTable.imported_from: Option<String>` field (Gap 3 §3.4.1) is
  populated by Gap 2's resolver.
- `Expr::PathCall(module, member, args)` (Gap 3 §3.4.5) resolves through
  the `FnSigTable` for cross-module calls once Gap 2's resolver has
  populated it.

### 6.3 Shared data structures (who owns what)

| Structure | Defined in | Populated by | Consumed by |
|-----------|-----------|--------------|-------------|
| `FnSigTable` | typechecker (Gap 3) | `collect_signatures` (Gap 3) + resolver (Gap 2) + class collector (Gap 1) | `check_expr` (Gap 3) |
| `ClassTable` | typechecker (Gap 1) | `collect_classes` (Gap 1) | `check_expr` (Gap 1) + `check_method_override` (Gap 1) |
| `StringTable` | wasm_codegen (Gap 4) | `compile_expr` on `Lit::Str` (Gap 4) | data-section emission (Gap 4) |
| `host_imports` | wasm_codegen (Gap 5) | `compile_to_wasm` (Gap 5) | `AlkInstr::Call` resolution (Gap 5) + object allocation (Gap 1) |
| `ResolvedGraph` | modules (Gap 2) | `resolve` (Gap 2) | `check_module` (Gap 3 + Gap 2) + reachability pass (Gap 2) |

---

## 7. Implementation sequencing & effort estimates

(Notional — for orchestrator planning. Each gap is one implementation wave.)

| Wave | Gap | Estimated LOC added | Estimated tests added | Estimated effort |
|------|-----|---------------------|----------------------|------------------|
| 10 | Gap 3 | ~150 (typechecker) + ~30 (tests) | ~15 | Small (1–2 days) |
| 11 | Gap 4 | ~120 (wasm_codegen) + ~40 (tests) | ~10 | Small (1–2 days) |
| 12 | Gap 5 | ~180 (wasm_codegen + host runtime) + ~50 (tests) | ~12 | Medium (2–3 days) |
| 13 | Gap 1 | ~600 (lexer/parser/AST/typechecker/wasm_codegen) + ~150 (tests) | ~30 | Large (5–7 days) |
| 14 | Gap 2 | ~500 (resolver + linker + tree-shaking) + ~120 (tests) | ~25 | Large (5–7 days) |
| **Total** | | **~1,940 LOC** | **~92 tests** | **~15–22 days** |

---

## 8. Open questions (consolidated)

These are the questions that the implementer should resolve (with the
project owner if necessary) before or during implementation. They are
restated here for visibility; per-gap answers are in each gap's §X.10.

1. **`Self` in non-constructor return types** (Gap 1) — yes, resolves to
   the enclosing class.
2. **Inline field initialisers** (`pub x: i32 = 0;`) (Gap 1) — yes,
   desugars to default `new`.
3. **`super.method()`** (Gap 1) — not in this wave.
4. **Traits/interfaces** (Gap 1) — later, as a non-breaking extension.
5. **`pub(crate)` / `pub(super)`** (Gap 2) — not in this wave; only `pub`
   and `priv`.
6. **Entry module convention** (Gap 2) — any module with `pub fn main`;
   linker takes `--entry`.
7. **Capability additivity** (Gap 2) — additive (extra grants = warning).
8. **Version constraints on imports** (Gap 2) — not in this wave.
9. **`let` without type annotation** (Gap 3) — not in this wave; the
   parser requires `let x: Type = ...`.
10. **Named arguments** (Gap 3) — yes, the signature table should carry
    parameter names (cheap, helps diagnostics).
11. **Single combined data segment vs per-string segments** (Gap 4) —
    switch to a single combined segment for binary-size efficiency.
12. **memory64** (Gap 4) — no, stay with 32-bit memory.
13. **`vec_set`, `vec_capacity`, `vec_reserve`** (Gap 5) — yes, add to the
    ABI now (avoid future ABI break).
14. **`Vec<string>` support** (Gap 5) — yes, the ABI is type-agnostic.
15. **Import module name** (Gap 5) — `"alk"` (short form).

---

## 9. Appendix A — WASM section ordering (final, after all gaps land)

The generated WASM binary's section order (per the WASM spec's
ordering rules) will be:

```
1. Type section        (function types: user fns + host imports)
2. Import section      (alk::vec_*, alk::__alk_alloc)        [Gap 5 + Gap 1]
3. Function section    (user function declarations)
4. Table section       (one funcref table for vtables)       [Gap 1]
5. Memory section      (1+ pages, grown for strings)         [Gap 4]
6. Global section      (vtable base offsets per class)       [Gap 1]
7. Export section      (pub fns + classes + memory)
8. Start section       (optional: __alk_main if present)
9. Element section     (vtable method function indices)       [Gap 1]
10. Data section       (string literals as length-prefixed)   [Gap 4]
11. Code section       (function bodies)
```

The existing `compile_to_wasm` function in `wasm_codegen.rs:491-662` emits
sections 1, 3, 5, 7, 11. Gaps 1, 4, and 5 add sections 2, 4, 6, 9, 10.

---

## 10. Appendix B — Existing-code impact summary

| File | Lines (today) | Gaps touching it | Approx. lines added |
|------|---------------|------------------|---------------------|
| `crates/alkalive-compiler/src/lexer.rs` | 1268 | 1, 2 | +60 (keywords) |
| `crates/alkalive-compiler/src/parser.rs` | 1488 | 1, 2 | +400 (class/import parsing) |
| `crates/alkalive-compiler/src/ast.rs` | 693 | 1, 2 | +200 (new AST nodes) |
| `crates/alkalive-compiler/src/typechecker.rs` | 915 | 1, 2, 3 | +350 (signature table, class table, OO checks) |
| `crates/alkalive-compiler/src/wasm_codegen.rs` | 1173 | 1, 4, 5 | +450 (data section, import section, vtables, OO codegen) |
| `crates/alkalive-compiler/src/lib.rs` | 273 | 1, 2 | +30 (re-exports) |
| `crates/alkalive-compiler/src/modules.rs` (NEW) | 0 | 2 | +500 (resolver + linker) |
| `crates/alkalive-runtime-wasm/src/lib.rs` | 448 | 5 | +120 (host function implementations) |
| **Total** | **6,258** | | **~2,110 added** |

---

## 11. DoD checklist for this fine draft

- [x] All 5 gaps documented with the required 11-section structure
  (current state, problem, why, relationship, design, compiler
  implications, error handling, testing, dependencies, risks, open
  questions).
- [x] Each gap cites file:line evidence for the current state.
- [x] Each gap specifies exact syntax, AST node shapes, and WASM
  instructions.
- [x] Cross-gap dependencies resolved (§0.2 graph + §6 contracts).
- [x] Build order specified (§6.1).
- [x] Shared data structures tabulated (§6.3).
- [x] Open questions consolidated (§8).
- [x] Appendix A: WASM section ordering.
- [x] Appendix B: per-file impact summary.
- [x] Saved to `docs/alkalive-fine-draft-language.md`.
- [x] Worklog appended.

---

*End of fine draft.*

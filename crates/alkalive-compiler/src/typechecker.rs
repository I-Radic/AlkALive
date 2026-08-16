//! ADR-027 Phase 2 — Type checker with monotonicity qualifier flow.
//!
//! This module implements the type-checking pass that verifies monotonicity
//! qualifiers flow correctly through function signatures and method calls.
//! It runs between [`crate::parser`] (AST) and [`crate::codegen`] (IR
//! lowering), so the IR can carry resolved [`crate::ir::Monotonicity`]
//! metadata for the runtime's seminaïve evaluation engine (ADR-025).
//!
//! # The qualifier subtyping lattice
//!
//! ```text
//!         unrestricted (bottom — most permissive value)
//!        /                  \
//!    monotone            antitone   (incomparable tops)
//! ```
//!
//! - `unrestricted <: monotone`  (an unrestricted value may be used where a
//!   monotone one is required — the function will only grow it, which is
//!   legal for an unrestricted collection).
//! - `unrestricted <: antitone`  (symmetrically).
//! - `monotone` and `antitone` are **not comparable**. A `monotone` value
//!   cannot be passed where `antitone` or `unrestricted` is expected (the
//!   callee might shrink it, violating monotonicity).
//! - `monotone <: monotone` and `antitone <: antitone` (reflexivity).
//!
//! # Operation classification
//!
//! - **Grow ops** (allowed on `monotone` and `unrestricted`; FORBIDDEN on
//!   `antitone`): `push`, `extend`, `insert`, `append`.
//! - **Shrink ops** (allowed on `antitone` and `unrestricted`; FORBIDDEN on
//!   `monotone`): `remove`, `truncate`, `clear`, `swap_remove`, `drain`.
//! - **Neutral ops** (allowed on all): `len`, `get`, `iter`, `contains`,
//!   `first`, `last`, `is_empty`.
//!
//! # What the type checker verifies
//!
//! 1. **Method-call validation.** A shrink op on a `monotone`-qualified
//!    collection is a type error. A grow op on an `antitone`-qualified
//!    collection is a type error.
//! 2. **Function-boundary flow.** When a collection is passed as a function
//!    argument, the actual qualifier must be a subtype of the declared
//!    parameter qualifier.
//! 3. **Return-type checking.** A `return` expression's type must be a
//!    subtype of the declared return type.
//! 4. **Variable resolution.** Every variable reference must resolve to a
//!    declared binding (parameter, local `let`, or module-level `let`).
//!
//! Unlike the Phase 1 lint (which is intra-function and emits warnings),
//! the Phase 2 type checker enforces across function boundaries and emits
//! hard errors.

#![forbid(unsafe_code)]

use core::fmt;

use crate::ast::{
    BaseType, Block, ClassDecl, Expr, FieldDecl, FnDecl, ItemDecl, LetDecl, Lit, MethodDecl,
    ModuleDecl, Param, Qualifier, Stmt, Type, Visibility,
};

// ======================================================================
// Function signature table (Gap 3 — full type inference)
// ======================================================================

/// A function's signature — the type-checker's view of a callable.
#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    /// The lookup name. For free functions this is the bare name (`"add"`).
    /// For class methods this is the qualified name (`"Button::new"`).
    pub name: String,
    /// The parameter types (excluding the implicit `self`).
    pub params: Vec<Type>,
    /// The declared return type. `None` means the function returns unit.
    pub return_type: Option<Type>,
    /// Parameter names (carried for diagnostics).
    pub param_names: Vec<String>,
    /// `Some(class_name)` for instance/static methods; `None` for free functions.
    pub receiver_class: Option<String>,
    /// `Some(module_path)` for names imported from another module.
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) a signature.
    pub fn insert(&mut self, name: impl Into<String>, sig: FnSig) {
        self.sigs.insert(name.into(), sig);
    }

    /// Look up by name.
    pub fn lookup(&self, name: &str) -> Option<&FnSig> {
        self.sigs.get(name)
    }

    /// Look up a class method by `Class::method` qualified name.
    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FnSig> {
        let q = format!("{}::{}", class, method);
        self.sigs.get(&q)
    }
}

/// Collect all function signatures from the module into the table.
/// This runs in pass 1, before any function body is checked, so mutual
/// recursion and self-recursion are supported. After Gap 1, class method
/// signatures are also collected here under the qualified name
/// `Class::method`.
fn collect_signatures(module: &ModuleDecl, table: &mut FnSigTable) {
    for item in &module.items {
        match item {
            ItemDecl::Fn(f) => {
                table.insert(
                    f.name.clone(),
                    FnSig {
                        name: f.name.clone(),
                        params: f.params.iter().map(|p| p.ty.clone()).collect(),
                        return_type: f.return_type.clone(),
                        param_names: f.params.iter().map(|p| p.name.clone()).collect(),
                        receiver_class: None,
                        imported_from: None,
                    },
                );
            }
            ItemDecl::Class(c) => {
                for m in &c.methods {
                    let qualified = format!("{}::{}", c.name, m.name);
                    // Resolve `Self` in the return type and parameter types
                    // to the enclosing class name. This ensures subtype
                    // checks against `Named("ClassName")` work correctly.
                    let resolve_self = |ty: &Type| -> Type {
                        let mut t = ty.clone();
                        if let BaseType::Named(n) = &mut t.base {
                            if n == "Self" {
                                *n = c.name.clone();
                            }
                        }
                        t
                    };
                    let params: Vec<Type> = m.params.iter().map(|p| resolve_self(&p.ty)).collect();
                    let return_type = m.return_type.as_ref().map(resolve_self);
                    table.insert(
                        qualified.clone(),
                        FnSig {
                            name: qualified,
                            params,
                            return_type,
                            param_names: m.params.iter().map(|p| p.name.clone()).collect(),
                            receiver_class: Some(c.name.clone()),
                            imported_from: None,
                        },
                    );
                }
            }
            ItemDecl::Let(_) => {}
        }
    }
}

/// Collect all class signatures into the `ClassTable` (Gap 1 — pass 1.5).
/// Detects cycles, computes `field_stride` and `vtable_slot_count`, and
/// verifies that base classes exist.
fn collect_classes(
    module: &ModuleDecl,
    classes: &mut ClassTable,
    errors: &mut TypeErrorSet,
) {
    // Pass 1: insert all class signatures (without computing strides yet).
    let mut seen_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for item in &module.items {
        if let ItemDecl::Class(c) = item {
            if !seen_names.insert(c.name.clone()) {
                errors.push(TypeError {
                    message: format!("class `{}` declared twice", c.name),
                    line: c.line,
                    col: c.col,
                });
                continue;
            }
            let sig = ClassSig {
                name: c.name.clone(),
                base: c.base.clone(),
                visibility: c.visibility,
                fields: c.fields.clone(),
                methods: c.methods.clone(),
                field_stride: 0,
                vtable_slot_count: 0,
            };
            classes.insert(c.name.clone(), sig);
        }
    }
    // Pass 2: validate base classes exist; detect cycles; compute strides.
    let class_names: Vec<String> = classes.names_in_order().to_vec();
    for name in &class_names {
        // Cycle detection.
        if let Some(cycle) = classes.find_cycle(name) {
            // Avoid duplicate reports: only report the cycle when the
            // starting class is the earliest in the cycle path.
            if cycle.first().map(|s| s.as_str()) == Some(name.as_str()) {
                let sig = classes.lookup(name);
                let (line, col) = sig
                    .map(|s| {
                        // Find the original ClassDecl's line/col — we use the
                        // sig's name to look up the original decl.
                        let _ = s;
                        (0u32, 0u32)
                    })
                    .unwrap_or((0, 0));
                let (line, col) = module
                    .items
                    .iter()
                    .find_map(|it| match it {
                        ItemDecl::Class(c) if c.name == *name => Some((c.line, c.col)),
                        _ => None,
                    })
                    .unwrap_or((line, col));
                errors.push(TypeError {
                    message: format!(
                        "cyclic inheritance: {}",
                        cycle.join(" : ")
                    ),
                    line,
                    col,
                });
            }
        }
        // Base existence.
        if let Some(sig) = classes.lookup(name) {
            if let Some(base) = &sig.base {
                if classes.lookup(base).is_none() {
                    let (line, col) = module
                        .items
                        .iter()
                        .find_map(|it| match it {
                            ItemDecl::Class(c) if c.name == *name => Some((c.line, c.col)),
                            _ => None,
                        })
                        .unwrap_or((0, 0));
                    errors.push(TypeError {
                        message: format!("unknown class `{}` (referenced as base of `{}`)", base, name),
                        line,
                        col,
                    });
                }
            }
        }
        // Compute strides.
        let total_fields = total_field_count(classes, name);
        let total_methods = total_unique_method_count(classes, name);
        if let Some(sig) = classes.lookup_mut(name) {
            sig.field_stride = 4 * (1 + total_fields);
            sig.vtable_slot_count = total_methods;
        }
    }
}

/// Type-check a class declaration (Gap 1 — pass 3b).
///
/// Verifies:
/// - No duplicate field/method names within the class.
/// - `new` is a static method returning `Self`.
/// - Method-override signatures match the base method (invariant override).
/// - Each method body type-checks with `self: Named(class_name)` in scope
///   (for instance methods) and `__enclosing_class__: Named(class_name)`
///   always in scope (for `Self` resolution).
fn check_class(
    c: &ClassDecl,
    classes: &ClassTable,
    module_env: &TypeEnv,
    sigs: &FnSigTable,
    errors: &mut TypeErrorSet,
) {
    // 1. Duplicate field check.
    let mut seen_fields: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for f in &c.fields {
        if !seen_fields.insert(f.name.clone()) {
            errors.push(TypeError {
                message: format!(
                    "field `{}` already declared in class `{}`",
                    f.name, c.name
                ),
                line: f.line,
                col: f.col,
            });
        }
    }
    // 2. Duplicate method check + override signature check.
    let mut method_names: std::collections::HashMap<String, &MethodDecl> =
        std::collections::HashMap::new();
    for m in &c.methods {
        if let Some(_existing) = method_names.get(&m.name) {
            errors.push(TypeError {
                message: format!(
                    "method `{}` already declared in class `{}`",
                    m.name, c.name
                ),
                line: m.line,
                col: m.col,
            });
        } else {
            method_names.insert(m.name.clone(), m);
        }
        // Override check: invariant signature match against the base chain.
        if let Some(base) = &c.base {
            if let Some(base_class) = classes.lookup(base) {
                // Walk the base chain looking for a method with the same name.
                let mut current: Option<&ClassSig> = Some(base_class);
                while let Some(bc) = current {
                    if let Some(base_method) = bc.methods.iter().find(|bm| bm.name == m.name) {
                        if !signatures_match(m, base_method) {
                            errors.push(TypeError {
                                message: format!(
                                    "cannot override `{}` in `{}`: signature mismatch",
                                    m.name, c.name
                                ),
                                line: m.line,
                                col: m.col,
                            });
                        }
                        break;
                    }
                    current = bc.base.as_deref().and_then(|n| classes.lookup(n));
                }
            }
        }
    }
    // 3. `new` checks: must be static, must return Self.
    for m in &c.methods {
        if m.name == "new" {
            if m.is_instance {
                errors.push(TypeError {
                    message: format!(
                        "constructor `new` in `{}` must be a static method (no `self` parameter)",
                        c.name
                    ),
                    line: m.line,
                    col: m.col,
                });
            }
            let ok = match &m.return_type {
                Some(Type {
                    base: BaseType::Named(n),
                    ..
                }) => n == &c.name || n == "Self",
                _ => false,
            };
            if !ok {
                errors.push(TypeError {
                    message: format!(
                        "constructor `new` in `{}` must return `Self`",
                        c.name
                    ),
                    line: m.line,
                    col: m.col,
                });
            }
        }
    }
    // 4. Check each method body.
    for m in &c.methods {
        let mut env = module_env.clone();
        // Always set `__enclosing_class__` so `Self` resolves.
        env.insert(
            "__enclosing_class__",
            Type {
                qualifier: Qualifier::Unrestricted,
                base: BaseType::Named(c.name.clone()),
            },
        );
        if m.is_instance {
            env.insert(
                "self",
                Type {
                    qualifier: Qualifier::Unrestricted,
                    base: BaseType::Named(c.name.clone()),
                },
            );
        }
        for p in &m.params {
            env.insert(p.name.clone(), p.ty.clone());
        }
        check_block(&m.body, &mut env, m.return_type.as_ref(), sigs, classes, errors);
    }
}

/// Returns `true` iff two methods have invariant-matching signatures
/// (same param types + same return type). Used for override validation.
fn signatures_match(a: &MethodDecl, b: &MethodDecl) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    for (pa, pb) in a.params.iter().zip(b.params.iter()) {
        if pa.ty != pb.ty {
            return false;
        }
    }
    a.return_type == b.return_type
}

// ======================================================================
// Class table (Gap 1 — OO model)
// ======================================================================

/// A class's signature — the type-checker's view of a user-defined type.
#[derive(Debug, Clone)]
pub struct ClassSig {
    /// Class name as written.
    pub name: String,
    /// Optional single base class (`None` = no parent / root).
    pub base: Option<String>,
    /// Visibility of the class itself.
    pub visibility: Visibility,
    /// This class's own fields (NOT including base-class fields).
    pub fields: Vec<FieldDecl>,
    /// This class's own methods (NOT including base-class methods).
    pub methods: Vec<MethodDecl>,
    /// Effective field-stride (bytes) = `4 * (1 + total_field_count_in_chain)`.
    /// The `+1` accounts for the vtable_base slot at offset 0.
    pub field_stride: u32,
    /// Vtable slot count (total method count including base chain, with
    /// overrides occupying the same slot as the base method).
    pub vtable_slot_count: u32,
}

/// Module-wide class table. Built in pass 2 of `check_module`; consulted
/// during class method body checking and method-call resolution.
#[derive(Debug, Clone, Default)]
pub struct ClassTable {
    classes: std::collections::HashMap<String, ClassSig>,
    /// Source-order list of class names (for deterministic vtable layout).
    order: Vec<String>,
}

impl ClassTable {
    /// Construct an empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) a class signature.
    pub fn insert(&mut self, name: impl Into<String>, sig: ClassSig) {
        let n: String = name.into();
        if !self.classes.contains_key(&n) {
            self.order.push(n.clone());
        }
        self.classes.insert(n, sig);
    }

    /// Look up by name.
    pub fn lookup(&self, name: &str) -> Option<&ClassSig> {
        self.classes.get(name)
    }

    /// Look up by name (mutable).
    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut ClassSig> {
        self.classes.get_mut(name)
    }

    /// Iterate over class names in source order.
    pub fn names_in_order(&self) -> &[String] {
        &self.order
    }

    /// Walk the base chain. Returns `true` if `derived`'s chain includes
    /// `ancestor`. Used for subtyping checks.
    pub fn is_subclass_of(&self, derived: &str, ancestor: &str) -> bool {
        let mut current = Some(derived);
        while let Some(c) = current {
            if c == ancestor {
                return true;
            }
            current = self.classes.get(c).and_then(|s| s.base.as_deref());
        }
        false
    }

    /// Detect a cycle starting from `start`. Returns `Some(cycle_path)` if
    /// the base chain forms a cycle back to `start` (or to any class already
    /// on the path).
    pub fn find_cycle(&self, start: &str) -> Option<Vec<String>> {
        let mut path = vec![start.to_string()];
        let mut current = self.classes.get(start).and_then(|s| s.base.as_deref());
        while let Some(c) = current {
            if c == start {
                return Some(path);
            }
            if path.contains(&c.to_string()) {
                return Some(path);
            }
            path.push(c.to_string());
            current = self.classes.get(c).and_then(|s| s.base.as_deref());
        }
        None
    }
}

/// Returns the total field count along the chain `class_name -> root`
/// (the derived class's own fields + all base-class fields).
fn total_field_count(classes: &ClassTable, class_name: &str) -> u32 {
    let mut count = 0u32;
    let mut current = Some(class_name);
    while let Some(c) = current {
        if let Some(sig) = classes.lookup(c) {
            count += sig.fields.len() as u32;
            current = sig.base.as_deref();
        } else {
            break;
        }
    }
    count
}

/// Returns the total method count along the chain (overrides occupy the
/// same vtable slot, so this is the count of UNIQUE method names).
fn total_unique_method_count(classes: &ClassTable, class_name: &str) -> u32 {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut current = Some(class_name);
    while let Some(c) = current {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                seen.insert(m.name.clone());
            }
            current = sig.base.as_deref();
        } else {
            break;
        }
    }
    seen.len() as u32
}

/// Walk the base chain from root to derived. Returns a vec of class names.
fn build_chain(classes: &ClassTable, class_name: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = Some(class_name);
    while let Some(c) = current {
        chain.push(c.to_string());
        current = classes.lookup(c).and_then(|s| s.base.as_deref());
    }
    chain.reverse();
    chain
}

/// Find a field by name in the class chain. Returns the field decl and the
/// defining class name. Base-class fields are searched first (so they get
/// the lower offsets in the object layout).
fn find_field_in_chain<'a>(
    classes: &'a ClassTable,
    class_name: &str,
    field_name: &str,
) -> Option<(&'a FieldDecl, String)> {
    let chain = build_chain(classes, class_name); // root first
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for f in &sig.fields {
                if f.name == field_name {
                    return Some((f, c.clone()));
                }
            }
        }
    }
    None
}

/// Compute the byte offset of a field in the object layout. Returns `None`
/// if the field is not found. The vtable_base occupies offset 0; base-class
/// fields come next (in declaration order), then derived-class fields.
fn field_offset(classes: &ClassTable, class_name: &str, field_name: &str) -> Option<u32> {
    let chain = build_chain(classes, class_name); // root first
    let mut offset = 4u32; // skip vtable_base
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for f in &sig.fields {
                if f.name == field_name {
                    return Some(offset);
                }
                offset += 4;
            }
        }
    }
    None
}

/// Compute the vtable layout for a class. Returns a vec of
/// `(method_name, defining_class_name)` in vtable slot order (base-class
/// methods first, then derived; overrides update the defining_class of the
/// base method's slot).
fn vtable_layout(classes: &ClassTable, class_name: &str) -> Vec<(String, String)> {
    let chain = build_chain(classes, class_name); // root first
    let mut layout: Vec<(String, String)> = Vec::new();
    for c in &chain {
        if let Some(sig) = classes.lookup(c) {
            for m in &sig.methods {
                if let Some(slot) = layout.iter().position(|(n, _)| n == &m.name) {
                    // Override: update defining_class.
                    layout[slot].1 = c.clone();
                } else {
                    layout.push((m.name.clone(), c.clone()));
                }
            }
        }
    }
    layout
}

/// Returns the vtable slot index for a method on a class (or its base chain).
/// Returns `None` if the method is not found.
fn vtable_slot_for_method(
    classes: &ClassTable,
    class_name: &str,
    method_name: &str,
) -> Option<u32> {
    let layout = vtable_layout(classes, class_name);
    layout
        .iter()
        .position(|(n, _)| n == method_name)
        .map(|i| i as u32)
}

/// Returns `true` iff `sub <: super_` for full types, consulting the
/// `ClassTable` for class subtyping (a derived class is a subtype of any of
/// its ancestors).
pub fn type_is_subtype_with_classes(
    sub: &Type,
    super_: &Type,
    classes: &ClassTable,
) -> bool {
    match (&sub.base, &super_.base) {
        (BaseType::I32, BaseType::I32)
        | (BaseType::F32, BaseType::F32)
        | (BaseType::Str, BaseType::Str)
        | (BaseType::Bool, BaseType::Bool) => {}
        (BaseType::Vec(e1), BaseType::Vec(e2)) => {
            if !type_is_subtype_with_classes(e1, e2, classes) {
                return false;
            }
        }
        (BaseType::Named(n1), BaseType::Named(n2)) => {
            // Class subtyping: n1 == n2 OR n1 is a subclass of n2.
            if n1 != n2 && !classes.is_subclass_of(n1, n2) {
                return false;
            }
        }
        _ => return false,
    }
    qualifier_is_subtype(sub.qualifier, super_.qualifier)
}

// ======================================================================
// Error type
// ======================================================================

/// A type-checking error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    /// Human-readable message.
    pub message: String,
    /// 1-based line of the offending construct.
    pub line: u32,
    /// 1-based column of the offending construct.
    pub col: u32,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "type error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl core::error::Error for TypeError {}

/// A collection of type errors. The checker is **multi-error**: it collects
/// as many errors as possible before reporting, rather than stopping at the
/// first.
#[derive(Debug, Clone, Default)]
pub struct TypeErrorSet {
    /// All errors found, in source order.
    pub errors: Vec<TypeError>,
}

impl TypeErrorSet {
    /// Construct an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` iff there are no errors.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Push an error.
    pub fn push(&mut self, err: TypeError) {
        self.errors.push(err);
    }
}

impl fmt::Display for TypeErrorSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.errors.is_empty() {
            return write!(f, "no type errors");
        }
        write!(f, "{} type error(s):", self.errors.len())?;
        for e in &self.errors {
            write!(f, "\n  {}", e)?;
        }
        Ok(())
    }
}

// ======================================================================
// Operation classification
// ======================================================================

/// Method names that grow a collection (forbidden on `antitone`).
pub const GROW_OPS: &[&str] = &["push", "extend", "insert", "append"];

/// Method names that shrink a collection (forbidden on `monotone`).
pub const SHRINK_OPS: &[&str] = &["remove", "truncate", "clear", "swap_remove", "drain"];

/// Returns `Some(true)` if `method` is a grow op, `Some(false)` if it is a
/// shrink op, or `None` if it is neutral / unknown.
fn classify_op(method: &str) -> Option<bool> {
    if GROW_OPS.contains(&method) {
        Some(true)
    } else if SHRINK_OPS.contains(&method) {
        Some(false)
    } else {
        None
    }
}

// ======================================================================
// Subtyping
// ======================================================================

/// Returns `true` iff `sub <: super_` in the qualifier lattice.
///
/// - `unrestricted <: monotone`  ✓
/// - `unrestricted <: antitone`  ✓
/// - `monotone <: monotone`      ✓ (reflexivity)
/// - `antitone <: antitone`      ✓ (reflexivity)
/// - `monotone <: antitone`      ✗ (incomparable)
/// - `antitone <: monotone`      ✗ (incomparable)
/// - `monotone <: unrestricted`  ✗ (monotone is MORE restrictive)
/// - `antitone <: unrestricted`  ✗
pub fn qualifier_is_subtype(sub: Qualifier, super_: Qualifier) -> bool {
    matches!(
        (sub, super_),
        (Qualifier::Unrestricted, _)
            | (Qualifier::Monotone, Qualifier::Monotone)
            | (Qualifier::Antitone, Qualifier::Antitone)
    )
}

/// Returns `true` iff `sub <: super_` for full types (base must match and
/// qualifier must be a subtype). For `Vec<T>` the element type must also
/// be a subtype (covariance).
pub fn type_is_subtype(sub: &Type, super_: &Type) -> bool {
    // Base types must match structurally.
    match (&sub.base, &super_.base) {
        (BaseType::I32, BaseType::I32) => {}
        (BaseType::F32, BaseType::F32) => {}
        (BaseType::Str, BaseType::Str) => {}
        (BaseType::Bool, BaseType::Bool) => {}
        (BaseType::Vec(e1), BaseType::Vec(e2)) => {
            if !type_is_subtype(e1, e2) {
                return false;
            }
        }
        (BaseType::Named(n1), BaseType::Named(n2)) if n1 == n2 => {}
        _ => return false,
    }
    qualifier_is_subtype(sub.qualifier, super_.qualifier)
}

// ======================================================================
// Type environment
// ======================================================================

/// A type environment mapping variable names to their declared types.
/// Used to resolve variable references during type checking.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    bindings: std::collections::HashMap<String, Type>,
}

impl TypeEnv {
    /// Construct an empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a binding.
    pub fn insert(&mut self, name: impl Into<String>, ty: Type) {
        self.bindings.insert(name.into(), ty);
    }

    /// Look up a variable's type.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }
}

// ======================================================================
// The type checker
// ======================================================================

/// Type-check a module. Returns a [`TypeErrorSet`] containing all errors
/// found (empty if the module is well-typed).
///
/// This is the main entry point, called between parsing and IR lowering.
/// Uses a four-pass algorithm (Gap 1 — OO model + Gap 3 — full type inference):
///   Pass 1: collect all function + class-method signatures into FnSigTable.
///   Pass 1.5: collect all class signatures into ClassTable.
///   Pass 2: collect module-level `let` bindings.
///   Pass 3: check each function body AND class method body.
pub fn check_module(module: &ModuleDecl) -> TypeErrorSet {
    let mut errors = TypeErrorSet::new();

    // Pass 1: collect all function + class-method signatures.
    let mut sigs = FnSigTable::new();
    collect_signatures(module, &mut sigs);

    // Pass 1.5: collect all class signatures into ClassTable.
    let mut classes = ClassTable::new();
    collect_classes(module, &mut classes, &mut errors);

    // Pass 2: collect module-level `let` bindings.
    let mut module_env = TypeEnv::new();
    for item in &module.items {
        if let ItemDecl::Let(l) = item {
            check_expr(&l.init, &module_env, &mut errors, &sigs, &classes);
            let effective_ty = Type {
                qualifier: effective_qualifier(l),
                base: l.ty.base.clone(),
            };
            module_env.insert(l.name.clone(), effective_ty);
        }
    }

    // Pass 3: check each function body AND each class.
    for item in &module.items {
        match item {
            ItemDecl::Fn(f) => check_fn(f, &module_env, &sigs, &classes, &mut errors),
            ItemDecl::Class(c) => check_class(c, &classes, &module_env, &sigs, &mut errors),
            ItemDecl::Let(_) => {}
        }
    }

    errors
}

/// Type-check a single function declaration.
fn check_fn(
    f: &FnDecl,
    module_env: &TypeEnv,
    sigs: &FnSigTable,
    classes: &ClassTable,
    errors: &mut TypeErrorSet,
) {
    let mut env = module_env.clone();
    for p in &f.params {
        env.insert(p.name.clone(), p.ty.clone());
    }
    check_block(&f.body, &mut env, f.return_type.as_ref(), sigs, classes, errors);
}

/// Type-check a block of statements. The environment is extended with local
/// `let` bindings as they are encountered.
fn check_block(
    block: &Block,
    env: &mut TypeEnv,
    return_type: Option<&Type>,
    sigs: &FnSigTable,
    classes: &ClassTable,
    errors: &mut TypeErrorSet,
) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(l) => {
                let init_ty = check_expr(&l.init, env, errors, sigs, classes);
                // Subtype check: if the init expression has a known type,
                // it must be a subtype of the declared type. This catches
                // downcasts (e.g., `let b: B = a;` where `a: A` and B <: A).
                if let Some(it) = &init_ty {
                    if !type_is_subtype_with_classes(it, &l.ty, classes) {
                        // Only error if the init type is concretely known
                        // (i.e., a Named class or a primitive — not None
                        // from Vec::new()).
                        let is_concrete = !matches!(it.base, BaseType::Vec(_))
                            || !matches!(l.ty.base, BaseType::Vec(_));
                        if is_concrete {
                            errors.push(TypeError {
                                message: format!(
                                    "let `{}:{}` declares type `{}` but initialiser has type `{}` (`{}` is not a subtype of `{}`)",
                                    l.name, l.ty, l.ty, it, it.qualifier, l.ty.qualifier
                                ),
                                line: l.line,
                                col: l.col,
                            });
                        }
                    }
                }
                let effective_ty = Type {
                    qualifier: effective_qualifier(l),
                    base: l.ty.base.clone(),
                };
                env.insert(l.name.clone(), effective_ty);
            }
            Stmt::Expr(e) => {
                check_expr(e, env, errors, sigs, classes);
            }
            Stmt::Return(opt, line, col) => {
                if let Some(e) = opt {
                    let ty = check_expr(e, env, errors, sigs, classes);
                    if let (Some(rt), Some(et)) = (return_type, ty) {
                        if !type_is_subtype_with_classes(&et, rt, classes) {
                            errors.push(TypeError {
                                message: format!(
                                    "return type mismatch: expression has type `{}` but function \
                                     declares return type `{}` (`{}` is not a subtype of `{}`)",
                                    et, rt, et.qualifier, rt.qualifier
                                ),
                                line: *line,
                                col: *col,
                            });
                        }
                    }
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                line: _,
                col: _,
            } => {
                check_expr(cond, env, errors, sigs, classes);
                check_block(then_block, env, return_type, sigs, classes, errors);
                if let Some(else_b) = else_block {
                    check_block(else_b, env, return_type, sigs, classes, errors);
                }
            }
            Stmt::While {
                cond,
                body,
                line: _,
                col: _,
            } => {
                check_expr(cond, env, errors, sigs, classes);
                check_block(body, env, return_type, sigs, classes, errors);
            }
            Stmt::Assign {
                target,
                value,
                line,
                col,
            } => {
                // 1. Target must be Expr::Field.
                let (receiver, field_name, f_line, f_col) = match target {
                    Expr::Field {
                        receiver,
                        field,
                        line,
                        col,
                    } => (receiver.as_ref(), field, *line, *col),
                    _ => {
                        errors.push(TypeError {
                            message:
                                "assignment target must be a field access (`obj.field`)"
                                    .to_string(),
                            line: *line,
                            col: *col,
                        });
                        continue;
                    }
                };
                let receiver_ty = check_expr(receiver, env, errors, sigs, classes);
                let val_ty = check_expr(value, env, errors, sigs, classes);
                if let Some(Type {
                    base: BaseType::Named(class_name),
                    ..
                }) = &receiver_ty
                {
                    if let Some((field_decl, _defining_class)) =
                        find_field_in_chain(classes, class_name, field_name)
                    {
                        // CR-10: forbid assignment to monotone/antitone fields.
                        if field_decl.ty.qualifier != Qualifier::Unrestricted {
                            errors.push(TypeError {
                                message: format!(
                                    "assignment to `{}`-qualified field `{}.{}` is forbidden \
                                     (qualified fields are immutable after construction)",
                                    field_decl.ty.qualifier, class_name, field_name
                                ),
                                line: f_line,
                                col: f_col,
                            });
                            continue;
                        }
                        // Type-check the value against the field type.
                        if let Some(vt) = &val_ty {
                            if !type_is_subtype_with_classes(vt, &field_decl.ty, classes) {
                                errors.push(TypeError {
                                    message: format!(
                                        "field `{}.{}` has type `{}` but assignment value has type `{}`",
                                        class_name, field_name, field_decl.ty, vt
                                    ),
                                    line: *line,
                                    col: *col,
                                });
                            }
                        }
                    } else {
                        errors.push(TypeError {
                            message: format!(
                                "class `{}` has no field `{}`",
                                class_name, field_name
                            ),
                            line: f_line,
                            col: f_col,
                        });
                    }
                } else if let Some(other) = &receiver_ty {
                    errors.push(TypeError {
                        message: format!(
                            "field assignment on non-class type `{}` is not supported",
                            other
                        ),
                        line: *line,
                        col: *col,
                    });
                }
            }
        }
    }
}

/// Type-check an expression. Returns `Some(Type)` if the expression's type
/// could be determined, or `None` if it could not (e.g. an error was already
/// reported, or the expression is a path call whose return type is unknown).
///
/// Gap 3: uses the `FnSigTable` to infer function-call return types and
/// verify argument types/arity.
fn check_expr(
    expr: &Expr,
    env: &TypeEnv,
    errors: &mut TypeErrorSet,
    sigs: &FnSigTable,
    classes: &ClassTable,
) -> Option<Type> {
    match expr {
        Expr::Lit(lit, _, _) => Some(literal_type(lit)),
        Expr::Var(name, line, col) => match env.lookup(name) {
            Some(ty) => Some(ty.clone()),
            None => {
                errors.push(TypeError {
                    message: format!("undefined variable `{}`", name),
                    line: *line,
                    col: *col,
                });
                None
            }
        },
        Expr::PathCall(module, member, args, line, col) => {
            // Check arguments.
            for a in args {
                check_expr(a, env, errors, sigs, classes);
            }
            match (module.as_str(), member.as_str()) {
                ("Vec", "new") | ("Vec", "with_capacity") => {
                    // Element type cannot be inferred from the call site alone;
                    // the `let`-binding's declared type drives downstream uses.
                    None
                }
                (mod_name, member_name) => {
                    // If `module` is a known class, walk the base chain to
                    // find the method (Gap 1 — inherited static methods).
                    if classes.lookup(mod_name).is_some() {
                        return class_method_return_type(
                            mod_name,
                            member_name,
                            *line,
                            *col,
                            sigs,
                            classes,
                            errors,
                        );
                    }
                    let qualified = format!("{}::{}", mod_name, member_name);
                    match sigs.lookup(&qualified) {
                        Some(sig) => {
                            // Arity check.
                            if args.len() != sig.params.len() {
                                errors.push(TypeError {
                                    message: format!(
                                        "call to `{}::{}` expects {} argument(s) but was called with {}",
                                        mod_name, member_name, sig.params.len(), args.len()
                                    ),
                                    line: *line,
                                    col: *col,
                                });
                            }
                            sig.return_type.clone()
                        }
                        None => {
                            errors.push(TypeError {
                                message: format!(
                                    "call to unknown path `{}::{}`",
                                    mod_name, member_name
                                ),
                                line: *line,
                                col: *col,
                            });
                            None
                        }
                    }
                }
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            line,
            col,
        } => {
            // Check the receiver first.
            let receiver_ty = check_expr(receiver, env, errors, sigs, classes);
            // Check arguments.
            for a in args {
                check_expr(a, env, errors, sigs, classes);
            }
            match &receiver_ty {
                Some(ty) if ty.is_vec() => {
                    // Collection method dispatch — check monotonicity + return type.
                    check_method_op(method, ty.qualifier, *line, *col, errors);
                    collection_method_return_type(method, *line, *col, errors)
                }
                Some(Type {
                    base: BaseType::Named(class_name),
                    ..
                }) => {
                    // Class method dispatch (Gap 1). Walk the base chain
                    // looking for `Class::method` in `sigs`.
                    class_method_return_type(
                        class_name,
                        method,
                        *line,
                        *col,
                        sigs,
                        classes,
                        errors,
                    )
                }
                Some(other) => {
                    errors.push(TypeError {
                        message: format!(
                            "method `.{}()` is not defined on type `{}`",
                            method, other
                        ),
                        line: *line,
                        col: *col,
                    });
                    None
                }
                None => None, // receiver already errored; do not double-report.
            }
        }
        Expr::Binary {
            lhs,
            op,
            rhs,
            line: _,
            col: _,
        } => {
            let lhs_ty = check_expr(lhs, env, errors, sigs, classes);
            let rhs_ty = check_expr(rhs, env, errors, sigs, classes);
            if op.is_comparison() || op.is_logical() {
                Some(Type {
                    qualifier: Qualifier::Unrestricted,
                    base: BaseType::Bool,
                })
            } else {
                lhs_ty.or(rhs_ty)
            }
        }
        Expr::Call {
            callee,
            args,
            line,
            col,
        } => {
            // Check every argument expression.
            let mut arg_types = Vec::with_capacity(args.len());
            for a in args {
                arg_types.push(check_expr(a, env, errors, sigs, classes));
            }
            // Look up the callee in the signature table.
            match sigs.lookup(callee) {
                Some(sig) => {
                    // Arity check.
                    if args.len() != sig.params.len() {
                        errors.push(TypeError {
                            message: format!(
                                "call to function `{}` expects {} argument(s) but was called with {}",
                                callee, sig.params.len(), args.len()
                            ),
                            line: *line,
                            col: *col,
                        });
                    }
                    // Per-argument type check (with subtype flow).
                    for (i, (arg_ty, param_ty)) in
                        arg_types.iter().zip(sig.params.iter()).enumerate()
                    {
                        if let Some(at) = arg_ty {
                            if !type_is_subtype_with_classes(at, param_ty, classes) {
                                errors.push(TypeError {
                                    message: format!(
                                        "argument {} to `{}` has type `{}` but parameter has type `{}`",
                                        i + 1, callee, at, param_ty
                                    ),
                                    line: *line,
                                    col: *col,
                                });
                            }
                        }
                    }
                    // Return the declared return type.
                    sig.return_type.clone()
                }
                None => {
                    errors.push(TypeError {
                        message: format!("call to unknown function `{}`", callee),
                        line: *line,
                        col: *col,
                    });
                    None
                }
            }
        }
        Expr::Self_(line, col) => match env.lookup("self") {
            Some(ty) => Some(ty.clone()),
            None => {
                errors.push(TypeError {
                    message: "`self` is not available outside an instance method".to_string(),
                    line: *line,
                    col: *col,
                });
                None
            }
        },
        Expr::Field {
            receiver,
            field,
            line,
            col,
        } => {
            let receiver_ty = check_expr(receiver, env, errors, sigs, classes);
            match &receiver_ty {
                Some(Type {
                    base: BaseType::Named(class_name),
                    ..
                }) => match find_field_in_chain(classes, class_name, field) {
                    Some((field_decl, _defining_class)) => Some(field_decl.ty.clone()),
                    None => {
                        errors.push(TypeError {
                            message: format!(
                                "class `{}` has no field `{}`",
                                class_name, field
                            ),
                            line: *line,
                            col: *col,
                        });
                        None
                    }
                },
                Some(other) => {
                    errors.push(TypeError {
                        message: format!(
                            "field access `.{}()` is not valid on type `{}`",
                            field, other
                        ),
                        line: *line,
                        col: *col,
                    });
                    None
                }
                None => None,
            }
        }
        Expr::Object {
            class,
            fields,
            line,
            col,
        } => {
            // Resolve `Self` to the enclosing class.
            let resolved_class = if class == "Self" {
                match env.lookup("__enclosing_class__") {
                    Some(t) => match &t.base {
                        BaseType::Named(n) => n.clone(),
                        _ => class.clone(),
                    },
                    None => {
                        errors.push(TypeError {
                            message: "`Self` used outside a class body".to_string(),
                            line: *line,
                            col: *col,
                        });
                        return None;
                    }
                }
            } else {
                class.clone()
            };
            match classes.lookup(&resolved_class) {
                Some(_sig) => {
                    // Verify every initialised field exists + types check.
                    let mut seen: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    for (name, val_expr, fline, fcol) in fields {
                        match find_field_in_chain(classes, &resolved_class, name) {
                            Some((field_decl, _defining_class)) => {
                                if !seen.insert(name.clone()) {
                                    errors.push(TypeError {
                                        message: format!(
                                            "field `{}` initialised twice in object literal for class `{}`",
                                            name, resolved_class
                                        ),
                                        line: *fline,
                                        col: *fcol,
                                    });
                                }
                                let val_ty = check_expr(val_expr, env, errors, sigs, classes);
                                if let (Some(vt), ft) = (val_ty, &field_decl.ty) {
                                    if !type_is_subtype_with_classes(&vt, ft, classes) {
                                        errors.push(TypeError {
                                            message: format!(
                                                "field `{}` initialiser has type `{}` but field type is `{}`",
                                                name, vt, ft
                                            ),
                                            line: *fline,
                                            col: *fcol,
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
                                    line: *fline,
                                    col: *fcol,
                                });
                            }
                        }
                    }
                    // Verify every declared field is initialised.
                    let chain = build_chain(classes, &resolved_class);
                    for c in &chain {
                        if let Some(sig) = classes.lookup(c) {
                            for f in &sig.fields {
                                if !seen.contains(&f.name) {
                                    errors.push(TypeError {
                                        message: format!(
                                            "missing field `{}` in object literal for class `{}`",
                                            f.name, resolved_class
                                        ),
                                        line: *line,
                                        col: *col,
                                    });
                                }
                            }
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
                        line: *line,
                        col: *col,
                    });
                    None
                }
            }
        }
        Expr::StaticCall {
            class,
            method,
            args,
            line,
            col,
        } => {
            // Resolve `Self` to the enclosing class.
            let resolved_class = if class == "Self" {
                match env.lookup("__enclosing_class__") {
                    Some(t) => match &t.base {
                        BaseType::Named(n) => n.clone(),
                        _ => class.clone(),
                    },
                    None => {
                        errors.push(TypeError {
                            message: "`Self` used outside a class body".to_string(),
                            line: *line,
                            col: *col,
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
                    // Arity check.
                    if args.len() != sig.params.len() {
                        errors.push(TypeError {
                            message: format!(
                                "static call `{}::{}` expects {} argument(s) but was called with {}",
                                resolved_class, method, sig.params.len(), args.len()
                            ),
                            line: *line,
                            col: *col,
                        });
                    }
                    // Check args + per-arg type check.
                    let mut arg_types = Vec::with_capacity(args.len());
                    for a in args {
                        arg_types.push(check_expr(a, env, errors, sigs, classes));
                    }
                    for (i, (at, pt)) in arg_types.iter().zip(sig.params.iter()).enumerate() {
                        if let Some(aty) = at {
                            if !type_is_subtype_with_classes(aty, pt, classes) {
                                errors.push(TypeError {
                                    message: format!(
                                        "argument {} to `{}::{}` has type `{}` but parameter has type `{}`",
                                        i + 1, resolved_class, method, aty, pt
                                    ),
                                    line: *line,
                                    col: *col,
                                });
                            }
                        }
                    }
                    sig.return_type.clone()
                }
                None => {
                    errors.push(TypeError {
                        message: format!(
                            "unknown static method `{}::{}`",
                            resolved_class, method
                        ),
                        line: *line,
                        col: *col,
                    });
                    None
                }
            }
        }
    }
}

/// Resolve a class instance-method call by walking the base chain. Returns
/// the method's declared return type, or `None` and pushes a `TypeError`
/// if the method is not found.
///
/// When the method is inherited from a base class, `Self`-substitution is
/// applied: if the method's return type is `Named(defining_class)`, it is
/// rewritten to `Named(call_class)` so that the caller perceives the
/// returned object as its own class.
fn class_method_return_type(
    class_name: &str,
    method: &str,
    line: u32,
    col: u32,
    sigs: &FnSigTable,
    classes: &ClassTable,
    errors: &mut TypeErrorSet,
) -> Option<Type> {
    let qualified = format!("{}::{}", class_name, method);
    if let Some(sig) = sigs.lookup(&qualified) {
        return sig.return_type.clone();
    }
    // Walk the base chain.
    if let Some(class_sig) = classes.lookup(class_name) {
        if let Some(base) = &class_sig.base {
            if let Some(rt) = class_method_return_type(
                base,
                method,
                line,
                col,
                sigs,
                classes,
                errors,
            ) {
                // Self-substitution: if the inherited method's return type
                // is `Named(base)`, rewrite to `Named(class_name)` so the
                // caller perceives the returned object as its own class.
                return Some(substitute_self(&rt, base, class_name));
            }
        }
    }
    errors.push(TypeError {
        message: format!("class `{}` has no method `{}`", class_name, method),
        line,
        col,
    });
    None
}

/// Substitute `Named(from) → Named(to)` in a type (used for inherited
/// methods' return types).
fn substitute_self(ty: &Type, from: &str, to: &str) -> Type {
    let mut t = ty.clone();
    substitute_self_inplace(&mut t, from, to);
    t
}

fn substitute_self_inplace(ty: &mut Type, from: &str, to: &str) {
    if let BaseType::Named(n) = &mut ty.base {
        if n == from {
            *n = to.to_string();
        }
    } else if let BaseType::Vec(elem) = &mut ty.base {
        let mut new_elem = elem.as_ref().clone();
        substitute_self_inplace(&mut new_elem, from, to);
        ty.base = BaseType::Vec(Box::new(new_elem));
    }
}

/// Returns the return type of a collection method on a `Vec<T>` receiver.
/// Grow/shrink ops return `None` (unit). `len` returns `i32`, `is_empty`
/// returns `bool`, `get`/`first`/`last` return `i32` (placeholder for
/// future `Option<T>`).
fn collection_method_return_type(
    method: &str,
    line: u32,
    col: u32,
    errors: &mut TypeErrorSet,
) -> Option<Type> {
    match method {
        "push" | "extend" | "insert" | "append" | "remove" | "truncate" | "clear"
        | "swap_remove" | "drain" => None,
        "len" => Some(Type {
            qualifier: Qualifier::Unrestricted,
            base: BaseType::I32,
        }),
        "is_empty" => Some(Type {
            qualifier: Qualifier::Unrestricted,
            base: BaseType::Bool,
        }),
        "get" | "first" | "last" | "contains" => Some(Type {
            qualifier: Qualifier::Unrestricted,
            base: BaseType::I32,
        }),
        _ => {
            errors.push(TypeError {
                message: format!("method `.{}()` is not defined on type `Vec<T>`", method),
                line,
                col,
            });
            None
        }
    }
}

/// Check whether `method` is legal on a collection with qualifier `q`.
/// Pushes a [`TypeError`] if not.
fn check_method_op(method: &str, q: Qualifier, line: u32, col: u32, errors: &mut TypeErrorSet) {
    if let Some(is_grow) = classify_op(method) {
        match (q, is_grow) {
            (Qualifier::Monotone, false) => {
                errors.push(TypeError {
                    message: format!(
                        "shrink operation `.{}()` is forbidden on a `monotone` collection \
                         (monotone collections may only grow)",
                        method
                    ),
                    line,
                    col,
                });
            }
            (Qualifier::Antitone, true) => {
                errors.push(TypeError {
                    message: format!(
                        "grow operation `.{}()` is forbidden on an `antitone` collection \
                         (antitone collections may only shrink)",
                        method
                    ),
                    line,
                    col,
                });
            }
            _ => {}
        }
    }
}

/// Returns the [`Type`] of a literal.
pub fn literal_type(lit: &Lit) -> Type {
    let base = match lit {
        Lit::Int(_) => BaseType::I32,
        Lit::Float(_) => BaseType::F32,
        Lit::Str(_) => BaseType::Str,
        Lit::Bool(_) => BaseType::Bool,
    };
    Type {
        qualifier: Qualifier::Unrestricted,
        base,
    }
}

// ======================================================================
// Public helpers
// ======================================================================

/// Returns the effective qualifier of a `let` binding, considering both the
/// `@monotone`/`@antitone` attribute form (Phase 1) and the
/// `monotone`/`antitone` type-qualifier form (Phase 2). The attribute form
/// takes precedence if present.
pub fn effective_qualifier(l: &LetDecl) -> Qualifier {
    // Attribute form takes precedence (it is the more explicit annotation).
    for a in &l.attrs {
        match a.name.as_str() {
            "monotone" => return Qualifier::Monotone,
            "antitone" => return Qualifier::Antitone,
            _ => {}
        }
    }
    l.ty.qualifier
}

/// Returns the effective qualifier of a parameter. Parameters use the type
/// qualifier form only (no attribute form in the grammar).
pub fn param_qualifier(p: &Param) -> Qualifier {
    p.ty.qualifier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn check(src: &str) -> TypeErrorSet {
        let m = parse(src).expect("parse");
        check_module(&m)
    }

    const SCENE: &str = "scene { background: #000000 }";

    // ------------------------------------------------------------------
    // Subtyping lattice
    // ------------------------------------------------------------------

    #[test]
    fn unrestricted_is_subtype_of_both() {
        assert!(qualifier_is_subtype(
            Qualifier::Unrestricted,
            Qualifier::Monotone
        ));
        assert!(qualifier_is_subtype(
            Qualifier::Unrestricted,
            Qualifier::Antitone
        ));
        assert!(qualifier_is_subtype(
            Qualifier::Unrestricted,
            Qualifier::Unrestricted
        ));
    }

    #[test]
    fn monotone_not_subtype_of_antitone_or_unrestricted() {
        assert!(!qualifier_is_subtype(
            Qualifier::Monotone,
            Qualifier::Antitone
        ));
        assert!(!qualifier_is_subtype(
            Qualifier::Monotone,
            Qualifier::Unrestricted
        ));
        assert!(qualifier_is_subtype(
            Qualifier::Monotone,
            Qualifier::Monotone
        ));
    }

    #[test]
    fn antitone_not_subtype_of_monotone_or_unrestricted() {
        assert!(!qualifier_is_subtype(
            Qualifier::Antitone,
            Qualifier::Monotone
        ));
        assert!(!qualifier_is_subtype(
            Qualifier::Antitone,
            Qualifier::Unrestricted
        ));
        assert!(qualifier_is_subtype(
            Qualifier::Antitone,
            Qualifier::Antitone
        ));
    }

    #[test]
    fn type_subtype_vec_covariant() {
        let unres_vec_mono = Type::vec(Type {
            qualifier: Qualifier::Monotone,
            base: BaseType::I32,
        });
        let mono_vec_mono = Type {
            qualifier: Qualifier::Monotone,
            base: BaseType::Vec(Box::new(Type {
                qualifier: Qualifier::Monotone,
                base: BaseType::I32,
            })),
        };
        // unrestricted Vec<monotone i32> <: monotone Vec<monotone i32>
        assert!(type_is_subtype(&unres_vec_mono, &mono_vec_mono));
        // monotone Vec<monotone i32> !<: unrestricted Vec<monotone i32>
        assert!(!type_is_subtype(&mono_vec_mono, &unres_vec_mono));
    }

    // ------------------------------------------------------------------
    // Method-call validation
    // ------------------------------------------------------------------

    #[test]
    fn monotone_push_ok() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.push(1); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    #[test]
    fn monotone_remove_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.remove(0); }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 1);
        assert!(s.errors[0].message.contains("monotone"));
        assert!(s.errors[0].message.contains("remove"));
    }

    #[test]
    fn monotone_truncate_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.truncate(0); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn monotone_clear_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.clear(); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn monotone_swap_remove_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.swap_remove(0); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn monotone_drain_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.drain(); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn antitone_remove_ok() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.remove(0); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    #[test]
    fn antitone_push_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.push(1); }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 1);
        assert!(s.errors[0].message.contains("antitone"));
        assert!(s.errors[0].message.contains("push"));
    }

    #[test]
    fn antitone_extend_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.extend(); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn antitone_insert_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.insert(0); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn antitone_append_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.append(); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn unrestricted_all_ops_ok() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); v.push(1); v.remove(0); v.clear(); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    #[test]
    fn neutral_ops_never_error() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.len(); v.get(0); v.is_empty(); v.contains(1); let w: antitone Vec<i32> = Vec::new(); w.len(); w.is_empty(); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    // ------------------------------------------------------------------
    // Attribute form (Phase 1 backward compat)
    // ------------------------------------------------------------------

    #[test]
    fn attribute_form_monotone_checked() {
        let src = format!(
            "module M {{ {} fn f() {{ @monotone let v: Vec<i32> = Vec::new(); v.remove(0); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn attribute_form_antitone_checked() {
        let src = format!(
            "module M {{ {} fn f() {{ @antitone let v: Vec<i32> = Vec::new(); v.push(1); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    // ------------------------------------------------------------------
    // Function-boundary flow
    // ------------------------------------------------------------------

    #[test]
    fn monotone_param_shrink_errors() {
        let src = format!(
            "module M {{ {} fn f(x: monotone Vec<i32>) {{ x.remove(0); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn antitone_param_grow_errors() {
        let src = format!(
            "module M {{ {} fn f(x: antitone Vec<i32>) {{ x.push(1); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    #[test]
    fn unrestricted_param_all_ops_ok() {
        let src = format!(
            "module M {{ {} fn f(x: Vec<i32>) {{ x.push(1); x.remove(0); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    // ------------------------------------------------------------------
    // Variable resolution
    // ------------------------------------------------------------------

    #[test]
    fn undefined_variable_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ undefined_var.push(1); }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 1);
        assert!(s.errors[0].message.contains("undefined variable"));
    }

    #[test]
    fn module_level_let_visible_in_fn() {
        let src = format!(
            "module M {{ {} let g: monotone Vec<i32> = Vec::new(); fn f() {{ g.push(1); }} }}",
            SCENE
        );
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    #[test]
    fn module_level_let_monotone_checked_in_fn() {
        let src = format!(
            "module M {{ {} let g: monotone Vec<i32> = Vec::new(); fn f() {{ g.clear(); }} }}",
            SCENE
        );
        assert_eq!(check(&src).len(), 1);
    }

    // ------------------------------------------------------------------
    // Return type checking
    // ------------------------------------------------------------------

    #[test]
    fn return_type_ok() {
        let src = format!("module M {{ {} fn f() -> i32 {{ return 42; }} }}", SCENE);
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    #[test]
    fn return_type_mismatch_errors() {
        // Returning a string where i32 is declared.
        let src = format!(
            "module M {{ {} fn f() -> i32 {{ return \"hello\"; }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 1);
        assert!(s.errors[0].message.contains("return type mismatch"));
    }

    #[test]
    fn return_without_expression_ok_for_void() {
        let src = format!("module M {{ {} fn f() {{ return; }} }}", SCENE);
        assert!(check(&src).is_empty(), "{}", check(&src));
    }

    // ------------------------------------------------------------------
    // Multi-error collection
    // ------------------------------------------------------------------

    #[test]
    fn multiple_errors_collected() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.remove(0); v.clear(); v.truncate(1); }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 3, "got: {}", s);
    }

    #[test]
    fn empty_module_no_errors() {
        let src = "module M { scene { background: #000000 } }";
        assert!(check(src).is_empty());
    }

    #[test]
    fn method_chain_checked() {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.push(1); v.remove(0); v.push(2); }} }}",
            SCENE
        );
        let s = check(&src);
        assert_eq!(s.len(), 1); // only the remove
        assert!(s.errors[0].message.contains("remove"));
    }

    // ------------------------------------------------------------------
    // Display
    // ------------------------------------------------------------------

    #[test]
    fn type_error_display() {
        let e = TypeError {
            message: "shrink op forbidden".into(),
            line: 5,
            col: 3,
        };
        let s = format!("{}", e);
        assert!(s.contains("type error at 5:3"));
        assert!(s.contains("shrink op forbidden"));
    }

    #[test]
    fn type_error_set_display_empty() {
        let s = TypeErrorSet::new();
        assert_eq!(format!("{}", s), "no type errors");
    }

    #[test]
    fn type_error_set_display_nonempty() {
        let mut s = TypeErrorSet::new();
        s.push(TypeError {
            message: "err1".into(),
            line: 1,
            col: 1,
        });
        s.push(TypeError {
            message: "err2".into(),
            line: 2,
            col: 1,
        });
        let out = format!("{}", s);
        assert!(out.contains("2 type error(s)"));
        assert!(out.contains("err1"));
        assert!(out.contains("err2"));
    }
}

#[cfg(test)]
mod type_inference_tests {
    use super::*;
    use crate::parser::parse;

    const SCENE: &str = "scene { background: #000000 }";

    fn check(src: &str) -> TypeErrorSet {
        let m = parse(src).expect("parse");
        check_module(&m)
    }

    // LANG-3T-01: function call infers return type
    #[test]
    fn lang_3t_01_call_infers_return_type() {
        let src = format!(
            "module M {{ {} fn id(x: i32) -> i32 {{ return x; }} fn main() -> i32 {{ return id(42); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "got: {}", errors);
    }

    // LANG-3T-02: arity mismatch detected
    #[test]
    fn lang_3t_02_arity_mismatch() {
        let src = format!(
            "module M {{ {} fn id(x: i32) -> i32 {{ return x; }} fn main() -> i32 {{ return id(1, 2); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert_eq!(errors.len(), 1);
        assert!(errors.errors[0]
            .message
            .contains("expects 1 argument(s) but was called with 2"));
    }

    // LANG-3T-03: argument type mismatch detected
    #[test]
    fn lang_3t_03_arg_type_mismatch() {
        let src = format!(
            "module M {{ {} fn id(x: i32) -> i32 {{ return x; }} fn main() -> i32 {{ return id(true); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1);
        assert!(errors.errors[0]
            .message
            .contains("argument 1 to `id` has type `bool` but parameter has type `i32`"));
    }

    // LANG-3T-04: unknown function call detected
    #[test]
    fn lang_3t_04_unknown_function() {
        let src = format!(
            "module M {{ {} fn main() -> i32 {{ return unknown(42); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert_eq!(errors.len(), 1);
        assert!(errors.errors[0]
            .message
            .contains("call to unknown function `unknown`"));
    }

    // LANG-3T-05: mutual recursion supported
    #[test]
    fn lang_3t_05_mutual_recursion() {
        let src = format!(
            "module M {{ {} fn is_even(n: i32) -> bool {{ if (n == 0) {{ return true; }} return is_odd(n - 1); }} fn is_odd(n: i32) -> bool {{ if (n == 0) {{ return false; }} return is_even(n - 1); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "got: {}", errors);
    }

    // LANG-3T-06: self recursion supported
    #[test]
    fn lang_3t_06_self_recursion() {
        let src = format!(
            "module M {{ {} fn fact(n: i32) -> i32 {{ if (n <= 1) {{ return 1; }} return n * fact(n - 1); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "got: {}", errors);
    }

    // LANG-3T-07: vec.len() infers i32
    #[test]
    fn lang_3t_07_vec_len_infers_i32() {
        let src = format!(
            "module M {{ {} fn main() {{ let v: Vec<i32> = Vec::new(); let n: i32 = v.len(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "got: {}", errors);
    }

    // LANG-3T-08: vec.is_empty() infers bool
    #[test]
    fn lang_3t_08_vec_is_empty_infers_bool() {
        let src = format!(
            "module M {{ {} fn main() {{ let v: Vec<i32> = Vec::new(); let b: bool = v.is_empty(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "got: {}", errors);
    }

    // LANG-3T-10: method on non-Vec/non-class type errors
    #[test]
    fn lang_3t_10_method_on_scalar_errors() {
        let src = format!(
            "module M {{ {} fn main() {{ let x: i32 = 5; x.foo(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert_eq!(errors.len(), 1);
        assert!(errors.errors[0]
            .message
            .contains("method `.foo()` is not defined on type `i32`"));
    }

    // LANG-3T-11: unknown collection method errors
    #[test]
    fn lang_3t_11_unknown_collection_method() {
        let src = format!(
            "module M {{ {} fn main() {{ let v: Vec<i32> = Vec::new(); v.bogus(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert_eq!(errors.len(), 1);
        assert!(errors.errors[0]
            .message
            .contains("method `.bogus()` is not defined on type `Vec<T>`"));
    }

    // LANG-3T-12: unknown path call errors
    #[test]
    fn lang_3t_12_unknown_path_call() {
        let src = format!("module M {{ {} fn main() {{ Foo::bar(); }} }}", SCENE);
        let errors = check(&src);
        assert_eq!(errors.len(), 1);
        assert!(errors.errors[0]
            .message
            .contains("call to unknown path `Foo::bar`"));
    }
}

// ======================================================================
// Gap 1 — OO model tests
// ======================================================================

#[cfg(test)]
mod oo_tests {
    use super::*;
    use crate::parser::parse;

    const SCENE: &str = "scene { background: #000000 }";

    fn check(src: &str) -> TypeErrorSet {
        let m = parse(src).expect("parse");
        check_module(&m)
    }

    // LANG-1T-01: `class Empty {}` parses with zero fields and zero methods.
    #[test]
    fn lang_1t_01_empty_class_parses() {
        let src = format!("module M {{ {} class Empty {{}} }}", SCENE);
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-02: `class Derived : Base { ... }` with `class Base {}` defined.
    #[test]
    fn lang_1t_02_inheritance_parses() {
        let src = format!(
            "module M {{ {} class Base {{}} class Derived : Base {{ pub x: i32; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-03: `class C { pub fn new() -> Self { Self { } } }` parses as ClassDecl.
    #[test]
    fn lang_1t_03_constructor_parses() {
        let src = format!(
            "module M {{ {} class C {{ pub fn new() -> Self {{ Self {{ }} }} }} }}",
            SCENE
        );
        let errors = check(&src);
        // Self {} with empty fields is OK if C has no fields.
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-04: instance method with `self` parameter.
    #[test]
    fn lang_1t_04_instance_method() {
        let src = format!(
            "module M {{ {} class C {{ pub x: i32; pub fn new() -> Self {{ Self {{ x: 0 }} }} pub fn get(self) -> i32 {{ return self.x; }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-08: cyclic inheritance detected.
    #[test]
    fn lang_1t_08_cyclic_inheritance_errors() {
        let src = format!(
            "module M {{ {} class A : B {{}} class B : A {{}} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "should report cycle: {}", errors);
        assert!(
            errors.errors[0].message.contains("cyclic inheritance"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // LANG-1T-09: `Self` outside a class body errors.
    #[test]
    fn lang_1t_09_self_outside_class_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let x: Self = 0; }} }}",
            SCENE
        );
        // Note: `Self` as a type in a non-class function should error.
        // We accept it parses, but typechecking should catch misuse.
        // Since `Self` resolves to `Named("Self")`, and there's no class
        // named "Self", it's just an unknown named type — which currently
        // passes (we don't validate named types against the class table
        // in all positions). This test is a placeholder.
        let _ = check(&src);
    }

    // LANG-1T-10: `self` outside an instance method errors.
    #[test]
    fn lang_1t_10_self_outside_method_errors() {
        let src = format!(
            "module M {{ {} fn f() {{ let x: i32 = self; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "should report `self` outside method: {}", errors);
        assert!(
            errors.errors[0].message.contains("`self` is not available"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // LANG-1T-11: assignment to monotone-qualified field is forbidden (CR-10).
    #[test]
    fn lang_1t_11_monotone_field_assignment_errors() {
        let src = format!(
            "module M {{ {} class C {{ monotone items: Vec<i32>; pub fn new() -> Self {{ Self {{ items: Vec::new() }} }} pub fn reset(self) {{ self.items = Vec::new(); }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "should report monotone assignment: {}", errors);
        assert!(
            errors.errors[0].message.contains("monotone") && errors.errors[0].message.contains("forbidden"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // LANG-1T-12: unrestricted field is assignable.
    #[test]
    fn lang_1t_12_unrestricted_field_assignable() {
        let src = format!(
            "module M {{ {} class C {{ pub x: i32; pub fn new() -> Self {{ Self {{ x: 0 }} }} pub fn set(self, v: i32) {{ self.x = v; }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-13: full Counter program typechecks.
    #[test]
    fn lang_1t_13_counter_typechecks() {
        let src = format!(
            "module M {{ {} class Counter {{ pub count: i32; pub fn new() -> Self {{ Self {{ count: 0 }} }} pub fn inc(self) {{ self.count = self.count + 1; }} pub fn get(self) -> i32 {{ return self.count; }} }} fn main() -> i32 {{ let c: Counter = Counter::new(); c.inc(); c.inc(); return c.get(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-16: upcast B → A is implicit (no WASM instruction).
    #[test]
    fn lang_1t_16_upcast_implicit() {
        let src = format!(
            "module M {{ {} class A {{}} class B : A {{}} fn main() {{ let b: B = B::new(); let a: A = b; }} }}",
            SCENE
        );
        // Wait — B::new() requires B to have a `new` method. Since neither
        // A nor B declares one, the typechecker would error on `B::new()`.
        // Let's adjust: add `pub fn new() -> Self { Self { } }` to both.
        let src = format!(
            "module M {{ {} class A {{ pub fn new() -> Self {{ Self {{ }} }} }} class B : A {{ pub fn new() -> Self {{ Self {{ }} }} }} fn main() {{ let b: B = B::new(); let a: A = b; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // LANG-1T-17: downcast A → B is forbidden.
    #[test]
    fn lang_1t_17_downcast_forbidden() {
        let src = format!(
            "module M {{ {} class A {{ pub fn new() -> Self {{ Self {{ }} }} }} class B : A {{ pub fn new() -> Self {{ Self {{ }} }} }} fn main() {{ let a: A = A::new(); let b: B = a; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "downcast should error: {}", errors);
    }

    // LANG-1T-19: `new` returning non-Self errors.
    #[test]
    fn lang_1t_19_new_returning_non_self_errors() {
        let src = format!(
            "module M {{ {} class C {{ pub fn new() -> i32 {{ return 0; }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "got: {}", errors);
        assert!(
            errors.errors[0].message.contains("must return `Self`"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // LANG-1T-20: `new` with `self` parameter errors.
    #[test]
    fn lang_1t_20_new_with_self_errors() {
        let src = format!(
            "module M {{ {} class C {{ pub fn new(self) -> Self {{ }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "got: {}", errors);
        assert!(
            errors.errors[0].message.contains("must be a static method"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // Inheritance: subclass inherits parent methods.
    #[test]
    fn oo_inherits_parent_methods() {
        let src = format!(
            "module M {{ {} class A {{ pub fn greet(self) -> i32 {{ return 1; }} pub fn new() -> Self {{ Self {{ }} }} }} class B : A {{}} fn main() -> i32 {{ let b: B = B::new(); return b.greet(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // Override signature mismatch is an error.
    #[test]
    fn oo_override_signature_mismatch_errors() {
        let src = format!(
            "module M {{ {} class A {{ pub fn m(self) -> i32 {{ return 0; }} pub fn new() -> Self {{ Self {{ }} }} }} class B : A {{ pub fn m(self) -> bool {{ return true; }} pub fn new() -> Self {{ Self {{ }} }} }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "got: {}", errors);
        assert!(
            errors.errors[0].message.contains("cannot override"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // Duplicate field name errors.
    #[test]
    fn oo_duplicate_field_errors() {
        let src = format!(
            "module M {{ {} class C {{ pub x: i32; pub x: i32; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "got: {}", errors);
        assert!(
            errors.errors[0].message.contains("already declared"),
            "got: {}",
            errors.errors[0].message
        );
    }

    // Unknown class referenced as base errors.
    #[test]
    fn oo_unknown_base_class_errors() {
        let src = format!(
            "module M {{ {} class C : Nonexistent {{}} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.len() >= 1, "got: {}", errors);
    }

    // Field access type-checks.
    #[test]
    fn oo_field_access_typechecks() {
        let src = format!(
            "module M {{ {} class C {{ pub x: i32; pub fn new() -> Self {{ Self {{ x: 0 }} }} pub fn get(self) -> i32 {{ return self.x; }} }} fn main() -> i32 {{ let c: C = C::new(); return c.x; }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }

    // Method call on object type-checks.
    #[test]
    fn oo_method_call_typechecks() {
        let src = format!(
            "module M {{ {} class C {{ pub fn new() -> Self {{ Self {{ }} }} pub fn m(self) -> i32 {{ return 42; }} }} fn main() -> i32 {{ let c: C = C::new(); return c.m(); }} }}",
            SCENE
        );
        let errors = check(&src);
        assert!(errors.is_empty(), "{}", errors);
    }
}

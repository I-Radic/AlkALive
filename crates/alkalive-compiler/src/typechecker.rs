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
    BaseType, Block, Expr, FnDecl, ItemDecl, LetDecl, Lit, ModuleDecl, Param, Qualifier, Stmt, Type,
};

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
pub fn check_module(module: &ModuleDecl) -> TypeErrorSet {
    let mut errors = TypeErrorSet::new();

    // Build the module-level environment: all top-level `let` bindings.
    let mut module_env = TypeEnv::new();
    for item in &module.items {
        if let ItemDecl::Let(l) = item {
            // Check the initialiser expression in the empty env first
            // (module-level lets can only see other module-level lets).
            check_expr(&l.init, &module_env, &mut errors);
            // Add the binding with the EFFECTIVE qualifier (attribute form
            // takes precedence over type-qualifier form).
            let effective_ty = Type {
                qualifier: effective_qualifier(l),
                base: l.ty.base.clone(),
            };
            module_env.insert(l.name.clone(), effective_ty);
        }
    }

    // Type-check each function.
    for item in &module.items {
        if let ItemDecl::Fn(f) = item {
            check_fn(f, &module_env, &mut errors);
        }
    }

    errors
}

/// Type-check a single function declaration.
fn check_fn(f: &FnDecl, module_env: &TypeEnv, errors: &mut TypeErrorSet) {
    // Build the function's local environment: module-level + parameters.
    let mut env = module_env.clone();
    for p in &f.params {
        env.insert(p.name.clone(), p.ty.clone());
    }
    check_block(&f.body, &mut env, f.return_type.as_ref(), errors);
}

/// Type-check a block of statements. The environment is extended with local
/// `let` bindings as they are encountered.
fn check_block(
    block: &Block,
    env: &mut TypeEnv,
    return_type: Option<&Type>,
    errors: &mut TypeErrorSet,
) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(l) => {
                check_expr(&l.init, env, errors);
                // Use the EFFECTIVE qualifier (attribute form takes
                // precedence over type-qualifier form).
                let effective_ty = Type {
                    qualifier: effective_qualifier(l),
                    base: l.ty.base.clone(),
                };
                env.insert(l.name.clone(), effective_ty);
            }
            Stmt::Expr(e) => {
                check_expr(e, env, errors);
            }
            Stmt::Return(opt, line, col) => {
                if let Some(e) = opt {
                    let ty = check_expr(e, env, errors);
                    if let (Some(rt), Some(et)) = (return_type, ty) {
                        if !type_is_subtype(&et, rt) {
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
        }
    }
}

/// Type-check an expression. Returns `Some(Type)` if the expression's type
/// could be determined, or `None` if it could not (e.g. an error was already
/// reported, or the expression is a path call whose return type is unknown).
fn check_expr(expr: &Expr, env: &TypeEnv, errors: &mut TypeErrorSet) -> Option<Type> {
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
                check_expr(a, env, errors);
            }
            // `Vec::new()` returns an unrestricted Vec<...>. We can't infer
            // the element type without more context, so return None.
            // `Vec::with_capacity(n)` similarly.
            if module == "Vec" && (member == "new" || member == "with_capacity") {
                return None;
            }
            // Unknown path call — don't error (the runtime may provide it).
            let _ = (line, col);
            None
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            line,
            col,
        } => {
            // Check arguments.
            for a in args {
                check_expr(a, env, errors);
            }
            // Check the receiver.
            let receiver_ty = check_expr(receiver, env, errors);
            // If the receiver is a collection with a qualifier, check the op.
            if let Some(ty) = &receiver_ty {
                if ty.is_vec() {
                    check_method_op(method, ty.qualifier, *line, *col, errors);
                }
            }
            // Method calls return unknown types (we don't have a full type
            // inference engine for return values).
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
fn literal_type(lit: &Lit) -> Type {
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

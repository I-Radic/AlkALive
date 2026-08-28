//! Monotonicity lint (ADR-027 Phase 1).
//!
//! This lint scans the AST for `@monotone` / `@antitone` attribute
//! annotations and emits findings for misuse.
//!
//! # Phase 1 checks
//!
//! 1. **Unknown monotonicity attribute name.** Any `@ident` attribute
//!    whose name is not `monotone` or `antitone` produces a warning.
//!    This catches typos like `@monotonic` or `@antitone_x`.
//!
//! 2. **`#![deny(monotonicity)]` is honoured.** When the module carries
//!    this file-level attribute, all `Warning` findings emitted by this
//!    lint are upgraded to `Deny` (hard errors) by [`super::LintSet::add`].
//!
//! 3. **Illegal operations on attributed collections.** Spec §4.4: for
//!    every `let` binding carrying `@monotone` / `@antitone` (top-level
//!    or function-local), the lint scans the function scopes for illegal
//!    method calls — shrinking operations (`.remove()`, `.truncate()`,
//!    `.clear()`, `.swap_remove()`, `.drain()`) on `@monotone`
//!    collections and growing operations (`.push()`, `.extend()`,
//!    `.insert()`, `.append()`) on `@antitone` collections — and emits
//!    advisory `Warning` findings at the call site. This is the Phase 1
//!    *standalone* contract: no type checker is required, so users who
//!    adopted attribute annotations get advisory findings from
//!    `compile_with_lints` / `--lint` alone. The Phase 2 type checker
//!    remains the hard-error authority (it enforces the same operation
//!    sets for both qualifiers *and* attributes via
//!    `effective_qualifier`).
//!
//! 4. **`@monotone` / `@antitone` on a scene node declaration** emits a
//!    warning: scene nodes (text / input-field) are not collections, so
//!    the attribute has no effect there.

#![forbid(unsafe_code)]

use super::{LintReport, LintSet, LintSeverity};
use crate::ast::{Block, Expr, ItemDecl, LetDecl, ModuleDecl, NodeDecl, Stmt};
use crate::typechecker::{GROW_OPS, SHRINK_OPS};

/// The set of recognised monotonicity attribute names.
///
/// Any `@ident` attribute whose name is NOT in this set will trigger an
/// "unknown monotonicity attribute" warning.
pub const KNOWN_ATTRIBUTES: &[&str] = &["monotone", "antitone"];

/// Lint name used in the `monotonicity:` prefix of every finding message
/// produced by this pass.
pub const LINT_NAME: &str = "monotonicity";

/// Run the monotonicity lint, adding findings to `set`.
///
/// This function walks the module's items and scene (if any):
///
/// - Scene node declarations have their attributes linted for placement
///   (unknown names, non-collection targets).
/// - Every `let` binding carrying a `@monotone` / `@antitone` attribute
///   (top-level or function-local) is recorded, and all function bodies
///   are scanned for illegal method calls on those bindings (see the
///   [module-level docs](self)).
pub fn run(ast: &ModuleDecl, set: &mut LintSet) {
    let Some(scene) = ast.scene.as_ref() else {
        lint_items(ast, set);
        return;
    };

    // Walk every node declaration and inspect its attributes.
    for node in &scene.nodes {
        lint_node(node, set);
    }

    // The scene block itself may carry attributes (rare). Lint them too.
    for attr in &scene.attributes {
        lint_scene_attribute(attr, set);
    }

    lint_items(ast, set);
}

/// Lint the module's top-level items: record attributed `let` bindings
/// and scan every function body for illegal operations on them.
fn lint_items(ast: &ModuleDecl, set: &mut LintSet) {
    // 1. Collect every attributed `let` binding (top-level + the local
    //    lets inside each function body), name → ("monotone"|"antitone").
    //    Attribute-based only: this is the Phase 1 standalone contract
    //    (the Phase 2 type checker is the hard-error authority for
    //    qualifier-based declarations).
    let mut attributed: Vec<(String, String, u32, u32)> = Vec::new(); // (name, kind, line, col)
    for item in &ast.items {
        match item {
            ItemDecl::Let(l) => collect_attributed_let(l, &mut attributed),
            ItemDecl::Fn(f) => {
                collect_attributed_block(&f.body, &mut attributed);
            }
            ItemDecl::Class(c) => {
                // OO class methods have bodies too: record their local
                // attributed lets, then scan their bodies for calls below
                // (same module-wide name set).
                for method in &c.methods {
                    collect_attributed_block(&method.body, &mut attributed);
                }
            }
        }
    }

    // 2. Scan every function body (top-level fns + class methods) for
    //    illegal method calls on the attributed names.
    for item in &ast.items {
        if let ItemDecl::Fn(f) = item {
            scan_block(&f.body, &attributed, set);
        }
    }
    for item in &ast.items {
        if let ItemDecl::Class(c) = item {
            for method in &c.methods {
                scan_block(&method.body, &attributed, set);
            }
        }
    }
}

/// Record an attributed `let` binding if it carries `@monotone` or
/// `@antitone`.
fn collect_attributed_let(l: &LetDecl, out: &mut Vec<(String, String, u32, u32)>) {
    for attr in &l.attrs {
        if attr.name == "monotone" || attr.name == "antitone" {
            out.push((l.name.clone(), attr.name.clone(), attr.line, attr.col));
        }
    }
}

/// Record every attributed `let` binding inside a block (recursively,
/// including nested if/while blocks).
fn collect_attributed_block(block: &Block, out: &mut Vec<(String, String, u32, u32)>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(l) => collect_attributed_let(l, out),
            Stmt::Expr(_) | Stmt::Return(..) => {}
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                collect_attributed_block(then_block, out);
                if let Some(eb) = else_block {
                    collect_attributed_block(eb, out);
                }
            }
            Stmt::While { body, .. } => collect_attributed_block(body, out),
            Stmt::Assign { .. } => {}
        }
    }
}

/// Scan a block's statements and (recursively) expressions for illegal
/// method calls on attributed collection names.
fn scan_block(block: &Block, attributed: &[(String, String, u32, u32)], set: &mut LintSet) {
    for stmt in &block.stmts {
        scan_stmt(stmt, attributed, set);
    }
}

fn scan_stmt(stmt: &Stmt, attributed: &[(String, String, u32, u32)], set: &mut LintSet) {
    match stmt {
        Stmt::Let(l) => scan_expr(&l.init, attributed, set),
        Stmt::Expr(e) => scan_expr(e, attributed, set),
        Stmt::Return(Some(e), ..) => scan_expr(e, attributed, set),
        Stmt::Return(None, ..) => {}
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            scan_expr(cond, attributed, set);
            scan_block(then_block, attributed, set);
            if let Some(eb) = else_block {
                scan_block(eb, attributed, set);
            }
        }
        Stmt::While { cond, body, .. } => {
            scan_expr(cond, attributed, set);
            scan_block(body, attributed, set);
        }
        Stmt::Assign { value, .. } => scan_expr(value, attributed, set),
    }
}

/// Scan an expression tree for illegal method calls. Only direct
/// `name.method(...)` calls on attributed bindings match; the walk
/// recurses into sub-expressions (operators, call arguments, nested
/// method calls) so calls like `helper(x.remove(0))` are found too.
fn scan_expr(expr: &Expr, attributed: &[(String, String, u32, u32)], set: &mut LintSet) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            line,
            col,
        } => {
            // Check the receiver before recursing.
            if let Expr::Var(name, ..) = receiver.as_ref() {
                if let Some((_, kind, decl_line, decl_col)) =
                    attributed.iter().find(|(n, ..)| n == name)
                {
                    let illegal = match kind.as_str() {
                        "monotone" => SHRINK_OPS.contains(&method.as_str()),
                        "antitone" => GROW_OPS.contains(&method.as_str()),
                        _ => false,
                    };
                    if illegal {
                        set.add(LintReport::new(
                            LintSeverity::Warning,
                            format!(
                                "{}: illegal `{}` on `@{}` collection `{}` (declared at \
                                 {}:{}); `{}` collections must only {}",
                                LINT_NAME,
                                method,
                                kind,
                                name,
                                decl_line,
                                decl_col,
                                kind,
                                if *kind == "monotone" {
                                    "grow (or stay the same size)"
                                } else {
                                    "shrink (or stay the same size)"
                                }
                            ),
                            *line,
                            *col,
                        ));
                    }
                }
            }
            // Recurse into receiver and arguments (nested calls).
            scan_expr(receiver, attributed, set);
            for arg in args {
                scan_expr(arg, attributed, set);
            }
        }
        Expr::Binary { lhs, rhs, .. } => {
            scan_expr(lhs, attributed, set);
            scan_expr(rhs, attributed, set);
        }
        Expr::PathCall(_, _, args, ..) | Expr::Call { args, .. } => {
            for arg in args {
                scan_expr(arg, attributed, set);
            }
        }
        Expr::Lit(..) | Expr::Var(..) | Expr::Self_(..) => {}
        Expr::Field { receiver, .. } => scan_expr(receiver, attributed, set),
        Expr::Object { fields, .. } => {
            for (_, value, ..) in fields {
                scan_expr(value, attributed, set);
            }
        }
        Expr::StaticCall { args, .. } => {
            for arg in args {
                scan_expr(arg, attributed, set);
            }
        }
    }
}

fn lint_node(node: &NodeDecl, set: &mut LintSet) {
    let (kind_label, line, col) = match node {
        NodeDecl::Text(t) => ("text node", t.line, t.col),
        NodeDecl::InputField(f) => ("input-field node", f.line, f.col),
    };

    for attr in node.attributes() {
        if !KNOWN_ATTRIBUTES.contains(&attr.name.as_str()) {
            set.add(LintReport::new(
                LintSeverity::Warning,
                format!(
                    "{}: unknown monotonicity attribute `@{}`; \
                     recognised names are `monotone` and `antitone`",
                    LINT_NAME, attr.name
                ),
                attr.line,
                attr.col,
            ));
            continue;
        }

        // Scene nodes (text / input-field) are not collections: the
        // monotonicity attribute has no effect on them. Emit a warning
        // pointing users at `let` collection declarations, where the
        // attribute is meaningful.
        set.add(LintReport::new(
            LintSeverity::Warning,
            format!(
                "{}: `@{}` applied to {} at {}:{}; scene nodes are not \
                 collections, so this attribute has no effect here — apply it \
                 to a `let` collection declaration instead",
                LINT_NAME, attr.name, kind_label, line, col
            ),
            attr.line,
            attr.col,
        ));
    }
}

fn lint_scene_attribute(attr: &crate::ast::Attribute, set: &mut LintSet) {
    // The scene block rarely carries attributes; if it does, treat them
    // the same way as node attributes.
    if !KNOWN_ATTRIBUTES.contains(&attr.name.as_str()) {
        set.add(LintReport::new(
            LintSeverity::Warning,
            format!(
                "{}: unknown monotonicity attribute `@{}` on `scene` block; \
                 recognised names are `monotone` and `antitone`",
                LINT_NAME, attr.name
            ),
            attr.line,
            attr.col,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Attribute, InputFieldNode, ModuleDecl, NodeDecl, SceneDecl, TextNode};

    fn module_with_text(attrs: Vec<Attribute>) -> ModuleDecl {
        ModuleDecl {
            name: "M".into(),
            scene: Some(SceneDecl {
                background: None,
                nodes: vec![NodeDecl::Text(TextNode {
                    content: "x".into(),
                    color: None,
                    font_size: None,
                    rotation: None,
                    position: None,
                    attributes: attrs,
                    line: 2,
                    col: 3,
                })],
                attributes: Vec::new(),
                line: 1,
                col: 1,
            }),
            attributes: Vec::new(),
            items: Vec::new(),
            imports: Vec::new(),
            line: 1,
            col: 1,
        }
    }

    #[test]
    fn empty_module_produces_no_findings() {
        let m = ModuleDecl {
            name: "M".into(),
            scene: None,
            attributes: Vec::new(),
            items: Vec::new(),
            imports: Vec::new(),
            line: 1,
            col: 1,
        };
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn module_with_scene_but_no_attrs_produces_no_findings() {
        let m = module_with_text(Vec::new());
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn monotone_attr_on_text_node_warns() {
        let m = module_with_text(vec![Attribute::new("monotone", 5, 1)]);
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert_eq!(set.len(), 1);
        assert_eq!(set.reports[0].severity, LintSeverity::Warning);
        assert!(
            set.reports[0].message.contains("@monotone"),
            "got: {}",
            set.reports[0].message
        );
        assert!(!set.has_errors());
    }

    #[test]
    fn antitone_attr_on_input_field_warns() {
        let m = ModuleDecl {
            name: "M".into(),
            scene: Some(SceneDecl {
                background: None,
                nodes: vec![NodeDecl::InputField(InputFieldNode {
                    placeholder: None,
                    position: None,
                    attributes: vec![Attribute::new("antitone", 4, 5)],
                    line: 3,
                    col: 1,
                })],
                attributes: Vec::new(),
                line: 1,
                col: 1,
            }),
            attributes: Vec::new(),
            items: Vec::new(),
            imports: Vec::new(),
            line: 1,
            col: 1,
        };
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert_eq!(set.len(), 1);
        assert!(
            set.reports[0].message.contains("@antitone"),
            "got: {}",
            set.reports[0].message
        );
    }

    #[test]
    fn unknown_attr_name_produces_warning() {
        let m = module_with_text(vec![Attribute::new("monotonic", 1, 1)]);
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert_eq!(set.len(), 1);
        assert!(
            set.reports[0].message.contains("unknown"),
            "got: {}",
            set.reports[0].message
        );
        assert!(
            set.reports[0].message.contains("monotonic"),
            "got: {}",
            set.reports[0].message
        );
    }

    #[test]
    fn deny_monotonicity_upgrades_warning_to_deny() {
        let m = ModuleDecl {
            name: "M".into(),
            scene: Some(SceneDecl {
                background: None,
                nodes: vec![NodeDecl::Text(TextNode {
                    content: "x".into(),
                    color: None,
                    font_size: None,
                    rotation: None,
                    position: None,
                    attributes: vec![Attribute::new("monotone", 5, 1)],
                    line: 2,
                    col: 3,
                })],
                attributes: Vec::new(),
                line: 1,
                col: 1,
            }),
            attributes: vec![Attribute::new("deny(monotonicity)", 1, 1)],
            items: Vec::new(),
            imports: Vec::new(),
            line: 1,
            col: 1,
        };
        let mut set = LintSet::new();
        set.deny_monotonicity = true; // simulate pre-scan
        run(&m, &mut set);
        assert!(set.has_errors());
        assert_eq!(set.reports[0].severity, LintSeverity::Deny);
    }

    #[test]
    fn multiple_attrs_produce_multiple_findings() {
        let m = module_with_text(vec![
            Attribute::new("monotone", 1, 1),
            Attribute::new("antitone", 1, 11),
        ]);
        let mut set = LintSet::new();
        run(&m, &mut set);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn known_attributes_constant_includes_monotone_and_antitone() {
        assert!(KNOWN_ATTRIBUTES.contains(&"monotone"));
        assert!(KNOWN_ATTRIBUTES.contains(&"antitone"));
    }

    // ---- Illegal-operation scan (spec §4.4 Phase 1 contract) ----

    /// Helper: parse source, run the lint, return the LintSet.
    fn lint_src(src: &str) -> LintSet {
        let ast = crate::parser::parse(src).expect("parse ok");
        let mut set = LintSet::new();
        run(&ast, &mut set);
        set
    }

    #[test]
    fn op_scan_warns_on_shrink_of_monotone_collection() {
        let set = lint_src(
            "module M { @monotone let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.remove(0); } }",
        );
        assert_eq!(set.len(), 1, "got: {:?}", set.reports);
        assert_eq!(set.reports[0].severity, LintSeverity::Warning);
        assert!(
            set.reports[0].message.contains("illegal `remove` on `@monotone`"),
            "got: {}",
            set.reports[0].message
        );
        assert!(!set.has_errors());
    }

    #[test]
    fn op_scan_allows_grow_on_monotone_collection() {
        let set = lint_src(
            "module M { @monotone let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.push(1); } }",
        );
        assert!(
            set.is_empty(),
            "push on @monotone is legal; got: {:?}",
            set.reports
        );
    }

    #[test]
    fn op_scan_warns_on_grow_of_antitone_collection() {
        let set = lint_src(
            "module M { @antitone let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.push(1); } }",
        );
        assert_eq!(set.len(), 1, "got: {:?}", set.reports);
        assert!(
            set.reports[0].message.contains("illegal `push` on `@antitone`"),
            "got: {}",
            set.reports[0].message
        );
    }

    #[test]
    fn op_scan_allows_shrink_of_antitone_collection() {
        let set = lint_src(
            "module M { @antitone let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.remove(0); } }",
        );
        assert!(
            set.is_empty(),
            "remove on @antitone is legal; got: {:?}",
            set.reports
        );
    }

    #[test]
    fn op_scan_covers_every_documented_operation() {
        // Every shrink op on @monotone must warn.
        for op in SHRINK_OPS {
            let set = lint_src(&format!(
                "module M {{ @monotone let xs: Vec<i32> = Vec::new(); \
                 fn f() {{ xs.{op}(0); }} }}"
            ));
            assert_eq!(
                set.len(),
                1,
                "shrink op `{op}` must warn; got: {:?}",
                set.reports
            );
        }
        // Every grow op on @antitone must warn.
        for op in GROW_OPS {
            let set = lint_src(&format!(
                "module M {{ @antitone let xs: Vec<i32> = Vec::new(); \
                 fn f() {{ xs.{op}(0); }} }}"
            ));
            assert_eq!(
                set.len(),
                1,
                "grow op `{op}` must warn; got: {:?}",
                set.reports
            );
        }
    }

    #[test]
    fn op_scan_ignores_unattributed_collections() {
        // P1 contract: attribute-based only (the P2 type checker is the
        // authority for qualifier-based declarations).
        let set = lint_src(
            "module M { let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.remove(0); } }",
        );
        assert!(set.is_empty(), "got: {:?}", set.reports);
    }

    #[test]
    fn op_scan_finds_calls_nested_in_expressions_and_control_flow() {
        let set = lint_src(
            "module M { @monotone let xs: Vec<i32> = Vec::new(); \
             fn f() { if (xs.len() > 0) { helper(xs.remove(0)); } while (xs.len() > 1) { xs.clear(); } } \
             fn helper(v: i32) { } }",
        );
        // remove (nested in call arg, inside if) + clear (inside while)
        assert_eq!(set.len(), 2, "got: {:?}", set.reports);
        let mut msgs: Vec<&str> = set.reports.iter().map(|r| r.message.as_str()).collect();
        msgs.sort();
        assert!(msgs[0].contains("illegal `clear`"), "got: {}", msgs[0]);
        assert!(msgs[1].contains("illegal `remove`"), "got: {}", msgs[1]);
    }

    #[test]
    fn op_scan_finds_local_attributed_lets() {
        let set = lint_src(
            "module M { fn f() { @monotone let ys: Vec<i32> = Vec::new(); ys.truncate(1); } }",
        );
        assert_eq!(set.len(), 1, "got: {:?}", set.reports);
        assert!(
            set.reports[0].message.contains("illegal `truncate` on `@monotone`"),
            "got: {}",
            set.reports[0].message
        );
    }

    #[test]
    fn op_scan_reports_call_site_position() {
        let set = lint_src(
            "module M { @monotone let xs: Vec<i32> = Vec::new(); \
             fn f() { xs.remove(0); } }",
        );
        assert!(set.reports[0].line >= 1);
        assert!(set.reports[0].col >= 1);
    }

    #[test]
    fn op_scan_warns_when_declaration_is_in_another_function_scope() {
        // Top-level attributed lets are module-scoped: any function body
        // may reference them.
        let set = lint_src(
            "module M { @monotone let xs: Vec<i32> = Vec::new(); \
             fn a() { xs.push(1); } fn b() { xs.swap_remove(0); } }",
        );
        assert_eq!(set.len(), 1, "got: {:?}", set.reports);
        assert!(
            set.reports[0].message.contains("swap_remove"),
            "got: {}",
            set.reports[0].message
        );
    }

    #[test]
    fn op_scan_respects_deny_monotonicity() {
        let src = "#![deny(monotonicity)]\nmodule M { @monotone let xs: Vec<i32> = Vec::new(); fn f() { xs.remove(0); } }";
        let ast = crate::parser::parse(src).expect("parse ok");
        let mut set = LintSet::new();
        set.deny_monotonicity = true; // simulate the pre-scan in run_lints
        run(&ast, &mut set);
        assert!(set.has_errors());
        assert_eq!(set.reports[0].severity, LintSeverity::Deny);
    }
}

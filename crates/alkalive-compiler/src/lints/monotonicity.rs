//! Monotonicity lint (ADR-027 Phase 1).
//!
//! This lint scans the AST for `@monotone` / `@antitone` attribute
//! annotations and emits findings for misuse. In the current Hello-World
//! subset of `.alk`, there are no collection-typed declarations, no
//! function bodies, and no method calls — so the lint can only check that
//! the attributes are placed sensibly. The full intra-function
//! enforcement (rejecting `.remove()` on `@monotone` collections, etc.)
//! is deferred to a future phase when the grammar grows.
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
//! 3. **Forward-compat placeholder.** `@monotone` / `@antitone` on a
//!    non-collection declaration currently emits a *warning* (not an
//!    error) because the Hello-World `.alk` grammar has no collection
//!    types yet. The infrastructure is in place so that, once collections
//!    are added, this lint can become a hard check.
//!
//! # Future expansion (Phase 2 and beyond)
//!
//! When the `.alk` grammar grows function bodies and method calls, this
//! lint will additionally:
//!
//! - Walk each function scope and find `@monotone` / `@antitone`
//!   collections.
//! - Reject shrinking operations (`.remove()`, `.truncate()`, `.clear()`,
//!   `.swap_remove()`, `.drain()`) on `@monotone` collections.
//! - Reject growing operations (`.push()`, `.extend()`, `.insert()`,
//!   `.append()`) on `@antitone` collections.
//!
//! That work is out of scope for Phase 1.

#![forbid(unsafe_code)]

use super::{LintReport, LintSet, LintSeverity};
use crate::ast::{ModuleDecl, NodeDecl};

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
/// This function walks the module's scene (if any), inspects every node
/// declaration's attributes, and emits findings as described in the
/// [module-level docs](self).
pub fn run(ast: &ModuleDecl, set: &mut LintSet) {
    let Some(scene) = ast.scene.as_ref() else {
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

        // Forward-compatible check: in the current `.alk` grammar, there
        // are no collection types, so `@monotone` / `@antitone` on a node
        // declaration is a no-op at best. Emit a warning so users know
        // the attribute is not yet enforced on this kind of declaration.
        set.add(LintReport::new(
            LintSeverity::Warning,
            format!(
                "{}: `@{}` applied to {} at {}:{}; \
                 the current `.alk` grammar has no collection types, so this \
                 attribute is not yet enforced (it will become active when \
                 collections are introduced in a future phase)",
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
}

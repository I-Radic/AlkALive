//! Integration tests for the ADR-027 Phase 1 monotonicity lint system.
//!
//! These tests exercise the public API surface as an external consumer
//! would: parse `.alk` source with attributes, run lints, and verify the
//! [`LintSet`] findings.
//!
//! Scope:
//! - `@monotone` attribute parsed correctly on a text node.
//! - `@antitone` attribute parsed correctly on an input field.
//! - `#![deny(monotonicity)]` file-level attribute parsed and sets the
//!   deny flag.
//! - Lint reports are generated for attributes.
//! - Existing `.alk` files without attributes compile unchanged.
//! - `compile_with_lints()` returns both `SceneIR` and `LintSet`.

#![forbid(unsafe_code)]

use alkalive_compiler::{
    compile, compile_with_lints, parse, Attribute, LintSeverity, NodeDecl, TokenKind, tokenize,
};

/// Helper: the canonical Hello-World source (no attributes).
const HELLO_WORLD: &str = r#"
module HelloWorld {
  scene {
    background: #000000
    text "Hello World!" {
      color: gold
      font-size: 64
      rotation: y-axis 0.5
      position: center
    }
    input-field {
      placeholder: "Type here..."
      position: below text
    }
  }
}
"#;

// ---------------------------------------------------------------------------
// 1. `@monotone` on a text node
// ---------------------------------------------------------------------------

#[test]
fn monotone_attribute_parsed_on_text_node() {
    let src = r#"module M { scene { @monotone text "Hi" { } } }"#;
    let ast = parse(src).expect("parse ok");
    let scene = ast.scene.expect("scene");
    assert_eq!(scene.nodes.len(), 1);
    match &scene.nodes[0] {
        NodeDecl::Text(t) => {
            assert_eq!(t.content, "Hi");
            assert_eq!(t.attributes.len(), 1, "expected exactly one attribute");
            assert_eq!(t.attributes[0].name, "monotone");
            // Position of the `@` token.
            assert_eq!(t.attributes[0].line, 1);
            assert_eq!(t.attributes[0].col, 20);
        }
        other => panic!("expected Text node, got {:?}", other),
    }
}

#[test]
fn monotone_lexes_as_at_then_ident() {
    // The `@` character must be its own token; `monotone` is NOT a keyword.
    let src = "@monotone";
    let toks = tokenize(src).expect("lex ok");
    let kinds: Vec<_> = toks
        .iter()
        .filter(|t| t.kind != TokenKind::Newline)
        .map(|t| t.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![TokenKind::At, TokenKind::Ident, TokenKind::Eof]
    );
    let ident = toks
        .iter()
        .find(|t| t.kind == TokenKind::Ident)
        .expect("ident token");
    assert_eq!(ident.value, "monotone");
}

// ---------------------------------------------------------------------------
// 2. `@antitone` on an input field
// ---------------------------------------------------------------------------

#[test]
fn antitone_attribute_parsed_on_input_field() {
    let src = r#"module M { scene { text "T" { } @antitone input-field { } } }"#;
    let ast = parse(src).expect("parse ok");
    let scene = ast.scene.expect("scene");
    assert_eq!(scene.nodes.len(), 2);
    match &scene.nodes[1] {
        NodeDecl::InputField(f) => {
            assert_eq!(f.attributes.len(), 1);
            assert_eq!(f.attributes[0].name, "antitone");
        }
        other => panic!("expected InputField, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 3. `#![deny(monotonicity)]` file-level attribute
// ---------------------------------------------------------------------------

#[test]
fn deny_monotonicity_attribute_parsed_on_module() {
    let src = "#![deny(monotonicity)]\nmodule M { scene { } }";
    let ast = parse(src).expect("parse ok");
    assert_eq!(ast.attributes.len(), 1);
    assert_eq!(ast.attributes[0].name, "deny(monotonicity)");
}

#[test]
fn deny_monotonicity_attribute_sets_deny_flag_in_lint_set() {
    let src = "#![deny(monotonicity)]\nmodule M { scene { @monotone text \"X\" { } } }";
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(
        lint_set.deny_monotonicity,
        "deny_monotonicity flag must be set; got: {:?}",
        lint_set
    );
}

#[test]
fn deny_monotonicity_upgrades_warnings_to_errors() {
    // With `#![deny(monotonicity)]`, the @monotone warning must be
    // upgraded to a Deny finding.
    let src = "#![deny(monotonicity)]\nmodule M { scene { @monotone text \"X\" { } } }";
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(lint_set.has_errors(), "expected at least one error");
    assert!(
        lint_set
            .iter()
            .any(|r| r.severity == LintSeverity::Deny),
        "expected a Deny finding, got: {:?}",
        lint_set
    );
}

// ---------------------------------------------------------------------------
// 4. Lint reports generated for attributes
// ---------------------------------------------------------------------------

#[test]
fn lint_reports_generated_for_monotone_attribute() {
    let src = r#"module M { scene { @monotone text "Hi" { } } }"#;
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(!lint_set.is_empty(), "expected at least one lint report");
    let report = lint_set
        .iter()
        .find(|r| r.message.contains("@monotone"))
        .expect("should have a report mentioning @monotone");
    assert!(report.message.contains("monotonicity:"), "got: {}", report.message);
    assert_eq!(report.severity, LintSeverity::Warning);
}

#[test]
fn lint_reports_generated_for_antitone_attribute() {
    let src = r#"module M { scene { text "T" { } @antitone input-field { } } }"#;
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(!lint_set.is_empty());
    assert!(
        lint_set.iter().any(|r| r.message.contains("@antitone")),
        "missing @antitone report: {:?}",
        lint_set
    );
}

#[test]
fn lint_reports_generated_for_unknown_attribute_name() {
    // `@monotonic` (with a trailing 'c') is not a recognised attribute.
    let src = r#"module M { scene { @monotonic text "Hi" { } } }"#;
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(
        lint_set
            .iter()
            .any(|r| r.message.contains("unknown") && r.message.contains("monotonic")),
        "expected unknown-attribute report: {:?}",
        lint_set
    );
}

#[test]
fn lint_reports_include_source_position() {
    let src = r#"
module M {
  scene {
    @monotone text "Hi" { }
  }
}
"#;
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    let report = lint_set
        .iter()
        .find(|r| r.message.contains("@monotone"))
        .expect("should have a @monotone report");
    assert!(report.line >= 4, "line should point at the @ on line 4; got: {}", report.line);
    assert!(report.col >= 5, "col should point at the @ in col 5; got: {}", report.col);
}

// ---------------------------------------------------------------------------
// 5. Existing `.alk` files without attributes compile unchanged
// ---------------------------------------------------------------------------

#[test]
fn existing_alk_without_attributes_compiles_unchanged() {
    // The canonical Hello-World source must compile via the legacy
    // `compile()` (no lints) and produce the same IR.
    let ir = compile(HELLO_WORLD).expect("hello world should compile");
    assert_eq!(ir.module_name, "HelloWorld");
    assert_eq!(ir.background, (0, 0, 0));
    assert_eq!(ir.nodes.len(), 2);
    assert!(ir.has_text());
    assert!(ir.has_input_field());
}

#[test]
fn existing_alk_without_attributes_has_no_lint_findings() {
    // When run through `compile_with_lints()`, the canonical Hello-World
    // source must produce an empty `LintSet`.
    let (ir, lint_set) = compile_with_lints(HELLO_WORLD).expect("compile ok");
    assert_eq!(ir.module_name, "HelloWorld");
    assert!(lint_set.is_empty(), "expected no lint findings; got: {:?}", lint_set);
    assert!(!lint_set.deny_monotonicity);
}

#[test]
fn ast_attributes_are_empty_for_canonical_hello_world() {
    let ast = parse(HELLO_WORLD).expect("parse ok");
    assert!(ast.attributes.is_empty());
    let scene = ast.scene.expect("scene");
    assert!(scene.attributes.is_empty());
    for n in &scene.nodes {
        assert!(n.attributes().is_empty(), "node {:?} has unexpected attrs", n);
    }
}

// ---------------------------------------------------------------------------
// 6. `compile_with_lints()` returns both SceneIR and LintSet
// ---------------------------------------------------------------------------

#[test]
fn compile_with_lints_returns_ir_and_lint_set() {
    let src = r#"module M { scene { @monotone text "Hi" { } } }"#;
    let (ir, lint_set) = compile_with_lints(src).expect("compile ok");
    // IR is still produced.
    assert_eq!(ir.module_name, "M");
    assert!(ir.has_text());
    // LintSet is non-empty.
    assert!(!lint_set.is_empty());
}

#[test]
fn compile_with_lints_preserves_ir_equivalence_with_compile() {
    // The IR returned by `compile_with_lints` must equal the IR returned
    // by `compile` for the same source. (Lints must not mutate the IR.)
    let src = HELLO_WORLD;
    let ir_no_lint = compile(src).expect("compile ok");
    let (ir_with_lint, _set) = compile_with_lints(src).expect("compile_with_lints ok");
    assert_eq!(ir_no_lint, ir_with_lint);
}

#[test]
fn compile_with_lints_does_not_abort_on_warning() {
    // Even when there are lint warnings, `compile_with_lints` must still
    // return the IR (it never escalates warnings to CompileError).
    let src = r#"module M { scene { @monotone text "Hi" { } } }"#;
    let (ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert!(!lint_set.is_empty());
    assert!(!lint_set.has_errors());
    assert!(ir.has_text());
}

// ---------------------------------------------------------------------------
// 7. Multiple attributes and combined scenarios
// ---------------------------------------------------------------------------

#[test]
fn multiple_attributes_produce_multiple_reports() {
    let src = r#"module M {
  scene {
    @monotone text "first" { }
    @antitone input-field { }
  }
}"#;
    let (_ir, lint_set) = compile_with_lints(src).expect("compile ok");
    assert_eq!(
        lint_set.len(),
        2,
        "expected 2 reports (one per attribute); got: {:?}",
        lint_set
    );
}

#[test]
fn attribute_re_export_works() {
    // The `Attribute` struct is re-exported at the crate root.
    let a = Attribute::new("monotone", 1, 2);
    assert_eq!(a.name, "monotone");
    assert_eq!(a.line, 1);
    assert_eq!(a.col, 2);
}

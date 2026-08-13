//! ADR-027 Phase 2 — comprehensive integration tests.
//!
//! These tests verify the full Phase 2 pipeline end-to-end:
//! - `compile_typecheck()` integration (parse → typecheck → lower)
//! - `lower_collection_decl()` unit tests (AST → IR monotonicity lowering)
//! - Runtime seminaïve evaluation hook (`seminative` module)
//! - IR `to_json()` serialization of the `collections` array
//! - Parser tests for `fn`, `let`, and type-qualifier syntax
//! - Negative / error-case tests
//! - Migration tests (`@monotone` attribute → type qualifier)
//! - Regression tests for Phase 1 (lint still works)
//! - Backward compatibility (existing `compile()` unchanged)

#![forbid(unsafe_code)]

use alkalive_compiler::{
    check_module, collection_strategies, collection_strategy, compile, compile_typecheck,
    compile_with_lints, has_seminive_collections, parse, seminive_eligible_count, CollectionDeclIR,
    EvaluationStrategy, Monotonicity, Qualifier,
};

const SCENE: &str = "scene { background: #000000 }";

// ======================================================================
// 1. compile_typecheck() integration tests
// ======================================================================

#[test]
fn compile_typecheck_valid_module() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() {{
    let v: monotone Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
  }}
}}
"#,
        SCENE
    );
    let ir = compile_typecheck(&src).expect("should compile");
    assert_eq!(ir.module_name, "M");
    assert_eq!(ir.collections.len(), 0); // the let is inside fn body, not top-level
}

#[test]
fn compile_typecheck_rejects_monotone_shrink() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() {{
    let v: monotone Vec<i32> = Vec::new();
    v.remove(0);
  }}
}}
"#,
        SCENE
    );
    let err = compile_typecheck(&src).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("type error"), "got: {}", msg);
    assert!(msg.contains("remove"), "got: {}", msg);
    assert!(msg.contains("monotone"), "got: {}", msg);
}

#[test]
fn compile_typecheck_rejects_antitone_grow() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() {{
    let v: antitone Vec<i32> = Vec::new();
    v.push(1);
  }}
}}
"#,
        SCENE
    );
    let err = compile_typecheck(&src).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("push"), "got: {}", msg);
    assert!(msg.contains("antitone"), "got: {}", msg);
}

#[test]
fn compile_typecheck_collects_multiple_errors() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() {{
    let v: monotone Vec<i32> = Vec::new();
    v.remove(0);
    v.clear();
    v.truncate(1);
  }}
}}
"#,
        SCENE
    );
    let err = compile_typecheck(&src).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("3 type error"), "got: {}", msg);
}

#[test]
fn compile_typecheck_accepts_unrestricted() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() {{
    let v: Vec<i32> = Vec::new();
    v.push(1);
    v.remove(0);
    v.clear();
  }}
}}
"#,
        SCENE
    );
    compile_typecheck(&src).expect("unrestricted should accept all ops");
}

#[test]
fn compile_typecheck_param_flow() {
    let src = format!(
        r#"
module M {{
  {}
  fn f(x: monotone Vec<i32>) {{
    x.push(1);
  }}
  fn g(x: monotone Vec<i32>) {{
    x.remove(0);
  }}
}}
"#,
        SCENE
    );
    let err = compile_typecheck(&src).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("remove"), "got: {}", msg);
}

#[test]
fn compile_typecheck_return_type() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() -> i32 {{
    return 42;
  }}
}}
"#,
        SCENE
    );
    compile_typecheck(&src).expect("return i32 should be ok");
}

#[test]
fn compile_typecheck_return_type_mismatch() {
    let src = format!(
        r#"
module M {{
  {}
  fn f() -> i32 {{
    return "hello";
  }}
}}
"#,
        SCENE
    );
    let err = compile_typecheck(&src).unwrap_err();
    assert!(format!("{}", err).contains("return type mismatch"));
}

// ======================================================================
// 2. lower_collection_decl() — AST → IR monotonicity lowering
// ======================================================================

#[test]
fn lower_top_level_monotone_collection() {
    let src = format!(
        "module M {{ {} let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections.len(), 1);
    assert_eq!(ir.collections[0].name, "v");
    assert_eq!(ir.collections[0].element_type, "i32");
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Monotone);
}

#[test]
fn lower_top_level_antitone_collection() {
    let src = format!(
        "module M {{ {} let v: antitone Vec<string> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections.len(), 1);
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Antitone);
    assert_eq!(ir.collections[0].element_type, "string");
}

#[test]
fn lower_top_level_unrestricted_collection() {
    let src = format!("module M {{ {} let v: Vec<bool> = Vec::new(); }}", SCENE);
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections.len(), 1);
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Unrestricted);
    assert_eq!(ir.collections[0].element_type, "bool");
}

#[test]
fn lower_attribute_form_monotone() {
    // Phase 1 @monotone attribute form should lower to Monotonicity::Monotone.
    let src = format!(
        "module M {{ {} @monotone let v: Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections.len(), 1);
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Monotone);
}

#[test]
fn lower_attribute_form_antitone() {
    let src = format!(
        "module M {{ {} @antitone let v: Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Antitone);
}

#[test]
fn lower_multiple_collections() {
    let src = format!(
        "module M {{ {} let a: monotone Vec<i32> = Vec::new(); let b: antitone Vec<string> = Vec::new(); let c: Vec<bool> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections.len(), 3);
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Monotone);
    assert_eq!(ir.collections[1].monotonicity, Monotonicity::Antitone);
    assert_eq!(ir.collections[2].monotonicity, Monotonicity::Unrestricted);
}

#[test]
fn lower_no_collections() {
    let src = "module M { scene { background: #000000 } }";
    let ir = compile(src).expect("compile");
    assert!(ir.collections.is_empty());
}

// ======================================================================
// 3. Runtime seminaïve evaluation hook
// ======================================================================

#[test]
fn seminive_strategy_monotone() {
    let col = CollectionDeclIR {
        name: "v".into(),
        element_type: "i32".into(),
        monotonicity: Monotonicity::Monotone,
    };
    assert_eq!(collection_strategy(&col), EvaluationStrategy::SeminiveNew);
}

#[test]
fn seminive_strategy_antitone() {
    let col = CollectionDeclIR {
        name: "v".into(),
        element_type: "i32".into(),
        monotonicity: Monotonicity::Antitone,
    };
    assert_eq!(
        collection_strategy(&col),
        EvaluationStrategy::SeminiveRemoved
    );
}

#[test]
fn seminive_strategy_unrestricted() {
    let col = CollectionDeclIR {
        name: "v".into(),
        element_type: "i32".into(),
        monotonicity: Monotonicity::Unrestricted,
    };
    assert_eq!(collection_strategy(&col), EvaluationStrategy::Full);
}

#[test]
fn seminive_strategies_from_ir() {
    let src = format!(
        "module M {{ {} let a: monotone Vec<i32> = Vec::new(); let b: antitone Vec<i32> = Vec::new(); let c: Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    let strategies = collection_strategies(&ir);
    assert_eq!(strategies.len(), 3);
    assert_eq!(strategies[0], ("a".into(), EvaluationStrategy::SeminiveNew));
    assert_eq!(
        strategies[1],
        ("b".into(), EvaluationStrategy::SeminiveRemoved)
    );
    assert_eq!(strategies[2], ("c".into(), EvaluationStrategy::Full));
}

#[test]
fn has_seminive_true_when_monotone_present() {
    let src = format!(
        "module M {{ {} let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert!(has_seminive_collections(&ir));
    assert_eq!(seminive_eligible_count(&ir), 1);
}

#[test]
fn has_seminive_false_when_all_unrestricted() {
    let src = format!("module M {{ {} let v: Vec<i32> = Vec::new(); }}", SCENE);
    let ir = compile(&src).expect("compile");
    assert!(!has_seminive_collections(&ir));
}

#[test]
fn has_seminive_false_when_no_collections() {
    let src = "module M { scene { background: #000000 } }";
    let ir = compile(src).expect("compile");
    assert!(!has_seminive_collections(&ir));
}

// ======================================================================
// 4. to_json() collections serialization
// ======================================================================

#[test]
fn to_json_serializes_collections() {
    let src = format!(
        "module M {{ {} let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    let json = ir.to_json();
    assert!(json.contains("\"collections\":"), "got: {}", json);
    assert!(json.contains("\"name\":\"v\""), "got: {}", json);
    assert!(json.contains("\"element_type\":\"i32\""), "got: {}", json);
    assert!(
        json.contains("\"monotonicity\":\"monotone\""),
        "got: {}",
        json
    );
}

#[test]
fn to_json_serializes_antitone_collection() {
    let src = format!(
        "module M {{ {} let v: antitone Vec<string> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    let json = ir.to_json();
    assert!(
        json.contains("\"monotonicity\":\"antitone\""),
        "got: {}",
        json
    );
    assert!(
        json.contains("\"element_type\":\"string\""),
        "got: {}",
        json
    );
}

#[test]
fn to_json_empty_collections_array() {
    let src = "module M { scene { background: #000000 } }";
    let ir = compile(src).expect("compile");
    let json = ir.to_json();
    assert!(json.contains("\"collections\":[]"), "got: {}", json);
}

#[test]
fn to_json_multiple_collections() {
    let src = format!(
        "module M {{ {} let a: monotone Vec<i32> = Vec::new(); let b: antitone Vec<string> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    let json = ir.to_json();
    // Count collection objects
    let count = json.matches("\"monotonicity\":").count();
    assert_eq!(count, 2, "expected 2 collections in: {}", json);
    assert!(json.contains("\"monotonicity\":\"monotone\""));
    assert!(json.contains("\"monotonicity\":\"antitone\""));
}

// ======================================================================
// 5. Parser tests for fn/let/type syntax
// ======================================================================

#[test]
fn parse_fn_declaration() {
    let src = format!("module M {{ {} fn f() {{ }} }}", SCENE);
    let m = parse(&src).expect("parse");
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Fn(f) => {
            assert_eq!(f.name, "f");
            assert!(f.params.is_empty());
            assert!(f.return_type.is_none());
            assert!(f.body.stmts.is_empty());
        }
        other => panic!("expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_fn_with_params_and_return() {
    let src = format!(
        "module M {{ {} fn add(a: i32, b: i32) -> i32 {{ return a; }} }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Fn(f) => {
            assert_eq!(f.name, "add");
            assert_eq!(f.params.len(), 2);
            assert_eq!(f.params[0].name, "a");
            assert!(f.return_type.is_some());
        }
        other => panic!("expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_fn_with_monotone_param() {
    let src = format!("module M {{ {} fn f(x: monotone Vec<i32>) {{ }} }}", SCENE);
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Fn(f) => {
            assert_eq!(f.params[0].ty.qualifier, Qualifier::Monotone);
        }
        other => panic!("expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_let_declaration() {
    let src = format!("module M {{ {} let v: Vec<i32> = Vec::new(); }}", SCENE);
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Let(l) => {
            assert_eq!(l.name, "v");
            assert_eq!(l.ty.qualifier, Qualifier::Unrestricted);
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn parse_let_with_monotone_qualifier() {
    let src = format!(
        "module M {{ {} let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Let(l) => {
            assert_eq!(l.ty.qualifier, Qualifier::Monotone);
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn parse_let_with_antitone_qualifier() {
    let src = format!(
        "module M {{ {} let v: antitone Vec<string> = Vec::new(); }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Let(l) => {
            assert_eq!(l.ty.qualifier, Qualifier::Antitone);
            assert!(l.ty.is_vec());
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn parse_type_nested_vec() {
    let src = format!(
        "module M {{ {} let v: Vec<Vec<i32>> = Vec::new(); }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Let(l) => {
            assert!(l.ty.is_vec());
            // The element type should also be Vec<i32>
            match &l.ty.base {
                alkalive_compiler::BaseType::Vec(elem) => {
                    assert!(elem.is_vec());
                }
                other => panic!("expected Vec elem, got {:?}", other),
            }
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn parse_method_call_chain() {
    let src = format!(
        "module M {{ {} fn f() {{ let v: Vec<i32> = Vec::new(); v.push(1); v.push(2); v.len(); }} }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Fn(f) => {
            assert_eq!(f.body.stmts.len(), 4); // let + 3 expr stmts
        }
        other => panic!("expected Fn, got {:?}", other),
    }
}

#[test]
fn parse_attribute_on_let() {
    let src = format!(
        "module M {{ {} @monotone let v: Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    match &m.items[0] {
        alkalive_compiler::ItemDecl::Let(l) => {
            assert_eq!(l.attrs.len(), 1);
            assert_eq!(l.attrs[0].name, "monotone");
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn parse_mixed_scene_and_items() {
    let src = format!(
        "module M {{ {} let g: monotone Vec<i32> = Vec::new(); fn f() {{ g.push(1); }} }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    assert_eq!(m.items.len(), 2);
}

// ======================================================================
// 6. Negative / error-case tests
// ======================================================================

#[test]
fn typecheck_undefined_variable() {
    let src = format!(
        "module M {{ {} fn f() {{ undefined_var.push(1); }} }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    let errors = check_module(&m);
    assert_eq!(errors.len(), 1);
    assert!(errors.errors[0].message.contains("undefined variable"));
}

#[test]
fn typecheck_monotone_all_shrink_ops_error() {
    for op in &["remove", "truncate", "clear", "swap_remove", "drain"] {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.{}(); }} }}",
            SCENE, op
        );
        let m = parse(&src).expect("parse");
        let errors = check_module(&m);
        assert_eq!(errors.len(), 1, "op {} should error, got: {}", op, errors);
        assert!(errors.errors[0].message.contains(op));
    }
}

#[test]
fn typecheck_antitone_all_grow_ops_error() {
    for op in &["push", "extend", "insert", "append"] {
        let src = format!(
            "module M {{ {} fn f() {{ let v: antitone Vec<i32> = Vec::new(); v.{}(1); }} }}",
            SCENE, op
        );
        let m = parse(&src).expect("parse");
        let errors = check_module(&m);
        assert_eq!(errors.len(), 1, "op {} should error, got: {}", op, errors);
        assert!(errors.errors[0].message.contains(op));
    }
}

#[test]
fn typecheck_neutral_ops_never_error() {
    for op in &["len", "get", "is_empty", "contains", "first", "last"] {
        let src = format!(
            "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); let w: antitone Vec<i32> = Vec::new(); v.{}(); w.{}(); }} }}",
            SCENE, op, op
        );
        let m = parse(&src).expect("parse");
        let errors = check_module(&m);
        assert!(
            errors.is_empty(),
            "op {} should not error, got: {}",
            op,
            errors
        );
    }
}

#[test]
fn typecheck_module_level_collection_visible_in_fn() {
    let src = format!(
        "module M {{ {} let g: monotone Vec<i32> = Vec::new(); fn f() {{ g.clear(); }} }}",
        SCENE
    );
    let m = parse(&src).expect("parse");
    let errors = check_module(&m);
    assert_eq!(errors.len(), 1);
    assert!(errors.errors[0].message.contains("clear"));
}

// ======================================================================
// 7. Migration tests (@attribute → type qualifier)
// ======================================================================

#[test]
fn migration_attribute_and_qualifier_equivalent() {
    // @monotone let v: Vec<i32>  and  let v: monotone Vec<i32>
    // should both lower to Monotonicity::Monotone.
    let attr_src = format!(
        "module M {{ {} @monotone let v: Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let qual_src = format!(
        "module M {{ {} let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let attr_ir = compile(&attr_src).expect("compile");
    let qual_ir = compile(&qual_src).expect("compile");
    assert_eq!(
        attr_ir.collections[0].monotonicity,
        qual_ir.collections[0].monotonicity
    );
    assert_eq!(attr_ir.collections[0].monotonicity, Monotonicity::Monotone);
}

#[test]
fn migration_attribute_takes_precedence() {
    // @antitone let v: monotone Vec<i32>  →  effective = antitone (attribute wins)
    let src = format!(
        "module M {{ {} @antitone let v: monotone Vec<i32> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    assert_eq!(ir.collections[0].monotonicity, Monotonicity::Antitone);
}

#[test]
fn migration_typecheck_uses_effective_qualifier() {
    // The typechecker should check @monotone let v: Vec<i32> the same as
    // let v: monotone Vec<i32>.
    let attr_src = format!(
        "module M {{ {} fn f() {{ @monotone let v: Vec<i32> = Vec::new(); v.remove(0); }} }}",
        SCENE
    );
    let qual_src = format!(
        "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.remove(0); }} }}",
        SCENE
    );
    let attr_errors = check_module(&parse(&attr_src).expect("parse"));
    let qual_errors = check_module(&parse(&qual_src).expect("parse"));
    assert_eq!(attr_errors.len(), qual_errors.len());
    assert_eq!(attr_errors.len(), 1);
}

// ======================================================================
// 8. Regression: Phase 1 lint still works
// ======================================================================

#[test]
fn phase1_lint_still_runs() {
    // compile_with_lints should still work and return lint findings.
    // The @monotone attribute is on a text node INSIDE the scene block.
    let src = r#"
module M {
  scene {
    background: #000000
    @monotone text "Hi" { }
  }
}
"#;
    let (ir, lints) = compile_with_lints(src).expect("compile with lints");
    assert_eq!(ir.module_name, "M");
    // The Phase 1 lint emits a warning for @monotone on a text node.
    assert!(
        !lints.is_empty(),
        "Phase 1 lint should still produce findings"
    );
}

#[test]
fn phase1_deny_monotonicity_still_works() {
    let src = r#"
#![deny(monotonicity)]
module M {
  scene {
    background: #000000
    @monotone text "Hi" { }
  }
}
"#;
    let (ir, lints) = compile_with_lints(src).expect("compile with lints");
    assert_eq!(ir.module_name, "M");
    assert!(
        lints.has_errors(),
        "deny(monotonicity) should upgrade to errors"
    );
}

// ======================================================================
// 9. Backward compatibility: compile() unchanged
// ======================================================================

#[test]
fn compile_still_works_without_typecheck() {
    // The legacy compile() function does NOT run the type checker.
    // A module with a type error should still compile successfully via compile()
    // (backward compat — the type error is only caught by compile_typecheck()).
    let src = format!(
        "module M {{ {} fn f() {{ let v: monotone Vec<i32> = Vec::new(); v.remove(0); }} }}",
        SCENE
    );
    let ir = compile(&src).expect("legacy compile should succeed (no typecheck)");
    assert_eq!(ir.module_name, "M");
}

#[test]
fn hello_world_still_compiles() {
    let src = r#"
module HelloWorld {
  scene {
    background: #000000
    text "Hello World!" {
      color: gold
      font-size: 64
      rotation: y-axis 0.5
      position: center
    }
  }
}
"#;
    let ir = compile(src).expect("hello world should compile");
    assert_eq!(ir.module_name, "HelloWorld");
    assert!(ir.has_text());
    assert!(ir.collections.is_empty());
}

// ======================================================================
// 10. IR JSON roundtrip structural integrity
// ======================================================================

#[test]
fn ir_json_collections_well_formed() {
    let src = format!(
        "module M {{ {} let a: monotone Vec<i32> = Vec::new(); let b: antitone Vec<string> = Vec::new(); }}",
        SCENE
    );
    let ir = compile(&src).expect("compile");
    let json = ir.to_json();
    // Verify balanced braces and brackets.
    let braces_open = json.chars().filter(|&c| c == '{').count();
    let braces_close = json.chars().filter(|&c| c == '}').count();
    assert_eq!(braces_open, braces_close, "unbalanced braces: {}", json);
    let brackets_open = json.chars().filter(|&c| c == '[').count();
    let brackets_close = json.chars().filter(|&c| c == ']').count();
    assert_eq!(
        brackets_open, brackets_close,
        "unbalanced brackets: {}",
        json
    );
}

#[test]
fn monotonicity_display() {
    assert_eq!(format!("{}", Monotonicity::Unrestricted), "unrestricted");
    assert_eq!(format!("{}", Monotonicity::Monotone), "monotone");
    assert_eq!(format!("{}", Monotonicity::Antitone), "antitone");
}

#[test]
fn monotonicity_from_qualifier() {
    assert_eq!(
        Monotonicity::from_qualifier(Qualifier::Unrestricted),
        Monotonicity::Unrestricted
    );
    assert_eq!(
        Monotonicity::from_qualifier(Qualifier::Monotone),
        Monotonicity::Monotone
    );
    assert_eq!(
        Monotonicity::from_qualifier(Qualifier::Antitone),
        Monotonicity::Antitone
    );
}

#[test]
fn monotonicity_supports_seminive() {
    assert!(!Monotonicity::Unrestricted.supports_seminive());
    assert!(Monotonicity::Monotone.supports_seminive());
    assert!(!Monotonicity::Antitone.supports_seminive());
}

//! AlkALive compiler frontend.
//!
//! Lexes, parses, and lowers `.alk` source files (Hello-World subset)
//! into a runtime-consumable [`ir::AlgorithmIR`] (the *algorithm* IR), then
//! applies the ADR-024 [`schedule::schedule_lowering`] pass to produce a
//! [`schedule::ScheduledScene`] (algorithm + schedule).
//!
//! # Pipeline
//!
//! ```text
//! .alk source ──► [lexer] ──► Vec<Token>
//!                  │
//!                  ▼
//!                [parser] ──► ast::ModuleDecl
//!                  │
//!                  ▼
//!                [lints]   ──► LintSet   (ADR-027 Phase 1)
//!                  │
//!                  ▼
//!                [codegen] ──► ir::AlgorithmIR
//!                  │
//!                  ▼
//!                [schedule_lowering] ──► schedule::ScheduledScene  (ADR-024)
//! ```
//!
//! # Example
//!
//! ```
//! use alkalive_compiler::compile;
//!
//! let src = r#"
//! module HelloWorld {
//!   scene {
//!     background: #000000
//!     text "Hello World!" {
//!       color: gold
//!       font-size: 64
//!       rotation: y-axis 0.5
//!       position: center
//!     }
//!   }
//! }
//! "#;
//! let ir = compile(src).expect("hello world should compile");
//! assert_eq!(ir.module_name, "HelloWorld");
//! assert!(ir.has_text());
//! ```
//!
//! # Zero-dependency library surface
//!
//! The library modules (`lexer`, `ast`, `parser`, `ir`, `codegen`,
//! `lints`, `schedule`) use only `alkalive-core` (an internal workspace
//! crate) and `std`/`core`. The optional `cli` feature pulls in `serde_json`
//! for the binary; it does NOT affect the library's public API.
//!
//! # Lints (ADR-027 Phase 1)
//!
//! Use [`compile_with_lints`] to obtain both the lowered [`ir::AlgorithmIR`]
//! and the [`lints::LintSet`] produced by the lint passes. The legacy
//! [`compile`] function remains lint-free for backward compatibility.
//!
//! # ADR-024 — Algorithm/Schedule Separation
//!
//! The legacy [`SceneIR`](ir::SceneIR) type is now an alias for
//! [`AlgorithmIR`](ir::AlgorithmIR). Use [`compile_scheduled`] to obtain a
//! [`schedule::ScheduledScene`] containing both the algorithm and the
//! default schedule.
//!
//! # ADR-025 — Incremental Computation
//!
//! Use [`compile_with_deps`] to additionally obtain a
//! [`incremental::DependencyGraph`] for the scheduled scene. The runtime
//! uses this graph to propagate dirtiness from changed signals to the
//! passes that depend on them, reducing per-frame work from O(n) to
//! O(Δ) (per ADR-025).
//!
//! # ADR-026 — E-Graph Optimization
//!
//! Use [`compile_full`] to additionally run
//! [`egraph::egraph_optimization`] on the [`DependencyGraph`]. The
//! optimizer applies four rewrite rules (`state_store_load_forward`,
//! `dead_store_elimination`, `read_merge`, `evaluation_reorder`) to a
//! custom e-graph data structure (no `egg` crate, per ADR-018) and
//! extracts an optimized [`DependencyGraph`] via cost-based extraction.
//! For the canonical Hello World scene (all passes have empty
//! `outputs`), the optimization is structurally a no-op — the
//! infrastructure is in place for scenes with intra-frame signal
//! outputs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod ast;
pub mod codegen;
pub mod egraph;
pub mod incremental;
pub mod ir;
pub mod lexer;
pub mod lints;
pub mod parser;
pub mod schedule;
pub mod seminative;
pub mod typechecker;
pub mod wasm_codegen;

// Re-export the primary public surface at the crate root for convenience.
pub use ast::{
    Attribute, BaseType, BinOp, Block, Color, Expr, FnDecl, InputFieldNode, ItemDecl, LetDecl, Lit,
    ModuleDecl, NodeDecl, Param, PositionDecl, Qualifier, RotationDecl, SceneDecl, Stmt, TextNode,
    Type,
};
pub use codegen::{
    compile, compile_full, compile_scheduled, compile_typecheck, compile_with_deps,
    compile_with_lints, lower, CodegenError, CompileError, DEFAULT_FONT_SIZE,
};
pub use egraph::{
    apply_dead_store_elimination, apply_read_merge, apply_state_store_load_forward,
    build_from_dep_graph, egraph_optimization, evaluation_reorder, extract, op_cost, EClass,
    EClassData, EClassId, EGraph, ENode, EOp, EOpKind,
};
pub use incremental::{incremental_analysis, DepNode, DepNodeId, DependencyGraph, SignalId};
pub use ir::{mint_module_id, AlgorithmIR, ColorIR, NodeIR, PositionIR, SceneIR};
pub use ir::{CollectionDeclIR, Monotonicity};
pub use lexer::{tokenize, LexError, Lexer, Token, TokenKind};
pub use lints::{run_lints, LintReport, LintSet, LintSeverity};
pub use parser::{parse, ParseError, Parser};
pub use schedule::{
    schedule_lowering, BatchingStrategy, PassKind, RenderPass, ScheduleIR, ScheduledScene, ShaderId,
};
pub use seminative::{
    collection_strategies, collection_strategy, has_seminive_collections, seminive_eligible_count,
    EvaluationStrategy,
};
pub use typechecker::{
    check_module, effective_qualifier, param_qualifier, qualifier_is_subtype, type_is_subtype,
    FnSig, FnSigTable, TypeEnv, TypeError, TypeErrorSet,
};
pub use wasm_codegen::{
    alk_full_type_to_wasm, alk_type_to_wasm, compile_src_to_wasm, compile_to_wasm,
    WasmCodegenError, WasmModule,
};

/// Re-export of [`alkalive_core::ModuleId`] so downstream consumers can
/// reference the type without adding `alkalive-core` as a direct dependency.
pub use alkalive_core::ModuleId;

#[cfg(test)]
mod integration_tests {
    use super::*;

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

    #[test]
    fn full_pipeline_hello_world() {
        let ir = compile(HELLO_WORLD).expect("hello world should compile");
        assert_eq!(ir.module_name, "HelloWorld");
        assert_eq!(ir.background, (0, 0, 0));
        assert_eq!(ir.nodes.len(), 2);
        assert!(ir.has_text());
        assert!(ir.has_input_field());
    }

    #[test]
    fn full_pipeline_json_roundtrip_shape() {
        let ir = compile(HELLO_WORLD).unwrap();
        let json = ir.to_json();
        // Smoke-test key fields appear in the JSON.
        assert!(json.contains("\"module_name\":\"HelloWorld\""), "{}", json);
        assert!(json.contains("\"background\":[0,0,0]"), "{}", json);
        assert!(json.contains("\"type\":\"text\""), "{}", json);
        assert!(json.contains("\"content\":\"Hello World!\""), "{}", json);
        assert!(json.contains("\"color\":\"#FFD700\""), "{}", json);
        assert!(json.contains("\"type\":\"input-field\""), "{}", json);
        assert!(
            json.contains("\"placeholder\":\"Type here...\""),
            "{}",
            json
        );
    }

    #[test]
    fn lex_parse_lower_stages_independent() {
        // Verify the three stages can be invoked independently.
        let tokens = tokenize(HELLO_WORLD).expect("lex ok");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Module));

        let ast = parse(HELLO_WORLD).expect("parse ok");
        assert_eq!(ast.name, "HelloWorld");

        let ir = lower(&ast).expect("lower ok");
        assert_eq!(ir.module_name, "HelloWorld");
    }

    #[test]
    fn compile_error_from_lex_error() {
        // Unterminated string → lex error surfaces as a parse error in the
        // top-level `compile` convenience function (it wraps lex errors).
        let err = compile("module M { scene { text \"unterminated { } } }").unwrap_err();
        let s = format!("{}", err);
        assert!(
            s.contains("lex error") || s.contains("unterminated") || s.contains("parse error"),
            "got: {}",
            s
        );
    }

    // ---- ADR-024: compile_scheduled() integration tests ----

    #[test]
    fn compile_scheduled_hello_world() {
        let scheduled =
            compile_scheduled(HELLO_WORLD).expect("hello world should compile (scheduled)");
        // Algorithm should match the legacy compile() output.
        assert_eq!(scheduled.algorithm.module_name, "HelloWorld");
        assert!(scheduled.algorithm.has_text());
        assert!(scheduled.algorithm.has_input_field());
        // Five passes: Clear, InputFieldBackground, InputFieldBorder, TitleText, InputText.
        assert_eq!(scheduled.schedule.passes.len(), 5);
        // pass_order is identity by default.
        assert_eq!(scheduled.schedule.pass_order, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn compile_scheduled_matches_compile_algorithm() {
        // The algorithm produced by compile_scheduled must equal what
        // compile() returns (the schedule lowering pass must not mutate
        // the algorithm IR).
        let just_algo = compile(HELLO_WORLD).unwrap();
        let scheduled = compile_scheduled(HELLO_WORLD).unwrap();
        assert_eq!(scheduled.algorithm, just_algo);
    }

    #[test]
    fn compile_scheduled_pass_kinds_in_expected_order() {
        let scheduled = compile_scheduled(HELLO_WORLD).unwrap();
        let kinds: Vec<_> = scheduled.schedule.passes.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![
                schedule::PassKind::Clear,
                schedule::PassKind::InputFieldBackground,
                schedule::PassKind::InputFieldBorder,
                schedule::PassKind::TitleText,
                schedule::PassKind::InputText,
            ]
        );
    }

    #[test]
    fn scene_ir_alias_compiles_with_new_name() {
        // The type alias `SceneIR = AlgorithmIR` must work: callers can
        // use either name interchangeably.
        let ir_as_alias: SceneIR = compile(HELLO_WORLD).unwrap();
        let ir_as_algo: AlgorithmIR = compile(HELLO_WORLD).unwrap();
        assert_eq!(ir_as_alias, ir_as_algo);
    }
}

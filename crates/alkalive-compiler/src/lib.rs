//! AlkALive compiler frontend.
//!
//! Lexes, parses, and lowers `.alk` source files (Hello-World subset)
//! into a runtime-consumable [`ir::SceneIR`].
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
//!                [codegen] ──► ir::SceneIR
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
//! The library modules (`lexer`, `ast`, `parser`, `ir`, `codegen`) use
//! only `alkalive-core` (an internal workspace crate) and `std`/`core`.
//! The optional `cli` feature pulls in `serde_json` for the binary; it
//! does NOT affect the library's public API.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod ast;
pub mod codegen;
pub mod ir;
pub mod lexer;
pub mod parser;

// Re-export the primary public surface at the crate root for convenience.
pub use ast::{
    Color, InputFieldNode, ModuleDecl, NodeDecl, PositionDecl, RotationDecl, SceneDecl, TextNode,
};
pub use codegen::{lower, compile, CodegenError, CompileError, DEFAULT_FONT_SIZE};
pub use ir::{mint_module_id, ColorIR, NodeIR, PositionIR, SceneIR};
pub use lexer::{tokenize, LexError, Lexer, Token, TokenKind};
pub use parser::{parse, ParseError, Parser};

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
        assert!(json.contains("\"placeholder\":\"Type here...\""), "{}", json);
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
}

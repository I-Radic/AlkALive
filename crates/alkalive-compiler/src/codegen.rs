//! Codegen — lowers the [`crate::ast::ModuleDecl`] to a validated
//! [`crate::ir::SceneIR`].
//!
//! This is the *semantic* pass: defaults are applied, named colors are
//! resolved, positions are validated, and missing required fields are
//! reported as [`CodegenError`].
//!
//! Defaults (per `PURE_ALKALIVE_PIPELINE_PLAN.md` Wave 2):
//!
//! | Field                | Default           |
//! |----------------------|-------------------|
//! | `background`         | `(0, 0, 0)` black |
//! | text `color`         | `Gold`            |
//! | text `font_size`     | `32.0`            |
//! | text `rotation_speed`| `0.0`             |
//! | text `position`      | `Center`          |
//! | input-field `placeholder` | `""`         |
//! | input-field `position`    | `Center`     |

#![forbid(unsafe_code)]

use core::fmt;

use crate::ast::{
    Color, ModuleDecl, NodeDecl, PositionDecl, RotationDecl, SceneDecl, TextNode,
    InputFieldNode,
};
use crate::ir::{mint_module_id, ColorIR, NodeIR, PositionIR, SceneIR};

/// A semantic (codegen) error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    /// Human-readable message.
    pub message: String,
    /// 1-based line of the offending construct (best-effort).
    pub line: u32,
    /// 1-based column of the offending construct (best-effort).
    pub col: u32,
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "codegen error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl core::error::Error for CodegenError {}

/// Default text font size when none is declared.
pub const DEFAULT_FONT_SIZE: f32 = 32.0;

/// Lower a parsed [`ModuleDecl`] to a validated [`SceneIR`].
pub fn lower(module: &ModuleDecl) -> Result<SceneIR, CodegenError> {
    let module_id = mint_module_id(&module.name);
    let mut ir = SceneIR::new(module_id, &module.name);

    let scene = module.scene.as_ref().ok_or(CodegenError {
        message: format!(
            "module `{}` declares no `scene` block; a scene is required",
            module.name
        ),
        line: module.line,
        col: module.col,
    })?;

    if let Some(bg) = &scene.background {
        ir.background = lower_color_to_rgb(bg, scene.line, scene.col)?;
    }

    for node in &scene.nodes {
        let node_ir = lower_node(node, scene)?;
        ir.nodes.push(node_ir);
    }

    Ok(ir)
}

fn lower_node(node: &NodeDecl, scene: &SceneDecl) -> Result<NodeIR, CodegenError> {
    match node {
        NodeDecl::Text(t) => lower_text_node(t),
        NodeDecl::InputField(f) => lower_input_field_node(f, scene),
    }
}

fn lower_text_node(t: &TextNode) -> Result<NodeIR, CodegenError> {
    let color = match &t.color {
        Some(c) => lower_color(c, t.line, t.col)?,
        None => ColorIR::Gold,
    };
    let font_size = t.font_size.unwrap_or(DEFAULT_FONT_SIZE);
    let rotation_speed = t
        .rotation
        .as_ref()
        .map(|r: &RotationDecl| r.speed)
        .unwrap_or(0.0);
    let position = match &t.position {
        Some(p) => lower_position(p, t.line, t.col)?,
        None => PositionIR::Center,
    };

    // Semantic validation: font-size must be positive and finite.
    if !font_size.is_finite() || font_size <= 0.0 {
        return Err(CodegenError {
            message: format!(
                "font-size must be a positive finite number, got {}",
                font_size
            ),
            line: t.line,
            col: t.col,
        });
    }
    // Rotation speed must be finite (zero is fine, negative is allowed —
    // it just spins the other way).
    if !rotation_speed.is_finite() {
        return Err(CodegenError {
            message: format!("rotation speed must be finite, got {}", rotation_speed),
            line: t.line,
            col: t.col,
        });
    }

    Ok(NodeIR::Text {
        content: t.content.clone(),
        color,
        font_size,
        rotation_speed,
        position,
    })
}

fn lower_input_field_node(
    f: &InputFieldNode,
    scene: &SceneDecl,
) -> Result<NodeIR, CodegenError> {
    let placeholder = f.placeholder.clone().unwrap_or_default();
    let position = match &f.position {
        Some(p) => lower_position(p, f.line, f.col)?,
        None => PositionIR::Center,
    };

    // Semantic validation: `below text` requires a text node to precede
    // this input-field in source order.
    if let PositionIR::BelowText = position {
        let has_preceding_text = scene.nodes.iter().any(|n| matches!(n, NodeDecl::Text(_)));
        if !has_preceding_text {
            return Err(CodegenError {
                message: "`position: below text` requires a `text` node to be declared in the same scene".into(),
                line: f.line,
                col: f.col,
            });
        }
    }

    Ok(NodeIR::InputField {
        placeholder,
        position,
    })
}

/// Lower a [`Color`] to a [`ColorIR`].
fn lower_color(c: &Color, line: u32, col: u32) -> Result<ColorIR, CodegenError> {
    match c {
        Color::Hex(r, g, b) => Ok(ColorIR::Solid(*r, *g, *b)),
        Color::Named(name) => match name.as_str() {
            "gold" => Ok(ColorIR::Gold),
            other => Err(CodegenError {
                message: format!(
                    "unknown named color `{}`; only `gold` is supported in this subset (use #RRGGBB for other colors)",
                    other
                ),
                line,
                col,
            }),
        },
    }
}

/// Lower a [`Color`] to an `(r, g, b)` triple (resolving named colors).
fn lower_color_to_rgb(c: &Color, line: u32, col: u32) -> Result<(u8, u8, u8), CodegenError> {
    lower_color(c, line, col).map(|c| c.rgb())
}

/// Lower a [`PositionDecl`] to a [`PositionIR`].
fn lower_position(p: &PositionDecl, line: u32, col: u32) -> Result<PositionIR, CodegenError> {
    match p {
        PositionDecl::Center => Ok(PositionIR::Center),
        PositionDecl::Below(name) => {
            if name == "text" {
                Ok(PositionIR::BelowText)
            } else {
                Err(CodegenError {
                    message: format!(
                        "`below {}` is not supported; only `below text` is valid in this subset",
                        name
                    ),
                    line,
                    col,
                })
            }
        }
        PositionDecl::Custom(x, y) => {
            if !x.is_finite() || !y.is_finite() {
                return Err(CodegenError {
                    message: format!(
                        "custom position coordinates must be finite, got ({}, {})",
                        x, y
                    ),
                    line,
                    col,
                });
            }
            Ok(PositionIR::Custom(*x, *y))
        }
    }
}

/// Convenience: tokenize + parse + lower in one call.
pub fn compile(src: &str) -> Result<SceneIR, CompileError> {
    let module = crate::parser::parse(src).map_err(CompileError::Parse)?;
    lower(&module).map_err(CompileError::Codegen)
}

/// Convenience: tokenize + parse + lint + lower in one call.
///
/// This is the ADR-027 Phase 1 entry point. It runs the lint pass
/// *after* parsing but *before* codegen, returning both the lowered
/// [`SceneIR`] and the [`LintSet`] of findings. The legacy [`compile`]
/// function remains lint-free for backward compatibility.
///
/// Lint findings are surfaced to the caller — this function does NOT
/// abort compilation when `deny_monotonicity` is set. Callers that want
/// hard errors should inspect [`LintSet::has_errors`] and act accordingly.
///
/// # Errors
///
/// Returns [`CompileError`] only if lexing, parsing, or codegen fails.
/// Lint findings never produce a `CompileError`.
pub fn compile_with_lints(src: &str) -> Result<(SceneIR, crate::lints::LintSet), CompileError> {
    let module = crate::parser::parse(src).map_err(CompileError::Parse)?;
    let lint_set = crate::lints::run_lints(&module);
    let ir = lower(&module).map_err(CompileError::Codegen)?;
    Ok((ir, lint_set))
}

/// Top-level error for the full `compile` pipeline (lex+parse+lower).
#[derive(Debug, Clone)]
pub enum CompileError {
    /// Lexing or parsing failed.
    Parse(crate::parser::ParseError),
    /// Semantic validation (codegen) failed.
    Codegen(CodegenError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Parse(e) => write!(f, "{}", e),
            CompileError::Codegen(e) => write!(f, "{}", e),
        }
    }
}

impl core::error::Error for CompileError {}

impl From<crate::parser::ParseError> for CompileError {
    fn from(e: crate::parser::ParseError) -> Self {
        CompileError::Parse(e)
    }
}

impl From<CodegenError> for CompileError {
    fn from(e: CodegenError) -> Self {
        CompileError::Codegen(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn lower_ok(src: &str) -> SceneIR {
        let m = parse(src).unwrap_or_else(|e| panic!("parse failed: {}", e));
        lower(&m).unwrap_or_else(|e| panic!("lower failed: {}", e))
    }

    fn lower_err(src: &str) -> CodegenError {
        let m = parse(src).unwrap_or_else(|e| panic!("parse failed: {}", e));
        match lower(&m) {
            Ok(_) => panic!("expected codegen error, got success"),
            Err(e) => e,
        }
    }

    #[test]
    fn lower_module_without_scene_errors() {
        let m = parse("module M { }").unwrap();
        let err = lower(&m).unwrap_err();
        assert!(err.message.contains("no `scene`"), "got: {}", err.message);
    }

    #[test]
    fn lower_empty_scene_defaults_black_background() {
        let ir = lower_ok("module M { scene { } }");
        assert_eq!(ir.background, (0, 0, 0));
        assert!(ir.nodes.is_empty());
    }

    #[test]
    fn lower_background_hex() {
        let ir = lower_ok("module M { scene { background: #112233 } }");
        assert_eq!(ir.background, (0x11, 0x22, 0x33));
    }

    #[test]
    fn lower_background_named_gold_resolves_to_rgb() {
        let ir = lower_ok("module M { scene { background: gold } }");
        assert_eq!(ir.background, (0xFF, 0xD7, 0x00));
    }

    #[test]
    fn lower_text_node_defaults() {
        let ir = lower_ok(r#"module M { scene { text "Hi" { } } }"#);
        match &ir.nodes[0] {
            NodeIR::Text {
                content,
                color,
                font_size,
                rotation_speed,
                position,
            } => {
                assert_eq!(content, "Hi");
                assert_eq!(*color, ColorIR::Gold);
                assert_eq!(*font_size, DEFAULT_FONT_SIZE);
                assert_eq!(*rotation_speed, 0.0);
                assert_eq!(*position, PositionIR::Center);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn lower_text_node_explicit_values() {
        let ir = lower_ok(
            r#"module M { scene { text "Hi" { color: #FF0000 font-size: 48 rotation: y-axis 1.5 position: center } } }"#,
        );
        match &ir.nodes[0] {
            NodeIR::Text {
                color,
                font_size,
                rotation_speed,
                ..
            } => {
                assert_eq!(*color, ColorIR::Solid(0xFF, 0x00, 0x00));
                assert_eq!(*font_size, 48.0);
                assert_eq!(*rotation_speed, 1.5);
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn lower_text_node_color_named_gold() {
        let ir = lower_ok(r#"module M { scene { text "Hi" { color: gold } } }"#);
        match &ir.nodes[0] {
            NodeIR::Text { color, .. } => assert_eq!(*color, ColorIR::Gold),
            _ => panic!(),
        }
    }

    #[test]
    fn lower_text_node_unknown_color_errors() {
        let err = lower_err(r#"module M { scene { text "Hi" { color: purple } } }"#);
        assert!(err.message.contains("unknown named color"), "got: {}", err.message);
    }

    #[test]
    fn lower_text_node_zero_font_size_errors() {
        let err = lower_err(r#"module M { scene { text "Hi" { font-size: 0 } } }"#);
        assert!(err.message.contains("positive"), "got: {}", err.message);
    }

    #[test]
    fn lower_text_node_negative_font_size_errors() {
        let err = lower_err(r#"module M { scene { text "Hi" { font-size: -10 } } }"#);
        assert!(err.message.contains("positive"), "got: {}", err.message);
    }

    #[test]
    fn lower_input_field_defaults() {
        let ir = lower_ok(
            r#"module M { scene { text "Hi" { } input-field { } } }"#,
        );
        match &ir.nodes[1] {
            NodeIR::InputField { placeholder, position } => {
                assert_eq!(placeholder, "");
                assert_eq!(*position, PositionIR::Center);
            }
            other => panic!("expected InputField, got {:?}", other),
        }
    }

    #[test]
    fn lower_input_field_explicit() {
        let ir = lower_ok(
            r#"module M { scene { text "Hi" { } input-field { placeholder: "Type" position: below text } } }"#,
        );
        match &ir.nodes[1] {
            NodeIR::InputField { placeholder, position } => {
                assert_eq!(placeholder, "Type");
                assert_eq!(*position, PositionIR::BelowText);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn lower_input_field_below_text_without_text_node_errors() {
        let err = lower_err(r#"module M { scene { input-field { position: below text } } }"#);
        assert!(
            err.message.contains("requires a `text` node"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn lower_input_field_below_other_ref_errors() {
        let err =
            lower_err(r#"module M { scene { input-field { position: below button } } }"#);
        assert!(err.message.contains("below button"), "got: {}", err.message);
    }

    #[test]
    fn lower_position_custom_coords() {
        let ir = lower_ok(r#"module M { scene { text "Hi" { position: 0.5 0.25 } } }"#);
        match &ir.nodes[0] {
            NodeIR::Text { position, .. } => {
                assert_eq!(*position, PositionIR::Custom(0.5, 0.25));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn lower_hello_world_full_source() {
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
    input-field {
      placeholder: "Type here..."
      position: below text
    }
  }
}
"#;
        let ir = lower_ok(src);
        assert_eq!(ir.module_name, "HelloWorld");
        assert_eq!(ir.background, (0, 0, 0));
        assert_eq!(ir.nodes.len(), 2);

        match &ir.nodes[0] {
            NodeIR::Text {
                content,
                color,
                font_size,
                rotation_speed,
                position,
            } => {
                assert_eq!(content, "Hello World!");
                assert_eq!(*color, ColorIR::Gold);
                assert_eq!(*font_size, 64.0);
                assert_eq!(*rotation_speed, 0.5);
                assert_eq!(*position, PositionIR::Center);
            }
            other => panic!("expected Text, got {:?}", other),
        }
        match &ir.nodes[1] {
            NodeIR::InputField { placeholder, position } => {
                assert_eq!(placeholder, "Type here...");
                assert_eq!(*position, PositionIR::BelowText);
            }
            other => panic!("expected InputField, got {:?}", other),
        }
    }

    #[test]
    fn lower_module_id_deterministic() {
        let ir1 = lower_ok("module Same { scene { } }");
        let ir2 = lower_ok("module Same { scene { } }");
        assert_eq!(ir1.module_id, ir2.module_id);

        let ir3 = lower_ok("module Different { scene { } }");
        assert_ne!(ir1.module_id, ir3.module_id);
    }

    #[test]
    fn compile_full_pipeline_ok() {
        let ir = compile(r#"module M { scene { text "Hi" { } } }"#).unwrap();
        assert_eq!(ir.module_name, "M");
        assert!(ir.has_text());
    }

    #[test]
    fn compile_full_pipeline_parse_error() {
        let err = compile("module { }").unwrap_err();
        assert!(matches!(err, CompileError::Parse(_)));
    }

    #[test]
    fn compile_full_pipeline_codegen_error() {
        let err = compile("module M { }").unwrap_err();
        assert!(matches!(err, CompileError::Codegen(_)));
    }

    #[test]
    fn compile_error_display() {
        let err = compile("module M { }").unwrap_err();
        let s = format!("{}", err);
        assert!(s.contains("codegen error") || s.contains("parse error"), "got: {}", s);
    }

    #[test]
    fn codegen_error_display() {
        let err = lower_err(r#"module M { scene { text "Hi" { color: purple } } }"#);
        let s = format!("{}", err);
        assert!(s.contains("codegen error at"), "got: {}", s);
    }
}

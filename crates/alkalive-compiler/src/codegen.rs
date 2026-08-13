//! Codegen — lowers the [`crate::ast::ModuleDecl`] to a validated
//! [`crate::ir::AlgorithmIR`] (the *algorithm* IR, per ADR-024).
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
    Color, InputFieldNode, ModuleDecl, NodeDecl, PositionDecl, RotationDecl, SceneDecl, TextNode,
};
use crate::egraph::egraph_optimization;
use crate::incremental::{incremental_analysis, DependencyGraph};
use crate::ir::{mint_module_id, AlgorithmIR, ColorIR, NodeIR, PositionIR};
use crate::schedule::{schedule_lowering, ScheduledScene};

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

/// Lower a parsed [`ModuleDecl`] to a validated [`AlgorithmIR`].
///
/// Per ADR-024, this returns the *algorithm* IR — the pure scene
/// description with no rendering-strategy fields. (Legacy callers may
/// know this type as `SceneIR`; the alias is re-exported at the crate
/// root.) Use [`compile_scheduled`] to additionally produce the default
/// rendering schedule.
pub fn lower(module: &ModuleDecl) -> Result<AlgorithmIR, CodegenError> {
    let module_id = mint_module_id(&module.name);
    let mut ir = AlgorithmIR::new(module_id, &module.name);

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

    // ADR-027 Phase 2: lower collection declarations with monotonicity metadata.
    for item in &module.items {
        if let crate::ast::ItemDecl::Let(l) = item {
            ir.collections.push(lower_collection_decl(l));
        }
    }

    Ok(ir)
}

/// Lower an [`crate::ast::LetDecl`] to a [`crate::ir::CollectionDeclIR`],
/// resolving the effective monotonicity (attribute form takes precedence
/// over type-qualifier form).
fn lower_collection_decl(l: &crate::ast::LetDecl) -> crate::ir::CollectionDeclIR {
    use crate::ast::BaseType;
    use crate::ir::{CollectionDeclIR, Monotonicity};
    // For Vec<T>, the element type is the inner Type's display string.
    // For non-Vec types, use the whole Type's display string.
    let element_type = match &l.ty.base {
        BaseType::Vec(elem) => format!("{}", elem),
        _ => format!("{}", l.ty),
    };
    let monotonicity = Monotonicity::from_qualifier(crate::typechecker::effective_qualifier(l));
    CollectionDeclIR {
        name: l.name.clone(),
        element_type,
        monotonicity,
    }
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

fn lower_input_field_node(f: &InputFieldNode, scene: &SceneDecl) -> Result<NodeIR, CodegenError> {
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
                message:
                    "`position: below text` requires a `text` node to be declared in the same scene"
                        .into(),
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
///
/// Returns the *algorithm* IR ([`AlgorithmIR`], aliased as `SceneIR`).
/// Per ADR-024, this is the pure scene description with no rendering
/// strategy — use [`compile_scheduled`] if you also need the schedule.
pub fn compile(src: &str) -> Result<AlgorithmIR, CompileError> {
    let module = crate::parser::parse(src).map_err(CompileError::Parse)?;
    lower(&module).map_err(CompileError::Codegen)
}

/// Convenience: tokenize + parse + lint + lower in one call.
///
/// This is the ADR-027 Phase 1 entry point. It runs the lint pass
/// *after* parsing but *before* codegen, returning both the lowered
/// [`AlgorithmIR`] and the [`LintSet`] of findings. The legacy [`compile`]
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
pub fn compile_with_lints(src: &str) -> Result<(AlgorithmIR, crate::lints::LintSet), CompileError> {
    let module = crate::parser::parse(src).map_err(CompileError::Parse)?;
    let lint_set = crate::lints::run_lints(&module);
    let ir = lower(&module).map_err(CompileError::Codegen)?;
    Ok((ir, lint_set))
}

/// Convenience: tokenize + parse + typecheck + lower in one call.
///
/// This is the ADR-027 Phase 2 entry point. It runs the type checker
/// *after* parsing but *before* codegen. If the type checker finds any
/// errors (e.g. a shrink op on a `monotone` collection), compilation aborts
/// with [`CompileError::Type`].
///
/// # Errors
///
/// Returns [`CompileError::Parse`] if lexing/parsing fails,
/// [`CompileError::Type`] if the type checker finds errors, or
/// [`CompileError::Codegen`] if semantic lowering fails.
pub fn compile_typecheck(src: &str) -> Result<AlgorithmIR, CompileError> {
    let module = crate::parser::parse(src).map_err(CompileError::Parse)?;
    let type_errors = crate::typechecker::check_module(&module);
    if !type_errors.is_empty() {
        return Err(CompileError::Type(type_errors));
    }
    lower(&module).map_err(CompileError::Codegen)
}

/// Convenience: tokenize + parse + lower + schedule-lower in one call.
///
/// This is the ADR-024 entry point. It runs the full pipeline:
/// `.alk source → AlgorithmIR → ScheduledScene`. The returned
/// [`ScheduledScene`] contains both the algorithm (what to render) and
/// the default [`ScheduleIR`](crate::schedule::ScheduleIR) (how to
/// render it).
///
/// # Errors
///
/// Returns [`CompileError`] only if lexing, parsing, or codegen fails.
/// The [`schedule_lowering`] pass is infallible (it never fails for a
/// well-formed [`AlgorithmIR`]).
///
/// # Example
///
/// ```
/// use alkalive_compiler::compile_scheduled;
///
/// let src = r#"
/// module HelloWorld {
///   scene {
///     background: #000000
///     text "Hello World!" {
///       color: gold
///       font-size: 64
///       rotation: y-axis 0.5
///       position: center
///     }
///     input-field {
///       placeholder: "Type here..."
///       position: below text
///     }
///   }
/// }
/// "#;
/// let scheduled = compile_scheduled(src).expect("hello world should compile");
/// assert_eq!(scheduled.algorithm.module_name, "HelloWorld");
/// // Five passes: Clear, InputFieldBackground, InputFieldBorder, TitleText, InputText.
/// assert_eq!(scheduled.schedule.passes.len(), 5);
/// ```
pub fn compile_scheduled(src: &str) -> Result<ScheduledScene, CompileError> {
    let algorithm = compile(src)?;
    let schedule = schedule_lowering(&algorithm);
    Ok(ScheduledScene {
        algorithm,
        schedule,
    })
}

/// Convenience: tokenize + parse + lower + schedule-lower + incremental
/// analysis in one call.
///
/// This is the ADR-025 entry point. It runs the full pipeline:
/// `.alk source → AlgorithmIR → ScheduledScene → DependencyGraph`. The
/// returned tuple contains both the [`ScheduledScene`] (algorithm + default
/// schedule, per ADR-024) and the [`DependencyGraph`] (per ADR-025) that
/// the runtime uses to propagate dirtiness from changed signals to the
/// passes that depend on them.
///
/// The dependency graph is computed *from* the scheduled scene via
/// [`incremental_analysis`], so it is consistent with the schedule's pass
/// list. The graph is infallible once `compile_scheduled` has succeeded
/// (the analysis is a pure data-shuffle with no failure modes).
///
/// # Errors
///
/// Returns [`CompileError`] only if lexing, parsing, or codegen fails.
/// Both [`schedule_lowering`] and [`incremental_analysis`] are infallible.
///
/// # Example
///
/// ```
/// use alkalive_compiler::compile_with_deps;
///
/// let src = r#"
/// module HelloWorld {
///   scene {
///     background: #000000
///     text "Hello World!" {
///       color: gold
///       font-size: 64
///       rotation: y-axis 0.5
///       position: center
///     }
///     input-field {
///       placeholder: "Type here..."
///       position: below text
///     }
///   }
/// }
/// "#;
/// let (scheduled, dep_graph) = compile_with_deps(src).expect("hello world should compile");
/// assert_eq!(scheduled.algorithm.module_name, "HelloWorld");
/// // 5 passes -> 5 dependency-graph nodes.
/// assert_eq!(dep_graph.nodes.len(), scheduled.schedule.passes.len());
/// ```
pub fn compile_with_deps(src: &str) -> Result<(ScheduledScene, DependencyGraph), CompileError> {
    let scheduled = compile_scheduled(src)?;
    let dep_graph = incremental_analysis(&scheduled);
    Ok((scheduled, dep_graph))
}

/// Convenience: tokenize + parse + lower + schedule-lower + incremental
/// analysis + e-graph optimization in one call.
///
/// This is the ADR-026 entry point. It runs the full pipeline:
/// `.alk source → AlgorithmIR → ScheduledScene → DependencyGraph
/// → optimized DependencyGraph`. The returned tuple contains both the
/// [`ScheduledScene`] (algorithm + default schedule, per ADR-024) and
/// the e-graph-optimized [`DependencyGraph`] (per ADR-026).
///
/// The dependency graph is computed *from* the scheduled scene via
/// [`incremental_analysis`], then optimized via
/// [`egraph_optimization`](crate::egraph::egraph_optimization). The
/// optimizer applies four rewrite rules (`state_store_load_forward`,
/// `dead_store_elimination`, `read_merge`, `evaluation_reorder`) to a
/// custom e-graph data structure (no `egg` crate, per ADR-018).
///
/// For the canonical Hello World scene (5 passes, 6 signals, all empty
/// `outputs`), the optimization is structurally a no-op: hash-consing
/// during the build phase already merges all `SignalRead(s)` e-nodes
/// for the same `s` into a single e-class (rule 3, `read_merge`), and
/// there are no `SignalWrite` e-nodes for rules 1 and 2 to act on.
/// The extracted dep graph is structurally identical to the input
/// from [`compile_with_deps`].
///
/// # Errors
///
/// Returns [`CompileError`] only if lexing, parsing, or codegen fails.
/// Both [`schedule_lowering`], [`incremental_analysis`], and
/// [`egraph_optimization`](crate::egraph::egraph_optimization) are
/// infallible.
///
/// # Example
///
/// ```
/// use alkalive_compiler::compile_full;
///
/// let src = r#"
/// module HelloWorld {
///   scene {
///     background: #000000
///     text "Hello World!" {
///       color: gold
///       font-size: 64
///       rotation: y-axis 0.5
///       position: center
///     }
///     input-field {
///       placeholder: "Type here..."
///       position: below text
///     }
///   }
/// }
/// "#;
/// let (scheduled, dep_graph) = compile_full(src).expect("hello world should compile");
/// assert_eq!(scheduled.algorithm.module_name, "HelloWorld");
/// // 5 passes -> 5 dependency-graph nodes (Hello World has no signal
/// // outputs, so the e-graph optimizer preserves all passes).
/// assert_eq!(dep_graph.nodes.len(), scheduled.schedule.passes.len());
/// ```
pub fn compile_full(src: &str) -> Result<(ScheduledScene, DependencyGraph), CompileError> {
    let scheduled = compile_scheduled(src)?;
    let dep_graph = incremental_analysis(&scheduled);
    let optimized = egraph_optimization(&dep_graph);
    Ok((scheduled, optimized))
}

/// Top-level error for the full `compile` pipeline (lex+parse+typecheck+lower).
#[derive(Debug, Clone)]
pub enum CompileError {
    /// Lexing or parsing failed.
    Parse(crate::parser::ParseError),
    /// Type checking (ADR-027 Phase 2) failed.
    Type(crate::typechecker::TypeErrorSet),
    /// Semantic validation (codegen) failed.
    Codegen(CodegenError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Parse(e) => write!(f, "{}", e),
            CompileError::Type(set) => write!(f, "{}", set),
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

    fn lower_ok(src: &str) -> AlgorithmIR {
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
        assert!(
            err.message.contains("unknown named color"),
            "got: {}",
            err.message
        );
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
        let ir = lower_ok(r#"module M { scene { text "Hi" { } input-field { } } }"#);
        match &ir.nodes[1] {
            NodeIR::InputField {
                placeholder,
                position,
            } => {
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
            NodeIR::InputField {
                placeholder,
                position,
            } => {
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
        let err = lower_err(r#"module M { scene { input-field { position: below button } } }"#);
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
            NodeIR::InputField {
                placeholder,
                position,
            } => {
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
        assert!(
            s.contains("codegen error") || s.contains("parse error"),
            "got: {}",
            s
        );
    }

    #[test]
    fn codegen_error_display() {
        let err = lower_err(r#"module M { scene { text "Hi" { color: purple } } }"#);
        let s = format!("{}", err);
        assert!(s.contains("codegen error at"), "got: {}", s);
    }

    // ---- ADR-024: compile_scheduled() tests ----

    #[test]
    fn compile_scheduled_full_pipeline_ok() {
        let scheduled =
            compile_scheduled(r#"module M { scene { text "Hi" { } input-field { } } }"#)
                .expect("compile_scheduled should succeed");
        assert_eq!(scheduled.algorithm.module_name, "M");
        assert!(scheduled.algorithm.has_text());
        assert!(scheduled.algorithm.has_input_field());
        // Clear + InputFieldBackground + InputFieldBorder + TitleText + InputText = 5 passes.
        assert_eq!(scheduled.schedule.passes.len(), 5);
        assert_eq!(scheduled.schedule.pass_order.len(), 5);
    }

    #[test]
    fn compile_scheduled_text_only_has_two_passes() {
        let scheduled = compile_scheduled(r#"module M { scene { text "Hi" { } } }"#).unwrap();
        // Clear + TitleText = 2 passes.
        assert_eq!(scheduled.schedule.passes.len(), 2);
        assert_eq!(
            scheduled.schedule.passes[0].kind,
            crate::schedule::PassKind::Clear
        );
        assert_eq!(
            scheduled.schedule.passes[1].kind,
            crate::schedule::PassKind::TitleText
        );
    }

    #[test]
    fn compile_scheduled_empty_scene_has_only_clear_pass() {
        let scheduled = compile_scheduled("module M { scene { } }").unwrap();
        assert_eq!(scheduled.schedule.passes.len(), 1);
        assert_eq!(
            scheduled.schedule.passes[0].kind,
            crate::schedule::PassKind::Clear
        );
    }

    #[test]
    fn compile_scheduled_propagates_parse_errors() {
        let err = compile_scheduled("module { }").unwrap_err();
        assert!(matches!(err, CompileError::Parse(_)));
    }

    #[test]
    fn compile_scheduled_propagates_codegen_errors() {
        let err = compile_scheduled("module M { }").unwrap_err();
        assert!(matches!(err, CompileError::Codegen(_)));
    }

    #[test]
    fn compile_scheduled_algorithm_matches_compile_output() {
        // The algorithm field of compile_scheduled should be byte-for-byte
        // identical to what compile() returns.
        let src = r#"module Same { scene { text "Hi" { } input-field { } } }"#;
        let just_algo = compile(src).unwrap();
        let scheduled = compile_scheduled(src).unwrap();
        assert_eq!(scheduled.algorithm, just_algo);
    }

    // ---- ADR-025: compile_with_deps() tests ----

    #[test]
    fn compile_with_deps_full_pipeline_ok() {
        let (scheduled, dep_graph) =
            compile_with_deps(r#"module M { scene { text "Hi" { } input-field { } } }"#)
                .expect("compile_with_deps should succeed");
        // The scheduled scene matches what compile_scheduled returns.
        assert_eq!(scheduled.algorithm.module_name, "M");
        assert!(scheduled.algorithm.has_text());
        assert!(scheduled.algorithm.has_input_field());
        // 5 passes -> 5 dep-graph nodes.
        assert_eq!(dep_graph.nodes.len(), scheduled.schedule.passes.len());
        assert_eq!(dep_graph.nodes.len(), 5);
    }

    #[test]
    fn compile_with_deps_text_only_has_two_nodes() {
        let (scheduled, dep_graph) =
            compile_with_deps(r#"module M { scene { text "Hi" { } } }"#).unwrap();
        // Clear + TitleText = 2 passes -> 2 nodes.
        assert_eq!(scheduled.schedule.passes.len(), 2);
        assert_eq!(dep_graph.nodes.len(), 2);
    }

    #[test]
    fn compile_with_deps_empty_scene_has_one_node() {
        let (scheduled, dep_graph) = compile_with_deps("module M { scene { } }").unwrap();
        assert_eq!(scheduled.schedule.passes.len(), 1);
        assert_eq!(dep_graph.nodes.len(), 1);
        assert_eq!(dep_graph.nodes[0].description, "Clear");
    }

    #[test]
    fn compile_with_deps_propagates_parse_errors() {
        let err = compile_with_deps("module { }").unwrap_err();
        assert!(matches!(err, CompileError::Parse(_)));
    }

    #[test]
    fn compile_with_deps_propagates_codegen_errors() {
        let err = compile_with_deps("module M { }").unwrap_err();
        assert!(matches!(err, CompileError::Codegen(_)));
    }

    #[test]
    fn compile_with_deps_graph_matches_incremental_analysis() {
        // The dep graph produced by compile_with_deps must equal what
        // incremental_analysis produces from the scheduled scene.
        let src = r#"module Same { scene { text "Hi" { } input-field { } } }"#;
        let (scheduled, dep_graph) = compile_with_deps(src).unwrap();
        let manual_graph = crate::incremental::incremental_analysis(&scheduled);
        assert_eq!(dep_graph.nodes.len(), manual_graph.nodes.len());
        for (a, b) in dep_graph.nodes.iter().zip(manual_graph.nodes.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.inputs, b.inputs);
            assert_eq!(a.outputs, b.outputs);
            assert_eq!(a.pass_index, b.pass_index);
            assert_eq!(a.description, b.description);
        }
    }

    #[test]
    fn compile_with_deps_passes_match_compile_scheduled() {
        // The scheduled scene produced by compile_with_deps must equal what
        // compile_scheduled returns (the incremental analysis must not
        // mutate the scheduled scene).
        let src = r#"module Same { scene { text "Hi" { } input-field { } } }"#;
        let (scheduled_with_deps, _graph) = compile_with_deps(src).unwrap();
        let scheduled = compile_scheduled(src).unwrap();
        assert_eq!(scheduled_with_deps.algorithm, scheduled.algorithm);
        assert_eq!(
            scheduled_with_deps.schedule.passes.len(),
            scheduled.schedule.passes.len()
        );
    }

    // ---- ADR-026: compile_full() tests ----

    #[test]
    fn compile_full_full_pipeline_ok() {
        let (scheduled, dep_graph) =
            compile_full(r#"module M { scene { text "Hi" { } input-field { } } }"#)
                .expect("compile_full should succeed");
        // The scheduled scene matches what compile_scheduled returns.
        assert_eq!(scheduled.algorithm.module_name, "M");
        assert!(scheduled.algorithm.has_text());
        assert!(scheduled.algorithm.has_input_field());
        // 5 passes -> 5 dep-graph nodes (Hello World has no signal
        // outputs, so the e-graph optimizer preserves all passes).
        assert_eq!(dep_graph.nodes.len(), scheduled.schedule.passes.len());
        assert_eq!(dep_graph.nodes.len(), 5);
    }

    #[test]
    fn compile_full_text_only_has_two_nodes() {
        let (scheduled, dep_graph) =
            compile_full(r#"module M { scene { text "Hi" { } } }"#).unwrap();
        // Clear + TitleText = 2 passes -> 2 nodes.
        assert_eq!(scheduled.schedule.passes.len(), 2);
        assert_eq!(dep_graph.nodes.len(), 2);
    }

    #[test]
    fn compile_full_empty_scene_has_one_node() {
        let (scheduled, dep_graph) = compile_full("module M { scene { } }").unwrap();
        assert_eq!(scheduled.schedule.passes.len(), 1);
        assert_eq!(dep_graph.nodes.len(), 1);
        assert_eq!(dep_graph.nodes[0].description, "Clear");
    }

    #[test]
    fn compile_full_propagates_parse_errors() {
        let err = compile_full("module { }").unwrap_err();
        assert!(matches!(err, CompileError::Parse(_)));
    }

    #[test]
    fn compile_full_propagates_codegen_errors() {
        let err = compile_full("module M { }").unwrap_err();
        assert!(matches!(err, CompileError::Codegen(_)));
    }

    #[test]
    fn compile_full_preserves_pass_count_for_hello_world() {
        // Hello World has no signal outputs, so the e-graph optimizer
        // is a structural no-op: all 5 passes are preserved.
        let (scheduled, dep_graph) = compile_full(
            r#"
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
"#,
        )
        .expect("hello world should compile");
        assert_eq!(scheduled.schedule.passes.len(), 5);
        assert_eq!(dep_graph.nodes.len(), 5);
    }

    #[test]
    fn compile_full_dep_graph_inputs_preserved_for_hello_world() {
        // The e-graph optimizer must not change which signals each pass
        // reads (for Hello World, where there are no writes to forward
        // or eliminate).
        let (scheduled, dep_graph_optimized) = compile_full(
            r#"
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
"#,
        )
        .unwrap();
        let dep_graph_unoptimized = incremental_analysis(&scheduled);
        // Same number of nodes.
        assert_eq!(
            dep_graph_optimized.nodes.len(),
            dep_graph_unoptimized.nodes.len()
        );
        // For each pass, the set of input signals should be the same
        // (order may differ due to topological sort, so we compare as
        // sets).
        for unopt in &dep_graph_unoptimized.nodes {
            let opt = dep_graph_optimized
                .node_for_pass(unopt.pass_index)
                .expect("optimized graph should have a node for each pass");
            let unopt_inputs: std::collections::HashSet<_> = unopt.inputs.iter().collect();
            let opt_inputs: std::collections::HashSet<_> = opt.inputs.iter().collect();
            assert_eq!(
                unopt_inputs, opt_inputs,
                "pass {:?} inputs changed: {:?} -> {:?}",
                unopt.description, unopt.inputs, opt.inputs
            );
        }
    }

    #[test]
    fn compile_full_passes_match_compile_scheduled() {
        // The scheduled scene produced by compile_full must equal what
        // compile_scheduled returns (the e-graph optimizer must not
        // mutate the scheduled scene — it only touches the dep graph).
        let src = r#"module Same { scene { text "Hi" { } input-field { } } }"#;
        let (scheduled_full, _graph) = compile_full(src).unwrap();
        let scheduled = compile_scheduled(src).unwrap();
        assert_eq!(scheduled_full.algorithm, scheduled.algorithm);
        assert_eq!(
            scheduled_full.schedule.passes.len(),
            scheduled.schedule.passes.len()
        );
    }
}
